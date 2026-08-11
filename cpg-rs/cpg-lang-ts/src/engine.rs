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
    scan(spec, b, file_node, root, src, &mut count, None);
    count
}

/// Recurse the tree; build a method when we hit a function-def kind, otherwise
/// descend. Nested functions are built by `build_method` itself, so we do not
/// descend into a function's body here. `enclosing` carries the name of the
/// innermost type container (class/object/trait) for constructor-sugar
/// qualification.
fn scan(
    spec: &TsLangSpec,
    b: &mut CpgBuilder,
    file: NodeId,
    node: Node,
    src: &[u8],
    count: &mut usize,
    enclosing: Option<&str>,
) {
    // Error recovery: when the grammar chokes inside a type header (e.g. a
    // standalone parameter annotation in a Scala class parameter list),
    // tree-sitter keeps a bodyless `class_definition name: X` and scatters
    // the members across FOLLOWING SIBLINGS — error subtrees and mis-parsed
    // expressions alike. A bodyless container (`pending`) plus at least one
    // ERROR sibling after it is that split: from the ERROR on, walk the
    // remaining siblings with X as the enclosing type so its methods still
    // qualify as `X.method`. Without ERROR evidence a bodyless container is
    // a plain forward declaration (`class Status;`) and changes nothing.
    let mut recovered: Option<String> = None;
    let mut pending: Option<String> = None;
    for c in named_children(node) {
        if c.is_error() && pending.is_some() {
            recovered = pending.take();
        }
        let rec = recovered.clone();
        let enc = rec.as_deref().or(enclosing);
        if spec.is_function(c.kind()) {
            build_method(spec, b, file, c, src, enc);
            *count += 1;
        } else if spec.type_container_kinds.contains(&c.kind()) {
            let name = c.child_by_field_name("name").map(|n| text(n, src));
            let has_body = c.child_by_field_name("body").is_some();
            // A class/struct with a body is a type declaration worth a node:
            // its base classes identify RPC handler implementations and its
            // members carry declared types for receiver-hint resolution.
            // Bodyless forms (`class Status;`) are forward declarations.
            if spec.type_decl_kinds.contains(&c.kind()) && has_body {
                if let Some(n) = name.filter(|n| !n.is_empty()) {
                    emit_type_decl(spec, b, file, c, n, enc, src);
                }
            }
            // A new container ends any active recovery region; a bodyless
            // one becomes the next recovery candidate.
            recovered = None;
            pending = if has_body {
                None
            } else {
                name.filter(|n| !n.is_empty()).map(str::to_string)
            };
            scan(spec, b, file, c, src, count, name.or(enc));
        } else {
            scan(spec, b, file, c, src, count, enc);
        }
    }
}

/// TypeDecl node for a class/struct body: base-class simple names (stored
/// comma-joined in the signature column) and Member children with declared
/// types. Base names strip namespaces/templates the same way receiver hints
/// do, so `public example::gateway::GatewayServerIf` matches a
/// `GatewayServerIf` key.
fn emit_type_decl(
    spec: &TsLangSpec,
    b: &mut CpgBuilder,
    file: NodeId,
    node: Node,
    name: &str,
    enclosing: Option<&str>,
    src: &[u8],
) {
    let full_name = match enclosing {
        Some(e) => format!("{e}{}{name}", spec.namespace_delim),
        None => name.to_string(),
    };
    let mut bases: Vec<String> = Vec::new();
    for clause in named_children(node) {
        if !spec.base_clause_kinds.contains(&clause.kind()) {
            continue;
        }
        for base in named_children(clause) {
            if base.kind() == "access_specifier" {
                continue;
            }
            let t = innermost_type_identifier(base, src);
            if !t.is_empty() {
                bases.push(t.to_string());
            }
        }
    }
    let td = b.type_decl(name, &full_name, &bases, line(node));
    b.contains(file, td);
    let Some(body) = node.child_by_field_name("body") else {
        return;
    };
    for field in named_children(body) {
        if !spec.member_kinds.contains(&field.kind()) {
            continue;
        }
        // A field_declaration holding a function_declarator is a method
        // prototype, not a data member.
        if find_descendant_of_kinds(field, &["function_declarator"]).is_some() {
            continue;
        }
        let Some(ty) = field
            .child_by_field_name("type")
            .and_then(|t| resolved_type(spec, t, src))
        else {
            continue;
        };
        // One declaration can name several members (`int count_, total_;`).
        let mut cur = field.walk();
        for d in field.children_by_field_name("declarator", &mut cur) {
            let mname = innermost_identifier(d, src);
            if !mname.is_empty() {
                b.member(td, mname, &ty);
            }
        }
    }
}

