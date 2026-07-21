//! Structural (non-dataflow) detectors: bug shapes that are visible in the
//! AST/call structure alone, where a taint query has nothing to trace because
//! the defect IS the missing use of a value (or the missing sibling call).
//! Driven from rule packs via the rule `kind` field (see `cpg-cli::rules`);
//! findings reuse [`Finding`] so SARIF/serve output needs no new plumbing.
//!
//! Two shapes, both distilled from confirmed cross-tenant bugs:
//!
//! * [`discarded_returns`] — a verification call's return values are bound to
//!   blank identifiers (`_, _, ok := cache.CheckToken(tok, uuid)`): the
//!   verified principal is discarded, and the caller almost always substitutes
//!   a caller-controlled value (header/cookie) for the same semantic role.
//!   The cache-hit account-swap shape.
//!
//! * [`append_without_delete`] — a header (or map-like) `Add` of a constant
//!   trust-decision key in a method that never `Del`s/`Set`s that key: on a
//!   proxy that copies inbound headers wholesale, the pinned value is APPENDED
//!   after the client's copies, and downstream `Get` readers see the FIRST
//!   (client) value. The duplicate-header smuggling shape.

use crate::pass::ast_descendants;
use crate::taint::{Finding, Provenance, Step};
use cpg_core::{Cpg, EdgeKind, NodeKind, Query};
use std::collections::{HashMap, HashSet};

/// Case-sensitive name match with `*` wildcards anywhere in the pattern
/// (`Check*Token`, `X-*`, `*Authenticate*`). A lone `*` matches everything.
fn pat_match(pat: &str, s: &str) -> bool {
    fn rec(p: &[u8], t: &[u8]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some((b'*', rest)) => {
                (0..=t.len()).any(|i| rec(rest, &t[i..]))
            }
            Some((c, rest)) => t.split_first().is_some_and(|(tc, tr)| tc == c && rec(rest, tr)),
        }
    }
    rec(pat.as_bytes(), s.as_bytes())
}

fn matches_any(pats: &[&str], s: &str) -> bool {
    pats.iter().any(|p| pat_match(p, s))
}

/// Test-code filter: both censuses skip methods living in test files —
/// deliberately-partial bindings and fixture headers are the norm there
/// (measured: every one of a large service's hits was a `_test.go`).
fn is_test_path(path: &str) -> bool {
    let base = path.rsplit('/').next().unwrap_or(path);
    base.ends_with("_test.go")
        || base.starts_with("test_")
        || base.contains(".test.")
        || base.contains(".spec.")
        || base.ends_with("Test.scala")
        || base.ends_with("Test.java")
        || base.ends_with("Spec.scala")
}

fn in_test_file(cpg: &Cpg, method: cpg_core::NodeId) -> bool {
    cpg.path_of(cpg.file_of(method)).is_some_and(is_test_path)
}

/// Strip one layer of matching string quotes from a literal's code
/// (`"X-Method"` / `'k'` / `` `raw` ``); returns `None` for non-string
/// literals (numbers, bools) so key checks skip them.
fn literal_string(code: &str) -> Option<&str> {
    let b = code.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'' || b[0] == b'`') && b[b.len() - 1] == b[0] {
        Some(&code[1..code.len() - 1])
    } else {
        None
    }
}

