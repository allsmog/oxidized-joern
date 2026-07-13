//! The shared mapping engine: tree-sitter parse tree → CPG, driven entirely by
//! a `TsLangSpec`. There is no language-specific code path here — only lookups
//! into the spec. This is the contract under maximum stress: one engine, six
//! grammars.

use crate::spec::TsLangSpec;
use cpg_core::{CpgBuilder, NodeId};
use tree_sitter::Node;

pub fn build(spec: &TsLangSpec, b: &mut CpgBuilder, root: Node, src: &[u8], path: &str) -> usize {
    let file_node = b.file_node(path);
    let mut count = 0;
    scan(spec, b, file_node, root, src, &mut count);
    count
}

/// Recurse the tree; build a method when we hit a function-def kind, otherwise
/// descend. Nested functions are built by `build_method` itself, so we do not
/// descend into a function's body here.
fn scan(spec: &TsLangSpec, b: &mut CpgBuilder, file: NodeId, node: Node, src: &[u8], count: &mut usize) {
    for c in named_children(node) {
        if spec.is_function(c.kind()) {
            build_method(spec, b, file, c, src);
            *count += 1;
        } else {
            scan(spec, b, file, c, src, count);
        }
    }
}

fn build_method(spec: &TsLangSpec, b: &mut CpgBuilder, file: NodeId, node: Node, src: &[u8]) -> NodeId {
    let name = node
        .child_by_field_name("name")
        .map(|n| innermost_identifier(n, src))
        .filter(|s| !s.is_empty())
        // Anonymous function (e.g. a JS arrow): borrow the name of the binding
        // it is assigned to, so `const g = () => …` is the method `g`.
        .or_else(|| {
            let p = node.parent()?;
            let target = p.child_by_field_name("name").or_else(|| p.child_by_field_name("left"))?;
            let n = innermost_identifier(target, src);
            if n.is_empty() {
                None
            } else {
                Some(n)
            }
        })
        .unwrap_or("<anon>");
    let method = b.method(name, name, &format!("{name}()"), line(node));
    b.contains(file, method);

    // Parameters: prefer the `parameters` field, else a known container kind.
    let params = node.child_by_field_name("parameters").or_else(|| {
        named_children(node)
            .into_iter()
            .find(|c| spec.param_container_kinds.contains(&c.kind()))
    });
    if let Some(params) = params {
        let mut idx = 1;
        for p in named_children(params) {
            // Prefer the explicit name/pattern field: in several grammars (e.g.
            // Java `String p`) the *type* is itself a `*_identifier` and precedes
            // the name in child order, so a blind first-identifier scan would
            // pick the type. Fall back to a scan only for bare-identifier params.
            let pname = p
                .child_by_field_name("name")
                .or_else(|| p.child_by_field_name("pattern"))
                .map(|n| innermost_identifier(n, src))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| innermost_identifier(p, src));
            if pname.is_empty() || pname == "self" {
                continue;
            }
            let param = b.parameter(pname, "ANY", idx);
            b.ast_child(method, param);
            idx += 1;
        }
    }
    let ret = b.method_return("ANY");
    b.ast_child(method, ret);

    let block = b.block();
    b.ast_child(method, block);
    if let Some(body) = node.child_by_field_name("body") {
        if spec.implicit_return {
            walk_body_with_tail_return(spec, b, file, block, body, src);
        } else {
            walk_stmts(spec, b, file, block, body, src);
        }
    }
    method
}

/// For expression-bodied languages (Rust, Ruby), the final expression of the
/// body is the return value. Walk all but the last child normally, and if the
/// last child is a bare expression, wrap it in a Return node so the shared
/// dataflow engine sees the param→return flow exactly as for an explicit
/// `return`.
fn walk_body_with_tail_return(
    spec: &TsLangSpec,
    b: &mut CpgBuilder,
    file: NodeId,
    block: NodeId,
    body: Node,
    src: &[u8],
) {
    let children = named_children(body);
    let last = children.len().saturating_sub(1);
    for (i, c) in children.iter().enumerate() {
        let is_tail_expr = i == last && is_tail_expression(spec, c.kind());
        if is_tail_expr {
            if let Some(e) = build_expr(spec, b, *c, src) {
                let ret = b.ret(text(*c, src), line(*c));
                b.ast_child(ret, e);
                b.ast_child(block, ret);
                continue;
            }
        }
        walk_stmts(spec, b, file, block, *c, src);
    }
}

