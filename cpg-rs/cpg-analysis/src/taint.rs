//! Interprocedural source→sink taint queries over function summaries.
//!
//! This is the security-facing query the whole platform exists to answer:
//! *does attacker-controlled data from a `source` reach a dangerous `sink`?*
//! It runs on top of the summary cache, so it inherits the engine's two key
//! properties — it scales (summaries are precomputed and reused, no per-query
//! re-exploration of callees) and it stays correct across edits (a changed
//! file invalidates exactly the affected summaries before the next query).
//!
//! The analysis is intraprocedural taint *within* each method, lifted
//! interprocedurally by consulting callee summaries: a call propagates taint
//! from a tainted argument to the call's result iff the callee's summary maps
//! that parameter to its return *raw* (unsanitized). A finding is raised when
//! a tainted value reaches an argument of a configured sink.
//!
//! Sanitizers: a call to a name in [`TaintSpec::sanitizers`] (or in the
//! summary store's sanitizer set) never propagates taint from its arguments
//! to its result, and callee-summary flows marked sanitized are not lifted.
//! When a computed callee's raw flow is lifted, the callee's internal
//! expression chain is additionally re-checked against the sanitizer set, so
//! a path that is only realisable through a sanitizer inside the callee is
//! not reported either.
//!
//! Witness paths are auditable: every [`Step`] records its [`Provenance`]
//! (intraprocedural propagation, a computed-summary lift, or an external
//! summary with no body) and a `depth` marker. Lifting through an analysable
//! callee splices the callee's internal source-param→return chain into the
//! path at `depth + 1`; external (JSON) summaries have no body, so those hops
//! appear as a single summary-only step.

use crate::pass::method_name_index;
use crate::summaries::{is_operator, lhs_name, SummaryOrigin, SummaryStore};
use cpg_core::{Cpg, NodeId, NodeKind, Query};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// Callee-splicing recursion bound: beyond this depth a lifted hop is shown
/// summary-only (no internal steps) rather than expanded further.
const MAX_SPLICE_DEPTH: u32 = 8;

/// What counts as a source, a sink, and a sanitizer, by function name.
pub struct TaintSpec {
    /// Calls to these names produce tainted values (their return is tainted).
    pub sources: HashSet<String>,
    /// Calls to these names are dangerous; a tainted argument is a finding.
    pub sinks: HashSet<String>,
    /// Calls to these names neutralise taint: the result does NOT inherit
    /// taint from the arguments, so a path that only exists through one of
    /// them is never reported.
    pub sanitizers: HashSet<String>,
}

impl TaintSpec {
    pub fn new(sources: &[&str], sinks: &[&str]) -> Self {
        Self::with_sanitizers(sources, sinks, &[])
    }

    pub fn with_sanitizers(sources: &[&str], sinks: &[&str], sanitizers: &[&str]) -> Self {
        TaintSpec {
            sources: sources.iter().map(|s| s.to_string()).collect(),
            sinks: sinks.iter().map(|s| s.to_string()).collect(),
            sanitizers: sanitizers.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// What produced a witness step — makes every finding auditable, which
/// matters once non-computed summary tiers (external JSON today, an LLM tier
/// later) can influence results.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub enum Provenance {
    /// Intraprocedural propagation within the enclosing method's body.
    IntraProc,
    /// Taint lifted through the *computed* summary of an analysable callee.
    SummaryFlow { callee_fqn: String },
    /// Taint lifted through an external (JSON) summary — no body exists, so
    /// the hop is summary-only and cannot be expanded.
    ExternalSummary { callee_fqn: String },
}

/// One step along a taint witness (a tainted expression and where it occurs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    pub code: String,
    pub line: Option<u32>,
    /// What produced this hop.
    pub provenance: Provenance,
    /// Call nesting: 0 = in the method the finding is reported in; k+1 = a
    /// step spliced from inside a callee whose summary lifted the taint at
    /// depth k.
    pub depth: u32,
}

impl Step {
    fn intra(code: &str, line: Option<u32>, depth: u32) -> Step {
        Step { code: code.to_string(), line, provenance: Provenance::IntraProc, depth }
    }
}

/// The provenance of a tainted value: where it originated and the chain of
/// expressions that carried it. Cloned as taint propagates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Trace {
    pub origin: String,
    pub steps: Vec<Step>,
}

