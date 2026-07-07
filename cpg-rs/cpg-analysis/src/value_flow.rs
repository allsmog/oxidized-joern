//! Sparse value-flow graph view.
//!
//! This provides the analysis-facing abstraction needed to move taint away from
//! dense CFG walks. Today it can be built from existing DDG edges when present;
//! as the Joern-parity reaching-def builder is ported into `cpg-analysis`, this
//! becomes the primary dataflow substrate.

use cpg_core::{Cpg, EdgeKind, NodeId};
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
    pub fn new() -> Self { Self::default() }

    pub fn from_ddg(cpg: &Cpg) -> Self {
        let mut graph = SparseValueFlow::new();
        for n in cpg.nodes() {
            for dst in cpg.out_kind(n, EdgeKind::Ddg) {
                graph.add_edge(n, dst, ValueFlowKind::DataDependence);
            }
        }
        graph
    }

    pub fn add_edge(&mut self, from: NodeId, to: NodeId, kind: ValueFlowKind) {
        let edge = ValueFlowEdge { from, to, kind };
        self.out.entry(from).or_default().push(edge);
        self.incoming.entry(to).or_default().push(edge);
    }

    pub fn outgoing(&self, n: NodeId) -> &[ValueFlowEdge] {
        self.out.get(&n).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn incoming(&self, n: NodeId) -> &[ValueFlowEdge] {
        self.incoming.get(&n).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Demand-driven reverse reachability from target nodes to origin nodes.
    pub fn reverse_reachable(&self, origins: &HashSet<NodeId>, targets: &[NodeId]) -> HashSet<NodeId> {
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
}
