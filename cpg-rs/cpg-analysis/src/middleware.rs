//! Router-level authorization census with middleware/interceptor mining.
//!
//! [`crate::authz::annotate_authz`] (task #54) answers the per-FINDING
//! question; its `None` verdict deliberately conflates two very different
//! situations: *no check exists* and *the check lives in framework
//! middleware the flow never touches*. This module lifts the question to
//! the ENTRY-POINT level and resolves that ambiguity by mining the
//! middleware registrations themselves.
//!
//! For every entry-point method the census assigns one verdict, strongest
//! evidence first:
//!
//! - `inline@<line>` — an authz-shaped invocation inside the handler body
//!   dominates the handler's final return (every execution completing
//!   normally passed the check).
//! - `wrapped@<name>` — the handler was registered wrapped in an
//!   authz-shaped call (`Handle("/x", requireAuth(handleFoo))`) — a
//!   route-level gate.
//! - `middleware@<name>` — a server/router-scope middleware chain
//!   registration ([`SCOPE_MW_CALLS`]: gRPC interceptor options, router
//!   `Use`) includes an authz-ENFORCING middleware function. NOTE: this
//!   tier is module-scope attribution — the census does not bind
//!   individual routes to individual router instances, so one enforcing
//!   chain in the graph annotates every remaining entry. Honest for the
//!   common one-server-per-module layout; read it as "a chain exists",
//!   not "this route provably sits behind it".
//! - `subject-gated@<line>` — a non-dominating authz invocation whose
//!   enforcement is parameterized by caller-supplied context, such as a
//!   caller-claims collection. Some systems delegate access decisions to
//!   the authenticated caller population when that collection is empty.
//!   Triage = verify the configured caller convention once, not per-site.
//!   Over-approximates: a caller-parameterized check that fails to dominate
//!   for an unrelated reason also lands here. The serialized label is kept
//!   for backward compatibility.
//! - `inline-partial@<line>` — an authz invocation exists in the body but
//!   does not dominate the final return (branch-only check, or check after
//!   the work). The authz-bypass shape — but note guard-clause-heavy
//!   handlers whose success path is not the lexically-last return can
//!   read as partial; advisory only.
//! - `none` — no evidence anywhere. Triage FIRST.
//!
//! A middleware function counts as ENFORCING when its name is authz-shaped
//! ([`crate::authz::is_authz_name`] or the pack's `authz` list) or its own
//! body contains an authz-shaped invocation. One recorded limit: a
//! middleware that delegates enforcement to a callee two hops down is
//! reported non-enforcing — the census under-claims, never over-claims.

use crate::authz::{authz_calls, is_authz_name};
use crate::pass::ast_descendants;
use cpg_core::{Cpg, NodeId, NodeKind, Query};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Server/router-scope middleware registration APIs: every function-valued
/// argument applies to all handlers behind that server/router. The gRPC
/// interceptor family plus the near-universal HTTP-router `Use` (chi, gin,
/// echo, gorilla, fiber all spell it that way). `Use` collides with
/// non-router APIs, so a call only counts when at least one argument
/// resolves to a defined function (see [`mine_middleware_gates`]).
const SCOPE_MW_CALLS: &[&str] = &[
    "UnaryInterceptor",
    "ChainUnaryInterceptor",
    "StreamInterceptor",
    "ChainStreamInterceptor",
    "WithUnaryServerChain",
    "WithStreamServerChain",
    // go-grpc-middleware's chain builders — the values they combine are all
    // server-scope interceptors even though the builder itself is then
    // passed to UnaryInterceptor(...).
    "ChainUnaryServer",
    "ChainStreamServer",
    "Use",
];

/// Configuration for product-neutral authorization-census conventions.
///
/// Caller-context markers are compared after removing non-alphanumeric
/// characters and folding ASCII case. This lets a phrase such as
/// `"caller claims"` match camelCase, snake_case, or concatenated spellings.
/// An empty marker list disables the caller-context classification. Framework
/// constructor names are exact call-name matches; an empty list disables
/// framework-construction evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthzCensusConfig {
    pub caller_context_markers: Vec<String>,
    pub framework_server_calls: Vec<String>,
}

impl Default for AuthzCensusConfig {
    fn default() -> Self {
        Self {
            caller_context_markers: vec!["subject context".into()],
            framework_server_calls: vec![
                "NewGRPCServer".into(),
                "NewWrappedGRPCServer".into(),
                "NewGRPCServerByConfig".into(),
            ],
        }
    }
}

/// A mined server/router-scope middleware registration.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MiddlewareGate {
    /// The middleware function's name (or the authz-shaped wrapper
    /// invocation's name when the function itself is not resolvable).
    pub name: String,
    /// The registration API it was passed to (`ChainUnaryInterceptor`, ...).
    pub scope: String,
    /// Line of the registration call.
    pub line: u32,
    /// Whether the middleware is authz-enforcing (name or body evidence).
    pub enforcing: bool,
    /// Path of the file containing the registration call — the anchor for
    /// binary-root binding (see [`binary_root`]).
    pub file: String,
    /// Receiver variable of the registration call (`authorized.Use(..)` ->
    /// `authorized`, from the member-call signature stamp) — the anchor for
    /// route-group binding (see [`group_gated_entries`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recv: Option<String>,
}

/// Continuation-invocation names inside middleware bodies: the wrapped
/// handler being called (gRPC unary/stream interceptor `handler(ctx, req)`,
/// HTTP middleware `next.ServeHTTP(w, r)` / `next(w, r)`, filter-chain
/// `proceed(...)`).
const CONTINUATION_CALLS: &[&str] = &["handler", "next", "ServeHTTP", "proceed", "invoke"];

/// Rejection constructors that are AUTH-specific: a middleware body that can
/// answer the request 401/Unauthenticated (`httperror.Unauthorized(..)`) or
/// gRPC PermissionDenied is enforcement-shaped even when neither its name nor
/// any call it makes carries authz vocabulary — signature and session
/// verifiers reject anonymously. `Forbidden` is deliberately absent: bare
/// 403s also come from operational gates (readonly/maintenance modes, CSRF,
/// site checks) and would launder them into authz credits (observed: a
/// migration readonly-mode blocker claiming a service's whole REST surface).
/// Consulted ONLY inside middleware-gate bodies ([`gate_body_enforces`]),
/// never for the inline tier, so a handler that merely returns Unauthorized
/// somewhere cannot claim an inline verdict through this list.
const REJECTION_CALLS: &[&str] = &["Unauthorized", "PermissionDenied", "Unauthenticated"];

/// Body evidence for a middleware gate, dominance-checked. Presence of an
/// authz-shaped call is not enough: a check reachable only inside a
/// method-allowlist branch (`if info.FullMethod == x { verifyRBAC(...) }`)
/// does not gate the OTHER methods. When a method in the factory's closure
/// set both checks and invokes its continuation, every continuation call
/// must be CFG-dominated by a check IN THAT METHOD; bodies with checks but
/// no recognizable continuation keep presence semantics (can't test — the
/// census under-strictens rather than inventing a dominance anchor).
fn gate_body_enforces(
    cpg: &Cpg,
    authz_names: &HashSet<String>,
    by_name: &HashMap<&str, Vec<NodeId>>,
    bindings: &HashMap<(&str, &str), &str>,
    factory: NodeId,
) -> bool {
    // The factory + every closure reachable through MethodRefs (same
    // traversal contract as `authz_calls`, but per-method granularity),
    // plus one extra edge kind: invoking a LOCAL VALUE bound from a defined
    // function (`verify := requestsignature.NewRequestVerifier(kp); ...`
    // `return verify(w, req)`)
    // chases into that function — the near-universal way an anonymous
    // middleware delegates its enforcement to a verifier built earlier in
    // the same file.
    let mut ref_index: Option<HashMap<(&str, u32), Vec<NodeId>>> = None;
    let mut visited: HashSet<NodeId> = HashSet::new();
    let mut work = vec![factory];
    let mut methods = Vec::new();
    while let Some(m) = work.pop() {
        if !visited.insert(m) {
            continue;
        }
        methods.push(m);
        let file = cpg.path_of(cpg.file_of(m)).unwrap_or("");
        for n in ast_descendants(cpg, m) {
            if cpg.kind_of(n) == NodeKind::MethodRef {
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
            if cpg.kind_of(n) == NodeKind::Call {
                if let Some(nm) = cpg.name_of(n) {
                    if let Some(bound) = bindings.get(&(file, nm)) {
                        if let Some(ms) = by_name.get(bound).filter(|ms| ms.len() <= 3) {
                            work.extend(ms.iter().copied());
                        }
                    }
                }
            }
        }
    }
    let mut any_check = false;
    for &m in &methods {
        let local_checks: Vec<NodeId> = ast_descendants(cpg, m)
            .into_iter()
            .filter(|&n| cpg.kind_of(n) == NodeKind::Call)
            .filter(|&n| {
                let (Some(nm), Some(code)) = (cpg.name_of(n), cpg.code_of(n)) else {
                    return false;
                };
                // `With*` is the derive-new-value convention
                // (`context.WithValue`, `WithElevatedRole`, `WithControlPlane`):
                // a middleware invoking one CONSTRUCTS authz context — often an
                // ELEVATION — and enforces nothing. The head can't be banned in
                // the global name shape because `WithOauth`-style route WRAPPERS
                // are legitimately authz-shaped; only a gate body's own
                // invocations carry this reading.
                if crate::authz::word_tokens(nm)
                    .first()
                    .is_some_and(|t| t == "with")
                {
                    return false;
                }
                (authz_names.contains(nm) || is_authz_name(nm) || REJECTION_CALLS.contains(&nm))
                    && crate::authz::is_invocation(code, nm)
            })
            .collect();
        if !local_checks.is_empty() {
            any_check = true;
        }
        let continuations: Vec<NodeId> = ast_descendants(cpg, m)
            .into_iter()
            .filter(|&n| cpg.kind_of(n) == NodeKind::Call)
            .filter(|&n| {
                cpg.name_of(n)
                    .is_some_and(|nm| CONTINUATION_CALLS.contains(&nm))
                    && cpg
                        .code_of(n)
                        .zip(cpg.name_of(n))
                        .is_some_and(|(c, nm)| crate::authz::is_invocation(c, nm))
            })
            .collect();
        if continuations.is_empty() {
            continue;
        }
        let adj = adjacency(&crate::cfg::cfg_edges_for_method(cpg, m));
        for &cont in &continuations {
            if !reaches(&adj, m, cont, None) {
                continue; // continuation not on the CFG — no evidence either way
            }
            let dominated = local_checks
                .iter()
                .any(|&chk| chk != cont && !reaches(&adj, m, cont, Some(chk)));
            if !dominated {
                return false; // a path invokes the handler without any check
            }
        }
    }
    any_check
}

/// The subtree an enforcing server-scope gate binds to. A gate wired in one
/// binary's setup must not claim entries served by OTHER binaries in the same
/// module (observed: one rbac interceptor in `broker-service/cmd/run.go`
/// flipping every service under `platform/` to `middleware@`). The binding
/// unit is the gate file's path up to a `/cmd/` segment when present (the Go
/// binary-root convention), else the gate file's directory — conservative:
/// a mis-rooted gate under-claims (`none`), never over-claims.
/// A file directly in `cmd/` (`cmd/run.go`) is the single-binary module
/// layout — the gate covers the module. A file in a `cmd/<bin>/` SUBDIR is
/// the multi-binary layout (`cmd/gateway/`, `cmd/eventdelivery/`, ... in one
/// module): the gate binds to its own binary's subtree only, else one
/// binary's auth gate claims every other binary's handlers.
pub(crate) fn binary_root(gate_file: &str) -> String {
    let per_binary = |cmd_end: usize| -> Option<String> {
        let rest = &gate_file[cmd_end..];
        rest.find('/')
            .map(|j| gate_file[..cmd_end + j + 1].to_string())
    };
    if let Some(rest) = gate_file.strip_prefix("cmd/") {
        let _ = rest;
        return per_binary(4).unwrap_or_default();
    }
    if let Some(i) = gate_file.find("/cmd/") {
        return per_binary(i + 5).unwrap_or_else(|| gate_file[..i + 1].to_string());
    }
    match gate_file.rfind('/') {
        Some(i) => gate_file[..i + 1].to_string(),
        None => String::new(),
    }
}

/// The census result: mined gates plus one `(entry, verdict)` row per
/// entry-point method, deterministically ordered.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuthzCensus {
    pub gates: Vec<MiddlewareGate>,
    pub rows: Vec<(String, String)>,
    /// The mined route table (URL/topic -> handler); additive column so
    /// `none` rows can be read as concrete unauthenticated paths.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<crate::entries::RouteEntry>,
}

