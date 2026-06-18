use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::Path;
use tree_sitter::{Language, Node, Parser, Point, Tree};

pub fn parse_file(root: &Path, path: &Path) -> Result<Value> {
    let content =
        fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_source(root, path, &content)
}

pub fn parse_source(root: &Path, path: &Path, source: &str) -> Result<Value> {
    let tree = parse_tree(path, source)?;
    Ok(file_json(root, path, source, &tree))
}

fn parse_tree(path: &Path, source: &str) -> Result<Tree> {
    let mut last_error = None;
    for language in language_candidates(path) {
        let tree = parse_with_language(source, language)
            .with_context(|| format!("parsing {} as {}", path.display(), language.name()))?;
        if !tree.root_node().has_error() {
            return Ok(tree);
        }
        last_error = Some(language.name());
    }

    if let Some(language) = last_error {
        bail!("parser reported syntax errors after trying {language}");
    }
    bail!("parser reported syntax errors");
}

fn parse_with_language(source: &str, language: SourceLanguage) -> Result<Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&language.language())
        .with_context(|| format!("initializing {} parser", language.name()))?;
    parser
        .parse(source, None)
        .context("parser returned no tree")
}

#[derive(Clone, Copy)]
enum SourceLanguage {
    JavaScript,
    TypeScript,
    Tsx,
}

impl SourceLanguage {
    fn language(self) -> Language {
        match self {
            SourceLanguage::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            SourceLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            SourceLanguage::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }

    fn name(self) -> &'static str {
        match self {
            SourceLanguage::JavaScript => "JavaScript",
            SourceLanguage::TypeScript => "TypeScript",
            SourceLanguage::Tsx => "TSX",
        }
    }
}

fn language_candidates(path: &Path) -> Vec<SourceLanguage> {
    match path.extension().and_then(|x| x.to_str()) {
        Some("ts") => vec![SourceLanguage::TypeScript, SourceLanguage::JavaScript],
        Some("tsx") => vec![
            SourceLanguage::Tsx,
            SourceLanguage::TypeScript,
            SourceLanguage::JavaScript,
        ],
        Some("jsx") => vec![
            SourceLanguage::JavaScript,
            SourceLanguage::Tsx,
            SourceLanguage::TypeScript,
        ],
        _ => vec![
            SourceLanguage::JavaScript,
            SourceLanguage::TypeScript,
            SourceLanguage::Tsx,
        ],
    }
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
        "class_declaration" => class_declaration_json(node, source),
        "ambient_declaration" => ambient_declaration_json(node, source),
        "import_statement" => import_statement_json(node, source),
        "export_statement" => export_statement_json(node, source),
        "internal_module" | "module" => ts_module_declaration_json(node, source),
        "statement_block" => block_statement_json(node, source),
        "return_statement" => return_statement_json(node, source),
        "if_statement" => if_statement_json(node, source),
        "with_statement" => with_statement_json(node, source),
        "while_statement" => while_statement_json(node, source),
        "do_statement" => do_while_statement_json(node, source),
        "for_statement" => for_statement_json(node, source),
        "for_in_statement" => for_in_of_statement_json(node, source),
        "switch_statement" => switch_statement_json(node, source),
        "labeled_statement" => labeled_statement_json(node, source),
        "break_statement" => jump_statement_json("BreakStatement", node, source),
        "continue_statement" => jump_statement_json("ContinueStatement", node, source),
        "try_statement" => try_statement_json(node, source),
        "throw_statement" => throw_statement_json(node, source),
        "expression_statement" => expression_statement_json(node, source),
        "empty_statement" => with_span("EmptyStatement", node, json!({})),
        _ => noop_json(node),
    }
}