impl Trace {
    fn extend(&self, code: &str, line: Option<u32>, provenance: Provenance, depth: u32) -> Trace {
        let mut steps = self.steps.clone();
        steps.push(Step { code: code.to_string(), line, provenance, depth });
        Trace { origin: self.origin.clone(), steps }
    }

    /// Append pre-built steps (a callee's internal chain) before a hop.
    fn splice(&self, inner: Vec<Step>) -> Trace {
        let mut steps = self.steps.clone();
        steps.extend(inner);
        Trace { origin: self.origin.clone(), steps }
    }
}

/// A source→sink flow found in one method, with a witness path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub method: String,
    pub sink: String,
    pub sink_line: Option<u32>,
    /// The source that tainted the value.
    pub origin: String,
    /// The witness: source expression → … → sink, each with its line, the
    /// provenance that produced the hop, and a callee-nesting depth.
    pub path: Vec<Step>,
}

/// Shared, immutable state for one `find_flows` run.
struct Ctx<'a> {
    cpg: &'a Cpg,
    summaries: &'a SummaryStore,
    spec: &'a TaintSpec,
    /// name -> defining method nodes, for locating callee bodies to splice.
    methods_by_name: HashMap<String, Vec<NodeId>>,
}

impl Ctx<'_> {
    /// Query-time sanitizers = the spec's plus whatever the summary store was
    /// computed with (so both walkers agree on what neutralises taint).
    fn is_sanitizer(&self, name: &str) -> bool {
        self.spec.sanitizers.contains(name) || self.summaries.sanitizer_names().contains(name)
    }

    /// The body of the callee a computed summary describes, if present.
    fn body_of(&self, name: &str, fqn: &str) -> Option<NodeId> {
        let candidates = self.methods_by_name.get(name)?;
        candidates
            .iter()
            .find(|&&m| self.cpg.full_name_of(m) == Some(fqn))
            .or_else(|| candidates.first())
            .copied()
    }
}

/// Run the taint query across every method, returning all findings.
pub fn find_flows(cpg: &Cpg, summaries: &SummaryStore, spec: &TaintSpec) -> Vec<Finding> {
    let ctx = Ctx { cpg, summaries, spec, methods_by_name: method_name_index(cpg) };
    let mut findings = Vec::new();
    for m in cpg.methods() {
        analyse_method(&ctx, m, &mut findings);
    }
    findings
}

fn analyse_method(ctx: &Ctx, method: NodeId, out: &mut Vec<Finding>) {
    let cpg = ctx.cpg;
    let method_name = cpg.full_name_of(method).unwrap_or("<anon>").to_string();

    // Tainted variable names, each carrying the provenance that tainted it.
    let mut taint: HashMap<String, Trace> = HashMap::new();

    let mut stmts: Vec<NodeId> = crate::pass::ast_descendants(cpg, method)
        .into_iter()
        .filter(|&n| matches!(cpg.kind_of(n), NodeKind::Call | NodeKind::Return))
        .filter(|&n| cpg.line_of(n).is_some())
        .collect();
    stmts.sort_by_key(|&n| (cpg.line_of(n).unwrap_or(0), n.0));

    for n in stmts {
        if cpg.kind_of(n) == NodeKind::Call && cpg.name_of(n) == Some("=") {
            let args = cpg.arguments_of(n);
            if args.len() == 2 {
                if let Some(trace) = expr_taint(ctx, args[1], &taint) {
                    if let Some(name) = lhs_name(cpg, args[0]) {
                        let trace = trace.extend(
                            cpg.code_of(n).unwrap_or(&name),
                            cpg.line_of(n),
                            Provenance::IntraProc,
                            0,
                        );
                        taint.insert(name, trace);
                    }
                } else if let Some(name) = lhs_name(cpg, args[0]) {
                    taint.remove(&name); // reassignment clears taint
                }
            }
        }
        // Any call (including the assignment's rhs) may be a sink.
        check_sinks(ctx, n, &taint, &method_name, out);
    }
}