impl AuthzCensus {
    /// Verdict-class counts as
    /// `(inline, wrapped, middleware, subject_gated, partial, none)`.
    pub fn counts(&self) -> (usize, usize, usize, usize, usize, usize) {
        let mut c = (0, 0, 0, 0, 0, 0);
        for (_, v) in &self.rows {
            if v.starts_with("inline@") {
                c.0 += 1;
            } else if v.starts_with("wrapped@") {
                c.1 += 1;
            } else if v.starts_with("middleware@") {
                c.2 += 1;
            } else if v.starts_with("subject-gated@") {
                c.3 += 1;
            } else if v.starts_with("inline-partial@") {
                c.4 += 1;
            } else {
                c.5 += 1;
            }
        }
        c
    }
}

/// Run the census over the entry set. `entries` (curated + registration-mined
/// names) are trusted verbatim, matched by full or simple method name.
/// `idl_entries` (bulk-mined rpc names from .proto/.thrift) mirror the taint
/// entry matcher's rule: a full-name match is qualified evidence and trusted;
/// a SIMPLE-name match keeps only methods passing the handler-shape gate
/// ([`crate::taint::looks_like_handler`]) — otherwise an rpc named `Get`
/// fills the census with every same-named utility in the module.
/// `authz_names` is the union of the pack rules' `authz` arrays.
pub fn authz_census(
    cpg: &Cpg,
    authz_names: &HashSet<String>,
    entries: &[String],
    idl_entries: &[String],
) -> AuthzCensus {
    authz_census_with_config(
        cpg,
        authz_names,
        entries,
        idl_entries,
        &AuthzCensusConfig::default(),
    )
}

/// Run the census with configurable caller-context and framework-constructor
/// conventions. See [`authz_census`] for entry matching and verdict semantics.
pub fn authz_census_with_config(
    cpg: &Cpg,
    authz_names: &HashSet<String>,
    entries: &[String],
    idl_entries: &[String],
    config: &AuthzCensusConfig,
) -> AuthzCensus {
    let by_name = methods_by_name(cpg);
    let bindings = local_value_bindings(cpg);
    let mut gates = mine_middleware_gates(cpg, authz_names, &by_name, &bindings);
    gates.extend(mine_framework_gates(cpg, &config.framework_server_calls));
    let wrapped = mine_route_wrappers(cpg, authz_names, &by_name);
    let group_gated = group_gated_entries(cpg, &gates, authz_names, &by_name, &bindings);
    // Binary-root binding is the lane for server-option gates (gRPC
    // interceptors) whose blast radius is not visible in the graph. A `Use`
    // or `Before` gate's blast radius IS visible — its receiver group/router
    // — so those gates bind only through the route-group lane; letting them
    // claim by binary root blanket-credits every handler in the binary
    // (observed: an event-delivery binary's auth gate claiming all gateway routes
    // in a multi-binary monorepo).
    let enforcing_gates: Vec<(String, String)> = gates
        .iter()
        .filter(|g| g.enforcing && g.scope != "Use" && g.scope != "Before")
        .map(|g| (g.name.clone(), binary_root(&g.file)))
        .collect();
    let verdict_evidence = EntryVerdictEvidence {
        wrapped: &wrapped,
        group_gated: &group_gated,
        enforcing_gates: &enforcing_gates,
        caller_context_markers: &config.caller_context_markers,
    };

    // Resolve each entry name once; BTreeMap keeps output deterministic and
    // collapses duplicate spellings of the same method.
    let mut rows: BTreeMap<String, String> = BTreeMap::new();
    let census_one = |e: &String, shape_gated: bool, rows: &mut BTreeMap<String, String>| {
        let methods = resolve_entry(cpg, &by_name, e);
        for m in methods {
            // For shape-gated (IDL) entries, only a SIMPLE-name match is
            // gated — an exact full-name match is qualified evidence, same
            // as the taint matcher's `qualified_guarded` tier.
            if shape_gated
                && cpg.full_name_of(m) != Some(e.as_str())
                && !crate::taint::looks_like_handler(cpg, m)
            {
                continue;
            }
            // A position-qualified entry (inline closure) keeps its own
            // spelling as the row key — closure full names are all `<anon>`
            // and would otherwise collapse into a single census row.
            let key = if crate::entries::parse_positional(e).is_some() {
                e.clone()
            } else {
                cpg.full_name_of(m)
                    .or(cpg.name_of(m))
                    .unwrap_or(e)
                    .to_string()
            };
            let verdict = entry_verdict(cpg, authz_names, &verdict_evidence, m, &key);
            rows.entry(key).or_insert(verdict);
        }
        // Unmatched entries are the coverage report's job.
    };
    for e in entries {
        census_one(e, false, &mut rows);
    }
    for e in idl_entries {
        census_one(e, true, &mut rows);
    }
    AuthzCensus {
        gates,
        rows: rows.into_iter().collect(),
        routes: crate::entries::mine_routes(cpg),
    }
}

/// Mine configured server-constructor call sites as non-enforcing
/// `framework`-scope gates: module-local evidence that the service surface is
/// framework-constructed, without claiming out-of-module wiring. Test/mock
/// files are skipped; deterministic order.
fn mine_framework_gates(cpg: &Cpg, framework_server_calls: &[String]) -> Vec<MiddlewareGate> {
    let mut gates = Vec::new();
    for c in cpg.calls() {
        let Some(name) = cpg.name_of(c) else { continue };
        if !framework_server_calls
            .iter()
            .any(|candidate| candidate == name)
        {
            continue;
        }
        if cpg
            .path_of(cpg.file_of(c))
            .is_some_and(crate::callgraph::is_test_path)
        {
            continue;
        }
        gates.push(MiddlewareGate {
            name: name.to_string(),
            scope: "framework".to_string(),
            line: cpg.line_of(c).unwrap_or(0),
            enforcing: false,
            file: cpg.path_of(cpg.file_of(c)).unwrap_or("").to_string(),
            recv: None,
        });
    }
    gates.sort_by(|a, b| (&a.name, a.line).cmp(&(&b.name, b.line)));
    gates
}

struct EntryVerdictEvidence<'a> {
    wrapped: &'a HashMap<String, String>,
    group_gated: &'a HashMap<String, String>,
    enforcing_gates: &'a [(String, String)],
    caller_context_markers: &'a [String],
}