fn expr_json(node: Node, source: &str) -> Value {
    match node.kind() {
        "identifier"
        | "type_identifier"
        | "property_identifier"
        | "private_property_identifier"
        | "statement_identifier"
        | "shorthand_property_identifier_pattern" => identifier_json(node, source),
        "number" => numeric_literal_json(node, source),
        "string" => string_literal_json(node, source),
        "template_string" => template_string_json(node, source),
        "true" => boolean_literal_json(node, true),
        "false" => boolean_literal_json(node, false),
        "null" => with_span("NullLiteral", node, json!({ "value": Value::Null })),
        "this" => with_span("ThisExpression", node, json!({})),
        "binary_expression" => binary_expression_json(node, source),
        "unary_expression" => unary_expression_json(node, source),
        "await_expression" => await_expression_json(node, source),
        "as_expression" => ts_as_expression_json(node, source),
        "type_assertion" => ts_type_assertion_json(node, source),
        "satisfies_expression" => ts_satisfies_expression_json(node, source),
        "assignment_expression" | "augmented_assignment_expression" => {
            assignment_expression_json(node, source)
        }
        "update_expression" => update_expression_json(node, source),
        "ternary_expression" => conditional_expression_json(node, source),
        "call_expression" => call_expression_json(node, source),
        "new_expression" => new_expression_json(node, source),
        "member_expression" => member_expression_json(node, source),
        "subscript_expression" => subscript_expression_json(node, source),
        "array" => array_expression_json(node, source),
        "object" => object_expression_json(node, source),
        "array_pattern" => array_pattern_json(node, source),
        "object_pattern" => object_pattern_json(node, source),
        "assignment_pattern" => assignment_pattern_json(node, source),
        "function_expression" => function_expression_json(node, source),
        "function_declaration" => function_declaration_json(node, source),
        "arrow_function" => arrow_function_json(node, source),
        "class" => class_expression_json(node, source),
        "non_null_expression" => ts_non_null_expression_json(node, source),
        "required_parameter" | "optional_parameter" => parameter_json(node, source),
        "sequence_expression" => sequence_expression_json(node, source),
        "rest_pattern" => unary_argument_json("RestElement", node, source),
        "spread_element" => unary_argument_json("SpreadElement", node, source),
        "parenthesized_expression" => node
            .named_child(0)
            .map(|child| expr_json(child, source))
            .unwrap_or_else(|| noop_json(node)),
        "predefined_type" | "type_annotation" => ts_type_json(node, source),
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

fn ambient_declaration_json(node: Node, source: &str) -> Value {
    match node.named_child(0).map(|child| child.kind()) {
        Some("function_declaration" | "function_signature") => {
            let function = node.named_child(0).unwrap();
            function_like_json_with_span("TSDeclareFunction", node, function, source)
        }
        Some(_) => node
            .named_child(0)
            .map(|child| stmt_json(child, source))
            .unwrap_or_else(|| noop_json(node)),
        None => noop_json(node),
    }
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
    function_like_json("FunctionDeclaration", node, source)
}

fn function_expression_json(node: Node, source: &str) -> Value {
    function_like_json("FunctionExpression", node, source)
}

fn class_declaration_json(node: Node, source: &str) -> Value {
    class_like_json("ClassDeclaration", node, source)
}

fn class_expression_json(node: Node, source: &str) -> Value {
    class_like_json("ClassExpression", node, source)
}

fn class_like_json(kind: &str, node: Node, source: &str) -> Value {
    let id = node
        .child_by_field_name("name")
        .map(|child| identifier_json(child, source))
        .unwrap_or(Value::Null);
    let body = node
        .child_by_field_name("body")
        .map(|child| class_body_json(child, source))
        .unwrap_or_else(|| with_span("ClassBody", node, json!({ "body": [] })));
    let super_class = node
        .child_by_field_name("superclass")
        .map(|child| expr_json(child, source))
        .unwrap_or(Value::Null);

    with_span(
        kind,
        node,
        json!({
            "id": id,
            "superClass": super_class,
            "body": body,
            "decorators": [],
            "implements": [],
            "mixins": []
        }),
    )
}

fn class_body_json(node: Node, source: &str) -> Value {
    let body = named_children(node)
        .filter_map(|child| class_member_json(child, source))
        .collect::<Vec<_>>();

    with_span("ClassBody", node, json!({ "body": body }))
}

fn class_member_json(node: Node, source: &str) -> Option<Value> {
    match node.kind() {
        "method_definition" => Some(class_method_json(node, source)),
        _ => None,
    }
}

fn class_method_json(node: Node, source: &str) -> Value {
    let key_node = node.child_by_field_name("name").unwrap_or(node);
    let computed = key_node.kind() == "computed_property_name";
    let key = object_key_json(key_node, source);
    let params = params_json(node, source);
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| block_from_node(node));

    with_span(
        "ClassMethod",
        node,
        json!({
            "kind": object_method_kind(node, source),
            "key": key,
            "id": Value::Null,
            "params": params,
            "body": body,
            "computed": computed,
            "static": has_keyword_child(node, source, "static"),
            "generator": has_keyword_child(node, source, "*"),
            "async": has_keyword_child(node, source, "async")
        }),
    )
}

fn function_like_json(kind: &str, node: Node, source: &str) -> Value {
    function_like_json_with_span(kind, node, node, source)
}

fn function_like_json_with_span(
    kind: &str,
    span_node: Node,
    function_node: Node,
    source: &str,
) -> Value {
    let id = field_json(function_node, "name", source).unwrap_or(Value::Null);
    let params = function_node
        .child_by_field_name("parameters")
        .map(|params_node| {
            named_children(params_node)
                .filter(|child| child.kind() != "(" && child.kind() != ")")
                .map(|child| expr_json(child, source))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let body = function_node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| block_from_node(function_node));
    let return_type = function_node
        .child_by_field_name("return_type")
        .map(|child| ts_type_annotation_json(child, source));

    let mut fields = json!({
            "id": id,
            "params": params,
            "body": body,
            "generator": false,
            "async": false
    });
    if let Some(return_type) = return_type {
        fields = with_extra_field(fields, "returnType", return_type);
    }

    with_span(kind, span_node, fields)
}

fn ts_module_declaration_json(node: Node, source: &str) -> Value {
    let id = node
        .child_by_field_name("name")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let body = node
        .child_by_field_name("body")
        .map(|child| ts_module_block_json(child, source))
        .unwrap_or(Value::Null);

    with_span(
        "TSModuleDeclaration",
        node,
        json!({
            "id": id,
            "body": body,
            "declare": has_keyword_child(node, source, "declare")
        }),
    )
}

fn ts_module_block_json(node: Node, source: &str) -> Value {
    let body = named_children(node)
        .filter(|child| !is_comment(*child))
        .map(|child| stmt_json(child, source))
        .filter(|value| !value.is_null())
        .collect::<Vec<_>>();

    with_span("TSModuleBlock", node, json!({ "body": body }))
}

fn import_statement_json(node: Node, source: &str) -> Value {
    if let Some(require_clause) =
        named_children(node).find(|child| child.kind() == "import_require_clause")
    {
        return ts_import_equals_declaration_json(node, require_clause, source);
    }

    let source_node = node.child_by_field_name("source");
    let source_value = source_node
        .map(|child| string_literal_json(child, source))
        .unwrap_or(Value::Null);
    let specifiers = named_children(node)
        .find(|child| child.kind() == "import_clause")
        .map(|child| import_specifiers_json(child, source))
        .unwrap_or_default();

    with_span(
        "ImportDeclaration",
        node,
        json!({
            "source": source_value,
            "specifiers": specifiers
        }),
    )
}

fn ts_import_equals_declaration_json(node: Node, require_clause: Node, source: &str) -> Value {
    let id = require_clause
        .named_child(0)
        .map(|child| identifier_json(child, source))
        .unwrap_or_else(|| noop_json(require_clause));
    let expression = require_clause
        .child_by_field_name("source")
        .map(|child| string_literal_json(child, source))
        .unwrap_or(Value::Null);

    with_span(
        "TSImportEqualsDeclaration",
        node,
        json!({
            "id": id,
            "moduleReference": with_span(
                "TSExternalModuleReference",
                require_clause,
                json!({ "expression": expression })
            )
        }),
    )
}

fn import_specifiers_json(node: Node, source: &str) -> Vec<Value> {
    let mut specifiers = Vec::new();
    for child in named_children(node) {
        match child.kind() {
            "identifier" => specifiers.push(with_span(
                "ImportDefaultSpecifier",
                child,
                json!({ "local": identifier_json(child, source) }),
            )),
            "named_imports" => specifiers.extend(
                named_children(child)
                    .filter(|specifier| specifier.kind() == "import_specifier")
                    .map(|specifier| import_specifier_json(specifier, source)),
            ),
            "namespace_import" => {
                let local = child
                    .named_child(0)
                    .map(|identifier| identifier_json(identifier, source))
                    .unwrap_or_else(|| noop_json(child));
                specifiers.push(with_span(
                    "ImportNamespaceSpecifier",
                    child,
                    json!({ "local": local }),
                ));
            }
            _ => {}
        }
    }
    specifiers
}

fn import_specifier_json(node: Node, source: &str) -> Value {
    let imported_node = node.child_by_field_name("name").unwrap_or(node);
    let local_node = node.child_by_field_name("alias").unwrap_or(imported_node);
    with_span(
        "ImportSpecifier",
        node,
        json!({
            "imported": import_export_name_json(imported_node, source),
            "local": import_export_name_json(local_node, source)
        }),
    )
}

fn export_statement_json(node: Node, source: &str) -> Value {
    let source_value = node
        .child_by_field_name("source")
        .map(|child| string_literal_json(child, source))
        .unwrap_or(Value::Null);
    let specifiers = named_children(node)
        .find(|child| child.kind() == "export_clause")
        .map(|child| export_specifiers_json(child, source))
        .unwrap_or_default();

    if has_keyword_child(node, source, "default") {
        let declaration = node
            .child_by_field_name("declaration")
            .map(|child| stmt_json(child, source))
            .or_else(|| {
                node.child_by_field_name("value")
                    .map(|child| expr_json(child, source))
            })
            .or_else(|| {
                named_children(node)
                    .find(|child| child.kind() != "export_clause")
                    .map(|child| expr_json(child, source))
            })
            .unwrap_or(Value::Null);
        return with_span(
            "ExportDefaultDeclaration",
            node,
            json!({
                "declaration": declaration
            }),
        );
    }

    let declaration = node
        .child_by_field_name("declaration")
        .map(|child| stmt_json(child, source))
        .unwrap_or(Value::Null);

    with_span(
        "ExportNamedDeclaration",
        node,
        json!({
            "declaration": declaration,
            "specifiers": specifiers,
            "source": source_value
        }),
    )
}

fn export_specifiers_json(node: Node, source: &str) -> Vec<Value> {
    named_children(node)
        .filter(|child| child.kind() == "export_specifier")
        .map(|child| export_specifier_json(child, source))
        .collect()
}

fn export_specifier_json(node: Node, source: &str) -> Value {
    let local_node = node.child_by_field_name("name").unwrap_or(node);
    let exported_node = node.child_by_field_name("alias").unwrap_or(local_node);
    with_span(
        "ExportSpecifier",
        node,
        json!({
            "local": import_export_name_json(local_node, source),
            "exported": import_export_name_json(exported_node, source)
        }),
    )
}

fn import_export_name_json(node: Node, source: &str) -> Value {
    match node.kind() {
        "string" => string_literal_json(node, source),
        _ => identifier_json(node, source),
    }
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
        .or_else(|| node.named_child(0))
        .map(|child| expr_json(child, source))
        .unwrap_or(Value::Null);

    with_span("ReturnStatement", node, json!({ "argument": argument }))
}

fn if_statement_json(node: Node, source: &str) -> Value {
    let test = node
        .child_by_field_name("condition")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let consequent = node
        .child_by_field_name("consequence")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let alternate = node
        .child_by_field_name("alternative")
        .and_then(first_named_child)
        .map(|child| stmt_json(child, source))
        .unwrap_or(Value::Null);

    with_span(
        "IfStatement",
        node,
        json!({
            "test": test,
            "consequent": consequent,
            "alternate": alternate
        }),
    )
}

fn with_statement_json(node: Node, source: &str) -> Value {
    let object = node
        .child_by_field_name("object")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "WithStatement",
        node,
        json!({
            "object": object,
            "body": body
        }),
    )
}

fn while_statement_json(node: Node, source: &str) -> Value {
    let test = node
        .child_by_field_name("condition")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "WhileStatement",
        node,
        json!({
            "test": test,
            "body": body
        }),
    )
}

