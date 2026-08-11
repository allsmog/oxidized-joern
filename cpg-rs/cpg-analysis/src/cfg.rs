//! Intra-procedural control-flow pass (reads Ast, writes Cfg).
//!
//! A port of the byte-parity-validated `CfgBuilder` from `joern-parity`
//! (itself a reconstruction of Joern's CfgCreationPass semantics, validated
//! against the Joern v4.0.555 oracle) onto the `cpg-core` graph. The core
//! semantics carried over verbatim:
//!
//! - **Evaluation-order chaining**: a call's arguments execute in order, then
//!   the call node itself; leaves (identifiers/literals/...) are single CFG
//!   nodes.
//! - **Branching**: an `if`/loop condition's *root* node branches to both
//!   arms; short-circuit `&&`/`||` and `?:` branch from their first operand.
//! - **Loop shapes**: while (cond -> body -> cond back-edge), do-while (body
//!   -> cond -> body back-edge), for (init -> cond -> body -> update -> cond).
//! - **break/continue**: collected per enclosing breakable construct; break
//!   exits past it, continue targets the loop re-entry (cond for while/do,
//!   update for for-loops).
//! - **switch**: the condition branches to every case entry; natural chaining
//!   between cases is fallthrough; without a `default` the condition also
//!   flows to the continuation.
//! - Statement `Block`s are transparent; an *expression* block (a Block that
//!   is the child of a Call — the comma operator) is itself a CFG node after
//!   its children. Locals/params/modifiers are invisible.
//!
//! Divergences from the joern-parity implementation, forced by the simpler
//! cpg-core schema / frontends (documented here once, referenced below):
//!
//! 1. No `JUMP_TARGET` nodes: case labels are ControlStructures with code
//!    `case_statement`/`default_statement` (C frontend); the dispatch edge
//!    goes to the case's first CFG node instead of a jump-target node.
//! 2. No `goto`/label support: the frontends do not produce labels.
//! 3. Control structures produced by the generic tree-sitter frontend carry
//!    *flat* children (condition and body statements are siblings with no
//!    role markers). For those, `if`/`while`/`do` still treat the first (resp.
//!    last, for `do`) child as the condition; anything unrecognised falls back
//!    to sequential chaining — a sound linearisation, not a precise CFG.
//! 4. `INLINED` macro calls do not exist (the frontends never expand macros).

use crate::pass::{Pass, PassContext};
use cpg_core::{Cpg, EdgeKind, FileId, NodeId, NodeKind};

pub struct CfgPass;

impl Pass for CfgPass {
    fn name(&self) -> &'static str {
        "CfgPass"
    }
    fn reads(&self) -> &'static [cpg_core::Layer] {
        &[cpg_core::Layer::Ast]
    }
    fn writes(&self) -> &'static [cpg_core::Layer] {
        &[cpg_core::Layer::Cfg]
    }
    fn output_edge(&self) -> Option<EdgeKind> {
        Some(EdgeKind::Cfg)
    }
    fn run_file(&self, cpg: &mut Cpg, file: FileId, _ctx: &PassContext) {
        let methods: Vec<NodeId> = cpg
            .nodes_in_file(file)
            .iter()
            .copied()
            .filter(|&n| cpg.is_live(n) && cpg.kind_of(n) == NodeKind::Method)
            .collect();

        for m in methods {
            for (src, dst) in cfg_edges_for_method(cpg, m) {
                cpg.add_edge(src, dst, EdgeKind::Cfg);
            }
        }
    }
}

