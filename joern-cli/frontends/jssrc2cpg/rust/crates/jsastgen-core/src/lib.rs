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
        "template_string" => template_string_json(node, source),
        "true" => boolean_literal_json(node, true),
        "false" => boolean_literal_json(node, false),
        "null" => with_span("NullLiteral", node, json!({ "value": Value::Null })),
        "binary_expression" => binary_expression_json(node, source),
        "assignment_expression" => assignment_expression_json(node, source),
        "call_expression" => call_expression_json(node, source),
        "member_expression" => member_expression_json(node, source),
        "array" => array_expression_json(node, source),
        "object" => object_expression_json(node, source),
        "arrow_function" => arrow_function_json(node, source),
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

fn object_expression_json(node: Node, source: &str) -> Value {
    let properties = named_children(node)
        .filter_map(|child| object_property_json(child, source))
        .collect::<Vec<_>>();

    with_span(
        "ObjectExpression",
        node,
        json!({ "properties": properties }),
    )
}

fn object_property_json(node: Node, source: &str) -> Option<Value> {
    match node.kind() {
        "pair" => Some(object_pair_json(node, source)),
        "method_definition" => Some(object_method_json(node, source)),
        "spread_element" => Some(unary_argument_json("SpreadElement", node, source)),
        "shorthand_property_identifier" => Some(shorthand_object_property_json(node, source)),
        _ => None,
    }
}

fn object_pair_json(node: Node, source: &str) -> Value {
    let key_node = node.child_by_field_name("key").unwrap_or(node);
    let computed = key_node.kind() == "computed_property_name";
    let key = object_key_json(key_node, source);
    let value = field_json(node, "value", source).unwrap_or_else(|| noop_json(node));

    with_span(
        "ObjectProperty",
        node,
        json!({
            "key": key,
            "value": value,
            "computed": computed,
            "shorthand": false
        }),
    )
}

fn object_method_json(node: Node, source: &str) -> Value {
    let key_node = node.child_by_field_name("name").unwrap_or(node);
    let computed = key_node.kind() == "computed_property_name";
    let key = object_key_json(key_node, source);
    let params = params_json(node, source);
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| block_from_node(node));

    with_span(
        "ObjectMethod",
        node,
        json!({
            "kind": object_method_kind(node, source),
            "key": key,
            "params": params,
            "body": body,
            "computed": computed,
            "generator": has_keyword_child(node, source, "*"),
            "async": has_keyword_child(node, source, "async")
        }),
    )
}

fn object_method_kind(node: Node, source: &str) -> &'static str {
    if has_keyword_child(node, source, "get") {
        "get"
    } else if has_keyword_child(node, source, "set") {
        "set"
    } else {
        "method"
    }
}

fn shorthand_object_property_json(node: Node, source: &str) -> Value {
    let identifier = identifier_json(node, source);
    with_span(
        "ObjectProperty",
        node,
        json!({
            "key": identifier.clone(),
            "value": identifier,
            "computed": false,
            "shorthand": true
        }),
    )
}

fn object_key_json(node: Node, source: &str) -> Value {
    if node.kind() == "computed_property_name" {
        return node
            .named_child(0)
            .map(|child| expr_json(child, source))
            .unwrap_or_else(|| noop_json(node));
    }
    expr_json(node, source)
}

fn arrow_function_json(node: Node, source: &str) -> Value {
    let params = arrow_params_json(node, source);
    let body_node = node.child_by_field_name("body").unwrap_or(node);
    let expression = body_node.kind() != "statement_block";
    let body = if expression {
        expr_json(body_node, source)
    } else {
        stmt_json(body_node, source)
    };

    with_span(
        "ArrowFunctionExpression",
        node,
        json!({
            "id": Value::Null,
            "params": params,
            "body": body,
            "expression": expression,
            "generator": false,
            "async": has_keyword_child(node, source, "async")
        }),
    )
}

