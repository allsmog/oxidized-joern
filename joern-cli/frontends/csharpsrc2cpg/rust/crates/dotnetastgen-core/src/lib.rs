use anyhow::{anyhow, Result};
use quick_xml::events::Event;
use quick_xml::Reader;
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use tree_sitter::{Node, Parser, Point};

const AST_PREFIX: &str = "ast.";

thread_local! {
    /// Accumulates tree-sitter node kinds that fell through to `Unknown`, across all
    /// files processed in a CLI run. Surfaced as a single stderr summary line by the
    /// caller via [`take_unmapped_summary`]; never written to stdout/JSON.
    static UNMAPPED_KINDS: RefCell<BTreeMap<String, usize>> = const { RefCell::new(BTreeMap::new()) };
}

fn record_unmapped(kinds: BTreeMap<String, usize>) {
    if kinds.is_empty() {
        return;
    }
    UNMAPPED_KINDS.with(|totals| {
        let mut totals = totals.borrow_mut();
        for (kind, count) in kinds {
            *totals.entry(kind).or_insert(0) += count;
        }
    });
}

/// Drains the accumulated unmapped-kind counts and renders the CLI summary line,
/// e.g. `dotnetastgen: 3 unmapped node(s): delegate_declaration(x2), goto_case(x1)`.
/// Returns `None` when nothing was unmapped. Calling this resets the counter.
pub fn take_unmapped_summary() -> Option<String> {
    UNMAPPED_KINDS.with(|totals| {
        let kinds = std::mem::take(&mut *totals.borrow_mut());
        if kinds.is_empty() {
            return None;
        }
        let total: usize = kinds.values().sum();
        let details = kinds
            .iter()
            .map(|(kind, count)| format!("{kind}(x{count})"))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!("dotnetastgen: {total} unmapped node(s): {details}"))
    })
}

pub fn generate_file(path: &Path) -> Result<Value> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("xml") => generate_xml_summary_file(path),
        _ => {
            let source = fs::read_to_string(path)?;
            generate_source(path, &source)
        }
    }
}

pub fn generate_source(path: &Path, source: &str) -> Result<Value> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_c_sharp::LANGUAGE.into())?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow!("tree-sitter failed to parse {}", path.display()))?;

    let emitter = Emitter {
        bytes: source.as_bytes(),
        unmapped: RefCell::new(BTreeMap::new()),
    };
    if emitter.has_unrecoverable_error(tree.root_node()) {
        return Err(anyhow!("tree-sitter parse errors in {}", path.display()));
    }
    let ast_root = emitter.compilation_unit(tree.root_node());
    record_unmapped(emitter.unmapped.into_inner());
    Ok(json!({
        "FileName": path.to_string_lossy(),
        "AstRoot": ast_root,
    }))
}

pub fn generate_xml_summary_file(path: &Path) -> Result<Value> {
    let source = fs::read_to_string(path)?;
    generate_xml_summary(&source)
}

