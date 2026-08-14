use anyhow::{anyhow, Result};
use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};
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
/// e.g. `dotnetastgen: 3 unmapped node(s): skipped_tokens(x2), goto_case(x1)`.
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
                        let name = attr.decoded_and_normalized_value(
                            XmlVersion::Implicit1_0,
                            reader.decoder(),
                        )?;
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
            .filter(|child| matches!(child.kind(), "using_directive" | "extern_alias_directive"))
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
                .filter(|child| {
                    !matches!(
                        child.kind(),
                        "using_directive" | "extern_alias_directive" | "comment"
                    )
                })
                .filter_map(|child| self.emit_member(*child))
                .collect::<Vec<_>>();
            vec![self.namespace_declaration_with_members(*file_scoped, true, namespace_members)]
        } else {
            children
                .iter()
                .filter(|child| {
                    !matches!(child.kind(), "using_directive" | "extern_alias_directive")
                })
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
            "extern_alias_directive" => self.extern_alias_directive(node),
            "global_attribute" => self.global_attribute(node),
            "shebang_directive" => self.preprocessor_directive(node, "ShebangDirective"),
            "preproc_if" => self.preprocessor_branch(node, "PreprocessorIfDirective"),
            "preproc_elif" => self.preprocessor_branch(node, "PreprocessorElifDirective"),
            "preproc_else" => self.preprocessor_branch(node, "PreprocessorElseDirective"),
            "preproc_define"
            | "preproc_endregion"
            | "preproc_error"
            | "preproc_if_in_attribute_list"
            | "preproc_line"
            | "preproc_nullable"
            | "preproc_pragma"
            | "preproc_region"
            | "preproc_undef"
            | "preproc_warning" => self.preprocessor_directive(node, "PreprocessorDirective"),
            "namespace_declaration" => self.namespace_declaration(node, false),
            "file_scoped_namespace_declaration" => self.namespace_declaration(node, true),
            "global_statement" => self.global_statement(node),
            "class_declaration" => self.type_declaration(node, "ClassDeclaration"),
            "struct_declaration" => self.type_declaration(node, "StructDeclaration"),
            "interface_declaration" => self.type_declaration(node, "InterfaceDeclaration"),
            "record_declaration" => self.type_declaration(node, "RecordDeclaration"),
            "delegate_declaration" => self.delegate_declaration(node),
            "enum_declaration" => self.enum_declaration(node),
            "enum_member_declaration" => self.enum_member(node),
            "field_declaration" => self.field_declaration(node),
            "event_field_declaration" => {
                self.field_declaration_with_kind(node, "EventFieldDeclaration")
            }
            "event_declaration" => self.event_declaration(node),
            "property_declaration" => self.property_declaration(node),
            "indexer_declaration" => self.indexer_declaration(node),
            "constructor_declaration" => self.constructor_declaration(node),
            "method_declaration" => self.method_declaration(node, "MethodDeclaration"),
            "operator_declaration" => self.operator_declaration(node),
            "conversion_operator_declaration" => self.conversion_operator_declaration(node),
            "destructor_declaration" => self.destructor_declaration(node),
            "local_function_statement" => self.method_declaration(node, "LocalFunctionStatement"),
            "block" => self.block(node),
            "local_declaration_statement" => self.local_declaration_statement(node),
            "variable_declaration" => self.variable_declaration(node),
            "variable_declarator" => self.variable_declarator(node),
            "empty_statement" => self.node("EmptyStatement", node, vec![]),
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
            "catch_filter_clause" => self.catch_filter_clause(node),
            "goto_statement" => self.goto_statement(node),
            "labeled_statement" => self.labeled_statement(node),
            "lock_statement" => self.lock_statement(node),
            "checked_statement" => self.checked_statement(node),
            "unsafe_statement" => self.unsafe_statement(node),
            "fixed_statement" => self.fixed_statement(node),
            "yield_statement" => self.yield_statement(node),
            "identifier" => self.identifier(node),
            "qualified_name" | "alias_qualified_name" => self.qualified_name(node),
            "generic_name" => self.generic_name(node),
            "predefined_type" => self.node("PredefinedType", node, vec![]),
            "implicit_type" => self.identifier_like("IdentifierName", node, self.text(node)),
            "array_type" => self.array_type(node),
            "array_rank_specifier" => self.array_rank_specifier(node),
            "pointer_type" => self.pointer_type(node),
            "function_pointer_type" => self.function_pointer_type(node),
            "function_pointer_parameter" => self.function_pointer_parameter(node),
            "nullable_type" => self.nullable_type(node),
            "ref_type" => self.ref_type(node),
            "scoped_type" => self.scoped_type(node),
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
            "as_expression" => self.binary_type_expression(node, "AsExpression"),
            "is_expression" => self.binary_type_expression(node, "IsExpression"),
            "range_expression" => self.range_expression(node),
            "query_expression" => self.query_expression(node),
            "from_clause" => self.query_from_clause(node),
            "join_clause" => self.join_clause(node),
            "join_into_clause" => self.join_into_clause(node),
            "let_clause" => self.let_clause(node),
            "order_by_clause" => self.order_by_clause(node),
            "where_clause" => self.where_clause(node),
            "select_clause" => self.select_clause(node),
            "group_clause" => self.group_clause(node),
            "switch_expression" => self.switch_expression(node),
            "switch_expression_arm" => self.switch_expression_arm(node),
            "when_clause" => self.when_clause(node),
            "with_expression" => self.with_expression(node),
            "with_initializer" => self.with_initializer(node),
            "prefix_unary_expression" => self.unary_expression(node, false),
            "postfix_unary_expression" => self.unary_expression(node, true),
            "_pointer_indirection_expression" => self.unary_expression(node, false),
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
            "typeof_expression" => self.type_operand_expression(node, "TypeOfExpression", false),
            "sizeof_expression" => self.type_operand_expression(node, "SizeOfExpression", false),
            "default_expression" => self.type_operand_expression(node, "DefaultExpression", true),
            "throw_expression" => self.throw_expression(node, "ThrowExpression"),
            "ref_expression" => self.unary_child_expression(node, "RefExpression"),
            "makeref_expression" => self.unary_child_expression(node, "MakeRefExpression"),
            "reftype_expression" => self.unary_child_expression(node, "RefTypeExpression"),
            "refvalue_expression" => self.refvalue_expression(node),
            "checked_expression" => self.checked_expression(node),
            "parenthesized_expression" => self.parenthesized_expression(node),
            "element_access_expression" => self.element_access_expression(node),
            "this" => self.node("ThisExpression", node, vec![]),
            "base" => self.node("BaseExpression", node, vec![]),
            "lambda_expression" => self.lambda_expression(node),
            "anonymous_method_expression" => self.anonymous_method_expression(node),
            "implicit_array_creation_expression" => self.implicit_array_creation_expression(node),
            "array_creation_expression" => self.array_creation_expression(node),
            "stackalloc_expression" | "implicit_stackalloc_expression" => {
                self.stackalloc_expression(node)
            }
            "initializer_expression" => {
                self.initializer_expression(node, "ArrayInitializerExpression")
            }
            "collection_expression" => self.initializer_expression(node, "CollectionExpression"),
            "tuple_expression" => self.tuple_expression(node),
            "tuple_type" => self.tuple_type(node),
            "tuple_element" => self.tuple_element(node),
            "expression_element" => self.expression_element(node),
            "spread_element" => self.unary_child_expression(node, "SpreadElement"),
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
            "constructor_initializer" => self.constructor_initializer(node),
            "explicit_interface_specifier" => self.explicit_interface_specifier(node),
            "primary_constructor_base_type" => self.primary_constructor_base_type(node),
            "declaration_pattern" => self.declaration_pattern(node),
            "constant_pattern" => self.constant_pattern(node),
            "discard" => self.node("DiscardPattern", node, vec![]),
            "negated_pattern" => self.negated_pattern(node),
            "relational_pattern" => self.relational_pattern(node),
            "and_pattern" => self.binary_pattern(node, "AndPattern", "and"),
            "or_pattern" => self.binary_pattern(node, "OrPattern", "or"),
            "parenthesized_pattern" => self.parenthesized_pattern(node),
            "list_pattern" => self.list_pattern(node),
            "recursive_pattern" => self.recursive_pattern(node),
            "type_pattern" => self.type_pattern(node),
            "var_pattern" => self.var_pattern(node),
            "tuple_pattern" => self.tuple_pattern(node),
            "parenthesized_variable_designation" => self.parenthesized_variable_designation(node),
            "subpattern" => self.subpattern(node),
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
        let alias = node.child_by_field_name("name");
        let name = alias
            .and_then(|alias| {
                self.named_children(node)
                    .find(|child| child.start_byte() > alias.end_byte())
            })
            .or_else(|| {
                self.first_named_child_of_kind(
                    node,
                    &[
                        "qualified_name",
                        "alias_qualified_name",
                        "identifier",
                        "generic_name",
                    ],
                )
            });
        let mut fields = vec![
            ("Name", name.map(|n| self.emit(n)).unwrap_or(Value::Null)),
            ("Static", json!(self.has_direct_child_token(node, "static"))),
            ("Unsafe", json!(self.has_direct_child_token(node, "unsafe"))),
            ("Global", json!(self.has_direct_child_token(node, "global"))),
        ];
        if let Some(alias) = alias {
            fields.push(("Alias", self.emit(alias)));
        }
        self.node("UsingDirective", node, fields)
    }

    fn extern_alias_directive(&self, node: Node) -> Value {
        let name = node
            .child_by_field_name("name")
            .or_else(|| self.first_named_child_of_kind(node, &["identifier"]));
        self.node(
            "ExternAliasDirective",
            node,
            vec![("Name", name.map(|n| self.emit(n)).unwrap_or(Value::Null))],
        )
    }

    fn global_attribute(&self, node: Node) -> Value {
        let attributes = self
            .named_children(node)
            .filter(|child| child.kind() == "attribute")
            .map(|child| self.attribute(child))
            .collect::<Vec<_>>();
        let attribute_lists = if attributes.is_empty() {
            Vec::new()
        } else {
            vec![self.node(
                "AttributeList",
                node,
                vec![
                    ("Target", self.global_attribute_target(node)),
                    ("Attributes", json!(attributes)),
                ],
            )]
        };
        self.node(
            "GlobalAttribute",
            node,
            vec![("AttributeLists", json!(attribute_lists))],
        )
    }

    fn preprocessor_directive(&self, node: Node, kind: &'static str) -> Value {
        self.node(kind, node, vec![])
    }

    fn preprocessor_branch(&self, node: Node, kind: &'static str) -> Value {
        let directive_line = node.start_position().row;
        let members = self
            .named_children(node)
            .filter(|child| child.start_position().row > directive_line)
            .filter(|child| !matches!(child.kind(), "comment" | "preproc_arg"))
            .map(|child| self.emit(child))
            .collect::<Vec<_>>();
        self.node(kind, node, vec![("Members", json!(members))])
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
        self.field_declaration_with_kind(node, "FieldDeclaration")
    }

    fn field_declaration_with_kind(&self, node: Node, kind: &'static str) -> Value {
        let declaration = self
            .first_named_child_of_kind(node, &["variable_declaration"])
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node(
            kind,
            node,
            vec![
                ("Declaration", declaration),
                ("Modifiers", json!(self.modifiers(node))),
                ("AttributeLists", json!(self.attribute_lists(node))),
            ],
        )
    }

    fn event_declaration(&self, node: Node) -> Value {
        let accessors = node
            .child_by_field_name("accessors")
            .or_else(|| self.first_named_child_of_kind(node, &["accessor_list"]))
            .map(|n| self.accessor_list(n))
            .unwrap_or_else(|| self.accessor_list_from_arrow(node));
        let explicit_interface_specifier = self
            .first_named_child_of_kind(node, &["explicit_interface_specifier"])
            .map(|n| self.explicit_interface_specifier(n))
            .unwrap_or(Value::Null);
        self.node(
            "EventDeclaration",
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
                ("ExplicitInterfaceSpecifier", explicit_interface_specifier),
                ("AccessorList", accessors),
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
        let explicit_interface_specifier = self
            .first_named_child_of_kind(node, &["explicit_interface_specifier"])
            .map(|n| self.explicit_interface_specifier(n))
            .unwrap_or(Value::Null);
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
                ("ExplicitInterfaceSpecifier", explicit_interface_specifier),
                ("AccessorList", accessors),
                ("Modifiers", json!(self.modifiers(node))),
                ("AttributeLists", json!(self.attribute_lists(node))),
            ],
        )
    }

    fn indexer_declaration(&self, node: Node) -> Value {
        let accessors = node
            .child_by_field_name("accessors")
            .map(|n| self.accessor_list(n))
            .unwrap_or_else(|| self.accessor_list_from_arrow(node));
        let explicit_interface_specifier = self
            .first_named_child_of_kind(node, &["explicit_interface_specifier"])
            .map(|n| self.explicit_interface_specifier(n))
            .unwrap_or(Value::Null);
        self.node(
            "IndexerDeclaration",
            node,
            vec![
                ("Identifier", self.identifier_value("Item")),
                (
                    "Type",
                    node.child_by_field_name("type")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                (
                    "ParameterList",
                    node.child_by_field_name("parameters")
                        .map(|n| self.parameter_list(n))
                        .unwrap_or_else(|| {
                            self.node("ParameterList", node, vec![("Parameters", json!([]))])
                        }),
                ),
                ("ExplicitInterfaceSpecifier", explicit_interface_specifier),
                ("AccessorList", accessors),
                ("Modifiers", json!(self.modifiers(node))),
                ("AttributeLists", json!(self.attribute_lists(node))),
            ],
        )
    }

    fn constructor_declaration(&self, node: Node) -> Value {
        let initializer = self
            .first_named_child_of_kind(node, &["constructor_initializer"])
            .map(|n| self.constructor_initializer(n))
            .unwrap_or(Value::Null);
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
                ("Initializer", initializer),
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
        let identifier = self.identifier_token(node.child_by_field_name("name"));
        self.method_like_declaration(node, kind, identifier, return_type)
    }

    fn operator_declaration(&self, node: Node) -> Value {
        let operator = node
            .child_by_field_name("operator")
            .map(|n| self.text(n))
            .unwrap_or("");
        let identifier = self.identifier_value(format!("operator{operator}"));
        let return_type = node
            .child_by_field_name("type")
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.method_like_declaration(node, "OperatorDeclaration", identifier, return_type)
    }

    fn conversion_operator_declaration(&self, node: Node) -> Value {
        let conversion = self
            .children(node)
            .find(|child| matches!(child.kind(), "implicit" | "explicit"))
            .map(|n| self.text(n))
            .unwrap_or("operator");
        let typ = node.child_by_field_name("type");
        let type_code = typ.map(|n| self.text(n)).unwrap_or("");
        let identifier = self.identifier_value(format!("{conversion} operator {type_code}"));
        let return_type = typ.map(|n| self.emit(n)).unwrap_or(Value::Null);
        self.method_like_declaration(
            node,
            "ConversionOperatorDeclaration",
            identifier,
            return_type,
        )
    }

    fn destructor_declaration(&self, node: Node) -> Value {
        let name = node
            .child_by_field_name("name")
            .map(|n| format!("~{}", self.text(n)))
            .unwrap_or_else(|| "~".to_string());
        self.method_like_declaration(
            node,
            "DestructorDeclaration",
            self.identifier_value(name),
            self.synthetic_void_type(node),
        )
    }

    fn method_like_declaration(
        &self,
        node: Node,
        kind: &'static str,
        identifier: Value,
        return_type: Value,
    ) -> Value {
        let explicit_interface_specifier = self
            .first_named_child_of_kind(node, &["explicit_interface_specifier"])
            .map(|n| self.explicit_interface_specifier(n))
            .unwrap_or(Value::Null);
        self.node(
            kind,
            node,
            vec![
                ("Identifier", identifier),
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
                ("ExplicitInterfaceSpecifier", explicit_interface_specifier),
                ("Modifiers", json!(self.modifiers(node))),
                ("AttributeLists", json!(self.attribute_lists(node))),
                ("TypeParameterList", self.type_parameter_list_field(node)),
                ("ConstraintClauses", json!(self.constraint_clauses(node))),
            ],
        )
    }

    fn delegate_declaration(&self, node: Node) -> Value {
        let return_type = node
            .child_by_field_name("type")
            .map(|n| self.emit(n))
            .unwrap_or_else(|| self.node("PredefinedType", node, vec![]));
        self.node(
            "DelegateDeclaration",
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
                ("Modifiers", json!(self.modifiers(node))),
                ("AttributeLists", json!(self.attribute_lists(node))),
                ("TypeParameterList", self.type_parameter_list_field(node)),
                ("ConstraintClauses", json!(self.constraint_clauses(node))),
            ],
        )
    }

    fn block_or_arrow(&self, node: Node) -> Value {
        if node.kind() == "arrow_expression_clause" {
            self.block_from_arrow(node, true)
        } else {
            self.block(node)
        }
    }

    fn block_from_arrow(&self, node: Node, returns_expression: bool) -> Value {
        let stmt = self
            .first_named_child(node)
            .map(|n| {
                if returns_expression {
                    self.return_statement_like(n)
                } else {
                    self.expression_statement_like(n)
                }
            })
            .unwrap_or(Value::Null);
        self.node("Block", node, vec![("Statements", json!(vec![stmt]))])
    }

    fn block(&self, node: Node) -> Value {
        let statements = self
            .named_children(node)
            .filter(|child| child.kind() != "comment")
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
                (
                    "Await",
                    json!(
                        self.has_direct_child_token(node, "await")
                            || self.text(node).trim_start().starts_with("await using ")
                    ),
                ),
                (
                    "Using",
                    json!(
                        self.has_direct_child_token(node, "using")
                            || self.text(node).trim_start().starts_with("using ")
                    ),
                ),
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
        let mut fields = vec![
            ("Identifier", self.identifier_token(name)),
            ("Initializer", initializer),
        ];
        if let Some(designation) = name.filter(|child| child.kind() == "tuple_pattern") {
            fields.push(("Designation", self.tuple_pattern(designation)));
        }
        self.node("VariableDeclarator", node, fields)
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

    fn expression_statement_like(&self, expr: Node) -> Value {
        self.node(
            "ExpressionStatement",
            expr,
            vec![("Expression", self.emit(expr))],
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
        self.throw_expression(node, "ThrowStatement")
    }

    fn throw_expression(&self, node: Node, kind: &'static str) -> Value {
        let expression = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node(kind, node, vec![("Expression", expression)])
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
                    "Await",
                    json!(
                        self.has_direct_child_token(node, "await")
                            || self.text(node).trim_start().starts_with("await foreach ")
                    ),
                ),
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

    fn switch_expression(&self, node: Node) -> Value {
        let named = self.named_children(node).collect::<Vec<_>>();
        let governing_expression = named
            .iter()
            .find(|child| child.kind() != "switch_expression_arm")
            .map(|child| self.emit(*child))
            .unwrap_or(Value::Null);
        let arms = named
            .iter()
            .filter(|child| child.kind() == "switch_expression_arm")
            .map(|child| self.switch_expression_arm(*child))
            .collect::<Vec<_>>();
        self.node(
            "SwitchExpression",
            node,
            vec![
                ("GoverningExpression", governing_expression),
                ("Arms", json!(arms)),
            ],
        )
    }

    fn switch_expression_arm(&self, node: Node) -> Value {
        let named = self.named_children(node).collect::<Vec<_>>();
        let pattern = named
            .first()
            .map(|child| self.switch_expression_arm_pattern(*child))
            .unwrap_or(Value::Null);
        let when_clause = named
            .iter()
            .find(|child| child.kind() == "when_clause")
            .map(|child| self.when_clause(*child))
            .unwrap_or(Value::Null);
        let expression = named
            .iter()
            .rev()
            .find(|child| child.kind() != "when_clause")
            .filter(|child| Some(**child) != named.first().copied())
            .map(|child| self.emit(*child))
            .unwrap_or(Value::Null);
        self.node(
            "SwitchExpressionArm",
            node,
            vec![
                ("Pattern", pattern),
                ("WhenClause", when_clause),
                ("Expression", expression),
            ],
        )
    }

    fn switch_expression_arm_pattern(&self, node: Node) -> Value {
        if node.kind() == "constant_pattern" || is_constant_case_label(node.kind()) {
            let expression = if node.kind() == "constant_pattern" {
                self.first_named_child(node)
                    .map(|child| self.emit(child))
                    .unwrap_or(Value::Null)
            } else {
                self.emit(node)
            };
            self.node("ConstantPattern", node, vec![("Expression", expression)])
        } else {
            self.emit(node)
        }
    }

    fn when_clause(&self, node: Node) -> Value {
        let condition = self
            .first_named_child(node)
            .map(|child| self.emit(child))
            .unwrap_or(Value::Null);
        self.node("WhenClause", node, vec![("Condition", condition)])
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
                (
                    "Await",
                    json!(
                        self.has_direct_child_token(node, "await")
                            || self.text(node).trim_start().starts_with("await using ")
                    ),
                ),
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

    fn lock_statement(&self, node: Node) -> Value {
        let children = self.named_children(node).collect::<Vec<_>>();
        let expression = children
            .first()
            .map(|n| self.emit(*n))
            .unwrap_or(Value::Null);
        let statement = children
            .last()
            .map(|n| self.statement_as_block(*n))
            .unwrap_or(Value::Null);
        self.node(
            "LockStatement",
            node,
            vec![("Expression", expression), ("Statement", statement)],
        )
    }

    fn checked_statement(&self, node: Node) -> Value {
        let keyword = if self.text(node).trim_start().starts_with("unchecked") {
            "unchecked"
        } else {
            "checked"
        };
        self.node(
            "CheckedStatement",
            node,
            vec![
                ("Keyword", json!({ "Value": keyword })),
                (
                    "Statement",
                    self.first_named_child_of_kind(node, &["block"])
                        .map(|n| self.statement_as_block(n))
                        .unwrap_or(Value::Null),
                ),
            ],
        )
    }

    fn unsafe_statement(&self, node: Node) -> Value {
        self.node(
            "UnsafeStatement",
            node,
            vec![(
                "Statement",
                self.first_named_child_of_kind(node, &["block"])
                    .map(|n| self.statement_as_block(n))
                    .unwrap_or(Value::Null),
            )],
        )
    }

    fn fixed_statement(&self, node: Node) -> Value {
        let declaration = self
            .first_named_child_of_kind(node, &["variable_declaration"])
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        let statement = self
            .named_children(node)
            .find(|child| is_statement_node(child.kind()))
            .map(|n| self.statement_as_block(n))
            .unwrap_or(Value::Null);
        self.node(
            "FixedStatement",
            node,
            vec![("Declaration", declaration), ("Statement", statement)],
        )
    }

    fn yield_statement(&self, node: Node) -> Value {
        let rest = self
            .text(node)
            .trim_start()
            .strip_prefix("yield")
            .unwrap_or("")
            .trim_start();
        let keyword = if rest.starts_with("break") {
            "break"
        } else {
            "return"
        };
        let expression = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node(
            "YieldStatement",
            node,
            vec![
                ("Keyword", json!({ "Value": keyword })),
                ("Expression", expression),
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
        let filter = self
            .first_named_child_of_kind(node, &["catch_filter_clause"])
            .map(|n| self.catch_filter_clause(n))
            .unwrap_or(Value::Null);
        self.node(
            "CatchClause",
            node,
            vec![
                ("Declaration", declaration),
                ("Filter", filter),
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
        self.node(
            "CatchDeclaration",
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
                    node.child_by_field_name("name")
                        .map(|name| self.identifier_token(Some(name)))
                        .unwrap_or(Value::Null),
                ),
            ],
        )
    }

    fn catch_filter_clause(&self, node: Node) -> Value {
        let condition = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("CatchFilterClause", node, vec![("Condition", condition)])
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
        let element = node
            .child_by_field_name("type")
            .or_else(|| self.first_named_child(node))
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        let rank = node
            .child_by_field_name("rank")
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node(
            "ArrayType",
            node,
            vec![("ElementType", element), ("Rank", rank)],
        )
    }

    fn array_rank_specifier(&self, node: Node) -> Value {
        let expressions = self
            .named_children(node)
            .map(|child| self.emit(child))
            .collect::<Vec<_>>();
        self.node(
            "ArrayRankSpecifier",
            node,
            vec![("Expressions", json!(expressions))],
        )
    }

    fn pointer_type(&self, node: Node) -> Value {
        let element = node
            .child_by_field_name("type")
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("PointerType", node, vec![("ElementType", element)])
    }

    fn function_pointer_type(&self, node: Node) -> Value {
        let parameters = self
            .named_children(node)
            .filter(|child| child.kind() == "function_pointer_parameter")
            .map(|child| self.function_pointer_parameter(child))
            .collect::<Vec<_>>();
        let calling_convention = self
            .first_named_child_of_kind(node, &["calling_convention"])
            .map(|n| json!({ "Value": self.text(n) }))
            .unwrap_or(Value::Null);
        let return_type = node
            .child_by_field_name("returns")
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node(
            "FunctionPointerType",
            node,
            vec![
                ("CallingConvention", calling_convention),
                ("Parameters", json!(parameters)),
                ("ReturnType", return_type),
            ],
        )
    }

    fn function_pointer_parameter(&self, node: Node) -> Value {
        let text = self.text(node).trim_start();
        let ref_kind = ["ref", "out", "in"]
            .iter()
            .find(|keyword| {
                text.strip_prefix(**keyword)
                    .map(|rest| {
                        rest.chars()
                            .next()
                            .map(|c| c.is_whitespace())
                            .unwrap_or(false)
                    })
                    .unwrap_or(false)
            })
            .map(|keyword| json!({ "Value": *keyword }))
            .unwrap_or(Value::Null);
        let typ = node
            .child_by_field_name("type")
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node(
            "FunctionPointerParameter",
            node,
            vec![("RefKind", ref_kind), ("Type", typ)],
        )
    }

    fn nullable_type(&self, node: Node) -> Value {
        let element = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("NullableType", node, vec![("ElementType", element)])
    }

    fn ref_type(&self, node: Node) -> Value {
        let element = node
            .child_by_field_name("type")
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("RefType", node, vec![("ElementType", element)])
    }

    fn scoped_type(&self, node: Node) -> Value {
        let element = node
            .child_by_field_name("type")
            .or_else(|| self.first_named_child(node))
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("ScopedType", node, vec![("ElementType", element)])
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

    fn with_expression(&self, node: Node) -> Value {
        let named = self.named_children(node).collect::<Vec<_>>();
        let expression = named
            .iter()
            .find(|child| child.kind() != "with_initializer")
            .map(|child| self.emit(*child))
            .unwrap_or(Value::Null);
        let initializers = named
            .iter()
            .filter(|child| child.kind() == "with_initializer")
            .map(|child| self.with_initializer(*child))
            .collect::<Vec<_>>();
        let initializer = self.node(
            "ObjectInitializerExpression",
            node,
            vec![("Expressions", json!(initializers))],
        );
        self.node(
            "WithExpression",
            node,
            vec![("Expression", expression), ("Initializer", initializer)],
        )
    }

    fn with_initializer(&self, node: Node) -> Value {
        let named = self.named_children(node).collect::<Vec<_>>();
        let left = named
            .iter()
            .find(|child| child.kind() == "identifier")
            .map(|child| self.emit(*child))
            .unwrap_or(Value::Null);
        let right = named
            .iter()
            .rev()
            .find(|child| child.kind() != "identifier")
            .map(|child| self.emit(*child))
            .unwrap_or(Value::Null);
        self.node(
            "SimpleAssignmentExpression",
            node,
            vec![
                ("Left", left),
                ("Right", right),
                ("OperatorToken", json!({ "Value": "=" })),
            ],
        )
    }

    fn binary_expression(&self, node: Node) -> Value {
        let op = node
            .child_by_field_name("operator")
            .map(|n| self.text(n))
            .unwrap_or("");
        let left = node.child_by_field_name("left");
        let right = node.child_by_field_name("right");
        if self.is_compact_unsigned_shift(node, op, right) {
            if let Some(expression) = self.synthetic_expression(node, self.text(node)) {
                return expression;
            }
        }
        self.node(
            binary_kind(op),
            node,
            vec![
                ("Left", left.map(|n| self.emit(n)).unwrap_or(Value::Null)),
                ("Right", right.map(|n| self.emit(n)).unwrap_or(Value::Null)),
                ("OperatorToken", json!({ "Value": op })),
            ],
        )
    }

    fn binary_type_expression(&self, node: Node, kind: &'static str) -> Value {
        self.node(
            kind,
            node,
            vec![
                (
                    "Expression",
                    node.child_by_field_name("left")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                (
                    "Type",
                    node.child_by_field_name("right")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
            ],
        )
    }

    fn range_expression(&self, node: Node) -> Value {
        let children = self.named_children(node).collect::<Vec<_>>();
        let left = children.first().copied();
        let right = children
            .last()
            .copied()
            .filter(|right| Some(*right) != left);
        self.node(
            "RangeExpression",
            node,
            vec![
                ("Left", left.map(|n| self.emit(n)).unwrap_or(Value::Null)),
                ("Right", right.map(|n| self.emit(n)).unwrap_or(Value::Null)),
                ("OperatorToken", json!({ "Value": ".." })),
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
        let function = node.child_by_field_name("function");
        let kind = if function.is_some_and(|n| self.text(n) == "nameof") {
            "NameOfExpression"
        } else {
            "InvocationExpression"
        };
        self.node(
            kind,
            node,
            vec![
                (
                    "Expression",
                    function.map(|n| self.emit(n)).unwrap_or(Value::Null),
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

    fn tuple_expression(&self, node: Node) -> Value {
        let arguments = self
            .named_children(node)
            .filter(|child| child.kind() == "argument")
            .map(|child| self.argument(child))
            .collect::<Vec<_>>();
        self.node(
            "TupleExpression",
            node,
            vec![("Arguments", json!(arguments))],
        )
    }

    fn tuple_type(&self, node: Node) -> Value {
        let elements = self
            .named_children(node)
            .filter(|child| child.kind() == "tuple_element")
            .map(|child| self.tuple_element(child))
            .collect::<Vec<_>>();
        self.node("TupleType", node, vec![("Elements", json!(elements))])
    }

    fn tuple_element(&self, node: Node) -> Value {
        let typ = node
            .child_by_field_name("type")
            .map(|child| self.emit(child))
            .unwrap_or(Value::Null);
        self.node(
            "TupleElement",
            node,
            vec![
                ("Type", typ),
                (
                    "Identifier",
                    self.identifier_token(node.child_by_field_name("name")),
                ),
            ],
        )
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
        let expression_node = self.named_children(node).find(|child| Some(*child) != name);
        let expression = expression_node.map(|n| self.emit(n)).unwrap_or(Value::Null);
        let mut fields = vec![("Expression", expression)];
        if let (Some(name), Some(expression_node)) = (name, expression_node) {
            let kind = self.attribute_argument_name_kind(name, expression_node);
            fields.push((kind, self.node(kind, name, vec![("Name", self.emit(name))])));
        }
        self.node("AttributeArgument", node, fields)
    }

    fn attribute_argument_name_kind(&self, name: Node, expression: Node) -> &'static str {
        let separator = self
            .source_between(name.end_byte(), expression.start_byte())
            .trim_start();
        if separator.starts_with(':') {
            "NameColon"
        } else {
            "NameEquals"
        }
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
                (
                    "Initializer",
                    self.first_named_child_of_kind(node, &["initializer_expression"])
                        .map(|n| self.initializer_expression(n, "ObjectInitializerExpression"))
                        .unwrap_or(Value::Null),
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

    fn type_operand_expression(
        &self,
        node: Node,
        kind: &'static str,
        allow_missing_type: bool,
    ) -> Value {
        let typ = node
            .child_by_field_name("type")
            .map(|n| self.emit(n))
            .unwrap_or_else(|| {
                if allow_missing_type {
                    Value::Null
                } else {
                    self.unknown(node)
                }
            });
        self.node(kind, node, vec![("Type", typ)])
    }

    fn unary_child_expression(&self, node: Node, kind: &'static str) -> Value {
        let expression = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node(kind, node, vec![("Expression", expression)])
    }

    fn refvalue_expression(&self, node: Node) -> Value {
        let named = self.named_children(node).collect::<Vec<_>>();
        let expression_node = node
            .child_by_field_name("expression")
            .or_else(|| named.first().copied());
        let expression = node
            .child_by_field_name("expression")
            .or_else(|| named.first().copied())
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        let typ = node
            .child_by_field_name("type")
            .or_else(|| named.last().copied())
            .filter(|typ| match expression_node {
                Some(expression) => *typ != expression,
                None => true,
            })
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node(
            "RefValueExpression",
            node,
            vec![("Expression", expression), ("Type", typ)],
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

    fn checked_expression(&self, node: Node) -> Value {
        let keyword = if self.text(node).trim_start().starts_with("unchecked") {
            "unchecked"
        } else {
            "checked"
        };
        let expression = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node(
            "CheckedExpression",
            node,
            vec![
                ("Keyword", json!({ "Value": keyword })),
                ("Expression", expression),
            ],
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

    fn anonymous_method_expression(&self, node: Node) -> Value {
        let body = self
            .first_named_child_of_kind(node, &["block"])
            .map(|n| self.block(n))
            .unwrap_or(Value::Null);
        self.node(
            "AnonymousMethodExpression",
            node,
            vec![
                (
                    "ParameterList",
                    self.parameter_list_or_empty(node.child_by_field_name("parameters"), node),
                ),
                ("Body", body),
                ("Modifiers", json!(self.modifiers(node))),
            ],
        )
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

    fn stackalloc_expression(&self, node: Node) -> Value {
        self.node(
            "StackAllocExpression",
            node,
            vec![
                (
                    "Type",
                    node.child_by_field_name("type")
                        .map(|n| self.emit(n))
                        .unwrap_or(Value::Null),
                ),
                (
                    "Initializer",
                    self.first_named_child_of_kind(node, &["initializer_expression"])
                        .map(|n| self.initializer_expression(n, "ArrayInitializerExpression"))
                        .unwrap_or(Value::Null),
                ),
            ],
        )
    }

    fn query_expression(&self, node: Node) -> Value {
        let mut named = self.named_children(node);
        let from_clause = named.next().map(|n| self.emit(n)).unwrap_or(Value::Null);
        let clauses = named.map(|child| self.emit(child)).collect::<Vec<_>>();
        self.node(
            "QueryExpression",
            node,
            vec![("FromClause", from_clause), ("Clauses", json!(clauses))],
        )
    }

    fn query_from_clause(&self, node: Node) -> Value {
        let typ = node.child_by_field_name("type");
        let name = node.child_by_field_name("name");
        let expression = self
            .named_children(node)
            .find(|child| Some(*child) != typ && Some(*child) != name)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node(
            "FromClause",
            node,
            vec![
                ("Type", typ.map(|n| self.emit(n)).unwrap_or(Value::Null)),
                ("Identifier", self.identifier_token(name)),
                ("Expression", expression),
            ],
        )
    }

    fn join_clause(&self, node: Node) -> Value {
        let typ = node.child_by_field_name("type");
        let name = self.first_named_child_of_kind(node, &["identifier"]);
        let into_clause = self.first_named_child_of_kind(node, &["join_into_clause"]);
        let expressions = self
            .named_children(node)
            .filter(|child| {
                Some(*child) != typ && Some(*child) != name && Some(*child) != into_clause
            })
            .filter(|child| child.kind() != "join_into_clause")
            .map(|child| self.emit(child))
            .collect::<Vec<_>>();
        self.node(
            "JoinClause",
            node,
            vec![
                ("Type", typ.map(|n| self.emit(n)).unwrap_or(Value::Null)),
                ("Identifier", self.identifier_token(name)),
                (
                    "InExpression",
                    expressions.first().cloned().unwrap_or(Value::Null),
                ),
                (
                    "LeftExpression",
                    expressions.get(1).cloned().unwrap_or(Value::Null),
                ),
                (
                    "RightExpression",
                    expressions.get(2).cloned().unwrap_or(Value::Null),
                ),
                (
                    "Into",
                    into_clause
                        .map(|n| self.join_into_clause(n))
                        .unwrap_or(Value::Null),
                ),
            ],
        )
    }

    fn join_into_clause(&self, node: Node) -> Value {
        let name = self.first_named_child_of_kind(node, &["identifier"]);
        self.node(
            "JoinIntoClause",
            node,
            vec![("Identifier", self.identifier_token(name))],
        )
    }

    fn let_clause(&self, node: Node) -> Value {
        let name = self.first_named_child_of_kind(node, &["identifier"]);
        let expression = self
            .named_children(node)
            .find(|child| Some(*child) != name)
            .map(|child| self.emit(child))
            .unwrap_or(Value::Null);
        self.node(
            "LetClause",
            node,
            vec![
                ("Identifier", self.identifier_token(name)),
                ("Expression", expression),
            ],
        )
    }

    fn order_by_clause(&self, node: Node) -> Value {
        let expression_nodes = self.named_children(node).collect::<Vec<_>>();
        let expressions = expression_nodes
            .iter()
            .map(|child| self.emit(*child))
            .collect::<Vec<_>>();
        let directions = expression_nodes
            .iter()
            .enumerate()
            .map(|(idx, child)| {
                let end = expression_nodes
                    .get(idx + 1)
                    .map(|next| next.start_byte())
                    .unwrap_or_else(|| node.end_byte());
                self.ordering_direction(*child, end)
                    .map(|direction| json!(direction))
                    .unwrap_or(Value::Null)
            })
            .collect::<Vec<_>>();
        self.node(
            "OrderByClause",
            node,
            vec![
                ("Expressions", json!(expressions)),
                ("Directions", json!(directions)),
            ],
        )
    }

    fn ordering_direction(&self, expression: Node, end: usize) -> Option<&'static str> {
        let tail = self.source_between(expression.end_byte(), end);
        if tail.contains("descending") {
            Some("descending")
        } else if tail.contains("ascending") {
            Some("ascending")
        } else {
            None
        }
    }

    fn where_clause(&self, node: Node) -> Value {
        let condition = self
            .first_named_child(node)
            .map(|child| self.emit(child))
            .unwrap_or(Value::Null);
        self.node("WhereClause", node, vec![("Condition", condition)])
    }

    fn select_clause(&self, node: Node) -> Value {
        let expression = self
            .first_named_child(node)
            .map(|child| self.emit(child))
            .unwrap_or(Value::Null);
        self.node("SelectClause", node, vec![("Expression", expression)])
    }

    fn group_clause(&self, node: Node) -> Value {
        let expressions = self.named_children(node).collect::<Vec<_>>();
        self.node(
            "GroupClause",
            node,
            vec![
                (
                    "Expression",
                    expressions
                        .first()
                        .map(|n| self.emit(*n))
                        .unwrap_or(Value::Null),
                ),
                (
                    "ByExpression",
                    expressions
                        .get(1)
                        .map(|n| self.emit(*n))
                        .unwrap_or(Value::Null),
                ),
            ],
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
            .find(|child| {
                !matches!(
                    child.kind(),
                    "interpolation_brace"
                        | "interpolation_alignment_clause"
                        | "interpolation_format_clause"
                )
            })
            .map(|child| self.emit(child))
            .unwrap_or(Value::Null);
        let mut fields = vec![("Expression", expression)];
        if let Some(alignment) =
            self.first_named_child_of_kind(node, &["interpolation_alignment_clause"])
        {
            fields.push((
                "AlignmentClause",
                self.interpolation_alignment_clause(alignment),
            ));
        }
        if let Some(format) = self.first_named_child_of_kind(node, &["interpolation_format_clause"])
        {
            fields.push(("FormatClause", self.interpolation_format_clause(format)));
        }
        self.node("Interpolation", node, fields)
    }

    fn interpolation_alignment_clause(&self, node: Node) -> Value {
        let expression = self
            .first_named_child(node)
            .map(|child| self.emit(child))
            .unwrap_or(Value::Null);
        self.node(
            "InterpolationAlignmentClause",
            node,
            vec![("Expression", expression)],
        )
    }

    fn interpolation_format_clause(&self, node: Node) -> Value {
        self.node(
            "InterpolationFormatClause",
            node,
            vec![(
                "FormatStringToken",
                json!({ "Value": self.text(node).trim_start_matches(':') }),
            )],
        )
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
        let designation = named
            .last()
            .map(|n| match n.kind() {
                "parenthesized_variable_designation" => self.parenthesized_variable_designation(*n),
                "tuple_pattern" => self.tuple_pattern(*n),
                "discard" => self.node("DiscardPattern", *n, vec![]),
                _ => self.single_variable_designation(*n),
            })
            .unwrap_or(Value::Null);
        self.node(
            "DeclarationPattern",
            node,
            vec![
                (
                    "Type",
                    named.first().map(|n| self.emit(*n)).unwrap_or(Value::Null),
                ),
                ("Designation", designation),
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

    fn binary_pattern(&self, node: Node, kind: &'static str, operator: &'static str) -> Value {
        let named = self.named_children(node).collect::<Vec<_>>();
        let left = named.first().copied();
        let right = named.last().copied().filter(|right| Some(*right) != left);
        self.node(
            kind,
            node,
            vec![
                ("Left", left.map(|n| self.emit(n)).unwrap_or(Value::Null)),
                ("Right", right.map(|n| self.emit(n)).unwrap_or(Value::Null)),
                ("OperatorToken", json!({ "Value": operator })),
            ],
        )
    }

    fn parenthesized_pattern(&self, node: Node) -> Value {
        let pattern = self
            .first_named_child(node)
            .map(|child| self.emit(child))
            .unwrap_or(Value::Null);
        self.node("ParenthesizedPattern", node, vec![("Pattern", pattern)])
    }

    fn type_pattern(&self, node: Node) -> Value {
        let typ = node
            .child_by_field_name("type")
            .or_else(|| self.first_named_child(node))
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("TypePattern", node, vec![("Type", typ)])
    }

    fn var_pattern(&self, node: Node) -> Value {
        let designation = node
            .child_by_field_name("name")
            .or_else(|| self.first_named_child(node))
            .map(|n| match n.kind() {
                "parenthesized_variable_designation" => self.parenthesized_variable_designation(n),
                "tuple_pattern" => self.tuple_pattern(n),
                "discard" => self.node("DiscardPattern", n, vec![]),
                _ => self.single_variable_designation(n),
            })
            .unwrap_or(Value::Null);
        self.node("VarPattern", node, vec![("Designation", designation)])
    }

    fn tuple_pattern(&self, node: Node) -> Value {
        let patterns = self
            .named_children(node)
            .map(|child| match child.kind() {
                "identifier" => self.single_variable_designation(child),
                "discard" => self.node("DiscardPattern", child, vec![]),
                "tuple_pattern" => self.tuple_pattern(child),
                "parenthesized_variable_designation" => {
                    self.parenthesized_variable_designation(child)
                }
                _ => self.emit(child),
            })
            .collect::<Vec<_>>();
        self.node("TuplePattern", node, vec![("Patterns", json!(patterns))])
    }

    fn parenthesized_variable_designation(&self, node: Node) -> Value {
        let patterns = self
            .named_children(node)
            .map(|child| match child.kind() {
                "identifier" => self.single_variable_designation(child),
                "discard" => self.node("DiscardPattern", child, vec![]),
                "tuple_pattern" => self.tuple_pattern(child),
                "parenthesized_variable_designation" => {
                    self.parenthesized_variable_designation(child)
                }
                _ => self.emit(child),
            })
            .collect::<Vec<_>>();
        self.node(
            "ParenthesizedVariableDesignation",
            node,
            vec![("Patterns", json!(patterns))],
        )
    }

    fn list_pattern(&self, node: Node) -> Value {
        let patterns = self
            .named_children(node)
            .map(|child| self.emit(child))
            .collect::<Vec<_>>();
        let has_slice = self.children(node).any(|child| child.kind() == "..");
        let slice_index = if has_slice {
            self.children(node)
                .take_while(|child| child.kind() != "..")
                .filter(|child| child.is_named())
                .count()
        } else {
            patterns.len()
        };
        self.node(
            "ListPattern",
            node,
            vec![
                ("Patterns", json!(patterns)),
                ("HasSlice", json!(has_slice)),
                ("SliceIndex", json!(slice_index)),
            ],
        )
    }

    fn recursive_pattern(&self, node: Node) -> Value {
        let positional_patterns = self
            .named_children(node)
            .filter(|child| child.kind() == "positional_pattern_clause")
            .flat_map(|clause| self.named_children(clause))
            .filter(|child| child.kind() == "subpattern")
            .map(|child| self.subpattern(child))
            .collect::<Vec<_>>();
        let property_patterns = self
            .named_children(node)
            .filter(|child| child.kind() == "property_pattern_clause")
            .flat_map(|clause| self.named_children(clause))
            .filter(|child| child.kind() == "subpattern")
            .map(|child| self.subpattern(child))
            .collect::<Vec<_>>();
        let typ = node
            .child_by_field_name("type")
            .map(|child| self.emit(child))
            .unwrap_or(Value::Null);
        let designation = node
            .child_by_field_name("name")
            .map(|child| self.single_variable_designation(child))
            .unwrap_or(Value::Null);
        self.node(
            "RecursivePattern",
            node,
            vec![
                ("Type", typ),
                ("Designation", designation),
                ("PositionalPatterns", json!(positional_patterns)),
                ("PropertyPatterns", json!(property_patterns)),
            ],
        )
    }

    fn subpattern(&self, node: Node) -> Value {
        let named = self.named_children(node).collect::<Vec<_>>();
        let pattern_index = named
            .iter()
            .rposition(|child| is_pattern_node(child.kind()));
        let name = pattern_index
            .and_then(|idx| named[..idx].first().copied())
            .map(|child| self.emit(child))
            .unwrap_or(Value::Null);
        let pattern = pattern_index
            .map(|idx| self.emit(named[idx]))
            .unwrap_or(Value::Null);
        self.node(
            "Subpattern",
            node,
            vec![("Name", name), ("Pattern", pattern)],
        )
    }

    fn parameter_list_or_empty(&self, node: Option<Node>, fallback: Node) -> Value {
        node.map(|n| self.parameter_list(n)).unwrap_or_else(|| {
            self.node("ParameterList", fallback, vec![("Parameters", json!([]))])
        })
    }

    fn parameter_list(&self, node: Node) -> Value {
        let children = self.children(node).collect::<Vec<_>>();
        let mut params = Vec::new();
        let mut idx = 0;

        while idx < children.len() {
            let child = children[idx];
            match child.kind() {
                "parameter" | "implicit_parameter" => {
                    params.push(self.parameter_or_identifier(child));
                    idx += 1;
                }
                "params" => {
                    if let Some((param, next_idx)) = self.params_parameter(&children, idx) {
                        params.push(param);
                        idx = next_idx;
                    } else {
                        idx += 1;
                    }
                }
                "identifier" => {
                    params.push(self.parameter_or_identifier(child));
                    idx += 1;
                }
                _ => {
                    idx += 1;
                }
            }
        }

        self.node("ParameterList", node, vec![("Parameters", json!(params))])
    }

    fn params_parameter(&self, children: &[Node], params_idx: usize) -> Option<(Value, usize)> {
        let type_idx = children
            .iter()
            .enumerate()
            .skip(params_idx + 1)
            .find(|(_, child)| child.is_named() && child.kind() != "identifier")
            .map(|(idx, _)| idx)?;
        let ident_idx = children
            .iter()
            .enumerate()
            .skip(type_idx + 1)
            .find(|(_, child)| child.kind() == "identifier")
            .map(|(idx, _)| idx)?;
        let params_token = children[params_idx];
        let type_node = children[type_idx];
        let ident_node = children[ident_idx];
        let code = &self.bytes[params_token.start_byte()..ident_node.end_byte()];
        let code = std::str::from_utf8(code).ok()?;
        let param = self.synthetic_node(
            "Parameter",
            code,
            params_token.start_position(),
            ident_node.end_position(),
            vec![
                ("AttributeLists", json!(Vec::<Value>::new())),
                ("Identifier", self.identifier_token(Some(ident_node))),
                ("Type", self.emit(type_node)),
                ("Modifiers", json!(vec![json!({ "Value": "params" })])),
            ],
        );
        Some((param, ident_idx + 1))
    }

    fn parameter(&self, node: Node) -> Value {
        self.node(
            "Parameter",
            node,
            vec![
                ("AttributeLists", json!(self.attribute_lists(node))),
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
        let children = self
            .named_children(node)
            .filter(|child| child.kind() != "comment")
            .collect::<Vec<_>>();
        let mut types = Vec::new();
        let mut idx = 0;
        while idx < children.len() {
            let child = children[idx];
            if child.kind() == "primary_constructor_base_type" {
                types.push(self.primary_constructor_base_type(child));
                idx += 1;
            } else if idx + 1 < children.len() && children[idx + 1].kind() == "argument_list" {
                types.push(self.primary_constructor_base_type_from_parts(child, children[idx + 1]));
                idx += 2;
            } else if child.kind() == "argument_list" {
                idx += 1;
            } else {
                types.push(self.node("SimpleBaseType", child, vec![("Type", self.emit(child))]));
                idx += 1;
            }
        }
        self.node("BaseList", node, vec![("Types", json!(types))])
    }

    fn primary_constructor_base_type(&self, node: Node) -> Value {
        let typ = node
            .child_by_field_name("type")
            .or_else(|| {
                self.named_children(node)
                    .find(|child| child.kind() != "argument_list")
            })
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        let argument_list = self
            .first_named_child_of_kind(node, &["argument_list"])
            .map(|n| self.argument_list(n, "ArgumentList"))
            .unwrap_or_else(|| self.node("ArgumentList", node, vec![("Arguments", json!([]))]));
        self.node(
            "PrimaryConstructorBaseType",
            node,
            vec![("Type", typ), ("ArgumentList", argument_list)],
        )
    }

    fn primary_constructor_base_type_from_parts(&self, typ: Node, arguments: Node) -> Value {
        let code = self.source_between(typ.start_byte(), arguments.end_byte());
        self.synthetic_node(
            "PrimaryConstructorBaseType",
            code,
            typ.start_position(),
            arguments.end_position(),
            vec![
                ("Type", self.emit(typ)),
                (
                    "ArgumentList",
                    self.argument_list(arguments, "ArgumentList"),
                ),
            ],
        )
    }

    fn constructor_initializer(&self, node: Node) -> Value {
        let text = self.text(node);
        let kind = if text.contains("this") {
            "ThisConstructorInitializer"
        } else {
            "BaseConstructorInitializer"
        };
        let argument_list = self
            .first_named_child_of_kind(node, &["argument_list"])
            .map(|n| self.argument_list(n, "ArgumentList"))
            .unwrap_or_else(|| self.node("ArgumentList", node, vec![("Arguments", json!([]))]));
        self.node(kind, node, vec![("ArgumentList", argument_list)])
    }

    fn explicit_interface_specifier(&self, node: Node) -> Value {
        let name = self
            .first_named_child(node)
            .map(|n| self.emit(n))
            .unwrap_or(Value::Null);
        self.node("ExplicitInterfaceSpecifier", node, vec![("Name", name)])
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
        let body = node
            .child_by_field_name("value")
            .or_else(|| self.first_named_child_of_kind(node, &["arrow_expression_clause"]))
            .map(|n| self.block_from_arrow(n, true))
            .unwrap_or(Value::Null);
        let accessors = vec![self.node("GetAccessorDeclaration", node, vec![("Body", body)])];
        self.node("AccessorList", node, vec![("Accessors", json!(accessors))])
    }

    fn accessor_declaration(&self, node: Node) -> Value {
        let text = self.text(node);
        let keyword = text
            .split(|c: char| c.is_whitespace() || matches!(c, '{' | ';' | '=' | '>'))
            .find(|part| matches!(*part, "get" | "set" | "init" | "add" | "remove"))
            .unwrap_or("get");
        let kind = match keyword {
            "set" | "init" => "SetAccessorDeclaration",
            "add" => "AddAccessorDeclaration",
            "remove" => "RemoveAccessorDeclaration",
            _ => "GetAccessorDeclaration",
        };
        let body = node
            .child_by_field_name("body")
            .or_else(|| self.first_named_child_of_kind(node, &["block", "arrow_expression_clause"]))
            .map(|n| {
                if n.kind() == "arrow_expression_clause" {
                    self.block_from_arrow(n, kind == "GetAccessorDeclaration")
                } else {
                    self.block(n)
                }
            })
            .unwrap_or(Value::Null);
        self.node(
            kind,
            node,
            vec![("Body", body), ("Modifiers", json!(self.modifiers(node)))],
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
        let target = self
            .first_named_child_of_kind(node, &["attribute_target_specifier"])
            .map(|child| self.attribute_target_specifier(child))
            .unwrap_or(Value::Null);
        self.node(
            "AttributeList",
            node,
            vec![("Target", target), ("Attributes", json!(attributes))],
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

    fn attribute_target_specifier(&self, node: Node) -> Value {
        let target = self.text(node);
        let name = target.trim().trim_end_matches(':').trim();
        self.node(
            "AttributeTargetSpecifier",
            node,
            vec![("Identifier", self.identifier_value(name))],
        )
    }

    fn global_attribute_target(&self, node: Node) -> Value {
        let code = self.text(node);
        let target = code
            .trim_start()
            .strip_prefix('[')
            .and_then(|rest| rest.split_once(':'))
            .map(|(target, _)| target.trim())
            .filter(|target| matches!(*target, "assembly" | "module"));

        target
            .map(|target| {
                let start = node.start_position();
                let column = start.column + code.find(target).unwrap_or(1);
                self.synthetic_node(
                    "AttributeTargetSpecifier",
                    &format!("{target}:"),
                    Point {
                        row: start.row,
                        column,
                    },
                    Point {
                        row: start.row,
                        column: column + target.len() + 1,
                    },
                    vec![("Identifier", self.identifier_value(target))],
                )
            })
            .unwrap_or(Value::Null)
    }

    fn modifiers(&self, node: Node) -> Vec<Value> {
        let mut modifiers = self
            .named_children(node)
            .filter(|child| child.kind() == "modifier")
            .map(|child| json!({ "Value": self.text(child) }))
            .collect::<Vec<_>>();

        if node.kind() == "struct_declaration" && self.has_direct_child_token(node, "ref") {
            modifiers.push(json!({ "Value": "ref" }));
        }
        if node.kind() == "record_declaration" && self.has_direct_child_token(node, "struct") {
            modifiers.push(json!({ "Value": "struct" }));
        }
        if node.kind() == "parameter" {
            let code = self.text(node);
            let trimmed = code.trim_start();
            if trimmed.starts_with("params ") {
                modifiers.push(json!({ "Value": "params" }));
            }
            if trimmed.starts_with("scoped ") {
                modifiers.push(json!({ "Value": "scoped" }));
            }
        }

        modifiers
    }

    fn identifier_token(&self, node: Option<Node>) -> Value {
        self.identifier_value(node.map(|n| self.text(n)).unwrap_or(""))
    }

    fn identifier_value<S: Into<String>>(&self, value: S) -> Value {
        json!({ "Value": value.into() })
    }

    fn synthetic_void_type(&self, node: Node) -> Value {
        let start = node.start_position();
        self.synthetic_node(
            "PredefinedType",
            "void",
            start,
            Point {
                row: start.row,
                column: start.column + 4,
            },
            vec![],
        )
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

    fn is_compact_unsigned_shift(&self, node: Node, op: &str, right: Option<Node>) -> bool {
        op == ">>"
            && self.text(node).contains(">>>")
            && right.is_some_and(|n| self.text(n).starts_with('>'))
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

    fn has_direct_child_token(&self, node: Node, token: &str) -> bool {
        self.children(node)
            .any(|child| !child.is_named() && child.kind() == token)
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
            | "checked_statement"
            | "continue_statement"
            | "do_statement"
            | "empty_statement"
            | "expression_statement"
            | "fixed_statement"
            | "for_statement"
            | "foreach_statement"
            | "goto_statement"
            | "if_statement"
            | "labeled_statement"
            | "local_declaration_statement"
            | "local_function_statement"
            | "lock_statement"
            | "return_statement"
            | "switch_statement"
            | "throw_statement"
            | "try_statement"
            | "unsafe_statement"
            | "using_statement"
            | "while_statement"
            | "yield_statement"
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

fn is_pattern_node(kind: &str) -> bool {
    matches!(
        kind,
        "constant_pattern"
            | "declaration_pattern"
            | "discard"
            | "negated_pattern"
            | "relational_pattern"
            | "and_pattern"
            | "or_pattern"
            | "parenthesized_pattern"
            | "list_pattern"
            | "recursive_pattern"
            | "tuple_pattern"
            | "type_pattern"
            | "var_pattern"
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
    const OPS: [&str; 19] = [
        "==", "!=", "&&", "||", ">=", "<=", ">>>", ">>", "<<", ">", "<", "+", "-", "*", "/", "%",
        "&", "|", "^",
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
        "??=" => "CoalesceAssignmentExpression",
        ">>=" => "RightShiftAssignmentExpression",
        ">>>=" => "UnsignedRightShiftAssignmentExpression",
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
        "<<" => "LeftShiftExpression",
        ">>" => "RightShiftExpression",
        ">>>" => "UnsignedRightShiftExpression",
        "&" => "BitwiseAndExpression",
        "|" => "BitwiseOrExpression",
        "^" => "ExclusiveOrExpression",
        "??" => "CoalesceExpression",
        ".." => "RangeExpression",
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
        ("*", false) => "IndirectionExpression",
        ("^", _) => "IndexExpression",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains_kind(value: &Value, kind: &str) -> bool {
        match value {
            Value::Object(map) => {
                map.get("MetaData")
                    .and_then(|meta| meta.get("Kind"))
                    .and_then(Value::as_str)
                    .is_some_and(|candidate| candidate == kind)
                    || map.values().any(|child| contains_kind(child, kind))
            }
            Value::Array(items) => items.iter().any(|child| contains_kind(child, kind)),
            _ => false,
        }
    }

    fn modifier_values(value: &Value) -> Vec<&str> {
        value["Modifiers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|modifier| modifier["Value"].as_str().unwrap())
            .collect()
    }

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
    fn emits_static_and_alias_using_directives() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "using static System.Math;\nusing Alias = System.String;\nclass C { }\n",
        )
        .expect("json");
        let usings = json["AstRoot"]["Usings"].as_array().unwrap();
        let static_using = &usings[0];
        let alias_using = &usings[1];

        assert_eq!(static_using["MetaData"]["Kind"], "ast.UsingDirective");
        assert_eq!(static_using["Name"]["MetaData"]["Code"], "System.Math");
        assert_eq!(static_using["Static"], true);
        assert_eq!(static_using["Global"], false);
        assert_eq!(static_using["Unsafe"], false);
        assert!(static_using["Alias"].is_null());
        assert_eq!(alias_using["Name"]["MetaData"]["Code"], "System.String");
        assert_eq!(alias_using["Alias"]["MetaData"]["Code"], "Alias");
        assert_eq!(alias_using["Static"], false);
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
    fn emits_null_coalescing_expressions() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { string M(string a, string b) { a ??= b; return a ?? b; } }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();

        assert_eq!(
            statements[0]["Expression"]["MetaData"]["Kind"],
            "ast.CoalesceAssignmentExpression"
        );
        assert_eq!(
            statements[1]["Expression"]["MetaData"]["Kind"],
            "ast.CoalesceExpression"
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_shift_expressions() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M(uint u, int a, int b) { var left = a << b; var right = a >> b; var unsignedRight = u >>> b; u>>>b; } }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();

        assert_eq!(
            statements[0]["Declaration"]["Variables"][0]["Initializer"]["Value"]["MetaData"]
                ["Kind"],
            "ast.LeftShiftExpression"
        );
        assert_eq!(
            statements[1]["Declaration"]["Variables"][0]["Initializer"]["Value"]["MetaData"]
                ["Kind"],
            "ast.RightShiftExpression"
        );
        assert_eq!(
            statements[2]["Declaration"]["Variables"][0]["Initializer"]["Value"]["MetaData"]
                ["Kind"],
            "ast.UnsignedRightShiftExpression"
        );
        assert_eq!(
            statements[2]["Declaration"]["Variables"][0]["Initializer"]["Value"]["OperatorToken"]
                ["Value"],
            ">>>"
        );
        assert_eq!(
            statements[3]["Expression"]["MetaData"]["Kind"],
            "ast.UnsignedRightShiftExpression"
        );
        assert_eq!(statements[3]["Expression"]["OperatorToken"]["Value"], ">>>");
        assert_eq!(
            statements[3]["Expression"]["Right"]["MetaData"]["Code"],
            "b"
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_unsigned_shift_assignment_expressions() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M(uint u, int b) { u >>>= b; u>>>=b; } }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();

        for statement in statements {
            assert_eq!(
                statement["Expression"]["MetaData"]["Kind"],
                "ast.UnsignedRightShiftAssignmentExpression"
            );
            assert_eq!(statement["Expression"]["OperatorToken"]["Value"], ">>>=");
            assert_eq!(statement["Expression"]["Left"]["MetaData"]["Code"], "u");
            assert_eq!(statement["Expression"]["Right"]["MetaData"]["Code"], "b");
        }
        assert!(take_unmapped_summary().is_none());
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
    fn emits_expression_bodied_property_indexer_and_accessors() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { int backing; int P => backing + 1; int Q { get => backing + 2; set => backing = value; } int this[int i] => backing + i; }\n",
        )
        .expect("json");
        let members = json["AstRoot"]["Members"][0]["Members"].as_array().unwrap();
        let property_getter = &members[1]["AccessorList"]["Accessors"][0];
        let accessor_getter = &members[2]["AccessorList"]["Accessors"][0];
        let accessor_setter = &members[2]["AccessorList"]["Accessors"][1];
        let indexer_getter = &members[3]["AccessorList"]["Accessors"][0];

        assert_eq!(
            property_getter["Body"]["Statements"][0]["MetaData"]["Kind"],
            "ast.ReturnStatement"
        );
        assert_eq!(
            property_getter["Body"]["Statements"][0]["Expression"]["MetaData"]["Kind"],
            "ast.AddExpression"
        );
        assert_eq!(
            accessor_getter["Body"]["Statements"][0]["MetaData"]["Kind"],
            "ast.ReturnStatement"
        );
        assert_eq!(
            accessor_setter["Body"]["Statements"][0]["MetaData"]["Kind"],
            "ast.ExpressionStatement"
        );
        assert_eq!(
            accessor_setter["Body"]["Statements"][0]["Expression"]["MetaData"]["Kind"],
            "ast.SimpleAssignmentExpression"
        );
        assert_eq!(
            indexer_getter["Body"]["Statements"][0]["MetaData"]["Kind"],
            "ast.ReturnStatement"
        );
    }

    #[test]
    fn emits_interpolation_alignment_and_format_clauses() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { string M(int value) { var s = $\"Value {value,10:X2}!\"; return s; } }\n",
        )
        .expect("json");
        let interpolation = &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0]
            ["Declaration"]["Variables"][0]["Initializer"]["Value"]["Contents"][1];

        assert_eq!(interpolation["MetaData"]["Kind"], "ast.Interpolation");
        assert_eq!(
            interpolation["Expression"]["MetaData"]["Kind"],
            "ast.IdentifierName"
        );
        assert_eq!(
            interpolation["AlignmentClause"]["MetaData"]["Kind"],
            "ast.InterpolationAlignmentClause"
        );
        assert_eq!(
            interpolation["AlignmentClause"]["Expression"]["MetaData"]["Kind"],
            "ast.NumericLiteralExpression"
        );
        assert_eq!(
            interpolation["FormatClause"]["MetaData"]["Kind"],
            "ast.InterpolationFormatClause"
        );
        assert_eq!(
            interpolation["FormatClause"]["FormatStringToken"]["Value"],
            "X2"
        );
    }

    #[test]
    fn emits_raw_string_literal_and_raw_interpolated_metadata() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            r#"class C { void M() { var plain = """abc"""; var value = 7; var raw = $$"""Value {{value}}"""; } }
"#,
        )
        .expect("json");
        let statements = &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"];
        let plain_value = &statements[0]["Declaration"]["Variables"][0]["Initializer"]["Value"];
        let raw_value = &statements[2]["Declaration"]["Variables"][0]["Initializer"]["Value"];

        assert_eq!(
            plain_value["MetaData"]["Kind"],
            "ast.StringLiteralExpression"
        );
        assert_eq!(plain_value["MetaData"]["Code"], r#""""abc""""#);
        assert_eq!(
            raw_value["MetaData"]["Kind"],
            "ast.InterpolatedStringExpression"
        );
        assert_eq!(raw_value["MetaData"]["Code"], r#"$$"""Value {{value}}""""#);
        assert_eq!(raw_value["Contents"].as_array().expect("contents").len(), 2);
        assert_eq!(raw_value["Contents"][0]["TextToken"]["Value"], "Value ");
        assert_eq!(
            raw_value["Contents"][1]["Expression"]["MetaData"]["Code"],
            "value"
        );
    }

    #[test]
    fn emits_optional_catch_declaration_identifier() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "using System; class C { void M() { try { M(); } catch (InvalidOperationException) { M(); } catch (Exception ex) { M(); } catch { M(); } } }\n",
        )
        .expect("json");
        let catches = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0]
            ["Catches"]
            .as_array()
            .unwrap();

        assert_eq!(
            catches[0]["Declaration"]["Type"]["Identifier"]["Value"],
            "InvalidOperationException"
        );
        assert!(catches[0]["Declaration"]["Identifier"].is_null());
        assert_eq!(
            catches[1]["Declaration"]["Type"]["Identifier"]["Value"],
            "Exception"
        );
        assert_eq!(catches[1]["Declaration"]["Identifier"]["Value"], "ex");
        assert!(catches[2]["Declaration"].is_null());
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
    fn emits_base_expression() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class Base { protected int Count; protected void Touch() { } } class Derived : Base { void M() { base.Touch(); var count = base.Count; } }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][1]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();
        let base_call = &statements[0]["Expression"]["Expression"]["Expression"];
        let base_field =
            &statements[1]["Declaration"]["Variables"][0]["Initializer"]["Value"]["Expression"];

        assert_eq!(base_call["MetaData"]["Kind"], "ast.BaseExpression");
        assert_eq!(base_field["MetaData"]["Kind"], "ast.BaseExpression");
    }

    #[test]
    fn emits_conditional_element_access() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M(int[] values) { var x = values?[0]; } }\n",
        )
        .expect("json");
        let expression = &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0]
            ["Declaration"]["Variables"][0]["Initializer"]["Value"];

        assert_eq!(
            expression["MetaData"]["Kind"],
            "ast.ConditionalAccessExpression"
        );
        assert_eq!(
            expression["WhenNotNull"]["MetaData"]["Kind"],
            "ast.ElementAccessExpression"
        );
        assert!(expression["WhenNotNull"]["Expression"].is_null());
        assert_eq!(
            expression["WhenNotNull"]["ArgumentList"]["Arguments"][0]["Expression"]["MetaData"]
                ["Code"],
            "0"
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
    fn emits_switch_expressions() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { int M(int value) { return value switch { > 0 and < 10 => 1, 0 or 10 => 2, _ => 0 }; } }\n",
        )
        .expect("json");
        let switch_expr =
            &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0]["Expression"];
        let arms = switch_expr["Arms"].as_array().unwrap();

        assert_eq!(switch_expr["MetaData"]["Kind"], "ast.SwitchExpression");
        assert_eq!(
            switch_expr["GoverningExpression"]["MetaData"]["Kind"],
            "ast.IdentifierName"
        );
        assert_eq!(arms.len(), 3);
        assert_eq!(arms[0]["Pattern"]["MetaData"]["Kind"], "ast.AndPattern");
        assert_eq!(
            arms[0]["Pattern"]["Left"]["MetaData"]["Kind"],
            "ast.RelationalPattern"
        );
        assert_eq!(
            arms[0]["Pattern"]["Right"]["MetaData"]["Kind"],
            "ast.RelationalPattern"
        );
        assert_eq!(
            arms[0]["Expression"]["MetaData"]["Kind"],
            "ast.NumericLiteralExpression"
        );
        assert_eq!(arms[1]["Pattern"]["MetaData"]["Kind"], "ast.OrPattern");
        assert_eq!(
            arms[1]["Pattern"]["Left"]["MetaData"]["Kind"],
            "ast.ConstantPattern"
        );
        assert_eq!(
            arms[1]["Pattern"]["Right"]["MetaData"]["Kind"],
            "ast.ConstantPattern"
        );
        assert_eq!(
            arms[1]["Expression"]["MetaData"]["Kind"],
            "ast.NumericLiteralExpression"
        );
        assert_eq!(arms[2]["Pattern"]["MetaData"]["Kind"], "ast.DiscardPattern");
        assert_eq!(
            arms[2]["Expression"]["MetaData"]["Kind"],
            "ast.NumericLiteralExpression"
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_list_patterns() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { int M(int[] values) { return values switch { [1, 2] => 1, [1, ..] => 2, [] => 0, _ => -1 }; } }\n",
        )
        .expect("json");
        let switch_expr =
            &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0]["Expression"];
        let arms = switch_expr["Arms"].as_array().unwrap();

        assert_eq!(arms.len(), 4);
        assert_eq!(arms[0]["Pattern"]["MetaData"]["Kind"], "ast.ListPattern");
        assert_eq!(arms[0]["Pattern"]["HasSlice"], false);
        assert_eq!(arms[0]["Pattern"]["SliceIndex"], 2);
        assert_eq!(arms[0]["Pattern"]["Patterns"].as_array().unwrap().len(), 2);
        assert_eq!(
            arms[0]["Pattern"]["Patterns"][0]["MetaData"]["Kind"],
            "ast.ConstantPattern"
        );
        assert_eq!(
            arms[0]["Pattern"]["Patterns"][1]["MetaData"]["Kind"],
            "ast.ConstantPattern"
        );

        assert_eq!(arms[1]["Pattern"]["MetaData"]["Kind"], "ast.ListPattern");
        assert_eq!(arms[1]["Pattern"]["HasSlice"], true);
        assert_eq!(arms[1]["Pattern"]["SliceIndex"], 1);
        assert_eq!(arms[1]["Pattern"]["Patterns"].as_array().unwrap().len(), 1);

        assert_eq!(arms[2]["Pattern"]["MetaData"]["Kind"], "ast.ListPattern");
        assert_eq!(arms[2]["Pattern"]["HasSlice"], false);
        assert_eq!(arms[2]["Pattern"]["Patterns"].as_array().unwrap().len(), 0);
        assert_eq!(arms[3]["Pattern"]["MetaData"]["Kind"], "ast.DiscardPattern");
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_parenthesized_patterns() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { int M(int value) { var positive = value is (> 0); return value switch { (> 10) => 1, _ => 0 }; } }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();
        let is_pattern =
            &statements[0]["Declaration"]["Variables"][0]["Initializer"]["Value"]["Pattern"];
        let switch_expr = &statements[1]["Expression"];
        let arms = switch_expr["Arms"].as_array().unwrap();

        assert_eq!(is_pattern["MetaData"]["Kind"], "ast.ParenthesizedPattern");
        assert_eq!(
            is_pattern["Pattern"]["MetaData"]["Kind"],
            "ast.RelationalPattern"
        );
        assert_eq!(
            arms[0]["Pattern"]["MetaData"]["Kind"],
            "ast.ParenthesizedPattern"
        );
        assert_eq!(
            arms[0]["Pattern"]["Pattern"]["MetaData"]["Kind"],
            "ast.RelationalPattern"
        );
        assert_eq!(arms[1]["Pattern"]["MetaData"]["Kind"], "ast.DiscardPattern");
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_recursive_patterns() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { int M(string s) { var pair = (1, 2); var property = s is { Length: > 3 }; var tuple = pair is (1, > 0); return pair switch { (1, > 0) => 1, (_, _) => 2, _ => 0 }; } }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();
        let property_pattern =
            &statements[1]["Declaration"]["Variables"][0]["Initializer"]["Value"]["Pattern"];
        let tuple_pattern =
            &statements[2]["Declaration"]["Variables"][0]["Initializer"]["Value"]["Pattern"];
        let switch_expr = &statements[3]["Expression"];
        let arms = switch_expr["Arms"].as_array().unwrap();

        assert_eq!(property_pattern["MetaData"]["Kind"], "ast.RecursivePattern");
        assert_eq!(
            property_pattern["PropertyPatterns"][0]["MetaData"]["Kind"],
            "ast.Subpattern"
        );
        assert_eq!(
            property_pattern["PropertyPatterns"][0]["Name"]["Identifier"]["Value"],
            "Length"
        );
        assert_eq!(
            property_pattern["PropertyPatterns"][0]["Pattern"]["MetaData"]["Kind"],
            "ast.RelationalPattern"
        );
        assert_eq!(tuple_pattern["MetaData"]["Kind"], "ast.RecursivePattern");
        assert_eq!(
            tuple_pattern["PositionalPatterns"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            arms[0]["Pattern"]["MetaData"]["Kind"],
            "ast.RecursivePattern"
        );
        assert_eq!(
            arms[1]["Pattern"]["MetaData"]["Kind"],
            "ast.RecursivePattern"
        );
        assert_eq!(
            arms[1]["Pattern"]["PositionalPatterns"][0]["Pattern"]["MetaData"]["Kind"],
            "ast.DiscardPattern"
        );
        assert_eq!(arms[2]["Pattern"]["MetaData"]["Kind"], "ast.DiscardPattern");
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_tuple_designations_and_type_patterns() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { int M(object value, (object first, object second) pair) { if (value is var captured) return 1; if (value is string) return 2; var (left, right) = pair; return pair switch { (var x, _) => 3, (_, string) => 2, _ => 0 }; } }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();
        let deconstruction = &statements[2]["Declaration"]["Variables"][0]["Designation"];
        let switch_expr = &statements[3]["Expression"];
        let arms = switch_expr["Arms"].as_array().unwrap();

        assert_eq!(deconstruction["MetaData"]["Kind"], "ast.TuplePattern");
        assert_eq!(
            deconstruction["Patterns"][0]["MetaData"]["Kind"],
            "ast.SingleVariableDesignation"
        );
        assert_eq!(deconstruction["Patterns"][0]["Identifier"]["Value"], "left");
        assert_eq!(
            deconstruction["Patterns"][1]["Identifier"]["Value"],
            "right"
        );
        assert_eq!(
            arms[0]["Pattern"]["PositionalPatterns"][0]["Pattern"]["MetaData"]["Kind"],
            "ast.DeclarationPattern"
        );
        assert_eq!(
            arms[1]["Pattern"]["PositionalPatterns"][1]["Pattern"]["MetaData"]["Kind"],
            "ast.TypePattern"
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_with_expressions() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { Person M(Person p) { return p with { Age = 2 }; } }\n",
        )
        .expect("json");
        let with_expr =
            &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0]["Expression"];
        let initializers = with_expr["Initializer"]["Expressions"].as_array().unwrap();

        assert_eq!(with_expr["MetaData"]["Kind"], "ast.WithExpression");
        assert_eq!(
            with_expr["Expression"]["MetaData"]["Kind"],
            "ast.IdentifierName"
        );
        assert_eq!(
            with_expr["Initializer"]["MetaData"]["Kind"],
            "ast.ObjectInitializerExpression"
        );
        assert_eq!(initializers.len(), 1);
        assert_eq!(
            initializers[0]["MetaData"]["Kind"],
            "ast.SimpleAssignmentExpression"
        );
        assert_eq!(initializers[0]["Left"]["Identifier"]["Value"], "Age");
        assert_eq!(
            initializers[0]["Right"]["MetaData"]["Kind"],
            "ast.NumericLiteralExpression"
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_object_creation_initializers() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class Widget { public int X; public string Name; public Widget(int seed) { } } class C { void M() { var w = new Widget(7) { X = 1, Name = \"a\" }; } }\n",
        )
        .expect("json");
        let object_creation = &json["AstRoot"]["Members"][1]["Members"][0]["Body"]["Statements"][0]
            ["Declaration"]["Variables"][0]["Initializer"]["Value"];
        let initializer = &object_creation["Initializer"];

        assert_eq!(
            object_creation["MetaData"]["Kind"],
            "ast.ObjectCreationExpression"
        );
        assert_eq!(
            object_creation["ArgumentList"]["Arguments"][0]["Expression"]["MetaData"]["Kind"],
            "ast.NumericLiteralExpression"
        );
        assert_eq!(
            initializer["MetaData"]["Kind"],
            "ast.ObjectInitializerExpression"
        );
        assert_eq!(initializer["Expressions"].as_array().unwrap().len(), 2);
        assert_eq!(
            initializer["Expressions"][0]["MetaData"]["Kind"],
            "ast.SimpleAssignmentExpression"
        );
        assert_eq!(
            initializer["Expressions"][0]["Left"]["Identifier"]["Value"],
            "X"
        );
        assert_eq!(
            initializer["Expressions"][1]["Left"]["Identifier"]["Value"],
            "Name"
        );
        assert!(take_unmapped_summary().is_none());
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
    fn marks_using_declaration_statements() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M() { using Reader reader = Make(), backup = Make(); reader.ReadLine(); } Reader Make() => null; }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();

        assert_eq!(
            statements[0]["MetaData"]["Kind"],
            "ast.LocalDeclarationStatement"
        );
        assert_eq!(statements[0]["Using"], true);
        assert_eq!(
            statements[0]["Declaration"]["Type"]["Identifier"]["Value"],
            "Reader"
        );
        assert_eq!(
            statements[0]["Declaration"]["Variables"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(statements[1]["MetaData"]["Kind"], "ast.ExpressionStatement");
    }

    #[test]
    fn marks_await_using_forms() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { async Task M() { await using var reader = Make(); await using (var backup = Make()) { backup.ToString(); } } IAsyncDisposable Make() => null; }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();

        assert_eq!(
            statements[0]["MetaData"]["Kind"],
            "ast.LocalDeclarationStatement"
        );
        assert_eq!(statements[0]["Using"], true);
        assert_eq!(statements[0]["Await"], true);
        assert_eq!(statements[1]["MetaData"]["Kind"], "ast.UsingStatement");
        assert_eq!(statements[1]["Await"], true);
    }

    #[test]
    fn marks_await_foreach_statements() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { async Task M(IAsyncEnumerable<string> values) { await foreach (var value in values) { value.ToString(); } } }\n",
        )
        .expect("json");
        let statement = &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0];

        assert_eq!(statement["MetaData"]["Kind"], "ast.ForEachStatement");
        assert_eq!(statement["Await"], true);
        assert_eq!(statement["Identifier"]["Value"], "value");
    }

    #[test]
    fn emits_lock_statements() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M(object gate) { lock (gate) { gate.ToString(); } } }\n",
        )
        .expect("json");
        let lock_stmt = &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0];

        assert_eq!(lock_stmt["MetaData"]["Kind"], "ast.LockStatement");
        assert_eq!(
            lock_stmt["Expression"]["MetaData"]["Kind"],
            "ast.IdentifierName"
        );
        assert_eq!(lock_stmt["Expression"]["Identifier"]["Value"], "gate");
        assert_eq!(lock_stmt["Statement"]["MetaData"]["Kind"], "ast.Block");
        assert_eq!(
            lock_stmt["Statement"]["Statements"][0]["MetaData"]["Kind"],
            "ast.ExpressionStatement"
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_checked_and_unchecked_statements() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M() { checked { int i = 1; } unchecked { int j = 2; } } }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();

        assert_eq!(statements[0]["MetaData"]["Kind"], "ast.CheckedStatement");
        assert_eq!(statements[0]["Keyword"]["Value"], "checked");
        assert_eq!(statements[0]["Statement"]["MetaData"]["Kind"], "ast.Block");
        assert_eq!(
            statements[0]["Statement"]["Statements"][0]["MetaData"]["Kind"],
            "ast.LocalDeclarationStatement"
        );
        assert_eq!(statements[1]["MetaData"]["Kind"], "ast.CheckedStatement");
        assert_eq!(statements[1]["Keyword"]["Value"], "unchecked");
        assert_eq!(statements[1]["Statement"]["MetaData"]["Kind"], "ast.Block");
        assert_eq!(
            statements[1]["Statement"]["Statements"][0]["MetaData"]["Kind"],
            "ast.LocalDeclarationStatement"
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_checked_and_unchecked_expressions() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M(int value) { var next = checked(value + 1); var prev = unchecked(value - 1); } }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();
        let checked_expr = &statements[0]["Declaration"]["Variables"][0]["Initializer"]["Value"];
        let unchecked_expr = &statements[1]["Declaration"]["Variables"][0]["Initializer"]["Value"];

        assert_eq!(checked_expr["MetaData"]["Kind"], "ast.CheckedExpression");
        assert_eq!(checked_expr["Keyword"]["Value"], "checked");
        assert_eq!(
            checked_expr["Expression"]["MetaData"]["Kind"],
            "ast.AddExpression"
        );
        assert_eq!(unchecked_expr["MetaData"]["Kind"], "ast.CheckedExpression");
        assert_eq!(unchecked_expr["Keyword"]["Value"], "unchecked");
        assert_eq!(
            unchecked_expr["Expression"]["MetaData"]["Kind"],
            "ast.SubtractExpression"
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_unsafe_statements() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M() { unsafe { int i = 1; } } }\n",
        )
        .expect("json");
        let unsafe_stmt = &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0];

        assert_eq!(unsafe_stmt["MetaData"]["Kind"], "ast.UnsafeStatement");
        assert_eq!(unsafe_stmt["Statement"]["MetaData"]["Kind"], "ast.Block");
        assert_eq!(
            unsafe_stmt["Statement"]["Statements"][0]["MetaData"]["Kind"],
            "ast.LocalDeclarationStatement"
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_pointer_indirection_expression() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { unsafe void M() { int* p = stackalloc int[1]; var value = *p; } }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();
        let indirection = &statements[1]["Declaration"]["Variables"][0]["Initializer"]["Value"];

        assert_eq!(indirection["MetaData"]["Kind"], "ast.IndirectionExpression");
        assert_eq!(indirection["OperatorToken"]["Value"], "*");
        assert_eq!(
            indirection["Operand"]["MetaData"]["Kind"],
            "ast.IdentifierName"
        );
        assert_eq!(indirection["Operand"]["MetaData"]["Code"], "p");
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_fixed_statements_and_pointer_types() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { unsafe void M(int[] xs) { fixed (int* p = xs) { int value = 1; } } }\n",
        )
        .expect("json");
        let fixed_stmt = &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0];

        assert_eq!(fixed_stmt["MetaData"]["Kind"], "ast.FixedStatement");
        assert_eq!(
            fixed_stmt["Declaration"]["MetaData"]["Kind"],
            "ast.VariableDeclaration"
        );
        assert_eq!(
            fixed_stmt["Declaration"]["Type"]["MetaData"]["Kind"],
            "ast.PointerType"
        );
        assert_eq!(
            fixed_stmt["Declaration"]["Type"]["ElementType"]["MetaData"]["Kind"],
            "ast.PredefinedType"
        );
        assert_eq!(
            fixed_stmt["Declaration"]["Variables"][0]["Identifier"]["Value"],
            "p"
        );
        assert_eq!(
            fixed_stmt["Declaration"]["Variables"][0]["Initializer"]["Value"]["MetaData"]["Kind"],
            "ast.IdentifierName"
        );
        assert_eq!(fixed_stmt["Statement"]["MetaData"]["Kind"], "ast.Block");
        assert_eq!(
            fixed_stmt["Statement"]["Statements"][0]["MetaData"]["Kind"],
            "ast.LocalDeclarationStatement"
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_stackalloc_expressions() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { unsafe void M() { int* values = stackalloc int[3]; } }\n",
        )
        .expect("json");
        let stackalloc = &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0]
            ["Declaration"]["Variables"][0]["Initializer"]["Value"];

        assert_eq!(stackalloc["MetaData"]["Kind"], "ast.StackAllocExpression");
        assert_eq!(stackalloc["Type"]["MetaData"]["Kind"], "ast.ArrayType");
        assert_eq!(
            stackalloc["Type"]["ElementType"]["MetaData"]["Kind"],
            "ast.PredefinedType"
        );
        assert_eq!(
            stackalloc["Type"]["Rank"]["MetaData"]["Kind"],
            "ast.ArrayRankSpecifier"
        );
        assert_eq!(
            stackalloc["Type"]["Rank"]["Expressions"][0]["MetaData"]["Kind"],
            "ast.NumericLiteralExpression"
        );
        assert!(stackalloc["Initializer"].is_null());
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_implicit_stackalloc_expressions() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { unsafe void M() { int* values = stackalloc[] { 1, 2 }; } }\n",
        )
        .expect("json");
        let stackalloc = &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0]
            ["Declaration"]["Variables"][0]["Initializer"]["Value"];
        let expressions = stackalloc["Initializer"]["Expressions"].as_array().unwrap();

        assert_eq!(stackalloc["MetaData"]["Kind"], "ast.StackAllocExpression");
        assert!(stackalloc["Type"].is_null());
        assert_eq!(
            stackalloc["Initializer"]["MetaData"]["Kind"],
            "ast.ArrayInitializerExpression"
        );
        assert_eq!(expressions.len(), 2);
        assert_eq!(
            expressions[0]["MetaData"]["Kind"],
            "ast.NumericLiteralExpression"
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_query_expressions() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M(int[] xs) { var q = from x in xs where x > 0 select x; } }\n",
        )
        .expect("json");
        let query = &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0]
            ["Declaration"]["Variables"][0]["Initializer"]["Value"];
        let clauses = query["Clauses"].as_array().unwrap();

        assert_eq!(query["MetaData"]["Kind"], "ast.QueryExpression");
        assert_eq!(query["FromClause"]["MetaData"]["Kind"], "ast.FromClause");
        assert_eq!(query["FromClause"]["Identifier"]["Value"], "x");
        assert_eq!(
            query["FromClause"]["Expression"]["MetaData"]["Kind"],
            "ast.IdentifierName"
        );
        assert_eq!(clauses.len(), 2);
        assert_eq!(clauses[0]["MetaData"]["Kind"], "ast.WhereClause");
        assert_eq!(
            clauses[0]["Condition"]["MetaData"]["Kind"],
            "ast.GreaterThanExpression"
        );
        assert_eq!(clauses[1]["MetaData"]["Kind"], "ast.SelectClause");
        assert_eq!(
            clauses[1]["Expression"]["MetaData"]["Kind"],
            "ast.IdentifierName"
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_query_expression_clauses() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M(int[] xs, int[] ys) { var q = from x in xs let y = x + 1 join z in ys on y equals z into matches from m in matches where m > 0 orderby m descending, y ascending group m by y; } }\n",
        )
        .expect("json");
        let query = &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0]
            ["Declaration"]["Variables"][0]["Initializer"]["Value"];
        let clauses = query["Clauses"].as_array().unwrap();

        assert_eq!(query["MetaData"]["Kind"], "ast.QueryExpression");
        assert_eq!(clauses.len(), 6);
        assert_eq!(clauses[0]["MetaData"]["Kind"], "ast.LetClause");
        assert_eq!(clauses[0]["Identifier"]["Value"], "y");
        assert_eq!(clauses[1]["MetaData"]["Kind"], "ast.JoinClause");
        assert_eq!(clauses[1]["Identifier"]["Value"], "z");
        assert_eq!(clauses[1]["Into"]["MetaData"]["Kind"], "ast.JoinIntoClause");
        assert_eq!(clauses[1]["Into"]["Identifier"]["Value"], "matches");
        assert_eq!(clauses[2]["MetaData"]["Kind"], "ast.FromClause");
        assert_eq!(clauses[3]["MetaData"]["Kind"], "ast.WhereClause");
        assert_eq!(clauses[4]["MetaData"]["Kind"], "ast.OrderByClause");
        assert_eq!(clauses[4]["Expressions"].as_array().unwrap().len(), 2);
        assert_eq!(clauses[4]["Directions"][0], "descending");
        assert_eq!(clauses[4]["Directions"][1], "ascending");
        assert_eq!(clauses[5]["MetaData"]["Kind"], "ast.GroupClause");
        assert_eq!(
            clauses[5]["ByExpression"]["MetaData"]["Kind"],
            "ast.IdentifierName"
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_yield_statements() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { object M(int value) { yield return value; yield break; } }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();

        assert_eq!(statements[0]["MetaData"]["Kind"], "ast.YieldStatement");
        assert_eq!(statements[0]["Keyword"]["Value"], "return");
        assert_eq!(
            statements[0]["Expression"]["MetaData"]["Kind"],
            "ast.IdentifierName"
        );
        assert_eq!(statements[0]["Expression"]["Identifier"]["Value"], "value");
        assert_eq!(statements[1]["MetaData"]["Kind"], "ast.YieldStatement");
        assert_eq!(statements[1]["Keyword"]["Value"], "break");
        assert!(statements[1]["Expression"].is_null());
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_function_pointer_types() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "unsafe class C { delegate* unmanaged[Cdecl]<int, ref string, void> f; }\n",
        )
        .expect("json");
        let typ = &json["AstRoot"]["Members"][0]["Members"][0]["Declaration"]["Type"];

        assert_eq!(typ["MetaData"]["Kind"], "ast.FunctionPointerType");
        assert_eq!(typ["CallingConvention"]["Value"], "unmanaged[Cdecl]");
        assert_eq!(typ["Parameters"].as_array().unwrap().len(), 2);
        assert_eq!(
            typ["Parameters"][0]["MetaData"]["Kind"],
            "ast.FunctionPointerParameter"
        );
        assert!(typ["Parameters"][0]["RefKind"].is_null());
        assert_eq!(
            typ["Parameters"][0]["Type"]["MetaData"]["Kind"],
            "ast.PredefinedType"
        );
        assert_eq!(
            typ["Parameters"][1]["MetaData"]["Kind"],
            "ast.FunctionPointerParameter"
        );
        assert_eq!(typ["Parameters"][1]["RefKind"]["Value"], "ref");
        assert_eq!(
            typ["Parameters"][1]["Type"]["MetaData"]["Kind"],
            "ast.PredefinedType"
        );
        assert_eq!(typ["ReturnType"]["MetaData"]["Kind"], "ast.PredefinedType");
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_empty_statements() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M() { if (true) ; ; } }\n",
        )
        .expect("json");
        let body = &json["AstRoot"]["Members"][0]["Members"][0]["Body"];
        let if_body = &body["Statements"][0]["Statement"];

        assert_eq!(
            body["Statements"][1]["MetaData"]["Kind"],
            "ast.EmptyStatement"
        );
        assert_eq!(body["Statements"][1]["MetaData"]["Code"], ";");
        assert_eq!(if_body["MetaData"]["Kind"], "ast.Block");
        assert_eq!(
            if_body["Statements"][0]["MetaData"]["Kind"],
            "ast.EmptyStatement"
        );
        assert_eq!(if_body["Statements"][0]["MetaData"]["Code"], ";");
        assert!(take_unmapped_summary().is_none());
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
    fn emits_tuple_expression() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M() { var pair = (a: 1, b: 2); } }\n",
        )
        .expect("json");
        let tuple = &json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"][0]
            ["Declaration"]["Variables"][0]["Initializer"]["Value"];

        assert_eq!(tuple["MetaData"]["Kind"], "ast.TupleExpression");
        assert_eq!(tuple["Arguments"].as_array().unwrap().len(), 2);
        assert_eq!(
            tuple["Arguments"][0]["Expression"]["MetaData"]["Kind"],
            "ast.NumericLiteralExpression"
        );
        assert_eq!(
            tuple["Arguments"][1]["Expression"]["MetaData"]["Kind"],
            "ast.NumericLiteralExpression"
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_tuple_types() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { (int a, int b) M((int a, int b) pair) { (string name, int count) local = (\"x\", 1); return pair; } }\n",
        )
        .expect("json");
        let method = &json["AstRoot"]["Members"][0]["Members"][0];
        let return_type = &method["ReturnType"];
        let parameter_type = &method["ParameterList"]["Parameters"][0]["Type"];
        let local_type = &method["Body"]["Statements"][0]["Declaration"]["Type"];

        assert_eq!(return_type["MetaData"]["Kind"], "ast.TupleType");
        assert_eq!(return_type["Elements"].as_array().unwrap().len(), 2);
        assert_eq!(
            return_type["Elements"][0]["MetaData"]["Kind"],
            "ast.TupleElement"
        );
        assert_eq!(
            return_type["Elements"][0]["Type"]["MetaData"]["Kind"],
            "ast.PredefinedType"
        );
        assert_eq!(return_type["Elements"][0]["Identifier"]["Value"], "a");
        assert_eq!(return_type["Elements"][1]["Identifier"]["Value"], "b");
        assert_eq!(parameter_type["MetaData"]["Kind"], "ast.TupleType");
        assert_eq!(local_type["MetaData"]["Kind"], "ast.TupleType");
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_type_operator_expressions() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "using System; class C { void M(object o) { var casted = o as string; var ok = o is string; var typ = typeof(string); var size = sizeof(int); string text = default; var fallback = default(string); Func<int> f = () => throw new Exception(); } }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();
        let initializer =
            |idx: usize| &statements[idx]["Declaration"]["Variables"][0]["Initializer"]["Value"];

        assert_eq!(initializer(0)["MetaData"]["Kind"], "ast.AsExpression");
        assert_eq!(initializer(1)["MetaData"]["Kind"], "ast.IsExpression");
        assert_eq!(initializer(2)["MetaData"]["Kind"], "ast.TypeOfExpression");
        assert_eq!(initializer(3)["MetaData"]["Kind"], "ast.SizeOfExpression");
        assert_eq!(initializer(4)["MetaData"]["Kind"], "ast.DefaultExpression");
        assert!(initializer(4)["Type"].is_null());
        assert_eq!(initializer(5)["MetaData"]["Kind"], "ast.DefaultExpression");
        assert_eq!(
            initializer(5)["Type"]["MetaData"]["Kind"],
            "ast.PredefinedType"
        );
        assert!(contains_kind(&json["AstRoot"], "ast.ThrowExpression"));
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_nameof_expressions() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "class C { void M(int value) { var n = nameof(value); var t = nameof(C); } }\n",
        )
        .expect("json");
        let statements = json["AstRoot"]["Members"][0]["Members"][0]["Body"]["Statements"]
            .as_array()
            .unwrap();
        let initializer =
            |idx: usize| &statements[idx]["Declaration"]["Variables"][0]["Initializer"]["Value"];

        assert_eq!(initializer(0)["MetaData"]["Kind"], "ast.NameOfExpression");
        assert_eq!(initializer(0)["Expression"]["MetaData"]["Code"], "nameof");
        assert_eq!(
            initializer(0)["ArgumentList"]["Arguments"][0]["Expression"]["MetaData"]["Code"],
            "value"
        );
        assert_eq!(initializer(1)["MetaData"]["Kind"], "ast.NameOfExpression");
        assert_eq!(
            initializer(1)["ArgumentList"]["Arguments"][0]["Expression"]["MetaData"]["Code"],
            "C"
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_ref_spread_anonymous_method_and_special_members() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "extern alias Legacy; using System; [assembly: CLSCompliant(true)] class FeatureProbe { public event EventHandler? Changed; public event EventHandler? CustomChanged { add {} remove {} } public int this[int index] { get => index; set { Changed?.Invoke(this, EventArgs.Empty); } } ~FeatureProbe() { } public static FeatureProbe operator +(FeatureProbe left, FeatureProbe right) => left; public static explicit operator int(FeatureProbe value) => 1; void M(object value, int[] xs) { int[] ys = [0, .. xs, 3]; Predicate<int> pred = delegate (int n) { return n > 0; }; ref int r = ref xs[0]; r = ref xs[1]; TypedReference typed = __makeref(value); var refType = __reftype(typed); var refValue = __refvalue(typed, object); } }\n",
        )
        .expect("json");
        let root = &json["AstRoot"];

        for kind in [
            "ast.ExternAliasDirective",
            "ast.GlobalAttribute",
            "ast.EventFieldDeclaration",
            "ast.EventDeclaration",
            "ast.IndexerDeclaration",
            "ast.DestructorDeclaration",
            "ast.OperatorDeclaration",
            "ast.ConversionOperatorDeclaration",
            "ast.AnonymousMethodExpression",
            "ast.SpreadElement",
            "ast.RefType",
            "ast.RefExpression",
            "ast.MakeRefExpression",
            "ast.RefTypeExpression",
            "ast.RefValueExpression",
        ] {
            assert!(contains_kind(root, kind), "missing {kind} in {root}");
        }
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn preserves_modern_type_modifiers() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "file class Hidden { } public sealed partial class Closed { } public ref struct Buffer { public required string Name { get; init; } } public readonly record struct Point(int X, int Y);\n",
        )
        .expect("json");
        let members = json["AstRoot"]["Members"].as_array().unwrap();

        assert_eq!(modifier_values(&members[0]), vec!["file"]);
        assert_eq!(
            modifier_values(&members[1]),
            vec!["public", "sealed", "partial"]
        );
        assert_eq!(modifier_values(&members[2]), vec!["public", "ref"]);
        assert_eq!(
            modifier_values(&members[3]),
            vec!["public", "readonly", "struct"]
        );
        assert_eq!(
            modifier_values(&members[2]["Members"][0]),
            vec!["public", "required"]
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn preserves_parameter_modifiers() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "static class Ext { public static void M(this string text, ref int value, out int written, in int read, params string[] rest) { written = value + read; } }\n",
        )
        .expect("json");
        let params = json["AstRoot"]["Members"][0]["Members"][0]["ParameterList"]["Parameters"]
            .as_array()
            .unwrap();

        assert_eq!(modifier_values(&params[0]), vec!["this"]);
        assert_eq!(modifier_values(&params[1]), vec!["ref"]);
        assert_eq!(modifier_values(&params[2]), vec!["out"]);
        assert_eq!(modifier_values(&params[3]), vec!["in"]);
        assert_eq!(modifier_values(&params[4]), vec!["params"]);
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_preprocessor_directives_and_branch_members() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "#!/usr/bin/env dotnet-script\n#define FEATURE\n#pragma warning disable CS0168\n#nullable enable\n#region R\n#if FEATURE\nclass Enabled { void M() { int value = 1; } }\n#else\n#error disabled\n#endif\n#endregion\n#undef FEATURE\n#line 200 \"Generated.cs\"\n#warning generated\n",
        )
        .expect("json");
        let root = &json["AstRoot"];

        for kind in [
            "ast.ShebangDirective",
            "ast.PreprocessorDirective",
            "ast.PreprocessorIfDirective",
            "ast.PreprocessorElseDirective",
            "ast.ClassDeclaration",
            "ast.MethodDeclaration",
            "ast.LocalDeclarationStatement",
        ] {
            assert!(contains_kind(root, kind), "missing {kind} in {root}");
        }
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_scoped_types() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "using System; class Feature { void Scoped(scoped Span<int> span) { scoped ref int first = ref span[0]; } }\n",
        )
        .expect("json");
        let method = &json["AstRoot"]["Members"][0]["Members"][0];
        let parameter_type = &method["ParameterList"]["Parameters"][0]["Type"];
        let local_type = &method["Body"]["Statements"][0]["Declaration"]["Type"];

        assert_eq!(parameter_type["MetaData"]["Kind"], "ast.ScopedType");
        assert_eq!(
            parameter_type["ElementType"]["MetaData"]["Kind"],
            "ast.GenericName"
        );
        assert_eq!(local_type["MetaData"]["Kind"], "ast.ScopedType");
        assert_eq!(local_type["ElementType"]["MetaData"]["Kind"], "ast.RefType");
        assert!(take_unmapped_summary().is_none());
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
    fn parses_xml_tags_with_many_distinct_attributes_in_bounded_time() {
        use std::time::{Duration, Instant};

        let mut attributes = String::new();
        for index in 0..20_000 {
            attributes.push_str(&format!(r#" attribute{index}="value""#));
        }
        let xml =
            format!(r#"<doc><members><member name="T:Example.Type"{attributes}/></members></doc>"#);

        let started = Instant::now();
        let summary = generate_xml_summary(&xml).expect("summary");

        assert_eq!(summary["Example"][0]["name"], "Example.Type");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "attribute parsing exceeded the regression bound"
        );
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
    fn emits_delegate_declarations() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "[Obsolete] public delegate TResult Projector<T, TResult>(T item) where T : class;\n",
        )
        .expect("json");
        let delegate = &json["AstRoot"]["Members"][0];

        assert_eq!(delegate["MetaData"]["Kind"], "ast.DelegateDeclaration");
        assert_eq!(delegate["Identifier"]["Value"], "Projector");
        assert_eq!(
            delegate["ReturnType"]["MetaData"]["Kind"],
            "ast.IdentifierName"
        );
        assert_eq!(delegate["ReturnType"]["Identifier"]["Value"], "TResult");
        assert_eq!(
            delegate["ParameterList"]["MetaData"]["Kind"],
            "ast.ParameterList"
        );
        assert_eq!(
            delegate["ParameterList"]["Parameters"][0]["Identifier"]["Value"],
            "item"
        );
        assert_eq!(
            delegate["TypeParameterList"]["Parameters"][0]["Identifier"]["Value"],
            "T"
        );
        assert_eq!(
            delegate["ConstraintClauses"][0]["MetaData"]["Kind"],
            "ast.TypeParameterConstraintClause"
        );
        assert_eq!(delegate["AttributeLists"].as_array().unwrap().len(), 1);
        assert!(take_unmapped_summary().is_none());
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
            "[Example(positional: \"x\", Name = \"y\")] class Foo { }\n",
        )
        .expect("json");
        let arguments = json["AstRoot"]["Members"][0]["AttributeLists"][0]["Attributes"][0]
            ["ArgumentList"]["Arguments"]
            .as_array()
            .unwrap();

        let colon_argument = &arguments[0];
        assert_eq!(colon_argument["MetaData"]["Kind"], "ast.AttributeArgument");
        assert_eq!(
            colon_argument["NameColon"]["MetaData"]["Kind"],
            "ast.NameColon"
        );
        assert_eq!(
            colon_argument["NameColon"]["Name"]["Identifier"]["Value"],
            "positional"
        );
        assert!(colon_argument["NameEquals"].is_null());

        let equals_argument = &arguments[1];
        assert_eq!(
            equals_argument["NameEquals"]["MetaData"]["Kind"],
            "ast.NameEquals"
        );
        assert_eq!(
            equals_argument["NameEquals"]["Name"]["Identifier"]["Value"],
            "Name"
        );
        assert!(equals_argument["NameColon"].is_null());
    }

    #[test]
    fn emits_attribute_target_specifiers() {
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "using System;\n[assembly: CLSCompliant(true)]\n[module: Sample]\nclass C { [return: Sample] public int M([param: Sample] int x) { return x; } }\nclass SampleAttribute : Attribute { }\n",
        )
        .expect("json");
        let members = json["AstRoot"]["Members"].as_array().unwrap();

        let assembly_attribute_list = &members[0]["AttributeLists"][0];
        assert_eq!(
            assembly_attribute_list["Target"]["Identifier"]["Value"],
            "assembly"
        );
        assert_eq!(
            assembly_attribute_list["Attributes"][0]["Name"]["Identifier"]["Value"],
            "CLSCompliant"
        );

        let module_attribute_list = &members[1]["AttributeLists"][0];
        assert_eq!(
            module_attribute_list["Target"]["Identifier"]["Value"],
            "module"
        );
        assert_eq!(
            module_attribute_list["Attributes"][0]["Name"]["Identifier"]["Value"],
            "Sample"
        );

        let method = &members[2]["Members"][0];
        let return_attribute_list = &method["AttributeLists"][0];
        assert_eq!(
            return_attribute_list["Target"]["Identifier"]["Value"],
            "return"
        );
        assert_eq!(
            return_attribute_list["Attributes"][0]["Name"]["Identifier"]["Value"],
            "Sample"
        );

        let parameter = &method["ParameterList"]["Parameters"][0];
        let parameter_attribute_list = &parameter["AttributeLists"][0];
        assert_eq!(
            parameter_attribute_list["Target"]["Identifier"]["Value"],
            "param"
        );
        assert_eq!(
            parameter_attribute_list["Attributes"][0]["Name"]["Identifier"]["Value"],
            "Sample"
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
    fn emits_constructor_initializers_explicit_interfaces_and_catch_filters() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            r#"using System;
interface IWorker { void Work(); int Count { get; } int this[int index] { get; } }
class BaseWorker { public BaseWorker(int seed) { } }
class Worker(int seed) : BaseWorker(seed), IWorker
{
    public Worker() : this(1) { }
    public Worker(string text) : base(text.Length) { }
    void IWorker.Work() { }
    int IWorker.Count => seed;
    int IWorker.this[int index] => index + seed;
    public void Run(Action action)
    {
        try { action(); }
        catch (InvalidOperationException ex) when (ex.Message != null) { Console.WriteLine(ex.Message); }
    }
}
"#,
        )
        .expect("json");
        let worker = &json["AstRoot"]["Members"][2];

        assert_eq!(
            worker["ParameterList"]["Parameters"][0]["Identifier"]["Value"],
            "seed"
        );
        assert_eq!(
            worker["ParameterList"]["Parameters"][0]["Type"]["MetaData"]["Kind"],
            "ast.PredefinedType"
        );
        assert_eq!(
            worker["BaseList"]["Types"][0]["MetaData"]["Kind"],
            "ast.PrimaryConstructorBaseType"
        );
        assert_eq!(
            worker["BaseList"]["Types"][0]["Type"]["Identifier"]["Value"],
            "BaseWorker"
        );
        assert_eq!(
            worker["BaseList"]["Types"][0]["ArgumentList"]["Arguments"][0]["Expression"]
                ["Identifier"]["Value"],
            "seed"
        );
        assert_eq!(
            worker["BaseList"]["Types"][1]["MetaData"]["Kind"],
            "ast.SimpleBaseType"
        );

        let members = worker["Members"].as_array().unwrap();
        assert_eq!(
            members[0]["Initializer"]["MetaData"]["Kind"],
            "ast.ThisConstructorInitializer"
        );
        assert_eq!(
            members[0]["Initializer"]["ArgumentList"]["Arguments"][0]["Expression"]["MetaData"]
                ["Kind"],
            "ast.NumericLiteralExpression"
        );
        assert_eq!(
            members[1]["Initializer"]["MetaData"]["Kind"],
            "ast.BaseConstructorInitializer"
        );
        assert_eq!(
            members[1]["Initializer"]["ArgumentList"]["Arguments"][0]["Expression"]["MetaData"]
                ["Kind"],
            "ast.SimpleMemberAccessExpression"
        );
        assert_eq!(
            members[2]["ExplicitInterfaceSpecifier"]["MetaData"]["Kind"],
            "ast.ExplicitInterfaceSpecifier"
        );
        assert_eq!(
            members[2]["ExplicitInterfaceSpecifier"]["Name"]["Identifier"]["Value"],
            "IWorker"
        );
        assert_eq!(
            members[3]["ExplicitInterfaceSpecifier"]["Name"]["Identifier"]["Value"],
            "IWorker"
        );
        assert_eq!(
            members[4]["ExplicitInterfaceSpecifier"]["Name"]["Identifier"]["Value"],
            "IWorker"
        );

        let catch_clause = &members[5]["Body"]["Statements"][0]["Catches"][0];
        assert_eq!(
            catch_clause["Filter"]["MetaData"]["Kind"],
            "ast.CatchFilterClause"
        );
        assert_eq!(
            catch_clause["Filter"]["Condition"]["MetaData"]["Kind"],
            "ast.NotEqualsExpression"
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn emits_record_primary_constructor_base_type() {
        let _ = take_unmapped_summary();
        let json = generate_source(
            Path::new("/tmp/Test.cs"),
            "record Person(string Name); record Employee(string Name, int Id) : Person(Name);\n",
        )
        .expect("json");
        let employee = &json["AstRoot"]["Members"][1];

        assert_eq!(employee["MetaData"]["Kind"], "ast.RecordDeclaration");
        assert_eq!(
            employee["ParameterList"]["Parameters"][0]["Identifier"]["Value"],
            "Name"
        );
        assert_eq!(
            employee["ParameterList"]["Parameters"][1]["Identifier"]["Value"],
            "Id"
        );
        assert_eq!(
            employee["BaseList"]["Types"][0]["MetaData"]["Kind"],
            "ast.PrimaryConstructorBaseType"
        );
        assert_eq!(
            employee["BaseList"]["Types"][0]["Type"]["Identifier"]["Value"],
            "Person"
        );
        assert_eq!(
            employee["BaseList"]["Types"][0]["ArgumentList"]["Arguments"][0]["Expression"]
                ["Identifier"]["Value"],
            "Name"
        );
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn records_unmapped_node_kinds_in_summary() {
        // Drain any residual counts from earlier tests sharing this thread.
        let _ = take_unmapped_summary();
        record_unmapped(BTreeMap::from([("synthetic_unmapped_node".to_string(), 1)]));

        let summary = take_unmapped_summary().expect("expected an unmapped summary");
        assert!(
            summary.starts_with("dotnetastgen: "),
            "unexpected summary: {summary}"
        );
        assert!(
            summary.contains("synthetic_unmapped_node(x1)"),
            "unexpected summary: {summary}"
        );
        // The counter is drained on read.
        assert!(take_unmapped_summary().is_none());
    }
}
