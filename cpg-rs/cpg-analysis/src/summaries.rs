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
//! The summary payload is deliberately richer than a bare param->return pair:
//! labels and provenance are carried on the summary so later engines (LLM
//! verdicts, sanitizer rule packs, IFDS summaries) can use the same cache and
//! invalidation machinery without changing the interprocedural seam again.

use crate::pass::ast_descendants;
use cpg_core::{Cpg, NodeId, NodeKind, Query};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

/// An endpoint of a flow within a function's signature.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Point {
    /// 0-based formal parameter.
    Param(usize),
    /// The method's return value.
    Return,
}

/// Qualifiers on a flow. Plain reachability is represented by an empty label
/// set. A sanitized flow is still a real value-flow edge, but downstream taint
/// queries may choose not to report it as exploitable.
#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum FlowLabel {
    /// The flow passed through a named sanitizer.
    Sanitized(String),
    /// The flow was added or refined by a model-backed summary producer.
    Model(String),
    /// Free-form deterministic note for rule packs and external summaries.
    Note(String),
}

impl FlowLabel {
    pub fn sanitized_by(&self) -> Option<&str> {
        match self {
            FlowLabel::Sanitized(name) => Some(name.as_str()),
            _ => None,
        }
    }

    pub fn as_string(&self) -> String {
        match self {
            FlowLabel::Sanitized(name) => format!("sanitized:{name}"),
            FlowLabel::Model(name) => format!("model:{name}"),
            FlowLabel::Note(note) => note.clone(),
        }
    }

    fn from_external(s: &str) -> Self {
        if let Some(name) = s.strip_prefix("sanitized:") {
            return FlowLabel::Sanitized(name.to_string());
        }
        if let Some(name) = s.strip_prefix("model:") {
            return FlowLabel::Model(name.to_string());
        }
        FlowLabel::Note(s.to_string())
    }
}

/// A flow between signature endpoints, plus deterministic labels describing how
/// the value is transformed along the way. Keeping the type compact preserves
/// the old summary-first API while leaving space for sanitizers, LLM verdicts,
/// and future taint-kind qualifiers.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Flow {
    pub from: Point,
    pub to: Point,
    pub labels: Vec<FlowLabel>,
}

impl Flow {
    pub fn plain(from: Point, to: Point) -> Self {
        Flow { from, to, labels: Vec::new() }
    }

    pub fn with_labels(from: Point, to: Point, labels: Vec<FlowLabel>) -> Self {
        Flow { from, to, labels: normalise_labels(labels) }
    }

    pub fn is_sanitized(&self) -> bool {
        self.labels.iter().any(|l| l.sanitized_by().is_some())
    }

    pub fn label_strings(&self) -> Vec<String> {
        self.labels.iter().map(FlowLabel::as_string).collect()
    }
}

/// How a summary was produced. This is intentionally deterministic metadata:
/// the fixpoint compares flow sets, while provenance is used for audit trails
/// and finding explanations.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SummaryProvenance {
    StaticAnalysis { engine: String },
    External { source: String },
    Model { model: String, prompt_hash: String, cache_key: String },
}

