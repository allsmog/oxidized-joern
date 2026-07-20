//! Summaries-first dataflow (reads Ast/SymbolRef/CallGraph, writes Summaries).
//!
//! A function summary captures how data flows from a method's inputs (params,
//! receiver) to its outputs (params, return) — independent of call sites. Two
//! reasons this is the right spine, both raised in the architecture review:
//!
//! * **Scale.** Interprocedural taint that re-explores every callee per query
//!   blows up. Computing each method once into a summary and *reusing* it makes
//!   analysis roughly linear in code size — the only way to reach millions of
//!   lines.
//! * **Incrementality.** A summary is the natural invalidation boundary. When a
//!   file changes we drop summaries for its methods and any caller that
//!   transitively depended on them, then recompute *only those*. Everything
//!   else is served from cache.
//!
//! External/unanalysable functions (libc, third-party) get summaries from a
//! declarative JSON file, mirroring Fraunhofer's DFG-function-summary format.
//!
//! Flows carry an optional sanitizer marker (`Flow::via`): a summary can say
//! "param 0 reaches the return, but only through `escape`", which downstream
//! taint queries treat as neutralised. Sanitizer names are registered on the
//! [`SummaryStore`] (see [`SummaryStore::set_sanitizers`]) so summary
//! computation records them; the query-time spec in `taint.rs` adds its own.

use crate::pass::ast_descendants;
use cpg_core::{Cpg, NodeId, NodeKind, Query};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// An endpoint of a flow within a function's signature.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub enum Point {
    /// 0-based formal parameter.
    Param(usize),
    /// The method's return value.
    Return,
}

/// A sanitizing function a flow passed through (by name).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct Sanitizer(pub String);

impl Sanitizer {
    pub fn new(name: impl Into<String>) -> Self {
        Sanitizer(name.into())
    }

    pub fn name(&self) -> &str {
        &self.0
    }
}

/// A single input→output flow in a function summary.
///
/// `via: None` is a *raw* flow — taint passes through unchanged. `via:
/// Some(s)` means the data still flows, but every path goes through the
/// sanitizer `s`: the dependency is real (useful for auditing and for "what
/// sanitizes this?" queries) but it must not propagate raw taint.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize)]
pub struct Flow {
    pub from: Point,
    pub to: Point,
    /// The sanitizer this flow is laundered through, if any.
    pub via: Option<Sanitizer>,
}

impl Flow {
    /// A raw (unsanitized) flow — the historical two-field shape.
    pub fn direct(from: Point, to: Point) -> Self {
        Flow { from, to, via: None }
    }

    /// A flow that only exists through the named sanitizer.
    pub fn sanitized(from: Point, to: Point, sanitizer: impl Into<String>) -> Self {
        Flow { from, to, via: Some(Sanitizer(sanitizer.into())) }
    }

    pub fn is_sanitized(&self) -> bool {
        self.via.is_some()
    }
}

/// A call whose *result* flows to this function's return — the
/// returns-tainted summary tier. A wrapper like `fn readEnv() { return
/// getenv("X") }` has no param→return flow at all, so callers could never
/// see the taint it manufactures; recording the originating call's name
/// lets a query-time source match ("getenv" ∈ spec.sources) originate
/// taint at the *call site of the wrapper*. `via: Some(s)` means the
/// result only reaches the return through sanitizer `s` (recorded for
/// auditing, never lifted raw).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize)]
pub struct CallReturn {
    /// Simple (call-site) name of the originating call — spec source names
    /// are simple names, so matching happens on this form.
    pub call: String,
    pub via: Option<Sanitizer>,
}

#[derive(Clone, Debug, Default)]
pub struct FunctionSummary {
    pub fqn: String,
    pub flows: HashSet<Flow>,
    /// Calls whose results flow to the return (see [`CallReturn`]).
    pub call_returns: HashSet<CallReturn>,
}

