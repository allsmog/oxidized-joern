use anyhow::{anyhow, Context, Result};
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use tree_sitter::{Node, Parser, Tree};

thread_local! {
    static BYTE_OFFSET_MAP: RefCell<Option<Vec<usize>>> = const { RefCell::new(None) };
    /// Tracks tree-sitter node kinds the lowering passes could not map, keyed by
    /// kind with an occurrence count. `None` disables tracking so that library and
    /// test callers stay free of side effects; the CLI opts in via
    /// [`with_unmapped_summary`]. Thread-local to mirror `BYTE_OFFSET_MAP` and to
    /// keep parallel test runs isolated.
    static UNMAPPED_KINDS: RefCell<Option<BTreeMap<String, usize>>> = const { RefCell::new(None) };
}

/// Records an unmapped tree-sitter `kind`. No-op unless tracking is enabled.
fn record_unmapped_kind(kind: &str) {
    UNMAPPED_KINDS.with(|cell| {
        if let Some(counts) = cell.borrow_mut().as_mut() {
            *counts.entry(kind.to_string()).or_insert(0) += 1;
        }
    });
}

/// Enables unmapped-kind tracking for the duration of `f`, then returns a summary
/// line such as `phpastgen: 3 unmapped node(s): kind1(x2), kind2(x1)`, or `None`
/// when nothing was unmapped. Intended for CLI runs.
pub fn with_unmapped_summary<T>(f: impl FnOnce() -> T) -> (T, Option<String>) {
    UNMAPPED_KINDS.with(|cell| *cell.borrow_mut() = Some(BTreeMap::new()));
    let result = f();
    let counts = UNMAPPED_KINDS.with(|cell| cell.borrow_mut().take().unwrap_or_default());
    let summary = if counts.is_empty() {
        None
    } else {
        let total: usize = counts.values().sum();
        let detail = counts
            .iter()
            .map(|(kind, count)| format!("{kind}(x{count})"))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!("phpastgen: {total} unmapped node(s): {detail}"))
    };
    (result, summary)
}

pub fn generate_file(path: &Path) -> Result<Value> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let (source, offset_map) = decode_source_bytes(bytes);
    generate_source_with_offset_map(&source, offset_map)
}

pub fn generate_source(source: &str) -> Result<Value> {
    generate_source_with_offset_map(source, None)
}

fn generate_source_with_offset_map(source: &str, offset_map: Option<Vec<usize>>) -> Result<Value> {
    let normalized_source = normalize_recoverable_source(source);
    let parse_source = normalized_source.as_deref().unwrap_or(source);
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
        .map_err(|err| anyhow!("failed to initialize PHP grammar: {err:?}"))?;
    let tree = parser
        .parse(parse_source, None)
        .ok_or_else(|| anyhow!("tree-sitter failed to parse source"))?;
    let active_offset_map = offset_map.filter(|_| normalized_source.is_none());
    Ok(with_byte_offset_map(active_offset_map, || {
        lower_tree(&tree, parse_source)
    }))
}

pub fn debug_sexp(source: &str) -> Result<String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
        .map_err(|err| anyhow!("failed to initialize PHP grammar: {err:?}"))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow!("tree-sitter failed to parse source"))?;
    Ok(tree.root_node().to_sexp())
}

fn decode_source_bytes(bytes: Vec<u8>) -> (String, Option<Vec<usize>>) {
    match String::from_utf8(bytes) {
        Ok(source) => (source, None),
        Err(err) => decode_latin1_source(&err.into_bytes()),
    }
}

fn decode_latin1_source(bytes: &[u8]) -> (String, Option<Vec<usize>>) {
    let mut source = String::with_capacity(bytes.len());
    let mut offset_map = Vec::with_capacity(bytes.len() + 1);
    offset_map.push(0);

    for (original_offset, byte) in bytes.iter().enumerate() {
        let ch = char::from(*byte);
        let mut encoded = [0u8; 4];
        let encoded = ch.encode_utf8(&mut encoded);
        source.push_str(encoded);
        for idx in 0..encoded.len() {
            let mapped_offset = if idx + 1 == encoded.len() {
                original_offset + 1
            } else {
                original_offset
            };
            offset_map.push(mapped_offset);
        }
    }

    (source, Some(offset_map))
}

fn with_byte_offset_map<T>(offset_map: Option<Vec<usize>>, f: impl FnOnce() -> T) -> T {
    BYTE_OFFSET_MAP.with(|cell| {
        *cell.borrow_mut() = offset_map;
    });
    let result = f();
    BYTE_OFFSET_MAP.with(|cell| {
        *cell.borrow_mut() = None;
    });
    result
}

fn normalize_recoverable_source(source: &str) -> Option<String> {
    let without_lone_angle = remove_lone_trailing_angle_line(source);
    let with_anon_semis = insert_missing_anonymous_class_semicolons(&without_lone_angle);
    let normalized = insert_missing_call_semicolons_before_closing_brace(&with_anon_semis);
    (normalized != source).then_some(normalized)
}

fn remove_lone_trailing_angle_line(source: &str) -> String {
    let mut lines = source.lines().collect::<Vec<_>>();
    if lines.last().is_some_and(|line| line.trim() == ">") {
        lines.pop();
        let mut normalized = lines.join("\n");
        if source.ends_with('\n') {
            normalized.push('\n');
        }
        normalized
    } else {
        source.to_string()
    }
}

fn insert_missing_anonymous_class_semicolons(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    while let Some(relative_pos) = source[cursor..].find("new class") {
        let new_pos = cursor + relative_pos;
        let Some(open_relative) = source[new_pos..].find('{') else {
            break;
        };
        let open_pos = new_pos + open_relative;
        let mut depth = 0usize;
        let mut close_pos = None;
        for (idx, ch) in source[open_pos..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        close_pos = Some(open_pos + idx);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close_pos) = close_pos else {
            break;
        };
        output.push_str(&source[cursor..=close_pos]);
        let after_close = close_pos + 1;
        let rest = &source[after_close..];
        let next_non_ws = rest
            .char_indices()
            .find(|(_, ch)| !ch.is_whitespace())
            .map(|(_, ch)| ch);
        if !matches!(next_non_ws, Some(';') | Some(',') | Some(')')) {
            output.push(';');
        }
        cursor = after_close;
    }
    output.push_str(&source[cursor..]);
    output
}

fn insert_missing_call_semicolons_before_closing_brace(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    for (idx, ch) in source.char_indices() {
        if ch != ')' {
            continue;
        }
        let after_paren = idx + 1;
        let rest = &source[after_paren..];
        let next = rest.trim_start();
        if should_insert_missing_call_semicolon(source, idx, next)
            && !is_inside_braced_property_fetch(source, idx)
        {
            output.push_str(&source[cursor..after_paren]);
            output.push(';');
            cursor = after_paren;
        }
    }
    output.push_str(&source[cursor..]);
    output
}

fn should_insert_missing_call_semicolon(source: &str, close_paren_idx: usize, next: &str) -> bool {
    if next.starts_with('}') {
        return true;
    }
    if !next.starts_with("return") {
        return false;
    }
    if next
        .get("return".len()..)
        .and_then(|suffix| suffix.chars().next())
        .is_some_and(is_identifier_char)
    {
        return false;
    }
    let prefix = source.get(..close_paren_idx).unwrap_or(source);
    let statement_start = prefix
        .rfind([';', '{', '}'])
        .map(|idx| idx + 1)
        .unwrap_or(0);
    prefix
        .get(statement_start..)
        .is_some_and(|stmt| stmt.contains("->") || stmt.contains("::"))
}

fn is_inside_braced_property_fetch(source: &str, idx: usize) -> bool {
    source
        .get(..idx)
        .and_then(|prefix| prefix.rfind('{').map(|brace| &prefix[..brace]))
        .map(|before_brace| {
            let trimmed = before_brace.trim_end();
            trimmed.ends_with("->") || trimmed.ends_with("?->")
        })
        .unwrap_or(false)
}

fn lower_tree(tree: &Tree, source: &str) -> Value {
    let root = tree.root_node();
    if !source.contains("<?") && !source.trim().is_empty() {
        return Value::Array(vec![object(
            "Stmt_InlineHTML",
            root,
            source,
            [("value", Value::String(source.to_string()))],
        )]);
    }

    let mut children = Vec::new();
    let root_children = named_children(root)
        .into_iter()
        .filter(|child| child.kind() != "php_tag")
        .collect::<Vec<_>>();
    let mut idx = 0;
    while idx < root_children.len() {
        let child = root_children[idx];
        if child.kind() == "php_tag" {
            idx += 1;
            continue;
        }
        if child.kind() == "namespace_definition"
            && child.child_by_field_name("body").is_none()
            && idx + 1 < root_children.len()
        {
            let mut namespace_stmts = Vec::new();
            idx += 1;
            while idx < root_children.len() && root_children[idx].kind() != "namespace_definition" {
                if let Some(stmt) = lower_stmt(root_children[idx], source) {
                    namespace_stmts.push(stmt);
                }
                idx += 1;
            }
            children.push(namespace_stmt_with_stmts(
                child,
                source,
                Some(namespace_stmts),
            ));
            continue;
        }
        if let Some(stmt) = lower_stmt(child, source) {
            children.push(stmt);
        }
        idx += 1;
    }
    if children.is_empty() {
        children = recover_line_assignments(source);
    }
    if source.contains("function ")
        && source.contains("global ")
        && !has_stmt_type(&children, "Stmt_Function")
        && !has_stmt_type(&children, "Stmt_ClassMethod")
    {
        children.extend(recover_global_functions(source));
    }
    Value::Array(children)
}

fn lower_stmt(node: Node, source: &str) -> Option<Value> {
    match node.kind() {
        "namespace_definition" => Some(namespace_stmt(node, source)),
        "namespace_use_declaration" => Some(namespace_use_stmt(node, source)),
        "function_definition" => Some(function_stmt(node, source)),
        "method_declaration" => Some(class_method_stmt(node, source)),
        "class_declaration" => Some(class_like_stmt(node, source, "Stmt_Class")),
        "interface_declaration" => Some(class_like_stmt(node, source, "Stmt_Interface")),
        "trait_declaration" => Some(class_like_stmt(node, source, "Stmt_Trait")),
        "enum_declaration" => Some(class_like_stmt(node, source, "Stmt_Enum")),
        "enum_case" => Some(enum_case_stmt(node, source)),
        "use_declaration" => Some(trait_use_stmt(node, source)),
        "property_declaration" => Some(property_stmt(node, source)),
        "const_declaration" => Some(const_stmt(node, source, const_node_type(node))),
        "function_static_declaration" => Some(static_stmt(node, source)),
        "unset_statement" => Some(unset_stmt(node, source)),
        "global_declaration" => Some(global_stmt(node, source)),
        "declare_statement" => Some(declare_stmt(node, source)),
        "exit_statement" => Some(exit_stmt(node, source)),
        "goto_statement" => Some(goto_stmt(node, source)),
        "named_label_statement" => Some(label_stmt(node, source)),
        "echo_statement" => Some(echo_stmt(node, source)),
        "return_statement" => Some(return_stmt(node, source)),
        "expression_statement" if is_halt_compiler_stmt(node, source) => {
            Some(object("Stmt_HaltCompiler", node, source, []))
        }
        "expression_statement" => named_children(node)
            .into_iter()
            .find_map(|child| lower_expr(child, source))
            .map(|expr| object("Stmt_Expression", node, source, [("expr", expr)])),
        "compound_statement" => Some(stmts_as_block(node, source)),
        "if_statement" => Some(if_stmt(node, source)),
        "switch_statement" => Some(switch_stmt(node, source)),
        "while_statement" => Some(while_stmt(node, source)),
        "do_statement" => Some(do_stmt(node, source)),
        "for_statement" => Some(for_stmt(node, source)),
        "foreach_statement" => Some(foreach_stmt(node, source)),
        "try_statement" => Some(try_stmt(node, source)),
        "break_statement" => Some(object(
            "Stmt_Break",
            node,
            source,
            [("num", jump_depth(node, source))],
        )),
        "continue_statement" => Some(object(
            "Stmt_Continue",
            node,
            source,
            [("num", jump_depth(node, source))],
        )),
        "comment" => Some(object("Stmt_Nop", node, source, [])),
        _ => lower_expr(node, source)
            .map(|expr| object("Stmt_Expression", node, source, [("expr", expr)])),
    }
}

