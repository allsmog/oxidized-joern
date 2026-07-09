//! A C frontend built on tree-sitter.
//!
//! This is the "one frontend, end-to-end" proof from the roadmap: real parsing
//! (tree-sitter's forgiving GLR grammar, so incomplete/uncompilable C still
//! yields a graph), mapped onto the shared builder primitives — never onto the
//! columnar arrays directly. The mapping is a few hundred lines; the heavy
//! lifting (storage, schema, incremental delete/rebuild) is shared.

use cpg_core::{Cpg, NodeId};
use cpg_frontend::{BuildResult, Frontend, Language, LanguageTraits};
use tree_sitter::{Node, Parser};

pub struct C;

impl Language for C {
    fn name(&self) -> &'static str {
        "C"
    }
    fn namespace_delimiter(&self) -> &'static str {
        "."
    }
    fn traits(&self) -> LanguageTraits {
        // C: structs but no classes, function pointers, no generics/overloading.
        LanguageTraits::HAS_FUNCTION_POINTERS | LanguageTraits::ALLOWS_FORWARD_REFS
    }
    fn file_extensions(&self) -> &'static [&'static str] {
        &["c", "h"]
    }
}

pub struct CFrontend {
    lang: C,
    parser: Parser,
}

impl Default for CFrontend {
    fn default() -> Self {
        Self::new()
    }
}

impl CFrontend {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_c::LANGUAGE.into())
            .expect("load C grammar");
        CFrontend { lang: C, parser }
    }
}

impl Frontend for CFrontend {
    fn language(&self) -> &dyn Language {
        &self.lang
    }

    fn build_file(&mut self, cpg: &mut Cpg, path: &str, source: &str) -> BuildResult {
        let tree = self.parser.parse(source, None).expect("parse");
        let root = tree.root_node();
        let file = cpg.file_id(path);
        let mut b = cpg_core::CpgBuilder::new(cpg, file);
        let file_node = b.file_node(path);

        let mut methods = 0usize;
        let src = source.as_bytes();
        let mut cur = root.walk();
        for child in root.named_children(&mut cur) {
            if child.kind() == "function_definition" {
                if let Some(m) = build_function(&mut b, file_node, child, src) {
                    methods += 1;
                    let _ = m;
                }
            }
        }
        BuildResult { file, methods_built: methods }
    }
}

/// Collect a node's named children into an owned vec, so callers can iterate
/// without keeping the tree-sitter cursor borrowed across recursive builds.
fn named_children(node: Node) -> Vec<Node> {
    let mut cur = node.walk();
    node.named_children(&mut cur).collect()
}

fn text<'a>(node: Node, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or("")
}

fn line(node: Node) -> Option<u32> {
    Some(node.start_position().row as u32 + 1)
}

/// Build a `function_definition`: method node, parameters, return slot, and the
/// body's statements.
fn build_function(
    b: &mut cpg_core::CpgBuilder,
    file_node: NodeId,
    node: Node,
    src: &[u8],
) -> Option<NodeId> {
    let declarator = node.child_by_field_name("declarator")?;
    let fn_decl = find_function_declarator(declarator)?;
    let name_node = fn_decl.child_by_field_name("declarator")?;
    let name = text(name_node, src);
    let ret_type = node
        .child_by_field_name("type")
        .map(|t| text(t, src))
        .unwrap_or("ANY");

    let method = b.method(name, name, &format!("{name}()"), line(node));
    b.contains(file_node, method);

    // Parameters.
    if let Some(params) = fn_decl.child_by_field_name("parameters") {
        let mut cur = params.walk();
        let mut idx = 1i32;
        for p in params.named_children(&mut cur) {
            if p.kind() == "parameter_declaration" {
                let ptype = p
                    .child_by_field_name("type")
                    .map(|t| text(t, src))
                    .unwrap_or("ANY");
                let pname = p
                    .child_by_field_name("declarator")
                    .map(|d| innermost_identifier(d, src))
                    .unwrap_or("");
                let param = b.parameter(pname, ptype, idx);
                b.ast_child(method, param);
                idx += 1;
            }
        }
    }
    let ret = b.method_return(ret_type);
    b.ast_child(method, ret);

    // Body.
    let body = node.child_by_field_name("body");
    let block = b.block();
    b.ast_child(method, block);
    if let Some(body) = body {
        let mut cur = body.walk();
        for stmt in body.named_children(&mut cur) {
            build_stmt(b, block, stmt, src);
        }
    }
    Some(method)
}