/// If `node` (or a nested call) is a sink reached by a tainted argument, record it.
fn check_sinks(
    ctx: &Ctx,
    node: NodeId,
    taint: &HashMap<String, Trace>,
    method_name: &str,
    out: &mut Vec<Finding>,
) {
    let cpg = ctx.cpg;
    if cpg.kind_of(node) != NodeKind::Call {
        return;
    }
    let name = cpg.name_of(node).unwrap_or("");
    if ctx.spec.sinks.contains(name) {
        for arg in cpg.arguments_of(node) {
            if let Some(trace) = expr_taint(ctx, arg, taint) {
                let path = trace
                    .extend(
                        cpg.code_of(node).unwrap_or(name),
                        cpg.line_of(node),
                        Provenance::IntraProc,
                        0,
                    )
                    .steps;
                out.push(Finding {
                    method: method_name.to_string(),
                    sink: name.to_string(),
                    sink_line: cpg.line_of(node),
                    origin: trace.origin,
                    path,
                });
                break;
            }
        }
    }
    // Recurse into argument subtrees so nested sinks are caught.
    for arg in cpg.arguments_of(node) {
        check_sinks(ctx, arg, taint, method_name, out);
    }
}

/// Returns `Some(trace)` describing provenance if the expression is tainted.
fn expr_taint(ctx: &Ctx, node: NodeId, taint: &HashMap<String, Trace>) -> Option<Trace> {
    let cpg = ctx.cpg;
    match cpg.kind_of(node) {
        NodeKind::Identifier => cpg.name_of(node).and_then(|n| taint.get(n).cloned()),
        NodeKind::Literal => None,
        NodeKind::Call => {
            let name = cpg.name_of(node).unwrap_or("");
            let args = cpg.arguments_of(node);
            if ctx.is_sanitizer(name) {
                // A sanitizer's result never carries its arguments' taint.
                return None;
            }
            if ctx.spec.sources.contains(name) {
                return Some(Trace {
                    origin: name.to_string(),
                    steps: vec![Step::intra(
                        cpg.code_of(node).unwrap_or(name),
                        cpg.line_of(node),
                        0,
                    )],
                });
            }
            if is_operator(name) {
                // Operators taint their result if any operand is tainted.
                for a in &args {
                    if let Some(t) = expr_taint(ctx, *a, taint) {
                        return Some(t);
                    }
                }
                return None;
            }
            // Named callee: result is tainted iff a tainted argument flows to
            // the return RAW per the callee's summary (sanitized flows are
            // not lifted). Splice the callee's internal chain when we have
            // its body; external summaries are recorded summary-only.
            let (summary, origin) = ctx.summaries.get_with_origin(name)?;
            let fqn = summary.fqn.clone();
            for k in summary.flows_to_return() {
                if let Some(a) = args.get(k) {
                    if let Some(t) = expr_taint(ctx, *a, taint) {
                        let mut visiting = HashSet::new();
                        match lift(ctx, name, &fqn, origin, k, &mut visiting) {
                            Some((inner, prov)) => {
                                return Some(t.splice(inner).extend(
                                    cpg.code_of(node).unwrap_or(name),
                                    cpg.line_of(node),
                                    prov,
                                    0,
                                ));
                            }
                            // The callee's only internal path for this flow
                            // goes through a sanitizer: not liftable.
                            None => continue,
                        }
                    }
                }
            }
            None
        }
        _ => {
            for c in cpg.out_kind(node, cpg_core::EdgeKind::Ast) {
                if let Some(t) = expr_taint(ctx, c, taint) {
                    return Some(t);
                }
            }
            None
        }
    }
}