pub fn generate_xml_summary(source: &str) -> Result<Value> {
    let mut reader = Reader::from_str(source);
    reader.config_mut().trim_text(true);
    let mut summary = SummaryBuilder::default();

    loop {
        match reader.read_event()? {
            Event::Start(event) | Event::Empty(event) if event.name().as_ref() == b"member" => {
                for attr in event.attributes().flatten() {
                    if attr.key.as_ref() == b"name" {
                        let name = attr.decode_and_unescape_value(reader.decoder())?;
                        summary.add_doc_member(&name);
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(summary.into_json())
}

#[derive(Default)]
struct SummaryBuilder {
    namespaces: BTreeMap<String, BTreeMap<String, TypeSummary>>,
}

#[derive(Default)]
struct TypeSummary {
    methods: Vec<Value>,
    fields: Vec<Value>,
}

impl SummaryBuilder {
    fn add_doc_member(&mut self, member_name: &str) {
        if let Some(type_name) = member_name.strip_prefix("T:") {
            self.ensure_type(type_name);
        } else if let Some(method_signature) = member_name.strip_prefix("M:") {
            self.add_method(method_signature);
        } else if let Some(field_name) = member_name
            .strip_prefix("F:")
            .or_else(|| member_name.strip_prefix("P:"))
        {
            self.add_field(field_name);
        }
    }

    fn ensure_type(&mut self, type_name: &str) -> &mut TypeSummary {
        let namespace = namespace_for_type(type_name).to_string();
        self.namespaces
            .entry(namespace)
            .or_default()
            .entry(type_name.to_string())
            .or_default()
    }

    fn add_method(&mut self, method_signature: &str) {
        let (head, params) = split_method_signature(method_signature);
        let Some((type_name, method_name)) = split_type_member(head) else {
            return;
        };
        let parameter_types = params
            .map(split_doc_parameters)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(idx, typ)| json!([format!("arg{idx}"), normalize_doc_type(&typ)]))
            .collect::<Vec<_>>();
        self.ensure_type(type_name).methods.push(json!({
            "name": method_name,
            "returnType": "ANY",
            "parameterTypes": parameter_types,
            "isStatic": false,
        }));
    }

    fn add_field(&mut self, field_name: &str) {
        let Some((type_name, name)) = split_type_member(field_name) else {
            return;
        };
        self.ensure_type(type_name).fields.push(json!({
            "name": name,
            "typeName": "ANY",
        }));
    }

    fn into_json(self) -> Value {
        let mut root = Map::new();
        for (namespace, types) in self.namespaces {
            let type_values = types
                .into_iter()
                .map(|(name, typ)| {
                    json!({
                        "name": name,
                        "methods": typ.methods,
                        "fields": typ.fields,
                    })
                })
                .collect::<Vec<_>>();
            root.insert(namespace, json!(type_values));
        }
        Value::Object(root)
    }
}

struct Emitter<'a> {
    bytes: &'a [u8],
    unmapped: RefCell<BTreeMap<String, usize>>,
}

impl<'a> Emitter<'a> {
    fn compilation_unit(&self, node: Node) -> Value {
        let children = self.named_children(node).collect::<Vec<_>>();
        let usings = self
            .named_children(node)
            .filter(|child| child.kind() == "using_directive")
            .map(|child| self.emit(child))
            .collect::<Vec<_>>();
        let members = if let Some((idx, file_scoped)) = children
            .iter()
            .enumerate()
            .find(|(_, child)| child.kind() == "file_scoped_namespace_declaration")
        {
            let namespace_members = children
                .iter()
                .skip(idx + 1)
                .filter(|child| !matches!(child.kind(), "using_directive" | "comment"))
                .filter_map(|child| self.emit_member(*child))
                .collect::<Vec<_>>();
            vec![self.namespace_declaration_with_members(*file_scoped, true, namespace_members)]
        } else {
            children
                .iter()
                .filter(|child| child.kind() != "using_directive")
                .filter_map(|child| self.emit_member(*child))
                .collect::<Vec<_>>()
        };

        self.node(
            "CompilationUnit",
            node,
            vec![("Usings", json!(usings)), ("Members", json!(members))],
        )
    }

    fn emit(&self, node: Node) -> Value {
        match node.kind() {
            "using_directive" => self.using_directive(node),
            "namespace_declaration" => self.namespace_declaration(node, false),
            "file_scoped_namespace_declaration" => self.namespace_declaration(node, true),
            "global_statement" => self.global_statement(node),
            "class_declaration" => self.type_declaration(node, "ClassDeclaration"),
            "struct_declaration" => self.type_declaration(node, "StructDeclaration"),
            "interface_declaration" => self.type_declaration(node, "InterfaceDeclaration"),
            "record_declaration" => self.type_declaration(node, "RecordDeclaration"),
            "enum_declaration" => self.enum_declaration(node),
            "enum_member_declaration" => self.enum_member(node),
            "field_declaration" => self.field_declaration(node),
            "property_declaration" => self.property_declaration(node),
            "constructor_declaration" => self.constructor_declaration(node),
            "method_declaration" => self.method_declaration(node, "MethodDeclaration"),
            "local_function_statement" => self.method_declaration(node, "LocalFunctionStatement"),
            "block" => self.block(node),
            "local_declaration_statement" => self.local_declaration_statement(node),
            "variable_declaration" => self.variable_declaration(node),
            "variable_declarator" => self.variable_declarator(node),
            "expression_statement" => self.expression_statement(node),
            "return_statement" => self.return_statement(node),
            "break_statement" => self.node("BreakStatement", node, vec![]),
            "continue_statement" => self.node("ContinueStatement", node, vec![]),
            "throw_statement" => self.throw_statement(node),
            "if_statement" => self.if_statement(node),
            "while_statement" => self.while_statement(node),
            "do_statement" => self.do_statement(node),
            "for_statement" => self.for_statement(node),
            "foreach_statement" => self.foreach_statement(node),
            "switch_statement" => self.switch_statement(node),
            "using_statement" => self.using_statement(node),
            "try_statement" => self.try_statement(node),
            "goto_statement" => self.goto_statement(node),
            "labeled_statement" => self.labeled_statement(node),
            "identifier" => self.identifier(node),
            "qualified_name" | "alias_qualified_name" => self.qualified_name(node),
            "generic_name" => self.generic_name(node),
            "predefined_type" => self.node("PredefinedType", node, vec![]),
            "implicit_type" => self.identifier_like("IdentifierName", node, self.text(node)),
            "array_type" => self.array_type(node),
            "nullable_type" => self.nullable_type(node),
            "integer_literal" | "real_literal" => {
                self.node("NumericLiteralExpression", node, vec![])
            }
            "string_literal"
            | "verbatim_string_literal"
            | "raw_string_literal"
            | "character_literal" => self.node("StringLiteralExpression", node, vec![]),
            "boolean_literal" if self.text(node) == "true" => {
                self.node("TrueLiteralExpression", node, vec![])
            }
            "boolean_literal" => self.node("FalseLiteralExpression", node, vec![]),
            "null_literal" => self.node("NullLiteralExpression", node, vec![]),
            "assignment_expression" => self.assignment_expression(node),
            "binary_expression" => self.binary_expression(node),
            "prefix_unary_expression" => self.unary_expression(node, false),
            "postfix_unary_expression" => self.unary_expression(node, true),
            "invocation_expression" => self.invocation_expression(node),
            "member_access_expression" => self.member_access_expression(node),
            "conditional_access_expression" => self.conditional_access_expression(node),
            "member_binding_expression" => self.member_binding_expression(node),
            "element_binding_expression" => self.element_binding_expression(node),
            "declaration_expression" => self.declaration_expression(node),
            "argument_list" => self.argument_list(node, "ArgumentList"),
            "bracketed_argument_list" => self.argument_list(node, "BracketedArgumentList"),
            "argument" => self.argument(node),
            "object_creation_expression" | "implicit_object_creation_expression" => {
                self.object_creation_expression(node)
            }
            "anonymous_object_creation_expression" => {
                self.anonymous_object_creation_expression(node)
            }
            "cast_expression" => self.cast_expression(node),
            "parenthesized_expression" => self.parenthesized_expression(node),
            "element_access_expression" => self.element_access_expression(node),
            "this" => self.node("ThisExpression", node, vec![]),
            "lambda_expression" => self.lambda_expression(node),
            "implicit_array_creation_expression" => self.implicit_array_creation_expression(node),
            "array_creation_expression" => self.array_creation_expression(node),
            "initializer_expression" => {
                self.initializer_expression(node, "ArrayInitializerExpression")
            }
            "collection_expression" => self.initializer_expression(node, "CollectionExpression"),
            "expression_element" => self.expression_element(node),
            "collection_element" => self
                .first_named_child(node)
                .map(|n| self.emit(n))
                .unwrap_or_else(|| self.unknown(node)),
            "conditional_expression" => self.conditional_expression(node),
            "await_expression" => self.await_expression(node),
            "interpolated_string_expression" => self.interpolated_string_expression(node),
            "string_content" => self.interpolated_string_text(node),
            "interpolation" => self.interpolation(node),
            "is_pattern_expression" => self.is_pattern_expression(node),
            "declaration_pattern" => self.declaration_pattern(node),
            "constant_pattern" => self.constant_pattern(node),
            "negated_pattern" => self.negated_pattern(node),
            "relational_pattern" => self.relational_pattern(node),
            "ERROR" => self.recovered_error(node),
            _ => self.unknown(node),
        }
    }

    fn emit_member(&self, node: Node) -> Option<Value> {
        match node.kind() {
            "declaration_list" => None,
            "comment" => None,
            _ => Some(self.emit(node)),
        }
    }

    fn using_directive(&self, node: Node) -> Value {
        let name = node.child_by_field_name("name").or_else(|| {
            self.first_named_child_of_kind(node, &["qualified_name", "identifier", "generic_name"])
        });
        self.node(
            "UsingDirective",
            node,
            vec![("Name", name.map(|n| self.emit(n)).unwrap_or(Value::Null))],
        )
    }

    fn namespace_declaration(&self, node: Node, file_scoped: bool) -> Value {
        let body = node.child_by_field_name("body");
        let members = body
            .into_iter()
            .flat_map(|b| self.named_children(b))
            .filter_map(|child| self.emit_member(child))
            .collect::<Vec<_>>();
        self.namespace_declaration_with_members(node, file_scoped, members)
    }

    fn namespace_declaration_with_members(
        &self,
        node: Node,
        file_scoped: bool,
        members: Vec<Value>,
    ) -> Value {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        let kind = if file_scoped {
            "FileScopedNamespaceDeclaration"
        } else {
            "NamespaceDeclaration"
        };
        self.node(
            kind,
            node,
            vec![("Name", name), ("Members", json!(members))],
        )
    }

    fn global_statement(&self, node: Node) -> Value {
        let statement = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("GlobalStatement", node, vec![("Statement", statement)])
    }

    fn type_declaration(&self, node: Node, kind: &'static str) -> Value {
        let body = node.child_by_field_name("body");
        let members = body
            .into_iter()
            .flat_map(|b| self.named_children(b))
            .filter_map(|child| self.emit_member(child))
            .collect::<Vec<_>>();
        let mut fields = vec![
            (
                "Identifier",
                self.identifier_token(node.child_by_field_name("name")),
            ),
            ("Members", json!(members)),
            ("Modifiers", json!(self.modifiers(node))),
            ("AttributeLists", json!(self.attribute_lists(node))),
            ("TypeParameterList", self.type_parameter_list_field(node)),
            ("ConstraintClauses", json!(self.constraint_clauses(node))),
        ];
        if let Some(base_list) = self.first_named_child_of_kind(node, &["base_list"]) {
            fields.push(("BaseList", self.base_list(base_list)));
        }
        if let Some(params) = self.first_named_child_of_kind(node, &["parameter_list"]) {
            fields.push(("ParameterList", self.parameter_list(params)));
        }
        self.node(kind, node, fields)
    }

    fn enum_declaration(&self, node: Node) -> Value {
        let body = node.child_by_field_name("body");
        let members = body
            .into_iter()
            .flat_map(|b| self.named_children(b))
            .filter(|child| child.kind() == "enum_member_declaration")
            .map(|child| self.emit(child))
            .collect::<Vec<_>>();
        let mut fields = vec![
            (
                "Identifier",
                self.identifier_token(node.child_by_field_name("name")),
            ),
            ("Members", json!(members)),
            ("Modifiers", json!(self.modifiers(node))),
            ("AttributeLists", json!(self.attribute_lists(node))),
        ];
        if let Some(base_list) = self.first_named_child_of_kind(node, &["base_list"]) {
            fields.push(("BaseList", self.base_list(base_list)));
        }
        self.node("EnumDeclaration", node, fields)
    }

    fn enum_member(&self, node: Node) -> Value {
        let init = self
            .named_children(node)
            .find(|child| child.kind() != "identifier" && child.kind() != "attribute_list")
            .map(|expr| self.equals_value_clause(expr));
        let mut fields = vec![
            (
                "Identifier",
                self.identifier_token(
                    node.child_by_field_name("name")
                        .or_else(|| self.first_named_child(node)),
                ),
            ),
            ("Modifiers", json!(Vec::<Value>::new())),
            ("AttributeLists", json!(self.attribute_lists(node))),
        ];
        if let Some(init) = init {
            fields.push(("Initializer", init));
        }
        self.node("EnumMemberDeclaration", node, fields)
    }

    fn field_declaration(&self, node: Node) -> Value {
        let declaration = self
            .first_named_child_of_kind(node, &["variable_declaration"])
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node(
            "FieldDeclaration",
            node,
            vec![
                ("Declaration", declaration),
                ("Modifiers", json!(self.modifiers(node))),
                ("AttributeLists", json!(self.attribute_lists(node))),
            ],
        )
    }

    fn property_declaration(&self, node: Node) -> Value {
        let accessors = node
            .child_by_field_name("accessors")
            .map(|n| self.accessor_list(n))
            .unwrap_or_else(|| self.accessor_list_from_arrow(node));
        self.node(
            "PropertyDeclaration",
            node,
            vec![
                (
                    "Identifier",
                    self.identifier_token(node.child_by_field_name("name")),
                ),
                (
                    "Type",
                    node.child_by_field_name("type")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                ("AccessorList", accessors),
                ("Modifiers", json!(self.modifiers(node))),
                ("AttributeLists", json!(self.attribute_lists(node))),
            ],
        )
    }

    fn constructor_declaration(&self, node: Node) -> Value {
        self.node(
            "ConstructorDeclaration",
            node,
            vec![
                (
                    "Identifier",
                    self.identifier_token(node.child_by_field_name("name")),
                ),
                (
                    "ParameterList",
                    self.parameter_list_or_empty(node.child_by_field_name("parameters"), node),
                ),
                (
                    "Body",
                    node.child_by_field_name("body")
                        .map(|n| self.block_or_arrow(n))
                        .unwrap_or(Value::Null),
                ),
                ("Modifiers", json!(self.modifiers(node))),
            ],
        )
    }

    fn method_declaration(&self, node: Node, kind: &'static str) -> Value {
        let return_type = node
            .child_by_field_name("returns")
            .or_else(|| node.child_by_field_name("type"))
            .map(|n| self.emit(n))
            .unwrap_or_else(|| self.node("PredefinedType", node, vec![]));
        self.node(
            kind,
            node,
            vec![
                (
                    "Identifier",
                    self.identifier_token(node.child_by_field_name("name")),
                ),
                (
                    "ParameterList",
                    self.parameter_list_or_empty(node.child_by_field_name("parameters"), node),
                ),
                ("ReturnType", return_type),
                (
                    "Body",
                    node.child_by_field_name("body")
                        .map(|n| self.block_or_arrow(n))
                        .unwrap_or(Value::Null),
                ),
                ("Modifiers", json!(self.modifiers(node))),
                ("AttributeLists", json!(self.attribute_lists(node))),
                ("TypeParameterList", self.type_parameter_list_field(node)),
                ("ConstraintClauses", json!(self.constraint_clauses(node))),
            ],
        )
    }

    fn block_or_arrow(&self, node: Node) -> Value {
        if node.kind() == "arrow_expression_clause" {
            let expr = self
                .first_named_child(node)
                .map(|n| self.return_statement_like(n))
                .unwrap_or(Value::Null);
            self.node("Block", node, vec![("Statements", json!(vec![expr]))])
        } else {
            self.block(node)
        }
    }

    fn block(&self, node: Node) -> Value {
        let statements = self
            .named_children(node)
            .filter(|child| child.kind() != "comment")
            .filter(|child| self.text(*child).trim() != ";")
            .map(|child| self.emit(child))
            .collect::<Vec<_>>();
        self.node("Block", node, vec![("Statements", json!(statements))])
    }

    fn local_declaration_statement(&self, node: Node) -> Value {
        let declaration = self
            .first_named_child_of_kind(node, &["variable_declaration"])
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node(
            "LocalDeclarationStatement",
            node,
            vec![
                ("Declaration", declaration),
                ("Modifiers", json!(self.modifiers(node))),
            ],
        )
    }

    fn variable_declaration(&self, node: Node) -> Value {
        let variables = self
            .named_children(node)
            .filter(|child| child.kind() == "variable_declarator")
            .map(|child| self.emit(child))
            .collect::<Vec<_>>();
        self.node(
            "VariableDeclaration",
            node,
            vec![
                (
                    "Type",
                    node.child_by_field_name("type")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                ("Variables", json!(variables)),
            ],
        )
    }

    fn variable_declarator(&self, node: Node) -> Value {
        let name = node
            .child_by_field_name("name")
            .or_else(|| self.first_named_child(node));
        let initializer = self
            .named_children(node)
            .find(|child| Some(*child) != name && child.kind() != "bracketed_argument_list")
            .map(|expr| self.equals_value_clause(expr))
            .unwrap_or(Value::Null);
        self.node(
            "VariableDeclarator",
            node,
            vec![
                ("Identifier", self.identifier_token(name)),
                ("Initializer", initializer),
            ],
        )
    }

    fn expression_statement(&self, node: Node) -> Value {
        let expression = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node(
            "ExpressionStatement",
            node,
            vec![("Expression", expression)],
        )
    }

    fn return_statement_like(&self, expr: Node) -> Value {
        self.node(
            "ReturnStatement",
            expr,
            vec![("Expression", self.emit(expr))],
        )
    }

    fn return_statement(&self, node: Node) -> Value {
        let expression = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("ReturnStatement", node, vec![("Expression", expression)])
    }

    fn throw_statement(&self, node: Node) -> Value {
        let expression = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("ThrowStatement", node, vec![("Expression", expression)])
    }

    fn if_statement(&self, node: Node) -> Value {
        let alternative = node.child_by_field_name("alternative").map(|n| {
            let statement = if n.kind() == "if_statement" {
                self.emit(n)
            } else {
                self.statement_as_block(n)
            };
            self.node("ElseClause", n, vec![("Statement", statement)])
        });
        self.node(
            "IfStatement",
            node,
            vec![
                (
                    "Condition",
                    node.child_by_field_name("condition")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                (
                    "Statement",
                    node.child_by_field_name("consequence")
                        .map(|n| self.statement_as_block(n))
                        .unwrap_or(Value::Null),
                ),
                ("Else", alternative.unwrap_or(Value::Null)),
            ],
        )
    }

    fn while_statement(&self, node: Node) -> Value {
        self.node(
            "WhileStatement",
            node,
            vec![
                (
                    "Condition",
                    node.child_by_field_name("condition")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                (
                    "Statement",
                    node.child_by_field_name("body")
                        .map(|n| self.statement_as_block(n))
                        .unwrap_or(Value::Null),
                ),
            ],
        )
    }

    fn do_statement(&self, node: Node) -> Value {
        self.node(
            "DoStatement",
            node,
            vec![
                (
                    "Condition",
                    node.child_by_field_name("condition")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                (
                    "Statement",
                    node.child_by_field_name("body")
                        .map(|n| self.statement_as_block(n))
                        .unwrap_or(Value::Null),
                ),
            ],
        )
    }

    fn for_statement(&self, node: Node) -> Value {
        let declaration = self
            .children_by_field_name(node, "initializer")
            .find(|n| n.kind() == "variable_declaration")
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        let incrementors = self
            .children_by_field_name(node, "update")
            .filter(|n| n.is_named())
            .map(|n| self.emit(n))
            .collect::<Vec<_>>();
        self.node(
            "ForStatement",
            node,
            vec![
                ("Declaration", declaration),
                (
                    "Condition",
                    node.child_by_field_name("condition")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                ("Incrementors", json!(incrementors)),
                (
                    "Statement",
                    node.child_by_field_name("body")
                        .map(|n| self.statement_as_block(n))
                        .unwrap_or(Value::Null),
                ),
            ],
        )
    }

    fn foreach_statement(&self, node: Node) -> Value {
        self.node(
            "ForEachStatement",
            node,
            vec![
                (
                    "Type",
                    node.child_by_field_name("type")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                (
                    "Identifier",
                    self.identifier_token(node.child_by_field_name("left")),
                ),
                (
                    "Expression",
                    node.child_by_field_name("right")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                (
                    "Statement",
                    node.child_by_field_name("body")
                        .map(|n| self.statement_as_block(n))
                        .unwrap_or(Value::Null),
                ),
            ],
        )
    }

    fn switch_statement(&self, node: Node) -> Value {
        let sections = node
            .child_by_field_name("body")
            .into_iter()
            .flat_map(|body| self.named_children(body))
            .filter(|child| child.kind() == "switch_section")
            .map(|child| self.switch_section(child))
            .collect::<Vec<_>>();
        self.node(
            "SwitchStatement",
            node,
            vec![
                (
                    "Expression",
                    node.child_by_field_name("value")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                ("Sections", json!(sections)),
            ],
        )
    }

    fn switch_section(&self, node: Node) -> Value {
        let children = self.named_children(node).collect::<Vec<_>>();
        let first_statement = children
            .iter()
            .position(|child| is_statement_node(child.kind()))
            .unwrap_or(children.len());
        // The label region precedes the first statement. A `when_clause` guards the
        // preceding pattern label and is not itself a label, so it is skipped.
        let mut labels = children
            .iter()
            .take(first_statement)
            .filter(|child| child.kind() != "when_clause")
            .map(|child| self.case_switch_label(*child))
            .collect::<Vec<_>>();
        if labels.is_empty() && self.text(node).trim_start().starts_with("default") {
            labels.push(self.default_switch_label(node));
        }
        let statements = children
            .iter()
            .skip(first_statement)
            .map(|child| self.emit(*child))
            .collect::<Vec<_>>();
        self.node(
            "SwitchSection",
            node,
            vec![("Labels", json!(labels)), ("Statements", json!(statements))],
        )
    }

    fn case_switch_label(&self, node: Node) -> Value {
        let start = node.start_position();
        let end = node.end_position();
        let value = self.emit(node);
        let code = format!("case {}:", self.text(node));
        let span_start = Point {
            row: start.row,
            column: start.column.saturating_sub("case ".len()),
        };
        let span_end = Point {
            row: end.row,
            column: end.column + 1,
        };
        // A bare `case <constant>:` (parsed as `constant_pattern` by tree-sitter) is a
        // `CaseSwitchLabel`; any richer pattern (`case Foo f:`, `case > 0:`, `case not
        // null:`, ...) is a `CasePatternSwitchLabel`.
        if is_constant_case_label(node.kind()) {
            self.synthetic_node(
                "CaseSwitchLabel",
                &code,
                span_start,
                span_end,
                vec![("Value", value)],
            )
        } else {
            self.synthetic_node(
                "CasePatternSwitchLabel",
                &code,
                span_start,
                span_end,
                vec![("Pattern", value)],
            )
        }
    }

    fn default_switch_label(&self, node: Node) -> Value {
        let start = node.start_position();
        self.synthetic_node(
            "DefaultSwitchLabel",
            "default:",
            start,
            Point {
                row: start.row,
                column: start.column + "default:".len(),
            },
            vec![],
        )
    }

    fn using_statement(&self, node: Node) -> Value {
        let declaration = self
            .first_named_child_of_kind(node, &["variable_declaration"])
            .map(|n| self.emit(n))
            .or_else(|| {
                self.named_children(node)
                    .find(|child| child.kind() != "block")
                    .map(|child| self.emit(child))
            })
            .unwrap_or(Value::Null);
        self.node(
            "UsingStatement",
            node,
            vec![
                ("Declaration", declaration),
                (
                    "Statement",
                    node.child_by_field_name("body")
                        .map(|n| self.statement_as_block(n))
                        .unwrap_or(Value::Null),
                ),
            ],
        )
    }

    fn try_statement(&self, node: Node) -> Value {
        let catches = self
            .named_children(node)
            .filter(|child| child.kind() == "catch_clause")
            .map(|child| self.catch_clause(child))
            .collect::<Vec<_>>();
        let finally = self
            .named_children(node)
            .find(|child| child.kind() == "finally_clause")
            .map(|child| self.finally_clause(child))
            .unwrap_or(Value::Null);
        self.node(
            "TryStatement",
            node,
            vec![
                (
                    "Block",
                    node.child_by_field_name("body")
                        .map(|n| self.block(n))
                        .unwrap_or(Value::Null),
                ),
                ("Catches", json!(catches)),
                ("Finally", finally),
            ],
        )
    }

    fn catch_clause(&self, node: Node) -> Value {
        let declaration = self
            .first_named_child_of_kind(node, &["catch_declaration"])
            .map(|n| self.catch_declaration(n))
            .unwrap_or(Value::Null);
        self.node(
            "CatchClause",
            node,
            vec![
                ("Declaration", declaration),
                (
                    "Block",
                    node.child_by_field_name("body")
                        .map(|n| self.block(n))
                        .unwrap_or(Value::Null),
                ),
            ],
        )
    }

    fn catch_declaration(&self, node: Node) -> Value {
        let typ = self
            .first_named_child_of_kind(node, &["identifier", "qualified_name", "predefined_type"]);
        let name = self.named_children(node).last();
        self.node(
            "CatchDeclaration",
            node,
            vec![
                ("Type", typ.map(|n| self.emit(n)).unwrap_or(Value::Null)),
                ("Identifier", self.identifier_token(name)),
            ],
        )
    }

    fn finally_clause(&self, node: Node) -> Value {
        let block = self
            .first_named_child_of_kind(node, &["block"])
            .map(|n| self.block(n))
            .unwrap_or(Value::Null);
        self.node("FinallyClause", node, vec![("Block", block)])
    }

    fn goto_statement(&self, node: Node) -> Value {
        self.node(
            "GotoStatement",
            node,
            vec![(
                "Expression",
                self.first_named_child(node)
                    .map(|n| self.emit(n))
                    .unwrap_or(Value::Null),
            )],
        )
    }

    fn labeled_statement(&self, node: Node) -> Value {
        let label = self.first_named_child_of_kind(node, &["identifier"]);
        let statement = self
            .named_children(node)
            .find(|child| child.kind() != "identifier")
            .map(|child| self.statement_as_block(child))
            .unwrap_or(Value::Null);
        self.node(
            "LabeledStatement",
            node,
            vec![
                ("Identifier", self.identifier_token(label)),
                ("Statement", statement),
            ],
        )
    }

    fn statement_as_block(&self, node: Node) -> Value {
        if node.kind() == "block" {
            self.block(node)
        } else {
            self.node(
                "Block",
                node,
                vec![("Statements", json!(vec![self.emit(node)]))],
            )
        }
    }

    fn identifier(&self, node: Node) -> Value {
        self.identifier_like("IdentifierName", node, self.text(node))
    }

    fn identifier_like(&self, kind: &'static str, node: Node, name: &str) -> Value {
        self.node(kind, node, vec![("Identifier", json!({ "Value": name }))])
    }

    fn qualified_name(&self, node: Node) -> Value {
        let mut named = self.named_children(node).collect::<Vec<_>>();
        if named.len() >= 2 {
            let right = named.pop().unwrap();
            let left = named.remove(0);
            self.node(
                "QualifiedName",
                node,
                vec![("Left", self.emit(left)), ("Right", self.emit(right))],
            )
        } else {
            self.identifier_like("IdentifierName", node, self.text(node))
        }
    }

    fn generic_name(&self, node: Node) -> Value {
        let name = node
            .child_by_field_name("name")
            .or_else(|| self.first_named_child(node));
        let args = self
            .first_named_child_of_kind(node, &["type_argument_list"])
            .map(|n| self.type_argument_list(n))
            .unwrap_or_else(|| self.node("TypeArgumentList", node, vec![]));
        self.node(
            "GenericName",
            node,
            vec![
                ("Identifier", self.identifier_token(name)),
                ("TypeArgumentList", args),
            ],
        )
    }

    fn type_argument_list(&self, node: Node) -> Value {
        let arguments = self
            .named_children(node)
            .map(|child| self.emit(child))
            .collect::<Vec<_>>();
        self.node(
            "TypeArgumentList",
            node,
            vec![("Arguments", json!(arguments))],
        )
    }

    /// Emits the generic *declaration* `<T, U>` of a type/method/delegate as a
    /// `TypeParameterList` holding `TypeParameter` nodes, mirroring Roslyn. Returns
    /// `Value::Null` when the declaration is non-generic.
    fn type_parameter_list_field(&self, node: Node) -> Value {
        let list = node
            .child_by_field_name("type_parameters")
            .or_else(|| self.first_named_child_of_kind(node, &["type_parameter_list"]));
        match list {
            Some(list) => {
                let parameters = self
                    .named_children(list)
                    .filter(|child| child.kind() == "type_parameter")
                    .map(|child| self.type_parameter(child))
                    .collect::<Vec<_>>();
                self.node(
                    "TypeParameterList",
                    list,
                    vec![("Parameters", json!(parameters))],
                )
            }
            None => Value::Null,
        }
    }

    fn type_parameter(&self, node: Node) -> Value {
        self.node(
            "TypeParameter",
            node,
            vec![
                (
                    "Identifier",
                    self.identifier_token(node.child_by_field_name("name")),
                ),
                ("Modifiers", json!(self.modifiers(node))),
                ("AttributeLists", json!(self.attribute_lists(node))),
            ],
        )
    }

    /// Emits the `where T : ...` constraint clauses attached to a generic declaration.
    fn constraint_clauses(&self, node: Node) -> Vec<Value> {
        self.named_children(node)
            .filter(|child| child.kind() == "type_parameter_constraints_clause")
            .map(|child| self.type_parameter_constraints_clause(child))
            .collect()
    }

    fn type_parameter_constraints_clause(&self, node: Node) -> Value {
        let name = self.first_named_child_of_kind(node, &["identifier"]);
        let constraints = self
            .named_children(node)
            .filter(|child| child.kind() == "type_parameter_constraint")
            .map(|child| self.type_parameter_constraint(child))
            .collect::<Vec<_>>();
        self.node(
            "TypeParameterConstraintClause",
            node,
            vec![
                ("Name", name.map(|n| self.emit(n)).unwrap_or(Value::Null)),
                ("Constraints", json!(constraints)),
            ],
        )
    }

    fn type_parameter_constraint(&self, node: Node) -> Value {
        let typ = node
            .child_by_field_name("type")
            .or_else(|| self.first_named_child(node))
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("TypeParameterConstraint", node, vec![("Type", typ)])
    }

    fn array_type(&self, node: Node) -> Value {
        let element = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("ArrayType", node, vec![("ElementType", element)])
    }

    fn nullable_type(&self, node: Node) -> Value {
        let element = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("NullableType", node, vec![("ElementType", element)])
    }

    fn equals_value_clause(&self, expr: Node) -> Value {
        self.node("EqualsValueClause", expr, vec![("Value", self.emit(expr))])
    }

    fn assignment_expression(&self, node: Node) -> Value {
        let op = node
            .child_by_field_name("operator")
            .map(|n| self.text(n))
            .unwrap_or("=");
        self.node(
            assignment_kind(op),
            node,
            vec![
                (
                    "Left",
                    node.child_by_field_name("left")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                (
                    "Right",
                    node.child_by_field_name("right")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                ("OperatorToken", json!({ "Value": op })),
            ],
        )
    }

    fn binary_expression(&self, node: Node) -> Value {
        let op = node
            .child_by_field_name("operator")
            .map(|n| self.text(n))
            .unwrap_or("");
        self.node(
            binary_kind(op),
            node,
            vec![
                (
                    "Left",
                    node.child_by_field_name("left")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                (
                    "Right",
                    node.child_by_field_name("right")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                ("OperatorToken", json!({ "Value": op })),
            ],
        )
    }

    fn unary_expression(&self, node: Node, postfix: bool) -> Value {
        let operand = self.named_children(node).next().unwrap_or(node);
        let op = self
            .children(node)
            .find(|child| !child.is_named() && !matches!(child.kind(), "(" | ")"))
            .map(|n| self.text(n))
            .unwrap_or("");
        let operand = self.emit(operand);
        let mut fields = vec![
            ("Operand", operand.clone()),
            ("OperatorToken", json!({ "Value": op })),
        ];
        if postfix && op == "!" {
            fields.push(("Expression", operand.clone()));
            fields.push(("Name", operand));
        }
        self.node(unary_kind(op, postfix), node, fields)
    }

    fn invocation_expression(&self, node: Node) -> Value {
        self.node(
            "InvocationExpression",
            node,
            vec![
                (
                    "Expression",
                    node.child_by_field_name("function")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                (
                    "ArgumentList",
                    node.child_by_field_name("arguments")
                        .map(|n| self.argument_list(n, "ArgumentList"))
                        .unwrap_or_else(|| {
                            self.node("ArgumentList", node, vec![("Arguments", json!([]))])
                        }),
                ),
            ],
        )
    }

    fn member_access_expression(&self, node: Node) -> Value {
        self.node(
            "SimpleMemberAccessExpression",
            node,
            vec![
                (
                    "Expression",
                    node.child_by_field_name("expression")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                (
                    "Name",
                    node.child_by_field_name("name")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
            ],
        )
    }

    fn conditional_access_expression(&self, node: Node) -> Value {
        let condition = node.child_by_field_name("condition");
        let when_not_null = self
            .named_children(node)
            .find(|child| Some(*child) != condition)
            .map(|child| self.emit(child))
            .unwrap_or(Value::Null);
        self.node(
            "ConditionalAccessExpression",
            node,
            vec![
                (
                    "Expression",
                    condition.map(|n| self.emit(n)).unwrap_or(Value::Null),
                ),
                ("WhenNotNull", when_not_null),
            ],
        )
    }

    fn member_binding_expression(&self, node: Node) -> Value {
        self.node(
            "MemberBindingExpression",
            node,
            vec![(
                "Name",
                node.child_by_field_name("name")
                    .map(|n| self.emit(n))
                    .unwrap_or(Value::Null),
            )],
        )
    }

    fn element_binding_expression(&self, node: Node) -> Value {
        self.node(
            "ElementAccessExpression",
            node,
            vec![
                ("Expression", Value::Null),
                (
                    "ArgumentList",
                    self.argument_list(node, "BracketedArgumentList"),
                ),
            ],
        )
    }

    fn argument_list(&self, node: Node, kind: &'static str) -> Value {
        let arguments = self
            .named_children(node)
            .filter(|child| matches!(child.kind(), "argument" | "attribute_argument"))
            .map(|child| match child.kind() {
                "attribute_argument" => self.attribute_argument(child),
                _ => self.argument(child),
            })
            .collect::<Vec<_>>();
        self.node(kind, node, vec![("Arguments", json!(arguments))])
    }

    fn argument(&self, node: Node) -> Value {
        let expression = self
            .named_children(node)
            .last()
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("Argument", node, vec![("Expression", expression)])
    }

    fn expression_element(&self, node: Node) -> Value {
        let expression = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("ExpressionElement", node, vec![("Expression", expression)])
    }

    fn attribute_argument(&self, node: Node) -> Value {
        let name = node.child_by_field_name("name");
        let expression = self
            .named_children(node)
            .find(|child| Some(*child) != name)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        let mut fields = vec![("Expression", expression)];
        if let Some(name) = name {
            fields.push((
                "NameEquals",
                self.node("NameEquals", name, vec![("Name", self.emit(name))]),
            ));
        }
        self.node("AttributeArgument", node, fields)
    }

    fn object_creation_expression(&self, node: Node) -> Value {
        self.node(
            "ObjectCreationExpression",
            node,
            vec![
                (
                    "Type",
                    node.child_by_field_name("type")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                (
                    "ArgumentList",
                    node.child_by_field_name("arguments")
                        .map(|n| self.argument_list(n, "ArgumentList"))
                        .unwrap_or_else(|| {
                            self.node("ArgumentList", node, vec![("Arguments", json!([]))])
                        }),
                ),
            ],
        )
    }

    fn anonymous_object_creation_expression(&self, node: Node) -> Value {
        let initializers = self.anonymous_object_initializers(node);
        self.node(
            "AnonymousObjectCreationExpression",
            node,
            vec![
                ("Identifier", json!({ "Value": "" })),
                ("Members", json!(Vec::<Value>::new())),
                ("Modifiers", json!(Vec::<Value>::new())),
                ("Initializers", json!(initializers)),
            ],
        )
    }

    fn anonymous_object_initializers(&self, node: Node) -> Vec<Value> {
        let named = self.named_children(node).collect::<Vec<_>>();
        let mut initializers = Vec::new();
        let mut idx = 0;
        while idx < named.len() {
            let current = named[idx];
            let after_current = self
                .source_between(current.end_byte(), node.end_byte())
                .trim_start();
            if current.kind() == "identifier" && after_current.starts_with('=') {
                if let Some(expr) = named.get(idx + 1).copied() {
                    initializers.push(self.anonymous_object_member(Some(current), expr));
                    idx += 2;
                } else {
                    initializers.push(self.anonymous_object_member(None, current));
                    idx += 1;
                }
            } else {
                initializers.push(self.anonymous_object_member(None, current));
                idx += 1;
            }
        }
        initializers
    }

    fn anonymous_object_member(&self, name: Option<Node>, expr: Node) -> Value {
        let start = name.unwrap_or(expr).start_position();
        let end = expr.end_position();
        let code = self
            .source_between(name.unwrap_or(expr).start_byte(), expr.end_byte())
            .trim()
            .to_string();
        let mut fields = vec![("Expression", self.emit(expr))];
        if let Some(name) = name {
            fields.push(("NameEquals", json!({ "Name": self.emit(name) })));
        }
        self.synthetic_node("AnonymousObjectMemberDeclarator", &code, start, end, fields)
    }

    fn cast_expression(&self, node: Node) -> Value {
        let mut named = self.named_children(node).collect::<Vec<_>>();
        let typ = named
            .first()
            .copied()
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        let expr = named.pop().map(|n| self.emit(n)).unwrap_or(Value::Null);
        self.node(
            "CastExpression",
            node,
            vec![("Type", typ), ("Expression", expr)],
        )
    }

    fn parenthesized_expression(&self, node: Node) -> Value {
        let expression = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node(
            "ParenthesizedExpression",
            node,
            vec![("Expression", expression)],
        )
    }

    fn element_access_expression(&self, node: Node) -> Value {
        self.node(
            "ElementAccessExpression",
            node,
            vec![
                (
                    "Expression",
                    node.child_by_field_name("expression")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                (
                    "ArgumentList",
                    node.child_by_field_name("subscript")
                        .map(|n| self.argument_list(n, "BracketedArgumentList"))
                        .unwrap_or(Value::Null),
                ),
            ],
        )
    }

    fn lambda_expression(&self, node: Node) -> Value {
        let params = self.first_named_child_of_kind(node, &["parameter_list"]);
        let param = self
            .first_named_child_of_kind(node, &["parameter"])
            .or_else(|| {
                self.first_named_child_of_kind(node, &["implicit_parameter", "identifier"])
            });
        let body = self
            .named_children(node)
            .last()
            .map(|n| {
                if n.kind() == "block" {
                    self.block(n)
                } else {
                    self.emit(n)
                }
            })
            .unwrap_or(Value::Null);
        if let Some(params) = params {
            self.node(
                "ParenthesizedLambdaExpression",
                node,
                vec![
                    ("ParameterList", self.parameter_list(params)),
                    ("Body", body),
                    ("Modifiers", json!(Vec::<Value>::new())),
                ],
            )
        } else {
            self.node(
                "SimpleLambdaExpression",
                node,
                vec![
                    (
                        "Parameter",
                        param
                            .map(|n| self.parameter_or_identifier(n))
                            .unwrap_or(Value::Null),
                    ),
                    ("Body", body),
                    ("Modifiers", json!(Vec::<Value>::new())),
                ],
            )
        }
    }

    fn initializer_expression(&self, node: Node, kind: &'static str) -> Value {
        let expressions = self
            .named_children(node)
            .map(|child| self.emit(child))
            .collect::<Vec<_>>();
        let key = if kind == "CollectionExpression" {
            "Elements"
        } else {
            "Expressions"
        };
        self.node(kind, node, vec![(key, json!(expressions))])
    }

    fn implicit_array_creation_expression(&self, node: Node) -> Value {
        self.node(
            "ImplicitArrayCreationExpression",
            node,
            vec![(
                "Initializer",
                self.first_named_child_of_kind(node, &["initializer_expression"])
                    .map(|n| self.initializer_expression(n, "ArrayInitializerExpression"))
                    .unwrap_or(Value::Null),
            )],
        )
    }

    fn array_creation_expression(&self, node: Node) -> Value {
        self.node(
            "ImplicitArrayCreationExpression",
            node,
            vec![(
                "Initializer",
                self.first_named_child_of_kind(node, &["initializer_expression"])
                    .map(|n| self.initializer_expression(n, "ArrayInitializerExpression"))
                    .unwrap_or(Value::Null),
            )],
        )
    }

    fn conditional_expression(&self, node: Node) -> Value {
        let named = self.named_children(node).collect::<Vec<_>>();
        self.node(
            "ConditionalExpression",
            node,
            vec![
                (
                    "Condition",
                    named.first().map(|n| self.emit(*n)).unwrap_or(Value::Null),
                ),
                (
                    "WhenTrue",
                    named.get(1).map(|n| self.emit(*n)).unwrap_or(Value::Null),
                ),
                (
                    "WhenFalse",
                    named.get(2).map(|n| self.emit(*n)).unwrap_or(Value::Null),
                ),
            ],
        )
    }

    fn await_expression(&self, node: Node) -> Value {
        let expr = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("AwaitExpression", node, vec![("Expression", expr)])
    }

    fn interpolated_string_expression(&self, node: Node) -> Value {
        let contents = self
            .named_children(node)
            .filter(|child| matches!(child.kind(), "string_content" | "interpolation"))
            .map(|child| self.emit(child))
            .collect::<Vec<_>>();
        self.node(
            "InterpolatedStringExpression",
            node,
            vec![("Contents", json!(contents))],
        )
    }

    fn interpolated_string_text(&self, node: Node) -> Value {
        self.node(
            "InterpolatedStringText",
            node,
            vec![("TextToken", json!({ "Value": self.text(node) }))],
        )
    }

    fn interpolation(&self, node: Node) -> Value {
        let expression = self
            .named_children(node)
            .find(|child| !matches!(child.kind(), "interpolation_brace"))
            .map(|child| self.emit(child))
            .unwrap_or(Value::Null);
        self.node("Interpolation", node, vec![("Expression", expression)])
    }

    fn is_pattern_expression(&self, node: Node) -> Value {
        self.node(
            "IsPatternExpression",
            node,
            vec![
                (
                    "Expression",
                    node.child_by_field_name("left")
                        .map(|n| self.emit(n))
                        .unwrap_or_else(|| {
                            self.named_children(node)
                                .next()
                                .map(|n| self.emit(n))
                                .unwrap_or(Value::Null)
                        }),
                ),
                (
                    "Pattern",
                    self.named_children(node)
                        .last()
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
            ],
        )
    }

    fn declaration_pattern(&self, node: Node) -> Value {
        let named = self.named_children(node).collect::<Vec<_>>();
        self.node(
            "DeclarationPattern",
            node,
            vec![
                (
                    "Type",
                    named.first().map(|n| self.emit(*n)).unwrap_or(Value::Null),
                ),
                (
                    "Designation",
                    named
                        .last()
                        .map(|n| self.single_variable_designation(*n))
                        .unwrap_or(Value::Null),
                ),
            ],
        )
    }

    fn single_variable_designation(&self, node: Node) -> Value {
        self.node(
            "SingleVariableDesignation",
            node,
            vec![("Identifier", json!({ "Value": self.text(node) }))],
        )
    }

    fn constant_pattern(&self, node: Node) -> Value {
        let expr = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("ConstantPattern", node, vec![("Expression", expr)])
    }

    fn declaration_expression(&self, node: Node) -> Value {
        node.child_by_field_name("name")
            .map(|n| self.emit(n))
            .unwrap_or_else(|| self.unknown(node))
    }

    fn negated_pattern(&self, node: Node) -> Value {
        let pattern = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("NegatedPattern", node, vec![("Pattern", pattern)])
    }

    fn relational_pattern(&self, node: Node) -> Value {
        let expression = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node(
            "RelationalPattern",
            node,
            vec![
                ("Expression", expression),
                (
                    "OperatorToken",
                    json!({ "Value": relational_pattern_operator(self.text(node)) }),
                ),
            ],
        )
    }

    fn parameter_list_or_empty(&self, node: Option<Node>, fallback: Node) -> Value {
        node.map(|n| self.parameter_list(n)).unwrap_or_else(|| {
            self.node("ParameterList", fallback, vec![("Parameters", json!([]))])
        })
    }

    fn parameter_list(&self, node: Node) -> Value {
        let params = self
            .named_children(node)
            .filter(|child| {
                matches!(
                    child.kind(),
                    "parameter" | "implicit_parameter" | "identifier"
                )
            })
            .map(|child| self.parameter_or_identifier(child))
            .collect::<Vec<_>>();
        self.node("ParameterList", node, vec![("Parameters", json!(params))])
    }

    fn parameter(&self, node: Node) -> Value {
        self.node(
            "Parameter",
            node,
            vec![
                (
                    "Identifier",
                    self.identifier_token(node.child_by_field_name("name")),
                ),
                (
                    "Type",
                    node.child_by_field_name("type")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                ("Modifiers", json!(self.modifiers(node))),
            ],
        )
    }

    fn parameter_or_identifier(&self, node: Node) -> Value {
        match node.kind() {
            "parameter" => self.parameter(node),
            "implicit_parameter" | "identifier" => self.node(
                "Parameter",
                node,
                vec![
                    ("Identifier", self.identifier_token(Some(node))),
                    ("Type", Value::Null),
                    ("Modifiers", json!(Vec::<Value>::new())),
                ],
            ),
            _ => self.emit(node),
        }
    }

    fn base_list(&self, node: Node) -> Value {
        let types = self
            .named_children(node)
            .filter(|child| child.kind() != "comment")
            .map(|child| self.node("SimpleBaseType", child, vec![("Type", self.emit(child))]))
            .collect::<Vec<_>>();
        self.node("BaseList", node, vec![("Types", json!(types))])
    }

    fn accessor_list(&self, node: Node) -> Value {
        let accessors = self
            .named_children(node)
            .filter(|child| child.kind() == "accessor_declaration")
            .map(|child| self.accessor_declaration(child))
            .collect::<Vec<_>>();
        self.node("AccessorList", node, vec![("Accessors", json!(accessors))])
    }

    fn accessor_list_from_arrow(&self, node: Node) -> Value {
        let accessors =
            vec![self.node("GetAccessorDeclaration", node, vec![("Body", Value::Null)])];
        self.node("AccessorList", node, vec![("Accessors", json!(accessors))])
    }

    fn accessor_declaration(&self, node: Node) -> Value {
        let keyword = self.text(node).trim_start();
        let kind = if keyword.starts_with("set") || keyword.starts_with("init") {
            "SetAccessorDeclaration"
        } else {
            "GetAccessorDeclaration"
        };
        self.node(
            kind,
            node,
            vec![
                (
                    "Body",
                    self.first_named_child_of_kind(node, &["block"])
                        .map(|n| self.block(n))
                        .unwrap_or(Value::Null),
                ),
                ("Modifiers", json!(self.modifiers(node))),
            ],
        )
    }

    fn attribute_lists(&self, node: Node) -> Vec<Value> {
        self.named_children(node)
            .filter(|child| child.kind() == "attribute_list")
            .map(|child| self.attribute_list(child))
            .collect()
    }

    fn attribute_list(&self, node: Node) -> Value {
        let attributes = self
            .named_children(node)
            .filter(|child| child.kind() == "attribute")
            .map(|child| self.attribute(child))
            .collect::<Vec<_>>();
        self.node(
            "AttributeList",
            node,
            vec![("Attributes", json!(attributes))],
        )
    }

    fn attribute(&self, node: Node) -> Value {
        self.node(
            "Attribute",
            node,
            vec![
                (
                    "Name",
                    self.first_named_child(node)
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                (
                    "ArgumentList",
                    self.first_named_child_of_kind(node, &["attribute_argument_list"])
                        .map(|n| self.argument_list(n, "AttributeArgumentList"))
                        .unwrap_or(Value::Null),
                ),
            ],
        )
    }

    fn modifiers(&self, node: Node) -> Vec<Value> {
        self.named_children(node)
            .filter(|child| child.kind() == "modifier")
            .map(|child| json!({ "Value": self.text(child) }))
            .collect()
    }

    fn identifier_token(&self, node: Option<Node>) -> Value {
        json!({ "Value": node.map(|n| self.text(n)).unwrap_or("") })
    }

    fn node(&self, kind: &'static str, node: Node, fields: Vec<(&'static str, Value)>) -> Value {
        let mut obj = Map::new();
        obj.insert("MetaData".to_string(), self.metadata(kind, node));
        for (key, value) in fields {
            obj.insert(key.to_string(), value);
        }
        Value::Object(obj)
    }

    /// Emits an `Unknown` node and records the originating tree-sitter kind so the
    /// CLI can report a single summary of unmapped node kinds at the end of a run.
    fn unknown(&self, node: Node) -> Value {
        *self
            .unmapped
            .borrow_mut()
            .entry(node.kind().to_string())
            .or_insert(0) += 1;
        self.node("Unknown", node, vec![])
    }

    fn metadata(&self, kind: &'static str, node: Node) -> Value {
        let start = node.start_position();
        let end = node.end_position();
        json!({
            "Kind": format!("{AST_PREFIX}{kind}"),
            "Code": self.text(node),
            "LineStart": line(start),
            "ColumnStart": start.column,
            "LineEnd": line(end),
            "ColumnEnd": end.column,
        })
    }

    fn recovered_error(&self, node: Node) -> Value {
        let code = self.text(node).trim();
        if let Some(expression) = self.synthetic_expression(node, code) {
            self.node(
                "ExpressionStatement",
                node,
                vec![("Expression", expression)],
            )
        } else {
            self.unknown(node)
        }
    }

    fn synthetic_expression(&self, node: Node, code: &str) -> Option<Value> {
        split_binary_expression(code).map(|(left, op, right)| {
            let start = node.start_position();
            let left_col = start.column + code.find(left).unwrap_or(0);
            let op_col = start.column + code.find(op).unwrap_or(0);
            let right_col = op_col
                + op.len()
                + code[code.find(op).unwrap_or(0) + op.len()..]
                    .find(right)
                    .unwrap_or(0);
            self.synthetic_node(
                binary_kind(op),
                code,
                start,
                Point {
                    row: start.row,
                    column: start.column + code.len(),
                },
                vec![
                    ("Left", self.synthetic_atom(left, start.row, left_col)),
                    ("Right", self.synthetic_atom(right, start.row, right_col)),
                    ("OperatorToken", json!({ "Value": op })),
                ],
            )
        })
    }

    fn has_unrecoverable_error(&self, node: Node) -> bool {
        if node.is_error() || node.kind() == "ERROR" {
            let code = self.text(node).trim();
            return !code.is_empty() && split_binary_expression(code).is_none();
        }
        self.children(node)
            .any(|child| self.has_unrecoverable_error(child))
    }

    fn synthetic_atom(&self, code: &str, row: usize, column: usize) -> Value {
        let kind = if code == "true" {
            "TrueLiteralExpression"
        } else if code == "false" {
            "FalseLiteralExpression"
        } else if code == "null" {
            "NullLiteralExpression"
        } else if code.starts_with('"') || code.starts_with('\'') {
            "StringLiteralExpression"
        } else if code.chars().all(|c| c.is_ascii_digit()) {
            "NumericLiteralExpression"
        } else {
            return self.synthetic_node(
                "IdentifierName",
                code,
                Point { row, column },
                Point {
                    row,
                    column: column + code.len(),
                },
                vec![("Identifier", json!({ "Value": code }))],
            );
        };
        self.synthetic_node(
            kind,
            code,
            Point { row, column },
            Point {
                row,
                column: column + code.len(),
            },
            vec![],
        )
    }

    fn synthetic_node(
        &self,
        kind: &'static str,
        code: &str,
        start: Point,
        end: Point,
        fields: Vec<(&'static str, Value)>,
    ) -> Value {
        let mut obj = Map::new();
        obj.insert(
            "MetaData".to_string(),
            json!({
                "Kind": format!("{AST_PREFIX}{kind}"),
                "Code": code,
                "LineStart": start.row,
                "ColumnStart": start.column,
                "LineEnd": end.row,
                "ColumnEnd": end.column,
            }),
        );
        for (key, value) in fields {
            obj.insert(key.to_string(), value);
        }
        Value::Object(obj)
    }

    fn text(&self, node: Node) -> &'a str {
        node.utf8_text(self.bytes).unwrap_or("")
    }

    fn source_between(&self, start: usize, end: usize) -> &'a str {
        std::str::from_utf8(&self.bytes[start..end]).unwrap_or("")
    }

    fn first_named_child<'tree>(&self, node: Node<'tree>) -> Option<Node<'tree>> {
        self.named_children(node).next()
    }

    fn first_named_child_of_kind<'tree>(
        &self,
        node: Node<'tree>,
        kinds: &[&str],
    ) -> Option<Node<'tree>> {
        self.named_children(node)
            .find(|child| kinds.contains(&child.kind()))
    }

    fn named_children<'tree>(
        &self,
        node: Node<'tree>,
    ) -> impl Iterator<Item = Node<'tree>> + 'tree {
        (0..node.named_child_count()).filter_map(move |idx| node.named_child(idx as u32))
    }

    fn children<'tree>(&self, node: Node<'tree>) -> impl Iterator<Item = Node<'tree>> + 'tree {
        (0..node.child_count()).filter_map(move |idx| node.child(idx as u32))
    }

    fn children_by_field_name<'tree>(
        &self,
        node: Node<'tree>,
        field_name: &'static str,
    ) -> impl Iterator<Item = Node<'tree>> + 'tree {
        (0..node.child_count()).filter_map(move |idx| {
            let idx = idx as u32;
            let child = node.child(idx)?;
            (node.field_name_for_child(idx) == Some(field_name)).then_some(child)
        })
    }
}

fn line(point: Point) -> usize {
    point.row
}

fn is_statement_node(kind: &str) -> bool {
    matches!(
        kind,
        "block"
            | "break_statement"
            | "continue_statement"
            | "do_statement"
            | "expression_statement"
            | "for_statement"
            | "foreach_statement"
            | "goto_statement"
            | "if_statement"
            | "labeled_statement"
            | "local_declaration_statement"
            | "local_function_statement"
            | "return_statement"
            | "switch_statement"
            | "throw_statement"
            | "try_statement"
            | "using_statement"
            | "while_statement"
    )
}

/// A `case` label is a plain constant label (`CaseSwitchLabel`) when tree-sitter parses
/// it as a bare constant; everything else is a pattern label (`CasePatternSwitchLabel`).
fn is_constant_case_label(kind: &str) -> bool {
    matches!(
        kind,
        "constant_pattern"
            | "integer_literal"
            | "real_literal"
            | "string_literal"
            | "verbatim_string_literal"
            | "raw_string_literal"
            | "character_literal"
            | "boolean_literal"
            | "null_literal"
            | "identifier"
            | "member_access_expression"
    )
}

fn namespace_for_type(type_name: &str) -> &str {
    type_name
        .rsplit_once('.')
        .map(|(namespace, _)| namespace)
        .unwrap_or("")
}

fn split_type_member(value: &str) -> Option<(&str, &str)> {
    value.rsplit_once('.')
}

fn split_method_signature(value: &str) -> (&str, Option<&str>) {
    if let Some(open_idx) = value.find('(') {
        let close_idx = value.rfind(')').unwrap_or(value.len());
        (&value[..open_idx], Some(&value[open_idx + 1..close_idx]))
    } else {
        (value, None)
    }
}

fn split_doc_parameters(value: &str) -> Vec<String> {
    let mut params = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (idx, ch) in value.char_indices() {
        match ch {
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                let param = value[start..idx].trim();
                if !param.is_empty() {
                    params.push(normalize_doc_type(param));
                }
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    let param = value[start..].trim();
    if !param.is_empty() {
        params.push(normalize_doc_type(param));
    }
    params
}

fn normalize_doc_type(value: &str) -> String {
    value
        .replace('{', "<")
        .replace('}', ">")
        .replace('@', "")
        .replace("`0", "T")
        .replace("`1", "T1")
}

fn split_binary_expression(code: &str) -> Option<(&str, &str, &str)> {
    const OPS: [&str; 18] = [
        "==", "!=", "&&", "||", ">=", "<=", ">>", "<<", ">", "<", "+", "-", "*", "/", "%", "&",
        "|", "^",
    ];
    OPS.iter().find_map(|op| {
        let idx = code.find(op)?;
        let left = code[..idx].trim();
        let right = code[idx + op.len()..].trim();
        (!left.is_empty() && !right.is_empty()).then_some((left, *op, right))
    })
}

fn assignment_kind(op: &str) -> &'static str {
    match op {
        "+=" => "AddAssignmentExpression",
        "-=" => "SubtractAssignmentExpression",
        "*=" => "MultiplyAssignmentExpression",
        "/=" => "DivideAssignmentExpression",
        "%=" => "ModuloAssignmentExpression",
        "&=" => "AndAssignmentExpression",
        "|=" => "OrAssignmentExpression",
        "^=" => "ExclusiveOrAssignmentExpression",
        ">>=" | ">>>=" => "RightShiftAssignmentExpression",
        "<<=" => "LeftShiftAssignmentExpression",
        _ => "SimpleAssignmentExpression",
    }
}

fn binary_kind(op: &str) -> &'static str {
    match op {
        "+" => "AddExpression",
        "-" => "SubtractExpression",
        "*" => "MultiplyExpression",
        "/" => "DivideExpression",
        "%" => "ModuloExpression",
        "==" => "EqualsExpression",
        "!=" => "NotEqualsExpression",
        "&&" => "LogicalAndExpression",
        "||" => "LogicalOrExpression",
        ">" => "GreaterThanExpression",
        "<" => "LessThanExpression",
        ">=" => "GreaterThanOrEqualExpression",
        "<=" => "LessThanOrEqualExpression",
        "&" => "BitwiseAndExpression",
        "|" => "BitwiseOrExpression",
        "^" => "ExclusiveOrExpression",
        _ => "NotHandledType",
    }
}

fn relational_pattern_operator(code: &str) -> &'static str {
    let trimmed = code.trim_start();
    if trimmed.starts_with(">=") {
        ">="
    } else if trimmed.starts_with("<=") {
        "<="
    } else if trimmed.starts_with('>') {
        ">"
    } else if trimmed.starts_with('<') {
        "<"
    } else {
        ""
    }
}

fn unary_kind(op: &str, postfix: bool) -> &'static str {
    match (op, postfix) {
        ("++", true) => "PostIncrementExpression",
        ("--", true) => "PostDecrementExpression",
        ("++", false) => "PreIncrementExpression",
        ("--", false) => "PreDecrementExpression",
        ("+", _) => "UnaryPlusExpression",
        ("-", _) => "UnaryMinusExpression",
        ("~", _) => "BitwiseNotExpression",
        ("!", true) => "SuppressNullableWarningExpression",
        ("!", false) => "LogicalNotExpression",
        ("&", _) => "AddressOfExpression",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_basic_compilation_unit() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "using System;\nclass C { int M() { return 1; } }\n",
        )
        .expect("json");
        assert_eq!(json["FileName"], "/tmp/Test.cs");
        assert_eq!(json["AstRoot"]["MetaData"]["Kind"], "ast.CompilationUnit");
        assert_eq!(json["AstRoot"]["Usings"].as_array().unwrap().len(), 1);
        assert_eq!(json["AstRoot"]["Members"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn emits_locals_and_binary_expressions() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M() { int a = 1; int b = a; a = a + 2; } }\n",
        )
        .expect("json");
        let class = &json["AstRoot"]["Members"][0];
        let method = &class["Members"][0];
        let statements = method["Body"]["Statements"].as_array().unwrap();
        assert_eq!(
            statements[0]["MetaData"]["Kind"],
            "ast.LocalDeclarationStatement"
        );
        assert_eq!(
            statements[1]["Declaration"]["Variables"][0]["Initializer"]["Value"]["MetaData"]
                ["Kind"],
            "ast.IdentifierName"
        );
        assert_eq!(
            statements[2]["Expression"]["Right"]["MetaData"]["Kind"],
            "ast.AddExpression"
        );
    }

    #[test]
    fn distinguishes_get_and_set_accessors() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { int P { get; set; } int Q { get; init; } }\n",
        )
        .expect("json");
        let members = json["AstRoot"]["Members"][0]["Members"].as_array().unwrap();

        assert_eq!(
            members[0]["AccessorList"]["Accessors"][0]["MetaData"]["Kind"],
            "ast.GetAccessorDeclaration"
        );
        assert_eq!(
            members[0]["AccessorList"]["Accessors"][1]["MetaData"]["Kind"],
            "ast.SetAccessorDeclaration"
        );
        assert_eq!(
            members[1]["AccessorList"]["Accessors"][1]["MetaData"]["Kind"],
            "ast.SetAccessorDeclaration"
        );
    }

    #[test]
    fn emits_lambda_identifier_parameters() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M() { System.Func<int, int> f = x => x + 1; var g = (x, y) => x + y; } }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();
        let simple_lambda = &statements[0]["Declaration"]["Variables"][0]["Initializer"]["Value"];
        let parenthesized_lambda =
            &statements[1]["Declaration"]["Variables"][0]["Initializer"]["Value"];

        assert_eq!(
            simple_lambda["MetaData"]["Kind"],
            "ast.SimpleLambdaExpression"
        );
        assert_eq!(
            simple_lambda["Parameter"]["MetaData"]["Kind"],
            "ast.Parameter"
        );
        assert_eq!(simple_lambda["Parameter"]["Identifier"]["Value"], "x");
        assert_eq!(
            parenthesized_lambda["ParameterList"]["Parameters"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn emits_conditional_access() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M(dynamic baz) { baz?.Qux(); } }\n",
        )
        .expect("json");
        let expression = &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0]
            ["Expression"]["Expression"];

        assert_eq!(
            expression["MetaData"]["Kind"],
            "ast.ConditionalAccessExpression"
        );
        assert_eq!(
            expression["WhenNotNull"]["MetaData"]["Kind"],
            "ast.MemberBindingExpression"
        );
    }

    #[test]
    fn distinguishes_null_forgiving_from_logical_not() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M(dynamic baz, bool flag) { var a = baz!.Qux(); var b = !flag; } }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();
        let null_forgiving_base = &statements[0]["Declaration"]["Variables"][0]["Initializer"]
            ["Value"]["Expression"]["Expression"];
        let logical_not = &statements[1]["Declaration"]["Variables"][0]["Initializer"]["Value"];

        assert_eq!(
            null_forgiving_base["MetaData"]["Kind"],
            "ast.SuppressNullableWarningExpression"
        );
        assert_eq!(null_forgiving_base["Operand"]["Identifier"]["Value"], "baz");
        assert_eq!(
            null_forgiving_base["Expression"]["Identifier"]["Value"],
            "baz"
        );
        assert_eq!(null_forgiving_base["Name"]["Identifier"]["Value"], "baz");

        assert_eq!(logical_not["MetaData"]["Kind"], "ast.LogicalNotExpression");
        assert_eq!(logical_not["Operand"]["Identifier"]["Value"], "flag");
    }

    #[test]
    fn emits_switch_sections_and_labels() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M(int i) { switch (i) { case > 0: i++; break; case 10: i--; break; default: i += 10; break; } } }\n",
        )
        .expect("json");
        let switch = &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0];

        assert_eq!(switch["MetaData"]["Kind"], "ast.SwitchStatement");
        assert_eq!(switch["Sections"].as_array().unwrap().len(), 3);
        // `case > 0:` is a relational pattern -> CasePatternSwitchLabel.
        assert_eq!(
            switch["Sections"][0]["Labels"][0]["MetaData"]["Kind"],
            "ast.CasePatternSwitchLabel"
        );
        assert_eq!(
            switch["Sections"][0]["Labels"][0]["MetaData"]["Code"],
            "case > 0:"
        );
        assert_eq!(
            switch["Sections"][0]["Labels"][0]["Pattern"]["MetaData"]["Kind"],
            "ast.RelationalPattern"
        );
        assert_eq!(
            switch["Sections"][0]["Labels"][0]["Pattern"]["OperatorToken"]["Value"],
            ">"
        );
        assert_eq!(
            switch["Sections"][0]["Labels"][0]["Pattern"]["Expression"]["MetaData"]["Kind"],
            "ast.NumericLiteralExpression"
        );
        // `case 10:` is a bare constant -> CaseSwitchLabel.
        assert_eq!(
            switch["Sections"][1]["Labels"][0]["MetaData"]["Kind"],
            "ast.CaseSwitchLabel"
        );
        assert_eq!(
            switch["Sections"][1]["Labels"][0]["Value"]["MetaData"]["Kind"],
            "ast.ConstantPattern"
        );
        assert_eq!(
            switch["Sections"][2]["Labels"][0]["MetaData"]["Kind"],
            "ast.DefaultSwitchLabel"
        );
    }

    #[test]
    fn emits_negated_patterns() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M(string line) { if (line is not null) { } } }\n",
        )
        .expect("json");
        let pattern = &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0]
            ["Condition"]["Pattern"];

        assert_eq!(pattern["MetaData"]["Kind"], "ast.NegatedPattern");
        assert_eq!(
            pattern["Pattern"]["MetaData"]["Kind"],
            "ast.ConstantPattern"
        );
        assert_eq!(
            pattern["Pattern"]["Expression"]["MetaData"]["Kind"],
            "ast.NullLiteralExpression"
        );
    }

    #[test]
    fn emits_out_declaration_arguments_as_identifiers() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M(string line) { if (int.TryParse(line, out int number)) { } } }\n",
        )
        .expect("json");
        let out_argument = &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0]
            ["Condition"]["ArgumentList"]["Arguments"][1];

        assert_eq!(
            out_argument["Expression"]["MetaData"]["Kind"],
            "ast.IdentifierName"
        );
        assert_eq!(out_argument["Expression"]["Identifier"]["Value"], "number");
    }

    #[test]
    fn emits_using_and_goto_statements() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M() { using (Reader reader = File.OpenText(\"numbers.txt\")) { reader.ReadLine(); } goto End; End: reader.ReadLine(); } }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();

        assert_eq!(statements[0]["MetaData"]["Kind"], "ast.UsingStatement");
        assert_eq!(
            statements[0]["Declaration"]["MetaData"]["Kind"],
            "ast.VariableDeclaration"
        );
        assert_eq!(statements[0]["Statement"]["MetaData"]["Kind"], "ast.Block");
        assert_eq!(statements[1]["MetaData"]["Kind"], "ast.GotoStatement");
        assert_eq!(
            statements[1]["Expression"]["MetaData"]["Kind"],
            "ast.IdentifierName"
        );
        assert_eq!(statements[1]["Expression"]["Identifier"]["Value"], "End");
        assert_eq!(statements[2]["MetaData"]["Kind"], "ast.LabeledStatement");
        assert_eq!(statements[2]["Identifier"]["Value"], "End");
        assert_eq!(
            statements[2]["Statement"]["Statements"][0]["MetaData"]["Kind"],
            "ast.ExpressionStatement"
        );
    }

    #[test]
    fn emits_collection_and_implicit_array_expressions() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M() { var list = [[1, 2], [3, 4]]; var arr = new[] {1, 2, 3}; } }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();
        let collection = &statements[0]["Declaration"]["Variables"][0]["Initializer"]["Value"];
        let implicit_array = &statements[1]["Declaration"]["Variables"][0]["Initializer"]["Value"];

        assert_eq!(collection["MetaData"]["Kind"], "ast.CollectionExpression");
        // Collection elements are wrapped in ExpressionElement (Roslyn shape); the inner
        // collection literal sits on the element's Expression.
        assert_eq!(
            collection["Elements"][0]["MetaData"]["Kind"],
            "ast.ExpressionElement"
        );
        let inner = &collection["Elements"][0]["Expression"];
        assert_eq!(inner["MetaData"]["Kind"], "ast.CollectionExpression");
        assert_eq!(
            inner["Elements"][0]["MetaData"]["Kind"],
            "ast.ExpressionElement"
        );
        assert_eq!(
            inner["Elements"][0]["Expression"]["MetaData"]["Kind"],
            "ast.NumericLiteralExpression"
        );
        assert_eq!(
            implicit_array["MetaData"]["Kind"],
            "ast.ImplicitArrayCreationExpression"
        );
        assert_eq!(
            implicit_array["Initializer"]["MetaData"]["Kind"],
            "ast.ArrayInitializerExpression"
        );
    }

    #[test]
    fn rejects_unrecoverable_parse_errors() {
        let result = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M() { Console.WriteLi\"Broken\" } }\n",
        );

        assert!(result.is_err());
    }

    #[test]
    fn emits_summary_from_xml_docs() {
        let summary = generate_xml_summary(
            r#"
            <doc>
              <members>
                <member name="T:CommandLine.Core.Token" />
                <member name="M:CommandLine.Core.Token.Value(System.String,System.Int32)" />
                <member name="P:CommandLine.Core.Token.Text" />
              </members>
            </doc>
            "#,
        )
        .expect("summary");
        let typ = &summary["CommandLine.Core"][0];

        assert_eq!(typ["name"], "CommandLine.Core.Token");
        assert_eq!(typ["methods"][0]["name"], "Value");
        assert_eq!(
            typ["methods"][0]["parameterTypes"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(typ["fields"][0]["name"], "Text");
    }

    #[test]
    fn emits_type_parameters_on_class_with_constraints() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class Box<T, U> where T : class where U : struct { }\n",
        )
        .expect("json");
        let class = &json["AstRoot"]["Members"][0];

        assert_eq!(class["MetaData"]["Kind"], "ast.ClassDeclaration");
        assert_eq!(
            class["TypeParameterList"]["MetaData"]["Kind"],
            "ast.TypeParameterList"
        );
        let params = class["TypeParameterList"]["Parameters"].as_array().unwrap();
        assert_eq!(params.len(), 2);
        assert_eq!(params[0]["MetaData"]["Kind"], "ast.TypeParameter");
        assert_eq!(params[0]["Identifier"]["Value"], "T");
        assert_eq!(params[1]["Identifier"]["Value"], "U");

        let clauses = class["ConstraintClauses"].as_array().unwrap();
        assert_eq!(clauses.len(), 2);
        assert_eq!(
            clauses[0]["MetaData"]["Kind"],
            "ast.TypeParameterConstraintClause"
        );
        assert_eq!(clauses[0]["Name"]["Identifier"]["Value"], "T");
        assert_eq!(
            clauses[0]["Constraints"][0]["MetaData"]["Kind"],
            "ast.TypeParameterConstraint"
        );
    }

    #[test]
    fn emits_type_parameters_on_method() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { T Echo<T>(T value) where T : new() => value; }\n",
        )
        .expect("json");
        let method = &json["AstRoot"]["Members"][0]["Members"][0];

        assert_eq!(method["MetaData"]["Kind"], "ast.MethodDeclaration");
        assert_eq!(
            method["TypeParameterList"]["MetaData"]["Kind"],
            "ast.TypeParameterList"
        );
        assert_eq!(
            method["TypeParameterList"]["Parameters"][0]["MetaData"]["Kind"],
            "ast.TypeParameter"
        );
        assert_eq!(
            method["TypeParameterList"]["Parameters"][0]["Identifier"]["Value"],
            "T"
        );
        assert_eq!(
            method["ConstraintClauses"][0]["MetaData"]["Kind"],
            "ast.TypeParameterConstraintClause"
        );
    }

    #[test]
    fn omits_type_parameter_list_for_non_generic_declarations() {
        let json =
            generate_source(Path::new("/tmp/Test.cs"), "class C { void M() { } }\n").expect("json");
        let class = &json["AstRoot"]["Members"][0];
        assert!(class["TypeParameterList"].is_null());
        assert_eq!(class["ConstraintClauses"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn emits_case_pattern_switch_label_for_declaration_pattern() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M(object o) { switch (o) { case string s: break; case 1: break; } } }\n",
        )
        .expect("json");
        let sections = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0]
            ["Sections"]
            .as_array()
            .unwrap();

        let pattern_label = &sections[0]["Labels"][0];
        assert_eq!(
            pattern_label["MetaData"]["Kind"],
            "ast.CasePatternSwitchLabel"
        );
        assert_eq!(
            pattern_label["Pattern"]["MetaData"]["Kind"],
            "ast.DeclarationPattern"
        );

        // A bare constant stays a CaseSwitchLabel.
        assert_eq!(
            sections[1]["Labels"][0]["MetaData"]["Kind"],
            "ast.CaseSwitchLabel"
        );
    }

    #[test]
    fn emits_attribute_arguments() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "[Obsolete(\"use Bar\", true)] class Foo { }\n",
        )
        .expect("json");
        let attribute = &json["AstRoot"]["Members"][0]["AttributeLists"][0]["Attributes"][0];
        let arguments = attribute["ArgumentList"]["Arguments"].as_array().unwrap();

        assert_eq!(arguments.len(), 2);
        assert_eq!(arguments[0]["MetaData"]["Kind"], "ast.AttributeArgument");
        assert_eq!(
            arguments[0]["Expression"]["MetaData"]["Kind"],
            "ast.StringLiteralExpression"
        );
        assert_eq!(
            arguments[1]["Expression"]["MetaData"]["Kind"],
            "ast.TrueLiteralExpression"
        );
    }

    #[test]
    fn emits_named_attribute_argument() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "[Example(Name = \"x\")] class Foo { }\n",
        )
        .expect("json");
        let argument = &json["AstRoot"]["Members"][0]["AttributeLists"][0]["Attributes"][0]
            ["ArgumentList"]["Arguments"][0];

        assert_eq!(argument["MetaData"]["Kind"], "ast.AttributeArgument");
        assert_eq!(argument["NameEquals"]["MetaData"]["Kind"], "ast.NameEquals");
        assert_eq!(
            argument["NameEquals"]["Name"]["Identifier"]["Value"],
            "Name"
        );
    }

    #[test]
    fn emits_expression_element_in_collection() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M() { var xs = [1, 2]; } }\n",
        )
        .expect("json");
        let element = &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0]
            ["Declaration"]["Variables"][0]["Initializer"]["Value"]["Elements"][0];

        assert_eq!(element["MetaData"]["Kind"], "ast.ExpressionElement");
        assert_eq!(
            element["Expression"]["MetaData"]["Kind"],
            "ast.NumericLiteralExpression"
        );
    }

    #[test]
    fn records_unmapped_node_kinds_in_summary() {
        // Drain any residual counts from earlier tests sharing this thread.
        let _ = take_unmapped_summary();
        // `delegate_declaration` has no dedicated mapping and falls through to `Unknown`.
        let _ =
            generate_source(Path::new("/tmp/Test.cs"), "delegate void D(int x);\n").expect("json");

        let summary = take_unmapped_summary().expect("expected an unmapped summary");
        assert!(
            summary.starts_with("dotnetastgen: "),
            "unexpected summary: {summary}"
        );
        assert!(
            summary.contains("delegate_declaration(x1)"),
            "unexpected summary: {summary}"
        );
        // The counter is drained on read.
        assert!(take_unmapped_summary().is_none());
    }
}