fn lower_expr(node: Node, source: &str) -> Option<Value> {
    match node.kind() {
        "variable_name" | "dynamic_variable_name" => Some(variable_expr(node, source)),
        "name" | "qualified_name" | "fully_qualified_name" => Some(
            magic_const_node_type(&text(node, source))
                .map(|node_type| object(node_type, node, source, []))
                .unwrap_or_else(|| name_expr(node, source, "Expr_ConstFetch")),
        ),
        "integer" => Some(object(
            "Scalar_LNumber",
            node,
            source,
            [("value", number_or_string(text(node, source)))],
        )),
        "float" => Some(object(
            "Scalar_DNumber",
            node,
            source,
            [("value", number_or_string(text(node, source)))],
        )),
        "string" | "string_value" | "nowdoc" => Some(string_expr(node, source)),
        "heredoc" => Some(heredoc_expr(node, source)),
        "encapsed_string" => Some(encapsed_expr(node, source)),
        "boolean" => Some(const_fetch(node, source, text(node, source))),
        "null" => Some(const_fetch(node, source, "NULL".to_string())),
        "assignment_expression" | "augmented_assignment_expression" => {
            Some(assign_expr(node, source))
        }
        "binary_expression" => Some(binary_expr(node, source)),
        "unary_op_expression" | "update_expression" | "reference_modifier" => {
            Some(unary_expr(node, source))
        }
        "parenthesized_expression" => named_children(node)
            .into_iter()
            .find_map(|child| lower_expr(child, source)),
        "function_call_expression" => Some(function_call_expr(node, source)),
        "member_call_expression" => Some(call_expr(node, source, "Expr_MethodCall")),
        "nullsafe_member_call_expression" => {
            Some(call_expr(node, source, "Expr_NullsafeMethodCall"))
        }
        "scoped_call_expression" => Some(call_expr(node, source, "Expr_StaticCall")),
        "class_constant_access_expression" => Some(class_const_fetch_expr(node, source)),
        "member_access_expression" => Some(property_fetch_expr(node, source, false, false)),
        "nullsafe_member_access_expression" => Some(property_fetch_expr(node, source, false, true)),
        "scoped_property_access_expression" => Some(property_fetch_expr(node, source, true, false)),
        "subscript_expression" => Some(array_dim_fetch_expr(node, source)),
        "array_creation_expression" => Some(array_expr(node, source)),
        "list_literal" => Some(list_expr(node, source)),
        "object_creation_expression" => Some(new_expr(node, source)),
        "conditional_expression" => Some(ternary_expr(node, source)),
        "match_expression" => Some(match_expr(node, source)),
        "cast_expression" => Some(cast_expr(node, source)),
        "throw_expression" => Some(throw_expr(node, source)),
        "clone_expression" => named_children(node).first().and_then(|expr| {
            lower_expr(*expr, source)
                .map(|expr| object("Expr_Clone", node, source, [("expr", expr)]))
        }),
        "error_suppression_expression" => named_children(node).first().and_then(|expr| {
            lower_expr(*expr, source)
                .map(|expr| object("Expr_ErrorSuppress", node, source, [("expr", expr)]))
        }),
        "shell_command_expression" => Some(shell_exec_expr(node, source)),
        "print_intrinsic" => named_children(node).first().and_then(|expr| {
            lower_expr(*expr, source)
                .map(|expr| object("Expr_Print", node, source, [("expr", expr)]))
        }),
        "exit_intrinsic" => Some(object(
            "Expr_Exit",
            node,
            source,
            [(
                "expr",
                named_children(node)
                    .first()
                    .and_then(|expr| lower_expr(*expr, source))
                    .unwrap_or(Value::Null),
            )],
        )),
        "include_expression"
        | "include_once_expression"
        | "require_expression"
        | "require_once_expression" => Some(include_expr(node, source)),
        "anonymous_function" => Some(closure_expr(node, source, false)),
        "arrow_function" => Some(closure_expr(node, source, true)),
        "yield_expression" => Some(yield_expr(node, source)),
        kind => {
            record_unmapped_kind(kind);
            None
        }
    }
}

fn namespace_stmt(node: Node, source: &str) -> Value {
    namespace_stmt_with_stmts(node, source, None)
}

fn namespace_stmt_with_stmts(
    node: Node,
    source: &str,
    stmts_override: Option<Vec<Value>>,
) -> Value {
    let name = node
        .child_by_field_name("name")
        .or_else(|| {
            child_by_kind(
                node,
                &[
                    "namespace_name",
                    "name",
                    "qualified_name",
                    "fully_qualified_name",
                ],
            )
        })
        .map(|name| name_node(name, source))
        .unwrap_or(Value::Null);
    let stmts = stmts_override.unwrap_or_else(|| {
        child_by_kind(node, &["compound_statement"])
            .map(|body| statements_in_body(body, source))
            .unwrap_or_else(|| {
                let name_node = node.child_by_field_name("name");
                let body_node = node.child_by_field_name("body");
                named_children(node)
                    .into_iter()
                    .filter(|child| Some(*child) != name_node && Some(*child) != body_node)
                    .filter_map(|child| lower_stmt(child, source))
                    .collect()
            })
    });
    object(
        "Stmt_Namespace",
        node,
        source,
        [("name", name), ("stmts", Value::Array(stmts))],
    )
}

fn namespace_use_stmt(node: Node, source: &str) -> Value {
    if child_by_kind(node, &["namespace_use_group"]).is_some() {
        group_use_stmt(node, source)
    } else {
        use_stmt(node, source)
    }
}

fn use_stmt(node: Node, source: &str) -> Value {
    let use_type = use_type_num(node);
    let uses = named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "namespace_use_clause")
        .filter_map(|child| use_use(child, source, use_type))
        .collect::<Vec<_>>();
    object(
        "Stmt_Use",
        node,
        source,
        [("uses", Value::Array(uses)), ("type", json!(use_type))],
    )
}

fn group_use_stmt(node: Node, source: &str) -> Value {
    let use_type = use_type_num(node);
    let prefix = child_by_kind(node, &["namespace_name"])
        .map(|prefix| name_node(prefix, source))
        .unwrap_or_else(|| name_from_text("", node, source));
    let uses = child_by_kind(node, &["namespace_use_group"])
        .map(|group| {
            named_children(node)
                .into_iter()
                .chain(named_children(group))
                .filter(|child| child.kind() == "namespace_use_clause")
                .filter_map(|child| use_use(child, source, use_type))
                .collect()
        })
        .unwrap_or_default();
    object(
        "Stmt_GroupUse",
        node,
        source,
        [
            ("prefix", prefix),
            ("uses", Value::Array(uses)),
            ("type", json!(use_type)),
        ],
    )
}

fn use_use(node: Node, source: &str, parent_type: i32) -> Option<Value> {
    let alias = node
        .child_by_field_name("alias")
        .map(|alias| name_node(alias, source))
        .unwrap_or(Value::Null);
    let name = named_children(node)
        .into_iter()
        .find(|child| {
            Some(*child) != node.child_by_field_name("alias")
                && matches!(
                    child.kind(),
                    "name" | "qualified_name" | "fully_qualified_name"
                )
        })
        .map(|name| name_node(name, source))?;
    Some(object(
        "Stmt_UseUse",
        node,
        source,
        [
            ("name", name),
            ("alias", alias),
            ("type", json!(use_type_num(node).max(parent_type))),
        ],
    ))
}

fn use_type_num(node: Node) -> i32 {
    node.child_by_field_name("type")
        .map(|typ| match typ.kind() {
            "function" => 2,
            "const" => 3,
            _ => 1,
        })
        .unwrap_or(0)
}

fn function_stmt(node: Node, source: &str) -> Value {
    let name = child_by_kind(node, &["name"])
        .map(|n| identifier_node(n, source))
        .unwrap_or_else(|| identifier_from_text("<anonymous>", node, source));
    let params = child_by_kind(node, &["formal_parameters"])
        .map(|p| params(p, source))
        .unwrap_or_default();
    let return_type = child_by_field_or_kind(
        node,
        "return_type",
        &["primitive_type", "named_type", "optional_type"],
    )
    .map(|typ| type_node(typ, source))
    .unwrap_or(Value::Null);
    let stmts = child_by_kind(node, &["compound_statement"])
        .map(|body| statements_in_body(body, source))
        .unwrap_or_default();
    object(
        "Stmt_Function",
        node,
        source,
        [
            ("byRef", json!(false)),
            ("name", name),
            ("params", Value::Array(params)),
            ("returnType", return_type),
            ("stmts", Value::Array(stmts)),
            ("namespacedName", Value::Null),
            ("attrGroups", Value::Array(attribute_groups(node, source))),
        ],
    )
}

fn class_method_stmt(node: Node, source: &str) -> Value {
    let name = child_by_kind(node, &["name"])
        .map(|n| identifier_node(n, source))
        .unwrap_or_else(|| identifier_from_text("<anonymous>", node, source));
    let params = child_by_kind(node, &["formal_parameters"])
        .map(|p| params(p, source))
        .unwrap_or_default();
    let return_type = child_by_field_or_kind(
        node,
        "return_type",
        &["primitive_type", "named_type", "optional_type"],
    )
    .map(|typ| type_node(typ, source))
    .unwrap_or(Value::Null);
    let stmts = child_by_kind(node, &["compound_statement"])
        .map(|body| statements_in_body(body, source))
        .map(Value::Array)
        .unwrap_or(Value::Null);
    object(
        "Stmt_ClassMethod",
        node,
        source,
        [
            ("flags", json!(modifier_flags(node, source))),
            ("byRef", json!(false)),
            ("name", name),
            ("params", Value::Array(params)),
            ("returnType", return_type),
            ("stmts", stmts),
            ("attrGroups", Value::Array(attribute_groups(node, source))),
        ],
    )
}

fn class_like_stmt(node: Node, source: &str, node_type: &str) -> Value {
    class_like_stmt_with_name(node, source, node_type, None)
}