/// Binding names on the LHS of an assignment statement's code text, in TRUE
/// source order. The graph alone is not enough in every language: Scala's
/// `val (_, _, ok) = f(x)` lowers to one `=` call per NAMED element — the
/// `_` wildcards never reach the graph — while Go captures them as `_`
/// identifiers. The statement text has them all. Bails (None) on
/// comparison/compound heads (`==`, `+=`, ...) and on any element that does
/// not start identifier-shaped, so callers can fall back to graph names.
fn lhs_bindings(code: &str) -> Option<Vec<String>> {
    let bytes = code.as_bytes();
    let mut eq = None;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'=' {
            if bytes.get(i + 1) == Some(&b'=')
                || (i > 0
                    && matches!(
                        bytes[i - 1],
                        b'<' | b'>' | b'!' | b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^'
                    ))
            {
                return None;
            }
            eq = Some(i);
            break;
        }
    }
    let lhs = code[..eq?].trim_end();
    let mut s = lhs.strip_suffix(':').unwrap_or(lhs).trim();
    // Declaration keywords (each only when followed by whitespace/paren, so
    // an identifier like `value` is never clipped).
    for kw in ["lazy", "val", "var", "let", "const"] {
        if let Some(rest) = s.strip_prefix(kw) {
            if rest.starts_with(|c: char| c.is_whitespace() || c == '(') {
                s = rest.trim_start();
            }
        }
    }
    // Tuple-pattern parens: `(_, _, ok)`.
    if let Some(inner) = s.strip_prefix('(').and_then(|t| t.trim_end().strip_suffix(')')) {
        s = inner;
    }
    let parts: Vec<String> = s
        .split(',')
        .map(|p| {
            p.trim()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                .collect::<String>()
        })
        .collect();
    if parts.is_empty() || parts.iter().any(String::is_empty) {
        return None;
    }
    Some(parts)
}

/// Findings for verification calls whose returns are discarded, two shapes:
///
/// * PARTIAL discard — an `=` binding whose rhs is a call named in
///   `callee_pats` and whose multi-target statement binds at least one blank
///   `_`. One finding per (method, line, callee) — the frontend lowers
///   `a, b := f()` to one `=` call per target (same line, same statement
///   text), so bindings are grouped before judging. Blank detection reads
///   the statement TEXT first ([`lhs_bindings`]) because Scala drops `_`
///   pattern elements from the graph entirely; `origin` carries the binding
///   list in source order (`discarded-return lhs=_,_,ok`).
///
/// * TOTAL discard — a vocabulary call in bare statement position (a direct
///   AST child of a Block: `cache.CheckToken(tok, id)` as its own
///   statement, legal in Go and Scala): every return, verdict included, is
///   unused. Wrapper shapes are naturally excluded — a returned call sits
///   under a Return node, not a Block.
pub fn discarded_returns(cpg: &Cpg, callee_pats: &[&str]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for m in cpg.methods() {
        if in_test_file(cpg, m) {
            continue;
        }
        // (line, callee, statement code) -> (lhs names in binding order, one witness call node)
        let mut groups: HashMap<(Option<u32>, String, String), (Vec<String>, cpg_core::NodeId)> =
            HashMap::new();
        let mut stmt_hits: Vec<cpg_core::NodeId> = Vec::new();
        let mut stmt_seen: HashSet<(Option<u32>, String)> = HashSet::new();
        for n in ast_descendants(cpg, m) {
            if cpg.kind_of(n) == NodeKind::Block {
                // Total discard: a vocabulary call as its own statement.
                for c in cpg.out_kind(n, EdgeKind::Ast) {
                    if cpg.kind_of(c) != NodeKind::Call {
                        continue;
                    }
                    let Some(name) = cpg.name_of(c) else { continue };
                    if name == "=" || !matches_any(callee_pats, name) {
                        continue;
                    }
                    if stmt_seen.insert((cpg.line_of(c), name.to_string())) {
                        stmt_hits.push(c);
                    }
                }
                continue;
            }
            if cpg.kind_of(n) != NodeKind::Call || cpg.name_of(n) != Some("=") {
                continue;
            }
            let args = cpg.arguments_of(n);
            let (Some(&lhs), Some(&rhs)) = (args.first(), args.get(1)) else { continue };
            if cpg.kind_of(lhs) != NodeKind::Identifier || cpg.kind_of(rhs) != NodeKind::Call {
                continue;
            }
            let Some(callee) = cpg.name_of(rhs) else { continue };
            if callee == "=" || !matches_any(callee_pats, callee) {
                continue;
            }
            let key = (
                cpg.line_of(n),
                callee.to_string(),
                cpg.code_of(n).unwrap_or("").to_string(),
            );
            let entry = groups.entry(key).or_insert_with(|| (Vec::new(), rhs));
            entry.0.push(cpg.name_of(lhs).unwrap_or("").to_string());
        }
        let method_name = cpg.full_name_of(m).unwrap_or("<unknown>").to_string();
        let mut hits: Vec<_> = groups
            .into_iter()
            .filter_map(|((line, callee, code), (graph_names, witness))| {
                // Statement text is authoritative when it parses to at least
                // as many bindings as the graph shows (it includes elements
                // the frontend dropped, and preserves source order); the
                // graph list — sorted, traversal order is arbitrary — is the
                // fallback.
                let names = match lhs_bindings(&code) {
                    Some(parsed) if parsed.len() >= graph_names.len() => parsed,
                    _ => {
                        let mut g = graph_names;
                        g.sort();
                        g
                    }
                };
                names
                    .iter()
                    .any(|n| n == "_")
                    .then_some((line, callee, code, names, witness))
            })
            .collect();
        hits.sort_by_key(|(line, callee, ..)| (*line, callee.clone()));
        for (line, callee, code, lhs_names, witness) in hits {
            findings.push(Finding {
                method: method_name.clone(),
                sink: callee,
                sink_line: line,
                sink_file: cpg.path_of(cpg.file_of(witness)).map(str::to_string),
                origin: format!("discarded-return lhs={}", lhs_names.join(",")),
                path: vec![Step {
                    code,
                    line,
                    provenance: Provenance::IntraProc,
                    depth: 0,
                }],
                guard: None,
                authz: None,
                confined: None,
            });
        }
        for c in stmt_hits {
            findings.push(Finding {
                method: method_name.clone(),
                sink: cpg.name_of(c).unwrap_or("").to_string(),
                sink_line: cpg.line_of(c),
                sink_file: cpg.path_of(cpg.file_of(c)).map(str::to_string),
                origin: "discarded-return statement-position (all returns unused)".to_string(),
                path: vec![Step {
                    code: cpg.code_of(c).unwrap_or("").to_string(),
                    line: cpg.line_of(c),
                    provenance: Provenance::IntraProc,
                    depth: 0,
                }],
                guard: None,
                authz: None,
                confined: None,
            });
        }
    }
    findings
}