/// Whether a node kind is a value-producing expression (eligible to be an
/// implicit return), as opposed to a statement/declaration/control structure.
fn is_tail_expression(spec: &TsLangSpec, k: &str) -> bool {
    spec.is_call(k)
        || is_identifier(k)
        || is_member(k)
        || is_binary(k)
        || is_literal(k)
        || k == "parenthesized_expression"
}

/// Walk a statement subtree, attaching built expressions to `parent`.
fn walk_stmts(spec: &TsLangSpec, b: &mut CpgBuilder, file: NodeId, parent: NodeId, node: Node, src: &[u8]) {
    let k = node.kind();
    if spec.is_function(k) {
        build_method(spec, b, file, node, src); // nested function → its own method
        return;
    }
    if spec.is_return(k) {
        let ret = b.ret(text(node, src), line(node));
        b.ast_child(parent, ret);
        for c in named_children(node) {
            if let Some(e) = build_expr(spec, b, c, src) {
                b.ast_child(ret, e);
            }
        }
        return;
    }
    if spec.is_control(k) {
        let cs = b.control_structure(k, line(node));
        b.ast_child(parent, cs);
        for c in named_children(node) {
            walk_stmts(spec, b, file, cs, c, src);
        }
        return;
    }
    if spec.assign_form(k).is_some() {
        if let Some(e) = build_expr(spec, b, node, src) {
            b.ast_child(parent, e);
        }
        return;
    }
    if let Some(e) = build_expr(spec, b, node, src) {
        b.ast_child(parent, e);
        return;
    }
    // Structural node (block, statement_list, expression_statement, …): descend.
    for c in named_children(node) {
        walk_stmts(spec, b, file, parent, c, src);
    }
}

/// Build an expression subtree, returning its root node id, or `None` if `node`
/// is not expression-shaped (callers then descend into it).
fn build_expr(spec: &TsLangSpec, b: &mut CpgBuilder, node: Node, src: &[u8]) -> Option<NodeId> {
    let k = node.kind();
    if spec.is_call(k) {
        return Some(build_call(spec, b, node, src));
    }
    if let Some(form) = spec.assign_form(k) {
        return build_assignment(spec, b, node, form.lhs_field, form.rhs_field, src);
    }
    if is_literal(k) {
        return Some(b.literal(text(node, src), line(node)));
    }
    if is_identifier(k) {
        return Some(b.identifier(text(node, src), line(node)));
    }
    if is_member(k) {
        // Member/selector access: surface the base so its taint flows; if no
        // base, fall back to the member name as an identifier.
        for f in ["object", "operand", "value", "receiver"] {
            if let Some(base) = node.child_by_field_name(f) {
                return build_expr(spec, b, base, src);
            }
        }
        let name = callee_name(node, src);
        return Some(b.identifier(&name, line(node)));
    }
    if is_binary(k) {
        let op = node.child_by_field_name("operator").map(|n| text(n, src)).unwrap_or("<op>");
        let call = b.call(op, text(node, src), line(node));
        let mut idx = 1;
        for c in named_children(node) {
            if let Some(e) = build_expr(spec, b, c, src) {
                b.add_argument(call, e, idx);
                idx += 1;
            }
        }
        return Some(call);
    }
    // Expression wrappers that delegate to an inner expression.
    if matches!(
        k,
        "parenthesized_expression" | "expression_list" | "unary_expression" | "unary_operator"
            | "await_expression" | "reference_expression" | "try_expression" | "group"
            | "argument" | "spread_element"
    ) {
        return named_children(node).into_iter().find_map(|c| build_expr(spec, b, c, src));
    }
    // Not an expression (block, statement, declaration container, …). Returning
    // None lets `walk_stmts` descend and process each child as a statement —
    // crucially, this is what stops a multi-statement block or a branch body
    // from collapsing to just its first expression.
    None
}

