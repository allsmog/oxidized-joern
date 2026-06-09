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
//! that parameter to its return. A finding is raised when a tainted value
//! reaches an argument of a configured sink.

use crate::summaries::{is_operator, lhs_name, SummaryStore};
use cpg_core::{Cpg, NodeId, NodeKind, Query};
use std::collections::{HashMap, HashSet};

/// What counts as a source and a sink, by function name.
pub struct TaintSpec {
    /// Calls to these names produce tainted values (their return is tainted).
    pub sources: HashSet<String>,
    /// Calls to these names are dangerous; a tainted argument is a finding.
    pub sinks: HashSet<String>,
}

impl TaintSpec {
    pub fn new(sources: &[&str], sinks: &[&str]) -> Self {
        TaintSpec {
            sources: sources.iter().map(|s| s.to_string()).collect(),
            sinks: sinks.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// One step along a taint witness (a tainted expression and where it occurs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub code: String,
    pub line: Option<u32>,
}

/// The provenance of a tainted value: where it originated and the chain of
/// expressions that carried it. Cloned as taint propagates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trace {
    pub origin: String,
    pub steps: Vec<Step>,
}

impl Trace {
    fn extend(&self, code: &str, line: Option<u32>) -> Trace {
        let mut steps = self.steps.clone();
        steps.push(Step { code: code.to_string(), line });
        Trace { origin: self.origin.clone(), steps }
    }
}

/// A source→sink flow found in one method, with a witness path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub method: String,
    pub sink: String,
    pub sink_line: Option<u32>,
    /// The source that tainted the value.
    pub origin: String,
    /// The witness: source expression → … → sink, each with its line.
    pub path: Vec<Step>,
}

/// Run the taint query across every method, returning all findings.
pub fn find_flows(cpg: &Cpg, summaries: &SummaryStore, spec: &TaintSpec) -> Vec<Finding> {
    let mut findings = Vec::new();
    for m in cpg.methods() {
        analyse_method(cpg, summaries, spec, m, &mut findings);
    }
    findings
}

fn analyse_method(
    cpg: &Cpg,
    summaries: &SummaryStore,
    spec: &TaintSpec,
    method: NodeId,
    out: &mut Vec<Finding>,
) {
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
                if let Some(trace) = expr_taint(cpg, summaries, spec, args[1], &taint) {
                    if let Some(name) = lhs_name(cpg, args[0]) {
                        let trace = trace.extend(cpg.code_of(n).unwrap_or(&name), cpg.line_of(n));
                        taint.insert(name, trace);
                    }
                } else if let Some(name) = lhs_name(cpg, args[0]) {
                    taint.remove(&name); // reassignment clears taint
                }
            }
        }
        // Any call (including the assignment's rhs) may be a sink.
        check_sinks(cpg, summaries, spec, n, &taint, &method_name, out);
    }
}

/// If `node` (or a nested call) is a sink reached by a tainted argument, record it.
fn check_sinks(
    cpg: &Cpg,
    summaries: &SummaryStore,
    spec: &TaintSpec,
    node: NodeId,
    taint: &HashMap<String, Trace>,
    method_name: &str,
    out: &mut Vec<Finding>,
) {
    if cpg.kind_of(node) != NodeKind::Call {
        return;
    }
    let name = cpg.name_of(node).unwrap_or("");
    if spec.sinks.contains(name) {
        for arg in cpg.arguments_of(node) {
            if let Some(trace) = expr_taint(cpg, summaries, spec, arg, taint) {
                let path = trace
                    .extend(cpg.code_of(node).unwrap_or(name), cpg.line_of(node))
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
        check_sinks(cpg, summaries, spec, arg, taint, method_name, out);
    }
}

/// Returns `Some(trace)` describing provenance if the expression is tainted.
fn expr_taint(
    cpg: &Cpg,
    summaries: &SummaryStore,
    spec: &TaintSpec,
    node: NodeId,
    taint: &HashMap<String, Trace>,
) -> Option<Trace> {
    match cpg.kind_of(node) {
        NodeKind::Identifier => cpg.name_of(node).and_then(|n| taint.get(n).cloned()),
        NodeKind::Literal => None,
        NodeKind::Call => {
            let name = cpg.name_of(node).unwrap_or("");
            let args = cpg.arguments_of(node);
            if spec.sources.contains(name) {
                return Some(Trace {
                    origin: name.to_string(),
                    steps: vec![Step {
                        code: cpg.code_of(node).unwrap_or(name).to_string(),
                        line: cpg.line_of(node),
                    }],
                });
            }
            if is_operator(name) {
                // Operators taint their result if any operand is tainted.
                for a in &args {
                    if let Some(t) = expr_taint(cpg, summaries, spec, *a, taint) {
                        return Some(t);
                    }
                }
                return None;
            }
            // Named callee: result is tainted iff a tainted argument flows to
            // the return per the callee's summary. Record the call as a hop.
            if let Some(summary) = summaries.get(name) {
                for k in summary.flows_to_return() {
                    if let Some(a) = args.get(k) {
                        if let Some(t) = expr_taint(cpg, summaries, spec, *a, taint) {
                            return Some(t.extend(cpg.code_of(node).unwrap_or(name), cpg.line_of(node)));
                        }
                    }
                }
            }
            None
        }
        _ => {
            for c in cpg.out_kind(node, cpg_core::EdgeKind::Ast) {
                if let Some(t) = expr_taint(cpg, summaries, spec, c, taint) {
                    return Some(t);
                }
            }
            None
        }
    }
}