impl FunctionSummary {
    /// Parameters with a *raw* (unsanitized) flow to the return value. This is
    /// what taint propagation consults: sanitized flows are excluded, so a
    /// callee whose only param→return path goes through a sanitizer does not
    /// propagate raw taint.
    pub fn flows_to_return(&self) -> impl Iterator<Item = usize> + '_ {
        self.flows.iter().filter_map(|f| match (f.from, f.to, &f.via) {
            (Point::Param(k), Point::Return, None) => Some(k),
            _ => None,
        })
    }

    /// Parameters that reach the return only through a sanitizer.
    pub fn sanitized_flows_to_return(&self) -> impl Iterator<Item = (usize, &Sanitizer)> + '_ {
        self.flows.iter().filter_map(|f| match (f.from, f.to, &f.via) {
            (Point::Param(k), Point::Return, Some(s)) => Some((k, s)),
            _ => None,
        })
    }

    /// Names of calls whose results flow *raw* to the return — what
    /// returns-tainted source matching consults; sanitized entries are
    /// excluded (the wrapper laundered the value).
    pub fn raw_call_returns(&self) -> impl Iterator<Item = &str> {
        self.call_returns.iter().filter(|c| c.via.is_none()).map(|c| c.call.as_str())
    }
}

/// Where a summary served by the store came from — needed by findings'
/// provenance so a result influenced by a non-computed tier is auditable.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
pub enum SummaryOrigin {
    /// Computed from the method's body by this engine.
    Computed,
    /// Loaded from the external (JSON) corpus; there is no body to inspect.
    External,
}

/// Cache of summaries plus the dependency graph needed to invalidate precisely.
#[derive(Default)]
pub struct SummaryStore {
    summaries: HashMap<String, FunctionSummary>,
    /// fqn -> set of callee fqns its summary depended on (the invalidation web).
    deps: HashMap<String, HashSet<String>>,
    /// Reverse web: callee fqn -> caller fqns whose summaries used it. Lets
    /// transitive invalidation run as a worklist BFS over the affected region
    /// instead of scanning every summary's deps.
    rdeps: HashMap<String, HashSet<String>>,
    /// External summaries loaded from JSON; never invalidated by source edits.
    external: HashMap<String, FunctionSummary>,
    /// Function names that neutralise taint: a call to one of these produces a
    /// *sanitized* flow rather than a raw one during summary computation.
    sanitizers: HashSet<String>,
    /// Diagnostic: how many method summaries were (re)computed in the last call.
    pub last_recomputed: HashSet<String>,
    /// Diagnostic: within-fixpoint memo hits during the last recomputation.
    /// A hit means a method's summary was *replayed* rather than recomputed
    /// because its inputs (callee summary states + sanitizer set) were
    /// unchanged — the determinism guard for future non-deterministic summary
    /// sources (e.g. an LLM tier).
    pub last_memo_hits: usize,
}