fn entry_verdict(
    cpg: &Cpg,
    authz_names: &HashSet<String>,
    evidence: &EntryVerdictEvidence<'_>,
    m: NodeId,
    key: &str,
) -> String {
    let checks = authz_calls(cpg, authz_names, m);
    let partial_line = checks.first().and_then(|&a| cpg.line_of(a));
    if let Some(line) = dominating_check(cpg, m, &checks) {
        return format!("inline@{line}");
    }
    let simple = cpg.name_of(m).unwrap_or(key);
    if let Some(w) = evidence
        .wrapped
        .get(key)
        .or_else(|| evidence.wrapped.get(simple))
    {
        return format!("wrapped@{w}");
    }
    // Route-group binding: the entry was registered on a router group whose
    // middleware chain (its own or an ancestor group's) carries an enforcing
    // authz gate.
    if let Some(g) = evidence
        .group_gated
        .get(key)
        .or_else(|| evidence.group_gated.get(simple))
    {
        return format!("middleware@{g}");
    }
    // Binary-root binding: an enforcing gate claims this entry only when the
    // entry's file sits under the gate's binary root (see [`binary_root`]).
    let entry_file = cpg.path_of(cpg.file_of(m)).unwrap_or("");
    if let Some((g, _)) = evidence
        .enforcing_gates
        .iter()
        .find(|(_, root)| entry_file.starts_with(root.as_str()))
    {
        return format!("middleware@{g}");
    }
    if partial_line.is_some() {
        if let Some(l) = caller_context_gated_line(cpg, m, &checks, evidence.caller_context_markers)
        {
            return format!("subject-gated@{l}");
        }
    }
    match partial_line {
        Some(l) => format!("inline-partial@{l}"),
        None => "none".to_string(),
    }
}

/// Is the non-dominating check governed by a configured caller-context
/// convention rather than a bypass? Evidence tiers: (1) a check whose own
/// code carries a configured marker; (2) the handler pulls a marked value
/// anywhere, even when a local rename hides the marker from the check call.
fn caller_context_gated_line(
    cpg: &Cpg,
    m: NodeId,
    checks: &[NodeId],
    caller_context_markers: &[String],
) -> Option<u32> {
    let normalized_markers: Vec<String> = caller_context_markers
        .iter()
        .map(|marker| normalize_marker(marker))
        .filter(|marker| !marker.is_empty())
        .collect();
    let mentions = |candidate: &str| mentions_marker(candidate, &normalized_markers);
    if let Some(&a) = checks
        .iter()
        .find(|&&a| cpg.code_of(a).is_some_and(&mentions))
    {
        return cpg.line_of(a);
    }
    let pulls_context = ast_descendants(cpg, m)
        .into_iter()
        .any(|n| cpg.name_of(n).is_some_and(&mentions));
    if pulls_context {
        checks.first().and_then(|&a| cpg.line_of(a))
    } else {
        None
    }
}