fn arrow_params_json(node: Node, source: &str) -> Vec<Value> {
    if let Some(params_node) = node.child_by_field_name("parameters") {
        return params_from_node(params_node, source);
    }
    node.child_by_field_name("parameter")
        .map(|param| vec![expr_json(param, source)])
        .unwrap_or_default()
}

fn params_json(node: Node, source: &str) -> Vec<Value> {
    node.child_by_field_name("parameters")
        .map(|params_node| params_from_node(params_node, source))
        .unwrap_or_default()
}

fn params_from_node(node: Node, source: &str) -> Vec<Value> {
    named_children(node)
        .filter(|child| !is_comment(*child))
        .map(|child| expr_json(child, source))
        .collect()
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
    let value = decode_js_string_literal(&raw);
    with_span(
        "StringLiteral",
        node,
        json!({ "value": value, "extra": { "raw": raw } }),
    )
}

fn template_string_json(node: Node, source: &str) -> Value {
    if named_children(node).any(|child| child.kind() == "template_substitution") {
        noop_json(node)
    } else {
        string_literal_json(node, source)
    }
}

fn decode_js_string_literal(raw: &str) -> String {
    let Some(quote) = raw.chars().next() else {
        return String::new();
    };
    if !matches!(quote, '"' | '\'' | '`') || !raw.ends_with(quote) || raw.len() < 2 {
        return raw.to_string();
    }
    let body = &raw[1..raw.len() - 1];
    decode_js_string_escapes(body)
}

fn decode_js_string_escapes(body: &str) -> String {
    let chars = body.chars().collect::<Vec<_>>();
    let mut decoded = String::with_capacity(body.len());
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        index += 1;
        if ch != '\\' {
            decoded.push(ch);
            continue;
        }

        if index >= chars.len() {
            decoded.push('\\');
            break;
        }

        let escaped = chars[index];
        index += 1;
        match escaped {
            '"' => decoded.push('"'),
            '\'' => decoded.push('\''),
            '`' => decoded.push('`'),
            '\\' => decoded.push('\\'),
            'b' => decoded.push('\u{0008}'),
            'f' => decoded.push('\u{000c}'),
            'n' => decoded.push('\n'),
            'r' => decoded.push('\r'),
            't' => decoded.push('\t'),
            'v' => decoded.push('\u{000b}'),
            '0' => decoded.push('\0'),
            '\n' => {}
            '\r' => {
                if index < chars.len() && chars[index] == '\n' {
                    index += 1;
                }
            }
            'x' if index + 2 <= chars.len()
                && chars[index..index + 2]
                    .iter()
                    .all(|c| c.is_ascii_hexdigit()) =>
            {
                if let Some(value) = decode_hex_escape(&chars[index..index + 2]) {
                    decoded.push(value);
                }
                index += 2;
            }
            'u' if index < chars.len() && chars[index] == '{' => {
                index += 1;
                let start = index;
                while index < chars.len() && chars[index] != '}' {
                    index += 1;
                }
                if index < chars.len() && chars[index] == '}' {
                    if let Some(value) = decode_hex_escape(&chars[start..index]) {
                        decoded.push(value);
                    }
                    index += 1;
                }
            }
            'u' if index + 4 <= chars.len()
                && chars[index..index + 4]
                    .iter()
                    .all(|c| c.is_ascii_hexdigit()) =>
            {
                if let Some(value) = decode_hex_escape(&chars[index..index + 4]) {
                    decoded.push(value);
                }
                index += 4;
            }
            other => decoded.push(other),
        }
    }
    decoded
}

