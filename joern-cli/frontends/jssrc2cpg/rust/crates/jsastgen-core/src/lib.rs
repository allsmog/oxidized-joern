use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::Path;
use tree_sitter::{Node, Parser, Tree};

pub fn parse_file(root: &Path, path: &Path) -> Result<Value> {
    let content =
        fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_source(root, path, &content)
}

pub fn parse_source(root: &Path, path: &Path, source: &str) -> Result<Value> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .context("initializing JavaScript parser")?;
    let tree = parser
        .parse(source, None)
        .with_context(|| format!("parsing {}", path.display()))?;

    if tree.root_node().has_error() {
        bail!("parser reported syntax errors");
    }

    Ok(file_json(root, path, source, &tree))
}

pub fn write_json(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

fn file_json(root: &Path, path: &Path, source: &str, tree: &Tree) -> Value {
    let relative_name = relative_name(root, path);
    let program = program_json(tree.root_node(), source);
    let ast = with_span(
        "File",
        tree.root_node(),
        json!({
            "program": program,
            "comments": [],
            "tokens": []
        }),
    );

    json!({
        "fullName": path.to_string_lossy(),
        "relativeName": relative_name,
        "ast": ast
    })
}

fn relative_name(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn program_json(root: Node, source: &str) -> Value {
    let body = named_children(root)
        .filter(|child| !is_comment(*child))
        .map(|child| stmt_json(child, source))
        .filter(|value| !value.is_null())
        .collect::<Vec<_>>();

    with_span(
        "Program",
        root,
        json!({
            "sourceType": "module",
            "interpreter": Value::Null,
            "directives": [],
            "body": body
        }),
    )
}

fn stmt_json(node: Node, source: &str) -> Value {
    match node.kind() {
        "lexical_declaration" | "variable_declaration" => variable_declaration_json(node, source),
        "function_declaration" => function_declaration_json(node, source),
        "statement_block" => block_statement_json(node, source),
        "return_statement" => return_statement_json(node, source),
        "expression_statement" => expression_statement_json(node, source),
        "empty_statement" => with_span("EmptyStatement", node, json!({})),
        _ => noop_json(node),
    }
}

fn expr_json(node: Node, source: &str) -> Value {
    match node.kind() {
        "identifier" | "property_identifier" => identifier_json(node, source),
        "number" => numeric_literal_json(node, source),
        "string" => string_literal_json(node, source),
        "true" => boolean_literal_json(node, true),
        "false" => boolean_literal_json(node, false),
        "null" => with_span("NullLiteral", node, json!({ "value": Value::Null })),
        "binary_expression" => binary_expression_json(node, source),
        "assignment_expression" => assignment_expression_json(node, source),
        "call_expression" => call_expression_json(node, source),
        "member_expression" => member_expression_json(node, source),
        "array" => array_expression_json(node, source),
        "rest_pattern" => unary_argument_json("RestElement", node, source),
        "spread_element" => unary_argument_json("SpreadElement", node, source),
        "parenthesized_expression" => node
            .named_child(0)
            .map(|child| expr_json(child, source))
            .unwrap_or_else(|| noop_json(node)),
        _ => noop_json(node),
    }
}

fn variable_declaration_json(node: Node, source: &str) -> Value {
    let kind = declaration_kind(node, source);
    let declarations = named_children(node)
        .filter(|child| child.kind() == "variable_declarator")
        .map(|child| variable_declarator_json(child, source))
        .collect::<Vec<_>>();

    with_span(
        "VariableDeclaration",
        node,
        json!({
            "kind": kind,
            "declarations": declarations
        }),
    )
}

fn variable_declarator_json(node: Node, source: &str) -> Value {
    let id = field_json(node, "name", source).unwrap_or_else(|| noop_json(node));
    let init = field_json(node, "value", source).unwrap_or(Value::Null);

    with_span(
        "VariableDeclarator",
        node,
        json!({
            "id": id,
            "init": init
        }),
    )
}

fn function_declaration_json(node: Node, source: &str) -> Value {
    let id = field_json(node, "name", source).unwrap_or(Value::Null);
    let params = node
        .child_by_field_name("parameters")
        .map(|params_node| {
            named_children(params_node)
                .filter(|child| child.kind() != "(" && child.kind() != ")")
                .map(|child| expr_json(child, source))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| block_from_node(node));

    with_span(
        "FunctionDeclaration",
        node,
        json!({
            "id": id,
            "params": params,
            "body": body,
            "generator": false,
            "async": false
        }),
    )
}

fn block_statement_json(node: Node, source: &str) -> Value {
    let body = named_children(node)
        .filter(|child| !is_comment(*child))
        .map(|child| stmt_json(child, source))
        .filter(|value| !value.is_null())
        .collect::<Vec<_>>();

    with_span(
        "BlockStatement",
        node,
        json!({ "body": body, "directives": [] }),
    )
}

fn block_from_node(node: Node) -> Value {
    with_span(
        "BlockStatement",
        node,
        json!({ "body": [], "directives": [] }),
    )
}

fn return_statement_json(node: Node, source: &str) -> Value {
    let argument = node
        .child_by_field_name("argument")
        .map(|child| expr_json(child, source))
        .unwrap_or(Value::Null);

    with_span("ReturnStatement", node, json!({ "argument": argument }))
}

fn expression_statement_json(node: Node, source: &str) -> Value {
    let expression = node
        .named_child(0)
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "ExpressionStatement",
        node,
        json!({ "expression": expression }),
    )
}

fn binary_expression_json(node: Node, source: &str) -> Value {
    let left = field_json(node, "left", source).unwrap_or_else(|| noop_json(node));
    let right = field_json(node, "right", source).unwrap_or_else(|| noop_json(node));
    let operator = node
        .child_by_field_name("operator")
        .map(|child| node_text(child, source))
        .unwrap_or_else(|| infer_operator(node, source));

    with_span(
        "BinaryExpression",
        node,
        json!({
            "left": left,
            "operator": operator,
            "right": right
        }),
    )
}

fn assignment_expression_json(node: Node, source: &str) -> Value {
    let left = field_json(node, "left", source).unwrap_or_else(|| noop_json(node));
    let right = field_json(node, "right", source).unwrap_or_else(|| noop_json(node));
    let operator = node
        .child_by_field_name("operator")
        .map(|child| node_text(child, source))
        .unwrap_or_else(|| "=".to_string());

    with_span(
        "AssignmentExpression",
        node,
        json!({
            "left": left,
            "operator": operator,
            "right": right
        }),
    )
}

fn call_expression_json(node: Node, source: &str) -> Value {
    let callee = node
        .child_by_field_name("function")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let arguments = node
        .child_by_field_name("arguments")
        .map(|args_node| {
            named_children(args_node)
                .map(|child| expr_json(child, source))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    with_span(
        "CallExpression",
        node,
        json!({
            "callee": callee,
            "arguments": arguments,
            "optional": false
        }),
    )
}

fn member_expression_json(node: Node, source: &str) -> Value {
    let object = field_json(node, "object", source).unwrap_or_else(|| noop_json(node));
    let property = field_json(node, "property", source).unwrap_or_else(|| noop_json(node));

    with_span(
        "MemberExpression",
        node,
        json!({
            "object": object,
            "property": property,
            "computed": false,
            "optional": false
        }),
    )
}

fn array_expression_json(node: Node, source: &str) -> Value {
    let elements = named_children(node)
        .filter(|child| !is_comment(*child))
        .map(|child| expr_json(child, source))
        .collect::<Vec<_>>();

    with_span("ArrayExpression", node, json!({ "elements": elements }))
}

fn unary_argument_json(kind: &str, node: Node, source: &str) -> Value {
    let argument = node
        .child_by_field_name("argument")
        .or_else(|| node.named_child(0))
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let fields = if kind == "RestElement" {
        json!({
            "argument": argument,
            "typeAnnotation": array_type_annotation_json(node)
        })
    } else {
        json!({ "argument": argument })
    };
    with_span(kind, node, fields)
}

fn array_type_annotation_json(node: Node) -> Value {
    with_span(
        "TSTypeAnnotation",
        node,
        json!({
            "typeAnnotation": with_span(
                "TSArrayType",
                node,
                json!({
                    "elementType": with_span("TSAnyKeyword", node, json!({}))
                })
            )
        }),
    )
}

fn identifier_json(node: Node, source: &str) -> Value {
    with_span(
        "Identifier",
        node,
        json!({ "name": node_text(node, source) }),
    )
}

fn numeric_literal_json(node: Node, source: &str) -> Value {
    let raw = node_text(node, source);
    let value = raw.parse::<f64>().ok().map_or(Value::Null, Value::from);
    with_span(
        "NumericLiteral",
        node,
        json!({ "value": value, "extra": { "raw": raw } }),
    )
}

fn string_literal_json(node: Node, source: &str) -> Value {
    let raw = node_text(node, source);
    let value = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| raw.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(&raw)
        .to_string();
    with_span(
        "StringLiteral",
        node,
        json!({ "value": value, "extra": { "raw": raw } }),
    )
}

fn boolean_literal_json(node: Node, value: bool) -> Value {
    with_span("BooleanLiteral", node, json!({ "value": value }))
}

fn field_json(node: Node, field: &str, source: &str) -> Option<Value> {
    node.child_by_field_name(field)
        .map(|child| expr_json(child, source))
}

fn declaration_kind(node: Node, source: &str) -> String {
    for index in 0..node.child_count() {
        if let Some(child) = node.child(index) {
            let text = node_text(child, source);
            if matches!(text.as_str(), "let" | "const" | "var") {
                return text;
            }
        }
    }
    "var".to_string()
}

fn with_span(kind: &str, node: Node, fields: Value) -> Value {
    let mut object = match fields {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    object.insert("type".into(), Value::String(kind.into()));
    object.insert("start".into(), Value::from(node.start_byte()));
    object.insert("end".into(), Value::from(node.end_byte()));
    object.insert(
        "loc".into(),
        json!({
            "start": {
                "line": node.start_position().row + 1,
                "column": node.start_position().column
            },
            "end": {
                "line": node.end_position().row + 1,
                "column": node.end_position().column
            }
        }),
    );
    Value::Object(object)
}

fn noop_json(node: Node) -> Value {
    with_span("Noop", node, json!({}))
}

fn node_text(node: Node, source: &str) -> String {
    node.utf8_text(source.as_bytes()).unwrap_or("").to_string()
}

fn named_children(node: Node) -> impl Iterator<Item = Node> {
    (0..node.named_child_count()).filter_map(move |index| node.named_child(index))
}

fn is_comment(node: Node) -> bool {
    matches!(node.kind(), "comment" | "hash_bang_line")
}

fn infer_operator(node: Node, source: &str) -> String {
    for index in 0..node.child_count() {
        if let Some(child) = node.child(index) {
            if !child.is_named() {
                let text = node_text(child, source);
                if !text.trim().is_empty() {
                    return text;
                }
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_babel_shaped_program_for_core_javascript() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "const answer = 40 + 2;\nfunction id(x) { return x; }\nid(answer);\n",
        )
        .expect("parse succeeds");

        assert_eq!(json["relativeName"], "app.js");
        assert_eq!(json["ast"]["type"], "File");
        assert_eq!(json["ast"]["program"]["type"], "Program");
        assert_eq!(
            json["ast"]["program"]["body"][0]["type"],
            "VariableDeclaration"
        );
        assert_eq!(
            json["ast"]["program"]["body"][0]["declarations"][0]["id"]["name"],
            "answer"
        );
        assert_eq!(
            json["ast"]["program"]["body"][1]["type"],
            "FunctionDeclaration"
        );
        assert_eq!(
            json["ast"]["program"]["body"][2]["type"],
            "ExpressionStatement"
        );
    }

    #[test]
    fn emits_rest_parameters_as_babel_rest_elements() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json =
            parse_source(root, path, "function method(x, ...args) {}\n").expect("parse succeeds");

        let params = &json["ast"]["program"]["body"][0]["params"];
        assert_eq!(params[0]["type"], "Identifier");
        assert_eq!(params[1]["type"], "RestElement");
        assert_eq!(params[1]["argument"]["name"], "args");
        assert_eq!(
            params[1]["typeAnnotation"]["typeAnnotation"]["type"],
            "TSArrayType"
        );
    }

    #[test]
    fn emits_array_literals_as_babel_array_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "const empty = [];\nconst values = [1, two, ...rest];\n",
        )
        .expect("parse succeeds");

        let empty = &json["ast"]["program"]["body"][0]["declarations"][0]["init"];
        assert_eq!(empty["type"], "ArrayExpression");
        assert_eq!(empty["elements"].as_array().unwrap().len(), 0);

        let values = &json["ast"]["program"]["body"][1]["declarations"][0]["init"];
        assert_eq!(values["type"], "ArrayExpression");
        assert_eq!(values["elements"][0]["type"], "NumericLiteral");
        assert_eq!(values["elements"][1]["type"], "Identifier");
        assert_eq!(values["elements"][1]["name"], "two");
        assert_eq!(values["elements"][2]["type"], "SpreadElement");
        assert_eq!(values["elements"][2]["argument"]["name"], "rest");
    }
}