fn class_like_stmt_with_name(
    node: Node,
    source: &str,
    node_type: &str,
    name_override: Option<Value>,
) -> Value {
    let name = name_override.unwrap_or_else(|| {
        child_by_kind(node, &["name"])
            .map(|n| identifier_node(n, source))
            .unwrap_or(Value::Null)
    });
    let stmts = child_by_kind(node, &["declaration_list", "enum_declaration_list"])
        .map(|body| statements_in_body(body, source))
        .unwrap_or_default();
    let extends = class_like_extends(node, source);
    let implements = child_by_kind(node, &["class_interface_clause"])
        .map(|clause| names_in_node(clause, source))
        .unwrap_or_default();
    let scalar_type = if node_type == "Stmt_Enum" {
        child_by_kind(
            node,
            &[
                "primitive_type",
                "named_type",
                "qualified_name",
                "fully_qualified_name",
            ],
        )
        .map(|typ| name_node(typ, source))
        .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let mut fields = vec![
        ("flags", json!(modifier_flags(node, source))),
        ("name", name),
        ("extends", extends),
        ("implements", Value::Array(implements)),
        ("stmts", Value::Array(stmts)),
        ("attrGroups", Value::Array(attribute_groups(node, source))),
    ];
    if node_type == "Stmt_Enum" {
        fields.push(("scalarType", scalar_type));
    }
    object(node_type, node, source, fields)
}

fn enum_case_stmt(node: Node, source: &str) -> Value {
    let name = node
        .child_by_field_name("name")
        .or_else(|| child_by_kind(node, &["name"]))
        .map(|n| identifier_node(n, source))
        .unwrap_or(Value::Null);
    let expr = node
        .child_by_field_name("value")
        .and_then(|value| lower_expr(value, source))
        .unwrap_or(Value::Null);
    object(
        "Stmt_EnumCase",
        node,
        source,
        [("name", name), ("expr", expr)],
    )
}

fn trait_use_stmt(node: Node, source: &str) -> Value {
    let adaptations = child_by_kind(node, &["use_list"])
        .map(|use_list| {
            named_children(use_list)
                .into_iter()
                .filter_map(|clause| match clause.kind() {
                    "use_instead_of_clause" => Some(trait_precedence_adaptation(clause, source)),
                    "use_as_clause" => Some(trait_alias_adaptation(clause, source)),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    object(
        "Stmt_TraitUse",
        node,
        source,
        [
            ("traits", Value::Array(names_in_node(node, source))),
            ("adaptations", Value::Array(adaptations)),
        ],
    )
}

fn trait_precedence_adaptation(node: Node, source: &str) -> Value {
    let children = named_children(node);
    let (trait_name, method) = children
        .first()
        .filter(|n| n.kind() == "class_constant_access_expression")
        .map(|n| trait_method_pair(*n, source))
        .unwrap_or((Value::Null, Value::Null));
    let insteadof = children
        .iter()
        .skip(1)
        .filter(|n| {
            matches!(
                n.kind(),
                "name" | "qualified_name" | "fully_qualified_name" | "relative_name"
            )
        })
        .map(|n| name_node(*n, source))
        .collect::<Vec<_>>();
    object(
        "Stmt_TraitUseAdaptation_Precedence",
        node,
        source,
        [
            ("trait", trait_name),
            ("method", method),
            ("insteadof", Value::Array(insteadof)),
        ],
    )
}

fn trait_alias_adaptation(node: Node, source: &str) -> Value {
    let children = named_children(node);
    let (trait_name, method) = match children.first() {
        Some(first) if first.kind() == "class_constant_access_expression" => {
            trait_method_pair(*first, source)
        }
        Some(first) if matches!(first.kind(), "name" | "qualified_name") => {
            (Value::Null, name_node(*first, source))
        }
        _ => (Value::Null, Value::Null),
    };
    let new_modifier = children
        .iter()
        .find(|n| n.kind() == "visibility_modifier")
        .map(|n| {
            json!(modifier_flags_from_code(
                &text(*n, source).to_ascii_lowercase()
            ))
        })
        .unwrap_or(Value::Null);
    let new_name = children
        .iter()
        .skip(1)
        .find(|n| matches!(n.kind(), "name" | "qualified_name" | "fully_qualified_name"))
        .map(|n| name_node(*n, source))
        .unwrap_or(Value::Null);
    object(
        "Stmt_TraitUseAdaptation_Alias",
        node,
        source,
        [
            ("trait", trait_name),
            ("method", method),
            ("newModifier", new_modifier),
            ("newName", new_name),
        ],
    )
}

fn trait_method_pair(node: Node, source: &str) -> (Value, Value) {
    let children = named_children(node);
    let trait_name = children
        .first()
        .map(|n| name_node(*n, source))
        .unwrap_or(Value::Null);
    let method = children
        .get(1)
        .map(|n| name_or_identifier(*n, source))
        .unwrap_or(Value::Null);
    (trait_name, method)
}

fn attribute_groups(node: Node, source: &str) -> Vec<Value> {
    node.child_by_field_name("attributes")
        .map(|attribute_list| {
            named_children(attribute_list)
                .into_iter()
                .filter(|child| child.kind() == "attribute_group")
                .map(|group| attribute_group(group, source))
                .collect()
        })
        .unwrap_or_default()
}

fn attribute_group(node: Node, source: &str) -> Value {
    let attrs = named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "attribute")
        .map(|attribute| attribute_node(attribute, source))
        .collect::<Vec<_>>();
    object(
        "AttributeGroup",
        node,
        source,
        [("attrs", Value::Array(attrs))],
    )
}

fn attribute_node(node: Node, source: &str) -> Value {
    let name = child_by_kind(node, &["name", "qualified_name", "fully_qualified_name"])
        .map(|name| name_node(name, source))
        .unwrap_or_else(|| name_from_text("UNKNOWN", node, source));
    let args = child_by_kind(node, &["arguments"])
        .map(|args| call_args(args, source))
        .unwrap_or_default();
    object(
        "Attribute",
        node,
        source,
        [("name", name), ("args", Value::Array(args))],
    )
}

fn class_like_extends(node: Node, source: &str) -> Value {
    let Some(base) = child_by_kind(node, &["base_clause"]) else {
        return Value::Null;
    };
    let names = names_in_node(base, source);
    match names.as_slice() {
        [] => Value::Null,
        [single] => single.clone(),
        _ => Value::Array(names),
    }
}

fn names_in_node(node: Node, source: &str) -> Vec<Value> {
    named_children(node)
        .into_iter()
        .filter(|child| {
            matches!(
                child.kind(),
                "name" | "qualified_name" | "fully_qualified_name" | "relative_name" | "named_type"
            )
        })
        .map(|child| name_node(child, source))
        .collect()
}

fn property_stmt(node: Node, source: &str) -> Value {
    let props = named_children(node)
        .into_iter()
        .filter(|child| matches!(child.kind(), "property_element" | "variable_name"))
        .map(|child| {
            let var = child_by_kind(child, &["variable_name"]).unwrap_or(child);
            let default = child
                .child_by_field_name("default_value")
                .and_then(|value| lower_expr(value, source))
                .unwrap_or(Value::Null);
            object(
                "Stmt_PropertyProperty",
                child,
                source,
                [
                    ("name", varlike_identifier(var, source)),
                    ("default", default),
                ],
            )
        })
        .collect::<Vec<_>>();
    let typ = node
        .child_by_field_name("type")
        .map(|typ| type_node(typ, source))
        .unwrap_or(Value::Null);
    object(
        "Stmt_Property",
        node,
        source,
        [
            ("flags", json!(modifier_flags(node, source))),
            ("props", Value::Array(props)),
            ("type", typ),
        ],
    )
}

fn is_halt_compiler_stmt(node: Node, source: &str) -> bool {
    named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "function_call_expression")
        .filter_map(|call| {
            call.child_by_field_name("function").or_else(|| {
                child_by_kind(call, &["name", "qualified_name", "fully_qualified_name"])
            })
        })
        .any(|name| text(name, source).eq_ignore_ascii_case("__halt_compiler"))
}

fn magic_const_node_type(text: &str) -> Option<&'static str> {
    match text.trim() {
        "__LINE__" => Some("Scalar_MagicConst_Line"),
        "__FILE__" => Some("Scalar_MagicConst_File"),
        "__DIR__" => Some("Scalar_MagicConst_Dir"),
        "__FUNCTION__" => Some("Scalar_MagicConst_Function"),
        "__CLASS__" => Some("Scalar_MagicConst_Class"),
        "__METHOD__" => Some("Scalar_MagicConst_Method"),
        "__NAMESPACE__" => Some("Scalar_MagicConst_Namespace"),
        "__TRAIT__" => Some("Scalar_MagicConst_Trait"),
        _ => None,
    }
}

fn const_node_type(node: Node) -> &'static str {
    let class_like = ancestor_by_kind(
        node,
        &[
            "class_declaration",
            "interface_declaration",
            "trait_declaration",
            "enum_declaration",
        ],
    );
    if class_like.is_some() {
        "Stmt_ClassConst"
    } else {
        "Stmt_Const"
    }
}

fn const_stmt(node: Node, source: &str, node_type: &str) -> Value {
    let consts = named_children(node)
        .into_iter()
        .filter(|child| matches!(child.kind(), "const_element" | "name"))
        .map(|child| {
            let name = child_by_kind(child, &["name"]).unwrap_or(child);
            let value = named_children(child)
                .into_iter()
                .find(|n| n.kind() != "name")
                .and_then(|n| lower_expr(n, source))
                .unwrap_or_else(|| const_fetch(name, source, "NULL".to_string()));
            object(
                "Const",
                child,
                source,
                [
                    ("name", identifier_node(name, source)),
                    ("value", value),
                    ("namespacedName", Value::Null),
                ],
            )
        })
        .collect::<Vec<_>>();
    object(
        node_type,
        node,
        source,
        [
            ("flags", json!(modifier_flags(node, source))),
            ("consts", Value::Array(consts)),
        ],
    )
}

fn echo_stmt(node: Node, source: &str) -> Value {
    let exprs = named_children(node)
        .into_iter()
        .flat_map(|child| expr_list(child, source))
        .collect::<Vec<_>>();
    object("Stmt_Echo", node, source, [("exprs", Value::Array(exprs))])
}

fn return_stmt(node: Node, source: &str) -> Value {
    let expr = named_children(node)
        .into_iter()
        .find_map(|child| lower_expr(child, source))
        .unwrap_or(Value::Null);
    object("Stmt_Return", node, source, [("expr", expr)])
}

fn if_stmt(node: Node, source: &str) -> Value {
    let cond = child_by_field_or_kind(node, "condition", &["parenthesized_expression"])
        .and_then(|n| lower_expr(n, source))
        .unwrap_or_else(|| const_fetch(node, source, "true".to_string()));
    let stmts = node
        .child_by_field_name("body")
        .map(|body| body_statements(body, source))
        .or_else(|| {
            child_by_kind(node, &["compound_statement"]).map(|body| body_statements(body, source))
        })
        .unwrap_or_default();
    let elseifs = named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "else_if_clause")
        .map(|child| {
            let cond = child
                .child_by_field_name("condition")
                .and_then(|cond| lower_expr(cond, source))
                .unwrap_or_else(|| const_fetch(child, source, "true".to_string()));
            let stmts = child
                .child_by_field_name("body")
                .map(|body| body_statements(body, source))
                .unwrap_or_default();
            object(
                "Stmt_ElseIf",
                child,
                source,
                [("cond", cond), ("stmts", Value::Array(stmts))],
            )
        })
        .collect::<Vec<_>>();
    let else_stmt = named_children(node)
        .into_iter()
        .find(|child| child.kind() == "else_clause")
        .map(|child| {
            let stmts = child
                .child_by_field_name("body")
                .map(|body| body_statements(body, source))
                .unwrap_or_default();
            object("Stmt_Else", child, source, [("stmts", Value::Array(stmts))])
        })
        .unwrap_or(Value::Null);
    object(
        "Stmt_If",
        node,
        source,
        [
            ("cond", cond),
            ("stmts", Value::Array(stmts)),
            ("elseifs", Value::Array(elseifs)),
            ("else", else_stmt),
        ],
    )
}

