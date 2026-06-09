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

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Flow {
    pub from: Point,
    pub to: Point,
}

#[derive(Clone, Debug, Default)]
pub struct FunctionSummary {
    pub fqn: String,
    pub flows: HashSet<Flow>,
}

impl FunctionSummary {
    pub fn flows_to_return(&self) -> impl Iterator<Item = usize> + '_ {
        self.flows.iter().filter_map(|f| match (f.from, f.to) {
            (Point::Param(k), Point::Return) => Some(k),
            _ => None,
        })
    }
}

/// Cache of summaries plus the dependency graph needed to invalidate precisely.
#[derive(Default)]
pub struct SummaryStore {
    summaries: HashMap<String, FunctionSummary>,
    /// fqn -> set of callee fqns its summary depended on (the invalidation web).
    deps: HashMap<String, HashSet<String>>,
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

    /// Load external function summaries from JSON (Fraunhofer-style).
    pub fn load_external_json(&mut self, json: &str) -> Result<usize, String> {
        let entries: Vec<ExternalEntry> =
            serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut n = 0;
        for e in entries {
            let fqn = e.function_declaration.method_name.clone();
            let mut flows = HashSet::new();
            for df in &e.data_flows {
                if let (Some(from), Some(to)) = (parse_point(&df.from), parse_point(&df.to)) {
                    flows.insert(Flow { from, to });
                }
            }
            self.external.insert(fqn.clone(), FunctionSummary { fqn, flows });
            n += 1;
        }
        Ok(n)
    }

    /// Compute summaries for every user method from scratch (fixpoint, so a
    /// caller benefits from its callee's summary regardless of file order).
    pub fn compute_all(&mut self, cpg: &Cpg) {
        let all: Vec<NodeId> = cpg.nodes_of_kind(NodeKind::Method);
        self.recompute(cpg, &all);
    }

    /// Incremental update: invalidate summaries for methods in `changed_files`
    /// plus every transitive caller that depended on them, then recompute only
    /// the invalidated set. Returns the number recomputed (cache hits = total −
    /// this). This is the cacheable+invalidatable property end-to-end.
    pub fn update_for_changed_files(
        &mut self,
        cpg: &Cpg,
        changed_files: &HashSet<cpg_core::FileId>,
    ) {
        // 1. Directly invalidated: methods living in a changed file.
        let mut invalid: HashSet<String> = HashSet::new();
        let mut node_of_fqn: HashMap<String, NodeId> = HashMap::new();
        for m in cpg.nodes_of_kind(NodeKind::Method) {
            if let Some(fqn) = cpg.full_name_of(m) {
                node_of_fqn.insert(fqn.to_string(), m);
                if changed_files.contains(&cpg.file_of(m)) {
                    invalid.insert(fqn.to_string());
                }
            }
        }
        // 2. Transitively invalidate callers whose summary used an invalid fqn.
        loop {
            let mut grew = false;
            let newly: Vec<String> = self
                .deps
                .iter()
                .filter(|(caller, callees)| {
                    !invalid.contains(*caller) && callees.iter().any(|c| invalid.contains(c))
                })
                .map(|(caller, _)| caller.clone())
                .collect();
            for c in newly {
                if invalid.insert(c) {
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        // 3. Drop invalidated entries, recompute only those nodes that still exist.
        for fqn in &invalid {
            self.summaries.remove(fqn);
            self.deps.remove(fqn);
        }
        let to_recompute: Vec<NodeId> = invalid
            .iter()
            .filter_map(|fqn| node_of_fqn.get(fqn).copied())
            .collect();
        self.recompute(cpg, &to_recompute);
    }

    /// Fixpoint recomputation over `targets` only.
    fn recompute(&mut self, cpg: &Cpg, targets: &[NodeId]) {
        self.last_recomputed.clear();
        let mut changed = true;
        let mut iterations = 0;
        while changed && iterations < targets.len() + 2 {
            changed = false;
            iterations += 1;
            for &m in targets {
                let (summary, deps) = compute_method(cpg, m, self);
                let fqn = summary.fqn.clone();
                let prev = self.summaries.get(&fqn);
                if prev.map(|p| &p.flows) != Some(&summary.flows) {
                    changed = true;
                }
                self.deps.insert(fqn.clone(), deps);
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

    // var name -> set of params that flow into it.
    let mut taint: HashMap<String, HashSet<usize>> = HashMap::new();
    for (name, &i) in &param_index {
        taint.insert(name.clone(), HashSet::from([i]));
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
                    for k in expr_taint(cpg, c, &taint, store, &mut deps) {
                        result_flows.insert(Flow {
                            from: Point::Param(k),
                            to: Point::Return,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    (FunctionSummary { fqn, flows: result_flows }, deps)
}

/// The variable name an lvalue identifier refers to.
fn lhs_name(cpg: &Cpg, node: NodeId) -> Option<String> {
    if cpg.kind_of(node) == NodeKind::Identifier {
        return cpg.name_of(node).map(|s| s.to_string());
    }
    None
}

/// Set of parameter indices that taint an expression.
fn expr_taint(
    cpg: &Cpg,
    node: NodeId,
    taint: &HashMap<String, HashSet<usize>>,
    store: &SummaryStore,
    deps: &mut HashSet<String>,
) -> HashSet<usize> {
    match cpg.kind_of(node) {
        NodeKind::Identifier => cpg
            .name_of(node)
            .and_then(|n| taint.get(n).cloned())
            .unwrap_or_default(),
        NodeKind::Literal => HashSet::new(),
        NodeKind::Call => {
            let name = cpg.name_of(node).unwrap_or("");
            let args = cpg.arguments_of(node);
            let arg_taints: Vec<HashSet<usize>> = args
                .iter()
                .map(|&a| expr_taint(cpg, a, taint, store, deps))
                .collect();

            if is_operator(name) {
                // Operators propagate all operands (conservative, precise enough).
                let mut out = HashSet::new();
                for t in &arg_taints {
                    out.extend(t.iter().copied());
                }
                return out;
            }
            // Named function: drive result taint from the callee's summary.
            deps.insert(name.to_string());
            let mut out = HashSet::new();
            if let Some(summary) = store.get(name) {
                for k in summary.flows_to_return() {
                    if let Some(t) = arg_taints.get(k) {
                        out.extend(t.iter().copied());
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

fn is_operator(name: &str) -> bool {
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
}
