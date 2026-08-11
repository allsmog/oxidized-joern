//! Authorization-dominance annotation (advisory post-pass, like
//! [`crate::taint::annotate_guards`]).
//!
//! For every finding, answer: *does an authorization check run on EVERY
//! execution that reaches the sink?* — i.e. does an authz-shaped call
//! CFG-dominate the flow. The three verdicts, in triage-priority order:
//!
//! - `None` — no authz-shaped call anywhere near the flow. Either the
//!   endpoint is genuinely unguarded (triage FIRST) or the check lives in
//!   framework middleware the CPG cannot see.
//! - `authz-partial@<line>` — an authz call exists in a method on the flow
//!   but does NOT dominate the sink: some path reaches the sink without
//!   passing it (a branch-only check, or a check placed after the sink).
//!   This is the authz-bypass bug shape — triage SECOND.
//! - `authz-dominated@<line>` — an authz call dominates the flow; every
//!   execution reaching the sink executed the check. Triage last.
//!
//! What counts as an authz call: names listed in a rule's `authz` array
//! (spec-driven, exact simple-name match) plus a general name heuristic
//! ([`is_authz_name`]) so the annotation is useful with zero configuration.
//!
//! Dominance is computed on the intra-procedural CFG
//! ([`crate::cfg::cfg_edges_for_method`], recomputed on the fly so the pass
//! works whether or not the Cfg layer was materialised): call `a` dominates
//! node `t` in method `m` iff `t` is unreachable from `m`'s entry once `a`
//! is removed. Two anchor methods are checked per finding — the ENTRY method
//! (does a check dominate the call site that continues the flow?) and the
//! SINK's own method (does a check dominate the sink call itself?) — which
//! covers the two common placements: the handler-level gate and the
//! sink-adjacent gate. Recorded limits: intermediate spliced callees are not
//! walked, and dominance of the CALL does not prove its RESULT gates
//! anything (`authorize(u); sink(x)` with an ignored result still counts) —
//! the annotation is evidence for triage ordering, never suppression.

use crate::pass::ast_descendants;
use crate::taint::{Finding, TaintSpec};
use cpg_core::{Cpg, NodeId, NodeKind, Query};
use std::collections::{HashMap, HashSet, VecDeque};

/// Annotate `findings` in place with authz-dominance evidence.
pub fn annotate_authz(cpg: &Cpg, spec: &TaintSpec, findings: &mut [Finding]) {
    if findings.is_empty() {
        return;
    }
    for f in findings.iter_mut() {
        // Anchor order = execution order: the entry-method gate runs before
        // the sink-adjacent gate, so it is reported preferentially.
        let mut anchors: Vec<(NodeId, NodeId)> = Vec::new();
        let sink = sink_anchor(cpg, f);
        if let Some(e) = entry_anchor(cpg, f) {
            if sink.is_none_or(|(sm, _)| sm != e.0) {
                anchors.push(e);
            }
        }
        anchors.extend(sink);

        let mut dominated: Option<u32> = None;
        let mut partial: Option<u32> = None;
        'outer: for (method, target) in anchors {
            let adj = adjacency(&crate::cfg::cfg_edges_for_method(cpg, method));
            if !reaches(&adj, method, target, None) {
                continue; // target not on the CFG (defensive) — no evidence
            }
            for a in authz_calls(cpg, &spec.authz_methods, method) {
                if a == target {
                    continue;
                }
                let line = cpg.line_of(a).unwrap_or(0);
                if partial.is_none() {
                    partial = Some(line);
                }
                if !reaches(&adj, method, target, Some(a)) {
                    dominated = Some(line);
                    break 'outer;
                }
            }
        }
        f.authz = match (dominated, partial) {
            (Some(l), _) => Some(format!("authz-dominated@{l}")),
            (None, Some(l)) => Some(format!("authz-partial@{l}")),
            (None, None) => None,
        };
    }
}