/// Lift a raw summary flow (param `k` → return of `name`): decide the hop's
/// provenance and reconstruct the callee's internal witness steps.
///
/// Returns `None` when the callee body shows every param-k→return path goes
/// through a sanitizer (the flow must not be lifted); `Some((steps, prov))`
/// otherwise, where `steps` is empty for external/unlocatable/recursive
/// callees (summary-only hop).
fn lift(
    ctx: &Ctx,
    name: &str,
    fqn: &str,
    origin: SummaryOrigin,
    k: usize,
    visiting: &mut HashSet<String>,
) -> Option<(Vec<Step>, Provenance)> {
    match origin {
        SummaryOrigin::External => Some((
            Vec::new(),
            Provenance::ExternalSummary { callee_fqn: fqn.to_string() },
        )),
        SummaryOrigin::Computed => {
            let prov = Provenance::SummaryFlow { callee_fqn: fqn.to_string() };
            let Some(body) = ctx.body_of(name, fqn) else {
                return Some((Vec::new(), prov)); // no body located: summary-only hop
            };
            if !visiting.insert(fqn.to_string()) {
                return Some((Vec::new(), prov)); // recursion: don't re-expand
            }
            let chain = callee_chain(ctx, body, k, 1, visiting);
            visiting.remove(fqn);
            chain.map(|steps| (steps, prov))
        }
    }
}

/// Reconstruct the intraprocedural witness chain inside `method` that carries
/// its parameter `param_idx` to its return, honouring sanitizers. Steps are
/// marked with `depth`. Returns `None` when no sanitizer-free path exists
/// (i.e. the raw summary flow is only realisable through a sanitizer per the
/// query's sanitizer set); `Some(steps)` otherwise.
fn callee_chain(
    ctx: &Ctx,
    method: NodeId,
    param_idx: usize,
    depth: u32,
    visiting: &mut HashSet<String>,
) -> Option<Vec<Step>> {
    if depth > MAX_SPLICE_DEPTH {
        return Some(Vec::new()); // too deep: keep the hop, drop the expansion
    }
    let cpg = ctx.cpg;
    let params = cpg.parameters_of(method);
    let Some(&pnode) = params.get(param_idx) else {
        return Some(Vec::new()); // signature mismatch: summary-only hop
    };
    let pname = cpg.name_of(pnode)?.to_string();

    // var name -> witness chain from the parameter to that var.
    let mut chains: HashMap<String, Vec<Step>> = HashMap::new();
    chains.insert(
        pname.clone(),
        vec![Step::intra(cpg.code_of(pnode).unwrap_or(&pname), cpg.line_of(pnode), depth)],
    );

    let mut stmts: Vec<NodeId> = crate::pass::ast_descendants(cpg, method)
        .into_iter()
        .filter(|&n| matches!(cpg.kind_of(n), NodeKind::Call | NodeKind::Return))
        .filter(|&n| cpg.line_of(n).is_some())
        .collect();
    stmts.sort_by_key(|&n| (cpg.line_of(n).unwrap_or(0), n.0));

    for n in stmts {
        match cpg.kind_of(n) {
            NodeKind::Call if cpg.name_of(n) == Some("=") => {
                let args = cpg.arguments_of(n);
                if args.len() == 2 {
                    match chain_expr(ctx, args[1], &chains, depth, visiting) {
                        Some(mut c) => {
                            if let Some(lhs) = lhs_name(cpg, args[0]) {
                                c.push(Step::intra(
                                    cpg.code_of(n).unwrap_or(&lhs),
                                    cpg.line_of(n),
                                    depth,
                                ));
                                chains.insert(lhs, c);
                            }
                        }
                        None => {
                            if let Some(lhs) = lhs_name(cpg, args[0]) {
                                chains.remove(&lhs); // reassignment clears
                            }
                        }
                    }
                }
            }
            NodeKind::Return => {
                for c in cpg.out_kind(n, cpg_core::EdgeKind::Ast) {
                    if let Some(mut chain) = chain_expr(ctx, c, &chains, depth, visiting) {
                        chain.push(Step::intra(
                            cpg.code_of(n).unwrap_or("return"),
                            cpg.line_of(n),
                            depth,
                        ));
                        return Some(chain);
                    }
                }
            }
            _ => {}
        }
    }
    None // no sanitizer-free param -> return path found
}

