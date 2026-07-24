//! Frozen CSR view for query-heavy workloads.
//!
//! The mutable `Cpg` adjacency lists are the right representation while files are
//! being edited. A read-mostly query engine wants a compact, dense, cache-friendly
//! layout. `FrozenCpg` is the bridge: build it from the current mutable graph,
//! answer repeated edge scans from CSR arrays, and discard/rebuild it after an
//! edit invalidates the read snapshot.

use crate::{Cpg, EdgeKind, FileId, NodeId, NodeKind};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct FrozenNode {
    pub original: NodeId,
    pub kind: NodeKind,
    pub file: FileId,
}

#[derive(Clone, Debug)]
pub struct CsrEdges {
    /// offsets[i]..offsets[i + 1] indexes `targets` for dense node i.
    pub offsets: Vec<u32>,
    pub targets: Vec<u32>,
}

impl CsrEdges {
    pub fn empty(nodes: usize) -> Self {
        CsrEdges { offsets: vec![0; nodes + 1], targets: Vec::new() }
    }

    pub fn targets_from_dense(&self, dense: usize) -> &[u32] {
        let start = self.offsets[dense] as usize;
        let end = self.offsets[dense + 1] as usize;
        &self.targets[start..end]
    }
}

#[derive(Clone, Debug)]
pub struct FrozenCpg {
    pub nodes: Vec<FrozenNode>,
    dense_of: HashMap<NodeId, u32>,
    edges: HashMap<EdgeKind, CsrEdges>,
}

impl FrozenCpg {
    pub fn build(cpg: &Cpg) -> Self {
        let mut nodes = Vec::new();
        let mut dense_of = HashMap::new();
        for n in cpg.nodes() {
            let dense = nodes.len() as u32;
            dense_of.insert(n, dense);
            nodes.push(FrozenNode { original: n, kind: cpg.kind_of(n), file: cpg.file_of(n) });
        }

        let mut edges = HashMap::new();
        for kind in EdgeKind::ALL {
            let mut offsets = Vec::with_capacity(nodes.len() + 1);
            let mut targets = Vec::new();
            offsets.push(0);
            for node in &nodes {
                for edge in cpg.out(node.original) {
                    if edge.kind == kind {
                        if let Some(&dense_target) = dense_of.get(&edge.other) {
                            targets.push(dense_target);
                        }
                    }
                }
                offsets.push(targets.len() as u32);
            }
            edges.insert(kind, CsrEdges { offsets, targets });
        }

        FrozenCpg { nodes, dense_of, edges }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn dense_id(&self, n: NodeId) -> Option<u32> {
        self.dense_of.get(&n).copied()
    }

    pub fn original_id(&self, dense: u32) -> Option<NodeId> {
        self.nodes.get(dense as usize).map(|n| n.original)
    }

    pub fn out_dense(&self, dense: u32, kind: EdgeKind) -> &[u32] {
        self.edges
            .get(&kind)
            .map(|csr| csr.targets_from_dense(dense as usize))
            .unwrap_or(&[])
    }

    pub fn out(&self, n: NodeId, kind: EdgeKind) -> Vec<NodeId> {
        let Some(dense) = self.dense_id(n) else { return Vec::new() };
        self.out_dense(dense, kind)
            .iter()
            .filter_map(|&d| self.original_id(d))
            .collect()
    }
}

pub trait Freeze {
    fn freeze(&self) -> FrozenCpg;
}

impl Freeze for Cpg {
    fn freeze(&self) -> FrozenCpg {
        FrozenCpg::build(self)
    }
}