/// The method containing the sink (closest preceding method start in the
/// sink's file — the same rule `annotate_guards` uses) and the sink call
/// node itself.
fn sink_anchor(cpg: &Cpg, f: &Finding) -> Option<(NodeId, NodeId)> {
    let line = f.sink_line?;
    let file = f.sink_file.as_deref()?;
    let mut best: Option<(u32, NodeId)> = None;
    for m in cpg.methods() {
        if cpg.path_of(cpg.file_of(m)) != Some(file) {
            continue;
        }
        let Some(ml) = cpg.line_of(m) else { continue };
        if ml <= line && best.is_none_or(|(bl, _)| ml > bl) {
            best = Some((ml, m));
        }
    }
    let (_, method) = best?;
    let at_line: Vec<NodeId> = ast_descendants(cpg, method)
        .into_iter()
        .filter(|&n| cpg.kind_of(n) == NodeKind::Call && cpg.line_of(n) == Some(line))
        .collect();
    let target = at_line
        .iter()
        .copied()
        .find(|&c| cpg.name_of(c) == Some(f.sink.as_str()))
        .or_else(|| at_line.first().copied())?;
    Some((method, target))
}

/// The finding's reporting method and, inside it, the outermost call at the
/// line of the last depth-0 witness step — the call site through which the
/// flow leaves the entry method (or the sink itself for a fully
/// intraprocedural finding, in which case `sink_anchor` supersedes this).
fn entry_anchor(cpg: &Cpg, f: &Finding) -> Option<(NodeId, NodeId)> {
    let method = cpg
        .methods()
        .into_iter()
        .find(|&m| cpg.full_name_of(m) == Some(f.method.as_str()))
        .or_else(|| {
            cpg.methods()
                .into_iter()
                .find(|&m| cpg.name_of(m) == Some(f.method.as_str()))
        })?;
    let step = f
        .path
        .iter()
        .rev()
        .find(|s| s.depth == 0 && s.line.is_some())?;
    let line = step.line?;
    let at_line: Vec<NodeId> = ast_descendants(cpg, method)
        .into_iter()
        .filter(|&n| cpg.kind_of(n) == NodeKind::Call && cpg.line_of(n) == Some(line))
        .collect();
    // Pre-order: the first call at the line is the outermost — executed
    // last, after its arguments, so dominating it dominates the descent.
    let target = at_line
        .iter()
        .copied()
        .find(|&c| cpg.code_of(c) == Some(step.code.as_str()))
        .or_else(|| at_line.first().copied())?;
    Some((method, target))
}

