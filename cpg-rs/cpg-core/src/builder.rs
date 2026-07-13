//! High-level construction primitives shared by every frontend.
//!
//! A frontend never pokes the columnar arrays directly; it calls these
//! builders. That indirection is the consolidation lever from the architecture
//! discussion: the rules for "what a Method node looks like" or "how a Call
//! wires its arguments" live here once, not re-implemented in twelve
//! frontends. Adding a language becomes "map your parser's nodes onto these
//! calls", which is a few thousand lines instead of tens of thousands.

use crate::graph::{Cpg, FileId, NodeId};
use crate::schema::{EdgeKind, NodeKind};

/// A builder scoped to a single file. Owns the `FileId` so frontends cannot
/// accidentally attribute nodes to the wrong incrementality unit.
pub struct CpgBuilder<'a> {
    pub cpg: &'a mut Cpg,
    pub file: FileId,
}

impl<'a> CpgBuilder<'a> {
    pub fn new(cpg: &'a mut Cpg, file: FileId) -> Self {
        CpgBuilder { cpg, file }
    }

    fn node(&mut self, kind: NodeKind) -> NodeId {
        self.cpg.add_node(kind, self.file)
    }

    /// AST containment edge.
    pub fn ast_child(&mut self, parent: NodeId, child: NodeId) {
        self.cpg.add_edge(parent, child, EdgeKind::Ast);
    }

    pub fn contains(&mut self, parent: NodeId, child: NodeId) {
        self.cpg.add_edge(parent, child, EdgeKind::Contains);
    }

    pub fn file_node(&mut self, path: &str) -> NodeId {
        let n = self.node(NodeKind::File);
        let s = self.cpg.intern(path);
        self.cpg.set_name(n, s);
        self.cpg.set_full_name(n, s);
        n
    }

    pub fn method(&mut self, name: &str, full_name: &str, signature: &str, line: Option<u32>) -> NodeId {
        let n = self.node(NodeKind::Method);
        let nm = self.cpg.intern(name);
        let fnm = self.cpg.intern(full_name);
        let sig = self.cpg.intern(signature);
        self.cpg.set_name(n, nm);
        self.cpg.set_full_name(n, fnm);
        self.cpg.set_signature(n, sig);
        if let Some(l) = line {
            self.cpg.set_line(n, l);
        }
        n
    }

    pub fn parameter(&mut self, name: &str, type_full_name: &str, index: i32) -> NodeId {
        let n = self.node(NodeKind::MethodParameterIn);
        let nm = self.cpg.intern(name);
        let ty = self.cpg.intern(type_full_name);
        self.cpg.set_name(n, nm);
        self.cpg.set_type_full_name(n, ty);
        self.cpg.set_order(n, index);
        self.cpg.set_argument_index(n, index);
        n
    }

    pub fn method_return(&mut self, type_full_name: &str) -> NodeId {
        let n = self.node(NodeKind::MethodReturn);
        let ty = self.cpg.intern(type_full_name);
        self.cpg.set_type_full_name(n, ty);
        n
    }

    pub fn block(&mut self) -> NodeId {
        self.node(NodeKind::Block)
    }

    /// A call site. `name` is the (unresolved) callee name; the call-graph pass
    /// later attaches a `Call` edge to the resolved Method.
    pub fn call(&mut self, name: &str, code: &str, line: Option<u32>) -> NodeId {
        let n = self.node(NodeKind::Call);
        let nm = self.cpg.intern(name);
        let c = self.cpg.intern(code);
        self.cpg.set_name(n, nm);
        self.cpg.set_code(n, c);
        if let Some(l) = line {
            self.cpg.set_line(n, l);
        }
        n
    }

    pub fn identifier(&mut self, name: &str, line: Option<u32>) -> NodeId {
        let n = self.node(NodeKind::Identifier);
        let nm = self.cpg.intern(name);
        self.cpg.set_name(n, nm);
        self.cpg.set_code(n, nm);
        if let Some(l) = line {
            self.cpg.set_line(n, l);
        }
        n
    }

    pub fn literal(&mut self, code: &str, line: Option<u32>) -> NodeId {
        let n = self.node(NodeKind::Literal);
        let c = self.cpg.intern(code);
        self.cpg.set_code(n, c);
        if let Some(l) = line {
            self.cpg.set_line(n, l);
        }
        n
    }

    pub fn local(&mut self, name: &str, type_full_name: &str) -> NodeId {
        let n = self.node(NodeKind::Local);
        let nm = self.cpg.intern(name);
        let ty = self.cpg.intern(type_full_name);
        self.cpg.set_name(n, nm);
        self.cpg.set_type_full_name(n, ty);
        n
    }

    pub fn ret(&mut self, code: &str, line: Option<u32>) -> NodeId {
        let n = self.node(NodeKind::Return);
        let c = self.cpg.intern(code);
        self.cpg.set_code(n, c);
        if let Some(l) = line {
            self.cpg.set_line(n, l);
        }
        n
    }

    pub fn control_structure(&mut self, code: &str, line: Option<u32>) -> NodeId {
        let n = self.node(NodeKind::ControlStructure);
        let c = self.cpg.intern(code);
        self.cpg.set_code(n, c);
        if let Some(l) = line {
            self.cpg.set_line(n, l);
        }
        n
    }

    /// Attach `arg` to a `call` as positional argument `index`.
    pub fn add_argument(&mut self, call: NodeId, arg: NodeId, index: i32) {
        self.cpg.set_argument_index(arg, index);
        self.cpg.add_edge(call, arg, EdgeKind::Argument);
        self.cpg.add_edge(call, arg, EdgeKind::Ast);
    }

    pub fn add_receiver(&mut self, call: NodeId, recv: NodeId) {
        self.cpg.add_edge(call, recv, EdgeKind::Receiver);
        self.cpg.add_edge(call, recv, EdgeKind::Ast);
    }
}