fn decode_hex_escape(digits: &[char]) -> Option<char> {
    let value = digits
        .iter()
        .collect::<String>()
        .chars()
        .try_fold(0_u32, |acc, ch| {
            ch.to_digit(16)
                .map(|digit| acc.saturating_mul(16).saturating_add(digit))
        })?;
    char::from_u32(value)
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

fn has_keyword_child(node: Node, source: &str, keyword: &str) -> bool {
    for index in 0..node.child_count() {
        if let Some(child) = node.child(index) {
            if !child.is_named() && node_text(child, source) == keyword {
                return true;
            }
        }
    }
    false
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

    #[test]
    fn emits_object_literals_as_babel_object_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "const x = { key1: \"value\", key2: 2, [1 + 1]: value(), shorthand, ...rest };\n",
        )
        .expect("parse succeeds");

        let object = &json["ast"]["program"]["body"][0]["declarations"][0]["init"];
        assert_eq!(object["type"], "ObjectExpression");
        assert_eq!(object["properties"].as_array().unwrap().len(), 5);

        assert_eq!(object["properties"][0]["type"], "ObjectProperty");
        assert_eq!(object["properties"][0]["key"]["name"], "key1");
        assert_eq!(object["properties"][0]["value"]["type"], "StringLiteral");
        assert_eq!(object["properties"][0]["computed"], false);

        assert_eq!(object["properties"][2]["key"]["type"], "BinaryExpression");
        assert_eq!(object["properties"][2]["computed"], true);

        assert_eq!(object["properties"][3]["key"]["name"], "shorthand");
        assert_eq!(object["properties"][3]["value"]["name"], "shorthand");
        assert_eq!(object["properties"][3]["shorthand"], true);

        assert_eq!(object["properties"][4]["type"], "SpreadElement");
        assert_eq!(object["properties"][4]["argument"]["name"], "rest");
    }

    #[test]
    fn emits_object_methods_as_babel_object_methods() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "const x = { foo(arg) { return arg; }, [bar]() {} };\n",
        )
        .expect("parse succeeds");

        let object = &json["ast"]["program"]["body"][0]["declarations"][0]["init"];
        let plain = &object["properties"][0];
        assert_eq!(plain["type"], "ObjectMethod");
        assert_eq!(plain["kind"], "method");
        assert_eq!(plain["key"]["name"], "foo");
        assert_eq!(plain["params"][0]["name"], "arg");
        assert_eq!(plain["body"]["type"], "BlockStatement");
        assert_eq!(plain["computed"], false);

        let computed = &object["properties"][1];
        assert_eq!(computed["type"], "ObjectMethod");
        assert_eq!(computed["key"]["name"], "bar");
        assert_eq!(computed["computed"], true);
    }

    #[test]
    fn emits_arrow_functions_as_babel_arrow_function_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "const value = () => 42;\nconst id = x => { return x; };\n",
        )
        .expect("parse succeeds");

        let value = &json["ast"]["program"]["body"][0]["declarations"][0]["init"];
        assert_eq!(value["type"], "ArrowFunctionExpression");
        assert_eq!(value["id"], Value::Null);
        assert_eq!(value["params"].as_array().unwrap().len(), 0);
        assert_eq!(value["body"]["type"], "NumericLiteral");
        assert_eq!(value["expression"], true);

        let id = &json["ast"]["program"]["body"][1]["declarations"][0]["init"];
        assert_eq!(id["type"], "ArrowFunctionExpression");
        assert_eq!(id["params"][0]["name"], "x");
        assert_eq!(id["body"]["type"], "BlockStatement");
        assert_eq!(id["expression"], false);
    }

    #[test]
    fn decodes_string_literal_values_like_babel() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "let a = \"\\\"abc\";\nlet b = 'abc\\'';\nlet c = `abc\ndef\n`;\n",
        )
        .expect("parse succeeds");

        assert_eq!(
            json["ast"]["program"]["body"][0]["declarations"][0]["init"]["value"],
            "\"abc"
        );
        assert_eq!(
            json["ast"]["program"]["body"][1]["declarations"][0]["init"]["value"],
            "abc'"
        );
        assert_eq!(
            json["ast"]["program"]["body"][2]["declarations"][0]["init"]["value"],
            "abc\ndef\n"
        );
    }
}