fn build_stmt(b: &mut cpg_core::CpgBuilder, parent: NodeId, node: Node, src: &[u8]) {
    match node.kind() {
        "return_statement" => {
            let ret = b.ret(text(node, src), line(node));
            b.ast_child(parent, ret);
            let mut cur = node.walk();
            for c in node.named_children(&mut cur) {
                if let Some(e) = build_expr(b, c, src) {
                    b.ast_child(ret, e);
                }
            }
        }
        // Control structures are built in a *canonical shape* the CFG pass can
        // rely on: the condition expression is attached directly to the
        // ControlStructure node, and each statement arm/clause is wrapped in
        // its own Block child. (Joern instead distinguishes children by
        // `order`/`condition` edges; the simplified cpg-core schema has
        // neither, so child position + Block wrappers carry the roles.)
        "if_statement" => {
            // Children: [cond, Block(then), Block(else)?].
            let cs = b.control_structure(node.kind(), line(node));
            b.ast_child(parent, cs);
            if let Some(e) = node.child_by_field_name("condition").and_then(|c| build_expr(b, c, src)) {
                b.ast_child(cs, e);
            }
            if let Some(cons) = node.child_by_field_name("consequence") {
                let blk = b.block();
                b.ast_child(cs, blk);
                build_stmt(b, blk, cons, src);
            }
            if let Some(alt) = node.child_by_field_name("alternative") {
                // `alternative` is an else_clause wrapping the else statement.
                let blk = b.block();
                b.ast_child(cs, blk);
                for c in named_children(alt) {
                    build_stmt(b, blk, c, src);
                }
            }
        }
        "while_statement" => {
            // Children: [cond, Block(body)].
            let cs = b.control_structure(node.kind(), line(node));
            b.ast_child(parent, cs);
            if let Some(e) = node.child_by_field_name("condition").and_then(|c| build_expr(b, c, src)) {
                b.ast_child(cs, e);
            }
            let blk = b.block();
            b.ast_child(cs, blk);
            if let Some(body) = node.child_by_field_name("body") {
                build_stmt(b, blk, body, src);
            }
        }
        "do_statement" => {
            // Children: [Block(body), cond] — body first, mirroring execution.
            let cs = b.control_structure(node.kind(), line(node));
            b.ast_child(parent, cs);
            let blk = b.block();
            b.ast_child(cs, blk);
            if let Some(body) = node.child_by_field_name("body") {
                build_stmt(b, blk, body, src);
            }
            if let Some(e) = node.child_by_field_name("condition").and_then(|c| build_expr(b, c, src)) {
                b.ast_child(cs, e);
            }
        }
        "for_statement" => {
            // Children: always four Blocks [init, cond, update, body]; an
            // absent clause leaves its Block empty. Positions are the truth,
            // exactly like Joern's placeholder BLOCKs for empty for-clauses.
            let cs = b.control_structure(node.kind(), line(node));
            b.ast_child(parent, cs);
            for field in ["initializer", "condition", "update"] {
                let blk = b.block();
                b.ast_child(cs, blk);
                if let Some(c) = node.child_by_field_name(field) {
                    build_stmt(b, blk, c, src);
                }
            }
            let blk = b.block();
            b.ast_child(cs, blk);
            if let Some(body) = node.child_by_field_name("body") {
                build_stmt(b, blk, body, src);
            }
        }
        "switch_statement" => {
            // Children: [cond, Block(body)]; the body Block holds the
            // case/default ControlStructures in source order.
            let cs = b.control_structure(node.kind(), line(node));
            b.ast_child(parent, cs);
            if let Some(e) = node.child_by_field_name("condition").and_then(|c| build_expr(b, c, src)) {
                b.ast_child(cs, e);
            }
            let blk = b.block();
            b.ast_child(cs, blk);
            if let Some(body) = node.child_by_field_name("body") {
                build_stmt(b, blk, body, src);
            }
        }
        "case_statement" => {
            // `case V: stmts` / `default: stmts`. Joern models the label as a
            // JUMP_TARGET sibling; the simplified schema has no JumpTarget
            // kind, so the case is a ControlStructure whose code distinguishes
            // case from default and whose children are [value?, stmts...].
            let value = node.child_by_field_name("value");
            let code = if value.is_some() { "case_statement" } else { "default_statement" };
            let cs = b.control_structure(code, line(node));
            b.ast_child(parent, cs);
            let value_id = value.map(|v| v.id());
            if let Some(e) = value.and_then(|v| build_expr(b, v, src)) {
                b.ast_child(cs, e);
            }
            for c in named_children(node) {
                if Some(c.id()) != value_id {
                    build_stmt(b, cs, c, src);
                }
            }
        }
        "break_statement" | "continue_statement" => {
            // Needed for CFG loop/switch exits; previously invisible.
            let cs = b.control_structure(node.kind(), line(node));
            b.ast_child(parent, cs);
        }
        "compound_statement" => {
            let mut cur = node.walk();
            for c in node.named_children(&mut cur) {
                build_stmt(b, parent, c, src);
            }
        }
        "declaration" => {
            // May contain several init_declarators; recurse so each one's
            // initialiser expression (which can itself be a call) is built.
            for c in named_children(node) {
                build_stmt(b, parent, c, src);
            }
        }
        "init_declarator" => {
            // `T x = <init>`: model as an assignment `=`(x, init) so dataflow
            // and taint see the binding, mirroring how assignment_expression is
            // handled. Without this, a declaration's initialiser would not taint
            // the declared variable.
            let value = node.child_by_field_name("value").and_then(|v| build_expr(b, v, src));
            let name = node
                .child_by_field_name("declarator")
                .map(|d| innermost_identifier(d, src))
                .unwrap_or("");
            match (value, name.is_empty()) {
                (Some(v), false) => {
                    let assign = b.call("=", text(node, src), line(node));
                    let lhs = b.identifier(name, line(node));
                    b.add_argument(assign, lhs, 1);
                    b.add_argument(assign, v, 2);
                    b.ast_child(parent, assign);
                }
                (Some(v), true) => b.ast_child(parent, v),
                _ => {}
            }
        }
        "expression_statement" => {
            for c in named_children(node) {
                if let Some(e) = build_expr(b, c, src) {
                    b.ast_child(parent, e);
                }
            }
        }
        _ => {
            // Expression-shaped node directly under a block.
            if let Some(e) = build_expr(b, node, src) {
                b.ast_child(parent, e);
            }
        }
    }
}