/// Compute the CFG edges of one method without mutating the graph. Public so
/// tests and downstream analyses can inspect the flow shape directly.
pub fn cfg_edges_for_method(cpg: &Cpg, method: NodeId) -> Vec<(NodeId, NodeId)> {
    let mret = cpg
        .out_kind(method, EdgeKind::Ast)
        .find(|&c| cpg.kind_of(c) == NodeKind::MethodReturn);
    let body = cpg
        .out_kind(method, EdgeKind::Ast)
        .find(|&c| cpg.kind_of(c) == NodeKind::Block);
    let Some(mret) = mret else { return Vec::new() };

    let mut b = CfgBuilder {
        cpg,
        mret,
        edges: Vec::new(),
        breaks: Vec::new(),
        continues: Vec::new(),
    };
    let (entry, outs) = match body {
        Some(body) => b.build(body),
        None => (None, Vec::new()),
    };
    // METHOD -> first node (or straight to METHOD_RETURN for an empty body).
    match entry {
        Some(e) => b.edges.push((method, e)),
        None => b.edges.push((method, mret)),
    }
    b.connect(&outs, mret);
    // Degenerate arms (e.g. an empty then AND else block) can contribute the
    // same fallthrough edge twice; keep the edge set duplicate-free.
    let mut seen = std::collections::HashSet::new();
    b.edges.retain(|e| seen.insert(*e));
    b.edges
}

struct CfgBuilder<'a> {
    cpg: &'a Cpg,
    mret: NodeId,
    edges: Vec<(NodeId, NodeId)>,
    breaks: Vec<Vec<NodeId>>,    // per breakable construct (loop/switch)
    continues: Vec<Vec<NodeId>>, // per loop
}