fn normalize_marker(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Match within one identifier-like token at a time. Normalizing an entire
/// expression would incorrectly join unrelated arguments across punctuation:
/// `enforce(subject, context)` is not a `subject_context` identifier.
fn mentions_marker(value: &str, normalized_markers: &[String]) -> bool {
    value
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .map(normalize_marker)
        .filter(|token| !token.is_empty())
        .any(|token| {
            normalized_markers
                .iter()
                .any(|marker| token.contains(marker))
        })
}

/// Does any of `checks` dominate the handler's normal completion? Anchor =
/// the lexically-last `Return` in the method (the success return); handlers
/// with no return statement (`func(w, r)` HTTP style) fall back to
/// requiring domination of every CFG exit. Returns the check's line.
fn dominating_check(cpg: &Cpg, method: NodeId, checks: &[NodeId]) -> Option<u32> {
    if checks.is_empty() {
        return None;
    }
    let edges = crate::cfg::cfg_edges_for_method(cpg, method);
    let adj = adjacency(&edges);
    let last_return = ast_descendants(cpg, method)
        .into_iter()
        .filter(|&n| cpg.kind_of(n) == NodeKind::Return)
        .max_by_key(|&n| cpg.line_of(n).unwrap_or(0));
    let targets: Vec<NodeId> = match last_return {
        Some(r) => vec![r],
        None => {
            // Every CFG node with no successor is an exit.
            let mut nodes: HashSet<NodeId> = HashSet::new();
            for &(s, d) in &edges {
                nodes.insert(s);
                nodes.insert(d);
            }
            nodes
                .into_iter()
                .filter(|n| adj.get(n).is_none_or(Vec::is_empty))
                .collect()
        }
    };
    if targets.is_empty() {
        return None;
    }
    for &a in checks {
        let reachable_targets: Vec<NodeId> = targets
            .iter()
            .copied()
            .filter(|&t| t != a && reaches(&adj, method, t, None))
            .collect();
        if reachable_targets.is_empty() {
            continue; // no CFG evidence either way
        }
        if reachable_targets
            .iter()
            .all(|&t| !reaches(&adj, method, t, Some(a)))
        {
            return cpg.line_of(a);
        }
    }
    None
}

/// Mine server/router-scope middleware registrations ([`SCOPE_MW_CALLS`]).
///
/// The common framework shape builds the chain in a local slice first:
/// ```go
/// unaries := []grpc.UnaryServerInterceptor{tracing()}
/// unaries = append(unaries, auth.JWTUnaryValidator(tls))
/// return grpc.UnaryInterceptor(middleware.ChainUnaryServer(unaries...))
/// ```
/// so an identifier argument that resolves to no function is chased one
/// level through same-method `append(<ident>, ...)` calls and the slice's
/// own initializer (a composite literal lowers to a type-named Call whose
/// elements `function_refs` already descends into).
fn mine_middleware_gates(
    cpg: &Cpg,
    authz_names: &HashSet<String>,
    by_name: &HashMap<&str, Vec<NodeId>>,
    bindings: &HashMap<(&str, &str), &str>,
) -> Vec<MiddlewareGate> {
    let mut gates = Vec::new();
    for c in cpg.calls() {
        let Some(scope) = cpg.name_of(c) else {
            continue;
        };
        // `Before` is the pre-routing hook of builder-style middleware
        // chains (`router.GlobalMiddleware().Before(verify)` — the chain
        // runs before dispatch, so a committing Before handler gates every
        // route on that router). The bare name collides with time
        // comparisons everywhere, so it only counts when the receiver text
        // shows a middleware-chain accessor. Before gates bind through the
        // route-group lane exclusively (see [`authz_census`]): their blast
        // radius is their receiver router, never the whole binary.
        let chained_before = scope == "Before"
            && cpg.code_of(c).is_some_and(|code| {
                code.contains("Middleware().") || code.contains("middleware().")
            });
        if !SCOPE_MW_CALLS.contains(&scope) && !chained_before {
            continue;
        }
        let line = cpg.line_of(c).unwrap_or(0);
        let file = cpg.path_of(cpg.file_of(c)).unwrap_or("").to_string();
        // The signature stamp carries the root receiver identifier; for a
        // chained receiver it can be absent, so fall back to the leading
        // identifier of the call text (`r.GlobalMiddleware().Before(..)` ->
        // `r`).
        let recv = cpg.signature_of(c).map(str::to_string).or_else(|| {
            let code = cpg.code_of(c)?;
            let head: String = code
                .chars()
                .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
                .collect();
            (!head.is_empty() && code[head.len()..].starts_with('.')).then_some(head)
        });
        for a in cpg.arguments_of(c) {
            let mut refs = function_refs(cpg, by_name, a, true);
            if refs.is_empty() && cpg.kind_of(a) == NodeKind::Identifier {
                refs = chain_variable_refs(cpg, by_name, c, a);
            }
            for r in refs {
                let enforcing = authz_names.contains(&r.name)
                    || is_authz_name(&r.name)
                    || crate::authz::is_authz_qualified(&r.spelling)
                    || r.methods
                        .iter()
                        .any(|&m| gate_body_enforces(cpg, authz_names, by_name, bindings, m));
                // An enforcing anonymous middleware reads as `<anon>` in the
                // census; name it after the local verifier its body invokes
                // (`verify := requestsignature.NewRequestVerifier(..)` ->
                // `NewRequestVerifier`).
                let name = if enforcing && (r.name.is_empty() || r.name.starts_with('<')) {
                    binding_alias(cpg, bindings, &r.methods).unwrap_or(r.name)
                } else {
                    r.name
                };
                gates.push(MiddlewareGate {
                    name,
                    scope: scope.to_string(),
                    line,
                    enforcing,
                    file: file.clone(),
                    recv: recv.clone(),
                });
            }
        }
    }
    gates.sort_by(|a, b| (&a.name, a.line).cmp(&(&b.name, b.line)));
    gates.dedup_by(|a, b| a.name == b.name && a.line == b.line);
    gates
}

/// The name of the first local-value binding a set of methods invokes —
/// the display alias for an enforcing anonymous middleware.
fn binding_alias(
    cpg: &Cpg,
    bindings: &HashMap<(&str, &str), &str>,
    methods: &[NodeId],
) -> Option<String> {
    for &m in methods {
        let file = cpg.path_of(cpg.file_of(m)).unwrap_or("");
        for n in ast_descendants(cpg, m) {
            if cpg.kind_of(n) != NodeKind::Call {
                continue;
            }
            if let Some(nm) = cpg.name_of(n) {
                if let Some(bound) = bindings.get(&(file, nm)) {
                    return Some((*bound).to_string());
                }
            }
        }
    }
    None
}

/// (file, local identifier) -> the callee NAME its value was bound from:
/// `verify := requestsignature.NewRequestVerifier(kp)` yields
/// `("main.go", "verify") -> "NewRequestVerifier"`. Fuel for the
/// local-value-invocation hop in [`gate_body_enforces`] and the anonymous-gate
/// alias.
fn local_value_bindings(cpg: &Cpg) -> HashMap<(&str, &str), &str> {
    let mut bindings: HashMap<(&str, &str), &str> = HashMap::new();
    for c in cpg.calls() {
        if cpg.name_of(c) != Some("=") {
            continue;
        }
        let args = cpg.arguments_of(c);
        if args.len() != 2
            || cpg.kind_of(args[0]) != NodeKind::Identifier
            || cpg.kind_of(args[1]) != NodeKind::Call
        {
            continue;
        }
        let (Some(lhs), Some(rhs)) = (cpg.name_of(args[0]), cpg.name_of(args[1])) else {
            continue;
        };
        if rhs == "=" || rhs == "append" {
            continue;
        }
        if let Some(f) = cpg.path_of(cpg.file_of(c)) {
            bindings.entry((f, lhs)).or_insert(rhs);
        }
    }
    bindings
}

/// Route-group gate binding: Gin/echo/chi-style routers wire authz
/// middleware per GROUP, not per server —
/// ```go
/// authorized := base.Group("/")
/// authorized.Use(authMiddleware.Authorize())
/// authorized.GET("/users", userController.GetUsers)
/// ```
/// — so the gate's blast radius is its receiver variable plus every group
/// derived from it, and the entries it claims are the handlers REGISTERED on
/// those receivers (the registration site carries the binding, wherever the
/// handler's body lives). Group variables are file-local; the ONE sanctioned
/// way a binding leaves its file is mount forwarding: passing the group as a
/// call argument moves the binding onto the callee's parameter (this is how
/// oapi-codegen surfaces wire up — `RegisterHandlers(group, si)` does the
/// actual verb registrations on its router PARAM, one or two call hops from
/// where the gate was attached). Returns entry spelling -> enforcing gate
/// name.
fn group_gated_entries(
    cpg: &Cpg,
    gates: &[MiddlewareGate],
    authz_names: &HashSet<String>,
    by_name: &HashMap<&str, Vec<NodeId>>,
    bindings: &HashMap<(&str, &str), &str>,
) -> HashMap<String, String> {
    // Group derivations: `child := parent.Group(..)` lowers to a `=` call
    // whose lhs is the child identifier and whose rhs is a `Group` call
    // stamped with the parent receiver. (file, child) -> parent.
    let mut parent: HashMap<(&str, &str), &str> = HashMap::new();
    for c in cpg.calls() {
        if cpg.name_of(c) != Some("=") {
            continue;
        }
        let args = cpg.arguments_of(c);
        if args.len() != 2
            || cpg.kind_of(args[0]) != NodeKind::Identifier
            || cpg.name_of(args[1]) != Some("Group")
        {
            continue;
        }
        let (Some(child), Some(par)) = (cpg.name_of(args[0]), cpg.signature_of(args[1])) else {
            continue;
        };
        if let Some(file) = cpg.path_of(cpg.file_of(c)) {
            parent.entry((file, child)).or_insert(par);
        }
    }
    // (file, receiver) -> enforcing Use/Before-gate name. Named gates are
    // seated first: when a router carries both a named auth gate and an
    // enforcing anonymous one (an IP-allowlist closure next to a SAML
    // middleware), the row should read the recognizable name.
    let mut gate_by: HashMap<(&str, &str), String> = HashMap::new();
    for anon_pass in [false, true] {
        for g in gates {
            if (g.scope == "Use" || g.scope == "Before")
                && g.enforcing
                && (g.name.starts_with('<') || g.name.is_empty()) == anon_pass
            {
                if let Some(recv) = &g.recv {
                    gate_by
                        .entry((g.file.as_str(), recv.as_str()))
                        .or_insert_with(|| g.name.clone());
                }
            }
        }
    }
    // Mount arg-gates: `Register(group, handler, auth.Authorize())` — a call
    // that receives a route group ALONGSIDE enforcing authz-named middleware
    // values gates that group, exactly like `group.Use(auth.Authorize())`.
    // This is the typed-facade idiom (the mount function forwards the
    // middleware to the framework's option struct). The group argument must
    // already be group-shaped — derived via `Group(..)` or already gated —
    // so an authz-named argument sailing past an unrelated identifier never
    // binds. Bare-identifier middleware args are deliberately NOT harvested
    // here (only invocations/member-values): an authz-NAMED plain identifier
    // sibling is more often a service handle than a middleware value.
    for c in cpg.calls() {
        let Some(cname) = cpg.name_of(c) else {
            continue;
        };
        if cname == "=" || cname == "Group" {
            continue;
        }
        let args = cpg.arguments_of(c);
        if args.len() < 2 {
            continue;
        }
        let Some(file) = cpg.path_of(cpg.file_of(c)) else {
            continue;
        };
        let Some(target) = args.iter().find_map(|&a| {
            if cpg.kind_of(a) != NodeKind::Identifier {
                return None;
            }
            cpg.name_of(a)
                .filter(|n| parent.contains_key(&(file, *n)) || gate_by.contains_key(&(file, *n)))
        }) else {
            continue;
        };
        for &a in &args {
            if cpg.kind_of(a) == NodeKind::Identifier {
                continue;
            }
            for r in function_refs(cpg, by_name, a, true) {
                let enforcing = authz_names.contains(&r.name)
                    || is_authz_name(&r.name)
                    || crate::authz::is_authz_qualified(&r.spelling)
                    || r.methods
                        .iter()
                        .any(|&m| gate_body_enforces(cpg, authz_names, by_name, bindings, m));
                if enforcing {
                    gate_by.entry((file, target)).or_insert(r.name);
                    break;
                }
            }
        }
    }
    if gate_by.is_empty() {
        return HashMap::new();
    }
    // Walk the derivation chain up from a receiver, visited-bounded (a
    // `g := g.Group(..)` rebind must not spin), to its gate if any.
    let chain_gate = |gate_by: &HashMap<(&str, &str), String>, file: &str, name: &str| {
        let mut cur = name.to_string();
        let mut seen: HashSet<String> = HashSet::new();
        while seen.insert(cur.clone()) {
            if let Some(g) = gate_by.get(&(file, cur.as_str())) {
                return Some(g.clone());
            }
            match parent.get(&(file, cur.as_str())) {
                Some(p) => cur = p.to_string(),
                None => break,
            }
        }
        None
    };
    // Mount forwarding to a fixpoint: every call that passes a gated (or
    // gated-via-derivation) group as an argument binds the same gate onto
    // the matching PARAMETER name in the callee's file. Callee resolution
    // uses the same simple-name map and ambiguity cap as function refs, so
    // an overloaded name binds at most a handful of candidate params — and a
    // binding onto a file that performs no registrations is inert.
    loop {
        let mut additions: Vec<((&str, &str), String)> = Vec::new();
        for c in cpg.calls() {
            let Some(cname) = cpg.name_of(c) else {
                continue;
            };
            if cname == "=" || cname == "Group" {
                continue;
            }
            let Some(file) = cpg.path_of(cpg.file_of(c)) else {
                continue;
            };
            // Gate-bound identifier arguments first — they are rare, and
            // callee resolution below is only worth doing when one exists.
            let bound: Vec<(usize, String)> = cpg
                .arguments_of(c)
                .iter()
                .enumerate()
                .filter(|&(_, &a)| cpg.kind_of(a) == NodeKind::Identifier)
                .filter_map(|(i, &a)| {
                    let an = cpg.name_of(a)?;
                    Some((i, chain_gate(&gate_by, file, an)?))
                })
                .collect();
            if bound.is_empty() {
                continue;
            }
            let Some(all) = by_name.get(cname) else {
                continue;
            };
            // Forwarding a route group only registers through a ROUTER-TYPED
            // parameter, so candidates whose matching param is anything else
            // (`Register(worker worker.Worker, ..)`) never bind. Among the
            // rest, same-named callees in different packages (every
            // oapi-codegen package defines `RegisterHandlersWithOptions`)
            // are broken by path-segment affinity with the CALLING file: the
            // caller's own neighborhood is the package it imports. A wrong
            // pick here is a wrong GATE CREDIT (observed: an event-delivery
            // binary's auth gate claiming the gateway's generated v2
            // surface), so only best-affinity candidates bind, and only when
            // few remain.
            for (i, g) in bound {
                let mut callees: Vec<NodeId> = all
                    .iter()
                    .copied()
                    .filter(|&m| {
                        cpg.parameters_of(m).get(i).is_some_and(|&p| {
                            cpg.type_full_name_of(p)
                                .is_some_and(crate::entries::is_router_type)
                        })
                    })
                    .collect();
                if callees.len() > 1 {
                    let affinity = |m: &NodeId| {
                        cpg.path_of(cpg.file_of(*m)).map_or(0, |p| {
                            let caller: HashSet<&str> = file.split('/').collect();
                            p.split('/').filter(|s| caller.contains(s)).count()
                        })
                    };
                    let best = callees.iter().map(affinity).max().unwrap_or(0);
                    callees.retain(|m| affinity(m) == best);
                }
                if callees.len() > 3 {
                    continue;
                }
                for m in callees {
                    let Some(&p) = cpg.parameters_of(m).get(i) else {
                        continue;
                    };
                    let Some(pn) = cpg.name_of(p) else { continue };
                    let Some(pf) = cpg.path_of(cpg.file_of(m)) else {
                        continue;
                    };
                    if !gate_by.contains_key(&(pf, pn)) {
                        additions.push(((pf, pn), g.clone()));
                    }
                }
            }
        }
        let mut changed = false;
        for (k, g) in additions {
            if let std::collections::hash_map::Entry::Vacant(e) = gate_by.entry(k) {
                e.insert(g);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut out = HashMap::new();
    for reg in crate::entries::mine_registrations(cpg) {
        let Some(recv) = reg.recv else { continue };
        let Some(file) = cpg.path_of(reg.file) else {
            continue;
        };
        if let Some(g) = chain_gate(&gate_by, file, &recv) {
            for e in &reg.entries {
                out.entry(e.clone()).or_insert_with(|| g.clone());
            }
        }
    }
    out
}

/// Chase a chain-slice identifier through its enclosing method: every
/// `append(<ident>, x...)` contributes its non-first arguments, and every
/// assignment `<ident> = rhs` / `<ident> := rhs` contributes its rhs (the
/// composite-literal initializer). One level only, by design.
fn chain_variable_refs(
    cpg: &Cpg,
    by_name: &HashMap<&str, Vec<NodeId>>,
    gate_call: NodeId,
    ident: NodeId,
) -> Vec<FnRef> {
    let Some(var) = cpg.name_of(ident) else {
        return Vec::new();
    };
    let Some(method) = enclosing_method(cpg, gate_call) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for n in ast_descendants(cpg, method) {
        if cpg.kind_of(n) != NodeKind::Call {
            continue;
        }
        match cpg.name_of(n) {
            Some("append") => {
                let args = cpg.arguments_of(n);
                if args.first().is_some_and(|&f| cpg.name_of(f) == Some(var)) {
                    for &x in &args[1..] {
                        out.extend(function_refs(cpg, by_name, x, true));
                    }
                }
            }
            Some("=") => {
                let args = cpg.arguments_of(n);
                if args.len() == 2 && cpg.name_of(args[0]) == Some(var) {
                    // Skip the self-append form (already harvested above).
                    if cpg.name_of(args[1]) != Some("append") {
                        out.extend(function_refs(cpg, by_name, args[1], true));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The method whose AST subtree contains `n` (same-file line-window scan —
/// gate calls are rare enough that the per-call cost is irrelevant).
fn enclosing_method(cpg: &Cpg, n: NodeId) -> Option<NodeId> {
    cpg.methods()
        .into_iter()
        .find(|&m| ast_descendants(cpg, m).contains(&n))
}

/// Mine route-level authz wrappers at handler registration sites:
/// `Handle("/x", requireAuth(handleFoo))` maps every method `handleFoo`
/// resolves to onto wrapper name `requireAuth`. Keyed by full AND simple
/// method name.
fn mine_route_wrappers(
    cpg: &Cpg,
    authz_names: &HashSet<String>,
    by_name: &HashMap<&str, Vec<NodeId>>,
) -> HashMap<String, String> {
    let mut wrapped = HashMap::new();
    for c in cpg.calls() {
        let Some(name) = cpg.name_of(c) else { continue };
        let strong = crate::entries::STRONG_CALLS.contains(&name);
        if !strong && !crate::entries::VERB_CALLS.contains(&name) {
            continue;
        }
        let args = cpg.arguments_of(c);
        if !strong
            && !args
                .first()
                .is_some_and(|&a| crate::entries::is_route_literal(cpg, a))
            && !crate::entries::receiver_is_router(cpg, c)
        {
            continue;
        }
        // Sibling-guard idiom (martini/express middleware-chain args):
        // `r.Get("/dash", PageRequireUserAuth, HTTPDashboard)` — an
        // authz-NAMED function VALUE among the arguments guards every
        // co-registered handler in the same call.
        let mut sibling_guard: Option<String> = None;
        if args.len() > 2 {
            for &a in &args {
                for r in function_refs(cpg, by_name, a, false) {
                    if !r.methods.is_empty()
                        && (authz_names.contains(&r.name) || is_authz_name(&r.name))
                    {
                        sibling_guard = Some(r.name.clone());
                        break;
                    }
                }
                if sibling_guard.is_some() {
                    break;
                }
            }
        }
        for &a in &args {
            if let Some(g) = &sibling_guard {
                for r in function_refs(cpg, by_name, a, false) {
                    if authz_names.contains(&r.name) || is_authz_name(&r.name) {
                        continue; // the guard itself is not a wrapped handler
                    }
                    for &m in &r.methods {
                        record_wrapped(cpg, &mut wrapped, m, g);
                    }
                }
            }
            // Only invocation-shaped arguments can be wrappers — and the
            // parens must belong to the wrapper name itself, not to a call
            // buried in a field-read base or literal element.
            if cpg.kind_of(a) != NodeKind::Call {
                continue;
            }
            let Some(w) = cpg.name_of(a) else { continue };
            if !crate::authz::is_invocation(cpg.code_of(a).unwrap_or(""), w) {
                continue;
            }
            if !(authz_names.contains(w) || is_authz_name(w)) {
                continue;
            }
            for inner in cpg.arguments_of(a) {
                for r in function_refs(cpg, by_name, inner, false) {
                    for &m in &r.methods {
                        record_wrapped(cpg, &mut wrapped, m, w);
                    }
                }
            }
        }
    }
    wrapped
}

/// Record a wrapped handler under its full and simple names — EXCEPT
/// anonymous ones, which all share the `<anon>` spelling: keying those by
/// name lets one wrapped closure claim every anonymous entry in the module
/// (observed: a pprof mux closure reading `wrapped@WithOauth` from an
/// unrelated file). Anons key positionally instead, matching the census row
/// spelling for position-qualified entries.
fn record_wrapped(cpg: &Cpg, wrapped: &mut HashMap<String, String>, m: NodeId, w: &str) {
    let anon = |s: &str| s.starts_with('<') || s.is_empty();
    let mut keyed = false;
    if let Some(fnm) = cpg.full_name_of(m) {
        if !anon(fnm) {
            wrapped.insert(fnm.to_string(), w.to_string());
            keyed = true;
        }
    }
    if let Some(snm) = cpg.name_of(m) {
        if !anon(snm) {
            wrapped.insert(snm.to_string(), w.to_string());
        } else if !keyed {
            if let (Some(f), Some(l)) = (cpg.path_of(cpg.file_of(m)), cpg.line_of(m)) {
                wrapped.insert(format!("{snm}@{f}:{l}"), w.to_string());
            }
        }
    }
}

/// A named function value resolved from a middleware-registration argument:
/// the simple name, the QUALIFIED spelling as written at the call site
/// (`rbac.UnaryServerInterceptor` — package/receiver qualifiers carry authz
/// vocabulary the simple name lacks), and the defined methods it resolves to.
pub(crate) struct FnRef {
    pub name: String,
    pub spelling: String,
    pub methods: Vec<NodeId>,
}

/// Resolve a node to named function values: identifiers, `MethodRef`s
/// (lambdas in expression position), member-value reads (`s.mw`, a Call with
/// no `(` in its code), and — when `descend` — one level into a wrapping
/// invocation (`Use(NewAuthMW(cfg))`, whose invocation name is also
/// reported so name-authz-shaped constructors still count).
fn function_refs(
    cpg: &Cpg,
    by_name: &HashMap<&str, Vec<NodeId>>,
    n: NodeId,
    descend: bool,
) -> Vec<FnRef> {
    const MAX_MATCHES: usize = 3;
    let mut out = Vec::new();
    let named = |name: &str, spelling: &str, out: &mut Vec<FnRef>| {
        let methods = by_name
            .get(name)
            .filter(|ms| ms.len() <= MAX_MATCHES)
            .cloned()
            .unwrap_or_default();
        if !methods.is_empty() || is_authz_name(name) || crate::authz::is_authz_qualified(spelling)
        {
            out.push(FnRef {
                name: name.to_string(),
                spelling: spelling.to_string(),
                methods,
            });
        }
    };
    match cpg.kind_of(n) {
        NodeKind::Identifier | NodeKind::MethodRef => {
            if let Some(name) = cpg.name_of(n) {
                // A MethodRef names exactly ONE method — resolve it by
                // (file, name, line) so anonymous closures (`<anon>`,
                // useless in the simple-name map) still carry their body
                // into enforcement checks. All three keys: same-line anons
                // in DIFFERENT files collide on (name, line) alone.
                if cpg.kind_of(n) == NodeKind::MethodRef {
                    let ln = cpg.line_of(n);
                    let fl = cpg.file_of(n);
                    let methods: Vec<NodeId> = cpg
                        .methods()
                        .into_iter()
                        .filter(|&m| {
                            cpg.name_of(m) == Some(name)
                                && cpg.line_of(m) == ln
                                && cpg.file_of(m) == fl
                        })
                        .collect();
                    if !methods.is_empty() {
                        out.push(FnRef {
                            name: name.to_string(),
                            spelling: cpg.code_of(n).unwrap_or(name).to_string(),
                            methods,
                        });
                        return out;
                    }
                }
                named(name, cpg.code_of(n).unwrap_or(name), &mut out);
            }
        }
        NodeKind::Call => {
            let code = cpg.code_of(n).unwrap_or("");
            if !code.contains('(') {
                if let Some(name) = cpg.name_of(n) {
                    named(name, code, &mut out);
                }
            } else if descend {
                // The invocation's own name counts (RequireAuth() built the
                // middleware), and so do function refs among its arguments.
                // The spelling is the call text up to the argument list.
                if let Some(name) = cpg.name_of(n) {
                    let spelling = code.split('(').next().unwrap_or(name).trim_end();
                    named(name, spelling, &mut out);
                }
                for a in cpg.arguments_of(n) {
                    out.extend(function_refs(cpg, by_name, a, false));
                }
            }
        }
        _ => {}
    }
    out
}

/// Simple name -> defined methods, test/mock/fake files excluded (same
/// convention as entry mining and call-graph test-double demotion).
fn methods_by_name(cpg: &Cpg) -> HashMap<&str, Vec<NodeId>> {
    let mut by_name: HashMap<&str, Vec<NodeId>> = HashMap::new();
    for m in cpg.methods() {
        if cpg
            .path_of(cpg.file_of(m))
            .is_some_and(crate::callgraph::is_test_path)
        {
            continue;
        }
        if let Some(n) = cpg.name_of(m) {
            by_name.entry(n).or_default().push(m);
        }
    }
    by_name
}

/// Entry-string resolution: position-qualified (`name@file:line`, how inline
/// closures are mined) first, then full name, then simple name as fallback —
/// the same rule the taint query and the coverage report use.
fn resolve_entry(cpg: &Cpg, by_name: &HashMap<&str, Vec<NodeId>>, entry: &str) -> Vec<NodeId> {
    if let Some((name, file, line)) = crate::entries::parse_positional(entry) {
        return cpg
            .methods()
            .into_iter()
            .filter(|&m| {
                cpg.name_of(m) == Some(name)
                    && cpg.line_of(m) == Some(line)
                    && cpg.path_of(cpg.file_of(m)) == Some(file)
            })
            .collect();
    }
    let full: Vec<NodeId> = cpg
        .methods()
        .into_iter()
        .filter(|&m| cpg.full_name_of(m) == Some(entry))
        .collect();
    if !full.is_empty() {
        return full;
    }
    by_name.get(entry).cloned().unwrap_or_default()
}

fn adjacency(edges: &[(NodeId, NodeId)]) -> HashMap<NodeId, Vec<NodeId>> {
    let mut adj: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for &(s, d) in edges {
        adj.entry(s).or_default().push(d);
    }
    adj
}

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
    let mut q = std::collections::VecDeque::from([from]);
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
    use cpg_core::Cpg;
    use cpg_frontend::Frontend;

    fn build_go(files: &[(&str, &str)]) -> Cpg {
        let mut cpg = Cpg::new();
        let mut fe = cpg_lang_ts::TsFrontend::go();
        let mut fids = Vec::new();
        for (path, src) in files {
            fids.push(fe.build_file(&mut cpg, path, src).file);
        }
        let pm = crate::standard_pipeline();
        let idx = crate::pass::method_name_index(&cpg);
        let ctx = crate::pass::PassContext {
            methods_by_name: Some(&idx),
        };
        pm.run_all(&mut cpg, &fids, &ctx);
        cpg
    }

    fn verdict_of<'a>(census: &'a AuthzCensus, needle: &str) -> &'a str {
        census
            .rows
            .iter()
            .find(|(e, _)| e.contains(needle))
            .map(|(_, v)| v.as_str())
            .unwrap_or_else(|| panic!("entry {needle} missing from census: {:?}", census.rows))
    }

    #[test]
    fn census_route_group_gate_binds_registrations() {
        // Gin group idiom: the authz middleware is wired per GROUP; handlers
        // registered on the gated group (or a group derived from it) are
        // middleware-gated, handlers on ungated sibling groups are not.
        let cpg = build_go(&[(
            "router.go",
            r#"package main

func Router(authMiddleware *AuthMiddleware, userController *UserController, statusController *StatusController) {
	router := gin.New()
	base := router.Group("/v1/")
	authorized := base.Group("/")
	authorized.Use(authMiddleware.Authorize())
	derived := authorized.Group("/deep/")
	authorized.GET("/users", userController.GetUsers)
	derived.GET("/tokens", userController.ListTokens)
	open := router.Group("/pub/")
	open.GET("/status", statusController.GetStatus)
}

func (u *UserController) GetUsers(c *GinContext)   { listUsers(c) }
func (u *UserController) ListTokens(c *GinContext) { listTokens(c) }
func (s *StatusController) GetStatus(c *GinContext) { render(c) }
func (a *AuthMiddleware) Authorize() func(c *GinContext) {
	return func(c *GinContext) {
		if !verifyToken(c) {
			c.Abort()
			return
		}
		c.Next()
	}
}
"#,
        )]);
        let none = HashSet::new();
        let entries = crate::entries::mine_registration_entries(&cpg);
        let census = authz_census(&cpg, &none, &entries, &[]);
        assert_eq!(
            verdict_of(&census, "GetUsers"),
            "middleware@Authorize",
            "{census:?}"
        );
        assert_eq!(
            verdict_of(&census, "ListTokens"),
            "middleware@Authorize",
            "derived group inherits the gate: {census:?}"
        );
        assert_eq!(
            verdict_of(&census, "GetStatus"),
            "none",
            "ungated sibling group must NOT be claimed: {census:?}"
        );
        // The route table joins URL -> handler for every registration.
        let users = census
            .routes
            .iter()
            .find(|r| r.handler.contains("GetUsers"))
            .expect("route row for GetUsers");
        assert_eq!(
            (users.route.as_str(), users.verb.as_str()),
            ("/users", "GET")
        );
        assert!(
            census.routes.iter().any(|r| r.route == "/status"),
            "{:?}",
            census.routes
        );
    }

    #[test]
    fn census_mount_facade_gate_forwards_to_generated_registrations() {
        // The oapi-codegen mount idiom: authz middleware is handed to a
        // typed mount facade ALONGSIDE the group (`Register(apiv2, handler,
        // auth.Authorize())`), and the actual verb registrations happen on a
        // router PARAMETER two call hops away, in a generated file. The
        // arg-gate must bind to the group, and mount forwarding must carry
        // the binding across both hops to the registration site.
        let cpg = build_go(&[
            (
                "router/router.go",
                r#"package router

func Router(v2Auth *AuthMiddleware, ssImpl *RequestHandler, statusController *StatusController) {
	router := gin.New()
	apiv2 := router.Group("/v2/")
	httpx.Register(apiv2, NewStrictHandler(ssImpl), v2Auth.Authorize())
	open := router.Group("/pub/")
	open.GET("/status", statusController.GetStatus)
}

func (s *StatusController) GetStatus(c *GinContext) { render(c) }
func (a *AuthMiddleware) Authorize() Middleware {
	return func(x Ctx) *APIError {
		if !verifyToken(x) {
			return unauthorized()
		}
		return nil
	}
}
"#,
            ),
            (
                "httpx/middleware.go",
                r#"package httpx

func Register(router gin.IRouter, si ServerInterface, middlewares ...Middleware) {
	RegisterHandlersWithOptions(router, si, GinServerOptions{Middlewares: middlewares})
}
"#,
            ),
            (
                "api/server.go",
                r#"package api

func RegisterHandlersWithOptions(router gin.IRouter, si ServerInterface, options GinServerOptions) {
	wrapper := ServerInterfaceWrapper{Handler: si}
	router.GET(options.BaseURL+"/accelerators", wrapper.ListAccelerators)
}

func (siw *ServerInterfaceWrapper) ListAccelerators(c *GinContext) {
	siw.Handler.ListAccelerators(c)
}
"#,
            ),
        ]);
        let none = HashSet::new();
        let entries = crate::entries::mine_registration_entries(&cpg);
        let census = authz_census(&cpg, &none, &entries, &[]);
        assert_eq!(
            verdict_of(&census, "ListAccelerators"),
            "middleware@Authorize",
            "facade-mounted generated registration must credit the arg-gate: {census:?}"
        );
        assert_eq!(
            verdict_of(&census, "GetStatus"),
            "none",
            "ungated sibling group must NOT be claimed: {census:?}"
        );
        // The generated registration's route expression is kept verbatim.
        assert!(
            census.routes.iter().any(
                |r| r.route.contains("/accelerators") && r.handler.contains("ListAccelerators")
            ),
            "{:?}",
            census.routes
        );
    }

    #[test]
    fn census_global_middleware_before_gate_binds_router_scope() {
        // The builder-chain global-middleware idiom:
        // `r.GlobalMiddleware().Before(fn)` runs fn before dispatch for
        // every route on r — a committing fn is a router-scope gate. The
        // enforcing case is an ANONYMOUS func delegating to a local value
        // bound from a request-signature verifier. Its returned closure
        // rejects with 401s. Controls: an elevation-only Before constructs
        // a privileged context but checks nothing, so it must NOT gate its
        // router; a time comparison named `Before` must not mine at all.
        let cpg = build_go(&[
            (
                "requestsignature/verifier.go",
                r#"package requestsignature

func NewRequestVerifier(kp KeyProvider) func(w ResponseWriter, req *Request) error {
	return func(w ResponseWriter, req *Request) error {
		tenant := req.Header.Get(HeaderTenant)
		if tenant == "" {
			return httperror.Unauthorized("unauthorized")
		}
		if !verifySig(kp, tenant, req) {
			return httperror.Unauthorized("bad signature")
		}
		return nil
	}
}
"#,
            ),
            (
                "elevation/middleware.go",
                r#"package elevation

func DefaultElevationHandler(w ResponseWriter, req *Request) error {
	ctx := WithElevatedRole(req.Context())
	return AddHeaderToReq(ctx, req)
}
"#,
            ),
            (
                "main.go",
                r#"package main

func main(kp KeyProvider, rh *RPCHandler, ph *ProxHandler, sh *StatusHandler, deadline Time, now Time) {
	r := routing.NewRouter()
	verify := requestsignature.NewRequestVerifier(kp)
	r.GlobalMiddleware().Before(func(w ResponseWriter, req *Request) error {
		return verify(w, req)
	})
	r.GET("/rpc/", rh.HandleWS)
	r.ALL("/proxy/", ph.HandleProxy)

	r2 := routing.NewRouter()
	r2.GlobalMiddleware().Before(elevation.DefaultElevationHandler)
	r2.GET("/members/", sh.HandleMembers)

	open := routing.NewRouter()
	open.GET("/status/", sh.GetStatus)

	if deadline.Before(now) {
		expire(r)
	}
}

func (h *RPCHandler) HandleWS(w ResponseWriter, req *Request) error    { return serveWS(w, req) }
func (h *ProxHandler) HandleProxy(w ResponseWriter, req *Request) error { return proxy(w, req) }
func (h *StatusHandler) HandleMembers(w ResponseWriter, req *Request) error { return members(w, req) }
func (h *StatusHandler) GetStatus(w ResponseWriter, req *Request) error { return status(w, req) }
"#,
            ),
        ]);
        let none = HashSet::new();
        let entries = crate::entries::mine_registration_entries(&cpg);
        let census = authz_census(&cpg, &none, &entries, &[]);
        assert_eq!(
            verdict_of(&census, "HandleWS"),
            "middleware@NewRequestVerifier",
            "request-signature Before gate must bind its router's routes: {census:?}"
        );
        assert_eq!(
            verdict_of(&census, "HandleProxy"),
            "middleware@NewRequestVerifier",
            "{census:?}"
        );
        assert_eq!(
            verdict_of(&census, "HandleMembers"),
            "none",
            "elevation-only Before must NOT gate: {census:?}"
        );
        assert_eq!(
            verdict_of(&census, "GetStatus"),
            "none",
            "router with no Before chain must NOT be claimed: {census:?}"
        );
        // The time comparison must not surface as a gate at all.
        assert!(
            !census
                .gates
                .iter()
                .any(|g| g.scope == "Before" && g.file.is_empty()),
            "{:?}",
            census.gates
        );
        assert!(
            census
                .gates
                .iter()
                .any(|g| g.scope == "Before" && g.enforcing && g.name == "NewRequestVerifier"),
            "enforcing Before gate should be mined with the verifier alias: {:?}",
            census.gates
        );
    }

    #[test]
    fn binary_root_layouts() {
        // Single-binary module layout: gate in cmd/ covers the module.
        assert_eq!(binary_root("cmd/run.go"), "");
        assert_eq!(binary_root("broker-service/cmd/run.go"), "broker-service/");
        // Multi-binary layout: gate binds to its own binary dir only.
        assert_eq!(
            binary_root("cmd/eventdelivery/main.go"),
            "cmd/eventdelivery/"
        );
        assert_eq!(
            binary_root("go/cmd/eventdelivery/main.go"),
            "go/cmd/eventdelivery/"
        );
        assert_eq!(binary_root("go/cmd/gateway/sub/main.go"), "go/cmd/gateway/");
        // No cmd segment: the gate file's directory.
        assert_eq!(binary_root("pkg/server/setup.go"), "pkg/server/");
    }

    #[test]
    fn census_inline_partial_and_none() {
        let cpg = build_go(&[(
            "h.go",
            r#"package main

func handleGated(req string) string {
	if !checkPermission(req) {
		return "denied"
	}
	doWork(req)
	return "ok"
}

func handleBranchy(req string, debug bool) string {
	if debug {
		checkPermission(req)
	}
	doWork(req)
	return "ok"
}

func handleOpen(req string) string {
	doWork(req)
	return "ok"
}

func checkPermission(r string) bool { return len(r) > 0 }
func doWork(r string)              {}
"#,
        )]);
        let none = HashSet::new();
        let census = authz_census(
            &cpg,
            &none,
            &[
                "handleGated".into(),
                "handleBranchy".into(),
                "handleOpen".into(),
            ],
            &[],
        );
        assert!(
            verdict_of(&census, "handleGated").starts_with("inline@"),
            "guard-clause check dominates the success return: {census:?}"
        );
        assert!(
            verdict_of(&census, "handleBranchy").starts_with("inline-partial@"),
            "branch-only check must not read as dominated: {census:?}"
        );
        assert_eq!(verdict_of(&census, "handleOpen"), "none");
    }

    #[test]
    fn census_caller_context_gated_and_field_read_shapes() {
        let cpg = build_go(&[(
            "sg.go",
            r#"package main

func handleCallerGated(req string, callerClaims []string) string {
	if len(callerClaims) > 0 {
		if !enforceRbacAccess(callerClaims, req) {
			return "denied"
		}
	}
	doWork(req)
	return "ok"
}

func handleRenamedClaims(req string) string {
	claims := GetCallerClaims(req)
	if len(claims) > 0 {
		if !enforcePolicyForCaller(claims, req) {
			return "denied"
		}
	}
	doWork(req)
	return "ok"
}

func handleFieldRead(in Req) string {
	acct := in.AuthzCtx.Account
	doWork(acct)
	return "ok"
}

func enforceRbacAccess(sc []string, r string) bool  { return len(sc) > 0 }
func enforcePolicyForCaller(claims []string, r string) bool { return len(claims) > 0 }
func GetCallerClaims(r string) []string             { return nil }
func doWork(r string)                               {}
"#,
        )]);
        let authz = HashSet::from(["enforcePolicyForCaller".to_string()]);
        let config = AuthzCensusConfig {
            caller_context_markers: vec!["caller claims".into()],
            ..AuthzCensusConfig::default()
        };
        let census = authz_census_with_config(
            &cpg,
            &authz,
            &[
                "handleCallerGated".into(),
                "handleRenamedClaims".into(),
                "handleFieldRead".into(),
            ],
            &[],
            &config,
        );
        assert!(
            verdict_of(&census, "handleCallerGated").starts_with("subject-gated@"),
            "check parameterized by caller claims: {census:?}"
        );
        assert!(
            verdict_of(&census, "handleRenamedClaims").starts_with("subject-gated@"),
            "renamed local still detected via the GetCallerClaims pull: {census:?}"
        );
        assert_eq!(
            verdict_of(&census, "handleFieldRead"),
            "none",
            "an AuthzCtx.Account field read is authz DATA, not a check: {census:?}"
        );
    }

    #[test]
    fn census_caller_context_markers_are_configurable_and_disableable() {
        let cpg = build_go(&[(
            "custom.go",
            r#"package main

func handleCustomContext(req string, accessTags []string) string {
	if len(accessTags) > 0 {
		if !enforceRbacAccess(accessTags, req) {
			return "denied"
		}
	}
	doWork(req)
	return "ok"
}

func enforceRbacAccess(tags []string, req string) bool { return len(tags) > 0 }
func doWork(req string)                                {}
"#,
        )]);
        let none = HashSet::new();
        let entries = ["handleCustomContext".to_string()];
        let custom = AuthzCensusConfig {
            caller_context_markers: vec!["access tag".into()],
            ..AuthzCensusConfig::default()
        };
        let custom_census = authz_census_with_config(&cpg, &none, &entries, &[], &custom);
        assert!(
            verdict_of(&custom_census, "handleCustomContext").starts_with("subject-gated@"),
            "custom marker should classify the conditional check: {custom_census:?}"
        );

        let disabled = AuthzCensusConfig {
            caller_context_markers: vec![],
            ..AuthzCensusConfig::default()
        };
        let disabled_census = authz_census_with_config(&cpg, &none, &entries, &[], &disabled);
        assert!(
            verdict_of(&disabled_census, "handleCustomContext").starts_with("inline-partial@"),
            "empty markers should disable caller-context classification: {disabled_census:?}"
        );
    }

    #[test]
    fn census_marker_does_not_join_separate_expression_tokens() {
        let cpg = build_go(&[(
            "separate.go",
            r#"package main

func handleSeparateArguments(req string, subject string, context string) string {
	if shouldCheck(req) {
		enforceRbacAccess(subject, context)
	}
	doWork(req)
	return "ok"
}

func shouldCheck(req string) bool                    { return true }
func enforceRbacAccess(subject string, context string) bool { return true }
func doWork(req string)                              {}
"#,
        )]);
        let none = HashSet::new();
        let census = authz_census(&cpg, &none, &["handleSeparateArguments".to_string()], &[]);
        assert!(
            verdict_of(&census, "handleSeparateArguments").starts_with("inline-partial@"),
            "separate subject and context arguments must not form one marker: {census:?}"
        );
    }

    #[test]
    fn census_default_marker_accepts_concatenated_spelling() {
        // Construct the historical spelling from generic words so the
        // compatibility check does not retain a product-shaped identifier.
        let marker = ["Subject", "Contexts"].concat();
        let source = format!(
            r#"package main

func handleConcatenated(req string, {marker} []string) string {{
	if len({marker}) > 0 {{
		if !enforceRbacAccess({marker}, req) {{
			return "denied"
		}}
	}}
	doWork(req)
	return "ok"
}}

func enforceRbacAccess(values []string, req string) bool {{ return len(values) > 0 }}
func doWork(req string)                                {{}}
"#
        );
        let cpg = build_go(&[("compat.go", source.as_str())]);
        let none = HashSet::new();
        let census = authz_census(&cpg, &none, &["handleConcatenated".to_string()], &[]);
        assert!(
            verdict_of(&census, "handleConcatenated").starts_with("subject-gated@"),
            "default phrase should match concatenated, case-varied spellings: {census:?}"
        );
    }

    #[test]
    fn census_framework_server_calls_are_configurable_and_disableable() {
        let cpg = build_go(&[(
            "server.go",
            r#"package main

func main() {
	server := BuildControlPlaneServer()
	server.Run()
}

func BuildControlPlaneServer() Server { return Server{} }
func handleThing(req string) string   { return req }
"#,
        )]);
        let none = HashSet::new();
        let entries = ["handleThing".to_string()];
        let custom = AuthzCensusConfig {
            framework_server_calls: vec!["BuildControlPlaneServer".into()],
            ..AuthzCensusConfig::default()
        };
        let custom_census = authz_census_with_config(&cpg, &none, &entries, &[], &custom);
        assert!(
            custom_census.gates.iter().any(|gate| {
                gate.scope == "framework"
                    && gate.name == "BuildControlPlaneServer"
                    && !gate.enforcing
            }),
            "custom framework constructor should be reported: {custom_census:?}"
        );

        let disabled = AuthzCensusConfig {
            framework_server_calls: vec![],
            ..AuthzCensusConfig::default()
        };
        let disabled_census = authz_census_with_config(&cpg, &none, &entries, &[], &disabled);
        assert!(
            disabled_census
                .gates
                .iter()
                .all(|gate| gate.scope != "framework"),
            "empty framework calls should disable framework evidence: {disabled_census:?}"
        );
    }

    #[test]
    fn census_switch_before_check_still_dominates() {
        // Service-validation example: an enum-mapping switch precedes an
        // unconditional check that gates the only success return. The switch
        // CFG must not break dominance.
        let cpg = build_go(&[(
            "sw.go",
            r#"package main

func handleSwitchThenCheck(req string, kind int) string {
	var p string
	switch kind {
	case 1:
		p = "ro"
	case 2:
		p = "rw"
	default:
		p = "none"
	}
	if !checkPermission(p) {
		return "denied"
	}
	doWork(req)
	return "ok"
}

func handleBareSwitchThenCheck(req string, kind int) string {
	var p string
	switch kind {
	case 1:
		p = "ro"
	case 2:
		p = "rw"
	}
	if !checkPermission(p) {
		return "denied"
	}
	doWork(req)
	return "ok"
}

func checkPermission(r string) bool { return len(r) > 0 }
func doWork(r string)               {}
"#,
        )]);
        let none = HashSet::new();
        let census = authz_census(
            &cpg,
            &none,
            &[
                "handleSwitchThenCheck".into(),
                "handleBareSwitchThenCheck".into(),
            ],
            &[],
        );
        assert!(
            verdict_of(&census, "handleSwitchThenCheck").starts_with("inline@"),
            "{census:?}"
        );
        assert!(
            verdict_of(&census, "handleBareSwitchThenCheck").starts_with("inline@"),
            "{census:?}"
        );
    }

    #[test]
    fn census_server_scope_interceptor_gates_remaining_entries() {
        let cpg = build_go(&[(
            "srv.go",
            r#"package main

func main() {
	s := NewServer(ChainUnaryInterceptor(authInterceptor, logInterceptor))
	s.run()
}

func authInterceptor(ctx string, req string) string {
	if !enforceAccess(ctx) {
		return "denied"
	}
	return req
}

func logInterceptor(ctx string, req string) string { return req }

func handleThing(req string) string {
	doWork(req)
	return "ok"
}

func enforceAccess(c string) bool { return true }
func doWork(r string)             {}
"#,
        )]);
        let none = HashSet::new();
        let census = authz_census(&cpg, &none, &["handleThing".into()], &[]);
        let gate = census
            .gates
            .iter()
            .find(|g| g.name == "authInterceptor")
            .expect("authInterceptor gate mined");
        assert!(gate.enforcing, "body contains enforceAccess: {census:?}");
        assert_eq!(gate.scope, "ChainUnaryInterceptor");
        assert!(
            verdict_of(&census, "handleThing").starts_with("middleware@authInterceptor"),
            "{census:?}"
        );
        // The non-enforcing sibling is mined but must not be the attributed gate.
        assert!(census
            .gates
            .iter()
            .any(|g| g.name == "logInterceptor" && !g.enforcing));
    }

    #[test]
    fn census_factory_closure_body_evidence_marks_enforcing() {
        // The interceptor FACTORY shape: the registered value is a call whose
        // defined function returns a closure, and the authz work lives in the
        // closure body (a separate Method behind a MethodRef). Body evidence
        // must follow the ref.
        let cpg = build_go(&[(
            "srv.go",
            r#"package main

func main() {
	s := NewServer(ChainUnaryInterceptor(UnaryServerInterceptor(verifier)))
	s.run()
}

func UnaryServerInterceptor(v string) func(string) string {
	return func(req string) string {
		if verifyRBAC(req) != "" {
			return "denied"
		}
		return req
	}
}

func verifyRBAC(r string) string { return "" }

func handleThing(req string) string { return req }
"#,
        )]);
        let none = HashSet::new();
        let census = authz_census(&cpg, &none, &["handleThing".into()], &[]);
        let gate = census
            .gates
            .iter()
            .find(|g| g.name == "UnaryServerInterceptor")
            .expect("factory gate mined");
        assert!(gate.enforcing, "closure body calls verifyRBAC: {census:?}");
        assert!(
            verdict_of(&census, "handleThing").starts_with("middleware@"),
            "{census:?}"
        );
    }

    #[test]
    fn census_gate_body_evidence_requires_dominance_over_continuation() {
        // An interceptor whose check runs on EVERY path to the wrapped
        // handler is enforcing; one whose check hides inside a
        // method-allowlist branch is not — other methods pass unchecked.
        let cpg = build_go(&[(
            "srv.go",
            r#"package main

func main() {
	s := NewServer(ChainUnaryInterceptor(GatedInterceptor(v), BranchyInterceptor(v)))
	s.run()
}

func GatedInterceptor(v string) func(string, func(string) string) string {
	return func(req string, handler func(string) string) string {
		if verifyRBAC(req) != "" {
			return "denied"
		}
		return handler(req)
	}
}

func BranchyInterceptor(v string) func(string, func(string) string) string {
	return func(req string, handler func(string) string) string {
		if req == "special" {
			if verifyRBAC(req) != "" {
				return "denied"
			}
		}
		return handler(req)
	}
}

func verifyRBAC(r string) string { return "" }

func handleThing(req string) string { return req }
"#,
        )]);
        let none = HashSet::new();
        let census = authz_census(&cpg, &none, &["handleThing".into()], &[]);
        let gated = census
            .gates
            .iter()
            .find(|g| g.name == "GatedInterceptor")
            .expect("gated gate mined");
        assert!(gated.enforcing, "check dominates handler(): {census:?}");
        let branchy = census
            .gates
            .iter()
            .find(|g| g.name == "BranchyInterceptor")
            .expect("branchy gate mined");
        assert!(
            !branchy.enforcing,
            "allowlist-branch check must not count as enforcing: {census:?}"
        );
    }

    #[test]
    fn census_qualified_spelling_marks_enforcing() {
        // An imported interceptor with no in-module definition: the authz
        // vocabulary lives in the package qualifier (`rbac.Interceptor`).
        // A telemetry qualifier must NOT transfer.
        let cpg = build_go(&[(
            "srv.go",
            r#"package main

func main() {
	s := NewServer(ChainUnaryInterceptor(rbac.Interceptor(cfg), grpcmetrics.StatusInterceptor(cfg)))
	s.run()
}

func handleThing(req string) string { return req }
"#,
        )]);
        let none = HashSet::new();
        let census = authz_census(&cpg, &none, &["handleThing".into()], &[]);
        let gate = census
            .gates
            .iter()
            .find(|g| g.name == "Interceptor")
            .expect("qualified rbac gate mined");
        assert!(gate.enforcing, "rbac qualifier transfers: {census:?}");
        assert!(
            !census
                .gates
                .iter()
                .any(|g| g.name == "StatusInterceptor" && g.enforcing),
            "telemetry qualifier must not transfer: {census:?}"
        );
    }

    #[test]
    fn census_chain_variable_append_framework_shape() {
        // The common-go/service framework shape: interceptors accumulate in
        // a local slice via append, the slice feeds a chain builder, and
        // the builder feeds UnaryInterceptor.
        let cpg = build_go(&[(
            "server.go",
            r#"package main

func buildServer(tls bool) string {
	unaries := makeChain()
	unaries = append(unaries, JWTUnaryValidator(tls), logRequests)
	return UnaryInterceptor(ChainUnaryServer(tracingInterceptor, unaries...))
}

func JWTUnaryValidator(tls bool) string { return "v" }
func logRequests(ctx string) string     { return ctx }
func tracingInterceptor(ctx string) string { return ctx }
func makeChain() string                 { return "" }

func handleThing(req string) string {
	doWork(req)
	return "ok"
}

func doWork(r string) {}
"#,
        )]);
        let none = HashSet::new();
        let census = authz_census(&cpg, &none, &["handleThing".into()], &[]);
        let gate = census
            .gates
            .iter()
            .find(|g| g.name == "JWTUnaryValidator")
            .unwrap_or_else(|| panic!("JWT gate must be mined via append-chase: {census:?}"));
        assert!(
            gate.enforcing,
            "JWTUnaryValidator is authz-shaped by name: {census:?}"
        );
        assert!(
            verdict_of(&census, "handleThing").starts_with("middleware@JWTUnaryValidator"),
            "{census:?}"
        );
    }

    #[test]
    fn census_route_wrapper_beats_module_middleware() {
        let cpg = build_go(&[(
            "mux.go",
            r#"package main

func main() {
	HandleFunc("/thing", requireAuth(handleThing))
	HandleFunc("/other", handleOther)
}

func requireAuth(h func(string) string) func(string) string { return h }

func handleThing(req string) string {
	doWork(req)
	return "ok"
}

func handleOther(req string) string {
	doWork(req)
	return "ok"
}

func doWork(r string) {}
"#,
        )]);
        let none = HashSet::new();
        let census = authz_census(
            &cpg,
            &none,
            &["handleThing".into(), "handleOther".into()],
            &[],
        );
        assert_eq!(
            verdict_of(&census, "handleThing"),
            "wrapped@requireAuth",
            "{census:?}"
        );
        assert_eq!(verdict_of(&census, "handleOther"), "none", "{census:?}");
    }
}
