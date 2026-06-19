use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use tree_sitter::{Node, Parser};

pub fn parse_file(input_root: &Path, file: &Path) -> Result<Value> {
    let source =
        fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))?;
    let relative = relative_file_path(input_root, file);
    let full_path = file
        .canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .into_owned();
    parse_source(&relative, &full_path, &source)
}

pub fn parse_source(relative_file_path: &str, full_file_path: &str, source: &str) -> Result<Value> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_swift::LANGUAGE.into())
        .context("failed to initialize Swift tree-sitter grammar")?;
    let tree = parser
        .parse(source, None)
        .context("Swift parser returned no tree")?;
    let root = tree.root_node();

    let emitter = SwiftSyntaxEmitter::new(source);
    emitter.source_file(root, relative_file_path, full_file_path)
}

pub fn write_json(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
}

fn relative_file_path(input_root: &Path, file: &Path) -> String {
    let base = if input_root.is_dir() {
        input_root
    } else {
        input_root.parent().unwrap_or(input_root)
    };
    file.strip_prefix(base)
        .unwrap_or(file)
        .to_string_lossy()
        .into_owned()
}

struct SwiftSyntaxEmitter<'a> {
    source: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> SwiftSyntaxEmitter<'a> {
    fn new(source: &'a str) -> Self {
        let mut line_starts = vec![0];
        for (index, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }
        Self {
            source,
            line_starts,
        }
    }

    fn source_file(
        &self,
        root: Node<'a>,
        relative_file_path: &str,
        full_file_path: &str,
    ) -> Result<Value> {
        let mut statement_items = Vec::new();
        for child in named_children(root) {
            if is_trivia_node(child) || self.is_ignorable_top_level_error(child) {
                continue;
            }
            statement_items.push(self.code_block_item(child)?);
        }

        let statements_range = self.covering_range_or_point(&statement_items, root.start_byte());
        let statements = self.with_name(
            self.syntax_node("CodeBlockItemListSyntax", statements_range, statement_items),
            "statements",
        );
        let eof = self.with_name(
            self.token_with_range("endOfFile", self.point_range(self.source.len())),
            "endOfFileToken",
        );

        let mut root_obj = self.syntax_node(
            "SourceFileSyntax",
            self.range_for_node(root),
            vec![statements, eof],
        );
        let obj = root_obj
            .as_object_mut()
            .expect("syntax_node always returns a JSON object");
        obj.insert(
            "relativeFilePath".into(),
            Value::String(relative_file_path.into()),
        );
        obj.insert("fullFilePath".into(), Value::String(full_file_path.into()));
        obj.insert("content".into(), Value::String(self.source.into()));
        obj.insert(
            "loc".into(),
            json!(self.source.bytes().filter(|b| *b == b'\n').count() + 1),
        );
        Ok(root_obj)
    }

    fn is_ignorable_top_level_error(&self, node: Node<'a>) -> bool {
        node.kind() == "ERROR"
            && self.text(node).trim() == "}"
            && named_children(node).all(is_trivia_node)
    }

    fn code_block_item(&self, node: Node<'a>) -> Result<Value> {
        let item = self.with_name(self.syntax_for_statement(node)?, "item");
        Ok(self.syntax_node("CodeBlockItemSyntax", self.range_for_node(node), vec![item]))
    }

    fn syntax_for_statement(&self, node: Node<'a>) -> Result<Value> {
        match node.kind() {
            "property_declaration" => self.variable_decl(node),
            "function_declaration" => self.function_decl(node),
            "class_declaration" => self.nominal_type_decl(node),
            "control_transfer_statement" => self.control_transfer_stmt(node),
            "for_statement" => self.for_stmt(node),
            "if_statement" => self.if_expr(node),
            "import_declaration" => self.import_decl(node),
            "while_statement" => self.while_stmt(node),
            "assignment"
            | "additive_expression"
            | "array_literal"
            | "boolean_literal"
            | "call_expression"
            | "comparison_expression"
            | "conjunction_expression"
            | "dictionary_literal"
            | "disjunction_expression"
            | "equality_expression"
            | "integer_literal"
            | "lambda_literal"
            | "multiplicative_expression"
            | "prefix_expression"
            | "range_expression"
            | "real_literal"
            | "simple_identifier"
            | "self_expression"
            | "tuple_expression"
            | "navigation_expression"
            | "line_string_literal" => self.expr(node),
            other => bail!("unsupported Swift syntax node '{other}'"),
        }
    }

    fn syntax_for_member_decl(&self, node: Node<'a>) -> Result<Value> {
        match node.kind() {
            "property_declaration" => self.variable_decl(node),
            "function_declaration" => self.function_decl(node),
            "class_declaration" => self.nominal_type_decl(node),
            other => bail!("unsupported Swift member declaration node '{other}'"),
        }
    }

    fn nominal_type_decl(&self, node: Node<'a>) -> Result<Value> {
        let declaration_kind = self
            .field_child(node, "declaration_kind")
            .context("nominal type declaration is missing declaration kind")?;
        let (node_type, keyword_name, keyword_kind) = match declaration_kind.kind() {
            "class" => (
                "ClassDeclSyntax",
                "classKeyword",
                "keyword(SwiftSyntax.Keyword.class)",
            ),
            "struct" => (
                "StructDeclSyntax",
                "structKeyword",
                "keyword(SwiftSyntax.Keyword.struct)",
            ),
            other => bail!("unsupported nominal type declaration kind '{other}'"),
        };
        let name_node = self
            .field_child(node, "name")
            .context("nominal type declaration is missing a name")?;
        let name = match name_node.kind() {
            "type_identifier" | "simple_identifier" => name_node,
            _ => self
                .first_descendant_kind(name_node, "type_identifier")
                .or_else(|| self.first_descendant_kind(name_node, "simple_identifier"))
                .context("nominal type declaration name is missing an identifier")?,
        };
        let body = self
            .field_child(node, "body")
            .context("nominal type declaration is missing a body")?;

        Ok(self.syntax_node(
            node_type,
            self.range_for_node(node),
            vec![
                self.with_name(self.attribute_list(node)?, "attributes"),
                self.with_name(self.modifier_list(node), "modifiers"),
                self.with_name(
                    self.token_for_node(declaration_kind, keyword_kind),
                    keyword_name,
                ),
                self.with_name(
                    self.token_for_node(
                        name,
                        &format!("identifier({})", quoted_text(self.text(name))),
                    ),
                    "name",
                ),
                self.with_name(self.member_block(body)?, "memberBlock"),
            ],
        ))
    }

    fn member_block(&self, node: Node<'a>) -> Result<Value> {
        let left_brace = self
            .immediate_child_kind(node, "{")
            .context("member block is missing '{'")?;
        let right_brace = self
            .immediate_child_kind(node, "}")
            .context("member block is missing '}'")?;
        let mut items = Vec::new();
        for child in named_children(node) {
            if is_trivia_node(child) {
                continue;
            }
            items.push(self.member_block_item(child)?);
        }
        let members_range = self.covering_range_or_point(&items, left_brace.end_byte());
        Ok(self.syntax_node(
            "MemberBlockSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.token_for_node(left_brace, "leftBrace"), "leftBrace"),
                self.with_name(
                    self.syntax_node("MemberBlockItemListSyntax", members_range, items),
                    "members",
                ),
                self.with_name(self.token_for_node(right_brace, "rightBrace"), "rightBrace"),
            ],
        ))
    }

    fn member_block_item(&self, node: Node<'a>) -> Result<Value> {
        Ok(self.syntax_node(
            "MemberBlockItemSyntax",
            self.range_for_node(node),
            vec![self.with_name(self.syntax_for_member_decl(node)?, "decl")],
        ))
    }

    fn attribute_list(&self, node: Node<'a>) -> Result<Value> {
        let mut attributes = Vec::new();
        for modifiers in named_children(node).filter(|child| child.kind() == "modifiers") {
            for attribute in named_children(modifiers).filter(|child| child.kind() == "attribute") {
                attributes.push(self.with_name(self.attribute(attribute)?, ""));
            }
        }
        let range = self.covering_range_or_point(&attributes, node.start_byte());
        Ok(self.syntax_node("AttributeListSyntax", range, attributes))
    }

    fn attribute(&self, node: Node<'a>) -> Result<Value> {
        let at_sign = self
            .immediate_child_kind(node, "@")
            .context("attribute is missing '@'")?;
        let name = self
            .immediate_named_child_kind(node, "user_type")
            .or_else(|| self.first_descendant_kind(node, "type_identifier"))
            .context("attribute is missing a name")?;
        let mut children = vec![
            self.with_name(self.token_for_node(at_sign, "atSign"), "atSign"),
            self.with_name(self.identifier_type(name)?, "attributeName"),
        ];
        if let Some(left_paren) = self.immediate_child_kind(node, "(") {
            children
                .push(self.with_name(self.token_for_node(left_paren, "leftParen"), "leftParen"));
        }
        if let Some(right_paren) = self.immediate_child_kind(node, ")") {
            children
                .push(self.with_name(self.token_for_node(right_paren, "rightParen"), "rightParen"));
        }

        Ok(self.syntax_node("AttributeSyntax", self.range_for_node(node), children))
    }

    fn modifier_list(&self, node: Node<'a>) -> Value {
        let mut modifiers = Vec::new();
        for modifier_container in named_children(node).filter(|child| child.kind() == "modifiers") {
            for modifier in
                named_children(modifier_container).filter(|child| child.kind() != "attribute")
            {
                modifiers.push(self.with_name(self.decl_modifier(modifier), ""));
            }
        }
        let range = self.covering_range_or_point(&modifiers, node.start_byte());
        self.syntax_node("DeclModifierListSyntax", range, modifiers)
    }

    fn decl_modifier(&self, node: Node<'a>) -> Value {
        self.syntax_node(
            "DeclModifierSyntax",
            self.range_for_node(node),
            vec![self.with_name(
                self.token_for_node(
                    node,
                    &format!("keyword(SwiftSyntax.Keyword.{})", self.text(node)),
                ),
                "name",
            )],
        )
    }

    fn import_decl(&self, node: Node<'a>) -> Result<Value> {
        let import_keyword = self
            .immediate_child_kind(node, "import")
            .context("import declaration is missing import keyword")?;
        let path = named_children(node)
            .find(|child| {
                child.kind() == "identifier" && child.start_byte() > import_keyword.end_byte()
            })
            .context("import declaration is missing import path")?;

        let mut decl_children = vec![
            self.with_name(self.attribute_list(node)?, "attributes"),
            self.with_name(self.modifier_list(node), "modifiers"),
            self.with_name(
                self.token_for_node(import_keyword, "keyword(SwiftSyntax.Keyword.import)"),
                "importKeyword",
            ),
        ];

        if let Some(kind) = children(node).find(|child| {
            child.start_byte() > import_keyword.end_byte()
                && child.end_byte() <= path.start_byte()
                && matches!(
                    child.kind(),
                    "typealias" | "struct" | "class" | "enum" | "protocol" | "let" | "var" | "func"
                )
        }) {
            decl_children.push(self.with_name(
                self.token_for_node(
                    kind,
                    &format!("keyword(SwiftSyntax.Keyword.{})", kind.kind()),
                ),
                "importKindSpecifier",
            ));
        }

        decl_children.push(self.with_name(self.import_path(path), "path"));

        Ok(self.syntax_node("ImportDeclSyntax", self.range_for_node(node), decl_children))
    }

    fn import_path(&self, node: Node<'a>) -> Value {
        let path_children: Vec<_> = named_children(node)
            .filter(|child| child.kind() == "simple_identifier")
            .collect();
        let mut components = Vec::new();

        for (index, component) in path_children.iter().enumerate() {
            let mut component_children = vec![self.with_name(
                self.token_for_node(
                    *component,
                    &format!("identifier({})", quoted_text(self.text(*component))),
                ),
                "name",
            )];

            if let Some(next_component) = path_children.get(index + 1) {
                if let Some(period) = self
                    .children_between(node, component.end_byte(), next_component.start_byte())
                    .into_iter()
                    .find(|child| child.kind() == "." || child.kind() == "::")
                {
                    let token_kind = if period.kind() == "::" {
                        "colonColon"
                    } else {
                        "period"
                    };
                    component_children.push(
                        self.with_name(self.token_for_node(period, token_kind), "trailingPeriod"),
                    );
                }
            }

            let component_end = component_children
                .last()
                .map(end_offset)
                .unwrap_or_else(|| component.end_byte());
            components.push(self.with_name(
                self.syntax_node(
                    "ImportPathComponentSyntax",
                    self.range_from_offsets(component.start_byte(), component_end),
                    component_children,
                ),
                "",
            ));
        }

        self.syntax_node(
            "ImportPathComponentListSyntax",
            self.range_for_node(node),
            components,
        )
    }

    fn variable_decl(&self, node: Node<'a>) -> Result<Value> {
        let binding_keyword = self
            .first_descendant_any_kind(node, "let")
            .or_else(|| self.first_descendant_any_kind(node, "var"))
            .context("property declaration is missing let/var")?;
        let pattern_node = self
            .field_child(node, "name")
            .context("property declaration is missing a name")?;
        let type_annotation_node = self.immediate_named_child_kind(node, "type_annotation");
        let value_node = self.field_child(node, "value");

        let mut binding_children = vec![self.with_name(self.pattern(pattern_node)?, "pattern")];
        if let Some(type_node) = type_annotation_node {
            binding_children
                .push(self.with_name(self.type_annotation(type_node)?, "typeAnnotation"));
        }
        if let Some(value) = value_node {
            let equal = self
                .immediate_child_kind(node, "=")
                .context("property initializer is missing '='")?;
            binding_children
                .push(self.with_name(self.initializer_clause(equal, value)?, "initializer"));
        }

        let binding_range = self.range_from_offsets(
            pattern_node.start_byte(),
            value_node
                .or(type_annotation_node)
                .unwrap_or(pattern_node)
                .end_byte(),
        );
        let binding = self.syntax_node("PatternBindingSyntax", binding_range, binding_children);
        let bindings = self.with_name(
            self.syntax_node(
                "PatternBindingListSyntax",
                self.range_for_node(pattern_node),
                vec![self.with_name(binding, "")],
            ),
            "bindings",
        );

        Ok(self.syntax_node(
            "VariableDeclSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.attribute_list(node)?, "attributes"),
                self.with_name(self.modifier_list(node), "modifiers"),
                self.with_name(
                    self.token_for_node(
                        binding_keyword,
                        &format!("keyword(SwiftSyntax.Keyword.{})", binding_keyword.kind()),
                    ),
                    "bindingSpecifier",
                ),
                bindings,
            ],
        ))
    }

    fn pattern(&self, node: Node<'a>) -> Result<Value> {
        match node.kind() {
            "pattern" if self.immediate_child_kind(node, "(").is_some() => self.tuple_pattern(node),
            "pattern" => {
                let child = named_children(node)
                    .find(|child| {
                        matches!(
                            child.kind(),
                            "identifier" | "pattern" | "simple_identifier" | "wildcard_pattern"
                        )
                    })
                    .context("pattern is empty")?;
                self.pattern(child)
            }
            "identifier" | "simple_identifier" => self.identifier_pattern(node),
            other => bail!("unsupported Swift pattern node '{other}'"),
        }
    }

    fn identifier_pattern(&self, node: Node<'a>) -> Result<Value> {
        let identifier = self
            .first_descendant_kind(node, "simple_identifier")
            .or_else(|| self.first_descendant_kind(node, "identifier"))
            .context("pattern is missing an identifier")?;
        Ok(self.syntax_node(
            "IdentifierPatternSyntax",
            self.range_for_node(identifier),
            vec![self.with_name(
                self.token_for_node(
                    identifier,
                    &format!("identifier({})", quoted_text(self.text(identifier))),
                ),
                "identifier",
            )],
        ))
    }

    fn type_annotation(&self, node: Node<'a>) -> Result<Value> {
        let colon = self
            .immediate_child_kind(node, ":")
            .context("type annotation is missing ':'")?;
        let type_node = self
            .field_child(node, "type")
            .or_else(|| self.field_child(node, "name"))
            .or_else(|| self.first_named_child_excluding(node, &["type_identifier"]))
            .or_else(|| self.first_descendant_kind(node, "user_type"))
            .or_else(|| self.first_descendant_kind(node, "type_identifier"))
            .context("type annotation is missing a type")?;
        Ok(self.syntax_node(
            "TypeAnnotationSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.token_for_node(colon, "colon"), "colon"),
                self.with_name(self.identifier_type(type_node)?, "type"),
            ],
        ))
    }

    fn tuple_pattern(&self, node: Node<'a>) -> Result<Value> {
        let left_paren = self
            .immediate_child_kind(node, "(")
            .context("tuple pattern is missing '('")?;
        let right_paren = self
            .immediate_child_kind(node, ")")
            .context("tuple pattern is missing ')'")?;
        let mut elements = Vec::new();
        for child in named_children(node).filter(|child| {
            child.start_byte() >= left_paren.end_byte()
                && child.end_byte() <= right_paren.start_byte()
        }) {
            if child.kind() == "type_annotation" {
                continue;
            }
            let trailing_comma = self.trailing_delimiter(node, child, ",");
            let mut element_children = vec![self.with_name(self.pattern(child)?, "pattern")];
            if let Some(comma) = trailing_comma {
                element_children
                    .push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
            }
            let element_end = trailing_comma.map_or(child.end_byte(), |comma| comma.end_byte());
            elements.push(self.with_name(
                self.syntax_node(
                    "TuplePatternElementSyntax",
                    self.range_from_offsets(child.start_byte(), element_end),
                    element_children,
                ),
                "",
            ));
        }

        Ok(self.syntax_node(
            "TuplePatternSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.token_for_node(left_paren, "leftParen"), "leftParen"),
                self.with_name(
                    self.syntax_node(
                        "TuplePatternElementListSyntax",
                        self.range_from_offsets(left_paren.end_byte(), right_paren.start_byte()),
                        elements,
                    ),
                    "elements",
                ),
                self.with_name(self.token_for_node(right_paren, "rightParen"), "rightParen"),
            ],
        ))
    }

    fn identifier_type(&self, node: Node<'a>) -> Result<Value> {
        let name = match node.kind() {
            "type_identifier" => node,
            _ => self
                .first_descendant_kind(node, "type_identifier")
                .context("type node is missing type_identifier")?,
        };
        Ok(self.syntax_node(
            "IdentifierTypeSyntax",
            self.range_for_node(node),
            vec![self.with_name(
                self.token_for_node(
                    name,
                    &format!("identifier({})", quoted_text(self.text(name))),
                ),
                "name",
            )],
        ))
    }

    fn initializer_clause(&self, equal: Node<'a>, value: Node<'a>) -> Result<Value> {
        Ok(self.syntax_node(
            "InitializerClauseSyntax",
            self.range_from_offsets(equal.start_byte(), value.end_byte()),
            vec![
                self.with_name(self.token_for_node(equal, "equal"), "equal"),
                self.with_name(self.expr(value)?, "value"),
            ],
        ))
    }

    fn function_decl(&self, node: Node<'a>) -> Result<Value> {
        let func_keyword = self
            .immediate_child_kind(node, "func")
            .context("function declaration is missing 'func'")?;
        let name = self
            .field_child(node, "name")
            .and_then(|n| {
                self.first_descendant_kind(n, "simple_identifier")
                    .or(Some(n))
            })
            .context("function declaration is missing a name")?;
        let body = self.field_child(node, "body");

        let mut children = vec![
            self.with_name(self.attribute_list(node)?, "attributes"),
            self.with_name(self.modifier_list(node), "modifiers"),
            self.with_name(
                self.token_for_node(func_keyword, "keyword(SwiftSyntax.Keyword.func)"),
                "funcKeyword",
            ),
            self.with_name(
                self.token_for_node(
                    name,
                    &format!("identifier({})", quoted_text(self.text(name))),
                ),
                "name",
            ),
            self.with_name(self.function_signature(node)?, "signature"),
        ];
        if let Some(body_node) = body {
            children.push(self.with_name(self.code_block(body_node)?, "body"));
        }
        Ok(self.syntax_node("FunctionDeclSyntax", self.range_for_node(node), children))
    }

    fn function_signature(&self, node: Node<'a>) -> Result<Value> {
        let left_paren = self
            .immediate_child_kind(node, "(")
            .context("function signature is missing '('")?;
        let right_paren = self
            .immediate_child_kind(node, ")")
            .context("function signature is missing ')'")?;

        let mut parameters = Vec::new();
        for param in named_children(node).filter(|child| child.kind() == "parameter") {
            parameters.push(self.with_name(self.function_parameter(param)?, ""));
        }
        let parameter_list = self.with_name(
            self.syntax_node(
                "FunctionParameterListSyntax",
                self.range_from_offsets(left_paren.end_byte(), right_paren.start_byte()),
                parameters,
            ),
            "parameters",
        );
        let parameter_clause = self.with_name(
            self.syntax_node(
                "FunctionParameterClauseSyntax",
                self.range_from_offsets(left_paren.start_byte(), right_paren.end_byte()),
                vec![
                    self.with_name(self.token_for_node(left_paren, "leftParen"), "leftParen"),
                    parameter_list,
                    self.with_name(self.token_for_node(right_paren, "rightParen"), "rightParen"),
                ],
            ),
            "parameterClause",
        );

        let mut signature_children = vec![parameter_clause];
        if let Some(return_type) = self.field_child(node, "return_type") {
            if let Some(arrow) = self.immediate_child_kind(node, "->") {
                signature_children.push(self.with_name(
                    self.syntax_node(
                        "ReturnClauseSyntax",
                        self.range_from_offsets(arrow.start_byte(), return_type.end_byte()),
                        vec![
                            self.with_name(self.token_for_node(arrow, "arrow"), "arrow"),
                            self.with_name(self.identifier_type(return_type)?, "type"),
                        ],
                    ),
                    "returnClause",
                ));
            }
        }

        let end = signature_children
            .last()
            .map_or(right_paren.end_byte(), end_offset);
        Ok(self.syntax_node(
            "FunctionSignatureSyntax",
            self.range_from_offsets(left_paren.start_byte(), end),
            signature_children,
        ))
    }

    fn function_parameter(&self, node: Node<'a>) -> Result<Value> {
        let name = self
            .field_child(node, "name")
            .and_then(|n| {
                self.first_descendant_kind(n, "simple_identifier")
                    .or(Some(n))
            })
            .context("function parameter is missing a name")?;
        let colon = self
            .immediate_child_kind(node, ":")
            .context("function parameter is missing ':'")?;
        let type_node = self
            .field_child(node, "type")
            .context("function parameter is missing a type")?;
        Ok(self.syntax_node(
            "FunctionParameterSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.attribute_list(node)?, "attributes"),
                self.with_name(self.modifier_list(node), "modifiers"),
                self.with_name(
                    self.token_for_node(
                        name,
                        &format!("identifier({})", quoted_text(self.text(name))),
                    ),
                    "firstName",
                ),
                self.with_name(self.token_for_node(colon, "colon"), "colon"),
                self.with_name(self.identifier_type(type_node)?, "type"),
            ],
        ))
    }

    fn code_block(&self, node: Node<'a>) -> Result<Value> {
        let left_brace = self
            .immediate_child_kind(node, "{")
            .context("function body is missing '{'")?;
        let right_brace = self
            .immediate_child_kind(node, "}")
            .context("function body is missing '}'")?;
        let statements = named_children(node).find(|child| child.kind() == "statements");
        self.code_block_from_statements(statements, left_brace, right_brace)
    }

    fn code_block_from_statements(
        &self,
        statements: Option<Node<'a>>,
        left_brace: Node<'a>,
        right_brace: Node<'a>,
    ) -> Result<Value> {
        let statement_nodes: Vec<_> = statements
            .map(named_children)
            .into_iter()
            .flatten()
            .filter(|child| !is_trivia_node(*child))
            .collect();
        let mut items = Vec::new();
        for child in statement_nodes {
            items.push(self.code_block_item(child)?);
        }
        let statements_range = self.covering_range_or_point(&items, left_brace.end_byte());
        Ok(self.syntax_node(
            "CodeBlockSyntax",
            self.range_from_offsets(left_brace.start_byte(), right_brace.end_byte()),
            vec![
                self.with_name(self.token_for_node(left_brace, "leftBrace"), "leftBrace"),
                self.with_name(
                    self.syntax_node("CodeBlockItemListSyntax", statements_range, items),
                    "statements",
                ),
                self.with_name(self.token_for_node(right_brace, "rightBrace"), "rightBrace"),
            ],
        ))
    }

    fn expr(&self, node: Node<'a>) -> Result<Value> {
        match node.kind() {
            "directly_assignable_expression" => {
                let child = named_children(node)
                    .next()
                    .context("assignable expression is empty")?;
                self.expr(child)
            }
            "assignment" => self.assignment_expr(node),
            "if_statement" => self.if_expr(node),
            "additive_expression"
            | "comparison_expression"
            | "conjunction_expression"
            | "disjunction_expression"
            | "equality_expression"
            | "multiplicative_expression"
            | "range_expression" => self.binary_operator_expr(node),
            "array_literal" => self.array_expr(node),
            "call_expression" => self.function_call_expr(node),
            "dictionary_literal" => self.dictionary_expr(node),
            "lambda_literal" => self.closure_expr(node),
            "navigation_expression" => self.member_access_expr(node),
            "prefix_expression" => self.prefix_expr(node),
            "boolean_literal" => Ok(self.syntax_node(
                "BooleanLiteralExprSyntax",
                self.range_for_node(node),
                vec![self.with_name(
                    self.token_for_node(
                        node,
                        &format!("keyword(SwiftSyntax.Keyword.{})", self.text(node)),
                    ),
                    "literal",
                )],
            )),
            "integer_literal" => Ok(self.syntax_node(
                "IntegerLiteralExprSyntax",
                self.range_for_node(node),
                vec![self.with_name(
                    self.token_for_node(
                        node,
                        &format!("integerLiteral({})", quoted_text(self.text(node))),
                    ),
                    "literal",
                )],
            )),
            "real_literal" => Ok(self.syntax_node(
                "FloatLiteralExprSyntax",
                self.range_for_node(node),
                vec![self.with_name(
                    self.token_for_node(
                        node,
                        &format!("floatLiteral({})", quoted_text(self.text(node))),
                    ),
                    "literal",
                )],
            )),
            "simple_identifier" | "identifier" => Ok(self.syntax_node(
                "DeclReferenceExprSyntax",
                self.range_for_node(node),
                vec![self.with_name(
                    self.token_for_node(
                        node,
                        &format!("identifier({})", quoted_text(self.text(node))),
                    ),
                    "baseName",
                )],
            )),
            "self_expression" => Ok(self.syntax_node(
                "DeclReferenceExprSyntax",
                self.range_for_node(node),
                vec![self.with_name(
                    self.token_for_node(node, "keyword(SwiftSyntax.Keyword.self)"),
                    "baseName",
                )],
            )),
            "line_string_literal" => self.string_literal(node),
            "tuple_expression" => self.tuple_expr(node),
            other => bail!("unsupported Swift expression node '{other}'"),
        }
    }

    fn array_expr(&self, node: Node<'a>) -> Result<Value> {
        let left_square = self
            .immediate_child_kind(node, "[")
            .context("array literal is missing '['")?;
        let right_square = self
            .immediate_child_kind(node, "]")
            .context("array literal is missing ']'")?;

        let mut elements = Vec::new();
        for child in named_children(node).filter(|child| {
            child.start_byte() >= left_square.end_byte()
                && child.end_byte() <= right_square.start_byte()
        }) {
            let trailing_comma = self.trailing_delimiter(node, child, ",");
            let element_end = trailing_comma.map_or(child.end_byte(), |comma| comma.end_byte());
            let mut element_children = vec![self.with_name(self.expr(child)?, "expression")];
            if let Some(comma) = trailing_comma {
                element_children
                    .push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
            }
            elements.push(self.with_name(
                self.syntax_node(
                    "ArrayElementSyntax",
                    self.range_from_offsets(child.start_byte(), element_end),
                    element_children,
                ),
                "",
            ));
        }

        Ok(self.syntax_node(
            "ArrayExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.token_for_node(left_square, "leftSquare"), "leftSquare"),
                self.with_name(
                    self.syntax_node(
                        "ArrayElementListSyntax",
                        self.range_from_offsets(left_square.end_byte(), right_square.start_byte()),
                        elements,
                    ),
                    "elements",
                ),
                self.with_name(
                    self.token_for_node(right_square, "rightSquare"),
                    "rightSquare",
                ),
            ],
        ))
    }

    fn dictionary_expr(&self, node: Node<'a>) -> Result<Value> {
        let left_square = self
            .immediate_child_kind(node, "[")
            .context("dictionary literal is missing '['")?;
        let right_square = self
            .immediate_child_kind(node, "]")
            .context("dictionary literal is missing ']'")?;
        let keys = self.field_children(node, "key");
        let values = self.field_children(node, "value");

        let content = if keys.is_empty() && values.is_empty() {
            match self.immediate_child_kind(node, ":") {
                Some(colon) => self.token_for_node(colon, "colon"),
                None => self.syntax_node(
                    "DictionaryElementListSyntax",
                    self.range_from_offsets(left_square.end_byte(), right_square.start_byte()),
                    Vec::new(),
                ),
            }
        } else {
            let mut elements = Vec::new();
            for (key, value) in keys.into_iter().zip(values) {
                let colon = self
                    .children_between(node, key.end_byte(), value.start_byte())
                    .into_iter()
                    .find(|child| child.kind() == ":")
                    .context("dictionary element is missing ':'")?;
                let trailing_comma = self.trailing_delimiter(node, value, ",");
                let element_end = trailing_comma.map_or(value.end_byte(), |comma| comma.end_byte());
                let mut element_children = vec![
                    self.with_name(self.expr(key)?, "key"),
                    self.with_name(self.token_for_node(colon, "colon"), "colon"),
                    self.with_name(self.expr(value)?, "value"),
                ];
                if let Some(comma) = trailing_comma {
                    element_children
                        .push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
                }
                elements.push(self.with_name(
                    self.syntax_node(
                        "DictionaryElementSyntax",
                        self.range_from_offsets(key.start_byte(), element_end),
                        element_children,
                    ),
                    "",
                ));
            }
            self.syntax_node(
                "DictionaryElementListSyntax",
                self.range_from_offsets(left_square.end_byte(), right_square.start_byte()),
                elements,
            )
        };

        Ok(self.syntax_node(
            "DictionaryExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.token_for_node(left_square, "leftSquare"), "leftSquare"),
                self.with_name(content, "content"),
                self.with_name(
                    self.token_for_node(right_square, "rightSquare"),
                    "rightSquare",
                ),
            ],
        ))
    }

    fn tuple_expr(&self, node: Node<'a>) -> Result<Value> {
        let left_paren = self
            .immediate_child_kind(node, "(")
            .context("tuple expression is missing '('")?;
        let right_paren = self
            .immediate_child_kind(node, ")")
            .context("tuple expression is missing ')'")?;
        let values = self.field_children(node, "value");
        let mut elements = Vec::new();
        for value in values {
            if value.kind() == "bang" && value.start_byte() == value.end_byte() {
                continue;
            }
            let trailing_comma = self.trailing_delimiter(node, value, ",");
            elements.push(self.with_name(self.labeled_expr_for_value(value, trailing_comma)?, ""));
        }

        Ok(self.syntax_node(
            "TupleExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.token_for_node(left_paren, "leftParen"), "leftParen"),
                self.with_name(
                    self.syntax_node(
                        "LabeledExprListSyntax",
                        self.range_from_offsets(left_paren.end_byte(), right_paren.start_byte()),
                        elements,
                    ),
                    "elements",
                ),
                self.with_name(self.token_for_node(right_paren, "rightParen"), "rightParen"),
            ],
        ))
    }

    fn closure_expr(&self, node: Node<'a>) -> Result<Value> {
        if self.field_child(node, "captures").is_some() {
            bail!("closure captures are not supported yet");
        }

        let left_brace = self
            .immediate_child_kind(node, "{")
            .context("closure literal is missing '{'")?;
        let right_brace = self
            .immediate_child_kind(node, "}")
            .context("closure literal is missing '}'")?;
        let statements = named_children(node).find(|child| child.kind() == "statements");
        let mut children =
            vec![self.with_name(self.token_for_node(left_brace, "leftBrace"), "leftBrace")];

        if let Some(function_type) = self.field_child(node, "type") {
            children
                .push(self.with_name(self.closure_signature(function_type, node)?, "signature"));
        }

        let statement_items = statements
            .map(named_children)
            .into_iter()
            .flatten()
            .filter(|child| !is_trivia_node(*child))
            .map(|child| self.code_block_item(child))
            .collect::<Result<Vec<_>>>()?;
        let statements_range =
            self.covering_range_or_point(&statement_items, left_brace.end_byte());
        children.push(self.with_name(
            self.syntax_node("CodeBlockItemListSyntax", statements_range, statement_items),
            "statements",
        ));
        children.push(self.with_name(self.token_for_node(right_brace, "rightBrace"), "rightBrace"));

        Ok(self.syntax_node("ClosureExprSyntax", self.range_for_node(node), children))
    }

    fn closure_signature(&self, node: Node<'a>, closure: Node<'a>) -> Result<Value> {
        let in_keyword = self
            .immediate_child_kind(closure, "in")
            .or_else(|| self.nearest_child_before(closure, "in", closure.end_byte()))
            .context("closure signature is missing 'in'")?;
        let mut children = vec![self.with_name(
            self.empty_collection("AttributeListSyntax", node.start_byte()),
            "attributes",
        )];

        if let Some(parameter_node) =
            named_children(node).find(|child| child.kind() == "lambda_function_type_parameters")
        {
            children.push(self.with_name(
                self.closure_parameter_clause(parameter_node)?,
                "parameterClause",
            ));
        }

        if let Some(return_type) = self.closure_return_type(node) {
            let arrow = self
                .immediate_child_kind(node, "->")
                .context("closure return type is missing '->'")?;
            children.push(self.with_name(
                self.syntax_node(
                    "ReturnClauseSyntax",
                    self.range_from_offsets(arrow.start_byte(), return_type.end_byte()),
                    vec![
                        self.with_name(self.token_for_node(arrow, "arrow"), "arrow"),
                        self.with_name(self.identifier_type(return_type)?, "type"),
                    ],
                ),
                "returnClause",
            ));
        }

        children.push(self.with_name(
            self.token_for_node(in_keyword, "keyword(SwiftSyntax.Keyword.in)"),
            "inKeyword",
        ));

        Ok(self.syntax_node(
            "ClosureSignatureSyntax",
            self.range_from_offsets(node.start_byte(), in_keyword.end_byte()),
            children,
        ))
    }

    fn closure_parameter_clause(&self, node: Node<'a>) -> Result<Value> {
        let parameters = named_children(node)
            .filter(|child| child.kind() == "lambda_parameter")
            .collect::<Vec<_>>();
        let has_typed_parameters = parameters
            .iter()
            .any(|parameter| self.lambda_parameter_type(*parameter).is_some());
        let has_parenthesized_parameters = self.is_parenthesized_closure_parameters(node);

        if has_typed_parameters || has_parenthesized_parameters {
            let mut parameter_values = Vec::new();
            for parameter in parameters {
                let trailing_comma = self.trailing_delimiter(node, parameter, ",");
                parameter_values
                    .push(self.with_name(self.closure_parameter(parameter, trailing_comma)?, ""));
            }
            let params_range = self.covering_range_or_point(&parameter_values, node.start_byte());
            let parameter_list =
                self.syntax_node("ClosureParameterListSyntax", params_range, parameter_values);

            let mut children = Vec::new();
            if let Some(left_paren) = self.immediate_child_kind(node, "(") {
                children.push(
                    self.with_name(self.token_for_node(left_paren, "leftParen"), "leftParen"),
                );
            }
            children.push(self.with_name(parameter_list, "parameters"));
            if let Some(right_paren) = self.immediate_child_kind(node, ")") {
                children.push(
                    self.with_name(self.token_for_node(right_paren, "rightParen"), "rightParen"),
                );
            }

            Ok(self.syntax_node(
                "ClosureParameterClauseSyntax",
                self.range_for_node(node),
                children,
            ))
        } else {
            let mut parameter_values = Vec::new();
            for parameter in parameters {
                let trailing_comma = self.trailing_delimiter(node, parameter, ",");
                parameter_values.push(self.with_name(
                    self.closure_shorthand_parameter(parameter, trailing_comma)?,
                    "",
                ));
            }
            Ok(self.syntax_node(
                "ClosureShorthandParameterListSyntax",
                self.range_for_node(node),
                parameter_values,
            ))
        }
    }

    fn closure_parameter(&self, node: Node<'a>, trailing_comma: Option<Node<'a>>) -> Result<Value> {
        let name = self.lambda_parameter_name(node)?;
        let type_node = self.lambda_parameter_type(node);
        let mut children = vec![
            self.with_name(self.attribute_list(node)?, "attributes"),
            self.with_name(self.modifier_list(node), "modifiers"),
            self.with_name(
                self.token_for_node(
                    name,
                    &format!("identifier({})", quoted_text(self.text(name))),
                ),
                "firstName",
            ),
        ];
        if let Some(type_node) = type_node {
            if let Some(colon) = self.immediate_child_kind(node, ":") {
                children.push(self.with_name(self.token_for_node(colon, "colon"), "colon"));
            }
            children.push(self.with_name(self.identifier_type(type_node)?, "type"));
        }
        if let Some(comma) = trailing_comma {
            children.push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
        }
        Ok(self.syntax_node(
            "ClosureParameterSyntax",
            self.range_for_node(node),
            children,
        ))
    }

    fn is_parenthesized_closure_parameters(&self, node: Node<'a>) -> bool {
        self.immediate_child_kind(node, "(").is_some()
            || self.source[..node.start_byte()].trim_end().ends_with('(')
            || self.text(node).trim_start().starts_with('(')
    }

    fn closure_shorthand_parameter(
        &self,
        node: Node<'a>,
        trailing_comma: Option<Node<'a>>,
    ) -> Result<Value> {
        let name = self.lambda_parameter_name(node)?;
        let mut children = vec![self.with_name(
            self.token_for_node(
                name,
                &format!("identifier({})", quoted_text(self.text(name))),
            ),
            "name",
        )];
        if let Some(comma) = trailing_comma {
            children.push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
        }
        Ok(self.syntax_node(
            "ClosureShorthandParameterSyntax",
            self.range_for_node(node),
            children,
        ))
    }

    fn lambda_parameter_name(&self, node: Node<'a>) -> Result<Node<'a>> {
        if let Some(external_name) = self.field_child(node, "external_name") {
            return Ok(external_name);
        }
        let mut cursor = node.walk();
        if let Some(name) = node
            .children_by_field_name("name", &mut cursor)
            .find(|child| matches!(child.kind(), "simple_identifier" | "identifier" | "_"))
        {
            return Ok(name);
        }
        named_children(node)
            .find(|child| matches!(child.kind(), "simple_identifier" | "identifier" | "_"))
            .context("closure parameter is missing a name")
    }

    fn lambda_parameter_type(&self, node: Node<'a>) -> Option<Node<'a>> {
        let mut type_cursor = node.walk();
        if let Some(type_node) = node
            .children_by_field_name("type", &mut type_cursor)
            .find(|child| child.is_named())
        {
            return Some(type_node);
        }
        let mut name_cursor = node.walk();
        let name_type = node
            .children_by_field_name("name", &mut name_cursor)
            .filter(|child| child.is_named())
            .nth(1);
        name_type
    }

    fn closure_return_type(&self, node: Node<'a>) -> Option<Node<'a>> {
        let arrow = self.immediate_child_kind(node, "->")?;
        named_children(node)
            .filter(|child| {
                child.start_byte() > arrow.end_byte() && child.end_byte() <= node.end_byte()
            })
            .find(|child| child.kind() != "lambda_function_type_parameters")
    }

    fn member_access_expr(&self, node: Node<'a>) -> Result<Value> {
        let suffix_node = self
            .field_child(node, "suffix")
            .context("member access expression is missing suffix")?;
        let suffix = self
            .field_child(suffix_node, "suffix")
            .or_else(|| named_children(suffix_node).next())
            .context("member access suffix is missing a name")?;
        let period = self
            .immediate_child_kind(suffix_node, ".")
            .context("member access expression is missing '.'")?;

        let mut children = Vec::new();
        if let Some(base) = self.field_child(node, "target") {
            children.push(self.with_name(self.expr(base)?, "base"));
        }
        children.push(self.with_name(self.token_for_node(period, "period"), "period"));
        children.push(self.with_name(self.decl_reference_expr(suffix), "declName"));

        Ok(self.syntax_node(
            "MemberAccessExprSyntax",
            self.range_for_node(node),
            children,
        ))
    }

    fn prefix_expr(&self, node: Node<'a>) -> Result<Value> {
        let operation = self
            .field_child(node, "operation")
            .or_else(|| children(node).find(|child| !child.is_named()))
            .context("prefix expression is missing operator")?;
        let target = self
            .field_child(node, "target")
            .context("prefix expression is missing target")?;

        if operation.kind() == "." || self.text(operation) == "." {
            let decl_name = match target.kind() {
                "identifier" | "integer_literal" | "simple_identifier" | "self_expression" => {
                    target
                }
                other => bail!("unsupported implicit member target '{other}'"),
            };
            return Ok(self.syntax_node(
                "MemberAccessExprSyntax",
                self.range_for_node(node),
                vec![
                    self.with_name(self.token_for_node(operation, "period"), "period"),
                    self.with_name(self.decl_reference_expr(decl_name), "declName"),
                ],
            ));
        }

        Ok(self.syntax_node(
            "PrefixOperatorExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.token_for_node(
                        operation,
                        &format!("prefixOperator({})", quoted_text(self.text(operation))),
                    ),
                    "operator",
                ),
                self.with_name(self.expr(target)?, "expression"),
            ],
        ))
    }

    fn binary_operator_expr(&self, node: Node<'a>) -> Result<Value> {
        let lhs = self
            .field_child(node, "lhs")
            .or_else(|| self.field_child(node, "start"))
            .context("binary expression is missing lhs")?;
        let op = self
            .field_child(node, "op")
            .context("binary expression is missing operator")?;
        let rhs = self
            .field_child(node, "rhs")
            .or_else(|| self.field_child(node, "end"))
            .context("binary expression is missing rhs")?;
        let operator = self.syntax_node(
            "BinaryOperatorExprSyntax",
            self.range_for_node(op),
            vec![self.with_name(
                self.token_for_node(
                    op,
                    &format!("binaryOperator({})", quoted_text(self.text(op))),
                ),
                "operator",
            )],
        );
        Ok(self.syntax_node(
            "InfixOperatorExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.expr(lhs)?, "leftOperand"),
                self.with_name(operator, "operator"),
                self.with_name(self.expr(rhs)?, "rightOperand"),
            ],
        ))
    }

    fn if_expr(&self, node: Node<'a>) -> Result<Value> {
        let if_keyword = self
            .immediate_child_kind(node, "if")
            .context("if expression is missing 'if'")?;
        let body_statements = named_children(node)
            .find(|child| child.kind() == "statements")
            .context("if expression is missing body statements")?;
        let condition = self
            .field_child(node, "condition")
            .filter(|child| child.is_named())
            .or_else(|| self.first_named_condition(node, if_keyword, body_statements))
            .context("if expression is missing condition")?;
        let left_brace = self
            .nearest_child_before(node, "{", body_statements.start_byte())
            .context("if expression body is missing '{'")?;
        let right_brace = self
            .nearest_child_after(node, "}", body_statements.end_byte())
            .context("if expression body is missing '}'")?;

        let mut children = vec![
            self.with_name(
                self.token_for_node(if_keyword, "keyword(SwiftSyntax.Keyword.if)"),
                "ifKeyword",
            ),
            self.with_name(self.condition_element_list(condition)?, "conditions"),
            self.with_name(
                self.code_block_from_statements(Some(body_statements), left_brace, right_brace)?,
                "body",
            ),
        ];

        if let Some(else_keyword) = named_children(node)
            .find(|child| child.kind() == "else" && child.start_byte() > right_brace.end_byte())
        {
            children.push(self.with_name(
                self.token_for_node(else_keyword, "keyword(SwiftSyntax.Keyword.else)"),
                "elseKeyword",
            ));
            if let Some(nested_if) = named_children(node).find(|child| {
                child.kind() == "if_statement" && child.start_byte() > else_keyword.end_byte()
            }) {
                children.push(self.with_name(self.if_expr(nested_if)?, "elseBody"));
            } else if let Some(else_statements) = named_children(node)
                .filter(|child| child.kind() == "statements")
                .find(|child| child.start_byte() > else_keyword.end_byte())
            {
                let else_left_brace = self
                    .nearest_child_before(node, "{", else_statements.start_byte())
                    .context("if else body is missing '{'")?;
                let else_right_brace = self
                    .nearest_child_after(node, "}", else_statements.end_byte())
                    .context("if else body is missing '}'")?;
                children.push(self.with_name(
                    self.code_block_from_statements(
                        Some(else_statements),
                        else_left_brace,
                        else_right_brace,
                    )?,
                    "elseBody",
                ));
            }
        }

        Ok(self.syntax_node("IfExprSyntax", self.range_for_node(node), children))
    }

    fn while_stmt(&self, node: Node<'a>) -> Result<Value> {
        let while_keyword = self
            .immediate_child_kind(node, "while")
            .context("while statement is missing 'while'")?;
        let body_statements = named_children(node)
            .find(|child| child.kind() == "statements")
            .context("while statement is missing body statements")?;
        let condition = self
            .field_child(node, "condition")
            .filter(|child| child.is_named())
            .or_else(|| self.first_named_condition(node, while_keyword, body_statements))
            .context("while statement is missing condition")?;
        let left_brace = self
            .nearest_child_before(node, "{", body_statements.start_byte())
            .context("while statement body is missing '{'")?;
        let right_brace = self
            .nearest_child_after(node, "}", body_statements.end_byte())
            .context("while statement body is missing '}'")?;

        Ok(self.syntax_node(
            "WhileStmtSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.token_for_node(while_keyword, "keyword(SwiftSyntax.Keyword.while)"),
                    "whileKeyword",
                ),
                self.with_name(self.condition_element_list(condition)?, "conditions"),
                self.with_name(
                    self.code_block_from_statements(
                        Some(body_statements),
                        left_brace,
                        right_brace,
                    )?,
                    "body",
                ),
            ],
        ))
    }

    fn for_stmt(&self, node: Node<'a>) -> Result<Value> {
        let for_keyword = self
            .immediate_child_kind(node, "for")
            .context("for statement is missing 'for'")?;
        let pattern = self
            .field_child(node, "item")
            .context("for statement is missing item pattern")?;
        let in_keyword = self
            .immediate_child_kind(node, "in")
            .context("for statement is missing 'in'")?;
        let sequence = self
            .field_child(node, "collection")
            .context("for statement is missing collection expression")?;
        let body_statements = named_children(node)
            .find(|child| child.kind() == "statements")
            .context("for statement is missing body statements")?;
        let left_brace = self
            .nearest_child_before(node, "{", body_statements.start_byte())
            .context("for statement body is missing '{'")?;
        let right_brace = self
            .nearest_child_after(node, "}", body_statements.end_byte())
            .context("for statement body is missing '}'")?;

        Ok(self.syntax_node(
            "ForStmtSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.token_for_node(for_keyword, "keyword(SwiftSyntax.Keyword.for)"),
                    "forKeyword",
                ),
                self.with_name(self.pattern(pattern)?, "pattern"),
                self.with_name(
                    self.token_for_node(in_keyword, "keyword(SwiftSyntax.Keyword.in)"),
                    "inKeyword",
                ),
                self.with_name(self.expr(sequence)?, "sequence"),
                self.with_name(
                    self.code_block_from_statements(
                        Some(body_statements),
                        left_brace,
                        right_brace,
                    )?,
                    "body",
                ),
            ],
        ))
    }

    fn condition_element_list(&self, condition: Node<'a>) -> Result<Value> {
        let element = self.syntax_node(
            "ConditionElementSyntax",
            self.range_for_node(condition),
            vec![self.with_name(self.expr(condition)?, "condition")],
        );
        Ok(self.syntax_node(
            "ConditionElementListSyntax",
            self.range_for_node(condition),
            vec![self.with_name(element, "")],
        ))
    }

    fn decl_reference_expr(&self, node: Node<'a>) -> Value {
        let token_kind = match node.kind() {
            "integer_literal" => format!("integerLiteral({})", quoted_text(self.text(node))),
            "self_expression" => "keyword(SwiftSyntax.Keyword.self)".to_string(),
            _ => format!("identifier({})", quoted_text(self.text(node))),
        };
        self.syntax_node(
            "DeclReferenceExprSyntax",
            self.range_for_node(node),
            vec![self.with_name(self.token_for_node(node, &token_kind), "baseName")],
        )
    }

    fn function_call_expr(&self, node: Node<'a>) -> Result<Value> {
        let callee = named_children(node)
            .find(|child| child.kind() != "call_suffix")
            .context("call expression is missing callee")?;
        let suffix = self
            .immediate_named_child_kind(node, "call_suffix")
            .context("call expression is missing call suffix")?;
        let trailing_closures = named_children(suffix)
            .filter(|child| child.kind() == "lambda_literal")
            .collect::<Vec<_>>();
        if trailing_closures.len() > 1 {
            bail!("multiple trailing closures are not supported yet");
        }

        if let Some(value_arguments) = self.immediate_named_child_kind(suffix, "value_arguments") {
            if self.subscript_delimiters(value_arguments).is_some() {
                return self.subscript_call_expr(
                    node,
                    callee,
                    value_arguments,
                    trailing_closures.first().copied(),
                    true,
                );
            }
        } else if let Some(trailing_closure) = trailing_closures.first().copied() {
            if let Some((inner_callee, inner_value_arguments)) = self.subscript_call_parts(callee) {
                return self.subscript_call_expr(
                    node,
                    inner_callee,
                    inner_value_arguments,
                    Some(trailing_closure),
                    false,
                );
            }
        }

        let mut children = vec![self.with_name(self.expr(callee)?, "calledExpression")];

        if let Some(value_arguments) = self.immediate_named_child_kind(suffix, "value_arguments") {
            let left_paren = self
                .immediate_child_kind(value_arguments, "(")
                .context("call arguments are missing '('")?;
            let right_paren = self
                .immediate_child_kind(value_arguments, ")")
                .context("call arguments are missing ')'")?;
            children
                .push(self.with_name(self.token_for_node(left_paren, "leftParen"), "leftParen"));
            children.push(self.with_name(
                self.labeled_expr_list(value_arguments, left_paren, right_paren)?,
                "arguments",
            ));
            children
                .push(self.with_name(self.token_for_node(right_paren, "rightParen"), "rightParen"));
        } else {
            children.push(self.with_name(
                self.empty_collection("LabeledExprListSyntax", suffix.start_byte()),
                "arguments",
            ));
        }

        if let Some(trailing_closure) = trailing_closures.first() {
            children.push(self.with_name(self.closure_expr(*trailing_closure)?, "trailingClosure"));
        }
        children.push(self.with_name(
            self.empty_collection("MultipleTrailingClosureElementListSyntax", node.end_byte()),
            "additionalTrailingClosures",
        ));

        Ok(self.syntax_node(
            "FunctionCallExprSyntax",
            self.range_for_node(node),
            children,
        ))
    }

    fn subscript_call_expr(
        &self,
        node: Node<'a>,
        callee: Node<'a>,
        value_arguments: Node<'a>,
        trailing_closure: Option<Node<'a>>,
        include_arguments: bool,
    ) -> Result<Value> {
        let (left_square, right_square) = self
            .subscript_delimiters(value_arguments)
            .context("subscript call is missing square brackets")?;
        let arguments = if include_arguments {
            self.labeled_expr_list(value_arguments, left_square, right_square)?
        } else {
            self.empty_collection("LabeledExprListSyntax", left_square.end_byte())
        };

        let mut children = vec![
            self.with_name(self.expr(callee)?, "calledExpression"),
            self.with_name(self.token_for_node(left_square, "leftSquare"), "leftSquare"),
            self.with_name(arguments, "arguments"),
            self.with_name(
                self.token_for_node(right_square, "rightSquare"),
                "rightSquare",
            ),
        ];
        if let Some(closure) = trailing_closure {
            children.push(self.with_name(self.closure_expr(closure)?, "trailingClosure"));
        }
        children.push(self.with_name(
            self.empty_collection("MultipleTrailingClosureElementListSyntax", node.end_byte()),
            "additionalTrailingClosures",
        ));

        Ok(self.syntax_node(
            "SubscriptCallExprSyntax",
            self.range_for_node(node),
            children,
        ))
    }

    fn subscript_call_parts(&self, node: Node<'a>) -> Option<(Node<'a>, Node<'a>)> {
        if node.kind() != "call_expression" {
            return None;
        }
        let callee = named_children(node).find(|child| child.kind() != "call_suffix")?;
        let suffix = self.immediate_named_child_kind(node, "call_suffix")?;
        let value_arguments = self.immediate_named_child_kind(suffix, "value_arguments")?;
        self.subscript_delimiters(value_arguments)?;
        Some((callee, value_arguments))
    }

    fn subscript_delimiters(&self, value_arguments: Node<'a>) -> Option<(Node<'a>, Node<'a>)> {
        let left_square = self.immediate_child_kind(value_arguments, "[")?;
        let right_square = self.immediate_child_kind(value_arguments, "]")?;
        Some((left_square, right_square))
    }

    fn labeled_expr_list(
        &self,
        value_arguments: Node<'a>,
        left_paren: Node<'a>,
        right_paren: Node<'a>,
    ) -> Result<Value> {
        let mut args = Vec::new();
        for arg in named_children(value_arguments).filter(|child| child.kind() == "value_argument")
        {
            let trailing_comma = self.trailing_delimiter(value_arguments, arg, ",");
            args.push(self.with_name(self.labeled_expr(arg, trailing_comma)?, ""));
        }
        Ok(self.syntax_node(
            "LabeledExprListSyntax",
            self.range_from_offsets(left_paren.end_byte(), right_paren.start_byte()),
            args,
        ))
    }

    fn labeled_expr(&self, node: Node<'a>, trailing_comma: Option<Node<'a>>) -> Result<Value> {
        let value = self
            .field_child(node, "value")
            .or_else(|| named_children(node).find(|child| child.kind() != "value_argument_label"))
            .context("call argument is missing value")?;
        let mut children = Vec::new();
        if let Some(label_node) = self.field_child(node, "name") {
            let label = self
                .first_descendant_kind(label_node, "simple_identifier")
                .unwrap_or(label_node);
            children.push(self.with_name(
                self.token_for_node(
                    label,
                    &format!("identifier({})", quoted_text(self.text(label))),
                ),
                "label",
            ));
            if let Some(colon) = self.immediate_child_kind(node, ":") {
                children.push(self.with_name(self.token_for_node(colon, "colon"), "colon"));
            }
        }
        children.push(self.with_name(self.expr(value)?, "expression"));
        if let Some(comma) = trailing_comma {
            children.push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
        }
        Ok(self.syntax_node("LabeledExprSyntax", self.range_for_node(node), children))
    }

    fn labeled_expr_for_value(
        &self,
        value: Node<'a>,
        trailing_comma: Option<Node<'a>>,
    ) -> Result<Value> {
        let mut children = vec![self.with_name(self.expr(value)?, "expression")];
        if let Some(comma) = trailing_comma {
            children.push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
        }
        let end = trailing_comma.map_or(value.end_byte(), |comma| comma.end_byte());
        Ok(self.syntax_node(
            "LabeledExprSyntax",
            self.range_from_offsets(value.start_byte(), end),
            children,
        ))
    }

    fn assignment_expr(&self, node: Node<'a>) -> Result<Value> {
        let lhs = self
            .field_child(node, "target")
            .context("assignment is missing lhs")?;
        let equal = self
            .field_child(node, "operator")
            .or_else(|| self.immediate_child_kind(node, "="))
            .context("assignment is missing '='")?;
        let rhs = self
            .field_child(node, "result")
            .context("assignment is missing rhs")?;
        let assignment_operator = self.syntax_node(
            "AssignmentExprSyntax",
            self.range_for_node(equal),
            vec![self.with_name(self.token_for_node(equal, "equal"), "equal")],
        );
        Ok(self.syntax_node(
            "InfixOperatorExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.expr(lhs)?, "leftOperand"),
                self.with_name(assignment_operator, "operator"),
                self.with_name(self.expr(rhs)?, "rightOperand"),
            ],
        ))
    }

    fn return_stmt(&self, node: Node<'a>) -> Result<Value> {
        let return_keyword = self
            .first_descendant_any_kind(node, "return")
            .context("return statement is missing return keyword")?;
        let mut children = vec![self.with_name(
            self.token_for_node(return_keyword, "keyword(SwiftSyntax.Keyword.return)"),
            "returnKeyword",
        )];
        if let Some(expression) = self
            .field_child(node, "result")
            .or_else(|| named_children(node).find(|child| child.kind() != "throw_keyword"))
        {
            children.push(self.with_name(self.expr(expression)?, "expression"));
        }
        Ok(self.syntax_node("ReturnStmtSyntax", self.range_for_node(node), children))
    }

    fn control_transfer_stmt(&self, node: Node<'a>) -> Result<Value> {
        if self.first_descendant_any_kind(node, "return").is_some() {
            return self.return_stmt(node);
        }
        if let Some(break_keyword) = self.first_descendant_any_kind(node, "break") {
            return Ok(self.jump_stmt(
                "BreakStmtSyntax",
                break_keyword,
                "breakKeyword",
                "keyword(SwiftSyntax.Keyword.break)",
            ));
        }
        if let Some(continue_keyword) = self.first_descendant_any_kind(node, "continue") {
            return Ok(self.jump_stmt(
                "ContinueStmtSyntax",
                continue_keyword,
                "continueKeyword",
                "keyword(SwiftSyntax.Keyword.continue)",
            ));
        }
        bail!("unsupported Swift control transfer statement");
    }

    fn jump_stmt(
        &self,
        node_type: &str,
        keyword: Node<'a>,
        keyword_name: &str,
        token_kind: &str,
    ) -> Value {
        self.syntax_node(
            node_type,
            self.range_for_node(keyword),
            vec![self.with_name(self.token_for_node(keyword, token_kind), keyword_name)],
        )
    }

    fn string_literal(&self, node: Node<'a>) -> Result<Value> {
        let text_node = self.field_child(node, "text");
        let open_quote = self.token_with_range(
            "stringQuote",
            self.range_from_offsets(node.start_byte(), node.start_byte() + 1),
        );
        let close_quote = self.token_with_range(
            "stringQuote",
            self.range_from_offsets(node.end_byte() - 1, node.end_byte()),
        );
        let mut segments = Vec::new();
        if let Some(text) = text_node {
            segments.push(self.with_name(
                self.syntax_node(
                    "StringSegmentSyntax",
                    self.range_for_node(text),
                    vec![self.with_name(
                        self.token_for_node(
                            text,
                            &format!("stringSegment({})", quoted_text(self.text(text))),
                        ),
                        "content",
                    )],
                ),
                "",
            ));
        }
        Ok(self.syntax_node(
            "StringLiteralExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(open_quote, "openingQuote"),
                self.with_name(
                    self.syntax_node(
                        "StringLiteralSegmentListSyntax",
                        self.range_for_node(node),
                        segments,
                    ),
                    "segments",
                ),
                self.with_name(close_quote, "closingQuote"),
            ],
        ))
    }

    fn text(&self, node: Node<'a>) -> &'a str {
        &self.source[node.start_byte()..node.end_byte()]
    }

    fn syntax_node(&self, node_type: &str, range: Value, children: Vec<Value>) -> Value {
        json!({
            "children": children,
            "tokenKind": "",
            "nodeType": node_type,
            "range": range,
            "index": -1
        })
    }

    fn empty_collection(&self, node_type: &str, offset: usize) -> Value {
        self.syntax_node(node_type, self.point_range(offset), Vec::new())
    }

    fn token_for_node(&self, node: Node<'a>, token_kind: &str) -> Value {
        self.token_with_range(token_kind, self.range_for_node(node))
    }

    fn token_with_range(&self, token_kind: &str, range: Value) -> Value {
        json!({
            "children": [],
            "tokenKind": token_kind,
            "nodeType": "",
            "range": range,
            "index": -1
        })
    }

    fn with_name(&self, mut value: Value, name: &str) -> Value {
        let obj = value
            .as_object_mut()
            .expect("SwiftSyntax JSON nodes are always objects");
        obj.insert("name".into(), Value::String(name.into()));
        obj.entry("index").or_insert(json!(-1));
        value
    }

    fn range_for_node(&self, node: Node<'a>) -> Value {
        json!({
            "startColumn": node.start_position().column + 1,
            "endLine": node.end_position().row + 1,
            "startLine": node.start_position().row + 1,
            "startOffset": node.start_byte(),
            "endOffset": node.end_byte(),
            "endColumn": node.end_position().column + 1
        })
    }

    fn point_range(&self, offset: usize) -> Value {
        self.range_from_offsets(offset, offset)
    }

    fn range_from_offsets(&self, start: usize, end: usize) -> Value {
        let (start_line, start_column) = self.line_column(start);
        let (end_line, end_column) = self.line_column(end);
        json!({
            "startColumn": start_column,
            "endLine": end_line,
            "startLine": start_line,
            "startOffset": start,
            "endOffset": end,
            "endColumn": end_column
        })
    }

    fn line_column(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.source.len());
        let line_index = match self.line_starts.binary_search(&offset) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };
        let line_start = self.line_starts[line_index];
        (line_index + 1, offset - line_start + 1)
    }

    fn covering_range_or_point(&self, values: &[Value], fallback_offset: usize) -> Value {
        let mut start = None::<usize>;
        let mut end = None::<usize>;
        for value in values {
            if let Some(range) = value.get("range").and_then(Value::as_object) {
                if let (Some(s), Some(e)) = (
                    range.get("startOffset").and_then(Value::as_u64),
                    range.get("endOffset").and_then(Value::as_u64),
                ) {
                    start = Some(start.map_or(s as usize, |current| current.min(s as usize)));
                    end = Some(end.map_or(e as usize, |current| current.max(e as usize)));
                }
            }
        }
        match (start, end) {
            (Some(start), Some(end)) => self.range_from_offsets(start, end),
            _ => self.point_range(fallback_offset),
        }
    }

    fn field_child(&self, node: Node<'a>, field: &str) -> Option<Node<'a>> {
        node.child_by_field_name(field)
    }

    fn field_children(&self, node: Node<'a>, field: &str) -> Vec<Node<'a>> {
        let mut cursor = node.walk();
        node.children_by_field_name(field, &mut cursor).collect()
    }

    fn children_between(&self, node: Node<'a>, start: usize, end: usize) -> Vec<Node<'a>> {
        children(node)
            .filter(|child| child.start_byte() >= start && child.end_byte() <= end)
            .collect()
    }

    fn immediate_child_kind(&self, node: Node<'a>, kind: &str) -> Option<Node<'a>> {
        children(node).find(|child| child.kind() == kind)
    }

    fn immediate_named_child_kind(&self, node: Node<'a>, kind: &str) -> Option<Node<'a>> {
        named_children(node).find(|child| child.kind() == kind)
    }

    fn nearest_child_before(&self, node: Node<'a>, kind: &str, offset: usize) -> Option<Node<'a>> {
        children(node)
            .filter(|child| child.kind() == kind && child.start_byte() <= offset)
            .last()
    }

    fn nearest_child_after(&self, node: Node<'a>, kind: &str, offset: usize) -> Option<Node<'a>> {
        children(node).find(|child| child.kind() == kind && child.start_byte() >= offset)
    }

    fn first_named_condition(
        &self,
        node: Node<'a>,
        keyword: Node<'a>,
        body_statements: Node<'a>,
    ) -> Option<Node<'a>> {
        named_children(node).find(|child| {
            child.start_byte() > keyword.end_byte()
                && child.end_byte() <= body_statements.start_byte()
                && child.kind() != "else"
                && child.kind() != "statements"
        })
    }

    fn trailing_delimiter(
        &self,
        parent: Node<'a>,
        node: Node<'a>,
        delimiter: &str,
    ) -> Option<Node<'a>> {
        let next_named =
            named_children(parent).find(|candidate| candidate.start_byte() > node.start_byte());
        children(parent).find(|child| {
            child.kind() == delimiter
                && child.start_byte() >= node.end_byte()
                && match next_named {
                    Some(next) => child.end_byte() <= next.start_byte(),
                    None => true,
                }
        })
    }

    fn first_named_child_excluding(&self, node: Node<'a>, excluded: &[&str]) -> Option<Node<'a>> {
        named_children(node).find(|child| !excluded.contains(&child.kind()))
    }

    fn first_descendant_kind(&self, node: Node<'a>, kind: &str) -> Option<Node<'a>> {
        if node.kind() == kind {
            return Some(node);
        }
        for child in named_children(node) {
            if let Some(found) = self.first_descendant_kind(child, kind) {
                return Some(found);
            }
        }
        None
    }

    fn first_descendant_any_kind(&self, node: Node<'a>, kind: &str) -> Option<Node<'a>> {
        if node.kind() == kind {
            return Some(node);
        }
        for child in children(node) {
            if let Some(found) = self.first_descendant_any_kind(child, kind) {
                return Some(found);
            }
        }
        None
    }
}

fn children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect::<Vec<_>>().into_iter()
}

fn named_children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .collect::<Vec<_>>()
        .into_iter()
}

fn is_trivia_node(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "comment" | "line_comment" | "multiline_comment" | "shebang_line"
    )
}

fn quoted_text(text: &str) -> String {
    serde_json::to_string(text).expect("serializing a string cannot fail")
}

fn end_offset(value: &Value) -> usize {
    value["range"]["endOffset"].as_u64().unwrap_or_default() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_empty_source_file() {
        let value = parse_source("Empty.swift", "/tmp/Empty.swift", "").unwrap();
        assert_eq!(value["nodeType"], "SourceFileSyntax");
        assert_eq!(value["loc"], 1);
        assert_eq!(value["children"][0]["nodeType"], "CodeBlockItemListSyntax");
        assert_eq!(value["children"][1]["tokenKind"], "endOfFile");
    }

    #[test]
    fn emits_import_declarations() {
        let source = "import Foundation\n@_exported import class Foundation.Thread\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let statements = value["children"][0]["children"].as_array().unwrap();
        assert_eq!(statements.len(), 2);

        let import = &statements[0]["children"][0];
        assert_eq!(import["nodeType"], "ImportDeclSyntax");
        assert_eq!(import["children"][0]["nodeType"], "AttributeListSyntax");
        assert_eq!(import["children"][1]["nodeType"], "DeclModifierListSyntax");
        assert_eq!(
            import["children"][2]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.import)"
        );
        let path = import["children"][3]["children"].as_array().unwrap();
        assert_eq!(path.len(), 1);
        assert_eq!(
            path[0]["children"][0]["tokenKind"],
            "identifier(\"Foundation\")"
        );

        let dotted_import = &statements[1]["children"][0];
        assert_eq!(dotted_import["nodeType"], "ImportDeclSyntax");
        assert_eq!(
            dotted_import["children"][2]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.import)"
        );
        assert_eq!(
            dotted_import["children"][3]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.class)"
        );
        let attributes = dotted_import["children"][0]["children"].as_array().unwrap();
        assert_eq!(attributes.len(), 1);
        assert_eq!(
            attributes[0]["children"][1]["children"][0]["tokenKind"],
            "identifier(\"_exported\")"
        );
        let path = dotted_import["children"][4]["children"].as_array().unwrap();
        assert_eq!(path.len(), 2);
        assert_eq!(
            path[0]["children"][0]["tokenKind"],
            "identifier(\"Foundation\")"
        );
        assert_eq!(path[0]["children"][1]["tokenKind"], "period");
        assert_eq!(
            path[1]["children"][0]["tokenKind"],
            "identifier(\"Thread\")"
        );
    }

    #[test]
    fn emits_basic_variable_declarations() {
        let source = "let x = 1\nvar y: String = \"2\"\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let statements = &value["children"][0]["children"];
        assert_eq!(statements.as_array().unwrap().len(), 2);
        assert_eq!(
            statements[0]["children"][0]["nodeType"],
            "VariableDeclSyntax"
        );
        assert_eq!(
            statements[0]["children"][0]["children"][2]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.let)"
        );
        assert_eq!(
            statements[1]["children"][0]["children"][3]["children"][0]["children"][1]["nodeType"],
            "TypeAnnotationSyntax"
        );
    }

    #[test]
    fn emits_tuple_variable_declaration_pattern() {
        let source = "var (a, b): Int = foo()\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let tuple = find_first_node_type(&value, "TuplePatternSyntax").unwrap();
        assert_eq!(tuple["children"][0]["tokenKind"], "leftParen");
        assert_eq!(
            tuple["children"][1]["nodeType"],
            "TuplePatternElementListSyntax"
        );
        let elements = tuple["children"][1]["children"].as_array().unwrap();
        assert_eq!(elements.len(), 2);
        assert_eq!(
            elements[0]["children"][0]["nodeType"],
            "IdentifierPatternSyntax"
        );
        assert_eq!(elements[0]["children"][1]["tokenKind"], "comma");
        assert_eq!(
            elements[1]["children"][0]["nodeType"],
            "IdentifierPatternSyntax"
        );
        assert_eq!(tuple["children"][2]["tokenKind"], "rightParen");
        let binding = find_first_node_type(&value, "PatternBindingSyntax").unwrap();
        assert_eq!(binding["children"][1]["nodeType"], "TypeAnnotationSyntax");
        assert_eq!(
            binding["children"][2]["nodeType"],
            "InitializerClauseSyntax"
        );
    }

    #[test]
    fn emits_function_with_body() {
        let source = "func foo() {\n  let z = x\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let function = &value["children"][0]["children"][0]["children"][0];
        assert_eq!(function["nodeType"], "FunctionDeclSyntax");
        assert_eq!(function["children"][3]["tokenKind"], "identifier(\"foo\")");
        assert_eq!(function["children"][5]["nodeType"], "CodeBlockSyntax");
    }

    #[test]
    fn emits_function_call_arguments() {
        let source = "foo(1, bar: \"x\")\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let call = &value["children"][0]["children"][0]["children"][0];
        assert_eq!(call["nodeType"], "FunctionCallExprSyntax");
        assert_eq!(call["children"][0]["name"], "calledExpression");
        assert_eq!(
            call["children"][0]["children"][0]["tokenKind"],
            "identifier(\"foo\")"
        );
        let args = &call["children"][2]["children"];
        assert_eq!(args.as_array().unwrap().len(), 2);
        assert_eq!(args[0]["nodeType"], "LabeledExprSyntax");
        assert_eq!(
            args[0]["children"][0]["nodeType"],
            "IntegerLiteralExprSyntax"
        );
        assert_eq!(args[0]["children"][1]["tokenKind"], "comma");
        assert_eq!(args[1]["children"][0]["tokenKind"], "identifier(\"bar\")");
        assert_eq!(args[1]["children"][1]["tokenKind"], "colon");
        assert_eq!(
            args[1]["children"][2]["nodeType"],
            "StringLiteralExprSyntax"
        );
    }

    #[test]
    fn emits_assignment_as_infix_operator() {
        let source = "a = foo()\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let assignment = &value["children"][0]["children"][0]["children"][0];
        assert_eq!(assignment["nodeType"], "InfixOperatorExprSyntax");
        assert_eq!(assignment["children"][0]["name"], "leftOperand");
        assert_eq!(
            assignment["children"][1]["nodeType"],
            "AssignmentExprSyntax"
        );
        assert_eq!(
            assignment["children"][1]["children"][0]["tokenKind"],
            "equal"
        );
        assert_eq!(
            assignment["children"][2]["nodeType"],
            "FunctionCallExprSyntax"
        );
    }

    #[test]
    fn emits_array_literal_expression() {
        let source = "let numbers = [1, foo(2), bar]\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let array_expr = find_first_node_type(&value, "ArrayExprSyntax").unwrap();
        assert_eq!(array_expr["children"][0]["tokenKind"], "leftSquare");
        assert_eq!(
            array_expr["children"][1]["nodeType"],
            "ArrayElementListSyntax"
        );
        assert_eq!(array_expr["children"][2]["tokenKind"], "rightSquare");

        let elements = array_expr["children"][1]["children"].as_array().unwrap();
        assert_eq!(elements.len(), 3);
        assert_eq!(elements[0]["children"][0]["name"], "expression");
        assert_eq!(
            elements[0]["children"][0]["nodeType"],
            "IntegerLiteralExprSyntax"
        );
        assert_eq!(elements[0]["children"][1]["tokenKind"], "comma");
        assert_eq!(
            elements[1]["children"][0]["nodeType"],
            "FunctionCallExprSyntax"
        );
        assert_eq!(elements[1]["children"][1]["tokenKind"], "comma");
        assert_eq!(
            elements[2]["children"][0]["nodeType"],
            "DeclReferenceExprSyntax"
        );
        assert_eq!(elements[2]["children"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn emits_dictionary_literal_expression() {
        let source = "let x = [\"a\": 1, \"b\": 2]\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let dictionary = find_first_node_type(&value, "DictionaryExprSyntax").unwrap();
        assert_eq!(dictionary["children"][0]["tokenKind"], "leftSquare");
        assert_eq!(
            dictionary["children"][1]["nodeType"],
            "DictionaryElementListSyntax"
        );
        assert_eq!(dictionary["children"][2]["tokenKind"], "rightSquare");

        let elements = dictionary["children"][1]["children"].as_array().unwrap();
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0]["children"][0]["name"], "key");
        assert_eq!(
            elements[0]["children"][0]["nodeType"],
            "StringLiteralExprSyntax"
        );
        assert_eq!(elements[0]["children"][1]["tokenKind"], "colon");
        assert_eq!(elements[0]["children"][2]["name"], "value");
        assert_eq!(
            elements[0]["children"][2]["nodeType"],
            "IntegerLiteralExprSyntax"
        );
        assert_eq!(elements[0]["children"][3]["tokenKind"], "comma");
    }

    #[test]
    fn emits_dictionary_literal_with_empty_tuple_values() {
        let source = "[1: (), 2: ()]\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let dictionary = find_first_node_type(&value, "DictionaryExprSyntax").unwrap();
        let elements = dictionary["children"][1]["children"].as_array().unwrap();
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0]["children"][2]["nodeType"], "TupleExprSyntax");
        assert_eq!(
            elements[0]["children"][2]["children"][1]["nodeType"],
            "LabeledExprListSyntax"
        );
        assert_eq!(
            elements[0]["children"][2]["children"][1]["children"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(elements[1]["children"][2]["nodeType"], "TupleExprSyntax");
    }

    #[test]
    fn emits_tuple_expression_with_float_literal() {
        let source = "var product = (\"MacBook\", 1099.99)\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let tuple = find_first_node_type(&value, "TupleExprSyntax").unwrap();
        assert_eq!(tuple["children"][0]["tokenKind"], "leftParen");
        assert_eq!(tuple["children"][1]["nodeType"], "LabeledExprListSyntax");
        assert_eq!(tuple["children"][2]["tokenKind"], "rightParen");

        let elements = tuple["children"][1]["children"].as_array().unwrap();
        assert_eq!(elements.len(), 2);
        assert_eq!(
            elements[0]["children"][0]["nodeType"],
            "StringLiteralExprSyntax"
        );
        assert_eq!(elements[0]["children"][1]["tokenKind"], "comma");
        assert_eq!(
            elements[1]["children"][0]["nodeType"],
            "FloatLiteralExprSyntax"
        );
        assert_eq!(
            elements[1]["children"][0]["children"][0]["tokenKind"],
            "floatLiteral(\"1099.99\")"
        );
    }

    #[test]
    fn emits_trailing_closure_function_call() {
        let source = "func f() {\n  numbers.forEach { num in\n    print(num)\n  }\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let call = find_node_types(&value, "FunctionCallExprSyntax")
            .into_iter()
            .find(|node| {
                node["children"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|child| child["name"] == "trailingClosure")
            })
            .unwrap();
        assert_eq!(call["children"][0]["nodeType"], "MemberAccessExprSyntax");
        assert_eq!(call["children"][1]["nodeType"], "LabeledExprListSyntax");
        assert_eq!(call["children"][1]["children"].as_array().unwrap().len(), 0);
        assert_eq!(call["children"][2]["nodeType"], "ClosureExprSyntax");

        let closure = &call["children"][2];
        assert_eq!(closure["children"][0]["tokenKind"], "leftBrace");
        assert_eq!(closure["children"][1]["nodeType"], "ClosureSignatureSyntax");
        assert_eq!(
            closure["children"][1]["children"][1]["nodeType"],
            "ClosureShorthandParameterListSyntax"
        );
        assert_eq!(
            closure["children"][1]["children"][1]["children"][0]["children"][0]["tokenKind"],
            "identifier(\"num\")"
        );
        assert_eq!(
            closure["children"][2]["children"][0]["children"][0]["nodeType"],
            "FunctionCallExprSyntax"
        );
        assert_eq!(closure["children"][3]["tokenKind"], "rightBrace");
    }

    #[test]
    fn emits_subscript_call_expression() {
        let source = "let first = items[0]\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let subscript = find_first_node_type(&value, "SubscriptCallExprSyntax").unwrap();
        assert_eq!(
            subscript["children"][0]["nodeType"],
            "DeclReferenceExprSyntax"
        );
        assert_eq!(subscript["children"][1]["tokenKind"], "leftSquare");
        assert_eq!(
            subscript["children"][2]["nodeType"],
            "LabeledExprListSyntax"
        );
        assert_eq!(
            subscript["children"][2]["children"][0]["children"][0]["nodeType"],
            "IntegerLiteralExprSyntax"
        );
        assert_eq!(subscript["children"][3]["tokenKind"], "rightSquare");
        assert_eq!(
            subscript["children"][4]["nodeType"],
            "MultipleTrailingClosureElementListSyntax"
        );
    }

    #[test]
    fn emits_trailing_closure_subscript_call_expression() {
        let source = "var button = View.Button[5, 4, 3] {\n  Text(\"ABC\")\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let subscript = find_first_node_type(&value, "SubscriptCallExprSyntax").unwrap();
        assert_eq!(
            subscript["children"][0]["nodeType"],
            "MemberAccessExprSyntax"
        );
        assert_eq!(subscript["children"][1]["tokenKind"], "leftSquare");
        assert_eq!(
            subscript["children"][2]["nodeType"],
            "LabeledExprListSyntax"
        );
        assert_eq!(
            subscript["children"][2]["children"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(subscript["children"][3]["tokenKind"], "rightSquare");
        assert_eq!(subscript["children"][4]["nodeType"], "ClosureExprSyntax");
        assert_eq!(
            subscript["children"][4]["children"][1]["children"][0]["children"][0]["nodeType"],
            "FunctionCallExprSyntax"
        );
        assert_eq!(
            subscript["children"][5]["nodeType"],
            "MultipleTrailingClosureElementListSyntax"
        );
    }

    #[test]
    fn emits_typed_closure_literal() {
        let source =
            "func f() {\n  let compare = { (s1: String, s2: String) -> Bool in\n    return s1 > s2\n  }\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let closure = find_first_node_type(&value, "ClosureExprSyntax").unwrap();
        let signature = &closure["children"][1];
        assert_eq!(signature["nodeType"], "ClosureSignatureSyntax");
        assert_eq!(
            signature["children"][1]["nodeType"],
            "ClosureParameterClauseSyntax"
        );
        let parameter_list = signature["children"][1]["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|child| child["name"] == "parameters")
            .unwrap();
        let parameters = &parameter_list["children"];
        assert_eq!(parameters.as_array().unwrap().len(), 2);
        assert_eq!(
            parameters[0]["children"][2]["tokenKind"],
            "identifier(\"s1\")"
        );
        assert_eq!(parameters[0]["children"][3]["tokenKind"], "colon");
        assert_eq!(
            parameters[0]["children"][4]["nodeType"],
            "IdentifierTypeSyntax"
        );
        assert_eq!(parameters[0]["children"][5]["tokenKind"], "comma");
        assert_eq!(signature["children"][2]["nodeType"], "ReturnClauseSyntax");
        assert_eq!(
            signature["children"][3]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.in)"
        );
        assert_eq!(
            closure["children"][2]["children"][0]["children"][0]["nodeType"],
            "ReturnStmtSyntax"
        );
    }

    #[test]
    fn emits_parenthesized_untyped_closure_parameter_clause() {
        let source = "func f() {\n  compactMap { (parserDiag) in }\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let closure = find_first_node_type(&value, "ClosureExprSyntax").unwrap();
        let signature = &closure["children"][1];
        assert_eq!(
            signature["children"][1]["nodeType"],
            "ClosureParameterClauseSyntax"
        );
        let parameter_list = signature["children"][1]["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|child| child["name"] == "parameters")
            .unwrap();
        let parameters = &parameter_list["children"];
        assert_eq!(parameters.as_array().unwrap().len(), 1);
        assert_eq!(
            parameters[0]["children"][2]["tokenKind"],
            "identifier(\"parserDiag\")"
        );
        assert!(parameters[0]["children"]
            .as_array()
            .unwrap()
            .iter()
            .all(|child| child["name"] != "type"));
    }

    #[test]
    fn skips_comments_inside_closure_body() {
        let source = "func f() {\n  let closure = { value in\n    // skip me\n    print(value) // and me\n  }\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let closure = find_first_node_type(&value, "ClosureExprSyntax").unwrap();
        let statements = &closure["children"][2]["children"];
        assert_eq!(statements.as_array().unwrap().len(), 1);
        assert_eq!(
            statements[0]["children"][0]["nodeType"],
            "FunctionCallExprSyntax"
        );
    }

    #[test]
    fn emits_binary_operator_expressions() {
        let source = "a = b + 1\nif a > 0 {\n  foo()\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let binary_ops = find_node_types(&value, "BinaryOperatorExprSyntax");
        let token_kinds = binary_ops
            .iter()
            .map(|node| node["children"][0]["tokenKind"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(token_kinds.contains(&"binaryOperator(\"+\")"));
        assert!(token_kinds.contains(&"binaryOperator(\">\")"));
    }

    #[test]
    fn emits_return_statement() {
        let source = "func f() -> Int {\n  return foo()\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let function = &value["children"][0]["children"][0]["children"][0];
        let body = &function["children"][5];
        let return_stmt = &body["children"][1]["children"][0]["children"][0];
        assert_eq!(return_stmt["nodeType"], "ReturnStmtSyntax");
        assert_eq!(
            return_stmt["children"][0]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.return)"
        );
        assert_eq!(
            return_stmt["children"][1]["nodeType"],
            "FunctionCallExprSyntax"
        );
    }

    #[test]
    fn emits_if_else_expression() {
        let source =
            "func f(flag: Bool) {\n  if flag {\n    foo()\n  } else {\n    bar()\n  }\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let if_expr = find_first_node_type(&value, "IfExprSyntax").unwrap();
        assert_eq!(
            if_expr["children"][0]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.if)"
        );
        assert_eq!(
            if_expr["children"][1]["nodeType"],
            "ConditionElementListSyntax"
        );
        assert_eq!(
            if_expr["children"][1]["children"][0]["children"][0]["nodeType"],
            "DeclReferenceExprSyntax"
        );
        assert_eq!(if_expr["children"][2]["nodeType"], "CodeBlockSyntax");
        assert_eq!(
            if_expr["children"][3]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.else)"
        );
        assert_eq!(if_expr["children"][4]["nodeType"], "CodeBlockSyntax");
    }

    #[test]
    fn emits_while_statement() {
        let source = "func f(i: Int) {\n  while i > 0 {\n    foo()\n  }\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let while_stmt = find_first_node_type(&value, "WhileStmtSyntax").unwrap();
        assert_eq!(
            while_stmt["children"][0]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.while)"
        );
        assert_eq!(
            while_stmt["children"][1]["nodeType"],
            "ConditionElementListSyntax"
        );
        assert_eq!(
            while_stmt["children"][1]["children"][0]["children"][0]["nodeType"],
            "InfixOperatorExprSyntax"
        );
        assert_eq!(while_stmt["children"][2]["nodeType"], "CodeBlockSyntax");
    }

    #[test]
    fn emits_simple_for_statement() {
        let source = "func f(items: Int) {\n  for item in items {\n    foo(item)\n  }\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let for_stmt = find_first_node_type(&value, "ForStmtSyntax").unwrap();
        assert_eq!(
            for_stmt["children"][0]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.for)"
        );
        assert_eq!(
            for_stmt["children"][1]["nodeType"],
            "IdentifierPatternSyntax"
        );
        assert_eq!(
            for_stmt["children"][2]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.in)"
        );
        assert_eq!(
            for_stmt["children"][3]["nodeType"],
            "DeclReferenceExprSyntax"
        );
        assert_eq!(for_stmt["children"][4]["nodeType"], "CodeBlockSyntax");
    }

    #[test]
    fn emits_break_and_continue_statements() {
        let source = "func f() {\n  while true {\n    continue\n    break\n  }\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let continue_stmt = find_first_node_type(&value, "ContinueStmtSyntax").unwrap();
        assert_eq!(
            continue_stmt["children"][0]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.continue)"
        );
        let break_stmt = find_first_node_type(&value, "BreakStmtSyntax").unwrap();
        assert_eq!(
            break_stmt["children"][0]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.break)"
        );
    }

    #[test]
    fn emits_simple_class_members() {
        let source = "class Foo {\n  var x = 1\n  func bar() {}\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let class_decl = find_first_node_type(&value, "ClassDeclSyntax").unwrap();
        assert_eq!(class_decl["children"][2]["name"], "classKeyword");
        assert_eq!(
            class_decl["children"][2]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.class)"
        );
        assert_eq!(
            class_decl["children"][3]["tokenKind"],
            "identifier(\"Foo\")"
        );

        let member_block = class_decl["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|child| child["name"] == "memberBlock")
            .unwrap();
        assert_eq!(member_block["nodeType"], "MemberBlockSyntax");
        let members = &member_block["children"][1]["children"];
        assert_eq!(members.as_array().unwrap().len(), 2);
        assert_eq!(members[0]["children"][0]["nodeType"], "VariableDeclSyntax");
        assert_eq!(members[1]["children"][0]["nodeType"], "FunctionDeclSyntax");
    }

    #[test]
    fn emits_declaration_attributes() {
        let source =
            "@bar(x: \"y\")\nfunc foo() -> {\n  let x = 1\n}\n@objc(Foo)\npublic class Foo {}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let attributes = find_node_types(&value, "AttributeSyntax");
        assert_eq!(attributes.len(), 2);
        assert_eq!(attributes[0]["children"][0]["tokenKind"], "atSign");
        assert_eq!(
            attributes[0]["children"][1]["children"][0]["tokenKind"],
            "identifier(\"bar\")"
        );
        assert_eq!(attributes[0]["children"][2]["tokenKind"], "leftParen");
        assert_eq!(attributes[0]["children"][3]["tokenKind"], "rightParen");
        assert_eq!(
            attributes[1]["children"][1]["children"][0]["tokenKind"],
            "identifier(\"objc\")"
        );

        let function = find_first_node_type(&value, "FunctionDeclSyntax").unwrap();
        assert_eq!(function["children"][0]["nodeType"], "AttributeListSyntax");
        assert_eq!(
            function["children"][0]["children"][0]["nodeType"],
            "AttributeSyntax"
        );
        let class_decl = find_first_node_type(&value, "ClassDeclSyntax").unwrap();
        assert_eq!(class_decl["children"][0]["nodeType"], "AttributeListSyntax");
        assert_eq!(
            class_decl["children"][0]["children"][0]["nodeType"],
            "AttributeSyntax"
        );
    }

    #[test]
    fn emits_declaration_modifiers() {
        let source = "private static func foo() -> {}\npublic class Foo {}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();

        let function = find_first_node_type(&value, "FunctionDeclSyntax").unwrap();
        let function_modifiers = &function["children"][1];
        assert_eq!(function_modifiers["nodeType"], "DeclModifierListSyntax");
        let modifiers = function_modifiers["children"].as_array().unwrap();
        assert_eq!(modifiers.len(), 2);
        assert_eq!(
            modifiers[0]["children"][0]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.private)"
        );
        assert_eq!(
            modifiers[1]["children"][0]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.static)"
        );

        let class_decl = find_first_node_type(&value, "ClassDeclSyntax").unwrap();
        let class_modifiers = &class_decl["children"][1];
        assert_eq!(class_modifiers["nodeType"], "DeclModifierListSyntax");
        assert_eq!(
            class_modifiers["children"][0]["children"][0]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.public)"
        );
    }

    #[test]
    fn emits_member_access_expressions() {
        let source = "class Foo {\n  var x = 1\n  func baz() {}\n  func bar() {\n    x = self.x\n    self.baz()\n  }\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let member_access = find_first_node_type(&value, "MemberAccessExprSyntax").unwrap();
        assert_eq!(member_access["children"][0]["name"], "base");
        assert_eq!(
            member_access["children"][0]["nodeType"],
            "DeclReferenceExprSyntax"
        );
        assert_eq!(member_access["children"][1]["tokenKind"], "period");
        assert_eq!(member_access["children"][2]["name"], "declName");
        assert_eq!(
            member_access["children"][2]["children"][0]["tokenKind"],
            "identifier(\"x\")"
        );
    }

    #[test]
    fn emits_implicit_member_function_calls() {
        let source = "let deps = [.package(name: \"DepA\", path: \"PathA\")]\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let call = find_first_node_type(&value, "FunctionCallExprSyntax").unwrap();
        let callee = &call["children"][0];
        assert_eq!(callee["nodeType"], "MemberAccessExprSyntax");
        assert_eq!(callee["children"][0]["name"], "period");
        assert_eq!(callee["children"][0]["tokenKind"], "period");
        assert_eq!(callee["children"][1]["name"], "declName");
        assert_eq!(
            callee["children"][1]["children"][0]["tokenKind"],
            "identifier(\"package\")"
        );
        assert_eq!(callee["children"].as_array().unwrap().len(), 2);

        let arguments = &call["children"][2]["children"];
        assert_eq!(arguments.as_array().unwrap().len(), 2);
        assert_eq!(
            arguments[0]["children"][0]["tokenKind"],
            "identifier(\"name\")"
        );
        assert_eq!(
            arguments[1]["children"][0]["tokenKind"],
            "identifier(\"path\")"
        );
    }

    #[test]
    fn emits_prefix_operator_expressions() {
        let source = "let value = !enabled\nlet other = -count\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let prefixes = find_node_types(&value, "PrefixOperatorExprSyntax");
        let token_kinds = prefixes
            .iter()
            .map(|node| node["children"][0]["tokenKind"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(token_kinds.contains(&"prefixOperator(\"!\")"));
        assert!(token_kinds.contains(&"prefixOperator(\"-\")"));
        assert!(prefixes
            .iter()
            .all(|node| node["children"][1]["nodeType"] == "DeclReferenceExprSyntax"));
    }

    #[test]
    fn emits_range_expressions() {
        let source =
            "let deps = [.package(url: \"https://github.com/DepC\", \"1.2.3\"..<\"1.2.6\"), .package(url: \"https://github.com/DepD\", \"1.2.3\"...\"1.2.6\")]\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let binary_ops = find_node_types(&value, "BinaryOperatorExprSyntax");
        let token_kinds = binary_ops
            .iter()
            .map(|node| node["children"][0]["tokenKind"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(token_kinds.contains(&"binaryOperator(\"..<\")"));
        assert!(token_kinds.contains(&"binaryOperator(\"...\")"));

        let infix_ops = find_node_types(&value, "InfixOperatorExprSyntax");
        assert!(infix_ops.iter().any(|node| {
            node["children"][0]["nodeType"] == "StringLiteralExprSyntax"
                && node["children"][1]["children"][0]["tokenKind"] == "binaryOperator(\"..<\")"
                && node["children"][2]["nodeType"] == "StringLiteralExprSyntax"
        }));
    }

    #[test]
    fn skips_unmatched_top_level_right_brace() {
        let source = "let x = 1\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let statements = value["children"][0]["children"].as_array().unwrap();
        assert_eq!(statements.len(), 1);
        assert_eq!(
            statements[0]["children"][0]["nodeType"],
            "VariableDeclSyntax"
        );
    }

    fn find_first_node_type<'v>(value: &'v Value, node_type: &str) -> Option<&'v Value> {
        if value.get("nodeType").and_then(Value::as_str) == Some(node_type) {
            return Some(value);
        }
        value
            .get("children")
            .and_then(Value::as_array)
            .and_then(|children| {
                children
                    .iter()
                    .find_map(|child| find_first_node_type(child, node_type))
            })
    }

    fn find_node_types<'v>(value: &'v Value, node_type: &str) -> Vec<&'v Value> {
        let mut values = Vec::new();
        if value.get("nodeType").and_then(Value::as_str) == Some(node_type) {
            values.push(value);
        }
        if let Some(children) = value.get("children").and_then(Value::as_array) {
            for child in children {
                values.extend(find_node_types(child, node_type));
            }
        }
        values
    }
}