impl SummaryProvenance {
    pub fn note(&self) -> String {
        match self {
            SummaryProvenance::StaticAnalysis { engine } => format!("static:{engine}"),
            SummaryProvenance::External { source } => format!("external:{source}"),
            SummaryProvenance::Model { model, prompt_hash, cache_key } => {
                format!("model:{model}:prompt={prompt_hash}:cache={cache_key}")
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FunctionSummary {
    pub fqn: String,
    pub flows: HashSet<Flow>,
    pub provenance: Vec<SummaryProvenance>,
}

impl FunctionSummary {
    /// Backwards-compatible helper for existing callers: all params whose value
    /// can reach the return, regardless of labels.
    pub fn flows_to_return(&self) -> impl Iterator<Item = usize> + '_ {
        self.flows.iter().filter_map(|f| match (f.from, f.to) {
            (Point::Param(k), Point::Return) => Some(k),
            _ => None,
        })
    }

    /// Rich return flows, including labels such as `sanitized:<name>`.
    pub fn return_flows(&self) -> impl Iterator<Item = &Flow> {
        self.flows.iter().filter(|f| matches!(f.to, Point::Return))
    }

    pub fn provenance_notes(&self) -> Vec<String> {
        self.provenance.iter().map(SummaryProvenance::note).collect()
    }
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
    /// Diagnostic: how many method summaries were (re)computed in the last call.
    pub last_recomputed: HashSet<String>,
}

impl SummaryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, fqn: &str) -> Option<&FunctionSummary> {
        self.summaries.get(fqn).or_else(|| self.external.get(fqn))
    }

    pub fn len(&self) -> usize {
        self.summaries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.summaries.is_empty()
    }

    /// Load external function summaries from JSON (Fraunhofer-style, with
    /// backwards-compatible extensions):
    ///
    /// ```json
    /// [{"functionDeclaration":{"methodName":"escape"},
    ///   "dataFlows":[{"from":"param0","to":"return",
    ///                  "labels":["sanitized:escape"]}]}]
    /// ```
    pub fn load_external_json(&mut self, json: &str) -> Result<usize, String> {
        let entries: Vec<ExternalEntry> =
            serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut n = 0;
        for e in entries {
            let fqn = e.function_declaration.method_name.clone();
            let mut flows = HashSet::new();
            for df in &e.data_flows {
                if let (Some(from), Some(to)) = (parse_point(&df.from), parse_point(&df.to)) {
                    let mut labels: Vec<FlowLabel> =
                        df.labels.iter().map(|l| FlowLabel::from_external(l)).collect();
                    if let Some(sanitizer) = &df.sanitizer {
                        labels.push(FlowLabel::Sanitized(sanitizer.clone()));
                    }
                    if df.kind.as_deref() == Some("sanitized") {
                        labels.push(FlowLabel::Sanitized(fqn.clone()));
                    }
                    flows.insert(Flow::with_labels(from, to, labels));
                }
            }
            let source = e.provenance.unwrap_or_else(|| "external-json".to_string());
            self.external.insert(
                fqn.clone(),
                FunctionSummary {
                    fqn,
                    flows,
                    provenance: vec![SummaryProvenance::External { source }],
                },
            );
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

    /// Fixpoint recomputation over `targets` only. Each round computes all
    /// targets in parallel against a snapshot of the store (Jacobi iteration),
    /// then applies updates serially; rounds repeat until no summary changes.
    /// Convergence needs one round per level of the call-dependency chain.
    fn recompute(&mut self, cpg: &Cpg, targets: &[NodeId]) {
        use rayon::prelude::*;
        self.last_recomputed.clear();
        let mut changed = true;
        let mut iterations = 0;
        while changed && iterations < targets.len() + 2 {
            changed = false;
            iterations += 1;
            let results: Vec<(FunctionSummary, HashSet<String>)> = targets
                .par_iter()
                .map(|&m| compute_method(cpg, m, self))
                .collect();
            for (summary, deps) in results {
                let fqn = summary.fqn.clone();
                let prev = self.summaries.get(&fqn);
                if prev.map(|p| &p.flows) != Some(&summary.flows) {
                    changed = true;
                }
                self.set_deps(&fqn, deps);
                self.last_recomputed.insert(fqn.clone());
                self.summaries.insert(fqn, summary);
            }
        }
    }
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

    // var name -> set of params/labels that flow into it.
    let mut taint: HashMap<String, HashSet<ParamTaint>> = HashMap::new();
    for (name, &i) in &param_index {
        taint.insert(name.clone(), HashSet::from([ParamTaint::new(i)]));
    }

    let mut deps: HashSet<String> = HashSet::new();
    let mut result_flows: HashSet<Flow> = HashSet::new();

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
                    for t in expr_taint(cpg, c, &taint, store, &mut deps) {
                        result_flows.insert(Flow::with_labels(
                            Point::Param(t.param),
                            Point::Return,
                            t.labels,
                        ));
                    }
                }
            }
            _ => {}
        }
    }

    (
        FunctionSummary {
            fqn,
            flows: result_flows,
            provenance: vec![SummaryProvenance::StaticAnalysis {
                engine: "name-based-summary-v2".to_string(),
            }],
        },
        deps,
    )
}

/// The variable name an lvalue identifier refers to.
pub(crate) fn lhs_name(cpg: &Cpg, node: NodeId) -> Option<String> {
    if cpg.kind_of(node) == NodeKind::Identifier {
        return cpg.name_of(node).map(|s| s.to_string());
    }
    None
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct ParamTaint {
    param: usize,
    labels: Vec<FlowLabel>,
}

impl ParamTaint {
    fn new(param: usize) -> Self {
        ParamTaint { param, labels: Vec::new() }
    }

    fn with_added_labels(&self, labels: &[FlowLabel]) -> Self {
        let mut merged = self.labels.clone();
        merged.extend_from_slice(labels);
        ParamTaint { param: self.param, labels: normalise_labels(merged) }
    }
}

/// Set of parameter indices and labels that taint an expression.
fn expr_taint(
    cpg: &Cpg,
    node: NodeId,
    taint: &HashMap<String, HashSet<ParamTaint>>,
    store: &SummaryStore,
    deps: &mut HashSet<String>,
) -> HashSet<ParamTaint> {
    match cpg.kind_of(node) {
        NodeKind::Identifier => cpg
            .name_of(node)
            .and_then(|n| taint.get(n).cloned())
            .unwrap_or_default(),
        NodeKind::Literal => HashSet::new(),
        NodeKind::Call => {
            let name = cpg.name_of(node).unwrap_or("");
            let args = cpg.arguments_of(node);
            let arg_taints: Vec<HashSet<ParamTaint>> = args
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
            // Named function: drive result taint from the callee's summary.
            deps.insert(name.to_string());
            let mut out = HashSet::new();
            if let Some(summary) = store.get(name) {
                for flow in summary.return_flows() {
                    if let Point::Param(k) = flow.from {
                        if let Some(tset) = arg_taints.get(k) {
                            for t in tset {
                                out.insert(t.with_added_labels(&flow.labels));
                            }
                        }
                    }
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

fn normalise_labels(mut labels: Vec<FlowLabel>) -> Vec<FlowLabel> {
    labels.sort();
    labels.dedup();
    labels
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

// --- JSON shapes for external summaries (Fraunhofer-style, extended) ---
#[derive(Deserialize)]
struct ExternalEntry {
    #[serde(rename = "functionDeclaration")]
    function_declaration: ExternalDecl,
    #[serde(rename = "dataFlows", default)]
    data_flows: Vec<ExternalFlow>,
    #[serde(default)]
    provenance: Option<String>,
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
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    sanitizer: Option<String>,
    #[serde(default)]
    kind: Option<String>,
}