/// The witness chain carrying the tracked parameter into `node`, if any.
/// Mirrors the summary walker's propagation rules (identifiers, literals,
/// operators, raw callee flows, wrapper nodes) with sanitizers killing.
fn chain_expr(
    ctx: &Ctx,
    node: NodeId,
    chains: &HashMap<String, Vec<Step>>,
    depth: u32,
    visiting: &mut HashSet<String>,
) -> Option<Vec<Step>> {
    let cpg = ctx.cpg;
    match cpg.kind_of(node) {
        NodeKind::Identifier => cpg.name_of(node).and_then(|n| chains.get(n).cloned()),
        NodeKind::Literal => None,
        NodeKind::Call => {
            let name = cpg.name_of(node).unwrap_or("");
            let args = cpg.arguments_of(node);
            if ctx.is_sanitizer(name) {
                return None; // sanitized inside the callee: path dies here
            }
            if is_operator(name) {
                for a in &args {
                    if let Some(c) = chain_expr(ctx, *a, chains, depth, visiting) {
                        return Some(c);
                    }
                }
                return None;
            }
            let (summary, origin) = ctx.summaries.get_with_origin(name)?;
            let fqn = summary.fqn.clone();
            for k in summary.flows_to_return() {
                if let Some(a) = args.get(k) {
                    if let Some(mut c) = chain_expr(ctx, *a, chains, depth, visiting) {
                        match lift_nested(ctx, name, &fqn, origin, k, depth, visiting) {
                            Some((inner, prov)) => {
                                c.extend(inner);
                                c.push(Step {
                                    code: cpg.code_of(node).unwrap_or(name).to_string(),
                                    line: cpg.line_of(node),
                                    provenance: prov,
                                    depth,
                                });
                                return Some(c);
                            }
                            None => continue, // that flow is sanitized inside
                        }
                    }
                }
            }
            None
        }
        _ => {
            for c in cpg.out_kind(node, cpg_core::EdgeKind::Ast) {
                if let Some(chain) = chain_expr(ctx, c, chains, depth, visiting) {
                    return Some(chain);
                }
            }
            None
        }
    }
}