/// Findings for constant-key appends with no clearing sibling: a call named
/// in `add_pats` whose FIRST argument is a string literal matching
/// `key_pats` (empty = any key), in a method with no `clear_pats` call on
/// the same key (case-insensitive — HTTP header names are). `Set` belongs in
/// `clear_pats`: it replaces all values for the key, so it cannot leave a
/// client copy in front. One finding per (method, line, key).
///
/// Second sink shape, for header lists that ride a struct transport instead
/// of a map API (`append(hdrs, &Header{Key: "X-Method", ...})` — the proto
/// tunnel spelling of the same bug): when an add-call's first argument is
/// NOT a string literal, every key-vocabulary-matching string literal in the
/// call's argument subtree is treated as an appended key. Requires a
/// non-empty `key_pats` (with an empty vocabulary every literal would
/// qualify), and slices have no canonical delete spelling, so these hits are
/// pure census — the triage question is whether the list ever held inbound
/// client entries.
pub fn append_without_delete(
    cpg: &Cpg,
    add_pats: &[&str],
    clear_pats: &[&str],
    key_pats: &[&str],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for m in cpg.methods() {
        if in_test_file(cpg, m) {
            continue;
        }
        let mut adds: Vec<(cpg_core::NodeId, String, String)> = Vec::new(); // (call, name, key)
        let mut cleared: HashSet<String> = HashSet::new();
        for n in ast_descendants(cpg, m) {
            if cpg.kind_of(n) != NodeKind::Call {
                continue;
            }
            let Some(name) = cpg.name_of(n) else { continue };
            let is_add = matches_any(add_pats, name);
            let is_clear = matches_any(clear_pats, name);
            if !is_add && !is_clear {
                continue;
            }
            let args = cpg.arguments_of(n);
            let Some(&k) = args.first() else { continue };
            if cpg.kind_of(k) == NodeKind::Literal {
                let Some(key) = cpg.code_of(k).and_then(literal_string) else { continue };
                if is_clear {
                    cleared.insert(key.to_ascii_lowercase());
                }
                if is_add {
                    adds.push((n, name.to_string(), key.to_string()));
                }
            } else if is_add && !key_pats.is_empty() {
                // Struct-transport shape: the key is a literal somewhere in
                // the argument subtree (composite literals lower to
                // constructor-shaped calls, reachable via Ast edges).
                // Non-constant CLEAR keys stay uncredited either way.
                for d in ast_descendants(cpg, n) {
                    if cpg.kind_of(d) != NodeKind::Literal {
                        continue;
                    }
                    let Some(key) = cpg.code_of(d).and_then(literal_string) else { continue };
                    if matches_any(key_pats, key) {
                        adds.push((n, name.to_string(), key.to_string()));
                    }
                }
            }
        }
        let method_name = cpg.full_name_of(m).unwrap_or("<unknown>").to_string();
        let mut seen: HashSet<(Option<u32>, String)> = HashSet::new();
        for (call, name, key) in adds {
            if !key_pats.is_empty() && !matches_any(key_pats, &key) {
                continue;
            }
            if cleared.contains(&key.to_ascii_lowercase()) {
                continue;
            }
            let line = cpg.line_of(call);
            if !seen.insert((line, key.to_ascii_lowercase())) {
                continue;
            }
            findings.push(Finding {
                method: method_name.clone(),
                sink: name,
                sink_line: line,
                sink_file: cpg.path_of(cpg.file_of(call)).map(str::to_string),
                origin: format!("append-without-delete key={key}"),
                path: vec![Step {
                    code: cpg.code_of(call).unwrap_or("").to_string(),
                    line,
                    provenance: Provenance::IntraProc,
                    depth: 0,
                }],
                guard: None,
                authz: None,
                confined: None,
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpg_frontend::Frontend;

    fn build_with(mut fe: cpg_lang_ts::TsFrontend, files: &[(&str, &str)]) -> Cpg {
        let mut cpg = Cpg::new();
        let mut fids = Vec::new();
        for (path, src) in files {
            fids.push(fe.build_file(&mut cpg, path, src).file);
        }
        let pm = crate::standard_pipeline();
        let idx = crate::pass::method_name_index(&cpg);
        let ctx = crate::pass::PassContext { methods_by_name: Some(&idx) };
        pm.run_all(&mut cpg, &fids, &ctx);
        cpg
    }

    fn build_go(files: &[(&str, &str)]) -> Cpg {
        build_with(cpg_lang_ts::TsFrontend::go(), files)
    }

    fn build_scala(files: &[(&str, &str)]) -> Cpg {
        build_with(cpg_lang_ts::TsFrontend::scala(), files)
    }

    #[test]
    fn lhs_bindings_parses_binding_shapes() {
        assert_eq!(
            lhs_bindings("_, _, ok := cache.CheckToken(t, u)"),
            Some(vec!["_".into(), "_".into(), "ok".into()])
        );
        assert_eq!(
            lhs_bindings("val (_, _, ok) = cache.CheckToken(t, u)"),
            Some(vec!["_".into(), "_".into(), "ok".into()])
        );
        assert_eq!(lhs_bindings("acct, err := f()"), Some(vec!["acct".into(), "err".into()]));
        assert_eq!(lhs_bindings("val ok = f()"), Some(vec!["ok".into()]));
        // Comparison / compound heads are not bindings.
        assert_eq!(lhs_bindings("ok == f()"), None);
        assert_eq!(lhs_bindings("n += f()"), None);
        // An identifier merely starting with a keyword is never clipped.
        assert_eq!(lhs_bindings("value = f()"), Some(vec!["value".into()]));
        assert_eq!(lhs_bindings("no assignment here"), None);
    }

    #[test]
    fn scala_tuple_blank_binding_is_flagged_via_statement_text() {
        // Scala drops `_` pattern elements from the graph entirely (one `=`
        // call per NAMED element), so this shape is only visible in the
        // statement text.
        let cpg = build_scala(&[(
            "Auth.scala",
            r#"object A {
  def hit(tok: String): Boolean = {
    val (_, _, ok) = cache.CheckToken(tok, "u")
    ok
  }
  def clean(tok: String): (String, Boolean) = {
    val (acct, exp, ok) = cache.CheckToken(tok, "u")
    log(exp)
    (acct, ok)
  }
}
"#,
        )]);
        let f = discarded_returns(&cpg, &["CheckToken"]);
        assert_eq!(f.len(), 1, "{f:#?}");
        assert!(f[0].method.contains("hit"), "{}", f[0].method);
        assert_eq!(f[0].origin, "discarded-return lhs=_,_,ok");
    }

    #[test]
    fn statement_position_call_is_a_total_discard() {
        let cpg = build_go(&[(
            "stmt.go",
            r#"package a

func hitStmt(tok string) {
    cache.CheckToken(tok, "u")
    doOther()
}

func wrapper(tok string) (string, string, bool) {
    return cache.CheckToken(tok, "u")
}
"#,
        )]);
        let f = discarded_returns(&cpg, &["CheckToken"]);
        // The bare statement is a total discard; the returned call is a
        // wrapper (sits under Return, not Block) and must not flag.
        assert_eq!(f.len(), 1, "{f:#?}");
        assert!(f[0].method.contains("hitStmt"), "{}", f[0].method);
        assert!(f[0].origin.contains("statement-position"), "{}", f[0].origin);

        // Scala spelling: mid-block statement flags; a last-expression call
        // (the method's value) does not.
        let sc = build_scala(&[(
            "Stmt.scala",
            r#"object S {
  def hit(tok: String): Unit = {
    cache.CheckToken(tok, "u")
    doOther()
  }
  def last(tok: String) = {
    cache.CheckToken(tok, "u")
  }
}
"#,
        )]);
        let f = discarded_returns(&sc, &["CheckToken"]);
        assert_eq!(f.len(), 1, "{f:#?}");
        assert!(f[0].method.contains("hit"), "{}", f[0].method);
    }

    #[test]
    fn pat_match_star_semantics() {
        assert!(pat_match("CheckToken", "CheckToken"));
        assert!(!pat_match("CheckToken", "checkToken"));
        assert!(pat_match("Check*Token", "CheckServiceToken"));
        assert!(pat_match("X-*", "X-Method"));
        assert!(pat_match("*Authenticate*", "reAuthenticateUser"));
        assert!(pat_match("*", "anything"));
        assert!(!pat_match("X-*", "Y-Method"));
    }

    #[test]
    fn test_files_are_excluded() {
        let cpg = build_go(&[(
            "auth_test.go",
            "package a\nfunc TestHit(t *T) {\n    _, _, ok := cache.CheckToken(\"t\", \"u\")\n    _ = ok\n    h.Add(\"X-Method\", \"GET\")\n}\n",
        )]);
        assert!(discarded_returns(&cpg, &["CheckToken"]).is_empty());
        assert!(append_without_delete(&cpg, &["Add"], &["Del"], &["X-*"]).is_empty());
    }

    #[test]
    fn literal_string_strips_quotes_only() {
        assert_eq!(literal_string("\"X-Method\""), Some("X-Method"));
        assert_eq!(literal_string("`raw`"), Some("raw"));
        assert_eq!(literal_string("42"), None);
        assert_eq!(literal_string("\"unterminated"), None);
    }

    #[test]
    fn discarded_return_flags_blank_bound_auth_call() {
        let cpg = build_go(&[(
            "auth.go",
            r#"package a

func hit(token string, uuid string) string {
    _, _, ok := cache.CheckToken(token, uuid)
    if !ok {
        return ""
    }
    return readCookie()
}

func full(token string, uuid string) string {
    acct, _, ok := cache.CheckToken(token, uuid)
    if !ok {
        return ""
    }
    return acct
}

func unrelated() {
    _, err := os.Open("f")
    _ = err
}
"#,
        )]);
        let f = discarded_returns(&cpg, &["CheckToken"]);
        // Both CheckToken statements bind a blank; the all-blank-but-ok one
        // and the acct-keeping one (still discards a return). `os.Open` is
        // out of vocabulary.
        assert_eq!(f.len(), 2, "{f:#?}");
        let hit = f.iter().find(|f| f.method.contains("hit")).expect("hit finding");
        assert_eq!(hit.sink, "CheckToken");
        assert!(hit.origin.contains("_,_,ok"), "origin lists bindings: {}", hit.origin);
        assert_eq!(hit.sink_file.as_deref(), Some("auth.go"));
        // No finding for a binding with no blanks at all.
        let none = discarded_returns(&cpg, &["Open"]);
        assert_eq!(none.len(), 1, "os.Open discards its handle here");
        let clean = build_go(&[(
            "clean.go",
            "package a\nfunc g(t string) (string, bool) {\n    acct, exp, ok := cache.CheckToken(t, \"u\")\n    _ = exp\n    return acct, ok\n}\n",
        )]);
        assert!(discarded_returns(&clean, &["CheckToken"]).is_empty());
    }

    #[test]
    fn append_without_delete_flags_undeleted_trust_header() {
        let cpg = build_go(&[(
            "proxy.go",
            r#"package p

func bad(dst http.Header, method string) {
    dst.Add("X-Method", method)
    dst.Add("Accept", "anything")
}

func good(dst http.Header, method string) {
    dst.Del("X-Method")
    dst.Add("X-Method", method)
}

func replaced(dst http.Header, method string) {
    dst.Set("X-Method", method)
}

func dynamic(dst http.Header, k string, v string) {
    dst.Add(k, v)
}
"#,
        )]);
        let f = append_without_delete(&cpg, &["Add"], &["Del", "Set"], &["X-*"]);
        assert_eq!(f.len(), 1, "{f:#?}");
        assert!(f[0].method.contains("bad"));
        assert_eq!(f[0].origin, "append-without-delete key=X-Method");
        // "Accept" fails the key vocabulary; `good` clears first; `replaced`
        // uses Set (not an add pattern); `dynamic` has no constant key.
        // Empty key vocabulary = every constant key is in scope.
        let all = append_without_delete(&cpg, &["Add"], &["Del", "Set"], &[]);
        assert_eq!(all.len(), 2, "{all:#?}");
    }

    #[test]
    fn append_flags_struct_transport_header_key() {
        let cpg = build_go(&[(
            "tunnel.go",
            r#"package p

func toTunnel(headers map[string][]string, method string, path string) []*Header {
    out := make([]*Header, 0, len(headers))
    for key, vs := range headers {
        for _, value := range vs {
            out = append(out, &Header{Key: key, Value: value})
        }
    }
    out = append(out, &Header{Key: "X-Method", Value: method})
    out = append(out, &Header{Key: "X-ActualURLPath", Value: path})
    return out
}

func unrelatedAppend(xs []int) []int {
    return append(xs, 1)
}
"#,
        )]);
        let f = append_without_delete(&cpg, &["Add", "append"], &["Del", "Set"], &["X-*"]);
        assert_eq!(f.len(), 2, "{f:#?}");
        let keys: Vec<&str> = f.iter().map(|f| f.origin.as_str()).collect();
        assert!(keys.contains(&"append-without-delete key=X-Method"), "{keys:?}");
        assert!(keys.contains(&"append-without-delete key=X-ActualURLPath"), "{keys:?}");
        // The wholesale-copy append (dynamic key) and the []int append carry
        // no vocabulary literal; with an empty key vocabulary the struct
        // shape is disabled entirely.
        assert!(append_without_delete(&cpg, &["append"], &[], &[]).is_empty());
    }

    #[test]
    fn clear_key_comparison_is_case_insensitive() {
        let cpg = build_go(&[(
            "case.go",
            "package p\nfunc f(dst http.Header, v string) {\n    dst.Del(\"x-method\")\n    dst.Add(\"X-Method\", v)\n}\n",
        )]);
        assert!(append_without_delete(&cpg, &["Add"], &["Del"], &["X-*"]).is_empty());
    }
}