impl CfgBuilder<'_> {
    fn kids(&self, n: NodeId) -> Vec<NodeId> {
        // Ast out-edges preserve insertion order == source/evaluation order.
        self.cpg.out_kind(n, EdgeKind::Ast).collect()
    }

    fn connect(&mut self, srcs: &[NodeId], dst: NodeId) {
        for &s in srcs {
            self.edges.push((s, dst));
        }
    }

    /// Sequence children as statements: pending outs flow into the next
    /// construct's entry; transparent children pass outs through.
    fn seq(&mut self, ids: &[NodeId]) -> (Option<NodeId>, Vec<NodeId>) {
        let mut entry: Option<NodeId> = None;
        let mut pending: Vec<NodeId> = Vec::new();
        for &c in ids {
            let (e, o) = self.build(c);
            if let Some(e) = e {
                self.connect(&pending, e);
                if entry.is_none() {
                    entry = Some(e);
                }
                pending = o;
            }
        }
        (entry, pending)
    }

    /// Build the CFG fragment for `id`: returns (entry node, dangling outs).
    fn build(&mut self, id: NodeId) -> (Option<NodeId>, Vec<NodeId>) {
        let kids = self.kids(id);
        match self.cpg.kind_of(id) {
            NodeKind::Identifier
            | NodeKind::Literal
            | NodeKind::FieldIdentifier
            | NodeKind::MethodRef
            | NodeKind::Unknown => (Some(id), vec![id]),
            NodeKind::Call => {
                let name = self.cpg.name_of(id).unwrap_or("");
                match name {
                    // Ternary: cond root branches to both value arms; both
                    // arms flow into the call node. (Engine frontends do not
                    // build `?:` today; kept for frontends that will.)
                    "<operator>.conditional" | "?:" if kids.len() >= 3 => {
                        let (e1, o1) = self.build(kids[0]);
                        let (e2, o2) = self.build(kids[1]);
                        let (e3, o3) = self.build(kids[2]);
                        if let Some(e2) = e2 {
                            self.connect(&o1, e2);
                        }
                        if let Some(e3) = e3 {
                            self.connect(&o1, e3);
                        }
                        self.connect(&o2, id);
                        self.connect(&o3, id);
                        (e1, vec![id])
                    }
                    // Short-circuit: the lhs root branches to the rhs entry
                    // and directly past it to the call node.
                    "<operator>.logicalAnd" | "<operator>.logicalOr" | "&&" | "||"
                        if kids.len() >= 2 =>
                    {
                        let (e1, o1) = self.build(kids[0]);
                        let (e2, o2) = self.build(kids[1]);
                        if let Some(e2) = e2 {
                            self.connect(&o1, e2);
                        }
                        self.connect(&o1, id);
                        self.connect(&o2, id);
                        (e1, vec![id])
                    }
                    // Ordinary call/operator: arguments in evaluation order,
                    // then the call node itself.
                    _ => {
                        let (entry, outs) = self.seq(&kids);
                        self.connect(&outs, id);
                        (entry.or(Some(id)), vec![id])
                    }
                }
            }
            NodeKind::Block => {
                // An expression block (comma operator) is the child of a
                // CALL and is itself a CFG node after its children;
                // statement blocks are transparent.
                let is_expr = self
                    .cpg
                    .in_kind(id, EdgeKind::Ast)
                    .next()
                    .is_some_and(|p| self.cpg.kind_of(p) == NodeKind::Call);
                let (entry, outs) = self.seq(&kids);
                if is_expr {
                    self.connect(&outs, id);
                    (entry.or(Some(id)), vec![id])
                } else {
                    (entry, outs)
                }
            }
            NodeKind::Return => {
                let (entry, outs) = self.seq(&kids);
                self.connect(&outs, id);
                let mret = self.mret;
                self.edges.push((id, mret));
                (entry.or(Some(id)), vec![])
            }
            NodeKind::ControlStructure => self.build_control(id, &kids),
            // Local, MethodParameterIn, MethodReturn, TypeDecl, Member,
            // nested Method, ...: invisible to control flow.
            _ => (None, vec![]),
        }
    }

    fn build_control(&mut self, id: NodeId, kids: &[NodeId]) -> (Option<NodeId>, Vec<NodeId>) {
        // The C frontend stores the tree-sitter kind ("if_statement", ...) as
        // the ControlStructure's code; the generic ts frontend does the same
        // for its grammars ("if_expression", "unless", ...). Dispatch on it.
        let code = self.cpg.code_of(id).unwrap_or("").to_string();
        let kind = code.split(['_', '(', ';', ' ']).next().unwrap_or("");
        let is_block = |b: &Self, c: NodeId| b.cpg.kind_of(c) == NodeKind::Block;
        match kind {
            "if" | "unless" => {
                // Canonical (C frontend): [cond, Block(then), Block(else)?].
                // Flat (generic frontend): [cond, stmt, stmt, ...] — no way to
                // tell a then arm from an else arm, so everything after the
                // condition is one arm and the condition also exits directly
                // (divergence 3 in the module docs).
                let cond = kids.iter().copied().find(|&c| !is_block(self, c));
                let arms: Vec<NodeId> = kids
                    .iter()
                    .copied()
                    .filter(|&c| is_block(self, c))
                    .collect();
                let (ce, co) = match cond {
                    Some(c) => self.build(c),
                    None => (None, vec![]),
                };
                let mut entry = ce;
                let mut outs = Vec::new();
                if !arms.is_empty() {
                    let (te, to) = self.build(arms[0]);
                    if let Some(te) = te {
                        self.connect(&co, te);
                        entry = entry.or(Some(te));
                        outs.extend(to);
                    } else {
                        // Empty then-arm: the condition falls through.
                        outs.extend(co.iter().copied());
                    }
                    if let Some(&e) = arms.get(1) {
                        let (ee, eo) = self.build(e);
                        if let Some(ee) = ee {
                            self.connect(&co, ee);
                            entry = entry.or(Some(ee));
                            outs.extend(eo);
                        } else {
                            outs.extend(co.iter().copied());
                        }
                    } else {
                        // No else: false branch flows to the continuation.
                        outs.extend(co);
                    }
                    (entry, outs)
                } else {
                    // Flat shape: sequence the non-cond children as the arm.
                    let rest: Vec<NodeId> =
                        kids.iter().copied().filter(|&c| Some(c) != cond).collect();
                    let (te, to) = self.seq(&rest);
                    if let Some(te) = te {
                        self.connect(&co, te);
                    }
                    outs.extend(to);
                    outs.extend(co);
                    (entry.or(te), outs)
                }
            }
            "while" | "until" => {
                self.breaks.push(Vec::new());
                self.continues.push(Vec::new());
                // [cond, body...]: first child is the condition, everything
                // after is the body (a single Block from the C frontend).
                let cond = kids.first().copied();
                let (ce, co) = match cond {
                    Some(c) => self.build(c),
                    None => (None, vec![]),
                };
                let (be, bo) = self.seq(kids.get(1..).unwrap_or(&[]));
                if let Some(be) = be {
                    self.connect(&co, be);
                }
                if let Some(ce) = ce {
                    // Loop back-edge: body exit -> condition entry.
                    self.connect(&bo, ce);
                }
                let brs = self.breaks.pop().unwrap();
                let conts = self.continues.pop().unwrap();
                if let Some(ce) = ce {
                    self.connect(&conts, ce);
                }
                let mut outs = co;
                outs.extend(brs);
                (ce.or(be), outs)
            }
            "do" => {
                self.breaks.push(Vec::new());
                self.continues.push(Vec::new());
                // [Block(body), cond]: the condition is the last (non-Block)
                // child; the body executes first.
                let cond = kids.iter().copied().rev().find(|&c| !is_block(self, c));
                let body: Vec<NodeId> = kids.iter().copied().filter(|&c| Some(c) != cond).collect();
                let (be, bo) = self.seq(&body);
                let (ce, co) = match cond {
                    Some(c) => self.build(c),
                    None => (None, vec![]),
                };
                if let Some(ce) = ce {
                    self.connect(&bo, ce);
                }
                if let Some(be) = be {
                    // Back-edge: condition true -> body entry.
                    self.connect(&co, be);
                }
                let brs = self.breaks.pop().unwrap();
                let conts = self.continues.pop().unwrap();
                if let Some(ce) = ce {
                    self.connect(&conts, ce);
                }
                let mut outs = co;
                outs.extend(brs);
                (be.or(ce), outs)
            }
            "for" | "loop" => {
                self.breaks.push(Vec::new());
                self.continues.push(Vec::new());
                if kids.len() == 4 && kids.iter().all(|&c| is_block(self, c)) {
                    // Canonical C shape: [init, cond, update, body] Blocks
                    // (empty clause = empty transparent Block), positions are
                    // the truth — same as Joern's placeholder BLOCKs.
                    let (ie, io) = self.build(kids[0]);
                    let (ce, co) = self.build(kids[1]);
                    let (ue, uo) = self.build(kids[2]);
                    let (be, bo) = self.build(kids[3]);
                    if let Some(ce) = ce {
                        self.connect(&io, ce);
                        self.connect(&uo, ce);
                    }
                    // True branch: body if present, else straight to the
                    // update (empty-body loops), else the cond itself.
                    if let Some(t) = be.or(ue).or(ce) {
                        self.connect(&co, t);
                    }
                    if let Some(ue) = ue {
                        self.connect(&bo, ue);
                    } else if let Some(ce) = ce {
                        // No update clause: body loops straight to the cond.
                        self.connect(&bo, ce);
                    }
                    let brs = self.breaks.pop().unwrap();
                    let conts = self.continues.pop().unwrap();
                    // continue targets the loop re-entry: the update when
                    // there is one, else the condition.
                    if let Some(t) = ue.or(ce) {
                        self.connect(&conts, t);
                    }
                    let mut outs = co;
                    outs.extend(brs);
                    // Entry: init, else cond, else body, else update.
                    // (joern-parity uses init-else-cond; the extra fallbacks
                    // keep degenerate `for(;;)` bodies reachable.)
                    (ie.or(ce).or(be).or(ue), outs)
                } else {
                    // Flat/foreach shape (generic frontend): linearise the
                    // children and add a back-edge to the head — an
                    // approximation, see divergence 3 in the module docs.
                    let (e, o) = self.seq(kids);
                    if let Some(e) = e {
                        self.connect(&o, e);
                    }
                    let brs = self.breaks.pop().unwrap();
                    let conts = self.continues.pop().unwrap();
                    if let Some(e) = e {
                        self.connect(&conts, e);
                    }
                    let mut outs = o;
                    outs.extend(brs);
                    (e, outs)
                }
            }
            "switch" | "match" => {
                self.breaks.push(Vec::new());
                // [cond, Block(body)] (C) or flat children (generic).
                let cond = kids.iter().copied().find(|&c| !is_block(self, c));
                let (ce, co) = match cond {
                    Some(c) => self.build(c),
                    None => (None, vec![]),
                };
                // The case list: the body Block's children when present,
                // otherwise the remaining flat children.
                let body: Vec<NodeId> = match kids.iter().copied().find(|&c| is_block(self, c)) {
                    Some(b) => self.kids(b),
                    None => kids.iter().copied().filter(|&c| Some(c) != cond).collect(),
                };
                let mut outs = Vec::new();
                let mut has_default = false;
                // Sequence the cases (natural chaining = fallthrough) while
                // routing a dispatch edge cond -> each case entry. Joern
                // targets JUMP_TARGET label nodes; without them the dispatch
                // goes to the case's first CFG node (divergence 1).
                let mut pending: Vec<NodeId> = Vec::new();
                for &c in &body {
                    let is_case = self.cpg.kind_of(c) == NodeKind::ControlStructure
                        && matches!(
                            self.cpg.code_of(c).unwrap_or(""),
                            "case_statement" | "default_statement"
                        );
                    if is_case && self.cpg.code_of(c) == Some("default_statement") {
                        has_default = true;
                    }
                    let (e, o) = self.build(c);
                    if let Some(e) = e {
                        self.connect(&pending, e);
                        if is_case {
                            self.connect(&co, e);
                        }
                        pending = o;
                    }
                }
                outs.extend(pending);
                if !has_default {
                    outs.extend(co.iter().copied());
                }
                outs.extend(self.breaks.pop().unwrap());
                (ce, outs)
            }
            // A case/default label region: transparent sequence — its value
            // expression (if any) chains before its statements, and
            // fallthrough happens naturally in the enclosing switch loop.
            "case" | "default" | "when" => self.seq(kids),
            "break" => {
                if let Some(b) = self.breaks.last_mut() {
                    b.push(id);
                }
                (Some(id), vec![])
            }
            "continue" => {
                if let Some(c) = self.continues.last_mut() {
                    c.push(id);
                }
                (Some(id), vec![])
            }
            // Unknown control kind (try/with/labeled/goto...): sequential
            // chaining pass-through (divergences 2 and 3).
            _ => self.seq(kids),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pass::PassContext;
    use cpg_core::Cpg;
    use cpg_frontend::Frontend;

    /// Build a single C file and run the CFG pass over it.
    fn build(src: &str) -> Cpg {
        let mut cpg = Cpg::new();
        let mut fe = cpg_lang_c::CFrontend::new();
        let r = fe.build_file(&mut cpg, "t.c", src);
        CfgPass.run_file(&mut cpg, r.file, &PassContext::empty());
        cpg
    }

    fn node_with_code(cpg: &Cpg, kind: NodeKind, code: &str) -> NodeId {
        cpg.nodes()
            .find(|&n| cpg.kind_of(n) == kind && cpg.code_of(n) == Some(code))
            .unwrap_or_else(|| panic!("no {kind:?} with code {code:?}"))
    }

    fn call_named(cpg: &Cpg, name: &str) -> NodeId {
        cpg.nodes()
            .find(|&n| cpg.kind_of(n) == NodeKind::Call && cpg.name_of(n) == Some(name))
            .unwrap_or_else(|| panic!("no call named {name:?}"))
    }

    fn has_cfg_edge(cpg: &Cpg, src: NodeId, dst: NodeId) -> bool {
        cpg.out_kind(src, EdgeKind::Cfg).any(|d| d == dst)
    }

    #[test]
    fn cfg_if_else_branches_from_condition() {
        let cpg = build("void f(int x) { if (x) { a(); } else { b(); } done(); }");
        // Condition root = the identifier x used as the if condition.
        let cond = cpg
            .nodes()
            .filter(|&n| cpg.kind_of(n) == NodeKind::Identifier && cpg.name_of(n) == Some("x"))
            .find(|&n| cpg.out_kind(n, EdgeKind::Cfg).count() == 2)
            .expect("branching condition node");
        let a = call_named(&cpg, "a");
        let b = call_named(&cpg, "b");
        let done = call_named(&cpg, "done");
        assert!(has_cfg_edge(&cpg, cond, a), "cond -> then arm");
        assert!(has_cfg_edge(&cpg, cond, b), "cond -> else arm");
        // Both arms converge on the continuation.
        assert!(has_cfg_edge(&cpg, a, done), "then arm -> continuation");
        assert!(has_cfg_edge(&cpg, b, done), "else arm -> continuation");
        // The condition must NOT skip to the continuation (there IS an else).
        assert!(
            !has_cfg_edge(&cpg, cond, done),
            "no cond -> continuation with else present"
        );
    }

    #[test]
    fn cfg_if_without_else_falls_through() {
        let cpg = build("void f(int x) { if (x) { a(); } done(); }");
        let cond = cpg
            .nodes()
            .filter(|&n| cpg.kind_of(n) == NodeKind::Identifier && cpg.name_of(n) == Some("x"))
            .find(|&n| cpg.out_kind(n, EdgeKind::Cfg).count() == 2)
            .expect("branching condition node");
        let a = call_named(&cpg, "a");
        let done = call_named(&cpg, "done");
        assert!(has_cfg_edge(&cpg, cond, a));
        assert!(
            has_cfg_edge(&cpg, cond, done),
            "false branch skips the then arm"
        );
    }

    #[test]
    fn cfg_while_loop_has_back_edge() {
        let cpg = build("void f(int n) { while (n) { g(); } after(); }");
        let cond = cpg
            .nodes()
            .find(|&n| cpg.kind_of(n) == NodeKind::Identifier && cpg.name_of(n) == Some("n"))
            .expect("condition identifier");
        let g = call_named(&cpg, "g");
        let after = call_named(&cpg, "after");
        assert!(has_cfg_edge(&cpg, cond, g), "cond -> body");
        assert!(has_cfg_edge(&cpg, g, cond), "body -> cond (back-edge)");
        assert!(has_cfg_edge(&cpg, cond, after), "cond -> loop exit");
    }

    #[test]
    fn cfg_do_while_executes_body_first() {
        let cpg = build("void f(int n) { do { g(); } while (n); }");
        let method = cpg
            .nodes()
            .find(|&n| cpg.kind_of(n) == NodeKind::Method)
            .unwrap();
        let mret = cpg
            .out_kind(method, EdgeKind::Ast)
            .find(|&c| cpg.kind_of(c) == NodeKind::MethodReturn)
            .unwrap();
        let cond = cpg
            .nodes()
            .find(|&n| cpg.kind_of(n) == NodeKind::Identifier && cpg.name_of(n) == Some("n"))
            .unwrap();
        let g = call_named(&cpg, "g");
        assert!(
            has_cfg_edge(&cpg, method, g),
            "method entry -> body (not cond)"
        );
        assert!(has_cfg_edge(&cpg, g, cond), "body -> cond");
        assert!(has_cfg_edge(&cpg, cond, g), "cond -> body (back-edge)");
        assert!(has_cfg_edge(&cpg, cond, mret), "cond false -> exit");
    }

    #[test]
    fn cfg_for_loop_shape() {
        let cpg = build("void f(int n) { for (i = 0; i < n; i = i + 1) { g(); } after(); }");
        let init = node_with_code(&cpg, NodeKind::Call, "i = 0");
        let cond = node_with_code(&cpg, NodeKind::Call, "i < n");
        let update = node_with_code(&cpg, NodeKind::Call, "i = i + 1");
        let g = call_named(&cpg, "g");
        let after = call_named(&cpg, "after");
        // init -> cond entry (the first leaf of the condition, its `i`).
        let cond_i = cpg
            .out_kind(init, EdgeKind::Cfg)
            .find(|&d| cpg.kind_of(d) == NodeKind::Identifier && cpg.name_of(d) == Some("i"))
            .expect("init flows into the condition's first leaf");
        assert!(has_cfg_edge(
            &cpg,
            cond_i,
            cpg.out_kind(cond_i, EdgeKind::Cfg).next().unwrap()
        ));
        assert!(has_cfg_edge(&cpg, cond, g), "cond true -> body");
        assert!(
            has_cfg_edge(&cpg, cond, after),
            "cond false -> continuation"
        );
        // body -> update entry -> ... -> update root -> cond entry (back-edge).
        let update_entry = cpg
            .out_kind(g, EdgeKind::Cfg)
            .next()
            .expect("body flows onward");
        assert!(
            std::iter::successors(Some(update_entry), |&n| cpg
                .out_kind(n, EdgeKind::Cfg)
                .next())
            .take(8)
            .any(|n| n == update),
            "body chains through the update expression"
        );
        assert!(
            has_cfg_edge(&cpg, update, cond_i),
            "update -> cond (back-edge)"
        );
    }

    #[test]
    fn cfg_arguments_chain_before_call() {
        let cpg = build("void f(int u, int v) { h(u, v); }");
        let u = cpg
            .nodes()
            .find(|&n| cpg.kind_of(n) == NodeKind::Identifier && cpg.name_of(n) == Some("u"))
            .unwrap();
        let v = cpg
            .nodes()
            .find(|&n| cpg.kind_of(n) == NodeKind::Identifier && cpg.name_of(n) == Some("v"))
            .unwrap();
        let h = call_named(&cpg, "h");
        let method = cpg
            .nodes()
            .find(|&n| cpg.kind_of(n) == NodeKind::Method)
            .unwrap();
        assert!(
            has_cfg_edge(&cpg, method, u),
            "method entry -> first argument"
        );
        assert!(has_cfg_edge(&cpg, u, v), "arguments evaluate in order");
        assert!(has_cfg_edge(&cpg, v, h), "last argument -> the call itself");
        assert!(!has_cfg_edge(&cpg, method, h), "the call is not the entry");
    }

    #[test]
    fn cfg_return_flows_to_method_return_only() {
        let cpg = build("int f(int x) { if (x) { return 1; } return 2; }");
        let method = cpg
            .nodes()
            .find(|&n| cpg.kind_of(n) == NodeKind::Method)
            .unwrap();
        let mret = cpg
            .out_kind(method, EdgeKind::Ast)
            .find(|&c| cpg.kind_of(c) == NodeKind::MethodReturn)
            .unwrap();
        let rets: Vec<NodeId> = cpg
            .nodes()
            .filter(|&n| cpg.kind_of(n) == NodeKind::Return)
            .collect();
        assert_eq!(rets.len(), 2);
        for r in rets {
            assert!(has_cfg_edge(&cpg, r, mret), "every return -> METHOD_RETURN");
            assert!(
                cpg.out_kind(r, EdgeKind::Cfg).all(|d| d == mret),
                "a return has no fallthrough successor"
            );
        }
    }

    #[test]
    fn cfg_break_and_continue() {
        let cpg = build("void f(int n) { while (n) { if (n) { break; } continue; } after(); }");
        let after = call_named(&cpg, "after");
        let brk = node_with_code(&cpg, NodeKind::ControlStructure, "break_statement");
        let cont = node_with_code(&cpg, NodeKind::ControlStructure, "continue_statement");
        assert!(has_cfg_edge(&cpg, brk, after), "break exits past the loop");
        // continue -> loop condition (an identifier n that branches).
        let cond = cpg
            .out_kind(cont, EdgeKind::Cfg)
            .next()
            .expect("continue has a successor");
        assert_eq!(cpg.kind_of(cond), NodeKind::Identifier);
        assert_eq!(cpg.name_of(cond), Some("n"));
    }

    #[test]
    fn cfg_switch_dispatch_and_fallthrough() {
        let cpg = build(
            "void f(int x) { switch (x) { case 1: a(); case 2: b(); break; default: c(); } after(); }",
        );
        let cond = cpg
            .nodes()
            .filter(|&n| cpg.kind_of(n) == NodeKind::Identifier && cpg.name_of(n) == Some("x"))
            .find(|&n| cpg.out_kind(n, EdgeKind::Cfg).count() >= 3)
            .expect("switch condition dispatches to every case");
        let a = call_named(&cpg, "a");
        let b = call_named(&cpg, "b");
        let c = call_named(&cpg, "c");
        let after = call_named(&cpg, "after");
        // a() falls through into case 2 (its value literal chains first).
        assert!(
            std::iter::successors(Some(a), |&n| cpg.out_kind(n, EdgeKind::Cfg).next())
                .take(4)
                .any(|n| n == b),
            "case 1 falls through to case 2"
        );
        // break after b() exits the switch; with a default the condition does
        // not flow to the continuation directly.
        let brk = node_with_code(&cpg, NodeKind::ControlStructure, "break_statement");
        assert!(has_cfg_edge(&cpg, brk, after));
        assert!(has_cfg_edge(&cpg, c, after), "default arm -> continuation");
        assert!(
            !has_cfg_edge(&cpg, cond, after),
            "default present: no cond -> continuation"
        );
    }
}