/// `lift` for hops encountered *inside* a spliced callee: identical policy,
/// but internal steps land one level deeper than the current chain.
fn lift_nested(
    ctx: &Ctx,
    name: &str,
    fqn: &str,
    origin: SummaryOrigin,
    k: usize,
    depth: u32,
    visiting: &mut HashSet<String>,
) -> Option<(Vec<Step>, Provenance)> {
    match origin {
        SummaryOrigin::External => Some((
            Vec::new(),
            Provenance::ExternalSummary { callee_fqn: fqn.to_string() },
        )),
        SummaryOrigin::Computed => {
            let prov = Provenance::SummaryFlow { callee_fqn: fqn.to_string() };
            let Some(body) = ctx.body_of(name, fqn) else {
                return Some((Vec::new(), prov));
            };
            if !visiting.insert(fqn.to_string()) {
                return Some((Vec::new(), prov));
            }
            let chain = callee_chain(ctx, body, k, depth + 1, visiting);
            visiting.remove(fqn);
            chain.map(|steps| (steps, prov))
        }
    }
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

    fn summarise(cpg: &Cpg) -> SummaryStore {
        let mut store = SummaryStore::new();
        store.compute_all(cpg);
        store
    }

    #[test]
    fn sanitizer_kills_finding_but_raw_path_still_reports() {
        // `u` is laundered through clean() — must NOT be reported; `t` flows
        // raw into the second sink — MUST be reported. One program, both cases.
        let cpg = build(&[(
            "v.c",
            "void h() {\n    char* t = getenv(\"X\");\n    char* u = clean(t);\n    system(u);\n    system(t);\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::with_sanitizers(&["getenv"], &["system"], &["clean"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "only the raw path may report: {findings:?}");
        assert_eq!(findings[0].sink, "system");
        assert!(
            findings[0].path.last().unwrap().code.contains("system(t)"),
            "the surviving finding must be the raw one: {:?}",
            findings[0].path
        );

        // Without the sanitizer configured, clean() has no summary at all, so
        // still only the raw path reports — but with a passthrough summary for
        // clean and no sanitizer marking, both would report. Prove the
        // sanitizer (not summary absence) is what kills the laundered path:
        let mut store2 = SummaryStore::new();
        store2
            .load_external_json(
                r#"[{"functionDeclaration":{"language":"C","methodName":"clean"},
                     "dataFlows":[{"from":"param0","to":"return"}]}]"#,
            )
            .unwrap();
        store2.compute_all(&cpg);
        let no_san = TaintSpec::new(&["getenv"], &["system"]);
        assert_eq!(find_flows(&cpg, &store2, &no_san).len(), 2, "both paths report without sanitizer");
        let with_san = TaintSpec::with_sanitizers(&["getenv"], &["system"], &["clean"]);
        assert_eq!(find_flows(&cpg, &store2, &with_san).len(), 1, "sanitizer kills the laundered path");
    }

    #[test]
    fn sanitized_callee_summary_does_not_propagate() {
        // wrap()'s summary is param0 -> return VIA escape (sanitized) because
        // the store knows `escape` is a sanitizer. The lift must not happen.
        let cpg = build(&[(
            "w.c",
            "char* wrap(char* s) {\n    return escape(s);\n}\nvoid h() {\n    char* t = getenv(\"X\");\n    system(wrap(t));\n}\n",
        )]);
        let mut store = SummaryStore::new();
        store.set_sanitizers(["escape"]);
        store.compute_all(&cpg);
        let spec = TaintSpec::with_sanitizers(&["getenv"], &["system"], &["escape"]);
        assert_eq!(
            find_flows(&cpg, &store, &spec).len(),
            0,
            "a sanitized summary flow must not lift raw taint"
        );
    }

    #[test]
    fn spec_only_sanitizer_inside_callee_kills_via_chain_recheck() {
        // The store computed wrap's summary WITHOUT knowing `clean` is a
        // sanitizer (clean has a raw external passthrough summary), so wrap's
        // flow looks raw. The query-time spec names `clean` a sanitizer; the
        // callee-chain recheck must discover the path is sanitizer-only and
        // kill the finding.
        let cpg = build(&[(
            "x.c",
            "char* wrap(char* s) {\n    return clean(s);\n}\nvoid h() {\n    char* t = getenv(\"X\");\n    system(wrap(t));\n}\n",
        )]);
        let mut store = SummaryStore::new();
        store
            .load_external_json(
                r#"[{"functionDeclaration":{"language":"C","methodName":"clean"},
                     "dataFlows":[{"from":"param0","to":"return"}]}]"#,
            )
            .unwrap();
        store.compute_all(&cpg);
        // Sanity: without the sanitizer the flow reports.
        assert_eq!(find_flows(&cpg, &store, &TaintSpec::new(&["getenv"], &["system"])).len(), 1);
        // With it, the only path is through clean() inside wrap(): killed.
        let spec = TaintSpec::with_sanitizers(&["getenv"], &["system"], &["clean"]);
        assert_eq!(find_flows(&cpg, &store, &spec).len(), 0);
    }

    #[test]
    fn witness_includes_callee_internal_steps_with_provenance() {
        let cpg = build(&[(
            "y.c",
            "char* wrap(char* s) {\n    char* r = s;\n    return r;\n}\nvoid h() {\n    char* t = getenv(\"X\");\n    system(wrap(t));\n}\n",
        )]);
        let store = summarise(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let path = &findings[0].path;

        // Ends run in the reporting method at depth 0.
        assert!(path.first().unwrap().code.contains("getenv"));
        assert_eq!(path.first().unwrap().depth, 0);
        assert!(path.last().unwrap().code.contains("system"));
        assert_eq!(path.last().unwrap().depth, 0);

        // The callee's internal chain is spliced in at depth 1.
        assert!(
            path.iter().any(|s| s.depth == 1 && s.code.contains("r = s")),
            "expected wrap's internal assignment in the witness: {path:?}"
        );
        assert!(
            path.iter().any(|s| s.depth == 1 && s.code.contains("return r")),
            "expected wrap's return in the witness: {path:?}"
        );

        // The hop through wrap carries computed-summary provenance at depth 0.
        assert!(
            path.iter().any(|s| s.depth == 0
                && s.provenance == Provenance::SummaryFlow { callee_fqn: "wrap".into() }),
            "expected a SummaryFlow hop for wrap: {path:?}"
        );
        // Internal steps are ordered between the source and the hop.
        let src = path.iter().position(|s| s.code.contains("getenv")).unwrap();
        let internal = path.iter().position(|s| s.code.contains("r = s")).unwrap();
        let hop = path
            .iter()
            .position(|s| matches!(s.provenance, Provenance::SummaryFlow { .. }))
            .unwrap();
        assert!(src < internal && internal < hop, "splice order wrong: {path:?}");
    }

    #[test]
    fn external_summary_hop_is_marked_summary_only() {
        let cpg = build(&[(
            "z.c",
            "void h() {\n    char* t = getenv(\"X\");\n    system(strdup(t));\n}\n",
        )]);
        let mut store = SummaryStore::new();
        store
            .load_external_json(
                r#"[{"functionDeclaration":{"language":"C","methodName":"strdup"},
                     "dataFlows":[{"from":"param0","to":"return"}]}]"#,
            )
            .unwrap();
        store.compute_all(&cpg);
        let spec = TaintSpec::new(&["getenv"], &["system"]);
        let findings = find_flows(&cpg, &store, &spec);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let path = &findings[0].path;
        assert!(
            path.iter().any(|s| s.provenance
                == Provenance::ExternalSummary { callee_fqn: "strdup".into() }),
            "expected an ExternalSummary hop: {path:?}"
        );
        // External summaries have no body: nothing spliced below depth 0.
        assert!(
            path.iter().all(|s| s.depth == 0),
            "external hops must be summary-only: {path:?}"
        );
    }

    #[test]
    fn intraproc_steps_carry_intraproc_provenance() {
        let cpg = build(&[(
            "p.c",
            "void h() {\n    char* t = getenv(\"X\");\n    system(t);\n}\n",
        )]);
        let store = summarise(&cpg);
        let findings = find_flows(&cpg, &store, &TaintSpec::new(&["getenv"], &["system"]));
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0]
                .path
                .iter()
                .all(|s| s.provenance == Provenance::IntraProc && s.depth == 0),
            "pure intraprocedural flow: {:?}",
            findings[0].path
        );
    }

    #[test]
    fn nested_lift_splices_two_levels() {
        // h -> outer -> inner: the witness must contain inner's steps at
        // depth 2 and outer's at depth 1, each hop with its own provenance.
        let cpg = build(&[(
            "n.c",
            "char* inner(char* a) {\n    return a;\n}\nchar* outer(char* b) {\n    return inner(b);\n}\nvoid h() {\n    char* t = getenv(\"X\");\n    system(outer(t));\n}\n",
        )]);
        let store = summarise(&cpg);
        let findings = find_flows(&cpg, &store, &TaintSpec::new(&["getenv"], &["system"]));
        assert_eq!(findings.len(), 1, "{findings:?}");
        let path = &findings[0].path;
        assert!(
            path.iter().any(|s| s.depth == 2 && s.code.contains("return a")),
            "inner's return should appear at depth 2: {path:?}"
        );
        assert!(
            path.iter().any(|s| s.depth == 1
                && s.provenance == Provenance::SummaryFlow { callee_fqn: "inner".into() }),
            "the inner() hop inside outer should be at depth 1: {path:?}"
        );
        assert!(
            path.iter().any(|s| s.depth == 0
                && s.provenance == Provenance::SummaryFlow { callee_fqn: "outer".into() }),
            "the outer() hop should be at depth 0: {path:?}"
        );
    }

    #[test]
    fn provenance_serializes() {
        let step = Step {
            code: "wrap(t)".into(),
            line: Some(3),
            provenance: Provenance::SummaryFlow { callee_fqn: "wrap".into() },
            depth: 0,
        };
        let js = serde_json::to_value(&step).unwrap();
        assert_eq!(js["provenance"]["SummaryFlow"]["callee_fqn"], "wrap");
        assert_eq!(js["depth"], 0);
    }
}
