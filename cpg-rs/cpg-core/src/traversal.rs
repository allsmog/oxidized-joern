//! Minimal traversal/query surface over the CPG.
//!
//! This is the language-independent query API that the conformance suite and
//! (eventually) the server speak. It is intentionally small and composable:
//! find node sets by kind/name, then walk typed edges. A richer DSL would be
//! built on top of these primitives rather than replacing them.

use crate::graph::{Cpg, NodeId};
use crate::schema::{EdgeKind, NodeKind};

pub trait Query {
    fn nodes_of_kind(&self, kind: NodeKind) -> Vec<NodeId>;
    fn methods(&self) -> Vec<NodeId>;
    fn calls(&self) -> Vec<NodeId>;
    fn method_named(&self, name: &str) -> Vec<NodeId>;
    fn calls_named(&self, name: &str) -> Vec<NodeId>;
    /// Explicit source parameters, ordered by positive AST order.
    /// Joern-compatible implicit receiver parameters live at index zero and
    /// remain visible through raw AST/CPGQL traversal without shifting the
    /// positional mapping used by native analyses.
    fn parameters_of(&self, method: NodeId) -> Vec<NodeId>;
    fn arguments_of(&self, call: NodeId) -> Vec<NodeId>;
    fn call_target(&self, call: NodeId) -> Option<NodeId>;
    /// All resolved targets of a call. RPC stitching adds fan-out Call edges
    /// (one per candidate handler), so consumers that follow dataflow across
    /// call sites must walk every edge, not just the first.
    fn call_targets(&self, call: NodeId) -> Vec<NodeId>;
}

impl Query for Cpg {
    fn nodes_of_kind(&self, kind: NodeKind) -> Vec<NodeId> {
        self.nodes().filter(|&n| self.kind_of(n) == kind).collect()
    }

    fn methods(&self) -> Vec<NodeId> {
        self.nodes_of_kind(NodeKind::Method)
    }

    fn calls(&self) -> Vec<NodeId> {
        self.nodes_of_kind(NodeKind::Call)
    }

    fn method_named(&self, name: &str) -> Vec<NodeId> {
        self.methods()
            .into_iter()
            .filter(|&m| self.name_of(m) == Some(name) || self.full_name_of(m) == Some(name))
            .collect()
    }

    fn calls_named(&self, name: &str) -> Vec<NodeId> {
        self.calls()
            .into_iter()
            .filter(|&c| self.name_of(c) == Some(name))
            .collect()
    }

    fn parameters_of(&self, method: NodeId) -> Vec<NodeId> {
        let mut ps: Vec<NodeId> = self
            .out_kind(method, EdgeKind::Ast)
            .filter(|&n| self.kind_of(n) == NodeKind::MethodParameterIn && self.order_of(n) > 0)
            .collect();
        ps.sort_by_key(|&p| self.order_of(p));
        ps
    }

    fn arguments_of(&self, call: NodeId) -> Vec<NodeId> {
        let mut args: Vec<NodeId> = self.out_kind(call, EdgeKind::Argument).collect();
        args.sort_by_key(|&a| self.argument_index_of(a));
        args
    }

    fn call_target(&self, call: NodeId) -> Option<NodeId> {
        self.out_kind(call, EdgeKind::Call).next()
    }

    fn call_targets(&self, call: NodeId) -> Vec<NodeId> {
        self.out_kind(call, EdgeKind::Call).collect()
    }
}