fn build_call(spec: &TsLangSpec, b: &mut CpgBuilder, node: Node, src: &[u8]) -> NodeId {
    let callee = node
        .child_by_field_name(spec.callee_field)
        .or_else(|| node.child_by_field_name("macro"));
    let name = callee.map(|c| callee_name(c, src)).unwrap_or_else(|| "<anon>".into());
    let call = b.call(&name, text(node, src), line(node));

    // Arguments: the `arguments` field for normal calls, or a Rust macro's
    // `token_tree` child (println!/format!/… carry their args there as an
    // unnamed child, not in a field).
    let args = node
        .child_by_field_name("arguments")
        .or_else(|| named_children(node).into_iter().find(|c| c.kind() == "token_tree"));
    if let Some(args) = args {
        let mut idx = 1;
        for a in named_children(args) {
            if let Some(arg) = build_expr(spec, b, a, src) {
                b.add_argument(call, arg, idx);
                idx += 1;
            }
        }
    }
    // Receiver, if the call carries one (method calls).
    if let Some(recv) = node.child_by_field_name("receiver").or_else(|| node.child_by_field_name("object")) {
        if let Some(r) = build_expr(spec, b, recv, src) {
            b.add_receiver(call, r);
        }
    }
    call
}

fn build_assignment(
    spec: &TsLangSpec,
    b: &mut CpgBuilder,
    node: Node,
    lhs_field: &str,
    rhs_field: &str,
    src: &[u8],
) -> Option<NodeId> {
    let name = node
        .child_by_field_name(lhs_field)
        .map(|n| innermost_identifier(n, src).to_string())
        .unwrap_or_default();
    let value = node
        .child_by_field_name(rhs_field)
        .and_then(|v| build_value(spec, b, v, src));
    match (name.is_empty(), value) {
        (false, Some(v)) => {
            let assign = b.call("=", text(node, src), line(node));
            let lhs = b.identifier(&name, line(node));
            b.add_argument(assign, lhs, 1);
            b.add_argument(assign, v, 2);
            Some(assign)
        }
        (true, Some(v)) => Some(v),
        _ => None,
    }
}

/// Unwrap list/paren wrappers, then build the underlying expression.
fn build_value(spec: &TsLangSpec, b: &mut CpgBuilder, node: Node, src: &[u8]) -> Option<NodeId> {
    let mut n = node;
    while matches!(n.kind(), "expression_list" | "parenthesized_expression") {
        match first_named_child(n) {
            Some(c) => n = c,
            None => break,
        }
    }
    build_expr(spec, b, n, src)
}

// --- node-kind predicates (language-independent heuristics) ---

fn is_identifier(k: &str) -> bool {
    k == "identifier" || k.ends_with("_identifier")
}

fn is_member(k: &str) -> bool {
    matches!(
        k,
        "selector_expression" | "member_expression" | "field_expression" | "scoped_identifier"
            | "attribute" | "field_access" | "scoped_call_expression"
    )
}

fn is_binary(k: &str) -> bool {
    matches!(k, "binary_expression" | "binary_operator" | "boolean_operator" | "comparison_operator")
}

fn is_literal(k: &str) -> bool {
    k.contains("literal")
        || matches!(
            k,
            "number" | "string" | "integer" | "float" | "true" | "false" | "nil" | "null"
                | "none" | "boolean" | "character" | "simple_symbol"
        )
}

// --- tree helpers ---

fn named_children(node: Node) -> Vec<Node> {
    let mut cur = node.walk();
    node.named_children(&mut cur).collect()
}

fn first_named_child(node: Node) -> Option<Node> {
    named_children(node).into_iter().next()
}

fn text<'a>(node: Node, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or("")
}

fn line(node: Node) -> Option<u32> {
    Some(node.start_position().row as u32 + 1)
}

/// First identifier-ish token in a subtree (for parameter/variable names).
fn innermost_identifier<'a>(node: Node, src: &'a [u8]) -> &'a str {
    if is_identifier(node.kind()) {
        return text(node, src);
    }
    for c in named_children(node) {
        let r = innermost_identifier(c, src);
        if !r.is_empty() {
            return r;
        }
    }
    ""
}

/// The callable name from a callee node: a bare identifier, or the trailing
/// member of a qualified/member expression.
fn callee_name(node: Node, src: &[u8]) -> String {
    if is_identifier(node.kind()) {
        return text(node, src).to_string();
    }
    for f in ["field", "property", "name", "constant", "method"] {
        if let Some(c) = node.child_by_field_name(f) {
            return callee_name(c, src);
        }
    }
    for c in named_children(node).into_iter().rev() {
        let r = callee_name(c, src);
        if !r.is_empty() && r != "<anon>" {
            return r;
        }
    }
    text(node, src).to_string()
}