impl SummaryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, fqn: &str) -> Option<&FunctionSummary> {
        self.summaries.get(fqn).or_else(|| self.external.get(fqn))
    }

    /// Like [`get`](Self::get) but also reports whether the summary was
    /// computed from a body or loaded from the external corpus. Computed
    /// summaries shadow external ones, as in `get`.
    pub fn get_with_origin(&self, fqn: &str) -> Option<(&FunctionSummary, SummaryOrigin)> {
        self.summaries
            .get(fqn)
            .map(|s| (s, SummaryOrigin::Computed))
            .or_else(|| self.external.get(fqn).map(|s| (s, SummaryOrigin::External)))
    }

    pub fn len(&self) -> usize {
        self.summaries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.summaries.is_empty()
    }

    /// Register the sanitizer function names summary computation should honour.
    /// A call to one of these propagates its arguments' dependency *marked
    /// sanitized* instead of as raw taint. Call before `compute_all` (or
    /// recompute afterwards) — summaries already in the cache are not revised.
    pub fn set_sanitizers<I, S>(&mut self, names: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.sanitizers = names.into_iter().map(Into::into).collect();
    }

    /// The sanitizer names summary computation honours.
    pub fn sanitizer_names(&self) -> &HashSet<String> {
        &self.sanitizers
    }

    /// Load external function summaries from JSON (Fraunhofer-style). Each
    /// dataFlow entry may carry an optional `"via": "<sanitizer>"` field
    /// marking the flow as sanitized.
    pub fn load_external_json(&mut self, json: &str) -> Result<usize, String> {
        let entries: Vec<ExternalEntry> =
            serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut n = 0;
        for e in entries {
            let fqn = e.function_declaration.method_name.clone();
            let mut flows = HashSet::new();
            for df in &e.data_flows {
                if let (Some(from), Some(to)) = (parse_point(&df.from), parse_point(&df.to)) {
                    flows.insert(Flow {
                        from,
                        to,
                        via: df.via.clone().map(Sanitizer),
                    });
                }
            }
            let call_returns = e
                .call_returns
                .iter()
                .map(|cr| CallReturn {
                    call: cr.call.clone(),
                    via: cr.via.clone().map(Sanitizer),
                })
                .collect();
            self.external.insert(fqn.clone(), FunctionSummary { fqn, flows, call_returns });
            n += 1;
        }
        Ok(n)
    }

    /// Replace `fqn`'s dependency set, keeping the reverse web consistent.
    fn set_deps(&mut self, fqn: &str, deps: HashSet<String>) {
        self.unhook_deps(fqn);
        for callee in &deps {
            self.rdeps
                .entry(callee.clone())
                .or_default()
                .insert(fqn.to_string());
        }
        self.deps.insert(fqn.to_string(), deps);
    }

    /// Remove `fqn`'s dependency entries from both webs.
    fn unhook_deps(&mut self, fqn: &str) {
        if let Some(old) = self.deps.remove(fqn) {
            for callee in old {
                if let Some(set) = self.rdeps.get_mut(&callee) {
                    set.remove(fqn);
                }
            }
        }
    }

    /// Compute summaries for every user method from scratch (fixpoint, so a
    /// caller benefits from its callee's summary regardless of file order).
    pub fn compute_all(&mut self, cpg: &Cpg) {
        let all: Vec<NodeId> = cpg.nodes_of_kind(NodeKind::Method);
        self.recompute(cpg, &all);
    }

    /// Incremental update: invalidate summaries for the directly-changed
    /// methods plus every transitive caller that depended on them, then
    /// recompute only the invalidated set; everything else is a cache hit.
    /// `node_of_fqn` is the caller-maintained method index, so this never
    /// scans the graph — the whole edit path stays O(affected).
    pub fn update_for_changed_methods(
        &mut self,
        cpg: &Cpg,
        directly_changed: HashSet<String>,
        node_of_fqn: &HashMap<String, NodeId>,
    ) {
        let mut invalid: HashSet<String> = directly_changed;
        // Transitively invalidate callers whose summary used an invalid fqn:
        // worklist BFS over the reverse-dependency web, O(affected region).
        let mut worklist: Vec<String> = invalid.iter().cloned().collect();
        while let Some(fqn) = worklist.pop() {
            if let Some(callers) = self.rdeps.get(&fqn) {
                let newly: Vec<String> = callers
                    .iter()
                    .filter(|c| !invalid.contains(*c))
                    .cloned()
                    .collect();
                for c in newly {
                    invalid.insert(c.clone());
                    worklist.push(c);
                }
            }
        }
        // Drop invalidated entries, recompute only those nodes that still exist.
        for fqn in &invalid {
            self.summaries.remove(fqn);
            self.unhook_deps(fqn);
        }
        let to_recompute: Vec<NodeId> = invalid
            .iter()
            .filter_map(|fqn| node_of_fqn.get(fqn).copied())
            .collect();
        self.recompute(cpg, &to_recompute);
    }

    /// Hash of the summary states of `deps` (plus the sanitizer set): the
    /// complete input state a method's summary computation depends on, beyond
    /// the method's own body. Used as the within-fixpoint memo key.
    fn dep_state_hash(&self, deps: &HashSet<String>) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        let mut names: Vec<&String> = deps.iter().collect();
        names.sort();
        for name in names {
            name.hash(&mut h);
            match self.get(name) {
                Some(s) => {
                    let mut flows: Vec<&Flow> = s.flows.iter().collect();
                    flows.sort();
                    flows.hash(&mut h);
                    let mut crs: Vec<&CallReturn> = s.call_returns.iter().collect();
                    crs.sort();
                    crs.hash(&mut h);
                }
                None => 0u8.hash(&mut h),
            }
        }
        let mut sans: Vec<&String> = self.sanitizers.iter().collect();
        sans.sort();
        sans.hash(&mut h);
        h.finish()
    }

    /// Fixpoint recomputation over `targets` only. Each round computes all
    /// targets in parallel against a snapshot of the store (Jacobi iteration),
    /// then applies updates serially; rounds repeat until no summary changes.
    /// Convergence needs one round per level of the call-dependency chain.
    ///
    /// Determinism guard: within one fixpoint, a method's summary is computed
    /// at most once per (fqn, callee-summary-state) — repeats are replayed
    /// from a memo. Convergence detection compares `prev.flows != new.flows`,
    /// so a summary source that could answer differently on identical inputs
    /// (a future LLM tier, a timeout-bounded solver) would otherwise be able
    /// to oscillate forever; the memo makes each input state answered once.
    fn recompute(&mut self, cpg: &Cpg, targets: &[NodeId]) {
        use rayon::prelude::*;
        self.last_recomputed.clear();
        self.last_memo_hits = 0;
        let mut memo: HashMap<(String, u64), (FunctionSummary, HashSet<String>)> =
            HashMap::new();
        let mut changed = true;
        let mut iterations = 0;
        while changed && iterations < targets.len() + 2 {
            changed = false;
            iterations += 1;
            // Replay memo hits; compute only the misses (in parallel).
            let mut results: Vec<(FunctionSummary, HashSet<String>)> = Vec::new();
            let mut to_compute: Vec<NodeId> = Vec::new();
            for &m in targets {
                let fqn = cpg.full_name_of(m).unwrap_or("<anon>");
                let cached = self.deps.get(fqn).and_then(|d| {
                    memo.get(&(fqn.to_string(), self.dep_state_hash(d))).cloned()
                });
                match cached {
                    Some(r) => {
                        self.last_memo_hits += 1;
                        results.push(r);
                    }
                    None => to_compute.push(m),
                }
            }
            let fresh: Vec<(FunctionSummary, HashSet<String>)> = to_compute
                .par_iter()
                .map(|&m| compute_method(cpg, m, self))
                .collect();
            // Memoise fresh results against the store state they were computed
            // from (before this round's updates are applied).
            for r in &fresh {
                let key = (r.0.fqn.clone(), self.dep_state_hash(&r.1));
                memo.insert(key, r.clone());
            }
            results.extend(fresh);
            for (summary, deps) in results {
                let fqn = summary.fqn.clone();
                let prev = self.summaries.get(&fqn);
                if prev.map(|p| (&p.flows, &p.call_returns))
                    != Some((&summary.flows, &summary.call_returns))
                {
                    changed = true;
                }
                self.set_deps(&fqn, deps);
                self.last_recomputed.insert(fqn.clone());
                self.summaries.insert(fqn, summary);
            }
        }
    }
}