fn do_while_statement_json(node: Node, source: &str) -> Value {
    let test = node
        .child_by_field_name("condition")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "DoWhileStatement",
        node,
        json!({
            "test": test,
            "body": body
        }),
    )
}

fn for_statement_json(node: Node, source: &str) -> Value {
    let init = node
        .child_by_field_name("initializer")
        .and_then(|child| non_empty_stmt_or_expr_json(child, source))
        .unwrap_or(Value::Null);
    let test = node
        .child_by_field_name("condition")
        .and_then(|child| non_empty_stmt_or_expr_json(child, source))
        .unwrap_or(Value::Null);
    let update = node
        .child_by_field_name("increment")
        .and_then(|child| non_empty_stmt_or_expr_json(child, source))
        .unwrap_or(Value::Null);
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "ForStatement",
        node,
        json!({
            "init": init,
            "test": test,
            "update": update,
            "body": body
        }),
    )
}

fn for_in_of_statement_json(node: Node, source: &str) -> Value {
    let operator = node
        .child_by_field_name("operator")
        .map(|child| node_text(child, source))
        .unwrap_or_default();
    let kind = if operator == "of" {
        "ForOfStatement"
    } else {
        "ForInStatement"
    };
    let left_node = node.child_by_field_name("left");
    let left = left_node
        .map(|child| for_in_of_left_json(node, child, source))
        .unwrap_or_else(|| noop_json(node));
    let right = node
        .child_by_field_name("right")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        kind,
        node,
        json!({
            "left": left,
            "right": right,
            "body": body,
            "await": has_keyword_child(node, source, "await")
        }),
    )
}

fn for_in_of_left_json(for_node: Node, left_node: Node, source: &str) -> Value {
    let id = pattern_or_expr_json(left_node, source);
    if let Some(kind) = declaration_kind_in_for_in_of(for_node, source) {
        let declarator = with_span(
            "VariableDeclarator",
            left_node,
            json!({
                "id": id,
                "init": Value::Null
            }),
        );
        with_span(
            "VariableDeclaration",
            left_node,
            json!({
                "kind": kind,
                "declarations": [declarator]
            }),
        )
    } else {
        id
    }
}

fn switch_statement_json(node: Node, source: &str) -> Value {
    let discriminant = node
        .child_by_field_name("value")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let cases = node
        .child_by_field_name("body")
        .map(|body| {
            named_children(body)
                .filter_map(|child| switch_case_json(child, source))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    with_span(
        "SwitchStatement",
        node,
        json!({
            "discriminant": discriminant,
            "cases": cases
        }),
    )
}

fn switch_case_json(node: Node, source: &str) -> Option<Value> {
    let test_node = node.child_by_field_name("value");
    let test = test_node
        .map(|child| expr_json(child, source))
        .unwrap_or(Value::Null);
    let consequent = named_children(node)
        .filter(|child| {
            test_node.is_none_or(|test| {
                child.kind() != test.kind()
                    || child.start_byte() != test.start_byte()
                    || child.end_byte() != test.end_byte()
            })
        })
        .map(|child| stmt_json(child, source))
        .filter(|value| !value.is_null())
        .collect::<Vec<_>>();
    let colon = colon_child(node, source).unwrap_or(node);

    match node.kind() {
        "switch_case" | "switch_default" => Some(with_span_bounds(
            "SwitchCase",
            node.start_byte(),
            node.start_position(),
            colon.end_byte(),
            colon.end_position(),
            json!({
                "test": test,
                "consequent": consequent
            }),
        )),
        _ => None,
    }
}

fn labeled_statement_json(node: Node, source: &str) -> Value {
    let label = node
        .child_by_field_name("label")
        .map(|child| identifier_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "LabeledStatement",
        node,
        json!({
            "label": label,
            "body": body
        }),
    )
}

fn non_empty_stmt_or_expr_json(node: Node, source: &str) -> Option<Value> {
    if node.kind() == "empty_statement" {
        None
    } else if matches!(
        node.kind(),
        "lexical_declaration" | "variable_declaration" | "function_declaration"
    ) {
        Some(stmt_json(node, source))
    } else {
        Some(expr_json(node, source))
    }
}

fn jump_statement_json(kind: &str, node: Node, source: &str) -> Value {
    let label = node
        .child_by_field_name("label")
        .map(|child| expr_json(child, source))
        .unwrap_or(Value::Null);
    with_span(kind, node, json!({ "label": label }))
}

fn try_statement_json(node: Node, source: &str) -> Value {
    let block = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| block_from_node(node));
    let handler = node
        .child_by_field_name("handler")
        .map(|child| catch_clause_json(child, source))
        .unwrap_or(Value::Null);
    let finalizer = node
        .child_by_field_name("finalizer")
        .and_then(|finally_clause| finally_clause.child_by_field_name("body"))
        .map(|child| stmt_json(child, source))
        .unwrap_or(Value::Null);

    with_span(
        "TryStatement",
        node,
        json!({
            "block": block,
            "handler": handler,
            "finalizer": finalizer
        }),
    )
}

fn catch_clause_json(node: Node, source: &str) -> Value {
    let param = node
        .child_by_field_name("parameter")
        .map(|child| expr_json(child, source))
        .unwrap_or(Value::Null);
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| block_from_node(node));

    with_span(
        "CatchClause",
        node,
        json!({
            "param": param,
            "body": body
        }),
    )
}