fn build_method(
    spec: &TsLangSpec,
    b: &mut CpgBuilder,
    file: NodeId,
    node: Node,
    src: &[u8],
    enclosing: Option<&str>,
) -> NodeId {
    // A declarator-style grammar (C++) nests the name — possibly scope-
    // qualified (`Foo::bar`) — inside the declarator chain.
    let decl_parts = spec
        .declarator_field
        .and_then(|df| node.child_by_field_name(df))
        .and_then(|d| declarator_name(d, src));
    let name = decl_parts
        .as_ref()
        .map(|(n, _)| n.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            node.child_by_field_name("name")
                .map(|n| innermost_identifier(n, src))
                .filter(|s| !s.is_empty())
        })
        // Anonymous function (e.g. a JS arrow): borrow the name of the binding
        // it is assigned to, so `const g = () => …` is the method `g`.
        .or_else(|| {
            let p = node.parent()?;
            let target = p
                .child_by_field_name("name")
                .or_else(|| p.child_by_field_name("left"))
                .or_else(|| p.child_by_field_name("pattern"))?;
            let n = innermost_identifier(target, src);
            if n.is_empty() {
                None
            } else {
                Some(n)
            }
        })
        .unwrap_or("<anon>");
    // Constructor sugar (Scala apply): register the method under its
    // container's name so `Foo(..)` call sites resolve; fullName keeps the
    // qualified form for display.
    let (name, full_name) = match (spec.ctor_sugar_method, enclosing) {
        (Some(sugar), Some(t)) if name == sugar => {
            (t.to_string(), format!("{t}{}{sugar}", spec.namespace_delim))
        }
        _ => (name.to_string(), name.to_string()),
    };
    // Receiver/container type: an explicit receiver (Go) wins, then a
    // declarator scope (C++ `void Foo::bar()`), else the enclosing type
    // container (Scala class, C++ inline method). Qualifies fullName and is
    // stored as the method's TYPE_FULL_NAME for type-aware call resolution.
    let recv_type: Option<String> = spec
        .receiver_field
        .and_then(|rf| node.child_by_field_name(rf))
        .map(|r| innermost_type_identifier(r, src).to_string())
        .filter(|t| !t.is_empty())
        .or_else(|| decl_parts.as_ref().and_then(|(_, scope)| scope.clone()))
        .or_else(|| enclosing.map(|t| t.to_string()));
    let full_name = match &recv_type {
        Some(t) if full_name == name => format!("{t}{}{name}", spec.namespace_delim),
        _ => full_name,
    };
    let name = name.as_str();
    let method = b.method(name, &full_name, &format!("{name}()"), line(node));
    if let Some(t) = &recv_type {
        let sym = b.cpg.intern(t);
        b.cpg.set_type_full_name(method, sym);
    }
    b.contains(file, method);
    // Locally-visible types (param/var name -> type name), threaded through
    // the body walk so call sites can carry a receiver-type hint.
    let mut types: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    // Parameters: prefer the `parameters` field, else a known container kind,
    // else (declarator grammars) the first container inside the declarator
    // chain — which cannot contain a nested function, so the search is safe.
    let params = node
        .child_by_field_name("parameters")
        .or_else(|| {
            named_children(node)
                .into_iter()
                .find(|c| spec.param_container_kinds.contains(&c.kind()))
        })
        .or_else(|| {
            let d = spec
                .declarator_field
                .and_then(|df| node.child_by_field_name(df))?;
            find_descendant_of_kinds(d, spec.param_container_kinds)
        });
    if let Some(params) = params {
        // A bare-identifier parameter list (Scala's `x => …`): the parameters
        // field IS the single parameter, not a container.
        let plist = if is_identifier(params.kind()) {
            vec![params]
        } else {
            named_children(params)
        };
        let mut idx = 1;
        for p in plist {
            // Prefer the explicit name/pattern field: in several grammars (e.g.
            // Java `String p`) the *type* is itself a `*_identifier` and precedes
            // the name in child order, so a blind first-identifier scan would
            // pick the type. Fall back to a scan only for bare-identifier params.
            // `declarator` before the blind scan: in C-family grammars the
            // type child precedes the name and is itself a `*_identifier`.
            let pname = p
                .child_by_field_name("name")
                .or_else(|| p.child_by_field_name("pattern"))
                .or_else(|| p.child_by_field_name("declarator"))
                .map(|n| innermost_identifier(n, src))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| innermost_identifier(p, src));
            if pname.is_empty() || pname == "self" {
                continue;
            }
            let ptype = p
                .child_by_field_name("type")
                .and_then(|t| resolved_type(spec, t, src))
                .unwrap_or_else(|| "ANY".to_string());
            if ptype != "ANY" {
                types.insert(pname.to_string(), ptype.clone());
            }
            let param = b.parameter(pname, &ptype, idx);
            b.ast_child(method, param);
            idx += 1;
        }
    }
    if let (Some(rf), Some(t)) = (spec.receiver_field, &recv_type) {
        if let Some(r) = node.child_by_field_name(rf) {
            let rname = innermost_identifier(r, src);
            if !rname.is_empty() && rname != t {
                types.insert(rname.to_string(), t.clone());
            }
        }
    }
    let ret = b.method_return("ANY");
    b.ast_child(method, ret);

    let block = b.block();
    b.ast_child(method, block);
    // Decorators (`@app.post("/score")` over a Python def, TS `@Get()`):
    // lowered INTO the decorated method, ahead of its body. A registration
    // decorator carries the route (entry mining), and an authz decorator
    // (`@require_admin`) is enforcement evidence that dominates the body —
    // both exactly the semantics of a call the method makes first.
    if let Some(p) = node.parent() {
        if p.kind() == "decorated_definition" || p.kind() == "decorated_declaration" {
            for d in named_children(p) {
                if d.kind() != "decorator" {
                    continue;
                }
                for inner in named_children(d) {
                    if let Some(e) = build_expr(spec, b, file, inner, src, &mut types) {
                        b.ast_child(block, e);
                    }
                }
            }
        }
    }
    // The body: the `body` field where the grammar has one; otherwise, for
    // anonymous functions only (e.g. Scala's lambda_expression, which has no
    // `body` field), the last named child after the parameters. Named
    // body-less declarations (abstract methods) get no body.
    let body = node.child_by_field_name("body").or_else(|| {
        if node.child_by_field_name("name").is_some() {
            return None;
        }
        named_children(node)
            .into_iter()
            .last()
            .filter(|c| params.is_none_or(|p| c.id() != p.id()))
    });
    if let Some(body) = body {
        if spec.implicit_return {
            walk_body_with_tail_return(spec, b, file, block, body, src, &mut types);
        } else {
            walk_stmts(spec, b, file, block, body, src, &mut types);
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
    types: &mut std::collections::HashMap<String, String>,
) {
    // A bare-expression body (`x => f(x)`, scala `def f = if (c) a else b`)
    // IS the tail expression — wrap it directly instead of iterating its
    // children as statements. Value shapes (branch/block bodies) count:
    // a method whose body is an if/match returns one of the branch values,
    // and summaries need that return link.
    // (is_value_block deliberately NOT accepted here: a braced method body
    // is kind `block` and must keep full statement walking below.)
    if is_tail_expression(spec, body.kind()) || is_value_branch(body.kind()) {
        if let Some(e) = build_expr(spec, b, file, body, src, types)
            .or_else(|| build_value_shape(spec, b, file, body, src, types))
        {
            let ret = b.ret(text(body, src), line(body));
            b.ast_child(ret, e);
            b.ast_child(block, ret);
            return;
        }
    }
    let children = named_children(body);
    let last = children.len().saturating_sub(1);
    for (i, c) in children.iter().enumerate() {
        let is_tail_expr =
            i == last && (is_tail_expression(spec, c.kind()) || is_value_branch(c.kind()));
        if is_tail_expr {
            if let Some(e) = build_expr(spec, b, file, *c, src, types)
                .or_else(|| build_value_shape(spec, b, file, *c, src, types))
            {
                let ret = b.ret(text(*c, src), line(*c));
                b.ast_child(ret, e);
                b.ast_child(block, ret);
                continue;
            }
        }
        walk_stmts(spec, b, file, block, *c, src, types);
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
fn walk_stmts(
    spec: &TsLangSpec,
    b: &mut CpgBuilder,
    file: NodeId,
    parent: NodeId,
    node: Node,
    src: &[u8],
    types: &mut std::collections::HashMap<String, String>,
) {
    let k = node.kind();
    if spec.is_function(k) {
        build_method(spec, b, file, node, src, None); // nested function → its own method
        return;
    }
    if spec.is_return(k) {
        let ret = b.ret(text(node, src), line(node));
        b.ast_child(parent, ret);
        for c in named_children(node) {
            if let Some(e) = build_expr(spec, b, file, c, src, types)
                .or_else(|| build_value_shape(spec, b, file, c, src, types))
            {
                b.ast_child(ret, e);
            }
        }
        return;
    }
    if spec.is_control(k) {
        let cs = b.control_structure(k, line(node));
        b.ast_child(parent, cs);
        bind_loop_var(spec, b, file, cs, node, src, types);
        for c in named_children(node) {
            walk_stmts(spec, b, file, cs, c, src, types);
        }
        return;
    }
    if spec.assign_form(k).is_some() {
        if let Some(e) = build_expr(spec, b, file, node, src, types) {
            b.ast_child(parent, e);
        }
        return;
    }
    if let Some(e) = build_expr(spec, b, file, node, src, types) {
        b.ast_child(parent, e);
        return;
    }
    // Structural node (block, statement_list, expression_statement, …): descend.
    for c in named_children(node) {
        walk_stmts(spec, b, file, parent, c, src, types);
    }
}

/// An iteration binding is an assignment in disguise: `for (const char& c :
/// path)` / `for x in xs` bind the loop variable to (an element of) the
/// iterated expression, and element taint must flow. Grammars that have one
/// expose it as a `declarator`/`left` binding plus a `right` source — either
/// on the control node itself (python for_statement, C++ for_range_loop) or
/// on a `*_clause` child (Go's range_clause); anything without both fields
/// is a no-op.
fn bind_loop_var(
    spec: &TsLangSpec,
    b: &mut CpgBuilder,
    file: NodeId,
    parent: NodeId,
    node: Node,
    src: &[u8],
    types: &mut std::collections::HashMap<String, String>,
) {
    let holder = if node.child_by_field_name("declarator").is_some()
        || node.child_by_field_name("left").is_some()
    {
        Some(node)
    } else {
        named_children(node).into_iter().find(|c| {
            c.kind().ends_with("_clause")
                && (c.child_by_field_name("declarator").is_some()
                    || c.child_by_field_name("left").is_some())
                && c.child_by_field_name("right").is_some()
        })
    };
    let Some(holder) = holder else { return };
    emit_loop_bindings(spec, b, file, parent, holder, line(node), src, types);
}

/// Emit one `x = <iterated>` binding per identifier in a loop pattern:
/// `for k, v in xs` binds BOTH k and v — a single-name binding drops every
/// identifier after the first, which is exactly where dict iteration
/// (`for name, info in cfg.items()`) carries its values. Each binding
/// rebuilds the iterated expression: a shared node would give one
/// expression several AST parents.
// Threads the shared mapping context (spec, builder, file, node, source,
// type map) that every emit_* helper in this module takes.
#[allow(clippy::too_many_arguments)]
fn emit_loop_bindings(
    spec: &TsLangSpec,
    b: &mut CpgBuilder,
    file: NodeId,
    parent: NodeId,
    holder: Node,
    ln: Option<u32>,
    src: &[u8],
    types: &mut std::collections::HashMap<String, String>,
) {
    let lhs = holder
        .child_by_field_name("declarator")
        .or_else(|| holder.child_by_field_name("left"));
    let (Some(lhs), Some(rhs)) = (lhs, holder.child_by_field_name("right")) else {
        return;
    };
    let mut names = Vec::new();
    pattern_identifiers(lhs, src, &mut names);
    names.truncate(8);
    let code = text(holder, src).lines().next().unwrap_or("").to_string();
    for name in names {
        if let Some(v) = build_expr(spec, b, file, rhs, src, types) {
            let assign = b.call("=", &code, ln);
            let lid = b.identifier(name, ln);
            b.add_argument(assign, lid, 1);
            b.add_argument(assign, v, 2);
            b.ast_child(parent, assign);
        }
    }
}

/// All identifier leaves of a binding pattern: `k, v` / `(a, b)` / `[x, y]`
/// yield every name; a plain identifier yields itself; any other shape
/// falls back to its first identifier (the old single-name behavior for
/// member/subscript lhs).
fn pattern_identifiers<'a>(node: Node<'a>, src: &'a [u8], out: &mut Vec<&'a str>) {
    if is_identifier(node.kind()) {
        out.push(text(node, src));
        return;
    }
    if matches!(
        node.kind(),
        "pattern_list"
            | "tuple_pattern"
            | "list_pattern"
            | "expression_list"
            | "array_pattern"
            | "parenthesized_expression"
            | "structured_binding_declarator"
            | "variable_declaration_list"
            | "tuple_expression"
    ) {
        for c in named_children(node) {
            pattern_identifiers(c, src, out);
        }
        return;
    }
    let one = innermost_identifier(node, src);
    if !one.is_empty() {
        out.push(one);
    }
}

/// Build an expression subtree, returning its root node id, or `None` if `node`
/// is not expression-shaped (callers then descend into it).
fn build_expr(
    spec: &TsLangSpec,
    b: &mut CpgBuilder,
    file: NodeId,
    node: Node,
    src: &[u8],
    types: &mut std::collections::HashMap<String, String>,
) -> Option<NodeId> {
    let k = node.kind();
    if spec.is_function(k) {
        // A function in expression position (lambda argument, function-valued
        // rhs): build it as a full method so its body gets CFG/DDG analysis,
        // and surface a MethodRef so the enclosing expression keeps a node.
        let m = build_method(spec, b, file, node, src, None);
        let mname = b.cpg.name_of(m).unwrap_or("<anon>").to_string();
        return Some(b.method_ref(&mname, line(node)));
    }
    if spec.is_call(k) {
        return Some(build_call(spec, b, node, src, file, types));
    }
    if let Some(form) = spec.assign_form(k) {
        return build_assignment(
            spec,
            b,
            file,
            node,
            form.lhs_field,
            form.rhs_field,
            src,
            types,
        );
    }
    // NOTE: blocks/branches in VALUE position (indented_block, block,
    // if_expression, match ...) are handled by `build_value_shape`, called
    // by value-position callers via `.or_else(..)` — build_expr itself
    // returns None for them so STATEMENT-position handling (walk_stmts
    // descent, control structures) is unchanged.
    // Comprehensions (`[f(x) for x in xs]` / `{k: g(v) for k, v in m.items()}`):
    // the clause is a loop binding in EXPRESSION position — none of the
    // control-structure machinery sees it, so the loop variables were never
    // bound and everything the body computes from them dropped its taint.
    // Emit the pattern bindings FIRST, stamped at the comprehension's start
    // line: the line-ordered taint pass must see the binding before the body
    // that reads it, and a dict comprehension's body sits textually ABOVE its
    // clause. The body lowers under an opaque call so element taint reaches
    // the comprehension's value; `if_clause` guards attach for visibility
    // (guard calls must exist in the graph) without carrying value taint.
    if k.ends_with("comprehension") || k == "generator_expression" {
        let call = b.call("<comprehension>", text(node, src), line(node));
        for c in named_children(node) {
            if c.kind() == "for_in_clause" {
                emit_loop_bindings(spec, b, file, call, c, line(node), src, types);
            } else if c.kind() == "if_clause" {
                for gc in named_children(c) {
                    if let Some(e) = build_expr(spec, b, file, gc, src, types) {
                        b.ast_child(call, e);
                    }
                }
            }
        }
        let mut idx = 1;
        for c in named_children(node) {
            if matches!(c.kind(), "for_in_clause" | "if_clause") {
                continue;
            }
            let kids: Vec<Node> = if c.kind() == "pair" {
                named_children(c)
            } else {
                vec![c]
            };
            for kid in kids {
                if let Some(e) = build_expr(spec, b, file, kid, src, types) {
                    b.add_argument(call, e, idx);
                    idx += 1;
                }
            }
        }
        return Some(call);
    }
    // Python keyword arguments (`f(key=value)`): keyword_argument is neither
    // call- nor assignment-shaped, so the whole argument — and any call
    // nested in its value — used to drop out of the graph entirely. Lower to
    // the nested `=` named-argument shape Scala produces (arg 1 = key
    // identifier, arg 2 = value): `=` is operator-shaped, so the value's
    // taint passes through to the enclosing call's argument position, and
    // the persistence named-arg harvest reads the key.
    if k == "keyword_argument" {
        let key = node
            .child_by_field_name("name")
            .map(|n| text(n, src))
            .filter(|t| !t.is_empty());
        let val = node
            .child_by_field_name("value")
            .and_then(|v| build_value(spec, b, file, v, src, types));
        return match (key, val) {
            (Some(key), Some(v)) => {
                let eq = b.call("=", text(node, src), line(node));
                let kid = b.identifier(key, line(node));
                b.add_argument(eq, kid, 1);
                b.add_argument(eq, v, 2);
                Some(eq)
            }
            (_, v) => v,
        };
    }
    if k == "literal_element" {
        // Go wraps every composite-literal element in a `literal_element`;
        // unwrap it here (it must be handled before `is_literal`, whose
        // substring match would otherwise swallow it as a constant).
        return named_children(node)
            .into_iter()
            .find_map(|c| build_expr(spec, b, file, c, src, types));
    }
    if k == "composite_literal"
        || k == "literal_value"
        || matches!(
            k,
            "object"
                | "array"
                | "list"
                | "tuple"
                | "set"
                | "dictionary"
                | "hash"
                | "tuple_expression"
        )
    {
        // Struct/composite literal: `Foo{K: v}` is constructor-shaped — a
        // Call named after the type whose elements are arguments, so value
        // taint reaches the constructed object (`is_literal` would otherwise
        // swallow it as an untainted constant and drop the flow entirely).
        // Keyed elements lower to the nested `=` named-argument shape
        // (`K = v`), the same shape Scala named args produce, so stores
        // through literal construction are visible to the persistence
        // stitch's named-arg harvest. A bare `literal_value` (nested
        // `[]Foo{{..}}`, map values) keeps its elements the same way under a
        // placeholder name. A JS/TS `object` literal (`{__html: x}`) is the
        // same shape with `pair` children, and python/JS collection
        // literals (`subprocess.run([bin, arg])`, `f([x])`) are the keyless
        // case — without this arm none of them is expression-shaped and
        // their values drop out of argument lists entirely.
        let tname = node
            .child_by_field_name("type")
            .map(|t| innermost_type_identifier(t, src).to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "<literal>".to_string());
        let call = b.call(&tname, text(node, src), line(node));
        let body = node.child_by_field_name("body").unwrap_or(node);
        let mut idx = 1;
        for el in named_children(body) {
            let built = if matches!(el.kind(), "keyed_element" | "pair") {
                let key = el
                    .child_by_field_name("key")
                    .map(|n| innermost_identifier(n, src).to_string())
                    .filter(|s| !s.is_empty());
                let val = el
                    .child_by_field_name("value")
                    .and_then(|v| build_expr(spec, b, file, v, src, types));
                match (key, val) {
                    (Some(kn), Some(v)) => {
                        let eq = b.call("=", text(el, src), line(el));
                        let kid = b.identifier(&kn, line(el));
                        b.add_argument(eq, kid, 1);
                        b.add_argument(eq, v, 2);
                        Some(eq)
                    }
                    (None, v) => v,
                    _ => None,
                }
            } else {
                build_expr(spec, b, file, el, src, types)
            };
            if let Some(e) = built {
                b.add_argument(call, e, idx);
                idx += 1;
            }
        }
        return Some(call);
    }
    // A string with interpolation is concatenation, not a constant:
    // `` sql(`.. ${x}`) `` / python f"..{x}" / scala s"..$x" must carry x's
    // taint, but every such node is string-shaped and would be swallowed by
    // the literal branch below. Lower to a `+`-shaped call over the
    // substituted expressions (operator semantics: the result carries any
    // operand's taint). Substitutions may sit one level down (scala wraps
    // them in an `interpolated_string` child); plain strings have none and
    // still fall through to the literal branch.
    if k.contains("string") {
        let is_sub = |c: &Node| matches!(c.kind(), "template_substitution" | "interpolation");
        let mut subs: Vec<Node> = Vec::new();
        for c in named_children(node) {
            if is_sub(&c) {
                subs.push(c);
            } else if c.kind().contains("string") {
                subs.extend(named_children(c).into_iter().filter(is_sub));
            }
        }
        if !subs.is_empty() {
            let call = b.call("+", text(node, src), line(node));
            let mut idx = 1;
            for s in subs {
                if let Some(e) = named_children(s)
                    .into_iter()
                    .find_map(|c| build_expr(spec, b, file, c, src, types))
                {
                    b.add_argument(call, e, idx);
                    idx += 1;
                }
            }
            return Some(call);
        }
    }
    if is_literal(k) || k == "sizeof_expression" {
        // sizeof(x) is a compile-time constant: modelling it as a literal
        // keeps sink argument indices aligned (snprintf(buf, sizeof(buf),
        // fmt, ..) must see fmt at position 2) and correctly untainted.
        return Some(b.literal(text(node, src), line(node)));
    }
    if is_identifier(k) {
        return Some(b.identifier(text(node, src), line(node)));
    }
    if is_member(k) {
        // Member/selector access. When the grammar names the accessed field
        // (`c.executionUser` — scala/go/java/cpp `field`, python `attribute`,
        // JS `property`), lower to a Call named after the field with the
        // base as its argument: taint still flows base -> read (an opaque
        // named call propagates its arguments' taint), but the field NAME
        // now exists in the graph for source/sink specs and the persistence
        // stitch to match. Subscripts, scoped identifiers, and C++
        // qualified_identifier have no such child and keep the old
        // base-collapse (a qualified_identifier's `scope` is a namespace,
        // not a value — it falls through to the identifier fallback).
        for f in ["object", "operand", "value", "receiver", "argument"] {
            if let Some(base) = node.child_by_field_name(f) {
                let fname = ["field", "attribute", "property"]
                    .iter()
                    .find_map(|ff| node.child_by_field_name(ff))
                    .map(|n| text(n, src))
                    .filter(|t| !t.is_empty());
                if let Some(fname) = fname {
                    if let Some(base_expr) = build_expr(spec, b, file, base, src, types) {
                        let call = b.call(fname, text(node, src), line(node));
                        b.add_argument(call, base_expr, 1);
                        if let Some(r) = root_identifier(node, src).filter(|r| !r.is_empty()) {
                            let sym = b.cpg.intern(r);
                            b.cpg.set_signature(call, sym);
                            // Receiver-type hint, same as on member CALLS: a
                            // member VALUE (`wrapper.ListPets` as a handler
                            // argument) names which type's method it refers
                            // to when the base is locally typed — entry
                            // mining uses it to break simple-name ties.
                            if let Some(t) = types.get(r) {
                                let sym = b.cpg.intern(t);
                                b.cpg.set_type_full_name(call, sym);
                            }
                        }
                        return Some(call);
                    }
                }
                return build_expr(spec, b, file, base, src, types);
            }
        }
        let name = callee_name(node, src);
        return Some(b.identifier(&name, line(node)));
    }
    if is_binary(k) {
        let op = node
            .child_by_field_name("operator")
            .map(|n| text(n, src))
            .unwrap_or("<op>");
        let call = b.call(op, text(node, src), line(node));
        let mut idx = 1;
        for c in named_children(node) {
            if let Some(e) = build_expr(spec, b, file, c, src, types) {
                b.add_argument(call, e, idx);
                idx += 1;
            }
        }
        return Some(call);
    }
    // Constructor call: `new Foo(args)` is a call named after the type —
    // its arguments carry dataflow exactly like a function call's
    // (`new String(tainted.c_str())` must not drop the taint). C++/JS use
    // new_expression, Scala instance_expression, Java
    // object_creation_expression; the type lives in a field or (Scala)
    // positionally before the arguments.
    if matches!(
        k,
        "new_expression" | "instance_expression" | "object_creation_expression"
    ) {
        let ctor = node
            .child_by_field_name("type")
            .or_else(|| node.child_by_field_name("constructor"))
            .map(|t| innermost_type_identifier(t, src).to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                Some(innermost_type_identifier(node, src).to_string()).filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| "<new>".to_string());
        let call = b.call(&ctor, text(node, src), line(node));
        if let Some(args) = node.child_by_field_name("arguments") {
            let mut idx = 1;
            for a in named_children(args) {
                if let Some(e) = build_expr(spec, b, file, a, src, types) {
                    b.add_argument(call, e, idx);
                    idx += 1;
                }
            }
        }
        return Some(call);
    }
    // JSX (JS/TSX): markup is call-shaped. `<Tag attr={v}>{child}</Tag>`
    // lowers to a Call named after the tag whose arguments are the lowered
    // attributes and expression children, and each attribute lowers to a
    // Call named after the attribute with its bound value as argument 1 —
    // the same shape as the member-read lowering, so attribute names exist
    // in the graph for sink specs (`dangerouslySetInnerHTML@0`). Without
    // this arm a component's whole return value is not expression-shaped
    // and everything inside the markup drops out of the dataflow.
    if k == "jsx_expression" {
        // The `{...}` brace container: a transparent wrapper.
        return named_children(node)
            .into_iter()
            .find_map(|c| build_expr(spec, b, file, c, src, types));
    }
    if k == "jsx_attribute" {
        let mut kids = named_children(node).into_iter();
        let name = kids
            .next()
            .map(|n| text(n, src).to_string())
            .filter(|s| !s.is_empty());
        let name = name?;
        let call = b.call(&name, text(node, src), line(node));
        if let Some(v) = kids.find_map(|c| build_expr(spec, b, file, c, src, types)) {
            b.add_argument(call, v, 1);
        }
        return Some(call);
    }
    if matches!(
        k,
        "jsx_element" | "jsx_self_closing_element" | "jsx_fragment"
    ) {
        let open = named_children(node)
            .into_iter()
            .find(|c| c.kind() == "jsx_opening_element");
        let tag_holder = open.unwrap_or(node);
        let tag = tag_holder
            .child_by_field_name("name")
            .map(|n| text(n, src).to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "<jsx>".to_string());
        let call = b.call(&tag, text(node, src), line(node));
        let name_id = tag_holder.child_by_field_name("name").map(|n| n.id());
        let mut parts: Vec<Node> = Vec::new();
        for c in named_children(node) {
            match c.kind() {
                "jsx_opening_element" => parts.extend(
                    named_children(c)
                        .into_iter()
                        .filter(|a| Some(a.id()) != name_id),
                ),
                "jsx_closing_element" => {}
                _ if Some(c.id()) == name_id => {}
                _ => parts.push(c),
            }
        }
        let mut idx = 1;
        for p in parts {
            if let Some(e) = build_expr(spec, b, file, p, src, types) {
                b.add_argument(call, e, idx);
                idx += 1;
            }
        }
        return Some(call);
    }
    // Expression wrappers that delegate to an inner expression.
    // prefix/postfix/ascription/generic_function are Scala (`!x`, `x_=`,
    // `x: T`, `f[T]`), type_assertion_expression is Go (`x.(T)`) — all were
    // measured dropping assignment values in a multi-language validation corpus.
    if matches!(
        k,
        "parenthesized_expression" | "expression_list" | "unary_expression" | "unary_operator"
            | "await_expression" | "reference_expression" | "try_expression" | "group"
            | "argument" | "spread_element" | "cast_expression" | "pointer_expression"
            | "prefix_expression" | "postfix_expression" | "ascription_expression"
            | "generic_function" | "type_assertion_expression"
            // Go `f(xs...)` — the spread wraps the slice expression; without
            // this the whole argument silently vanished from the call
            // (measured: exec.Command(name, args...) lost `args`).
            | "variadic_argument"
    ) {
        return named_children(node)
            .into_iter()
            .find_map(|c| build_expr(spec, b, file, c, src, types));
    }
    // Not an expression (block, statement, declaration container, …). Returning
    // None lets `walk_stmts` descend and process each child as a statement —
    // crucially, this is what stops a multi-statement block or a branch body
    // from collapsing to just its first expression.
    None
}

/// Whether a kind is a block that yields its last expression when it sits
/// in VALUE position (`val x = { a; b }`, ruby `begin..end`, then/else
/// bodies). In statement position these stay walk_stmts territory.
fn is_value_block(k: &str) -> bool {
    matches!(
        k,
        "block" | "indented_block" | "begin" | "then" | "else" | "body_statement"
    )
}

/// Whether a kind is a branch construct that yields one of its branches'
/// values when it sits in VALUE position (`val x = if (c) a else b`,
/// match/case, try/catch, ruby ternary, `a rescue b`).
fn is_value_branch(k: &str) -> bool {
    matches!(
        k,
        "if_expression"
            | "match_expression"
            | "case_block"
            | "try_expression"
            | "for_expression"
            | "conditional_expression"
            | "conditional"
            | "if"
            | "unless"
            | "case"
            | "elsif"
            | "ternary_expression"
            | "switch_expression"
            | "rescue_modifier"
    )
}

/// Value-position fallback for shapes `build_expr` deliberately treats as
/// statements: blocks-with-a-value and branches-as-values. `build_expr`
/// returning None for these is correct in STATEMENT position (walk_stmts
/// descends), but in value position (assignment rhs, call argument, return)
/// it used to drop the whole link — observed at scale in a Scala validation
/// corpus, where each dropped assignment was invisible to dataflow AND to
/// plain call visibility (walk_stmts does not descend into failed assignments).
/// Callers in value position use `build_expr(..).or_else(|| build_value_shape(..))`.
fn build_value_shape(
    spec: &TsLangSpec,
    b: &mut CpgBuilder,
    file: NodeId,
    node: Node,
    src: &[u8],
    types: &mut std::collections::HashMap<String, String>,
) -> Option<NodeId> {
    let k = node.kind();
    if is_value_block(k) {
        // Every child is built; the value is the LAST child that yields an
        // expression — recursively, so a block ending in a branch still has
        // a value. Earlier (side-effect) children are AST-attached under
        // the value node: analysis iterates ast_descendants of the method,
        // so a floating node is INVISIBLE to the statement walk, not just
        // unlinked — a sink call before the block's value would silently
        // stop being analysed (line sorting restores statement order).
        let mut built: Vec<NodeId> = Vec::new();
        for c in named_children(node) {
            if let Some(e) = build_expr(spec, b, file, c, src, types)
                .or_else(|| build_value_shape(spec, b, file, c, src, types))
            {
                built.push(e);
            }
        }
        let last = built.pop();
        if let Some(v) = last {
            for e in built {
                b.ast_child(v, e);
            }
        }
        return last;
    }
    if is_value_branch(k) {
        // An opaque call named "<branch>" whose arguments are each branch's
        // value: summary-less call semantics propagate any argument's taint
        // to the result — exactly the conservative reading of "the value is
        // one of the branches". The condition is built for visibility
        // (guard calls must exist in the graph) but is NOT an argument, so
        // condition taint does not leak into the value where the grammar
        // names the field. (Grammars without a condition field — python's
        // ternary — over-approximate by including it; conservative.)
        let cond_id = node.child_by_field_name("condition").map(|n| n.id());
        let call = b.call("<branch>", text(node, src), line(node));
        let mut idx = 1;
        for c in named_children(node) {
            if Some(c.id()) == cond_id {
                if let Some(e) = build_expr(spec, b, file, c, src, types) {
                    b.ast_child(call, e);
                }
                continue;
            }
            let built = build_expr(spec, b, file, c, src, types)
                .or_else(|| build_value_shape(spec, b, file, c, src, types))
                .or_else(|| {
                    // One-level descend: case_clause / else_clause / when
                    // wrappers hold their value one level down (last
                    // buildable grandchild = the clause body's value).
                    // Earlier grandchildren attach under it — same
                    // no-floating-nodes rule as the block arm.
                    let mut built: Vec<NodeId> = Vec::new();
                    for gc in named_children(c) {
                        if let Some(e) = build_expr(spec, b, file, gc, src, types)
                            .or_else(|| build_value_shape(spec, b, file, gc, src, types))
                        {
                            built.push(e);
                        }
                    }
                    let last = built.pop();
                    if let Some(v) = last {
                        for e in built {
                            b.ast_child(v, e);
                        }
                    }
                    last
                });
            if let Some(e) = built {
                b.add_argument(call, e, idx);
                idx += 1;
            }
        }
        return Some(call);
    }
    None
}

fn build_call(
    spec: &TsLangSpec,
    b: &mut CpgBuilder,
    node: Node,
    src: &[u8],
    file: NodeId,
    types: &mut std::collections::HashMap<String, String>,
) -> NodeId {
    let callee = node
        .child_by_field_name(spec.callee_field)
        .or_else(|| node.child_by_field_name("macro"));
    let mut name = callee
        .map(|c| callee_name(c, src))
        .unwrap_or_else(|| "<anon>".into());
    // Constructor factories (`std::make_shared<T>(args)`): the constructed
    // type is a template argument, not the callee name — surface the call
    // under T so type-named source/sink specs can match the construction.
    if spec.ctor_factories.contains(&name.as_str()) {
        if let Some(t) = callee
            .and_then(|c| {
                find_descendant_of_kinds(c, &["template_argument_list", "type_arguments"])
            })
            .and_then(|args| named_children(args).into_iter().last())
            .map(|last| innermost_type_identifier(last, src))
            .filter(|t| !t.is_empty())
        {
            name = t.to_string();
        }
    }
    let call = b.call(&name, text(node, src), line(node));

    // Arguments: the `arguments` field for normal calls, or a Rust macro's
    // `token_tree` child (println!/format!/… carry their args there as an
    // unnamed child, not in a field).
    let args = node.child_by_field_name("arguments").or_else(|| {
        named_children(node)
            .into_iter()
            .find(|c| c.kind() == "token_tree")
    });
    if let Some(args) = args {
        let mut idx = 1;
        for a in named_children(args) {
            // Value-shape fallback: `f(if (c) a else b)` / `f({ ..; v })`
            // previously dropped the argument entirely — which also shifted
            // every later argument's index, misaligning name@argIdx sinks.
            if let Some(arg) = build_expr(spec, b, file, a, src, types)
                .or_else(|| build_value_shape(spec, b, file, a, src, types))
            {
                b.add_argument(call, arg, idx);
                idx += 1;
            }
        }
    }
    // Receiver, if the call carries one (method calls). A chained call's
    // base (`f(x).g()`) is itself a call that must exist in the graph —
    // without this, the inner call (often the security-relevant one, e.g.
    // `exec.Command(...)` in `exec.Command(...).Run()`) is silently dropped.
    // A member-chain base (`request.body.asJson.getOrElse(..)`) is built for
    // the same reason: its field reads lower to named Calls, and those names
    // are what source specs match — a framework's request accessors are
    // almost always consumed in receiver position, so collapsing the chain
    // to its root identifier made them unmatchable. Bare identifier bases
    // stay un-built: the signature stamp below already carries them and a
    // lone Identifier node adds nothing a spec could match.
    let recv_node = node
        .child_by_field_name("receiver")
        .or_else(|| node.child_by_field_name("object"))
        .or_else(|| {
            callee.filter(|c| is_member(c.kind())).and_then(|c| {
                ["object", "operand", "value", "receiver", "argument"]
                    .iter()
                    .find_map(|f| c.child_by_field_name(f))
                    .filter(|base| {
                        spec.is_call(base.kind())
                            || is_member(base.kind())
                            || matches!(
                                base.kind(),
                                "new_expression"
                                    | "instance_expression"
                                    | "object_creation_expression"
                            )
                    })
            })
        });
    if let Some(recv) = recv_node {
        if let Some(r) = build_expr(spec, b, file, recv, src, types) {
            b.add_receiver(call, r);
        }
    }
    // Receiver-type hint: a member call whose base is a variable of locally
    // known type (`s.Handle(..)` where `s *Server`) records that type on the
    // call, so resolution can prefer methods of that receiver. A C++
    // qualified callee (`Foo::bar(..)`) names its scope outright — use the
    // trailing scope segment as the hint directly.
    if let Some(c) = callee {
        if c.kind() == "qualified_identifier" {
            // Innermost scope segment is the class (`ns::Foo::bar` -> Foo);
            // the chain nests right-associatively.
            let mut cur = c;
            let mut hint = None;
            while cur.kind() == "qualified_identifier" {
                hint = cur.child_by_field_name("scope").map(|s| text(s, src));
                match cur.child_by_field_name("name") {
                    Some(n) => cur = n,
                    None => break,
                }
            }
            if let Some(t) = hint.filter(|t| !t.is_empty()) {
                let sym = b.cpg.intern(t);
                b.cpg.set_type_full_name(call, sym);
            }
        } else if is_member(c.kind()) {
            let base = ["object", "operand", "value", "receiver", "argument"]
                .iter()
                .find_map(|f| c.child_by_field_name(f));
            if let Some(base) = base {
                // Which variable the call dispatches through: a bare
                // identifier, the field of an explicit `this->field_->m()`,
                // or the ROOT identifier of a member chain
                // (`request.args.get(..)` dispatches through `request`).
                // Stamped into the (otherwise unused) signature column so the
                // call-graph pass can look the receiver up in the enclosing
                // class's members and taint can pass through opaque calls —
                // locals/params can't type class fields.
                let recv_name = if is_identifier(base.kind()) {
                    Some(text(base, src))
                } else if is_member(base.kind())
                    && first_named_child(base).is_some_and(|o| o.kind() == "this")
                {
                    base.child_by_field_name("field").map(|f| text(f, src))
                } else if is_member(base.kind()) {
                    root_identifier(base, src)
                } else if is_literal(base.kind()) {
                    // `"{}".format(..)`: the receiver exists but is not a
                    // variable. The sentinel marks the call as a method call
                    // on a value (dynamic dispatch) without naming anything
                    // a member/taint lookup could match.
                    Some("<literal>")
                } else {
                    None
                };
                if let Some(r) = recv_name.filter(|r| !r.is_empty()) {
                    let sym = b.cpg.intern(r);
                    b.cpg.set_signature(call, sym);
                    if let Some(t) = types.get(r) {
                        let sym = b.cpg.intern(t);
                        b.cpg.set_type_full_name(call, sym);
                    }
                }
            }
        }
    }
    call
}

// Threads the shared mapping context; see `emit_loop_bindings`.
#[allow(clippy::too_many_arguments)]
fn build_assignment(
    spec: &TsLangSpec,
    b: &mut CpgBuilder,
    file: NodeId,
    node: Node,
    lhs_field: &str,
    rhs_field: &str,
    src: &[u8],
    types: &mut std::collections::HashMap<String, String>,
) -> Option<NodeId> {
    let name = node
        .child_by_field_name(lhs_field)
        .map(|n| innermost_identifier(n, src).to_string())
        .unwrap_or_default();
    // Local type inference for the receiver-hint map, cheapest wins only:
    // an explicit declared type (`var x Foo`), a composite literal
    // (`x := Foo{..}` / `&Foo{..}`), a constructor call (`x := NewFoo(..)`),
    // or a type-named call (`Foo(..)`: Go conversion / Scala apply).
    if !name.is_empty() {
        let declared = node
            .child_by_field_name("type")
            .and_then(|t| resolved_type(spec, t, src));
        let inferred = declared.or_else(|| {
            let mut v = node.child_by_field_name(rhs_field)?;
            while matches!(
                v.kind(),
                "unary_expression" | "parenthesized_expression" | "expression_list"
            ) {
                v = first_named_child(v)?;
            }
            if v.kind() == "composite_literal" {
                return v
                    .child_by_field_name("type")
                    .map(|t| innermost_type_identifier(t, src).to_string());
            }
            if spec.is_call(v.kind()) {
                let cn = callee_name(v.child_by_field_name(spec.callee_field)?, src);
                let stripped = cn.strip_prefix("New").unwrap_or(&cn);
                if !stripped.is_empty() && stripped.chars().next().is_some_and(|c| c.is_uppercase())
                {
                    return Some(stripped.to_string());
                }
            }
            None
        });
        if let Some(t) = inferred {
            types.insert(name.clone(), t);
        }
    }
    let mut value = node
        .child_by_field_name(rhs_field)
        .and_then(|v| build_value(spec, b, file, v, src, types));
    // C++ direct-initialization (`Type var(args);`): the init_declarator has
    // no `value` field — the initializer is a bare argument_list child, and
    // treating it like the missing-rhs case would swallow the whole statement
    // (dropping any `new T(...)` inside it from the graph). Lower it as
    // `var = Type(args)`: a call named after the declared type carrying the
    // built arguments, so the constructor and its dataflow stay visible.
    if value.is_none() {
        if let Some(args) = named_children(node)
            .into_iter()
            .find(|c| c.kind() == "argument_list")
        {
            let tyname = node
                .parent()
                .and_then(|p| p.child_by_field_name("type"))
                .map(|t| innermost_type_identifier(t, src).to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "<ctor>".to_string());
            let call = b.call(&tyname, text(node, src), line(node));
            let mut idx = 1;
            for a in named_children(args) {
                if let Some(e) = build_expr(spec, b, file, a, src, types) {
                    b.add_argument(call, e, idx);
                    idx += 1;
                }
            }
            value = Some(call);
        }
    }
    if value.is_none() {
        // Recall audit hook: an assignment whose rhs is not
        // expression-shaped drops the value link (and often the whole
        // statement). Log the rhs kind so unhandled value-position shapes
        // (block-in-expression, branch-as-value) can be sized on real
        // repos instead of guessed.
        if std::env::var_os("CPG_DEBUG_DROPPED_RHS").is_some() {
            if let Some(r) = node.child_by_field_name(rhs_field) {
                eprintln!(
                    "dropped-rhs\t{}\t{}",
                    r.kind(),
                    text(node, src).lines().next().unwrap_or("")
                );
            }
        }
    }
    match (name.is_empty(), value) {
        (false, Some(v)) => {
            let assign = b.call("=", text(node, src), line(node));
            let lhs = b.identifier(&name, line(node));
            b.add_argument(assign, lhs, 1);
            b.add_argument(assign, v, 2);
            bind_extra_targets(
                spec, b, file, node, lhs_field, rhs_field, assign, src, types,
            );
            Some(assign)
        }
        (true, Some(v)) => Some(v),
        _ => None,
    }
}

/// Multi-target destructuring: `a, b = f()` / `v, err := f(x)` bind EVERY
/// name — the primary assignment covers only the first identifier, which is
/// exactly wrong when the taint-relevant value lands in a later one
/// (`ok, data = parse(q)`). `a, b = x, y` binds pairwise when the two sides
/// have the same length. Secondary bindings attach as AST children of the
/// primary assignment: the descendant walk sees them, and they are not
/// arguments, so they never leak into the primary's value.
// Threads the shared mapping context; see `emit_loop_bindings`.
#[allow(clippy::too_many_arguments)]
fn bind_extra_targets(
    spec: &TsLangSpec,
    b: &mut CpgBuilder,
    file: NodeId,
    node: Node,
    lhs_field: &str,
    rhs_field: &str,
    primary: NodeId,
    src: &[u8],
    types: &mut std::collections::HashMap<String, String>,
) {
    let Some(lhs) = node.child_by_field_name(lhs_field) else {
        return;
    };
    let mut names = Vec::new();
    pattern_identifiers(lhs, src, &mut names);
    if names.len() < 2 {
        return;
    }
    names.truncate(8);
    let Some(rhs) = node.child_by_field_name(rhs_field) else {
        return;
    };
    let vals: Vec<Node> = if matches!(rhs.kind(), "expression_list" | "tuple" | "tuple_expression")
    {
        named_children(rhs)
    } else {
        Vec::new()
    };
    let pairwise = vals.len() == names.len();
    for (i, nm) in names.iter().enumerate().skip(1) {
        let src_node = if pairwise { vals[i] } else { rhs };
        let Some(v) = build_value(spec, b, file, src_node, src, types) else {
            continue;
        };
        let assign = b.call("=", text(node, src), line(node));
        let lid = b.identifier(nm, line(node));
        b.add_argument(assign, lid, 1);
        b.add_argument(assign, v, 2);
        b.ast_child(primary, assign);
    }
}

/// Unwrap list/paren wrappers, then build the underlying expression.
/// Falls back to the value-shape lowering so `x = { .. }` / `x = if ..`
/// keep their value link instead of dropping the whole statement.
fn build_value(
    spec: &TsLangSpec,
    b: &mut CpgBuilder,
    file: NodeId,
    node: Node,
    src: &[u8],
    types: &mut std::collections::HashMap<String, String>,
) -> Option<NodeId> {
    let mut n = node;
    while matches!(n.kind(), "expression_list" | "parenthesized_expression") {
        match first_named_child(n) {
            Some(c) => n = c,
            None => break,
        }
    }
    build_expr(spec, b, file, n, src, types)
        .or_else(|| build_value_shape(spec, b, file, n, src, types))
}

// --- node-kind predicates (language-independent heuristics) ---

fn is_identifier(k: &str) -> bool {
    // qualified_identifier (C++ `a::b`) is member-shaped, not a plain name.
    // Ruby spells a constant reference (`X = Y::CONST`) as bare `constant`.
    (k == "identifier" || k.ends_with("_identifier") || k == "constant")
        && k != "qualified_identifier"
}

fn is_member(k: &str) -> bool {
    // Subscripts count as member-shaped: `request.args['x']` / `argv[1]`
    // surface their base the same way `a.b` does, so the base's taint flows
    // instead of the whole expression being dropped from the argument list.
    matches!(
        k,
        "selector_expression"
            | "member_expression"
            | "field_expression"
            | "scoped_identifier"
            | "attribute"
            | "field_access"
            | "scoped_call_expression"
            | "qualified_identifier"
            | "subscript"
            | "subscript_expression"
            | "index_expression"
            | "element_access_expression"
            | "element_reference"
            | "slice_expression"
    )
}

fn is_binary(k: &str) -> bool {
    // Scala spells every operator application `infix_expression`; Ruby uses
    // bare `binary`. Operator-propagation semantics (result carries any
    // operand's taint) are right for all of them.
    matches!(
        k,
        "binary_expression"
            | "binary_operator"
            | "boolean_operator"
            | "comparison_operator"
            | "infix_expression"
            | "binary"
    )
}

fn is_literal(k: &str) -> bool {
    // `contains("string")` catches substitution-free interpolated strings
    // (scala `s"constant"` is interpolated_string_expression — measured
    // dropping 415 assignment values in one service). Safe because the
    // string-interpolation arm in build_expr runs FIRST and returns for
    // any string that actually carries substitutions.
    k.contains("literal")
        || k.contains("string")
        || matches!(
            k,
            "number"
                | "integer"
                | "float"
                | "true"
                | "false"
                | "nil"
                | "null"
                | "none"
                | "boolean"
                | "character"
                | "simple_symbol"
                | "regex"
                | "unit"
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

/// First *type* identifier in a subtree: prefers `type_identifier`-style
/// tokens (Go's `*Server` -> `Server`), falling back to the last identifier.
fn innermost_type_identifier<'a>(node: Node, src: &'a [u8]) -> &'a str {
    fn walk<'a>(node: Node, src: &'a [u8], any: &mut &'a str) -> Option<&'a str> {
        let k = node.kind();
        if k.ends_with("type_identifier") || k == "primitive_type" {
            return Some(text(node, src));
        }
        if is_identifier(k) {
            *any = text(node, src);
        }
        for c in named_children(node) {
            if let Some(r) = walk(c, src, any) {
                return Some(r);
            }
        }
        None
    }
    let mut any = "";
    walk(node, src, &mut any).unwrap_or(any)
}

/// Declared type of a member/param/local, unwrapping spec-listed smart
/// pointers: `std::shared_ptr<filesvc::FileServiceIf>` names `shared_ptr` as its
/// head type, but the type that matters for call resolution is the pointee.
/// `innermost_type_identifier` itself stays untouched — it also types Java
/// generics (`List<String>` must stay `List`), where descending template
/// arguments would be wrong.
fn resolved_type(spec: &TsLangSpec, node: Node, src: &[u8]) -> Option<String> {
    let head = innermost_type_identifier(node, src);
    if head.is_empty() {
        return None;
    }
    if spec.smart_ptr_names.contains(&head) {
        if let Some(args) = find_descendant_of_kinds(node, &["template_argument_list"]) {
            if let Some(last) = named_children(args).into_iter().last() {
                let inner = innermost_type_identifier(last, src);
                if !inner.is_empty() {
                    return Some(inner.to_string());
                }
            }
        }
    }
    Some(head.to_string())
}

/// The leftmost identifier a member chain hangs off: `request.args.files` →
/// `request`. Descends base fields only, so a chain rooted at a call or
/// literal yields None.
fn root_identifier<'a>(node: Node, src: &'a [u8]) -> Option<&'a str> {
    let mut cur = node;
    loop {
        if is_identifier(cur.kind()) {
            return Some(text(cur, src));
        }
        if !is_member(cur.kind()) {
            return None;
        }
        cur = ["object", "operand", "value", "receiver", "argument"]
            .iter()
            .find_map(|f| cur.child_by_field_name(f))?;
    }
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

/// First descendant of one of `kinds` (breadth-limited by the declarator
/// chain's natural depth — used to find a parameter_list inside it).
fn find_descendant_of_kinds<'a>(node: Node<'a>, kinds: &[&str]) -> Option<Node<'a>> {
    if kinds.contains(&node.kind()) {
        return Some(node);
    }
    named_children(node)
        .into_iter()
        .find_map(|c| find_descendant_of_kinds(c, kinds))
}