/// What a taint mark during summary computation derives from: a formal
/// parameter (the classic param→return tier) or the result of a named call
/// (the returns-tainted tier — the call's simple name is what query-time
/// source matching compares against).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum TagOrigin {
    Param(usize),
    Call(String),
}

/// A taint mark during summary computation: what the value derives from,
/// and the sanitizer it went through (if any).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Tag {
    origin: TagOrigin,
    via: Option<Sanitizer>,
}

/// Compute one method's summary by name-based taint over its body, using the
/// store's existing summaries for callees (summaries-first interprocedural).
fn compute_method(cpg: &Cpg, method: NodeId, store: &SummaryStore) -> (FunctionSummary, HashSet<String>) {
    let fqn = cpg
        .full_name_of(method)
        .unwrap_or("<anon>")
        .to_string();

    // Map parameter name -> 0-based index.
    let params = cpg.parameters_of(method);
    let mut param_index: HashMap<String, usize> = HashMap::new();
    for (i, &p) in params.iter().enumerate() {
        if let Some(n) = cpg.name_of(p) {
            param_index.insert(n.to_string(), i);
        }
    }

    // var name -> set of tagged origins that flow into it.
    let mut taint: HashMap<String, HashSet<Tag>> = HashMap::new();
    for (name, &i) in &param_index {
        taint.insert(
            name.clone(),
            HashSet::from([Tag { origin: TagOrigin::Param(i), via: None }]),
        );
    }

    let mut deps: HashSet<String> = HashSet::new();
    let mut result_flows: HashSet<Flow> = HashSet::new();
    let mut call_returns: HashSet<CallReturn> = HashSet::new();

    // Statements in source order under the method.
    let mut stmts: Vec<NodeId> = ast_descendants(cpg, method)
        .into_iter()
        .filter(|&n| {
            matches!(cpg.kind_of(n), NodeKind::Call | NodeKind::Return)
                // top-level only: parent is not itself an argument we already visit
                && cpg.line_of(n).is_some()
        })
        .collect();
    stmts.sort_by_key(|&n| (cpg.line_of(n).unwrap_or(0), n.0));

    for n in stmts {
        match cpg.kind_of(n) {
            NodeKind::Call if cpg.name_of(n) == Some("=") => {
                // assignment: arg0 = lhs, arg1 = rhs
                let args = cpg.arguments_of(n);
                if args.len() == 2 {
                    let rhs_taint = expr_taint(cpg, args[1], &taint, store, &mut deps);
                    if let Some(lhs_name) = lhs_name(cpg, args[0]) {
                        taint.insert(lhs_name, rhs_taint);
                    }
                }
            }
            NodeKind::Return => {
                for c in cpg.out_kind(n, cpg_core::EdgeKind::Ast) {
                    for tag in expr_taint(cpg, c, &taint, store, &mut deps) {
                        match tag.origin {
                            TagOrigin::Param(k) => {
                                result_flows.insert(Flow {
                                    from: Point::Param(k),
                                    to: Point::Return,
                                    via: tag.via,
                                });
                            }
                            TagOrigin::Call(name) => {
                                call_returns.insert(CallReturn { call: name, via: tag.via });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Bound the returns-tainted set: a method whose return derives from very
    // many distinct calls is an aggregator, not a source wrapper — keep the
    // lexicographically-first entries so the cap is deterministic.
    if call_returns.len() > MAX_CALL_RETURNS {
        let mut v: Vec<CallReturn> = call_returns.into_iter().collect();
        v.sort();
        v.truncate(MAX_CALL_RETURNS);
        call_returns = v.into_iter().collect();
    }

    (FunctionSummary { fqn, flows: result_flows, call_returns }, deps)
}

/// Cap on distinct [`CallReturn`] entries per summary (see `compute_method`).
const MAX_CALL_RETURNS: usize = 64;

/// The variable name an lvalue identifier refers to.
pub(crate) fn lhs_name(cpg: &Cpg, node: NodeId) -> Option<String> {
    if cpg.kind_of(node) == NodeKind::Identifier {
        return cpg.name_of(node).map(|s| s.to_string());
    }
    None
}

/// Set of tagged parameters that taint an expression.
fn expr_taint(
    cpg: &Cpg,
    node: NodeId,
    taint: &HashMap<String, HashSet<Tag>>,
    store: &SummaryStore,
    deps: &mut HashSet<String>,
) -> HashSet<Tag> {
    match cpg.kind_of(node) {
        NodeKind::Identifier => cpg
            .name_of(node)
            .and_then(|n| taint.get(n).cloned())
            .unwrap_or_default(),
        NodeKind::Literal => HashSet::new(),
        NodeKind::Call => {
            let name = cpg.name_of(node).unwrap_or("");
            let args = cpg.arguments_of(node);
            let arg_taints: Vec<HashSet<Tag>> = args
                .iter()
                .map(|&a| expr_taint(cpg, a, taint, store, deps))
                .collect();

            if is_operator(name) {
                // Operators propagate all operands (conservative, precise enough).
                let mut out = HashSet::new();
                for t in &arg_taints {
                    out.extend(t.iter().cloned());
                }
                return out;
            }
            // Field-read pass-through: the member-read lowering emits
            // `c->key` as a Call named "key" carrying the base — a field
            // name never has a summary, so without this the base's tags die
            // here and the `return c->key` accessor shape loses its
            // param→return flow. Only NON-invoked shapes qualify (no `(` in
            // the code — a spelled invocation always carries parens):
            // `c.method()` stays summary-driven. No self-tag either — a
            // field name is a read, not an originating call. (Gate on code
            // text, not the receiver stamp: the C frontend's field reads
            // carry no signature.)
            if cpg.code_of(node).is_some_and(|c| !c.contains('(')) {
                let mut out = HashSet::new();
                for t in &arg_taints {
                    out.extend(t.iter().cloned());
                }
                return out;
            }
            if store.sanitizers.contains(name) {
                // Sanitizer call: the result still *derives from* its
                // arguments (record the dependency, sanitized) but does not
                // carry raw taint. An already-sanitized tag keeps its first
                // sanitizer.
                deps.insert(name.to_string());
                let mut out = HashSet::new();
                for t in &arg_taints {
                    for tag in t {
                        out.insert(Tag {
                            origin: tag.origin.clone(),
                            via: tag
                                .via
                                .clone()
                                .or_else(|| Some(Sanitizer(name.to_string()))),
                        });
                    }
                }
                return out;
            }
            // Named function: drive result taint from the callee's summary.
            // Raw callee flows preserve the argument's tag; sanitized callee
            // flows mark it sanitized (so laundering is transitive).
            // Summaries are stored under the callee's FULL name
            // (`filesystem::JoinPaths`) while the call site carries the simple
            // name — resolve through the Call edge when there is one, so
            // qualified C++/Scala summaries actually compose. (This also keys
            // the dependency web by fqn, which is what incremental
            // invalidation matches against.)
            let key: String = {
                use cpg_core::Query;
                cpg.call_target(node)
                    .and_then(|m| cpg.full_name_of(m))
                    .unwrap_or(name)
                    .to_string()
            };
            deps.insert(key.clone());
            let mut out = HashSet::new();
            // Returns-tainted self-tag: this call's result derives from the
            // call itself — if its (simple) name is a spec source at query
            // time, the summary carrying this tag to its return is exactly
            // the `fn f() { return getenv(..) }` wrapper shape.
            if !name.is_empty() {
                out.insert(Tag { origin: TagOrigin::Call(name.to_string()), via: None });
            }
            if let Some(summary) = store.get(&key) {
                for f in &summary.flows {
                    let (Point::Param(k), Point::Return) = (f.from, f.to) else {
                        continue;
                    };
                    if let Some(t) = arg_taints.get(k) {
                        for tag in t {
                            out.insert(Tag {
                                origin: tag.origin.clone(),
                                via: tag.via.clone().or_else(|| f.via.clone()),
                            });
                        }
                    }
                }
                // Transitivity: the callee itself returns some call's result
                // (its own returns-tainted tier) — our result then derives
                // from that same originating call, one level further up.
                for cr in &summary.call_returns {
                    out.insert(Tag {
                        origin: TagOrigin::Call(cr.call.clone()),
                        via: cr.via.clone(),
                    });
                }
            }
            out
        }
        _ => {
            // Unknown wrapper: union of AST children.
            let mut out = HashSet::new();
            for c in cpg.out_kind(node, cpg_core::EdgeKind::Ast) {
                out.extend(expr_taint(cpg, c, taint, store, deps));
            }
            out
        }
    }
}

pub(crate) fn is_operator(name: &str) -> bool {
    name.chars()
        .next()
        .map(|c| !c.is_alphabetic() && c != '_')
        .unwrap_or(true)
}

fn parse_point(s: &str) -> Option<Point> {
    if s == "return" {
        return Some(Point::Return);
    }
    if let Some(rest) = s.strip_prefix("param") {
        return rest.parse::<usize>().ok().map(Point::Param);
    }
    None
}

// --- JSON shapes for external summaries (Fraunhofer-style) ---
#[derive(Deserialize)]
struct ExternalEntry {
    #[serde(rename = "functionDeclaration")]
    function_declaration: ExternalDecl,
    #[serde(rename = "dataFlows", default)]
    data_flows: Vec<ExternalFlow>,
    /// Optional returns-tainted declarations: calls whose results this
    /// function returns (e.g. a vendored wrapper around `getenv`).
    #[serde(rename = "callReturns", default)]
    call_returns: Vec<ExternalCallReturn>,
}

#[derive(Deserialize)]
struct ExternalCallReturn {
    call: String,
    #[serde(default)]
    via: Option<String>,
}

#[derive(Deserialize)]
struct ExternalDecl {
    #[serde(rename = "methodName")]
    method_name: String,
}

#[derive(Deserialize)]
struct ExternalFlow {
    from: String,
    to: String,
    /// Optional sanitizer name: the flow exists but is neutralised by it.
    #[serde(default)]
    via: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpg_frontend::Frontend;

    /// Build a CPG from C sources and run the standard pass pipeline —
    /// a minimal stand-in for the incremental driver, local to these tests.
    fn build(files: &[(&str, &str)]) -> Cpg {
        let mut cpg = Cpg::new();
        let mut fe = cpg_lang_c::CFrontend::new();
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

    #[test]
    fn sanitizer_call_marks_summary_flow_sanitized() {
        // wrap launders its parameter through `escape`: the summary must still
        // record param0 -> return (the dependency is real) but marked
        // sanitized, and the raw-flow view must be empty.
        let cpg = build(&[(
            "s.c",
            "char* wrap(char* s) {\n    return escape(s);\n}\n",
        )]);
        let mut store = SummaryStore::new();
        store.set_sanitizers(["escape"]);
        store.compute_all(&cpg);
        let wrap = store.get("wrap").expect("wrap summarised");
        assert!(
            wrap.flows.contains(&Flow::sanitized(Point::Param(0), Point::Return, "escape")),
            "expected sanitized param0->return, got {:?}",
            wrap.flows
        );
        assert_eq!(wrap.flows_to_return().count(), 0, "no RAW flow may remain");
        assert_eq!(
            wrap.sanitized_flows_to_return().collect::<Vec<_>>(),
            vec![(0, &Sanitizer::new("escape"))]
        );
    }

    #[test]
    fn sanitized_flow_propagates_transitively() {
        // outer(y) { return wrap(y); } where wrap's flow is sanitized: outer's
        // summary must carry the sanitized mark too, not resurrect raw taint.
        let cpg = build(&[(
            "t.c",
            "char* wrap(char* s) {\n    return escape(s);\n}\nchar* outer(char* y) {\n    return wrap(y);\n}\n",
        )]);
        let mut store = SummaryStore::new();
        store.set_sanitizers(["escape"]);
        store.compute_all(&cpg);
        let outer = store.get("outer").expect("outer summarised");
        assert_eq!(outer.flows_to_return().count(), 0, "raw taint must not leak through wrap");
        assert!(
            outer
                .sanitized_flows_to_return()
                .any(|(k, s)| k == 0 && s.name() == "escape"),
            "expected transitive sanitized flow, got {:?}",
            outer.flows
        );
    }

    #[test]
    fn non_sanitized_flow_still_raw() {
        let cpg = build(&[("r.c", "char* id(char* s) {\n    return s;\n}\n")]);
        let mut store = SummaryStore::new();
        store.set_sanitizers(["escape"]);
        store.compute_all(&cpg);
        let id = store.get("id").unwrap();
        assert!(id.flows.contains(&Flow::direct(Point::Param(0), Point::Return)));
        assert_eq!(id.flows_to_return().collect::<Vec<_>>(), vec![0]);
    }

    #[test]
    fn external_json_flow_with_via_is_sanitized() {
        let mut store = SummaryStore::new();
        store
            .load_external_json(
                r#"[{"functionDeclaration":{"language":"C","methodName":"html_escape"},
                     "dataFlows":[{"from":"param0","to":"return","via":"html_escape"}]},
                    {"functionDeclaration":{"language":"C","methodName":"strdup"},
                     "dataFlows":[{"from":"param0","to":"return"}]}]"#,
            )
            .unwrap();
        let esc = store.get("html_escape").unwrap();
        assert_eq!(esc.flows_to_return().count(), 0);
        assert!(esc.sanitized_flows_to_return().any(|(k, s)| k == 0 && s.name() == "html_escape"));
        // Backwards-compatible: entries without "via" stay raw.
        let dup = store.get("strdup").unwrap();
        assert_eq!(dup.flows_to_return().collect::<Vec<_>>(), vec![0]);
        assert_eq!(
            store.get_with_origin("strdup").map(|(_, o)| o),
            Some(SummaryOrigin::External)
        );
    }

    #[test]
    fn fixpoint_memoises_repeat_computations() {
        // A two-level chain forces multiple fixpoint rounds; later rounds must
        // replay unchanged methods from the memo rather than recompute them.
        let cpg = build(&[(
            "m.c",
            "int id(int x) {\n    return x;\n}\nint wrap(int y) {\n    return id(y);\n}\nint outer(int z) {\n    return wrap(z);\n}\n",
        )]);
        let mut store = SummaryStore::new();
        store.compute_all(&cpg);
        assert!(
            store.last_memo_hits > 0,
            "fixpoint rounds after the first should hit the memo"
        );
        // And the memo must not change results: converged summaries are correct.
        for fqn in ["id", "wrap", "outer"] {
            let s = store.get(fqn).unwrap_or_else(|| panic!("{fqn} summarised"));
            assert!(
                s.flows.contains(&Flow::direct(Point::Param(0), Point::Return)),
                "{fqn}: expected raw param0->return, got {:?}",
                s.flows
            );
        }
        // Determinism: recomputing from scratch yields identical flows.
        let mut store2 = SummaryStore::new();
        store2.compute_all(&cpg);
        for fqn in ["id", "wrap", "outer"] {
            assert_eq!(store.get(fqn).unwrap().flows, store2.get(fqn).unwrap().flows);
        }
    }

    #[test]
    fn call_returns_record_source_wrappers_raw_and_sanitized() {
        // readcfg returns getenv's result raw; readsafe only through the
        // sanitizer — the entry is recorded (auditable) but never raw.
        // relay returns readcfg's result: the originating call's name must
        // propagate transitively through the fixpoint.
        let cpg = build(&[(
            "cr.c",
            "char* clean(char* s) {\n    return s;\n}\nchar* readcfg() {\n    return getenv(\"X\");\n}\nchar* readsafe() {\n    return clean(getenv(\"X\"));\n}\nchar* relay() {\n    char* t = readcfg();\n    return t;\n}\n",
        )]);
        let mut store = SummaryStore::new();
        store.set_sanitizers(["clean"]);
        store.compute_all(&cpg);

        let cfg = store.get("readcfg").unwrap();
        assert!(cfg.raw_call_returns().any(|c| c == "getenv"), "{:?}", cfg.call_returns);

        let safe = store.get("readsafe").unwrap();
        assert!(
            !safe.raw_call_returns().any(|c| c == "getenv"),
            "sanitized wrapper must not expose getenv raw: {:?}",
            safe.call_returns
        );
        assert!(
            safe.call_returns
                .iter()
                .any(|c| c.call == "getenv" && c.via.as_ref().is_some_and(|s| s.name() == "clean")),
            "{:?}",
            safe.call_returns
        );

        let relay = store.get("relay").unwrap();
        assert!(
            relay.raw_call_returns().any(|c| c == "getenv"),
            "transitive call-return through readcfg: {:?}",
            relay.call_returns
        );
    }
}