fn throw_statement_json(node: Node, source: &str) -> Value {
    let argument = node
        .named_child(0)
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    with_span("ThrowStatement", node, json!({ "argument": argument }))
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

fn update_expression_json(node: Node, source: &str) -> Value {
    let argument = node
        .child_by_field_name("argument")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let operator = node
        .child_by_field_name("operator")
        .map(|child| node_text(child, source))
        .unwrap_or_else(|| infer_operator(node, source));
    let prefix = node
        .child(0)
        .map(|child| !child.is_named() && node_text(child, source) == operator)
        .unwrap_or(false);

    with_span(
        "UpdateExpression",
        node,
        json!({
            "argument": argument,
            "operator": operator,
            "prefix": prefix
        }),
    )
}

fn conditional_expression_json(node: Node, source: &str) -> Value {
    let test = field_json(node, "condition", source).unwrap_or_else(|| noop_json(node));
    let consequent = field_json(node, "consequence", source).unwrap_or_else(|| noop_json(node));
    let alternate = field_json(node, "alternative", source).unwrap_or_else(|| noop_json(node));

    with_span(
        "ConditionalExpression",
        node,
        json!({
            "test": test,
            "consequent": consequent,
            "alternate": alternate
        }),
    )
}

fn unary_expression_json(node: Node, source: &str) -> Value {
    let argument = node
        .child_by_field_name("argument")
        .or_else(|| node.named_child(0))
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let operator = node
        .child_by_field_name("operator")
        .map(|child| node_text(child, source))
        .unwrap_or_else(|| infer_operator(node, source));

    with_span(
        "UnaryExpression",
        node,
        json!({
            "operator": operator,
            "argument": argument,
            "prefix": true
        }),
    )
}

fn await_expression_json(node: Node, source: &str) -> Value {
    let argument = node
        .named_child(0)
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span("AwaitExpression", node, json!({ "argument": argument }))
}

fn ts_as_expression_json(node: Node, source: &str) -> Value {
    let expression = first_expression_child(node)
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let type_annotation = last_type_child(node)
        .map(|child| ts_type_json(child, source))
        .unwrap_or_else(|| with_span("TSAnyKeyword", node, json!({})));

    with_span(
        "TSAsExpression",
        node,
        json!({
            "expression": expression,
            "typeAnnotation": type_annotation
        }),
    )
}

fn ts_type_assertion_json(node: Node, source: &str) -> Value {
    let expression = first_expression_child(node)
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let type_annotation = last_type_child(node)
        .map(|child| ts_type_json(child, source))
        .unwrap_or_else(|| with_span("TSAnyKeyword", node, json!({})));

    with_span(
        "TSTypeAssertion",
        node,
        json!({
            "expression": expression,
            "typeAnnotation": type_annotation
        }),
    )
}

fn ts_satisfies_expression_json(node: Node, source: &str) -> Value {
    let expression = first_expression_child(node)
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let type_annotation = last_type_child(node)
        .map(|child| ts_type_json(child, source))
        .unwrap_or_else(|| with_span("TSAnyKeyword", node, json!({})));

    with_span(
        "TSSatisfiesExpression",
        node,
        json!({
            "expression": expression,
            "typeAnnotation": type_annotation
        }),
    )
}

fn ts_non_null_expression_json(node: Node, source: &str) -> Value {
    let expression = node
        .named_child(0)
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "TSNonNullExpression",
        node,
        json!({ "expression": expression }),
    )
}

fn sequence_expression_json(node: Node, source: &str) -> Value {
    let expressions = named_children(node)
        .map(|child| expr_json(child, source))
        .collect::<Vec<_>>();

    with_span(
        "SequenceExpression",
        node,
        json!({ "expressions": expressions }),
    )
}

fn parameter_json(node: Node, source: &str) -> Value {
    let left_node = node
        .child_by_field_name("pattern")
        .or_else(|| node.child_by_field_name("name"))
        .or_else(|| node.named_child(0));
    let type_annotation = node
        .child_by_field_name("type")
        .map(|child| ts_type_annotation_json(child, source));
    let left = match left_node {
        Some(child)
            if matches!(
                child.kind(),
                "identifier" | "property_identifier" | "type_identifier"
            ) =>
        {
            identifier_json_with_span(child, node, source, type_annotation.clone())
        }
        Some(child) => {
            let value = pattern_json(child, source);
            if let Some(annotation) = type_annotation.clone() {
                with_extra_field(value, "typeAnnotation", annotation)
            } else {
                value
            }
        }
        None => noop_json(node),
    };

    if let Some(right) = node.child_by_field_name("value") {
        return with_span(
            "AssignmentPattern",
            node,
            json!({
                "left": left,
                "right": expr_json(right, source)
            }),
        );
    }

    left
}