fn switch_stmt(node: Node, source: &str) -> Value {
    let cond = node
        .child_by_field_name("condition")
        .and_then(|n| lower_expr(n, source))
        .unwrap_or_else(|| const_fetch(node, source, "true".to_string()));
    let cases = node
        .child_by_field_name("body")
        .map(|body| {
            named_children(body)
                .into_iter()
                .filter_map(|child| match child.kind() {
                    "case_statement" | "default_statement" => Some(case_stmt(child, source)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    object(
        "Stmt_Switch",
        node,
        source,
        [("cond", cond), ("cases", Value::Array(cases))],
    )
}

fn case_stmt(node: Node, source: &str) -> Value {
    let value_node = node.child_by_field_name("value");
    let cond = value_node
        .and_then(|value| lower_expr(value, source))
        .unwrap_or(Value::Null);
    let stmts = named_children(node)
        .into_iter()
        .filter(|child| Some(*child) != value_node)
        .filter_map(|child| lower_stmt(child, source))
        .collect::<Vec<_>>();
    object(
        "Stmt_Case",
        node,
        source,
        [("cond", cond), ("stmts", Value::Array(stmts))],
    )
}

fn while_stmt(node: Node, source: &str) -> Value {
    let cond = child_by_field_or_kind(node, "condition", &["parenthesized_expression"])
        .and_then(|n| lower_expr(n, source))
        .unwrap_or_else(|| const_fetch(node, source, "true".to_string()));
    let stmts = child_by_kind(node, &["compound_statement"])
        .map(|body| statements_in_body(body, source))
        .unwrap_or_default();
    object(
        "Stmt_While",
        node,
        source,
        [("cond", cond), ("stmts", Value::Array(stmts))],
    )
}

fn do_stmt(node: Node, source: &str) -> Value {
    let cond = child_by_field_or_kind(node, "condition", &["parenthesized_expression"])
        .and_then(|n| lower_expr(n, source))
        .unwrap_or_else(|| const_fetch(node, source, "true".to_string()));
    let stmts = child_by_kind(node, &["compound_statement"])
        .map(|body| statements_in_body(body, source))
        .unwrap_or_default();
    object(
        "Stmt_Do",
        node,
        source,
        [("cond", cond), ("stmts", Value::Array(stmts))],
    )
}

fn for_stmt(node: Node, source: &str) -> Value {
    let init = node
        .child_by_field_name("initialize")
        .map(|expr| expr_list(expr, source))
        .unwrap_or_default();
    let cond = node
        .child_by_field_name("condition")
        .map(|expr| expr_list(expr, source))
        .unwrap_or_default();
    let loop_exprs = node
        .child_by_field_name("update")
        .map(|expr| expr_list(expr, source))
        .unwrap_or_default();
    let stmts = node
        .child_by_field_name("body")
        .map(|body| body_statements(body, source))
        .or_else(|| {
            child_by_kind(node, &["compound_statement"])
                .map(|body| statements_in_body(body, source))
        })
        .unwrap_or_default();
    object(
        "Stmt_For",
        node,
        source,
        [
            ("init", Value::Array(init)),
            ("cond", Value::Array(cond)),
            ("loop", Value::Array(loop_exprs)),
            ("stmts", Value::Array(stmts)),
        ],
    )
}

fn expr_list(node: Node, source: &str) -> Vec<Value> {
    if node.kind() == "sequence_expression" {
        named_children(node)
            .into_iter()
            .flat_map(|child| expr_list(child, source))
            .collect()
    } else {
        lower_expr(node, source).into_iter().collect()
    }
}

fn foreach_stmt(node: Node, source: &str) -> Value {
    let children = named_children(node);
    let exprs = children
        .iter()
        .filter(|child| !matches!(child.kind(), "by_ref" | "pair" | "compound_statement"))
        .filter_map(|child| lower_expr(*child, source))
        .collect::<Vec<_>>();
    let pair = children
        .iter()
        .find(|child| child.kind() == "pair")
        .and_then(|pair| pair_key_value(*pair, source));
    let iter = exprs
        .first()
        .cloned()
        .unwrap_or_else(|| variable_from_name("iter", node, source));
    let value = pair
        .as_ref()
        .map(|(_, value)| value.clone())
        .or_else(|| {
            children
                .iter()
                .find(|child| child.kind() == "by_ref")
                .and_then(|by_ref| {
                    named_children(*by_ref)
                        .into_iter()
                        .find_map(|child| lower_expr(child, source))
                })
        })
        .or_else(|| exprs.last().cloned())
        .unwrap_or_else(|| variable_from_name("value", node, source));
    let key_var = pair
        .as_ref()
        .map(|(key, _)| key.clone())
        .unwrap_or(Value::Null);
    let assign_by_ref = children.iter().any(|child| {
        child.kind() == "by_ref"
            || (child.kind() == "pair"
                && named_children(*child)
                    .iter()
                    .any(|pair_child| pair_child.kind() == "by_ref"))
    });
    let stmts = child_by_kind(node, &["compound_statement"])
        .map(|body| statements_in_body(body, source))
        .unwrap_or_default();
    object(
        "Stmt_Foreach",
        node,
        source,
        [
            ("expr", iter),
            ("keyVar", key_var),
            ("valueVar", value),
            ("byRef", json!(assign_by_ref)),
            ("stmts", Value::Array(stmts)),
        ],
    )
}

fn try_stmt(node: Node, source: &str) -> Value {
    let stmts = node
        .child_by_field_name("body")
        .map(|body| statements_in_body(body, source))
        .unwrap_or_default();
    let children = named_children(node);
    let catches = children
        .iter()
        .filter(|child| child.kind() == "catch_clause")
        .map(|catch| catch_stmt(*catch, source))
        .collect::<Vec<_>>();
    let finally_stmt = children
        .iter()
        .find(|child| child.kind() == "finally_clause")
        .map(|finally| finally_stmt(*finally, source))
        .unwrap_or(Value::Null);
    object(
        "Stmt_TryCatch",
        node,
        source,
        [
            ("stmts", Value::Array(stmts)),
            ("catches", Value::Array(catches)),
            ("finally", finally_stmt),
        ],
    )
}

fn catch_stmt(node: Node, source: &str) -> Value {
    let types = node
        .child_by_field_name("type")
        .map(|typ| type_names(typ, source))
        .unwrap_or_default();
    let var = node
        .child_by_field_name("name")
        .map(|name| variable_expr(name, source))
        .unwrap_or(Value::Null);
    let stmts = node
        .child_by_field_name("body")
        .map(|body| statements_in_body(body, source))
        .unwrap_or_default();
    object(
        "Stmt_Catch",
        node,
        source,
        [
            ("types", Value::Array(types)),
            ("var", var),
            ("stmts", Value::Array(stmts)),
        ],
    )
}

fn finally_stmt(node: Node, source: &str) -> Value {
    let stmts = node
        .child_by_field_name("body")
        .map(|body| statements_in_body(body, source))
        .unwrap_or_default();
    object(
        "Stmt_Finally",
        node,
        source,
        [("stmts", Value::Array(stmts))],
    )
}

fn type_names(node: Node, source: &str) -> Vec<Value> {
    match node.kind() {
        "name" | "qualified_name" | "fully_qualified_name" | "named_type" | "primitive_type" => {
            vec![name_node(node, source)]
        }
        _ => named_children(node)
            .into_iter()
            .flat_map(|child| type_names(child, source))
            .collect(),
    }
}

fn pair_key_value(node: Node, source: &str) -> Option<(Value, Value)> {
    let values = named_children(node)
        .into_iter()
        .filter_map(|child| lower_expr_or_wrapper(child, source))
        .collect::<Vec<_>>();
    (values.len() >= 2).then(|| (values[0].clone(), values[values.len() - 1].clone()))
}

fn assign_expr(node: Node, source: &str) -> Value {
    let children = named_children(node);
    let var = children
        .first()
        .and_then(|n| lower_expr(*n, source))
        .unwrap_or_else(|| variable_from_name("unknown", node, source));
    let expr = children
        .last()
        .and_then(|n| lower_expr(*n, source))
        .unwrap_or_else(|| const_fetch(node, source, "NULL".to_string()));
    let node_type = assign_node_type(&text(node, source));
    object(node_type, node, source, [("var", var), ("expr", expr)])
}

fn binary_expr(node: Node, source: &str) -> Value {
    let children = named_children(node);
    let left = children
        .first()
        .and_then(|n| lower_expr(*n, source))
        .unwrap_or_else(|| const_fetch(node, source, "NULL".to_string()));
    let right = children
        .last()
        .and_then(|n| lower_expr(*n, source))
        .unwrap_or_else(|| const_fetch(node, source, "NULL".to_string()));
    if text(node, source).contains("instanceof") {
        let class = children
            .last()
            .map(|n| {
                if matches!(
                    n.kind(),
                    "name" | "qualified_name" | "fully_qualified_name" | "relative_name"
                ) {
                    name_node(*n, source)
                } else {
                    right.clone()
                }
            })
            .unwrap_or_else(|| right.clone());
        return object(
            "Expr_Instanceof",
            node,
            source,
            [("expr", left), ("class", class)],
        );
    }
    let node_type = binary_node_type(&text(node, source));
    object(node_type, node, source, [("left", left), ("right", right)])
}

fn unary_expr(node: Node, source: &str) -> Value {
    let expr = named_children(node)
        .last()
        .and_then(|n| lower_expr(*n, source))
        .unwrap_or_else(|| const_fetch(node, source, "NULL".to_string()));
    let node_type = unary_node_type(&text(node, source));
    object(node_type, node, source, [("expr", expr)])
}

fn function_call_expr(node: Node, source: &str) -> Value {
    let name = node
        .child_by_field_name("function")
        .or_else(|| child_by_kind(node, &["name", "qualified_name", "fully_qualified_name"]))
        .map(|name| text(name, source).to_ascii_lowercase())
        .unwrap_or_default();
    match name.as_str() {
        "isset" => object(
            "Expr_Isset",
            node,
            source,
            [(
                "vars",
                Value::Array(
                    child_by_kind(node, &["arguments"])
                        .map(|args| {
                            call_args(args, source)
                                .into_iter()
                                .filter_map(|arg| arg.get("value").cloned())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                ),
            )],
        ),
        "empty" => object(
            "Expr_Empty",
            node,
            source,
            [(
                "expr",
                first_call_arg_value(node, source)
                    .unwrap_or_else(|| const_fetch(node, source, "NULL".to_string())),
            )],
        ),
        "eval" => object(
            "Expr_Eval",
            node,
            source,
            [(
                "expr",
                first_call_arg_value(node, source)
                    .unwrap_or_else(|| const_fetch(node, source, "NULL".to_string())),
            )],
        ),
        "exit" | "die" => object(
            "Expr_Exit",
            node,
            source,
            [(
                "expr",
                first_call_arg_value(node, source).unwrap_or(Value::Null),
            )],
        ),
        _ => call_expr(node, source, "Expr_FuncCall"),
    }
}

fn first_call_arg_value(node: Node, source: &str) -> Option<Value> {
    child_by_kind(node, &["arguments"])
        .and_then(|args| call_args(args, source).into_iter().next())
        .and_then(|arg| arg.get("value").cloned())
}

fn call_expr(node: Node, source: &str, fallback_type: &str) -> Value {
    let children = named_children(node);
    let args = child_by_kind(node, &["arguments"])
        .map(|args| call_args(args, source))
        .unwrap_or_default();
    let name = if fallback_type == "Expr_FuncCall" {
        children
            .iter()
            .find(|n| matches!(n.kind(), "variable_name" | "dynamic_variable_name"))
            .map(|n| variable_expr(*n, source))
            .or_else(|| {
                children
                    .iter()
                    .rev()
                    .find(|n| {
                        matches!(
                            n.kind(),
                            "name" | "qualified_name" | "fully_qualified_name" | "member_name"
                        )
                    })
                    .map(|n| name_or_identifier(*n, source))
            })
            .unwrap_or_else(|| identifier_from_text(text(node, source), node, source))
    } else {
        node.child_by_field_name("name")
            .map(|n| call_name_node(n, source))
            .or_else(|| {
                children
                    .iter()
                    .rev()
                    .find(|n| {
                        matches!(
                            n.kind(),
                            "name" | "qualified_name" | "fully_qualified_name" | "member_name"
                        )
                    })
                    .map(|n| name_or_identifier(*n, source))
            })
            .unwrap_or_else(|| identifier_from_text(text(node, source), node, source))
    };
    let mut fields = vec![("name", name), ("args", Value::Array(args))];
    if fallback_type != "Expr_FuncCall" {
        let target_node = if fallback_type == "Expr_StaticCall" {
            node.child_by_field_name("scope")
        } else {
            children.first().copied()
        };
        let target = target_node
            .map(|n| {
                if fallback_type == "Expr_StaticCall"
                    && matches!(
                        n.kind(),
                        "name"
                            | "qualified_name"
                            | "fully_qualified_name"
                            | "relative_name"
                            | "relative_scope"
                    )
                {
                    name_node(n, source)
                } else {
                    lower_expr(n, source)
                        .unwrap_or_else(|| variable_from_name("this", node, source))
                }
            })
            .unwrap_or_else(|| variable_from_name("this", node, source));
        fields.push(if fallback_type == "Expr_StaticCall" {
            ("class", target)
        } else {
            ("var", target)
        });
    }
    object(fallback_type, node, source, fields)
}

fn call_name_node(node: Node, source: &str) -> Value {
    match node.kind() {
        "variable_name" | "dynamic_variable_name" => variable_expr(node, source),
        "name" | "qualified_name" | "fully_qualified_name" | "relative_name" => {
            name_node(node, source)
        }
        _ => lower_expr(node, source).unwrap_or_else(|| name_or_identifier(node, source)),
    }
}

fn property_fetch_expr(node: Node, source: &str, is_static: bool, is_nullsafe: bool) -> Value {
    let children = named_children(node);
    let target_node = if is_static {
        node.child_by_field_name("scope")
    } else {
        children.first().copied()
    };
    let target = target_node
        .map(|n| {
            if is_static
                && matches!(
                    n.kind(),
                    "name"
                        | "qualified_name"
                        | "fully_qualified_name"
                        | "relative_name"
                        | "relative_scope"
                )
            {
                name_node(n, source)
            } else {
                lower_expr(n, source).unwrap_or_else(|| variable_from_name("this", node, source))
            }
        })
        .unwrap_or_else(|| variable_from_name("this", node, source));
    let name = children
        .last()
        .map(|n| match n.kind() {
            "name" | "qualified_name" | "fully_qualified_name" | "member_name" => {
                name_or_identifier(*n, source)
            }
            "variable_name" | "dynamic_variable_name" => variable_expr(*n, source),
            _ => lower_expr(*n, source).unwrap_or_else(|| name_or_identifier(*n, source)),
        })
        .unwrap_or_else(|| identifier_from_text("unknown", node, source));
    object(
        if is_static {
            "Expr_StaticPropertyFetch"
        } else if is_nullsafe {
            "Expr_NullsafePropertyFetch"
        } else {
            "Expr_PropertyFetch"
        },
        node,
        source,
        [
            (if is_static { "class" } else { "var" }, target),
            ("name", name),
        ],
    )
}

fn class_const_fetch_expr(node: Node, source: &str) -> Value {
    let children = named_children(node);
    let class = node
        .child_by_field_name("scope")
        .or_else(|| children.first().copied())
        .map(|n| {
            if matches!(
                n.kind(),
                "name"
                    | "qualified_name"
                    | "fully_qualified_name"
                    | "relative_name"
                    | "relative_scope"
            ) {
                name_node(n, source)
            } else {
                lower_expr(n, source).unwrap_or_else(|| name_or_identifier(n, source))
            }
        })
        .unwrap_or_else(|| name_from_text("UNKNOWN", node, source));
    let name = node
        .child_by_field_name("name")
        .or_else(|| children.last().copied())
        .map(|n| match n.kind() {
            "name" | "qualified_name" | "fully_qualified_name" | "relative_name" => {
                name_node(n, source)
            }
            "variable_name" | "dynamic_variable_name" => variable_expr(n, source),
            _ => lower_expr(n, source).unwrap_or_else(|| name_or_identifier(n, source)),
        })
        .unwrap_or_else(|| name_from_text("UNKNOWN", node, source));

    object(
        "Expr_ClassConstFetch",
        node,
        source,
        [("class", class), ("name", name)],
    )
}

fn array_dim_fetch_expr(node: Node, source: &str) -> Value {
    let children = named_children(node);
    let var = children
        .first()
        .and_then(|n| lower_expr(*n, source))
        .unwrap_or_else(|| variable_from_name("array", node, source));
    let dim = if children.len() > 1 {
        children
            .last()
            .and_then(|n| lower_expr(*n, source))
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    object(
        "Expr_ArrayDimFetch",
        node,
        source,
        [("var", var), ("dim", dim)],
    )
}

fn array_expr(node: Node, source: &str) -> Value {
    let items = named_children(node)
        .into_iter()
        .filter_map(|child| {
            if matches!(child.kind(), "array_element_initializer" | "pair") {
                array_item(child, source)
            } else {
                lower_expr(child, source).map(|value| {
                    object(
                        "ArrayItem",
                        child,
                        source,
                        [
                            ("key", Value::Null),
                            ("value", value),
                            ("byRef", json!(false)),
                            ("unpack", json!(false)),
                        ],
                    )
                })
            }
        })
        .collect::<Vec<_>>();
    object("Expr_Array", node, source, [("items", Value::Array(items))])
}

fn list_expr(node: Node, source: &str) -> Value {
    let children = named_children(node);
    let mut items = Vec::new();
    let mut idx = 0;
    while idx < children.len() {
        let child = children[idx];
        if idx + 1 < children.len() && has_double_arrow_between(child, children[idx + 1], source) {
            if let (Some(key), Some(value)) = (
                lower_expr_or_wrapper(child, source),
                lower_expr_or_wrapper(children[idx + 1], source),
            ) {
                items.push(object(
                    "ArrayItem",
                    child,
                    source,
                    [
                        ("key", key),
                        ("value", value),
                        ("byRef", json!(false)),
                        ("unpack", json!(false)),
                    ],
                ));
            }
            idx += 2;
            continue;
        }
        if matches!(child.kind(), "array_element_initializer" | "pair") {
            if let Some(item) = array_item(child, source) {
                items.push(item);
            }
        } else if let Some(value) = lower_expr_or_wrapper(child, source) {
            items.push(object(
                "ArrayItem",
                child,
                source,
                [
                    ("key", Value::Null),
                    ("value", value),
                    ("byRef", json!(child.kind() == "by_ref")),
                    ("unpack", json!(child.kind() == "variadic_unpacking")),
                ],
            ));
        }
        idx += 1;
    }
    object("Expr_List", node, source, [("items", Value::Array(items))])
}

fn has_double_arrow_between(left: Node, right: Node, source: &str) -> bool {
    source
        .get(left.end_byte()..right.start_byte())
        .map(|between| between.contains("=>"))
        .unwrap_or(false)
}

fn array_item(node: Node, source: &str) -> Option<Value> {
    let children = named_children(node);
    let exprs = children
        .iter()
        .filter_map(|child| lower_expr_or_wrapper(*child, source))
        .collect::<Vec<_>>();

    let value = exprs.last()?.clone();
    let key = if exprs.len() >= 2 {
        exprs.first().cloned().unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let by_ref = children.iter().any(|child| child.kind() == "by_ref");
    let unpack = children
        .iter()
        .any(|child| child.kind() == "variadic_unpacking");

    Some(object(
        "ArrayItem",
        node,
        source,
        [
            ("key", key),
            ("value", value),
            ("byRef", json!(by_ref)),
            ("unpack", json!(unpack)),
        ],
    ))
}

fn lower_expr_or_wrapper(node: Node, source: &str) -> Option<Value> {
    match node.kind() {
        "variadic_unpacking" => named_children(node)
            .into_iter()
            .find_map(|child| lower_expr_or_wrapper(child, source)),
        "by_ref" | "reference_modifier" => None,
        _ => lower_expr(node, source),
    }
}

fn new_expr(node: Node, source: &str) -> Value {
    let anonymous_class = child_by_kind(node, &["anonymous_class"]);
    let class = anonymous_class
        .map(|class_node| anonymous_class_stmt(class_node, node, source))
        .or_else(|| {
            child_by_kind(node, &["name", "qualified_name", "fully_qualified_name"])
                .map(|n| name_node(n, source))
        })
        .or_else(|| {
            child_by_kind(node, &["variable_name", "dynamic_variable_name"])
                .and_then(|n| lower_expr(n, source))
        })
        .unwrap_or_else(|| name_from_text("UNKNOWN", node, source));
    let args = child_by_kind(node, &["arguments"])
        .or_else(|| {
            anonymous_class.and_then(|class_node| child_by_kind(class_node, &["arguments"]))
        })
        .map(|args| call_args(args, source))
        .unwrap_or_default();
    object(
        "Expr_New",
        node,
        source,
        [("class", class), ("args", Value::Array(args))],
    )
}

fn anonymous_class_stmt(class_node: Node, new_node: Node, source: &str) -> Value {
    let class_name = anonymous_class_name(new_node, source);
    class_like_stmt_with_name(
        class_node,
        source,
        "Stmt_Class",
        Some(identifier_from_text(class_name, class_node, source)),
    )
}

fn anonymous_class_name(new_node: Node, source: &str) -> String {
    let method = ancestor_by_kind(new_node, &["method_declaration", "function_definition"]);
    let class = ancestor_by_kind(
        new_node,
        &[
            "class_declaration",
            "interface_declaration",
            "trait_declaration",
        ],
    );
    let method_name = method
        .and_then(|method_node| child_by_kind(method_node, &["name"]))
        .map(|name| text(name, source))
        .or_else(|| last_open_decl_name_before(source, new_node.start_byte(), "function"));
    let class_name = class
        .and_then(|class_node| child_by_kind(class_node, &["name"]))
        .map(|name| text(name, source))
        .or_else(|| last_open_decl_name_before(source, new_node.start_byte(), "class"));
    let scope_start = method
        .or(class)
        .map(|scope| scope.start_byte())
        .unwrap_or(0);
    let index = source
        .get(scope_start..new_node.start_byte())
        .map(|prefix| prefix.matches("new class").count())
        .unwrap_or(0);
    let mut parts = Vec::new();
    if let Some(class_name) = class_name {
        parts.push(class_name);
    }
    if let Some(method_name) = method_name {
        parts.push(method_name);
    }
    parts.push(format!("anon-class-{index}"));
    parts.join(".")
}

fn last_open_decl_name_before(source: &str, end_byte: usize, keyword: &str) -> Option<String> {
    let prefix = source.get(..end_byte).unwrap_or(source);
    let mut result = None;
    let mut search_start = 0;
    while let Some(relative_pos) = prefix[search_start..].find(keyword) {
        let pos = search_start + relative_pos;
        let before = prefix[..pos].chars().next_back();
        let after_keyword = pos + keyword.len();
        let after = prefix[after_keyword..].chars().next();
        let has_word_boundary_before = before.is_none_or(|ch| !is_identifier_char(ch));
        let has_word_boundary_after = after.is_none_or(|ch| !is_identifier_char(ch));
        if has_word_boundary_before && has_word_boundary_after {
            let rest = prefix[after_keyword..].trim_start();
            let name = rest
                .chars()
                .take_while(|ch| is_identifier_char(*ch))
                .collect::<String>();
            if !name.is_empty() {
                let name_end = after_keyword + rest.len() - rest[name.len()..].len();
                if declaration_is_open_at(source, name_end, end_byte) {
                    result = Some(name);
                }
            }
        }
        search_start = after_keyword;
    }
    result
}

fn declaration_is_open_at(source: &str, after_name: usize, end_byte: usize) -> bool {
    let Some(open_relative) = source.get(after_name..end_byte).and_then(|s| s.find('{')) else {
        return false;
    };
    let open = after_name + open_relative;
    let mut depth = 0usize;
    for ch in source[open..end_byte].chars() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    depth > 0
}

fn is_identifier_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn ternary_expr(node: Node, source: &str) -> Value {
    let cond = node
        .child_by_field_name("condition")
        .and_then(|n| lower_expr(n, source))
        .unwrap_or_else(|| const_fetch(node, source, "true".to_string()));
    let then_expr = node
        .child_by_field_name("body")
        .and_then(|n| lower_expr(n, source))
        .unwrap_or(Value::Null);
    let else_expr = node
        .child_by_field_name("alternative")
        .and_then(|n| lower_expr(n, source))
        .unwrap_or_else(|| const_fetch(node, source, "NULL".to_string()));
    object(
        "Expr_Ternary",
        node,
        source,
        [("cond", cond), ("if", then_expr), ("else", else_expr)],
    )
}

fn match_expr(node: Node, source: &str) -> Value {
    let cond = node
        .child_by_field_name("condition")
        .and_then(|n| lower_expr(n, source))
        .unwrap_or_else(|| const_fetch(node, source, "true".to_string()));
    let arms = node
        .child_by_field_name("body")
        .map(|body| {
            named_children(body)
                .into_iter()
                .filter_map(|arm| match arm.kind() {
                    "match_conditional_expression" | "match_default_expression" => {
                        Some(match_arm(arm, source))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    object(
        "Expr_Match",
        node,
        source,
        [("cond", cond), ("arms", Value::Array(arms))],
    )
}

fn match_arm(node: Node, source: &str) -> Value {
    let conds = node
        .child_by_field_name("conditional_expressions")
        .map(|conds| {
            Value::Array(
                named_children(conds)
                    .into_iter()
                    .filter_map(|cond| lower_expr(cond, source))
                    .collect(),
            )
        })
        .unwrap_or(Value::Null);
    let body = node
        .child_by_field_name("return_expression")
        .and_then(|expr| lower_expr(expr, source))
        .unwrap_or_else(|| const_fetch(node, source, "NULL".to_string()));
    object("MatchArm", node, source, [("conds", conds), ("body", body)])
}

fn cast_expr(node: Node, source: &str) -> Value {
    let expr = named_children(node)
        .last()
        .and_then(|n| lower_expr(*n, source))
        .unwrap_or_else(|| const_fetch(node, source, "NULL".to_string()));
    object(cast_node_type(node, source), node, source, [("expr", expr)])
}

fn throw_expr(node: Node, source: &str) -> Value {
    let expr = named_children(node)
        .into_iter()
        .find_map(|child| lower_expr(child, source))
        .unwrap_or_else(|| const_fetch(node, source, "NULL".to_string()));
    object("Expr_Throw", node, source, [("expr", expr)])
}

fn include_expr(node: Node, source: &str) -> Value {
    let expr = named_children(node)
        .last()
        .and_then(|n| lower_expr(*n, source))
        .unwrap_or_else(|| string_expr(node, source));
    object(
        "Expr_Include",
        node,
        source,
        [
            ("expr", expr),
            ("type", json!(include_type_num(node.kind()))),
        ],
    )
}

fn shell_exec_expr(node: Node, source: &str) -> Value {
    let raw = text(node, source);
    let value = raw.trim_matches('`').to_string();
    let part = object(
        "Scalar_EncapsedStringPart",
        node,
        source,
        [("value", Value::String(value))],
    );
    object(
        "Expr_ShellExec",
        node,
        source,
        [("parts", Value::Array(vec![part]))],
    )
}

fn yield_expr(node: Node, source: &str) -> Value {
    let raw = text(node, source);
    let children = named_children(node);
    if raw.trim_start().starts_with("yield from") {
        let expr = children
            .into_iter()
            .find_map(|child| lower_expr(child, source))
            .unwrap_or_else(|| const_fetch(node, source, "NULL".to_string()));
        return object("Expr_YieldFrom", node, source, [("expr", expr)]);
    }

    let (key, value) = children
        .first()
        .and_then(|child| {
            if child.kind() == "array_element_initializer" {
                let item = array_item(*child, source)?;
                let key = item.get("key").cloned().unwrap_or(Value::Null);
                let value = item.get("value").cloned().unwrap_or(Value::Null);
                Some((key, value))
            } else {
                lower_expr(*child, source).map(|value| (Value::Null, value))
            }
        })
        .unwrap_or((Value::Null, Value::Null));
    object("Expr_Yield", node, source, [("key", key), ("value", value)])
}

fn closure_expr(node: Node, source: &str, is_arrow: bool) -> Value {
    let params = child_by_kind(node, &["formal_parameters"])
        .map(|p| params(p, source))
        .unwrap_or_default();
    if is_arrow {
        let expr = named_children(node)
            .into_iter()
            .rev()
            .find_map(|child| lower_expr(child, source))
            .unwrap_or_else(|| const_fetch(node, source, "NULL".to_string()));
        object(
            "Expr_ArrowFunction",
            node,
            source,
            [
                ("static", json!(false)),
                ("byRef", json!(false)),
                ("params", Value::Array(params)),
                ("returnType", Value::Null),
                ("expr", expr),
            ],
        )
    } else {
        let stmts = child_by_kind(node, &["compound_statement"])
            .map(|body| statements_in_body(body, source))
            .unwrap_or_default();
        let uses = child_by_kind(node, &["anonymous_function_use_clause"])
            .map(|use_clause| closure_uses(use_clause, source))
            .unwrap_or_default();
        let return_type = child_by_field_or_kind(
            node,
            "return_type",
            &["primitive_type", "named_type", "optional_type"],
        )
        .map(|typ| type_node(typ, source))
        .unwrap_or(Value::Null);
        object(
            "Expr_Closure",
            node,
            source,
            [
                (
                    "static",
                    json!(node.child_by_field_name("static_modifier").is_some()),
                ),
                (
                    "byRef",
                    json!(node.child_by_field_name("reference_modifier").is_some()),
                ),
                ("params", Value::Array(params)),
                ("returnType", return_type),
                ("uses", Value::Array(uses)),
                ("stmts", Value::Array(stmts)),
            ],
        )
    }
}

fn closure_uses(node: Node, source: &str) -> Vec<Value> {
    named_children(node)
        .into_iter()
        .filter_map(|child| match child.kind() {
            "variable_name" => Some(closure_use(child, source, false, child)),
            "by_ref" => named_children(child)
                .into_iter()
                .find(|by_ref_child| by_ref_child.kind() == "variable_name")
                .map(|var| closure_use(child, source, true, var)),
            _ => None,
        })
        .collect()
}

fn closure_use(node: Node, source: &str, by_ref: bool, variable: Node) -> Value {
    object(
        "ClosureUse",
        node,
        source,
        [
            ("var", variable_expr(variable, source)),
            ("byRef", json!(by_ref)),
        ],
    )
}

fn params(node: Node, source: &str) -> Vec<Value> {
    named_children(node)
        .into_iter()
        .filter(|child| {
            matches!(
                child.kind(),
                "simple_parameter" | "variadic_parameter" | "optional_parameter"
            )
        })
        .map(|param| {
            let var = child_by_kind(param, &["variable_name"])
                .map(|var| variable_expr(var, source))
                .unwrap_or_else(|| variable_from_name("arg", param, source));
            let typ = child_by_field_or_kind(
                param,
                "type",
                &["primitive_type", "named_type", "optional_type"],
            )
            .map(|typ| type_node(typ, source))
            .unwrap_or(Value::Null);
            object(
                "Param",
                param,
                source,
                [
                    ("type", typ),
                    ("byRef", json!(false)),
                    ("variadic", json!(param.kind() == "variadic_parameter")),
                    ("var", var),
                    ("default", Value::Null),
                    ("flags", json!(0)),
                    ("attrGroups", Value::Array(attribute_groups(param, source))),
                ],
            )
        })
        .collect()
}

fn call_args(node: Node, source: &str) -> Vec<Value> {
    named_children(node)
        .into_iter()
        .filter_map(|child| {
            if child.kind() == "argument" {
                call_arg(child, source)
            } else {
                lower_expr(child, source).map(|value| {
                    object(
                        "Arg",
                        child,
                        source,
                        [
                            ("name", Value::Null),
                            ("value", value),
                            ("byRef", json!(false)),
                            ("unpack", json!(false)),
                        ],
                    )
                })
            }
        })
        .collect()
}

fn call_arg(node: Node, source: &str) -> Option<Value> {
    let children = named_children(node);
    let name_node = node.child_by_field_name("name");
    let name = name_node
        .map(|name| identifier_node(name, source))
        .unwrap_or(Value::Null);
    let value = children
        .iter()
        .rev()
        .filter(|child| name_node != Some(**child))
        .find_map(|child| lower_expr_or_wrapper(*child, source))?;
    let by_ref = node.child_by_field_name("reference_modifier").is_some();
    let unpack = children
        .iter()
        .any(|child| child.kind() == "variadic_unpacking");

    Some(object(
        "Arg",
        node,
        source,
        [
            ("name", name),
            ("value", value),
            ("byRef", json!(by_ref)),
            ("unpack", json!(unpack)),
        ],
    ))
}

fn static_stmt(node: Node, source: &str) -> Value {
    let vars = named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "static_variable_declaration")
        .map(|child| {
            let var = child
                .child_by_field_name("name")
                .map(|var| variable_expr(var, source))
                .unwrap_or_else(|| variable_from_name("unknown", child, source));
            let default = child
                .child_by_field_name("value")
                .and_then(|value| lower_expr(value, source))
                .unwrap_or(Value::Null);
            object(
                "StaticVar",
                child,
                source,
                [("var", var), ("default", default)],
            )
        })
        .collect::<Vec<_>>();
    object("Stmt_Static", node, source, [("vars", Value::Array(vars))])
}

fn unset_stmt(node: Node, source: &str) -> Value {
    let vars = named_children(node)
        .into_iter()
        .filter_map(|child| lower_expr(child, source))
        .collect::<Vec<_>>();
    object("Stmt_Unset", node, source, [("vars", Value::Array(vars))])
}

fn global_stmt(node: Node, source: &str) -> Value {
    let vars = named_children(node)
        .into_iter()
        .filter_map(|child| lower_expr(child, source))
        .collect::<Vec<_>>();
    object("Stmt_Global", node, source, [("vars", Value::Array(vars))])
}

fn declare_stmt(node: Node, source: &str) -> Value {
    let declares = declare_items_from_text(node, source);
    let body_children = named_children(node)
        .into_iter()
        .filter(|child| {
            child.kind() != "declare_directive"
                && matches!(
                    child.kind(),
                    "compound_statement"
                        | "colon_block"
                        | "expression_statement"
                        | "echo_statement"
                )
        })
        .collect::<Vec<_>>();
    let stmts = body_children
        .iter()
        .flat_map(|child| body_statements(*child, source))
        .collect::<Vec<_>>();
    let raw = text(node, source);
    let has_body = raw
        .split_once(')')
        .map(|(_, tail)| tail.trim_start().starts_with('{') || tail.trim_start().starts_with(':'))
        .unwrap_or(false);
    let stmt_value = if has_body {
        Value::Array(stmts)
    } else {
        Value::Null
    };
    object(
        "Stmt_Declare",
        node,
        source,
        [("declares", Value::Array(declares)), ("stmts", stmt_value)],
    )
}

fn exit_stmt(node: Node, source: &str) -> Value {
    let expr = named_children(node)
        .into_iter()
        .find_map(|child| lower_expr(child, source))
        .unwrap_or(Value::Null);
    object(
        "Stmt_Expression",
        node,
        source,
        [("expr", object("Expr_Exit", node, source, [("expr", expr)]))],
    )
}

fn jump_depth(node: Node, source: &str) -> Value {
    named_children(node)
        .into_iter()
        .find_map(|child| lower_expr(child, source))
        .unwrap_or(Value::Null)
}

fn goto_stmt(node: Node, source: &str) -> Value {
    let name = child_by_kind(node, &["name"])
        .map(|name| name_node(name, source))
        .unwrap_or_else(|| name_from_text("UNKNOWN", node, source));
    object("Stmt_Goto", node, source, [("name", name)])
}

fn label_stmt(node: Node, source: &str) -> Value {
    let name = child_by_kind(node, &["name"])
        .map(|name| name_node(name, source))
        .unwrap_or_else(|| name_from_text("UNKNOWN", node, source));
    object("Stmt_Label", node, source, [("name", name)])
}

fn declare_items_from_text(node: Node, source: &str) -> Vec<Value> {
    let raw = text(node, source);
    raw.split_once('(')
        .and_then(|(_, rest)| rest.rsplit_once(')').map(|(inside, _)| inside))
        .map(|inside| {
            inside
                .split(',')
                .filter_map(|part| declare_item_from_pair(node, source, part))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn declare_item_from_pair(node: Node, source: &str, raw: &str) -> Option<Value> {
    let (key, value) = raw.split_once('=')?;
    declare_item_from_key_value(node, source, key.trim(), value.trim())
}

fn declare_item_from_key_value(node: Node, source: &str, key: &str, value: &str) -> Option<Value> {
    let value_node = if value.starts_with('\'') || value.starts_with('"') {
        object(
            "Scalar_String",
            node,
            source,
            [(
                "value",
                Value::String(
                    value
                        .trim_matches('\'')
                        .trim_matches('"')
                        .replace("\\\"", "\"")
                        .replace("\\'", "'"),
                ),
            )],
        )
    } else {
        object(
            "Scalar_LNumber",
            node,
            source,
            [("value", number_or_string(value.to_string()))],
        )
    };
    Some(object(
        "DeclareItem",
        node,
        source,
        [
            ("key", name_from_text(key, node, source)),
            ("value", value_node),
        ],
    ))
}

fn statements_in_body(node: Node, source: &str) -> Vec<Value> {
    named_children(node)
        .into_iter()
        .filter_map(|child| lower_stmt(child, source))
        .collect()
}

fn body_statements(node: Node, source: &str) -> Vec<Value> {
    match node.kind() {
        "compound_statement" | "colon_block" | "declaration_list" => {
            statements_in_body(node, source)
        }
        _ => lower_stmt(node, source).into_iter().collect(),
    }
}

fn stmts_as_block(node: Node, source: &str) -> Value {
    object(
        "Stmt_Nop",
        node,
        source,
        [("stmts", Value::Array(statements_in_body(node, source)))],
    )
}

fn string_expr(node: Node, source: &str) -> Value {
    let raw = text(node, source);
    let value = raw
        .trim_matches('"')
        .trim_matches('\'')
        .trim_start_matches("<<<")
        .to_string();
    object(
        "Scalar_String",
        node,
        source,
        [("value", Value::String(value))],
    )
}

fn heredoc_expr(node: Node, source: &str) -> Value {
    let body = node
        .child_by_field_name("value")
        .or_else(|| child_by_kind(node, &["heredoc_body"]));
    let parts = body.map(named_children).unwrap_or_default();
    if parts.iter().all(|child| is_string_fragment(*child)) {
        return string_expr(node, source);
    }
    let encapsed_parts = parts
        .into_iter()
        .filter_map(|child| match child.kind() {
            "string_content" | "escape_sequence" => Some(object(
                "Scalar_EncapsedStringPart",
                child,
                source,
                [(
                    "value",
                    Value::String(decode_php_escape(&text(child, source))),
                )],
            )),
            _ => lower_expr(child, source),
        })
        .collect::<Vec<_>>();
    object(
        "Scalar_Encapsed",
        node,
        source,
        [("parts", Value::Array(encapsed_parts))],
    )
}

fn encapsed_expr(node: Node, source: &str) -> Value {
    let children = named_children(node);
    if children.iter().all(|child| is_string_fragment(*child)) {
        let value = if children.is_empty() {
            text(node, source).trim_matches('"').to_string()
        } else {
            children
                .iter()
                .map(|child| decode_php_escape(&text(*child, source)))
                .collect::<String>()
        };
        return object(
            "Scalar_String",
            node,
            source,
            [("value", Value::String(value))],
        );
    }

    let parts = children
        .into_iter()
        .filter_map(|child| match child.kind() {
            "string_content" | "escape_sequence" => Some(object(
                "Scalar_EncapsedStringPart",
                child,
                source,
                [(
                    "value",
                    Value::String(decode_php_escape(&text(child, source))),
                )],
            )),
            _ => lower_expr(child, source),
        })
        .collect::<Vec<_>>();
    object(
        "Scalar_Encapsed",
        node,
        source,
        [("parts", Value::Array(parts))],
    )
}

fn is_string_fragment(node: Node) -> bool {
    matches!(node.kind(), "string_content" | "escape_sequence")
}

fn variable_expr(node: Node, source: &str) -> Value {
    let raw = text(node, source);
    let name = if raw.trim_start().starts_with("$$") {
        object(
            "Expr_Variable",
            node,
            source,
            [("name", Value::String(normalize_variable_name(&raw[1..])))],
        )
    } else {
        Value::String(normalize_variable_name(&raw))
    };
    object("Expr_Variable", node, source, [("name", name)])
}

fn normalize_variable_name(raw: &str) -> String {
    raw.trim()
        .trim_start_matches('$')
        .trim_start_matches('{')
        .trim_start_matches('$')
        .trim_end_matches('}')
        .to_string()
}

fn decode_php_escape(raw: &str) -> String {
    match raw {
        "\\n" => "\n".to_string(),
        "\\r" => "\r".to_string(),
        "\\t" => "\t".to_string(),
        "\\\"" => "\"".to_string(),
        "\\\\" => "\\".to_string(),
        "\\$" => "$".to_string(),
        _ => raw.to_string(),
    }
}

fn recover_line_assignments(source: &str) -> Vec<Value> {
    let mut offset = 0usize;
    let mut recovered = Vec::new();
    for (idx, line) in source.lines().enumerate() {
        let row = idx + 1;
        let trimmed = line.trim();
        let leading = line.len().saturating_sub(line.trim_start().len());
        if trimmed.starts_with("<?php") || !trimmed.starts_with('$') || !trimmed.contains('=') {
            offset += line.len() + 1;
            continue;
        }

        let Some(eq_idx_trimmed) = trimmed.find('=') else {
            offset += line.len() + 1;
            continue;
        };
        let var_name = normalize_variable_name(&trimmed[..eq_idx_trimmed]);
        let rhs = trimmed[eq_idx_trimmed + 1..]
            .trim()
            .trim_end_matches(';')
            .trim();
        if !(rhs.starts_with('"')
            || rhs.starts_with('\'')
            || rhs.starts_with('$')
            || rhs.starts_with("&$"))
        {
            offset += line.len() + 1;
            continue;
        }

        let start = offset + leading;
        let end = start + trimmed.len().saturating_sub(1);
        let var_node = object_at(
            "Expr_Variable",
            row,
            start,
            start + eq_idx_trimmed.saturating_sub(1),
            [("name", Value::String(var_name))],
        );
        let literal_start = start + eq_idx_trimmed + 1 + trimmed[eq_idx_trimmed + 1..].len()
            - trimmed[eq_idx_trimmed + 1..].trim_start().len();
        let source_expr = if rhs.starts_with('$') || rhs.starts_with("&$") {
            object_at(
                "Expr_Variable",
                row,
                literal_start + usize::from(rhs.starts_with('&')),
                end,
                [(
                    "name",
                    Value::String(normalize_variable_name(rhs.trim_start_matches('&'))),
                )],
            )
        } else {
            let value = rhs
                .strip_prefix('"')
                .and_then(|x| x.strip_suffix('"'))
                .or_else(|| rhs.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
                .unwrap_or(rhs)
                .replace("\\\"", "\"")
                .replace("\\'", "'");
            object_at(
                "Scalar_String",
                row,
                literal_start,
                end,
                [("value", Value::String(value))],
            )
        };
        let assign = object_at(
            if rhs.starts_with('&') {
                "Expr_AssignRef"
            } else {
                "Expr_Assign"
            },
            row,
            start,
            end,
            [("var", var_node), ("expr", source_expr)],
        );
        recovered.push(object_at(
            "Stmt_Expression",
            row,
            start,
            end,
            [("expr", assign)],
        ));

        offset += line.len() + 1;
    }
    recovered
}

fn recover_global_functions(source: &str) -> Vec<Value> {
    let mut recovered = Vec::new();
    let mut offset = 0usize;
    let lines = source.lines().collect::<Vec<_>>();
    let mut line_offsets = Vec::with_capacity(lines.len());
    for line in &lines {
        line_offsets.push(offset);
        offset += line.len() + 1;
    }

    let mut idx = 0;
    while idx < lines.len() {
        let trimmed = lines[idx].trim();
        if !trimmed.starts_with("function ") {
            idx += 1;
            continue;
        }
        let fn_idx = idx;
        let name = trimmed
            .strip_prefix("function ")
            .and_then(|rest| rest.split_once('(').map(|(name, _)| name.trim()))
            .filter(|name| !name.is_empty())
            .unwrap_or("<anonymous>");
        let start = line_offsets[idx]
            + lines[idx]
                .len()
                .saturating_sub(lines[idx].trim_start().len());
        let mut stmts = Vec::new();
        let mut end = start + trimmed.len().saturating_sub(1);
        idx += 1;
        while idx < lines.len() {
            let body_trimmed = lines[idx].trim();
            let body_start = line_offsets[idx]
                + lines[idx]
                    .len()
                    .saturating_sub(lines[idx].trim_start().len());
            if body_trimmed.starts_with("global ") {
                let vars = body_trimmed
                    .trim_start_matches("global ")
                    .trim_end_matches(';')
                    .split(',')
                    .filter_map(|raw| {
                        let raw = raw.trim();
                        (raw.starts_with('$') && !raw.starts_with("$$")).then(|| {
                            object_at(
                                "Expr_Variable",
                                idx + 1,
                                body_start,
                                body_start + raw.len().saturating_sub(1),
                                [("name", Value::String(normalize_variable_name(raw)))],
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                stmts.push(object_at(
                    "Stmt_Global",
                    idx + 1,
                    body_start,
                    body_start + body_trimmed.len().saturating_sub(1),
                    [("vars", Value::Array(vars))],
                ));
            }
            end = body_start + body_trimmed.len().saturating_sub(1);
            if body_trimmed.starts_with('}') {
                break;
            }
            idx += 1;
        }
        let name_node = object_at(
            "Identifier",
            fn_idx + 1,
            line_offsets[fn_idx] + lines[fn_idx].find(name).unwrap_or(0),
            line_offsets[fn_idx]
                + lines[fn_idx].find(name).unwrap_or(0)
                + name.len().saturating_sub(1),
            [("name", Value::String(name.to_string()))],
        );
        recovered.push(object_at(
            "Stmt_Function",
            fn_idx + 1,
            start,
            end,
            [
                ("byRef", json!(false)),
                ("name", name_node),
                ("params", Value::Array(Vec::new())),
                ("returnType", Value::Null),
                ("stmts", Value::Array(stmts)),
                ("namespacedName", Value::Null),
                ("attrGroups", Value::Array(Vec::new())),
            ],
        ));
        idx += 1;
    }
    recovered
}

fn has_stmt_type(stmts: &[Value], node_type: &str) -> bool {
    stmts
        .iter()
        .any(|stmt| value_has_node_type(stmt, node_type))
}

fn value_has_node_type(value: &Value, node_type: &str) -> bool {
    match value {
        Value::Object(map) => {
            map.get("nodeType")
                .and_then(Value::as_str)
                .is_some_and(|typ| typ == node_type)
                || map
                    .values()
                    .any(|child| value_has_node_type(child, node_type))
        }
        Value::Array(values) => values
            .iter()
            .any(|child| value_has_node_type(child, node_type)),
        _ => false,
    }
}

fn object_at<'a>(
    node_type: &str,
    row: usize,
    start: usize,
    end: usize,
    fields: impl IntoIterator<Item = (&'a str, Value)>,
) -> Value {
    let mut map = Map::new();
    map.insert("nodeType".to_string(), Value::String(node_type.to_string()));
    map.insert(
        "attributes".to_string(),
        json!({
            "startLine": row,
            "startFilePos": start,
            "endLine": row,
            "endFilePos": end
        }),
    );
    for (key, value) in fields {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

fn variable_from_name(name: &str, node: Node, source: &str) -> Value {
    object(
        "Expr_Variable",
        node,
        source,
        [("name", Value::String(name.to_string()))],
    )
}

fn const_fetch(node: Node, source: &str, name: String) -> Value {
    object(
        "Expr_ConstFetch",
        node,
        source,
        [("name", name_from_text(&name, node, source))],
    )
}

fn name_expr(node: Node, source: &str, node_type: &str) -> Value {
    object(node_type, node, source, [("name", name_node(node, source))])
}

fn name_or_identifier(node: Node, source: &str) -> Value {
    if matches!(
        node.kind(),
        "name" | "qualified_name" | "fully_qualified_name"
    ) {
        name_node(node, source)
    } else {
        identifier_node(node, source)
    }
}

fn name_node(node: Node, source: &str) -> Value {
    let raw = text(node, source).trim_start_matches('\\').to_string();
    let parts = raw
        .split('\\')
        .filter(|part| !part.is_empty())
        .map(|part| Value::String(part.to_string()))
        .collect::<Vec<_>>();
    object(
        if text(node, source).starts_with('\\') {
            "Name_FullyQualified"
        } else {
            "Name"
        },
        node,
        source,
        [("parts", Value::Array(parts))],
    )
}

fn name_from_text(name: &str, node: Node, source: &str) -> Value {
    object(
        "Name",
        node,
        source,
        [(
            "parts",
            Value::Array(
                name.split('\\')
                    .map(|part| Value::String(part.to_string()))
                    .collect(),
            ),
        )],
    )
}

fn identifier_node(node: Node, source: &str) -> Value {
    identifier_from_text(text(node, source), node, source)
}

fn identifier_from_text(name: impl Into<String>, node: Node, source: &str) -> Value {
    object(
        "Identifier",
        node,
        source,
        [("name", Value::String(name.into()))],
    )
}

fn varlike_identifier(node: Node, source: &str) -> Value {
    object(
        "VarLikeIdentifier",
        node,
        source,
        [(
            "name",
            Value::String(normalize_variable_name(&text(node, source))),
        )],
    )
}

fn type_node(node: Node, source: &str) -> Value {
    identifier_from_text(text(node, source).trim_start_matches('?'), node, source)
}

fn assign_node_type(code: &str) -> &'static str {
    if code.contains("=&") || code.contains("= &") {
        "Expr_AssignRef"
    } else if code.contains("&=") {
        "Expr_AssignOp_BitwiseAnd"
    } else if code.contains("|=") {
        "Expr_AssignOp_BitwiseOr"
    } else if code.contains("^=") {
        "Expr_AssignOp_BitwiseXor"
    } else if code.contains("??=") {
        "Expr_AssignOp_Coalesce"
    } else if code.contains(".=") {
        "Expr_AssignOp_Concat"
    } else if code.contains("/=") {
        "Expr_AssignOp_Div"
    } else if code.contains("-=") {
        "Expr_AssignOp_Minus"
    } else if code.contains("%=") {
        "Expr_AssignOp_Mod"
    } else if code.contains("*=") && !code.contains("**=") {
        "Expr_AssignOp_Mul"
    } else if code.contains("+=") {
        "Expr_AssignOp_Plus"
    } else if code.contains("**=") {
        "Expr_AssignOp_Pow"
    } else if code.contains("<<=") {
        "Expr_AssignOp_ShiftLeft"
    } else if code.contains(">>=") {
        "Expr_AssignOp_ShiftRight"
    } else {
        "Expr_Assign"
    }
}

fn unary_node_type(code: &str) -> &'static str {
    let trimmed = code.trim();
    if trimmed.starts_with('~') {
        "Expr_BitwiseNot"
    } else if trimmed.starts_with('!') {
        "Expr_BooleanNot"
    } else if trimmed.ends_with("--") {
        "Expr_PostDec"
    } else if trimmed.ends_with("++") {
        "Expr_PostInc"
    } else if trimmed.starts_with("--") {
        "Expr_PreDec"
    } else if trimmed.starts_with("++") {
        "Expr_PreInc"
    } else if trimmed.starts_with('-') {
        "Expr_UnaryMinus"
    } else {
        "Expr_UnaryPlus"
    }
}

fn cast_node_type(node: Node, source: &str) -> &'static str {
    let typ = node
        .child_by_field_name("type")
        .map(|typ| text(typ, source).to_ascii_lowercase())
        .unwrap_or_else(|| text(node, source).to_ascii_lowercase());
    if typ.contains("array") {
        "Expr_Cast_Array"
    } else if typ.contains("bool") {
        "Expr_Cast_Bool"
    } else if typ.contains("double") || typ.contains("float") || typ.contains("real") {
        "Expr_Cast_Double"
    } else if typ.contains("int") {
        "Expr_Cast_Int"
    } else if typ.contains("object") {
        "Expr_Cast_Object"
    } else if typ.contains("unset") {
        "Expr_Cast_Unset"
    } else {
        "Expr_Cast_String"
    }
}

fn include_type_num(kind: &str) -> i32 {
    match kind {
        "include_once_expression" => 2,
        "require_expression" => 3,
        "require_once_expression" => 4,
        _ => 1,
    }
}

fn binary_node_type(code: &str) -> &'static str {
    if code.contains("===") {
        "Expr_BinaryOp_Identical"
    } else if code.contains("!==") {
        "Expr_BinaryOp_NotIdentical"
    } else if code.contains("<=>") {
        "Expr_BinaryOp_Spaceship"
    } else if code.contains("&&") {
        "Expr_BinaryOp_BooleanAnd"
    } else if code.contains("||") {
        "Expr_BinaryOp_BooleanOr"
    } else if code.contains(" and ") {
        "Expr_BinaryOp_LogicalAnd"
    } else if code.contains(" or ") {
        "Expr_BinaryOp_LogicalOr"
    } else if code.contains(" xor ") {
        "Expr_BinaryOp_LogicalXor"
    } else if code.contains("??") {
        "Expr_BinaryOp_Coalesce"
    } else if code.contains("==") {
        "Expr_BinaryOp_Equal"
    } else if code.contains("!=") || code.contains("<>") {
        "Expr_BinaryOp_NotEqual"
    } else if code.contains("<=") {
        "Expr_BinaryOp_SmallerOrEqual"
    } else if code.contains(">=") {
        "Expr_BinaryOp_GreaterOrEqual"
    } else if code.contains("**") {
        "Expr_BinaryOp_Pow"
    } else if code.contains("<<") {
        "Expr_BinaryOp_ShiftLeft"
    } else if code.contains(">>") {
        "Expr_BinaryOp_ShiftRight"
    } else if code.contains('&') {
        "Expr_BinaryOp_BitwiseAnd"
    } else if code.contains('|') {
        "Expr_BinaryOp_BitwiseOr"
    } else if code.contains('^') {
        "Expr_BinaryOp_BitwiseXor"
    } else if code.contains('.') {
        "Expr_BinaryOp_Concat"
    } else if code.contains('+') {
        "Expr_BinaryOp_Plus"
    } else if code.contains('-') {
        "Expr_BinaryOp_Minus"
    } else if code.contains('*') {
        "Expr_BinaryOp_Mul"
    } else if code.contains('%') {
        "Expr_BinaryOp_Mod"
    } else if code.contains('/') {
        "Expr_BinaryOp_Div"
    } else if code.contains('<') {
        "Expr_BinaryOp_Smaller"
    } else if code.contains('>') {
        "Expr_BinaryOp_Greater"
    } else {
        "Expr_BinaryOp_Plus"
    }
}

fn modifier_flags(node: Node, source: &str) -> i32 {
    let code = named_children(node)
        .into_iter()
        .filter(|child| child.kind().contains("modifier"))
        .map(|child| text(child, source))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    modifier_flags_from_code(&code)
}

fn modifier_flags_from_code(code: &str) -> i32 {
    let mut flags = 0;
    if code.contains("public") {
        flags |= 1;
    }
    if code.contains("protected") {
        flags |= 2;
    }
    if code.contains("private") {
        flags |= 4;
    }
    if code.contains("static") {
        flags |= 8;
    }
    if code.contains("abstract") {
        flags |= 16;
    }
    if code.contains("final") {
        flags |= 32;
    }
    if code.contains("readonly") {
        flags |= 64;
    }
    flags
}

fn child_by_kind<'a>(node: Node<'a>, kinds: &[&str]) -> Option<Node<'a>> {
    named_children(node)
        .into_iter()
        .find(|child| kinds.contains(&child.kind()))
}

fn child_by_field_or_kind<'a>(node: Node<'a>, field: &str, kinds: &[&str]) -> Option<Node<'a>> {
    node.child_by_field_name(field)
        .or_else(|| child_by_kind(node, kinds))
}

fn ancestor_by_kind<'a>(node: Node<'a>, kinds: &[&str]) -> Option<Node<'a>> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if kinds.contains(&parent.kind()) {
            return Some(parent);
        }
        current = parent.parent();
    }
    None
}

fn named_children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn text(node: Node, source: &str) -> String {
    node.utf8_text(source.as_bytes()).unwrap_or("").to_string()
}

fn number_or_string(raw: String) -> Value {
    raw.parse::<i64>()
        .map(Value::from)
        .or_else(|_| raw.parse::<f64>().map(Value::from))
        .unwrap_or(Value::String(raw))
}

fn object<'a>(
    node_type: &str,
    node: Node,
    source: &str,
    fields: impl IntoIterator<Item = (&'a str, Value)>,
) -> Value {
    let mut map = Map::new();
    map.insert("nodeType".to_string(), Value::String(node_type.to_string()));
    map.insert("attributes".to_string(), attributes(node, source));
    for (key, value) in fields {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

fn attributes(node: Node, _source: &str) -> Value {
    let start = node.start_position();
    let end = node.end_position();
    let start_byte = mapped_byte_offset(node.start_byte());
    let end_byte = mapped_byte_offset(node.end_byte());
    json!({
        "startLine": start.row + 1,
        "startFilePos": start_byte,
        "endLine": end.row + 1,
        "endFilePos": end_byte.saturating_sub(1)
    })
}

fn mapped_byte_offset(offset: usize) -> usize {
    BYTE_OFFSET_MAP.with(|cell| {
        cell.borrow()
            .as_ref()
            .and_then(|offset_map| offset_map.get(offset).copied())
            .unwrap_or(offset)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_function_assignment_and_return() {
        let json =
            generate_source("<?php\nfunction foo($x) { $y = 1 + 2; return $y; }\n").expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(arr[0]["nodeType"], "Stmt_Function");
        assert_eq!(arr[0]["name"]["name"], "foo");
    }

    #[test]
    fn emits_class_method() {
        let json = generate_source("<?php\nclass A { public function m($x) { echo $x; } }\n")
            .expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(arr[0]["nodeType"], "Stmt_Class");
    }

    #[test]
    fn emits_try_catch_finally() {
        let json = generate_source(
            "<?php\ntry { $body1; } catch (A | D $a) { $body2; } finally { $body3; }\n",
        )
        .expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(arr[0]["nodeType"], "Stmt_TryCatch");
        assert_eq!(arr[0]["catches"][0]["nodeType"], "Stmt_Catch");
        assert_eq!(arr[0]["catches"][0]["types"][0]["parts"][0], "A");
        assert_eq!(arr[0]["catches"][0]["types"][1]["parts"][0], "D");
        assert_eq!(arr[0]["catches"][0]["var"]["name"], "a");
        assert_eq!(arr[0]["finally"]["nodeType"], "Stmt_Finally");
    }

    #[test]
    fn emits_property_defaults_and_type() {
        let json = generate_source("<?php\nclass Foo { public string $a = \"a\", $b = \"b\"; }\n")
            .expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(arr[0]["stmts"][0]["nodeType"], "Stmt_Property");
        assert_eq!(arr[0]["stmts"][0]["type"]["name"], "string");
        assert_eq!(arr[0]["stmts"][0]["props"][0]["name"]["name"], "a");
        assert_eq!(arr[0]["stmts"][0]["props"][0]["default"]["value"], "a");
        assert_eq!(arr[0]["stmts"][0]["props"][1]["name"]["name"], "b");
        assert_eq!(arr[0]["stmts"][0]["props"][1]["default"]["value"], "b");
    }

    #[test]
    fn emits_class_const_fetch() {
        let json = generate_source("<?php\nFoo::X;\n").expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(arr[0]["nodeType"], "Stmt_Expression");
        assert_eq!(arr[0]["expr"]["nodeType"], "Expr_ClassConstFetch");
        assert_eq!(arr[0]["expr"]["class"]["parts"][0], "Foo");
        assert_eq!(arr[0]["expr"]["name"]["parts"][0], "X");
    }

    #[test]
    fn emits_class_implements_and_interface_extends() {
        let json = generate_source(
            "<?php\nclass A extends B implements C, D {}\ninterface I extends J, K {}\n",
        )
        .expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(arr[0]["extends"]["parts"][0], "B");
        assert_eq!(arr[0]["implements"][0]["parts"][0], "C");
        assert_eq!(arr[0]["implements"][1]["parts"][0], "D");
        assert_eq!(arr[1]["extends"][0]["parts"][0], "J");
        assert_eq!(arr[1]["extends"][1]["parts"][0], "K");
    }

    #[test]
    fn emits_enum_cases_and_methods() {
        let json = generate_source(
            "<?php\nenum E: string { case A; case B = \"B\"; public static function foo() {} }\n",
        )
        .expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(arr[0]["nodeType"], "Stmt_Enum");
        assert_eq!(arr[0]["scalarType"]["parts"][0], "string");
        assert_eq!(arr[0]["stmts"][0]["nodeType"], "Stmt_EnumCase");
        assert_eq!(arr[0]["stmts"][0]["name"]["name"], "A");
        assert_eq!(arr[0]["stmts"][1]["expr"]["value"], "B");
        assert_eq!(arr[0]["stmts"][2]["nodeType"], "Stmt_ClassMethod");
    }

    #[test]
    fn emits_dynamic_new_class_expr() {
        let json = generate_source("<?php\nnew $x();\n").expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(arr[0]["expr"]["nodeType"], "Expr_New");
        assert_eq!(arr[0]["expr"]["class"]["nodeType"], "Expr_Variable");
        assert_eq!(arr[0]["expr"]["class"]["name"], "x");
    }

    #[test]
    fn keeps_top_level_anonymous_classes_bare() {
        let json = generate_source(
            "<?php\nnew class { function __construct() {} }\nnew class { function __construct() {} }\n",
        )
        .expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(arr[0]["expr"]["class"]["name"]["name"], "anon-class-0");
        assert_eq!(arr[1]["expr"]["class"]["name"]["name"], "anon-class-1");
    }

    #[test]
    fn emits_trait_use_statements() {
        let json = generate_source("<?php\nclass Foo { use TraitA, TraitB; }\n").expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(arr[0]["stmts"][0]["nodeType"], "Stmt_TraitUse");
        assert_eq!(arr[0]["stmts"][0]["traits"][0]["parts"][0], "TraitA");
        assert_eq!(arr[0]["stmts"][0]["traits"][1]["parts"][0], "TraitB");
    }

    #[test]
    fn recovers_missing_chain_semicolon_before_return() {
        let json = generate_source(
            "<?php\nfunction f() { $queryBuilder\n  ->leftJoin()\n  ->setParameter()\nreturn $queryBuilder; }\n",
        )
        .expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(arr[0]["stmts"][0]["nodeType"], "Stmt_Expression");
        assert_eq!(arr[0]["stmts"][1]["nodeType"], "Stmt_Return");
    }

    #[test]
    fn maps_latin1_offsets_to_original_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let php = dir.path().join("latin1.php");
        let bytes = [
            b"<?php\n\n// ".as_slice(),
            &[0xe4, 0xe4, 0xfa],
            b"\n\nforeach ($arr as $key => $val) {};\n".as_slice(),
        ]
        .concat();
        fs::write(&php, bytes).expect("write php");

        let json = generate_file(&php).expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(arr[0]["nodeType"], "Stmt_Nop");
        assert_eq!(arr[1]["nodeType"], "Stmt_Foreach");
        assert_eq!(arr[1]["attributes"]["startFilePos"], 15);
        assert_eq!(arr[1]["attributes"]["endFilePos"], 47);
    }

    #[test]
    fn emits_nullsafe_method_call() {
        let json = generate_source("<?php\n$a?->b();\n").expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(arr[0]["expr"]["nodeType"], "Expr_NullsafeMethodCall");
        assert_eq!(arr[0]["expr"]["var"]["name"], "a");
        assert_eq!(arr[0]["expr"]["name"]["parts"][0], "b");
    }

    #[test]
    fn distinguishes_method_call_from_nullsafe() {
        let json = generate_source("<?php\n$a->b();\n").expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(arr[0]["expr"]["nodeType"], "Expr_MethodCall");
    }

    #[test]
    fn class_const_uses_class_const_node_type() {
        let json =
            generate_source("<?php\nconst TOP = 1;\nclass C { const INNER = 2; }\n").expect("json");
        let arr = json.as_array().expect("array");
        // Namespace/global const keeps Stmt_Const.
        assert_eq!(arr[0]["nodeType"], "Stmt_Const");
        assert_eq!(arr[0]["consts"][0]["name"]["name"], "TOP");
        // Class-level const becomes Stmt_ClassConst.
        assert_eq!(arr[1]["stmts"][0]["nodeType"], "Stmt_ClassConst");
        assert_eq!(arr[1]["stmts"][0]["consts"][0]["name"]["name"], "INNER");
    }

    #[test]
    fn interface_const_uses_class_const_node_type() {
        let json = generate_source("<?php\ninterface I { const C = 1; }\n").expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(arr[0]["stmts"][0]["nodeType"], "Stmt_ClassConst");
    }

    #[test]
    fn emits_magic_constants() {
        let json = generate_source(
            "<?php\necho __LINE__, __FILE__, __DIR__, __FUNCTION__, __CLASS__, __METHOD__, __NAMESPACE__, __TRAIT__;\n",
        )
        .expect("json");
        let arr = json.as_array().expect("array");
        let exprs = arr[0]["exprs"].as_array().expect("exprs");
        let kinds = exprs
            .iter()
            .map(|expr| expr["nodeType"].as_str().expect("nodeType"))
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                "Scalar_MagicConst_Line",
                "Scalar_MagicConst_File",
                "Scalar_MagicConst_Dir",
                "Scalar_MagicConst_Function",
                "Scalar_MagicConst_Class",
                "Scalar_MagicConst_Method",
                "Scalar_MagicConst_Namespace",
                "Scalar_MagicConst_Trait",
            ]
        );
    }

    #[test]
    fn emits_halt_compiler() {
        let json = generate_source("<?php\n__halt_compiler();\n").expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(arr[0]["nodeType"], "Stmt_HaltCompiler");
    }

    #[test]
    fn emits_trait_use_adaptations() {
        let json = generate_source(
            "<?php\nclass C { use A, B { A::foo insteadof B; B::bar as baz; foo as protected; } }\n",
        )
        .expect("json");
        let arr = json.as_array().expect("array");
        let trait_use = &arr[0]["stmts"][0];
        assert_eq!(trait_use["nodeType"], "Stmt_TraitUse");
        assert_eq!(trait_use["traits"][0]["parts"][0], "A");
        assert_eq!(trait_use["traits"][1]["parts"][0], "B");
        let adaptations = trait_use["adaptations"].as_array().expect("adaptations");
        assert_eq!(adaptations.len(), 3);
        // A::foo insteadof B
        assert_eq!(
            adaptations[0]["nodeType"],
            "Stmt_TraitUseAdaptation_Precedence"
        );
        assert_eq!(adaptations[0]["trait"]["parts"][0], "A");
        assert_eq!(adaptations[0]["method"]["parts"][0], "foo");
        assert_eq!(adaptations[0]["insteadof"][0]["parts"][0], "B");
        // B::bar as baz
        assert_eq!(adaptations[1]["nodeType"], "Stmt_TraitUseAdaptation_Alias");
        assert_eq!(adaptations[1]["trait"]["parts"][0], "B");
        assert_eq!(adaptations[1]["method"]["parts"][0], "bar");
        assert_eq!(adaptations[1]["newName"]["parts"][0], "baz");
        // foo as protected (no trait, modifier set)
        assert_eq!(adaptations[2]["nodeType"], "Stmt_TraitUseAdaptation_Alias");
        assert!(adaptations[2]["trait"].is_null());
        assert_eq!(adaptations[2]["method"]["parts"][0], "foo");
        // ModifierTypes.PROTECTED => bitmask 2
        assert_eq!(adaptations[2]["newModifier"], 2);
    }

    #[test]
    fn emits_interpolated_heredoc_as_encapsed() {
        let json = generate_source("<?php\n$y = <<<EOT\nhi $name end\nEOT;\n").expect("json");
        let arr = json.as_array().expect("array");
        let value = &arr[0]["expr"]["expr"];
        assert_eq!(value["nodeType"], "Scalar_Encapsed");
        let parts = value["parts"].as_array().expect("parts");
        assert!(parts
            .iter()
            .any(|part| part["nodeType"] == "Expr_Variable" && part["name"] == "name"));
    }

    #[test]
    fn plain_heredoc_stays_scalar_string() {
        let json = generate_source("<?php\n$y = <<<EOT\njust text\nEOT;\n").expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(arr[0]["expr"]["expr"]["nodeType"], "Scalar_String");
    }

    #[test]
    fn summary_reports_unmapped_kinds() {
        // A fully mapped parse must not emit a summary.
        let (_clean, clean_summary) =
            with_unmapped_summary(|| generate_source("<?php\n$x = 1;\n").expect("json"));
        assert!(clean_summary.is_none());

        // First-class callable syntax `strlen(...)` produces a tree-sitter
        // `variadic_placeholder` that the lowering passes do not map.
        let (_json, summary) =
            with_unmapped_summary(|| generate_source("<?php\n$f = strlen(...);\n").expect("json"));
        let summary = summary.expect("expected unmapped summary");
        assert_eq!(
            summary,
            "phpastgen: 1 unmapped node(s): variadic_placeholder(x1)"
        );
    }

    #[test]
    fn emits_attribute_groups() {
        let json = generate_source(
            "<?php\n#[Route(\"/api\")]\nclass Foo { #[Route(\"/edit\", name: \"hello\")] public function bar(#[SomeAttr] $pBar){} }\n",
        )
        .expect("json");
        let arr = json.as_array().expect("array");
        assert_eq!(
            arr[0]["attrGroups"][0]["attrs"][0]["name"]["parts"][0],
            "Route"
        );
        assert_eq!(
            arr[0]["stmts"][0]["attrGroups"][0]["attrs"][0]["name"]["parts"][0],
            "Route"
        );
        assert_eq!(
            arr[0]["stmts"][0]["params"][0]["attrGroups"][0]["attrs"][0]["name"]["parts"][0],
            "SomeAttr"
        );
    }
}