/// Name + qualifying scope from a C-style declarator chain. Descends through
/// wrapper declarators (pointer/reference) to the innermost
/// function_declarator's own declarator:
///   `bar`        -> ("bar", None)
///   `Foo::bar`   -> ("bar", Some("Foo"))   (trailing scope segment)
///   `~Foo` / `operator+` -> the literal text, no scope
fn declarator_name(node: Node, src: &[u8]) -> Option<(String, Option<String>)> {
    // The innermost node that owns a parameter list is the function
    // declarator; its `declarator` field is the name.
    let fdecl = find_descendant_of_kinds(node, &["function_declarator"])?;
    let mut name_node = fdecl.child_by_field_name("declarator")?;
    if name_node.kind() == "qualified_identifier" {
        // `ns::Foo::bar` nests right-associatively: qualified(scope: ns,
        // name: qualified(scope: Foo, name: bar)). Descend to the innermost
        // level — its scope is the class, which is what type-hint matching
        // compares against.
        let mut scope = None;
        while name_node.kind() == "qualified_identifier" {
            scope = name_node.child_by_field_name("scope").map(|s| text(s, src));
            match name_node.child_by_field_name("name") {
                Some(n) => name_node = n,
                None => break,
            }
        }
        let scope = scope.map(|s| s.to_string()).filter(|s| !s.is_empty());
        return Some((text(name_node, src).to_string(), scope));
    }
    // identifier / field_identifier / destructor_name / operator_name — the
    // literal text is the name in every case.
    Some((text(name_node, src).to_string(), None))
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