/// Build an expression subtree, returning its root node id.
fn build_expr(b: &mut cpg_core::CpgBuilder, node: Node, src: &[u8]) -> Option<NodeId> {
    match node.kind() {
        "call_expression" => {
            let callee = node.child_by_field_name("function");
            let name = callee.map(|c| text(c, src)).unwrap_or("<anon>");
            let call = b.call(name, text(node, src), line(node));
            if let Some(args) = node.child_by_field_name("arguments") {
                let mut cur = args.walk();
                let mut idx = 1i32;
                for a in args.named_children(&mut cur) {
                    if let Some(arg) = build_expr(b, a, src) {
                        b.add_argument(call, arg, idx);
                        idx += 1;
                    }
                }
            }
            Some(call)
        }
        "assignment_expression" => {
            // Model as a call `=`(lhs, rhs) so dataflow sees rhs -> lhs.
            let call = b.call("=", text(node, src), line(node));
            if let Some(lhs) = node.child_by_field_name("left").and_then(|n| build_expr(b, n, src)) {
                b.add_argument(call, lhs, 1);
            }
            if let Some(rhs) = node.child_by_field_name("right").and_then(|n| build_expr(b, n, src)) {
                b.add_argument(call, rhs, 2);
            }
            Some(call)
        }
        "binary_expression" => {
            let op = node.child(1).map(|n| text(n, src)).unwrap_or("<op>");
            let call = b.call(op, text(node, src), line(node));
            if let Some(lhs) = node.child_by_field_name("left").and_then(|n| build_expr(b, n, src)) {
                b.add_argument(call, lhs, 1);
            }
            if let Some(rhs) = node.child_by_field_name("right").and_then(|n| build_expr(b, n, src)) {
                b.add_argument(call, rhs, 2);
            }
            Some(call)
        }
        "parenthesized_expression" => {
            let children = named_children(node);
            children.into_iter().find_map(|c| build_expr(b, c, src))
        }
        "identifier" => Some(b.identifier(text(node, src), line(node))),
        "number_literal" | "string_literal" | "char_literal" | "true" | "false"
        | "concatenated_string" => Some(b.literal(text(node, src), line(node))),
        _ => {
            // Unknown wrapper: descend to its first buildable child.
            let children = named_children(node);
            children.into_iter().find_map(|c| build_expr(b, c, src))
        }
    }
}

/// A C declarator can be nested (pointers, arrays). Dig to the `function_declarator`.
fn find_function_declarator(node: Node) -> Option<Node> {
    if node.kind() == "function_declarator" {
        return Some(node);
    }
    let mut cur = node.walk();
    for c in node.named_children(&mut cur) {
        if let Some(f) = find_function_declarator(c) {
            return Some(f);
        }
    }
    None
}

/// Dig through pointer/array declarators to the bare identifier name.
fn innermost_identifier<'a>(node: Node, src: &'a [u8]) -> &'a str {
    if node.kind() == "identifier" {
        return text(node, src);
    }
    let mut cur = node.walk();
    for c in node.named_children(&mut cur) {
        let r = innermost_identifier(c, src);
        if !r.is_empty() {
            return r;
        }
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpg_core::Query;

    #[test]
    fn parses_function_with_params_and_call() {
        let mut cpg = Cpg::new();
        let mut fe = CFrontend::new();
        let code = r#"
            int add(int a, int b) {
                return a + b;
            }
            void main() {
                int r = add(1, 2);
                puts("hello");
            }
        "#;
        fe.build_file(&mut cpg, "t.c", code);

        let add = cpg.method_named("add");
        assert_eq!(add.len(), 1);
        assert_eq!(cpg.parameters_of(add[0]).len(), 2);

        assert_eq!(cpg.calls_named("add").len(), 1);
        assert_eq!(cpg.calls_named("puts").len(), 1);
        // `puts("hello")` has one argument.
        let puts = cpg.calls_named("puts")[0];
        assert_eq!(cpg.arguments_of(puts).len(), 1);
    }

    #[test]
    fn tolerates_incomplete_code() {
        // Missing include, unknown type — tree-sitter still yields a graph.
        let mut cpg = Cpg::new();
        let mut fe = CFrontend::new();
        let code = "int f(undeclared_t *x) { return g(x->field); }";
        let r = fe.build_file(&mut cpg, "broken.c", code);
        assert_eq!(r.methods_built, 1);
        assert_eq!(cpg.calls_named("g").len(), 1);
    }
}