fn call_expression_json(node: Node, source: &str) -> Value {
    if node
        .child_by_field_name("arguments")
        .is_some_and(|child| child.kind() == "template_string")
    {
        return tagged_template_expression_json(node, source);
    }

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

fn tagged_template_expression_json(node: Node, source: &str) -> Value {
    let tag = node
        .child_by_field_name("function")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let quasi = node
        .child_by_field_name("arguments")
        .map(|child| template_literal_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "TaggedTemplateExpression",
        node,
        json!({
            "tag": tag,
            "quasi": quasi
        }),
    )
}

fn new_expression_json(node: Node, source: &str) -> Value {
    let callee = node
        .child_by_field_name("constructor")
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
        "NewExpression",
        node,
        json!({
            "callee": callee,
            "arguments": arguments
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

fn subscript_expression_json(node: Node, source: &str) -> Value {
    let object = field_json(node, "object", source).unwrap_or_else(|| noop_json(node));
    let property = field_json(node, "index", source).unwrap_or_else(|| noop_json(node));

    with_span(
        "MemberExpression",
        node,
        json!({
            "object": object,
            "property": property,
            "computed": true,
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

fn array_pattern_json(node: Node, source: &str) -> Value {
    let elements = named_children(node)
        .filter(|child| !is_comment(*child))
        .map(|child| pattern_json(child, source))
        .collect::<Vec<_>>();

    with_span("ArrayPattern", node, json!({ "elements": elements }))
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

fn object_pattern_json(node: Node, source: &str) -> Value {
    let properties = named_children(node)
        .filter_map(|child| object_pattern_property_json(child, source))
        .collect::<Vec<_>>();

    with_span("ObjectPattern", node, json!({ "properties": properties }))
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

fn object_pattern_property_json(node: Node, source: &str) -> Option<Value> {
    match node.kind() {
        "pair_pattern" => Some(object_pair_pattern_json(node, source)),
        "object_assignment_pattern" => Some(object_assignment_pattern_json(node, source)),
        "rest_pattern" => Some(unary_argument_json("RestElement", node, source)),
        "shorthand_property_identifier_pattern" => {
            Some(shorthand_object_property_json(node, source))
        }
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

fn object_pair_pattern_json(node: Node, source: &str) -> Value {
    let key_node = node.child_by_field_name("key").unwrap_or(node);
    let computed = key_node.kind() == "computed_property_name";
    let key = object_key_json(key_node, source);
    let value = node
        .child_by_field_name("value")
        .map(|child| pattern_json(child, source))
        .unwrap_or_else(|| noop_json(node));

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

fn object_assignment_pattern_json(node: Node, source: &str) -> Value {
    let left = node.child_by_field_name("left").unwrap_or(node);
    let right = node
        .child_by_field_name("right")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let key = match left.kind() {
        "shorthand_property_identifier_pattern" | "identifier" | "property_identifier" => {
            identifier_json(left, source)
        }
        _ => pattern_json(left, source),
    };
    let value = with_span(
        "AssignmentPattern",
        node,
        json!({
            "left": pattern_json(left, source),
            "right": right
        }),
    );

    with_span(
        "ObjectProperty",
        node,
        json!({
            "key": key,
            "value": value,
            "computed": false,
            "shorthand": true
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

fn assignment_pattern_json(node: Node, source: &str) -> Value {
    let left = node
        .child_by_field_name("left")
        .map(|child| pattern_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let right = node
        .child_by_field_name("right")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "AssignmentPattern",
        node,
        json!({
            "left": left,
            "right": right
        }),
    )
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
    identifier_json_with_span(node, node, source, None)
}

fn identifier_json_with_span(
    name_node: Node,
    span_node: Node,
    source: &str,
    type_annotation: Option<Value>,
) -> Value {
    let mut fields = json!({ "name": node_text(name_node, source) });
    if let Some(annotation) = type_annotation {
        fields = with_extra_field(fields, "typeAnnotation", annotation);
    }
    with_span("Identifier", span_node, fields)
}

fn with_extra_field(value: Value, key: &str, field_value: Value) -> Value {
    let mut object = match value {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    object.insert(key.to_string(), field_value);
    Value::Object(object)
}

fn ts_type_annotation_json(node: Node, source: &str) -> Value {
    let type_annotation = node
        .named_child(0)
        .map(|child| ts_type_json(child, source))
        .unwrap_or_else(|| with_span("TSAnyKeyword", node, json!({})));

    with_span(
        "TSTypeAnnotation",
        node,
        json!({ "typeAnnotation": type_annotation }),
    )
}

fn ts_type_json(node: Node, source: &str) -> Value {
    match node.kind() {
        "type_annotation" => ts_type_annotation_json(node, source),
        "predefined_type" => ts_predefined_type_json(node, source),
        "type_identifier" | "nested_type_identifier" => with_span(
            "TSTypeReference",
            node,
            json!({ "typeName": identifier_json(node, source) }),
        ),
        "array_type" => with_span(
            "TSArrayType",
            node,
            json!({
                "elementType": node
                    .named_child(0)
                    .map(|child| ts_type_json(child, source))
                    .unwrap_or_else(|| with_span("TSAnyKeyword", node, json!({})))
            }),
        ),
        _ => with_span("TSAnyKeyword", node, json!({})),
    }
}

fn ts_predefined_type_json(node: Node, source: &str) -> Value {
    let kind = match node_text(node, source).as_str() {
        "any" => "TSAnyKeyword",
        "bigint" => "TSBigIntKeyword",
        "boolean" => "TSBooleanKeyword",
        "never" => "TSNeverKeyword",
        "null" => "TSNullKeyword",
        "number" => "TSNumberKeyword",
        "object" => "TSObjectKeyword",
        "string" => "TSStringKeyword",
        "symbol" => "TSSymbolKeyword",
        "undefined" => "TSUndefinedKeyword",
        "unknown" => "TSUnknownKeyword",
        "void" => "TSVoidKeyword",
        _ => "TSAnyKeyword",
    };
    with_span(kind, node, json!({}))
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
        template_literal_json(node, source)
    } else {
        string_literal_json(node, source)
    }
}

fn template_literal_json(node: Node, source: &str) -> Value {
    let substitutions = named_children(node)
        .filter(|child| child.kind() == "template_substitution")
        .collect::<Vec<_>>();
    let mut quasis = Vec::with_capacity(substitutions.len() + 1);
    let mut expressions = Vec::with_capacity(substitutions.len());
    let mut quasi_start = node.start_byte().saturating_add(1);
    let content_end = node.end_byte().saturating_sub(1);

    for substitution in &substitutions {
        quasis.push(template_element_json(
            quasi_start,
            substitution.start_byte(),
            false,
            source,
        ));
        if let Some(expression) = substitution.named_child(0) {
            expressions.push(expr_json(expression, source));
        }
        quasi_start = substitution.end_byte();
    }

    quasis.push(template_element_json(
        quasi_start,
        content_end,
        true,
        source,
    ));

    with_span(
        "TemplateLiteral",
        node,
        json!({
            "expressions": expressions,
            "quasis": quasis
        }),
    )
}

fn template_element_json(start_byte: usize, end_byte: usize, tail: bool, source: &str) -> Value {
    let raw = source
        .get(start_byte..end_byte)
        .unwrap_or_default()
        .to_string();
    with_span_bounds(
        "TemplateElement",
        start_byte,
        point_for_byte(source, start_byte),
        end_byte,
        point_for_byte(source, end_byte),
        json!({
            "value": {
                "raw": raw,
                "cooked": decode_js_string_escapes(&raw)
            },
            "tail": tail
        }),
    )
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

fn pattern_json(node: Node, source: &str) -> Value {
    match node.kind() {
        "array_pattern" => array_pattern_json(node, source),
        "object_pattern" => object_pattern_json(node, source),
        "assignment_pattern" => assignment_pattern_json(node, source),
        "rest_pattern" => unary_argument_json("RestElement", node, source),
        "parenthesized_expression" => node
            .named_child(0)
            .map(|child| pattern_json(child, source))
            .unwrap_or_else(|| noop_json(node)),
        _ => expr_json(node, source),
    }
}

fn pattern_or_expr_json(node: Node, source: &str) -> Value {
    match node.kind() {
        "array_pattern" | "object_pattern" | "assignment_pattern" | "rest_pattern" => {
            pattern_json(node, source)
        }
        _ => expr_json(node, source),
    }
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

fn declaration_kind_in_for_in_of(node: Node, source: &str) -> Option<String> {
    (0..node.child_count())
        .filter_map(|index| node.child(index))
        .filter(|child| !child.is_named())
        .map(|child| node_text(child, source))
        .find(|text| matches!(text.as_str(), "let" | "const" | "var"))
}

fn with_span(kind: &str, node: Node, fields: Value) -> Value {
    with_span_bounds(
        kind,
        node.start_byte(),
        node.start_position(),
        node.end_byte(),
        node.end_position(),
        fields,
    )
}

fn with_span_bounds(
    kind: &str,
    start_byte: usize,
    start_position: Point,
    end_byte: usize,
    end_position: Point,
    fields: Value,
) -> Value {
    let mut object = match fields {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    object.insert("type".into(), Value::String(kind.into()));
    object.insert("start".into(), Value::from(start_byte));
    object.insert("end".into(), Value::from(end_byte));
    object.insert(
        "loc".into(),
        json!({
            "start": {
                "line": start_position.row + 1,
                "column": start_position.column
            },
            "end": {
                "line": end_position.row + 1,
                "column": end_position.column
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

fn point_for_byte(source: &str, byte: usize) -> Point {
    let clamped = byte.min(source.len());
    let mut row = 0;
    let mut line_start = 0;
    for (index, value) in source.bytes().take(clamped).enumerate() {
        if value == b'\n' {
            row += 1;
            line_start = index + 1;
        }
    }
    Point {
        row,
        column: clamped.saturating_sub(line_start),
    }
}

fn named_children(node: Node) -> impl Iterator<Item = Node> {
    (0..node.named_child_count()).filter_map(move |index| node.named_child(index))
}

fn first_named_child(node: Node) -> Option<Node> {
    node.named_child(0)
}

fn first_expression_child(node: Node) -> Option<Node> {
    named_children(node).find(|child| is_expression_like(*child))
}

fn last_type_child(node: Node) -> Option<Node> {
    named_children(node)
        .filter(|child| is_type_like(*child))
        .last()
}

fn is_expression_like(node: Node) -> bool {
    matches!(
        node.kind(),
        "identifier"
            | "property_identifier"
            | "number"
            | "string"
            | "template_string"
            | "true"
            | "false"
            | "null"
            | "this"
            | "binary_expression"
            | "unary_expression"
            | "assignment_expression"
            | "augmented_assignment_expression"
            | "update_expression"
            | "ternary_expression"
            | "call_expression"
            | "new_expression"
            | "member_expression"
            | "subscript_expression"
            | "array"
            | "object"
            | "function_expression"
            | "arrow_function"
            | "class"
            | "non_null_expression"
            | "sequence_expression"
            | "parenthesized_expression"
            | "as_expression"
            | "type_assertion"
            | "satisfies_expression"
            | "await_expression"
    )
}

fn is_type_like(node: Node) -> bool {
    matches!(
        node.kind(),
        "predefined_type"
            | "type_identifier"
            | "nested_type_identifier"
            | "type_annotation"
            | "array_type"
            | "generic_type"
            | "union_type"
            | "intersection_type"
            | "object_type"
            | "tuple_type"
    )
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

fn colon_child<'a>(node: Node<'a>, source: &str) -> Option<Node<'a>> {
    (0..node.child_count())
        .filter_map(|index| node.child(index))
        .find(|child| !child.is_named() && node_text(*child, source) == ":")
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
            json["ast"]["program"]["body"][1]["body"]["body"][0]["argument"]["name"],
            "x"
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
    fn emits_if_statements_and_computed_member_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json =
            parse_source(root, path, "if (d = decorators[i]) foo();\n").expect("parse succeeds");

        let if_stmt = &json["ast"]["program"]["body"][0];
        assert_eq!(if_stmt["type"], "IfStatement");
        assert_eq!(if_stmt["test"]["type"], "AssignmentExpression");
        assert_eq!(if_stmt["test"]["right"]["type"], "MemberExpression");
        assert_eq!(if_stmt["test"]["right"]["computed"], true);
        assert_eq!(if_stmt["test"]["right"]["object"]["name"], "decorators");
        assert_eq!(if_stmt["test"]["right"]["property"]["name"], "i");
        assert_eq!(if_stmt["consequent"]["type"], "ExpressionStatement");
        assert_eq!(if_stmt["alternate"], Value::Null);
    }

    #[test]
    fn emits_ternaries_as_babel_conditional_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(root, path, "x ? y : z;\n").expect("parse succeeds");

        let expression = &json["ast"]["program"]["body"][0]["expression"];
        assert_eq!(expression["type"], "ConditionalExpression");
        assert_eq!(expression["test"]["name"], "x");
        assert_eq!(expression["consequent"]["name"], "y");
        assert_eq!(expression["alternate"]["name"], "z");
    }

    #[test]
    fn emits_loops_jumps_and_augmented_assignments() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "while (x < 1) { x += 1; break; }\ndo { continue loop1; } while (ok);\n",
        )
        .expect("parse succeeds");

        let while_stmt = &json["ast"]["program"]["body"][0];
        assert_eq!(while_stmt["type"], "WhileStatement");
        assert_eq!(while_stmt["test"]["type"], "BinaryExpression");
        let assignment = &while_stmt["body"]["body"][0]["expression"];
        assert_eq!(assignment["type"], "AssignmentExpression");
        assert_eq!(assignment["operator"], "+=");
        assert_eq!(while_stmt["body"]["body"][1]["type"], "BreakStatement");

        let do_stmt = &json["ast"]["program"]["body"][1];
        assert_eq!(do_stmt["type"], "DoWhileStatement");
        assert_eq!(do_stmt["test"]["name"], "ok");
        assert_eq!(do_stmt["body"]["body"][0]["type"], "ContinueStatement");
        assert_eq!(do_stmt["body"]["body"][0]["label"]["name"], "loop1");
    }

    #[test]
    fn emits_classic_for_loops_and_update_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "for (x = 0; x < 1; x++) { z += 1; }\nfor (;;) {}\n",
        )
        .expect("parse succeeds");

        let for_stmt = &json["ast"]["program"]["body"][0];
        assert_eq!(for_stmt["type"], "ForStatement");
        assert_eq!(for_stmt["init"]["type"], "AssignmentExpression");
        assert_eq!(for_stmt["test"]["type"], "BinaryExpression");
        assert_eq!(for_stmt["update"]["type"], "UpdateExpression");
        assert_eq!(for_stmt["update"]["operator"], "++");
        assert_eq!(for_stmt["update"]["prefix"], false);
        assert_eq!(for_stmt["body"]["body"][0]["expression"]["operator"], "+=");

        let empty_for = &json["ast"]["program"]["body"][1];
        assert_eq!(empty_for["type"], "ForStatement");
        assert_eq!(empty_for["init"], Value::Null);
        assert_eq!(empty_for["test"], Value::Null);
        assert_eq!(empty_for["update"], Value::Null);
    }

    #[test]
    fn emits_for_in_of_loops_and_destructuring_patterns() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "for (var i in arr) { foo(i); }\nfor (i of arr) { foo(i); }\nfor (var {a, b, c} of obj) { foo(a, b, c); }\nfor ([x, y] of arr) {}\n",
        )
        .expect("parse succeeds");

        let for_in = &json["ast"]["program"]["body"][0];
        assert_eq!(for_in["type"], "ForInStatement");
        assert_eq!(for_in["left"]["type"], "VariableDeclaration");
        assert_eq!(for_in["left"]["kind"], "var");
        assert_eq!(for_in["left"]["declarations"][0]["id"]["name"], "i");
        assert_eq!(for_in["left"]["declarations"][0]["init"], Value::Null);
        assert_eq!(for_in["right"]["name"], "arr");

        let for_of = &json["ast"]["program"]["body"][1];
        assert_eq!(for_of["type"], "ForOfStatement");
        assert_eq!(for_of["left"]["type"], "Identifier");
        assert_eq!(for_of["left"]["name"], "i");

        let object_pattern = &json["ast"]["program"]["body"][2]["left"]["declarations"][0]["id"];
        assert_eq!(object_pattern["type"], "ObjectPattern");
        assert_eq!(object_pattern["properties"].as_array().unwrap().len(), 3);
        assert_eq!(object_pattern["properties"][0]["type"], "ObjectProperty");
        assert_eq!(object_pattern["properties"][0]["key"]["name"], "a");
        assert_eq!(object_pattern["properties"][0]["value"]["name"], "a");
        assert_eq!(object_pattern["properties"][0]["shorthand"], true);

        let array_pattern = &json["ast"]["program"]["body"][3]["left"];
        assert_eq!(array_pattern["type"], "ArrayPattern");
        assert_eq!(array_pattern["elements"][0]["name"], "x");
        assert_eq!(array_pattern["elements"][1]["name"], "y");
    }

    #[test]
    fn emits_switch_labeled_and_this_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let source =
            "loop1: while (ok) { continue loop1; }\nswitch (x) { case 1: y; default: this.z; }\n";
        let json = parse_source(root, path, source).expect("parse succeeds");

        let labeled = &json["ast"]["program"]["body"][0];
        assert_eq!(labeled["type"], "LabeledStatement");
        assert_eq!(labeled["label"]["name"], "loop1");
        assert_eq!(labeled["body"]["type"], "WhileStatement");
        assert_eq!(labeled["body"]["body"]["body"][0]["label"]["name"], "loop1");

        let switch_stmt = &json["ast"]["program"]["body"][1];
        assert_eq!(switch_stmt["type"], "SwitchStatement");
        assert_eq!(switch_stmt["discriminant"]["name"], "x");
        assert_eq!(switch_stmt["cases"].as_array().unwrap().len(), 2);

        let case_label = &switch_stmt["cases"][0];
        assert_eq!(case_label["type"], "SwitchCase");
        assert_eq!(case_label["test"]["value"], 1.0);
        assert_eq!(case_label["consequent"][0]["expression"]["name"], "y");
        assert_eq!(
            &source[case_label["start"].as_u64().unwrap() as usize
                ..case_label["end"].as_u64().unwrap() as usize],
            "case 1:"
        );

        let default_label = &switch_stmt["cases"][1];
        assert_eq!(default_label["test"], Value::Null);
        assert_eq!(
            &source[default_label["start"].as_u64().unwrap() as usize
                ..default_label["end"].as_u64().unwrap() as usize],
            "default:"
        );
        assert_eq!(
            default_label["consequent"][0]["expression"]["object"]["type"],
            "ThisExpression"
        );
    }

    #[test]
    fn emits_with_statements() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(root, path, "with (foo()) { bar(); }\nwith (baz()) qux();\n")
            .expect("parse succeeds");

        let block_with = &json["ast"]["program"]["body"][0];
        assert_eq!(block_with["type"], "WithStatement");
        assert_eq!(block_with["object"]["type"], "CallExpression");
        assert_eq!(block_with["object"]["callee"]["name"], "foo");
        assert_eq!(block_with["body"]["type"], "BlockStatement");
        assert_eq!(
            block_with["body"]["body"][0]["expression"]["callee"]["name"],
            "bar"
        );

        let statement_with = &json["ast"]["program"]["body"][1];
        assert_eq!(statement_with["type"], "WithStatement");
        assert_eq!(statement_with["object"]["callee"]["name"], "baz");
        assert_eq!(statement_with["body"]["type"], "ExpressionStatement");
        assert_eq!(
            statement_with["body"]["expression"]["callee"]["name"],
            "qux"
        );
    }

    #[test]
    fn emits_try_catch_finally_and_throw_statements() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "try { open(); } catch (err) { throw err; } finally { close(); }\n",
        )
        .expect("parse succeeds");

        let try_stmt = &json["ast"]["program"]["body"][0];
        assert_eq!(try_stmt["type"], "TryStatement");
        assert_eq!(try_stmt["block"]["type"], "BlockStatement");
        assert_eq!(try_stmt["handler"]["type"], "CatchClause");
        assert_eq!(try_stmt["handler"]["param"]["name"], "err");
        assert_eq!(
            try_stmt["handler"]["body"]["body"][0]["type"],
            "ThrowStatement"
        );
        assert_eq!(try_stmt["finalizer"]["type"], "BlockStatement");
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
    fn emits_function_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "function method() { return function foo(x) { return x; }; }\n",
        )
        .expect("parse succeeds");

        let func = &json["ast"]["program"]["body"][0]["body"]["body"][0]["argument"];
        assert_eq!(func["type"], "FunctionExpression");
        assert_eq!(func["id"]["name"], "foo");
        assert_eq!(func["params"][0]["name"], "x");
        assert_eq!(func["body"]["body"][0]["argument"]["name"], "x");
    }

    #[test]
    fn emits_typescript_non_null_expressions_with_fallback_parser() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(root, path, "const foo = bar!\n").expect("parse succeeds");

        let init = &json["ast"]["program"]["body"][0]["declarations"][0]["init"];
        assert_eq!(init["type"], "TSNonNullExpression");
        assert_eq!(init["expression"]["name"], "bar");
        assert_eq!(init["start"], 12);
        assert_eq!(init["end"], 16);
        assert_eq!(init["expression"]["start"], 12);
        assert_eq!(init["expression"]["end"], 15);
    }

    #[test]
    fn emits_typescript_parameter_wrappers_as_plain_parameters() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.ts");
        let json = parse_source(
            root,
            path,
            "const obj = { [\"someNameComputation()\"](node: Node) { foo(node); } };\n",
        )
        .expect("parse succeeds");

        let method = &json["ast"]["program"]["body"][0]["declarations"][0]["init"]["properties"][0];
        assert_eq!(method["type"], "ObjectMethod");
        assert_eq!(method["computed"], true);
        assert_eq!(method["key"]["type"], "StringLiteral");
        assert_eq!(method["key"]["value"], "someNameComputation()");
        assert_eq!(method["params"][0]["type"], "Identifier");
        assert_eq!(method["params"][0]["name"], "node");
    }

    #[test]
    fn emits_template_literals_and_tagged_templates() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "foo(`Hello ${world}!`);\nx`a ${1+1} b`;\nString.raw`../${42}\\..`;\n",
        )
        .expect("parse succeeds");

        let template = &json["ast"]["program"]["body"][0]["expression"]["arguments"][0];
        assert_eq!(template["type"], "TemplateLiteral");
        assert_eq!(template["expressions"][0]["name"], "world");
        assert_eq!(template["quasis"][0]["type"], "TemplateElement");
        assert_eq!(template["quasis"][0]["value"]["raw"], "Hello ");
        assert_eq!(template["quasis"][0]["tail"], false);
        assert_eq!(template["quasis"][1]["value"]["raw"], "!");
        assert_eq!(template["quasis"][1]["tail"], true);

        let simple_tag = &json["ast"]["program"]["body"][1]["expression"];
        assert_eq!(simple_tag["type"], "TaggedTemplateExpression");
        assert_eq!(simple_tag["tag"]["name"], "x");
        assert_eq!(simple_tag["quasi"]["quasis"][0]["value"]["raw"], "a ");
        assert_eq!(simple_tag["quasi"]["expressions"][0]["operator"], "+");
        assert_eq!(simple_tag["quasi"]["quasis"][1]["value"]["raw"], " b");

        let member_tag = &json["ast"]["program"]["body"][2]["expression"];
        assert_eq!(member_tag["type"], "TaggedTemplateExpression");
        assert_eq!(member_tag["tag"]["type"], "MemberExpression");
        assert_eq!(member_tag["tag"]["property"]["name"], "raw");
        assert_eq!(member_tag["quasi"]["quasis"][0]["value"]["raw"], "../");
        assert_eq!(member_tag["quasi"]["quasis"][1]["value"]["raw"], "\\..");
    }

    #[test]
    fn emits_sequence_and_class_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json =
            parse_source(root, path, "let x = (class Foo {}, bar())\n").expect("parse succeeds");

        let init = &json["ast"]["program"]["body"][0]["declarations"][0]["init"];
        assert_eq!(init["type"], "SequenceExpression");
        assert_eq!(init["expressions"][0]["type"], "ClassExpression");
        assert_eq!(init["expressions"][0]["id"]["name"], "Foo");
        assert_eq!(init["expressions"][0]["body"]["type"], "ClassBody");
        assert_eq!(init["expressions"][1]["type"], "CallExpression");
        assert_eq!(init["expressions"][1]["callee"]["name"], "bar");
    }

    #[test]
    fn emits_constructor_calls_as_babel_new_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json =
            parse_source(root, path, "var x = new MyClass(arg1, arg2)\n").expect("parse succeeds");

        let init = &json["ast"]["program"]["body"][0]["declarations"][0]["init"];
        assert_eq!(init["type"], "NewExpression");
        assert_eq!(init["callee"]["name"], "MyClass");
        assert_eq!(init["arguments"][0]["name"], "arg1");
        assert_eq!(init["arguments"][1]["name"], "arg2");
    }

    #[test]
    fn emits_import_export_and_ts_import_equals_declarations() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.ts");
        let json = parse_source(
            root,
            path,
            "import {x as y} from \"foo\";\nimport fs = require('fs');\nexport const getApiA = () => {};\n",
        )
        .expect("parse succeeds");

        let import_decl = &json["ast"]["program"]["body"][0];
        assert_eq!(import_decl["type"], "ImportDeclaration");
        assert_eq!(import_decl["source"]["value"], "foo");
        assert_eq!(import_decl["specifiers"][0]["type"], "ImportSpecifier");
        assert_eq!(import_decl["specifiers"][0]["imported"]["name"], "x");
        assert_eq!(import_decl["specifiers"][0]["local"]["name"], "y");

        let import_equals = &json["ast"]["program"]["body"][1];
        assert_eq!(import_equals["type"], "TSImportEqualsDeclaration");
        assert_eq!(import_equals["id"]["name"], "fs");
        assert_eq!(
            import_equals["moduleReference"]["type"],
            "TSExternalModuleReference"
        );
        assert_eq!(
            import_equals["moduleReference"]["expression"]["value"],
            "fs"
        );

        let export_decl = &json["ast"]["program"]["body"][2];
        assert_eq!(export_decl["type"], "ExportNamedDeclaration");
        assert_eq!(export_decl["declaration"]["type"], "VariableDeclaration");
        assert_eq!(
            export_decl["declaration"]["declarations"][0]["id"]["name"],
            "getApiA"
        );
    }

    #[test]
    fn emits_ts_declare_function_modules_and_expression_wrappers() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.ts");
        let json = parse_source(
            root,
            path,
            "declare function foo(arg: string): string\nmodule M { export var [a, b] = [1, 2]; }\nasync function x(foo) { await foo(); }\ndelete foo.x;\nlet y = z satisfies T;\nlet u = req.user as UserDocument;\n",
        )
        .expect("parse succeeds");

        let declare_fn = &json["ast"]["program"]["body"][0];
        assert_eq!(declare_fn["type"], "TSDeclareFunction");
        assert_eq!(declare_fn["id"]["name"], "foo");
        assert_eq!(declare_fn["params"][0]["name"], "arg");
        assert_eq!(
            declare_fn["params"][0]["typeAnnotation"]["type"],
            "TSTypeAnnotation"
        );
        assert_eq!(
            declare_fn["params"][0]["typeAnnotation"]["typeAnnotation"]["type"],
            "TSStringKeyword"
        );
        assert_eq!(
            declare_fn["returnType"]["typeAnnotation"]["type"],
            "TSStringKeyword"
        );

        let module_decl = &json["ast"]["program"]["body"][1];
        assert_eq!(module_decl["type"], "TSModuleDeclaration");
        assert_eq!(module_decl["id"]["name"], "M");
        assert_eq!(module_decl["body"]["type"], "TSModuleBlock");
        assert_eq!(
            module_decl["body"]["body"][0]["type"],
            "ExportNamedDeclaration"
        );

        let await_expr = &json["ast"]["program"]["body"][2]["body"]["body"][0]["expression"];
        assert_eq!(await_expr["type"], "AwaitExpression");
        assert_eq!(await_expr["argument"]["type"], "CallExpression");

        let delete_expr = &json["ast"]["program"]["body"][3]["expression"];
        assert_eq!(delete_expr["type"], "UnaryExpression");
        assert_eq!(delete_expr["operator"], "delete");

        let satisfies_expr = &json["ast"]["program"]["body"][4]["declarations"][0]["init"];
        assert_eq!(satisfies_expr["type"], "TSSatisfiesExpression");
        assert_eq!(satisfies_expr["expression"]["name"], "z");

        let as_expr = &json["ast"]["program"]["body"][5]["declarations"][0]["init"];
        assert_eq!(as_expr["type"], "TSAsExpression");
        assert_eq!(as_expr["expression"]["type"], "MemberExpression");
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
