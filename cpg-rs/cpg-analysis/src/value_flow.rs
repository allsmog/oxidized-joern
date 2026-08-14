//! Sparse value-flow graph view.
//!
//! This is the analysis-facing view over the parity-validated
//! [`EdgeKind::ReachingDef`] layer. Interprocedural edges are derived from
//! resolved call targets and raw function-summary flows; no second DDG dialect
//! is consulted.

use crate::SummaryStore;
use cpg_core::{Cpg, EdgeKind, NodeId};
use cpg_core::{NodeKind, Query};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ValueFlowKind {
    DataDependence,
    Summary,
    External,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ValueFlowEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: ValueFlowKind,
}

#[derive(Clone, Debug, Default)]
pub struct SparseValueFlow {
    out: HashMap<NodeId, Vec<ValueFlowEdge>>,
    incoming: HashMap<NodeId, Vec<ValueFlowEdge>>,
}

impl SparseValueFlow {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_reaching_defs(cpg: &Cpg) -> Self {
        let mut graph = SparseValueFlow::new();
        for n in cpg.nodes() {
            for dst in cpg.out_kind(n, EdgeKind::ReachingDef) {
                graph.add_edge(n, dst, ValueFlowKind::DataDependence);
            }
        }
        graph
    }

    /// Build canonical intra- and interprocedural value flow. Call bridges are
    /// justified by resolved targets and raw summary facts:
    ///
    /// - call argument -> matching formal parameter, for flows into a callee;
    /// - call argument -> call result, only for declared param-to-return flow;
    /// - method return -> call result when the callee is known to return a
    ///   source call's value.
    pub fn from_cpg(cpg: &Cpg, summaries: &SummaryStore) -> Self {
        let mut graph = Self::from_reaching_defs(cpg);
        for call in cpg.calls() {
            let args = cpg.arguments_of(call);
            let targets = cpg.call_targets(call);
            if targets.is_empty() {
                if let Some(name) = cpg.name_of(call) {
                    if let Some(summary) = summaries.get(name) {
                        for k in summary.flows_to_return() {
                            if let Some(&arg) = args.get(k) {
                                graph.add_edge(arg, call, ValueFlowKind::External);
                            }
                        }
                    }
                }
                continue;
            }
            for target in targets {
                let params = cpg.parameters_of(target);
                for (&arg, &param) in args.iter().zip(&params) {
                    graph.add_edge(arg, param, ValueFlowKind::Summary);
                }
                let Some(fqn) = cpg.full_name_of(target) else {
                    continue;
                };
                let Some(summary) = summaries.get(fqn) else {
                    continue;
                };
                for k in summary.flows_to_return() {
                    if let Some(&arg) = args.get(k) {
                        graph.add_edge(arg, call, ValueFlowKind::Summary);
                    }
                }
                if !summary.call_returns.is_empty() {
                    for ret in cpg
                        .out_kind(target, EdgeKind::Ast)
                        .filter(|&node| cpg.kind_of(node) == NodeKind::MethodReturn)
                    {
                        graph.add_edge(ret, call, ValueFlowKind::Summary);
                    }
                }
            }
        }
        graph
    }

    pub fn add_edge(&mut self, from: NodeId, to: NodeId, kind: ValueFlowKind) {
        let edge = ValueFlowEdge { from, to, kind };
        let outgoing = self.out.entry(from).or_default();
        if outgoing.contains(&edge) {
            return;
        }
        outgoing.push(edge);
        self.incoming.entry(to).or_default().push(edge);
    }

    pub fn outgoing(&self, n: NodeId) -> &[ValueFlowEdge] {
        self.out.get(&n).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn incoming(&self, n: NodeId) -> &[ValueFlowEdge] {
        self.incoming.get(&n).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Demand-driven reverse reachability from target nodes to origin nodes.
    pub fn reverse_reachable(
        &self,
        origins: &HashSet<NodeId>,
        targets: &[NodeId],
    ) -> HashSet<NodeId> {
        let mut reached = HashSet::new();
        let mut q: VecDeque<NodeId> = targets.iter().copied().collect();
        while let Some(n) = q.pop_front() {
            if !reached.insert(n) {
                continue;
            }
            if origins.contains(&n) {
                continue;
            }
            for edge in self.incoming(n) {
                q.push_back(edge.from);
            }
        }
        reached
    }

    /// Whether any origin reaches any target without traversing a blocked
    /// node. Used by security queries to enforce sanitizer cuts over the
    /// canonical graph.
    pub fn reaches_avoiding(
        &self,
        origins: &HashSet<NodeId>,
        targets: &HashSet<NodeId>,
        blocked: &HashSet<NodeId>,
    ) -> bool {
        let mut seen = HashSet::new();
        let mut queue: VecDeque<NodeId> = origins
            .iter()
            .copied()
            .filter(|node| !blocked.contains(node))
            .collect();
        while let Some(node) = queue.pop_front() {
            if !seen.insert(node) {
                continue;
            }
            if targets.contains(&node) {
                return true;
            }
            for edge in self.outgoing(node) {
                if !blocked.contains(&edge.to) {
                    queue.push_back(edge.to);
                }
            }
        }
        false
    }
}