/// Every authz-shaped call in `method`: spec-listed names first-class, the
/// general name heuristic as the zero-config tier. Only actual INVOCATIONS
/// count (code carries an argument list): the member-read lowering turns
/// every field read into a Call node, and a field named `aclResolverFactory`
/// is authz DATA flowing by, not a check being enforced.
/// A factory-returned closure (`return func(...) { verifyRBAC(...) }`) is
/// lowered as a separate Method node with a MethodRef left in expression
/// position, so a plain subtree walk never sees its body. Follow every
/// MethodRef descendant to its Method (matched by name AND line — lambda
/// names like `<anon>` are ambiguous on their own), visited-bounded.
pub(crate) fn authz_calls(cpg: &Cpg, authz_names: &HashSet<String>, method: NodeId) -> Vec<NodeId> {
    let mut ref_index: Option<HashMap<(&str, u32), Vec<NodeId>>> = None;
    let mut visited: HashSet<NodeId> = HashSet::new();
    let mut work = vec![method];
    let mut out = Vec::new();
    while let Some(m) = work.pop() {
        if !visited.insert(m) {
            continue;
        }
        for n in ast_descendants(cpg, m) {
            match cpg.kind_of(n) {
                NodeKind::Call => {
                    let (Some(nm), Some(code)) = (cpg.name_of(n), cpg.code_of(n)) else {
                        continue;
                    };
                    if (authz_names.contains(nm) || is_authz_name(nm)) && is_invocation(code, nm) {
                        out.push(n);
                    }
                }
                NodeKind::MethodRef => {
                    let idx = ref_index.get_or_insert_with(|| {
                        let mut idx: HashMap<(&str, u32), Vec<NodeId>> = HashMap::new();
                        for m in cpg.methods() {
                            if let (Some(nm), Some(ln)) = (cpg.name_of(m), cpg.line_of(m)) {
                                idx.entry((nm, ln)).or_default().push(m);
                            }
                        }
                        idx
                    });
                    if let (Some(nm), Some(ln)) = (cpg.name_of(n), cpg.line_of(n)) {
                        if let Some(ms) = idx.get(&(nm, ln)) {
                            work.extend(ms.iter().copied());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// Is `code` an actual invocation OF `name` — i.e. does `name` itself carry
/// the argument list? `code.contains('(')` is not enough: the field-read
/// lowering emits `in.GetReqCtx().AuthzCtx` as a Call named `AuthzCtx` whose
/// code contains a paren from the BASE accessor, and a Go composite literal
/// `LoginSessionInfo{F: g()}` is a type-named Call with parens from its
/// element values. Both are authz DATA, not checks. The test: some
/// word-boundary occurrence of `name` in `code` is followed (after optional
/// whitespace) by `(`.
pub(crate) fn is_invocation(code: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let bytes = code.as_bytes();
    let mut from = 0;
    while let Some(pos) = code[from..].find(name) {
        let start = from + pos;
        let end = start + name.len();
        from = start + 1;
        let boundary_before =
            start == 0 || !(bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
        if !boundary_before {
            continue;
        }
        let rest = code[end..].trim_start();
        if rest.starts_with('(') {
            return true;
        }
    }
    false
}

/// Does this call name look like an authorization / access-control check?
/// General across codebases by construction: exact word-token matching over
/// the camelCase/snake_case split, never raw substrings (`cancel` must not
/// match `can`, `accessor` must not match `access`). Accessor-verb leading
/// tokens disqualify the match: `GetAuthzContext()` / `ListPermissions()`
/// READ authz data, they do not enforce it — counting them as checks would
/// wrongly deprioritize unenforced flows (observed when a context accessor
/// supplied an account identifier). Spec-listed names bypass this.
pub fn is_authz_name(name: &str) -> bool {
    // Single tokens that are authz vocabulary on their own.
    const SINGLE: &[&str] = &[
        "authz",
        "authn",
        "acl",
        "rbac",
        "authorize",
        "authorized",
        "authorizes",
        "authorization",
        "authorise",
        "authorised",
        "authorisation",
        "authenticate",
        "authenticated",
        "authentication",
        "permission",
        "permissions",
        "permitted",
        "privilege",
        "privileges",
        "entitlement",
        "entitlements",
        "entitled",
    ];
    // A check-verb combined with an access-control noun (hasRole, IsAdmin,
    // checkAccess, requireAuth, validateScope, ensureOwner, canAccess...).
    const VERBS: &[&str] = &[
        "check",
        "checks",
        "has",
        "have",
        "require",
        "requires",
        "required",
        "verify",
        "validate",
        "validator",
        "validators",
        "assert",
        "ensure",
        "must",
        "is",
        "can",
        "may",
        "enforce",
        "with",
        "guard",
    ];
    const NOUNS: &[&str] = &[
        "auth",
        "access",
        "admin",
        "role",
        "roles",
        "scope",
        "scopes",
        "owner",
        "ownership",
        "allowed",
        "perm",
        "perms",
        "jwt",
        "capability",
        "capabilities",
        "oauth",
    ];
    let toks = word_tokens(name);
    if !name_shape_ok(&toks) {
        return false;
    }
    if toks.iter().any(|t| SINGLE.contains(&t.as_str())) {
        return true;
    }
    toks.iter().any(|t| VERBS.contains(&t.as_str()))
        && toks.iter().any(|t| NOUNS.contains(&t.as_str()))
}

/// Shape guards shared by [`is_authz_name`] and [`is_authz_qualified`]: a
/// name whose leading token reads/constructs/mutates data, or whose trailing
/// token is a data descriptor, is authz DATA flowing by — never a check.
fn name_shape_ok(toks: &[String]) -> bool {
    // Leading tokens that read, shape, or construct authz DATA without
    // enforcing anything: accessors (`GetAuthzContext`), converters
    // (`NormalizeAuthzContextFromReqCtx`, `toAuthzProto`), constructors
    // (`NewEnforcer` builds the checker; the check is a later call on it).
    const ACCESSOR_VERBS: &[&str] = &[
        "get",
        "set",
        "list",
        "fetch",
        "load",
        "lookup",
        "new",
        "make",
        "create",
        "init",
        "parse",
        "convert",
        "normalize",
        "extract",
        "derive",
        "build",
        "wrap",
        "unwrap",
        "marshal",
        "unmarshal",
        "encode",
        "decode",
        "serialize",
        "deserialize",
        "to",
        "from",
        "compute",
        "recompute",
        "union",
        "merge",
    ];
    // Leading tokens that MUTATE authz data. `upsertRolesForNestedTenant`,
    // `updatePolicyGroupVersion`, `ArchivePolicyGroupsByRuleIDs`, and
    // `modifyPermissions` are operations ON permission/entitlement rows — the
    // very actions a check would gate, never the gate itself. Enforcement
    // names lead with check/require/validate/enforce/has/is/can/verify/...,
    // so a mutation-verb head disqualifies. Validation-corpus review found
    // this to be a recurring source of false inline-partial census verdicts.
    const MUTATION_VERBS: &[&str] = &[
        "upsert",
        "update",
        "insert",
        "delete",
        "remove",
        "add",
        "archive",
        "send",
        "publish",
        "emit",
        "provision",
        "modify",
        "save",
        "store",
        "write",
        "sync",
        "persist",
        "assign",
        "apply",
        "grant",
        "revoke",
        "terminate",
        "reset",
        "enable",
        "disable",
    ];
    // A name ENDING in a data-descriptor token names an authz-adjacent
    // value or component (`aclDeserializerFactory`, `authzContext`,
    // `AuthorizationError`), not an enforcement point.
    const DATA_SUFFIXES: &[&str] = &[
        "factory",
        "builder",
        "resolver",
        "context",
        "ctx",
        "config",
        "conf",
        "dao",
        "client",
        "provider",
        "serializer",
        "deserializer",
        "util",
        "utils",
        "helper",
        "error",
        "err",
        "string",
        "type",
        "types",
        "id",
        "ids",
        "key",
        "keys",
        "name",
        "names",
        "info",
        "group",
        "groups",
        "db",
        "database",
        "callback",
        "callbacks",
    ];
    if toks.first().is_some_and(|t| {
        ACCESSOR_VERBS.contains(&t.as_str()) || MUTATION_VERBS.contains(&t.as_str())
    }) {
        return false;
    }
    !toks
        .last()
        .is_some_and(|t| DATA_SUFFIXES.contains(&t.as_str()))
}

/// Authz classification for a QUALIFIED spelling (`rbac.UnaryServerInterceptor`,
/// `authz::check`, `acl.Middleware`): vocabulary living in the package/receiver
/// qualifier counts, but only when the base name itself has enforcement shape
/// (per [`name_shape_ok`]) — `authz.NewClient` constructs a client, `rbac.
/// PermissionDao` is data, neither enforces. The qualifier must clear the FULL
/// name test on its own (`rbac` hits the single-token vocabulary; a mere `auth`
/// noun does not), so telemetry qualifiers (`grpcmetrics.…`) never transfer.
pub(crate) fn is_authz_qualified(spelling: &str) -> bool {
    let trimmed = spelling.trim();
    let split = trimmed
        .char_indices()
        .rev()
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_'))
        .map(|(i, c)| (i, c.len_utf8()));
    let (qual, base) = match split {
        Some((i, w)) => (&trimmed[..i], &trimmed[i + w..]),
        None => return is_authz_name(trimmed),
    };
    if base.is_empty() {
        return false;
    }
    if is_authz_name(base) {
        return true;
    }
    if qual.is_empty() || !name_shape_ok(&word_tokens(base)) {
        return false;
    }
    is_authz_name(qual)
}

/// Split an identifier into lowercase word tokens: `checkACL` → [check, acl],
/// `RBACCheck` → [rbac, check], `has_role` → [has, role].
pub(crate) fn word_tokens(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    for i in 0..chars.len() {
        let c = chars[i];
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if c.is_uppercase() {
            let prev_lower = i > 0 && (chars[i - 1].is_lowercase() || chars[i - 1].is_numeric());
            let acronym_end = i > 0
                && chars[i - 1].is_uppercase()
                && chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if (prev_lower || acronym_end) && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        }
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn adjacency(edges: &[(NodeId, NodeId)]) -> HashMap<NodeId, Vec<NodeId>> {
    let mut adj: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for &(s, d) in edges {
        adj.entry(s).or_default().push(d);
    }
    adj
}

/// BFS reachability `from` → `to` over the CFG adjacency, optionally with one
/// node removed. `reaches(.., Some(a)) == false` ⇔ `a` dominates `to`.
fn reaches(
    adj: &HashMap<NodeId, Vec<NodeId>>,
    from: NodeId,
    to: NodeId,
    skip: Option<NodeId>,
) -> bool {
    if Some(from) == skip {
        return false;
    }
    let mut seen: HashSet<NodeId> = HashSet::new();
    let mut q = VecDeque::from([from]);
    seen.insert(from);
    while let Some(n) = q.pop_front() {
        if n == to {
            return true;
        }
        for &d in adj.get(&n).map(Vec::as_slice).unwrap_or(&[]) {
            if Some(d) != skip && seen.insert(d) {
                q.push_back(d);
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authz_name_heuristic_matches_words_not_substrings() {
        for yes in [
            "authorize",
            "Authorized",
            "checkPermission",
            "check_acl",
            "RBACCheck",
            "hasRole",
            "IsAdmin",
            "requireAuth",
            "canAccess",
            "validateScope",
            "ensureOwner",
            "isAllowed",
            "WithAuthorization",
            "verifyEntitlements",
            "JWTUnaryValidator",
            "checkCapability",
            "validateJWT",
        ] {
            assert!(is_authz_name(yes), "{yes} should look like authz");
        }
        for no in [
            "cancel",
            "accessor",
            "rolling",
            "hash",
            "IsEmpty",
            "checkError",
            "requireNonNull",
            "GetRole",
            "basicConfig",
            "canonicalize",
            "withContext",
            "administer",
            // Accessors READ authz state; they never enforce it.
            "GetAuthzContext",
            "getPermissions",
            "ListRoles",
            "loadACL",
            "SetPermission",
            // Data-descriptor names carry authz values, they don't check them.
            "aclDeserializerFactory",
            "aclResolverFactory",
            "authzContext",
            "AuthorizationError",
            "permissionDao",
            "roleId",
            "AuthzCtx",
            "LoginSessionInfo",
            // Shaping/constructing authz data is not enforcing it.
            "NormalizeAuthzContextFromReqCtx",
            "toAuthzProto",
            "NewAuthorizer",
            "parsePermissions",
            // Mutating authz data is the gated ACTION, not the gate. A
            // validation-corpus review found this to be a recurring class of
            // false partials.
            "upsertRolesForNestedTenant",
            "updatePolicyGroupVersion",
            "ArchivePolicyGroupsByRuleIDs",
            "modifyPermissions",
            "UpdateServiceAccountPrivileges",
            "upsertExternalAppAuthenticationStatus",
            "sendAccessGrantSuccessOrFailureEvent",
            "grantRole",
            "revokePermissions",
            "assignRoleToUser",
            "enableAdminConsoleWithoutPermission",
            // Type/enum conversions and groupings name authz-adjacent data.
            "PermissionGroup",
            "ResourceWithPermissionGroups",
        ] {
            assert!(!is_authz_name(no), "{no} must NOT look like authz");
        }
    }

    #[test]
    fn invocation_requires_name_adjacent_parens() {
        // Real invocations of the named call.
        assert!(is_invocation("checkPermission(req)", "checkPermission"));
        assert!(is_invocation("svc.rbac.Enforce(ctx, sub)", "Enforce"));
        assert!(is_invocation("Enforce (ctx)", "Enforce"));
        // Field reads whose BASE chain carries the parens.
        assert!(!is_invocation("in.GetReqCtx().AuthzCtx", "AuthzCtx"));
        assert!(!is_invocation("in.GetReqCtx().AuthzCtx.Account", "Account"));
        // Composite literal with call-bearing element values.
        assert!(!is_invocation(
            "LoginSessionInfo{SessionID: req.GetLogin().GetSessionId()}",
            "LoginSessionInfo"
        ));
        // Word boundary: `checkPermission(` must not satisfy `Permission`.
        assert!(!is_invocation("checkPermission(req)", "Permission"));
    }
}
