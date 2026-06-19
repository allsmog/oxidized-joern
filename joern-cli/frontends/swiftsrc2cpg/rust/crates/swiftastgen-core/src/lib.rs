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
        let root_children = named_children(root).collect::<Vec<_>>();
        let mut child_index = 0;
        while child_index < root_children.len() {
            let child = root_children[child_index];
            if is_trivia_node(child) || self.is_ignorable_top_level_error(child) {
                child_index += 1;
                continue;
            }
            if self.is_recoverable_precedence_group_error(child) {
                let (decl, next_index) =
                    self.recovered_precedence_group_decl(&root_children, child_index)?;
                statement_items.push(self.code_block_item_for_value(decl, "item"));
                child_index = next_index;
                continue;
            }
            if self.is_operator_designated_types_recovery_error(child) {
                child_index += 1;
                continue;
            }
            if self.is_recoverable_protocol_error(child) {
                let (decl, next_index) =
                    self.recovered_protocol_decl(&root_children, child_index)?;
                statement_items.push(self.code_block_item_for_value(decl, "item"));
                child_index = next_index;
                continue;
            }
            statement_items.push(self.code_block_item(child)?);
            child_index += 1;
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

    fn is_ignorable_member_error(&self, node: Node<'a>) -> bool {
        if node.kind() != "ERROR" {
            return false;
        }
        let trimmed = self.text(node).trim_start();
        if trimmed == "deinit" && named_children(node).all(is_trivia_node) {
            return true;
        }
        !(trimmed.starts_with("case")
            || trimmed.starts_with("subscript")
            || self.is_recoverable_property_error(node))
    }

    fn code_block_item(&self, node: Node<'a>) -> Result<Value> {
        let item = self.with_name(self.syntax_for_statement(node)?, "item");
        Ok(self.syntax_node("CodeBlockItemSyntax", self.range_for_node(node), vec![item]))
    }

    fn code_block_item_for_value(&self, value: Value, child_name: &str) -> Value {
        let range = value["range"].clone();
        self.syntax_node(
            "CodeBlockItemSyntax",
            range,
            vec![self.with_name(value, child_name)],
        )
    }

    fn syntax_for_statement(&self, node: Node<'a>) -> Result<Value> {
        match node.kind() {
            "property_declaration" => self.variable_decl(node),
            "protocol_property_declaration" => self.variable_decl(node),
            "function_declaration" => self.function_decl(node),
            "protocol_function_declaration" => self.function_decl(node),
            "class_declaration" => self.nominal_type_decl(node),
            "protocol_declaration" => self.protocol_decl(node),
            "operator_declaration" => self.operator_decl(node),
            "precedence_group_declaration" => self.precedence_group_decl(node),
            "control_transfer_statement" => self.control_transfer_stmt(node),
            "for_statement" => self.for_stmt(node),
            "if_statement" => self.if_expr(node),
            "import_declaration" => self.import_decl(node),
            "switch_statement" => self.switch_expr(node),
            "while_statement" => self.while_stmt(node),
            "ERROR" if self.is_recoverable_precedence_group_error(node) => {
                self.precedence_group_decl(node)
            }
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
            | "nil"
            | "self_expression"
            | "super_expression"
            | "tuple_expression"
            | "navigation_expression"
            | "line_string_literal" => self.expr(node),
            other => bail!("unsupported Swift syntax node '{other}'"),
        }
    }

    fn syntax_for_member_decl(&self, node: Node<'a>) -> Result<Value> {
        match node.kind() {
            "associatedtype_declaration" => self.associated_type_decl(node),
            "property_declaration" => self.variable_decl(node),
            "protocol_property_declaration" => self.variable_decl(node),
            "function_declaration" => self.function_decl(node),
            "protocol_function_declaration" => self.function_decl(node),
            "class_declaration" => self.nominal_type_decl(node),
            "protocol_declaration" => self.protocol_decl(node),
            "operator_declaration" => self.operator_decl(node),
            "precedence_group_declaration" => self.precedence_group_decl(node),
            "enum_entry" => self.enum_case_decl(node),
            "deinit_declaration" => self.deinitializer_decl(node),
            "init_declaration" => self.initializer_decl(node),
            "subscript_declaration" => self.subscript_decl(node),
            "ERROR" if self.is_recoverable_property_error(node) => {
                self.recovered_variable_decl(node, None)
            }
            "ERROR" if self.text(node).trim_start().starts_with("case") => {
                self.enum_case_decl(node)
            }
            "ERROR" if self.text(node).trim_start().starts_with("subscript") => {
                self.subscript_decl(node)
            }
            "ERROR" if self.is_recoverable_precedence_group_error(node) => {
                self.precedence_group_decl(node)
            }
            other => bail!("unsupported Swift member declaration node '{other}'"),
        }
    }

    fn nominal_type_decl(&self, node: Node<'a>) -> Result<Value> {
        let declaration_kind = self
            .field_child(node, "declaration_kind")
            .context("nominal type declaration is missing declaration kind")?;
        if declaration_kind.kind() == "extension" {
            return self.extension_decl(node, declaration_kind);
        }
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
            "enum" => (
                "EnumDeclSyntax",
                "enumKeyword",
                "keyword(SwiftSyntax.Keyword.enum)",
            ),
            "actor" => (
                "ActorDeclSyntax",
                "actorKeyword",
                "keyword(SwiftSyntax.Keyword.actor)",
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

        let mut children = vec![
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
        ];
        if let Some(inheritance_clause) = self.inheritance_clause(node)? {
            children.push(self.with_name(inheritance_clause, "inheritanceClause"));
        }
        children.push(self.with_name(self.member_block(body)?, "memberBlock"));

        Ok(self.syntax_node(node_type, self.range_for_node(node), children))
    }

    fn protocol_decl(&self, node: Node<'a>) -> Result<Value> {
        let protocol_keyword = self
            .field_child(node, "declaration_kind")
            .or_else(|| self.immediate_child_kind(node, "protocol"))
            .context("protocol declaration is missing 'protocol'")?;
        let name_node = self
            .field_child(node, "name")
            .context("protocol declaration is missing a name")?;
        let name = match name_node.kind() {
            "type_identifier" | "simple_identifier" => name_node,
            _ => self
                .first_descendant_kind(name_node, "type_identifier")
                .or_else(|| self.first_descendant_kind(name_node, "simple_identifier"))
                .context("protocol declaration name is missing an identifier")?,
        };
        let body = self
            .field_child(node, "body")
            .context("protocol declaration is missing a body")?;

        let mut children = vec![
            self.with_name(self.attribute_list(node)?, "attributes"),
            self.with_name(self.modifier_list(node), "modifiers"),
            self.with_name(
                self.token_for_node(protocol_keyword, "keyword(SwiftSyntax.Keyword.protocol)"),
                "protocolKeyword",
            ),
            self.with_name(
                self.token_for_node(
                    name,
                    &format!("identifier({})", quoted_text(self.text(name))),
                ),
                "name",
            ),
        ];
        if let Some(inheritance_clause) = self.inheritance_clause(node)? {
            children.push(self.with_name(inheritance_clause, "inheritanceClause"));
        }
        children.push(self.with_name(self.member_block(body)?, "memberBlock"));

        Ok(self.syntax_node("ProtocolDeclSyntax", self.range_for_node(node), children))
    }

    fn operator_decl(&self, node: Node<'a>) -> Result<Value> {
        let fixity = ["prefix", "postfix", "infix"]
            .iter()
            .find_map(|kind| self.immediate_child_kind(node, kind))
            .context("operator declaration is missing a fixity specifier")?;
        let operator_keyword = self
            .immediate_child_kind(node, "operator")
            .context("operator declaration is missing 'operator'")?;
        let name = named_children(node)
            .find(|child| {
                child.kind() == "custom_operator"
                    && child.start_byte() >= operator_keyword.end_byte()
            })
            .context("operator declaration is missing an operator name")?;

        let mut children = vec![
            self.with_name(
                self.token_for_node(
                    fixity,
                    &format!("keyword(SwiftSyntax.Keyword.{})", fixity.kind()),
                ),
                "fixitySpecifier",
            ),
            self.with_name(
                self.token_for_node(operator_keyword, "keyword(SwiftSyntax.Keyword.operator)"),
                "operatorKeyword",
            ),
            self.with_name(
                self.token_for_node(
                    name,
                    &format!(
                        "{}({})",
                        operator_token_kind(fixity.kind()),
                        quoted_text(self.text(name))
                    ),
                ),
                "name",
            ),
        ];

        if let Some(precedence_and_types) =
            self.operator_precedence_and_types(node, name.end_byte())?
        {
            children.push(self.with_name(precedence_and_types, "operatorPrecedenceAndTypes"));
        }

        Ok(self.syntax_node("OperatorDeclSyntax", self.range_for_node(node), children))
    }

    fn operator_precedence_and_types(
        &self,
        node: Node<'a>,
        name_end: usize,
    ) -> Result<Option<Value>> {
        let Some(colon) =
            children(node).find(|child| child.kind() == ":" && child.start_byte() >= name_end)
        else {
            return Ok(None);
        };
        let Some(precedence_group) = named_children(node).find(|child| {
            child.start_byte() >= colon.end_byte()
                && matches!(child.kind(), "simple_identifier" | "type_identifier")
        }) else {
            return Ok(None);
        };

        Ok(Some(self.syntax_node(
            "OperatorPrecedenceAndTypesSyntax",
            self.range_from_offsets(colon.start_byte(), precedence_group.end_byte()),
            vec![
                self.with_name(self.token_for_node(colon, "colon"), "colon"),
                self.with_name(
                    self.token_for_node(
                        precedence_group,
                        &format!("identifier({})", quoted_text(self.text(precedence_group))),
                    ),
                    "precedenceGroup",
                ),
                self.with_name(
                    self.empty_collection("DesignatedTypeListSyntax", precedence_group.end_byte()),
                    "designatedTypes",
                ),
            ],
        )))
    }

    fn precedence_group_decl(&self, node: Node<'a>) -> Result<Value> {
        let precedencegroup_keyword = self
            .immediate_child_kind(node, "precedencegroup")
            .or_else(|| self.first_descendant_any_kind(node, "precedencegroup"))
            .context("precedencegroup declaration is missing 'precedencegroup'")?;
        let left_brace = self
            .immediate_child_kind(node, "{")
            .context("precedencegroup declaration is missing '{'")?;
        let right_brace = self
            .immediate_child_kind(node, "}")
            .context("precedencegroup declaration is missing '}'")?;
        let name = self
            .first_descendant_kind_between(
                node,
                "simple_identifier",
                precedencegroup_keyword.end_byte(),
                left_brace.start_byte(),
            )
            .or_else(|| {
                self.first_descendant_kind_between(
                    node,
                    "type_identifier",
                    precedencegroup_keyword.end_byte(),
                    left_brace.start_byte(),
                )
            })
            .context("precedencegroup declaration is missing a name")?;

        self.precedence_group_decl_from_parts(
            node,
            precedencegroup_keyword,
            name,
            left_brace,
            right_brace,
            (node.start_byte(), node.end_byte()),
        )
    }

    fn recovered_precedence_group_decl(
        &self,
        siblings: &[Node<'a>],
        start_index: usize,
    ) -> Result<(Value, usize)> {
        let header = siblings[start_index];
        let precedencegroup_keyword = self
            .immediate_child_kind(header, "precedencegroup")
            .or_else(|| self.first_descendant_any_kind(header, "precedencegroup"))
            .context("recovered precedencegroup declaration is missing 'precedencegroup'")?;
        let left_brace = self
            .immediate_child_kind(header, "{")
            .context("recovered precedencegroup declaration is missing '{'")?;
        let name = self
            .first_descendant_kind_between(
                header,
                "simple_identifier",
                precedencegroup_keyword.end_byte(),
                left_brace.start_byte(),
            )
            .or_else(|| {
                self.first_descendant_kind_between(
                    header,
                    "type_identifier",
                    precedencegroup_keyword.end_byte(),
                    left_brace.start_byte(),
                )
            })
            .context("recovered precedencegroup declaration is missing a name")?;
        let (right_brace, close_index) =
            self.immediate_child_kind(header, "}")
                .map(|right_brace| (right_brace, start_index))
                .or_else(|| {
                    siblings.iter().enumerate().skip(start_index + 1).find_map(
                        |(index, sibling)| {
                            self.immediate_child_kind(*sibling, "}")
                                .map(|right_brace| (right_brace, index))
                        },
                    )
                })
                .context("recovered precedencegroup declaration is missing '}'")?;

        Ok((
            self.precedence_group_decl_from_parts(
                header,
                precedencegroup_keyword,
                name,
                left_brace,
                right_brace,
                (header.start_byte(), right_brace.end_byte()),
            )?,
            close_index + 1,
        ))
    }

    fn precedence_group_decl_from_parts(
        &self,
        node: Node<'a>,
        precedencegroup_keyword: Node<'a>,
        name: Node<'a>,
        left_brace: Node<'a>,
        right_brace: Node<'a>,
        range: (usize, usize),
    ) -> Result<Value> {
        Ok(self.syntax_node(
            "PrecedenceGroupDeclSyntax",
            self.range_from_offsets(range.0, range.1),
            vec![
                self.with_name(
                    self.attribute_list_before(node, precedencegroup_keyword.start_byte())?,
                    "attributes",
                ),
                self.with_name(
                    self.modifier_list_before(node, precedencegroup_keyword.start_byte()),
                    "modifiers",
                ),
                self.with_name(
                    self.token_for_node(
                        precedencegroup_keyword,
                        "keyword(SwiftSyntax.Keyword.precedencegroup)",
                    ),
                    "precedencegroupKeyword",
                ),
                self.with_name(
                    self.token_for_node(
                        name,
                        &format!("identifier({})", quoted_text(self.text(name))),
                    ),
                    "name",
                ),
                self.with_name(self.token_for_node(left_brace, "leftBrace"), "leftBrace"),
                self.with_name(
                    self.empty_collection(
                        "PrecedenceGroupAttributeListSyntax",
                        left_brace.end_byte(),
                    ),
                    "groupAttributes",
                ),
                self.with_name(self.token_for_node(right_brace, "rightBrace"), "rightBrace"),
            ],
        ))
    }

    fn is_recoverable_precedence_group_error(&self, node: Node<'a>) -> bool {
        node.kind() == "ERROR"
            && self
                .first_descendant_any_kind(node, "precedencegroup")
                .is_some()
    }

    fn is_operator_designated_types_recovery_error(&self, node: Node<'a>) -> bool {
        node.kind() == "ERROR" && self.text(node).trim_start().starts_with(',')
    }

    fn is_recoverable_protocol_error(&self, node: Node<'a>) -> bool {
        node.kind() == "ERROR"
            && self.immediate_child_kind(node, "protocol").is_some()
            && self.immediate_child_kind(node, "{").is_some()
    }

    fn recovered_protocol_decl(
        &self,
        siblings: &[Node<'a>],
        start_index: usize,
    ) -> Result<(Value, usize)> {
        let header = siblings[start_index];
        let protocol_keyword = self
            .immediate_child_kind(header, "protocol")
            .context("recovered protocol declaration is missing 'protocol'")?;
        let left_brace = self
            .immediate_child_kind(header, "{")
            .context("recovered protocol declaration is missing '{'")?;
        let name = self
            .first_descendant_kind_between(
                header,
                "simple_identifier",
                protocol_keyword.end_byte(),
                left_brace.start_byte(),
            )
            .or_else(|| {
                self.first_descendant_kind_between(
                    header,
                    "type_identifier",
                    protocol_keyword.end_byte(),
                    left_brace.start_byte(),
                )
            })
            .context("recovered protocol declaration is missing a name")?;
        let close_index = siblings
            .iter()
            .enumerate()
            .skip(start_index + 1)
            .find(|(_, sibling)| sibling.kind() == "ERROR" && self.text(**sibling).trim() == "}")
            .map(|(index, _)| index)
            .context("recovered protocol declaration is missing '}'")?;
        let right_brace = siblings[close_index];

        let mut members = Vec::new();
        let mut member_index = start_index + 1;
        if self.is_recoverable_property_error(header) {
            let initializer_continuation = siblings
                .get(member_index)
                .copied()
                .filter(|candidate| self.is_split_initializer_continuation(*candidate));
            members.push(self.member_block_item_for_value(
                self.recovered_variable_decl(header, initializer_continuation)?,
            ));
            if initializer_continuation.is_some() {
                member_index += 1;
            }
        }

        while member_index < close_index {
            let member = siblings[member_index];
            if !(is_trivia_node(member) || self.is_ignorable_member_error(member)) {
                members.push(self.member_block_item(member)?);
            }
            member_index += 1;
        }

        let members_range = self.covering_range_or_point(&members, left_brace.end_byte());
        let member_block = self.syntax_node(
            "MemberBlockSyntax",
            self.range_from_offsets(left_brace.start_byte(), right_brace.end_byte()),
            vec![
                self.with_name(self.token_for_node(left_brace, "leftBrace"), "leftBrace"),
                self.with_name(
                    self.syntax_node("MemberBlockItemListSyntax", members_range, members),
                    "members",
                ),
                self.with_name(self.token_for_node(right_brace, "rightBrace"), "rightBrace"),
            ],
        );

        let mut children = vec![
            self.with_name(
                self.attribute_list_before(header, protocol_keyword.start_byte())?,
                "attributes",
            ),
            self.with_name(
                self.modifier_list_before(header, protocol_keyword.start_byte()),
                "modifiers",
            ),
            self.with_name(
                self.token_for_node(protocol_keyword, "keyword(SwiftSyntax.Keyword.protocol)"),
                "protocolKeyword",
            ),
            self.with_name(
                self.token_for_node(
                    name,
                    &format!("identifier({})", quoted_text(self.text(name))),
                ),
                "name",
            ),
        ];
        if let Some(inheritance_clause) = self.inheritance_clause(header)? {
            children.push(self.with_name(inheritance_clause, "inheritanceClause"));
        }
        children.push(self.with_name(member_block, "memberBlock"));

        Ok((
            self.syntax_node(
                "ProtocolDeclSyntax",
                self.range_from_offsets(header.start_byte(), right_brace.end_byte()),
                children,
            ),
            close_index + 1,
        ))
    }

    fn enum_case_decl(&self, node: Node<'a>) -> Result<Value> {
        let case_keyword = self
            .immediate_child_kind(node, "case")
            .context("enum case declaration is missing 'case'")?;
        let names = self.enum_case_name_ranges(node, case_keyword);
        if names.is_empty() {
            bail!("enum case declaration is missing case elements");
        }
        let data_contents = self.field_children(node, "data_contents");
        let raw_values = self.field_children(node, "raw_value");

        let mut elements = Vec::new();
        for (index, (name_start, name_end)) in names.iter().copied().enumerate() {
            let next_name_start = names
                .get(index + 1)
                .map(|(next_start, _)| *next_start)
                .unwrap_or_else(|| node.end_byte());
            let data_content = data_contents.iter().copied().find(|candidate| {
                candidate.start_byte() >= name_end && candidate.end_byte() <= next_name_start
            });
            let raw_value = raw_values.iter().copied().find(|candidate| {
                candidate.start_byte() >= name_end && candidate.start_byte() < next_name_start
            });

            let mut element_children = vec![self.with_name(
                self.token_with_range(
                    &format!(
                        "identifier({})",
                        quoted_text(&self.source[name_start..name_end])
                    ),
                    self.range_from_offsets(name_start, name_end),
                ),
                "name",
            )];
            if let Some(parameter_node) = data_content {
                element_children.push(self.with_name(
                    self.enum_case_parameter_clause(parameter_node)?,
                    "parameterClause",
                ));
            }
            if let Some(raw_value) = raw_value {
                let equal = children(node)
                    .find(|child| {
                        child.kind() == "="
                            && child.start_byte() >= name_end
                            && child.end_byte() <= raw_value.start_byte()
                    })
                    .context("enum case raw value is missing '='")?;
                element_children
                    .push(self.with_name(self.initializer_clause(equal, raw_value)?, "rawValue"));
            }

            let element_content_end = raw_value
                .or(data_content)
                .map(|child| child.end_byte())
                .unwrap_or(name_end);
            let trailing_comma = children(node).find(|child| {
                child.kind() == ","
                    && child.start_byte() >= element_content_end
                    && child.end_byte() <= next_name_start
            });
            if let Some(comma) = trailing_comma {
                element_children
                    .push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
            }

            let element_end = trailing_comma
                .map(|comma| comma.end_byte())
                .unwrap_or(element_content_end);
            elements.push(self.with_name(
                self.syntax_node(
                    "EnumCaseElementSyntax",
                    self.range_from_offsets(name_start, element_end),
                    element_children,
                ),
                "",
            ));
        }

        let elements_range = self.covering_range_or_point(&elements, case_keyword.end_byte());
        Ok(self.syntax_node(
            "EnumCaseDeclSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.attribute_list(node)?, "attributes"),
                self.with_name(self.modifier_list(node), "modifiers"),
                self.with_name(
                    self.token_for_node(case_keyword, "keyword(SwiftSyntax.Keyword.case)"),
                    "caseKeyword",
                ),
                self.with_name(
                    self.syntax_node("EnumCaseElementListSyntax", elements_range, elements),
                    "elements",
                ),
            ],
        ))
    }

    fn enum_case_name_ranges(&self, node: Node<'a>, case_keyword: Node<'a>) -> Vec<(usize, usize)> {
        let names = self.field_children(node, "name");
        if !names.is_empty() {
            return names
                .into_iter()
                .map(|name| (name.start_byte(), name.end_byte()))
                .collect();
        }

        let mut ranges = Vec::new();
        let bytes = self.source.as_bytes();
        let mut cursor = case_keyword.end_byte();
        while cursor < node.end_byte() {
            while cursor < node.end_byte()
                && (bytes[cursor].is_ascii_whitespace()
                    || matches!(bytes[cursor], b',' | b';' | b'(' | b')'))
            {
                cursor += 1;
            }
            if cursor >= node.end_byte() {
                break;
            }

            let start = cursor;
            if bytes[cursor] == b'`' {
                cursor += 1;
                while cursor < node.end_byte() && bytes[cursor] != b'`' {
                    cursor += 1;
                }
                if cursor < node.end_byte() {
                    cursor += 1;
                }
            } else {
                while cursor < node.end_byte()
                    && !bytes[cursor].is_ascii_whitespace()
                    && !matches!(bytes[cursor], b',' | b'(' | b')' | b'=' | b';')
                {
                    cursor += 1;
                }
            }

            if start < cursor {
                ranges.push((start, cursor));
            }

            while cursor < node.end_byte() && bytes[cursor] != b',' {
                cursor += 1;
            }
        }
        ranges
    }

    fn enum_case_parameter_clause(&self, node: Node<'a>) -> Result<Value> {
        let left_paren = self
            .immediate_child_kind(node, "(")
            .context("enum case parameter clause is missing '('")?;
        let right_paren = self
            .immediate_child_kind(node, ")")
            .context("enum case parameter clause is missing ')'")?;
        Ok(self.syntax_node(
            "EnumCaseParameterClauseSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.token_for_node(left_paren, "leftParen"), "leftParen"),
                self.with_name(
                    self.syntax_node(
                        "EnumCaseParameterListSyntax",
                        self.range_from_offsets(left_paren.end_byte(), right_paren.start_byte()),
                        Vec::new(),
                    ),
                    "parameters",
                ),
                self.with_name(self.token_for_node(right_paren, "rightParen"), "rightParen"),
            ],
        ))
    }

    fn associated_type_decl(&self, node: Node<'a>) -> Result<Value> {
        let associatedtype_keyword = self
            .immediate_child_kind(node, "associatedtype")
            .context("associated type declaration is missing 'associatedtype'")?;
        let name = self
            .field_child(node, "name")
            .context("associated type declaration is missing a name")?;

        let mut decl_children = vec![
            self.with_name(self.attribute_list(node)?, "attributes"),
            self.with_name(self.modifier_list(node), "modifiers"),
            self.with_name(
                self.token_for_node(
                    associatedtype_keyword,
                    "keyword(SwiftSyntax.Keyword.associatedtype)",
                ),
                "associatedtypeKeyword",
            ),
            self.with_name(
                self.token_for_node(
                    name,
                    &format!("identifier({})", quoted_text(self.text(name))),
                ),
                "name",
            ),
        ];
        if let Some(inheritance_clause) = self.associated_type_inheritance_clause(node)? {
            decl_children.push(self.with_name(inheritance_clause, "inheritanceClause"));
        }
        if let Some(default_value) = self.field_child(node, "default_value") {
            let equal = children(node)
                .find(|child| {
                    child.kind() == "="
                        && child.start_byte() >= name.end_byte()
                        && child.end_byte() <= default_value.start_byte()
                })
                .context("associated type default value is missing '='")?;
            decl_children.push(self.with_name(
                self.type_initializer_clause(equal, default_value)?,
                "initializer",
            ));
        }

        Ok(self.syntax_node(
            "AssociatedTypeDeclSyntax",
            self.range_for_node(node),
            decl_children,
        ))
    }

    fn associated_type_inheritance_clause(&self, node: Node<'a>) -> Result<Option<Value>> {
        let colon = match self.immediate_child_kind(node, ":") {
            Some(colon) => colon,
            None => return Ok(None),
        };
        let inherited_nodes = self.field_children(node, "must_inherit");
        let boundary = self
            .immediate_child_kind(node, "=")
            .or_else(|| self.immediate_named_child_kind(node, "type_constraints"))
            .map(|child| child.start_byte())
            .unwrap_or_else(|| node.end_byte());
        let inherited_nodes = if inherited_nodes.is_empty() {
            self.type_node_after(node, colon.end_byte())
                .filter(|candidate| candidate.end_byte() <= boundary)
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            inherited_nodes
        };
        if inherited_nodes.is_empty() {
            return Ok(None);
        };

        let mut inherited_types = Vec::new();
        for inherited_node in inherited_nodes {
            inherited_types.push(self.with_name(
                self.syntax_node(
                    "InheritedTypeSyntax",
                    self.range_for_node(inherited_node),
                    vec![self.with_name(self.identifier_type(inherited_node)?, "type")],
                ),
                "",
            ));
        }
        let inherited_type_list_range =
            self.covering_range_or_point(&inherited_types, colon.end_byte());
        let inherited_type_list = self.syntax_node(
            "InheritedTypeListSyntax",
            inherited_type_list_range,
            inherited_types,
        );
        let clause_end = end_offset(&inherited_type_list);
        Ok(Some(self.syntax_node(
            "InheritanceClauseSyntax",
            self.range_from_offsets(colon.start_byte(), clause_end),
            vec![
                self.with_name(self.token_for_node(colon, "colon"), "colon"),
                self.with_name(inherited_type_list, "inheritedTypes"),
            ],
        )))
    }

    fn type_initializer_clause(&self, equal: Node<'a>, value: Node<'a>) -> Result<Value> {
        Ok(self.syntax_node(
            "TypeInitializerClauseSyntax",
            self.range_from_offsets(equal.start_byte(), value.end_byte()),
            vec![
                self.with_name(self.token_for_node(equal, "equal"), "equal"),
                self.with_name(self.identifier_type(value)?, "value"),
            ],
        ))
    }

    fn extension_decl(&self, node: Node<'a>, extension_keyword: Node<'a>) -> Result<Value> {
        let extended_type = self
            .field_child(node, "name")
            .context("extension declaration is missing extended type")?;
        let body = self
            .field_child(node, "body")
            .context("extension declaration is missing a body")?;

        let mut children = vec![
            self.with_name(self.attribute_list(node)?, "attributes"),
            self.with_name(self.modifier_list(node), "modifiers"),
            self.with_name(
                self.token_for_node(extension_keyword, "keyword(SwiftSyntax.Keyword.extension)"),
                "extensionKeyword",
            ),
            self.with_name(self.identifier_type(extended_type)?, "extendedType"),
        ];
        if let Some(inheritance_clause) = self.inheritance_clause(node)? {
            children.push(self.with_name(inheritance_clause, "inheritanceClause"));
        }
        children.push(self.with_name(
            self.member_block_without_case_recovery(body)?,
            "memberBlock",
        ));

        Ok(self.syntax_node("ExtensionDeclSyntax", self.range_for_node(node), children))
    }

    fn inheritance_clause(&self, node: Node<'a>) -> Result<Option<Value>> {
        let inherited_nodes: Vec<_> = named_children(node)
            .filter(|child| child.kind() == "inheritance_specifier")
            .collect();
        let Some(first_inherited) = inherited_nodes.first().copied() else {
            return Ok(None);
        };
        let colon = children(node)
            .find(|child| child.kind() == ":" && child.end_byte() <= first_inherited.start_byte())
            .context("inheritance clause is missing ':'")?;

        let mut inherited_types = Vec::new();
        for inherited_node in inherited_nodes {
            let type_node = self
                .field_child(inherited_node, "inherits_from")
                .or_else(|| self.first_named_child_excluding(inherited_node, &["attribute"]))
                .context("inheritance specifier is missing a type")?;
            let trailing_comma = self.trailing_delimiter(node, inherited_node, ",");
            let mut children = vec![self.with_name(self.identifier_type(type_node)?, "type")];
            if let Some(comma) = trailing_comma {
                children.push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
            }
            let end = trailing_comma.map_or(inherited_node.end_byte(), |comma| comma.end_byte());
            inherited_types.push(self.with_name(
                self.syntax_node(
                    "InheritedTypeSyntax",
                    self.range_from_offsets(inherited_node.start_byte(), end),
                    children,
                ),
                "",
            ));
        }

        let inherited_type_list_range =
            self.covering_range_or_point(&inherited_types, colon.end_byte());
        let inherited_type_list = self.syntax_node(
            "InheritedTypeListSyntax",
            inherited_type_list_range,
            inherited_types,
        );
        let clause_end = end_offset(&inherited_type_list);
        Ok(Some(self.syntax_node(
            "InheritanceClauseSyntax",
            self.range_from_offsets(colon.start_byte(), clause_end),
            vec![
                self.with_name(self.token_for_node(colon, "colon"), "colon"),
                self.with_name(inherited_type_list, "inheritedTypes"),
            ],
        )))
    }

    fn member_block(&self, node: Node<'a>) -> Result<Value> {
        self.member_block_with_case_recovery(node, true)
    }

    fn member_block_without_case_recovery(&self, node: Node<'a>) -> Result<Value> {
        self.member_block_with_case_recovery(node, false)
    }

    fn member_block_with_case_recovery(
        &self,
        node: Node<'a>,
        recover_case_errors: bool,
    ) -> Result<Value> {
        let left_brace = self
            .immediate_child_kind(node, "{")
            .context("member block is missing '{'")?;
        let right_brace = self
            .immediate_child_kind(node, "}")
            .context("member block is missing '}'")?;
        let mut items = Vec::new();
        for child in named_children(node) {
            if is_trivia_node(child)
                || self.is_ignorable_member_error(child)
                || (!recover_case_errors
                    && (child.kind() == "enum_entry"
                        || (child.kind() == "ERROR"
                            && self.text(child).trim_start().starts_with("case"))))
            {
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

    fn member_block_item_for_value(&self, value: Value) -> Value {
        let range = value["range"].clone();
        self.syntax_node(
            "MemberBlockItemSyntax",
            range,
            vec![self.with_name(value, "decl")],
        )
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

    fn attribute_list_before(&self, node: Node<'a>, boundary: usize) -> Result<Value> {
        let mut attributes = Vec::new();
        for modifiers in named_children(node)
            .filter(|child| child.kind() == "modifiers" && child.end_byte() <= boundary)
        {
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

    fn modifier_list_before(&self, node: Node<'a>, boundary: usize) -> Value {
        let mut modifiers = Vec::new();
        for modifier_container in named_children(node)
            .filter(|child| child.kind() == "modifiers" && child.end_byte() <= boundary)
        {
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

    fn initializer_decl(&self, node: Node<'a>) -> Result<Value> {
        let init_keyword = self
            .immediate_child_kind(node, "init")
            .context("initializer declaration is missing 'init'")?;
        let mut children = vec![
            self.with_name(self.attribute_list(node)?, "attributes"),
            self.with_name(self.modifier_list(node), "modifiers"),
            self.with_name(
                self.token_for_node(init_keyword, "keyword(SwiftSyntax.Keyword.init)"),
                "initKeyword",
            ),
        ];
        if let Some(optional_mark) = self.initializer_optional_mark(node, init_keyword) {
            let token_kind = if self.text(optional_mark) == "!" {
                "exclamationMark"
            } else {
                "postfixQuestionMark"
            };
            children.push(self.with_name(
                self.token_for_node(optional_mark, token_kind),
                "optionalMark",
            ));
        }
        children.push(self.with_name(self.function_signature(node)?, "signature"));
        if let Some(body_node) = self.field_child(node, "body") {
            children.push(self.with_name(self.code_block(body_node)?, "body"));
        }
        Ok(self.syntax_node("InitializerDeclSyntax", self.range_for_node(node), children))
    }

    fn deinitializer_decl(&self, node: Node<'a>) -> Result<Value> {
        let deinit_keyword = self
            .immediate_child_kind(node, "deinit")
            .context("deinitializer declaration is missing 'deinit'")?;
        let mut children = vec![
            self.with_name(self.attribute_list(node)?, "attributes"),
            self.with_name(self.modifier_list(node), "modifiers"),
            self.with_name(
                self.token_for_node(deinit_keyword, "keyword(SwiftSyntax.Keyword.deinit)"),
                "deinitKeyword",
            ),
        ];
        if let Some(body_node) = self.field_child(node, "body") {
            children.push(self.with_name(self.code_block(body_node)?, "body"));
        }
        Ok(self.syntax_node(
            "DeinitializerDeclSyntax",
            self.range_for_node(node),
            children,
        ))
    }

    fn subscript_decl(&self, node: Node<'a>) -> Result<Value> {
        let subscript_keyword = self
            .immediate_child_kind(node, "subscript")
            .context("subscript declaration is missing 'subscript'")?;
        let return_clause = self
            .return_clause(node)?
            .context("subscript declaration is missing return clause")?;
        let mut children = vec![
            self.with_name(self.attribute_list(node)?, "attributes"),
            self.with_name(self.modifier_list(node), "modifiers"),
            self.with_name(
                self.token_for_node(subscript_keyword, "keyword(SwiftSyntax.Keyword.subscript)"),
                "subscriptKeyword",
            ),
            self.with_name(self.function_parameter_clause(node)?, "parameterClause"),
            self.with_name(return_clause, "returnClause"),
        ];
        if let Some(accessor_block) = self.subscript_accessor_block(node)? {
            children.push(self.with_name(accessor_block, "accessorBlock"));
        }
        Ok(self.syntax_node("SubscriptDeclSyntax", self.range_for_node(node), children))
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
        let accessor_block_node = self
            .field_child(node, "computed_value")
            .or_else(|| self.immediate_named_child_kind(node, "protocol_property_requirements"));

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
        if let Some(accessor_block) = accessor_block_node {
            binding_children.push(self.with_name(
                self.variable_accessor_block(accessor_block)?,
                "accessorBlock",
            ));
        }

        let binding_range = self.range_from_offsets(
            pattern_node.start_byte(),
            accessor_block_node
                .or(value_node)
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

    fn is_recoverable_property_error(&self, node: Node<'a>) -> bool {
        if node.kind() != "ERROR" {
            return false;
        }
        let Some(binding_keyword) = self
            .first_descendant_any_kind(node, "let")
            .or_else(|| self.first_descendant_any_kind(node, "var"))
        else {
            return false;
        };
        self.recovered_property_name(node, binding_keyword)
            .is_some()
    }

    fn recovered_variable_decl(
        &self,
        node: Node<'a>,
        initializer_continuation: Option<Node<'a>>,
    ) -> Result<Value> {
        let binding_keyword = self
            .first_descendant_any_kind(node, "let")
            .or_else(|| self.first_descendant_any_kind(node, "var"))
            .context("recovered property declaration is missing let/var")?;
        let pattern_node = self
            .recovered_property_name(node, binding_keyword)
            .context("recovered property declaration is missing a name")?;
        let type_annotation_node = self.immediate_named_child_kind(node, "type_annotation");

        let mut binding_children = vec![self.with_name(self.pattern(pattern_node)?, "pattern")];
        if let Some(type_node) = type_annotation_node {
            binding_children
                .push(self.with_name(self.type_annotation(type_node)?, "typeAnnotation"));
        }
        if let Some(initializer) =
            self.recovered_property_initializer(node, pattern_node, initializer_continuation)?
        {
            binding_children.push(self.with_name(initializer, "initializer"));
        }

        let binding_range = self.range_from_offsets(
            pattern_node.start_byte(),
            binding_children
                .last()
                .map(end_offset)
                .unwrap_or_else(|| pattern_node.end_byte()),
        );
        let binding = self.syntax_node("PatternBindingSyntax", binding_range, binding_children);
        let bindings = self.with_name(
            self.syntax_node(
                "PatternBindingListSyntax",
                self.range_from_offsets(pattern_node.start_byte(), end_offset(&binding)),
                vec![self.with_name(binding, "")],
            ),
            "bindings",
        );
        let declaration_end = end_offset(&bindings);

        Ok(self.syntax_node(
            "VariableDeclSyntax",
            self.range_from_offsets(binding_keyword.start_byte(), declaration_end),
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

    fn recovered_property_name(
        &self,
        node: Node<'a>,
        binding_keyword: Node<'a>,
    ) -> Option<Node<'a>> {
        self.field_child(node, "name")
            .or_else(|| self.field_child(node, "bound_identifier"))
            .or_else(|| {
                self.first_descendant_kind_between(
                    node,
                    "simple_identifier",
                    binding_keyword.end_byte(),
                    node.end_byte(),
                )
            })
            .or_else(|| {
                self.first_descendant_kind_between(
                    node,
                    "identifier",
                    binding_keyword.end_byte(),
                    node.end_byte(),
                )
            })
    }

    fn recovered_property_initializer(
        &self,
        node: Node<'a>,
        pattern_node: Node<'a>,
        initializer_continuation: Option<Node<'a>>,
    ) -> Result<Option<Value>> {
        if let Some(continuation) = initializer_continuation {
            let equal_start = self
                .source
                .as_bytes()
                .get(continuation.start_byte()..continuation.end_byte())
                .and_then(|bytes| bytes.iter().position(|byte| *byte == b'='))
                .map(|relative| continuation.start_byte() + relative)
                .context("split property initializer is missing '='")?;
            return self
                .initializer_clause_from_offsets(equal_start, equal_start + 1, continuation, true)
                .map(Some);
        }

        let Some(equal) = children(node)
            .find(|child| child.kind() == "=" && child.start_byte() >= pattern_node.end_byte())
        else {
            return Ok(None);
        };
        let value = named_children(node)
            .filter(|child| child.start_byte() >= equal.end_byte())
            .find(|child| is_expression_like_node(*child))
            .context("recovered property initializer is missing a value")?;
        self.initializer_clause_from_offsets(equal.start_byte(), equal.end_byte(), value, false)
            .map(Some)
    }

    fn initializer_clause_from_offsets(
        &self,
        equal_start: usize,
        equal_end: usize,
        value: Node<'a>,
        strip_leading_equal: bool,
    ) -> Result<Value> {
        let value_expr = if strip_leading_equal {
            self.expr_for_split_initializer(value)?
        } else {
            self.expr(value)?
        };
        let end = end_offset(&value_expr);
        Ok(self.syntax_node(
            "InitializerClauseSyntax",
            self.range_from_offsets(equal_start, end),
            vec![
                self.with_name(
                    self.token_with_range("equal", self.range_from_offsets(equal_start, equal_end)),
                    "equal",
                ),
                self.with_name(value_expr, "value"),
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
            "wildcard_pattern" => self.wildcard_pattern(node),
            other => bail!("unsupported Swift pattern node '{other}'"),
        }
    }

    fn wildcard_pattern(&self, node: Node<'a>) -> Result<Value> {
        let wildcard = self
            .first_descendant_any_kind(node, "_")
            .or_else(|| self.immediate_child_kind(node, "_"))
            .unwrap_or(node);
        Ok(self.syntax_node(
            "WildcardPatternSyntax",
            self.range_for_node(node),
            vec![self.with_name(self.token_for_node(wildcard, "wildcard"), "wildcard")],
        ))
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

    fn identifier_type_from_offsets(&self, start: usize, end: usize) -> Value {
        let name = &self.source[start..end];
        self.syntax_node(
            "IdentifierTypeSyntax",
            self.range_from_offsets(start, end),
            vec![self.with_name(
                self.token_with_range(
                    &format!("identifier({})", quoted_text(name)),
                    self.range_from_offsets(start, end),
                ),
                "name",
            )],
        )
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
        let parameter_clause =
            self.with_name(self.function_parameter_clause(node)?, "parameterClause");

        let mut signature_children = vec![parameter_clause];
        if let Some(return_clause) = self.return_clause(node)? {
            signature_children.push(self.with_name(return_clause, "returnClause"));
        }

        let start = signature_children[0]["range"]["startOffset"]
            .as_u64()
            .unwrap_or_default() as usize;
        let end = signature_children.last().map_or(start, end_offset);
        Ok(self.syntax_node(
            "FunctionSignatureSyntax",
            self.range_from_offsets(start, end),
            signature_children,
        ))
    }

    fn function_parameter_clause(&self, node: Node<'a>) -> Result<Value> {
        let left_paren = self
            .immediate_child_kind(node, "(")
            .context("function parameter clause is missing '('")?;
        let right_paren = self
            .immediate_child_kind(node, ")")
            .context("function parameter clause is missing ')'")?;
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
        Ok(self.syntax_node(
            "FunctionParameterClauseSyntax",
            self.range_from_offsets(left_paren.start_byte(), right_paren.end_byte()),
            vec![
                self.with_name(self.token_for_node(left_paren, "leftParen"), "leftParen"),
                parameter_list,
                self.with_name(self.token_for_node(right_paren, "rightParen"), "rightParen"),
            ],
        ))
    }

    fn return_clause(&self, node: Node<'a>) -> Result<Option<Value>> {
        if let Some(arrow) = self.immediate_child_kind(node, "->") {
            let type_syntax = if let Some(return_type) = self
                .field_child(node, "return_type")
                .or_else(|| self.type_node_after(node, arrow.end_byte()))
            {
                Some(self.identifier_type(return_type)?)
            } else {
                self.synthetic_identifier_type_after_arrow(node, arrow)
            };
            if let Some(type_syntax) = type_syntax {
                return Ok(Some(self.syntax_node(
                    "ReturnClauseSyntax",
                    self.range_from_offsets(arrow.start_byte(), end_offset(&type_syntax)),
                    vec![
                        self.with_name(self.token_for_node(arrow, "arrow"), "arrow"),
                        self.with_name(type_syntax, "type"),
                    ],
                )));
            }
        }
        Ok(None)
    }

    fn function_parameter(&self, node: Node<'a>) -> Result<Value> {
        let name = self
            .field_child(node, "name")
            .and_then(|n| {
                self.first_descendant_kind(n, "simple_identifier")
                    .or(Some(n))
            })
            .context("function parameter is missing a name")?;
        let external_name = self.field_child(node, "external_name").and_then(|n| {
            self.first_descendant_kind(n, "simple_identifier")
                .or(Some(n))
        });
        let colon = self
            .immediate_child_kind(node, ":")
            .context("function parameter is missing ':'")?;
        let type_node = self
            .field_child(node, "type")
            .context("function parameter is missing a type")?;
        let mut children = vec![
            self.with_name(self.attribute_list(node)?, "attributes"),
            self.with_name(self.modifier_list(node), "modifiers"),
            self.with_name(
                self.identifier_or_wildcard_token(external_name.unwrap_or(name)),
                "firstName",
            ),
        ];
        if external_name.is_some() {
            children.push(self.with_name(self.identifier_or_wildcard_token(name), "secondName"));
        }
        children.push(self.with_name(self.token_for_node(colon, "colon"), "colon"));
        children.push(self.with_name(self.identifier_type(type_node)?, "type"));

        Ok(self.syntax_node(
            "FunctionParameterSyntax",
            self.range_for_node(node),
            children,
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

    fn code_block_item_list_from_statements(
        &self,
        statements: Option<Node<'a>>,
        fallback_offset: usize,
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
        let range = self.covering_range_or_point(&items, fallback_offset);
        Ok(self.syntax_node("CodeBlockItemListSyntax", range, items))
    }

    fn subscript_accessor_block(&self, node: Node<'a>) -> Result<Option<Value>> {
        let Some(computed_property) = self.immediate_named_child_kind(node, "computed_property")
        else {
            return Ok(None);
        };
        self.accessor_block_for_computed_property(computed_property, "subscript")
            .map(Some)
    }

    fn variable_accessor_block(&self, node: Node<'a>) -> Result<Value> {
        match node.kind() {
            "computed_property" => self.accessor_block_for_computed_property(node, "property"),
            "protocol_property_requirements" => self.protocol_property_accessor_block(node),
            other => bail!("unsupported variable accessor block node '{other}'"),
        }
    }

    fn accessor_block_for_computed_property(
        &self,
        computed_property: Node<'a>,
        context: &str,
    ) -> Result<Value> {
        let left_brace = self
            .immediate_child_kind(computed_property, "{")
            .with_context(|| format!("{context} accessor block is missing '{{'"))?;
        let right_brace = self
            .immediate_child_kind(computed_property, "}")
            .with_context(|| format!("{context} accessor block is missing '}}'"))?;

        let accessor_nodes: Vec<_> = named_children(computed_property)
            .filter(|child| {
                matches!(
                    child.kind(),
                    "computed_getter" | "computed_setter" | "computed_modify"
                )
            })
            .collect();
        let accessors = if accessor_nodes.is_empty() {
            self.with_name(
                self.code_block_item_list_from_statements(
                    self.immediate_named_child_kind(computed_property, "statements"),
                    left_brace.end_byte(),
                )?,
                "accessors",
            )
        } else {
            let mut accessor_items = Vec::new();
            for accessor in accessor_nodes {
                accessor_items.push(self.with_name(self.accessor_decl(accessor)?, ""));
            }
            let range = self.covering_range_or_point(&accessor_items, left_brace.end_byte());
            self.with_name(
                self.syntax_node("AccessorDeclListSyntax", range, accessor_items),
                "accessors",
            )
        };

        Ok(self.syntax_node(
            "AccessorBlockSyntax",
            self.range_for_node(computed_property),
            vec![
                self.with_name(self.token_for_node(left_brace, "leftBrace"), "leftBrace"),
                accessors,
                self.with_name(self.token_for_node(right_brace, "rightBrace"), "rightBrace"),
            ],
        ))
    }

    fn protocol_property_accessor_block(&self, node: Node<'a>) -> Result<Value> {
        let left_brace = self
            .immediate_child_kind(node, "{")
            .context("protocol property accessor block is missing '{'")?;
        let right_brace = self
            .immediate_child_kind(node, "}")
            .context("protocol property accessor block is missing '}'")?;
        let mut accessor_items = Vec::new();
        for accessor in named_children(node)
            .filter(|child| matches!(child.kind(), "getter_specifier" | "setter_specifier"))
        {
            accessor_items.push(self.with_name(self.accessor_decl(accessor)?, ""));
        }
        let range = self.covering_range_or_point(&accessor_items, left_brace.end_byte());
        Ok(self.syntax_node(
            "AccessorBlockSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.token_for_node(left_brace, "leftBrace"), "leftBrace"),
                self.with_name(
                    self.syntax_node("AccessorDeclListSyntax", range, accessor_items),
                    "accessors",
                ),
                self.with_name(self.token_for_node(right_brace, "rightBrace"), "rightBrace"),
            ],
        ))
    }

    fn accessor_decl(&self, node: Node<'a>) -> Result<Value> {
        let accessor_keyword = self
            .accessor_keyword_node(node)
            .context("accessor declaration is missing accessor keyword")?;
        let mut children = vec![self.with_name(self.attribute_list(node)?, "attributes")];
        if let Some(modifier) = self.first_descendant_kind(node, "mutation_modifier") {
            children.push(self.with_name(self.decl_modifier(modifier), "modifier"));
        }
        children.push(self.with_name(
            self.token_for_node(
                accessor_keyword,
                &format!(
                    "keyword(SwiftSyntax.Keyword.{})",
                    self.text(accessor_keyword)
                ),
            ),
            "accessorSpecifier",
        ));
        if let Some(parameters) = self.accessor_parameters(node)? {
            children.push(self.with_name(parameters, "parameters"));
        }
        if let Some(left_brace) = self.immediate_child_kind(node, "{") {
            if let Some(right_brace) = self.immediate_child_kind(node, "}") {
                let statements = self.immediate_named_child_kind(node, "statements");
                children.push(self.with_name(
                    self.code_block_from_statements(statements, left_brace, right_brace)?,
                    "body",
                ));
            }
        }
        Ok(self.syntax_node("AccessorDeclSyntax", self.range_for_node(node), children))
    }

    fn accessor_parameters(&self, node: Node<'a>) -> Result<Option<Value>> {
        let Some(left_paren) = self.immediate_child_kind(node, "(") else {
            return Ok(None);
        };
        let right_paren = self
            .immediate_child_kind(node, ")")
            .context("accessor parameters are missing ')'")?;
        let name = self
            .first_descendant_kind_between(
                node,
                "simple_identifier",
                left_paren.end_byte(),
                right_paren.start_byte(),
            )
            .or_else(|| {
                self.first_descendant_kind_between(
                    node,
                    "identifier",
                    left_paren.end_byte(),
                    right_paren.start_byte(),
                )
            })
            .context("accessor parameters are missing a name")?;
        Ok(Some(self.syntax_node(
            "AccessorParametersSyntax",
            self.range_from_offsets(left_paren.start_byte(), right_paren.end_byte()),
            vec![
                self.with_name(self.token_for_node(left_paren, "leftParen"), "leftParen"),
                self.with_name(
                    self.token_for_node(
                        name,
                        &format!("identifier({})", quoted_text(self.text(name))),
                    ),
                    "name",
                ),
                self.with_name(self.token_for_node(right_paren, "rightParen"), "rightParen"),
            ],
        )))
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
            "switch_statement" => self.switch_expr(node),
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
            "super_expression" => Ok(self.super_expr(node)),
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
            "nil" => Ok(self.syntax_node(
                "NilLiteralExprSyntax",
                self.range_for_node(node),
                vec![self.with_name(
                    self.token_for_node(node, "keyword(SwiftSyntax.Keyword.nil)"),
                    "nilKeyword",
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
            "ERROR" if is_identifier_like_text(self.text(node)) => {
                Ok(self.decl_reference_expr(node))
            }
            other => bail!("unsupported Swift expression node '{other}'"),
        }
    }

    fn is_split_initializer_continuation(&self, node: Node<'a>) -> bool {
        node.kind() == "call_expression" && self.text(node).trim_start().starts_with('=')
    }

    fn expr_for_split_initializer(&self, node: Node<'a>) -> Result<Value> {
        if self.is_split_initializer_continuation(node) {
            return self.recovered_call_expr_for_split_initializer(node);
        }
        self.expr(node)
    }

    fn recovered_call_expr_for_split_initializer(&self, node: Node<'a>) -> Result<Value> {
        let callee = named_children(node)
            .find(|child| child.kind() != "call_suffix")
            .context("split initializer call is missing callee")?;
        let suffix = self
            .immediate_named_child_kind(node, "call_suffix")
            .context("split initializer call is missing call suffix")?;
        let mut children = vec![self.with_name(self.expr(callee)?, "calledExpression")];

        if let Some(value_arguments) = self.immediate_named_child_kind(suffix, "value_arguments") {
            let left_paren = self
                .immediate_child_kind(value_arguments, "(")
                .context("split initializer call arguments are missing '('")?;
            let right_paren = self
                .immediate_child_kind(value_arguments, ")")
                .context("split initializer call arguments are missing ')'")?;
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
        children.push(self.with_name(
            self.empty_collection("MultipleTrailingClosureElementListSyntax", node.end_byte()),
            "additionalTrailingClosures",
        ));

        Ok(self.syntax_node(
            "FunctionCallExprSyntax",
            self.range_from_offsets(callee.start_byte(), node.end_byte()),
            children,
        ))
    }

    fn super_expr(&self, node: Node<'a>) -> Value {
        self.syntax_node(
            "SuperExprSyntax",
            self.range_for_node(node),
            vec![self.with_name(
                self.token_for_node(node, "keyword(SwiftSyntax.Keyword.super)"),
                "superKeyword",
            )],
        )
    }

    fn switch_expr(&self, node: Node<'a>) -> Result<Value> {
        let switch_keyword = self
            .immediate_child_kind(node, "switch")
            .context("switch statement is missing 'switch'")?;
        let subject = self
            .field_child(node, "expr")
            .context("switch statement is missing subject expression")?;
        let left_brace = self
            .immediate_child_kind(node, "{")
            .context("switch statement is missing '{'")?;
        let right_brace = self
            .immediate_child_kind(node, "}")
            .context("switch statement is missing '}'")?;

        let mut cases = Vec::new();
        for switch_entry in named_children(node).filter(|child| child.kind() == "switch_entry") {
            cases.push(self.with_name(self.switch_case(switch_entry)?, ""));
        }
        let cases_range = self.covering_range_or_point(&cases, left_brace.end_byte());
        Ok(self.syntax_node(
            "SwitchExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.token_for_node(switch_keyword, "keyword(SwiftSyntax.Keyword.switch)"),
                    "switchKeyword",
                ),
                self.with_name(self.expr(subject)?, "subject"),
                self.with_name(self.token_for_node(left_brace, "leftBrace"), "leftBrace"),
                self.with_name(
                    self.syntax_node("SwitchCaseListSyntax", cases_range, cases),
                    "cases",
                ),
                self.with_name(self.token_for_node(right_brace, "rightBrace"), "rightBrace"),
            ],
        ))
    }

    fn switch_case(&self, node: Node<'a>) -> Result<Value> {
        let colon = self
            .immediate_child_kind(node, ":")
            .context("switch case is missing ':'")?;
        let label = if let Some(default_keyword) = self.immediate_child_kind(node, "default") {
            self.syntax_node(
                "SwitchDefaultLabelSyntax",
                self.range_from_offsets(default_keyword.start_byte(), colon.end_byte()),
                vec![
                    self.with_name(
                        self.token_for_node(
                            default_keyword,
                            "keyword(SwiftSyntax.Keyword.default)",
                        ),
                        "defaultKeyword",
                    ),
                    self.with_name(self.token_for_node(colon, "colon"), "colon"),
                ],
            )
        } else {
            let case_keyword = self
                .immediate_child_kind(node, "case")
                .context("switch case is missing 'case'")?;
            let mut case_items = Vec::new();
            let patterns: Vec<_> = named_children(node)
                .filter(|child| child.kind() == "pattern" && child.end_byte() <= colon.start_byte())
                .collect();
            for (index, pattern) in patterns.iter().copied().enumerate() {
                let next_start = patterns
                    .get(index + 1)
                    .map(|next| next.start_byte())
                    .unwrap_or_else(|| colon.start_byte());
                let trailing_comma = children(node).find(|child| {
                    child.kind() == ","
                        && child.start_byte() >= pattern.end_byte()
                        && child.end_byte() <= next_start
                });
                let mut item_children =
                    vec![self.with_name(self.switch_case_pattern(pattern)?, "pattern")];
                if let Some(comma) = trailing_comma {
                    item_children
                        .push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
                }
                let item_end = trailing_comma.map_or(pattern.end_byte(), |comma| comma.end_byte());
                case_items.push(self.with_name(
                    self.syntax_node(
                        "SwitchCaseItemSyntax",
                        self.range_from_offsets(pattern.start_byte(), item_end),
                        item_children,
                    ),
                    "",
                ));
            }
            let item_range = self.covering_range_or_point(&case_items, case_keyword.end_byte());
            self.syntax_node(
                "SwitchCaseLabelSyntax",
                self.range_from_offsets(case_keyword.start_byte(), colon.end_byte()),
                vec![
                    self.with_name(
                        self.token_for_node(case_keyword, "keyword(SwiftSyntax.Keyword.case)"),
                        "caseKeyword",
                    ),
                    self.with_name(
                        self.syntax_node("SwitchCaseItemListSyntax", item_range, case_items),
                        "caseItems",
                    ),
                    self.with_name(self.token_for_node(colon, "colon"), "colon"),
                ],
            )
        };

        let statements = self.immediate_named_child_kind(node, "statements");
        Ok(self.syntax_node(
            "SwitchCaseSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(label, "label"),
                self.with_name(
                    self.code_block_item_list_from_statements(statements, colon.end_byte())?,
                    "statements",
                ),
            ],
        ))
    }

    fn switch_case_pattern(&self, node: Node<'a>) -> Result<Value> {
        if self.immediate_child_kind(node, "(").is_some() {
            return self.tuple_pattern(node);
        }
        if let Some(identifier) = self.first_descendant_kind(node, "simple_identifier") {
            return self.identifier_pattern(identifier);
        }
        if let Some(wildcard) = self.first_descendant_kind(node, "wildcard_pattern") {
            return self.wildcard_pattern(wildcard);
        }
        Ok(self.syntax_node(
            "ExpressionPatternSyntax",
            self.range_for_node(node),
            vec![self.with_name(self.expr(node)?, "expression")],
        ))
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

    fn identifier_or_wildcard_token(&self, node: Node<'a>) -> Value {
        if self.text(node) == "_" {
            self.token_for_node(node, "wildcard")
        } else {
            self.token_for_node(
                node,
                &format!("identifier({})", quoted_text(self.text(node))),
            )
        }
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

    fn initializer_optional_mark(
        &self,
        node: Node<'a>,
        init_keyword: Node<'a>,
    ) -> Option<Node<'a>> {
        let left_paren = self.immediate_child_kind(node, "(")?;
        children(node).find(|child| {
            matches!(child.kind(), "?" | "bang")
                && child.start_byte() >= init_keyword.end_byte()
                && child.end_byte() <= left_paren.start_byte()
        })
    }

    fn accessor_keyword_node(&self, node: Node<'a>) -> Option<Node<'a>> {
        ["get", "set", "_modify", "modify"]
            .iter()
            .find_map(|kind| self.first_descendant_any_kind(node, kind))
    }

    fn type_node_after(&self, node: Node<'a>, offset: usize) -> Option<Node<'a>> {
        named_children(node).find(|child| {
            child.start_byte() >= offset
                && matches!(
                    child.kind(),
                    "array_type"
                        | "bracket_qualified_type"
                        | "dictionary_type"
                        | "existential_type"
                        | "function_type"
                        | "metatype"
                        | "opaque_type"
                        | "optional_type"
                        | "protocol_composition_type"
                        | "suppressed_constraint"
                        | "tuple_type"
                        | "type_identifier"
                        | "type_pack_expansion"
                        | "type_parameter_pack"
                        | "user_type"
                )
        })
    }

    fn synthetic_identifier_type_after_arrow(
        &self,
        node: Node<'a>,
        arrow: Node<'a>,
    ) -> Option<Value> {
        let mut start = arrow.end_byte();
        let mut end = node.end_byte();
        while start < end && self.source.as_bytes()[start].is_ascii_whitespace() {
            start += 1;
        }
        while end > start && self.source.as_bytes()[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        (start < end).then(|| self.identifier_type_from_offsets(start, end))
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

    fn first_descendant_kind_between(
        &self,
        node: Node<'a>,
        kind: &str,
        start: usize,
        end: usize,
    ) -> Option<Node<'a>> {
        if node.kind() == kind && node.start_byte() >= start && node.end_byte() <= end {
            return Some(node);
        }
        for child in children(node) {
            if child.end_byte() < start || child.start_byte() > end {
                continue;
            }
            if let Some(found) = self.first_descendant_kind_between(child, kind, start, end) {
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

fn is_expression_like_node(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "additive_expression"
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
            | "line_string_literal"
            | "multiplicative_expression"
            | "navigation_expression"
            | "nil"
            | "prefix_expression"
            | "range_expression"
            | "real_literal"
            | "self_expression"
            | "simple_identifier"
            | "super_expression"
            | "tuple_expression"
    )
}

fn is_identifier_like_text(text: &str) -> bool {
    let trimmed = text.trim();
    let unquoted = trimmed
        .strip_prefix('`')
        .and_then(|rest| rest.strip_suffix('`'))
        .unwrap_or(trimmed);
    let mut chars = unquoted.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_alphabetic()) && chars.all(|ch| ch == '_' || ch.is_alphanumeric())
}

fn operator_token_kind(fixity: &str) -> &'static str {
    match fixity {
        "prefix" => "prefixOperator",
        "postfix" => "postfixOperator",
        _ => "binaryOperator",
    }
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
    fn emits_operator_and_precedence_group_declarations() {
        let source = r#"
precedencegroup F {
  higherThan: A, B
}
infix operator *-* : FunnyPrecedence
infix operator  <*<<< : MediumPrecedence, &
prefix operator ^^ : PrefixMagicOperatorProtocol
infix operator  <*< : MediumPrecedence, InfixMagicOperatorProtocol
postfix operator ^^ : PostfixMagicOperatorProtocol
infix operator ^^ : PostfixMagicOperatorProtocol, Class, Struct
protocol Proto {}
infix operator *<*< : F, Proto
class Foo {
  infix operator |||
}
prefix operator /^/
"#;
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();

        let precedence_groups = find_node_types(&value, "PrecedenceGroupDeclSyntax");
        assert_eq!(precedence_groups.len(), 1);
        assert_eq!(
            precedence_groups[0]["children"][3]["tokenKind"],
            "identifier(\"F\")"
        );

        let operators = find_node_types(&value, "OperatorDeclSyntax");
        assert_eq!(operators.len(), 9);
        let token_kinds = operators
            .iter()
            .map(|node| node["children"][2]["tokenKind"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(token_kinds.contains(&"binaryOperator(\"*-*\")"));
        assert!(token_kinds.contains(&"binaryOperator(\"<*<<<\")"));
        assert!(token_kinds.contains(&"prefixOperator(\"^^\")"));
        assert!(token_kinds.contains(&"binaryOperator(\"<*<\")"));
        assert!(token_kinds.contains(&"postfixOperator(\"^^\")"));
        assert!(token_kinds.contains(&"binaryOperator(\"*<*<\")"));
        assert!(token_kinds.contains(&"binaryOperator(\"|||\")"));
        assert!(token_kinds.contains(&"prefixOperator(\"/^/\")"));

        assert!(operators.iter().any(|node| {
            node["children"]
                .as_array()
                .unwrap()
                .iter()
                .any(|child| child["name"] == "operatorPrecedenceAndTypes")
        }));
    }

    #[test]
    fn recovers_split_precedence_group_declarations() {
        let source = r#"
precedencegroup FooGroup {
  higherThan: Group1, Group2
  lowerThan: Group3, Group4
  associativity: left
  assignment: false
}
precedencegroup FunnyPrecedence {
  associativity: left
  higherThan: MultiplicationPrecedence
}
"#;
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let statements = value["children"][0]["children"].as_array().unwrap();
        assert_eq!(statements.len(), 2);

        let precedence_groups = find_node_types(&value, "PrecedenceGroupDeclSyntax");
        assert_eq!(precedence_groups.len(), 2);
        let names = precedence_groups
            .iter()
            .map(|node| node["children"][3]["tokenKind"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "identifier(\"FooGroup\")",
                "identifier(\"FunnyPrecedence\")"
            ]
        );
    }

    #[test]
    fn recovers_protocol_decl_split_by_invalid_initialized_members() {
        let source = r#"
public protocol A {}
private protocol B {
  var b = 0.0
}
protocol Foo: Bar {
  public var a = A()
  private var b = false
  var c = 0.0
  var d: String?

  static var e = 1
  static var f = true

  var g: Double { return self * 1_000.0 }

  init(paramA: String, paramB: Int) {
    self.init()
  }

  private func someFunc() {}

  override internal func someMethod() {
    super.someMethod()
  }

  mutating func square() {
    self = self * self
  }
}
extension Foo: SomeProtocol, AnotherProtocol {
  func someOtherFunc() {}
}
"#;
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let protocols = find_node_types(&value, "ProtocolDeclSyntax");
        assert_eq!(protocols.len(), 3);

        let foo = protocols
            .iter()
            .find(|protocol| {
                protocol["children"][2]["tokenKind"] == "keyword(SwiftSyntax.Keyword.protocol)"
                    && protocol["children"][3]["tokenKind"] == "identifier(\"Foo\")"
            })
            .unwrap();
        let members = find_node_types(foo, "MemberBlockItemSyntax");
        assert_eq!(members.len(), 11);
        let variables = find_node_types(foo, "VariableDeclSyntax");
        assert_eq!(variables.len(), 7);
        let first_initializer =
            find_first_node_type(variables[0], "InitializerClauseSyntax").unwrap();
        assert_eq!(first_initializer["children"][0]["tokenKind"], "equal");
        assert_eq!(
            first_initializer["children"][1]["nodeType"],
            "FunctionCallExprSyntax"
        );
        assert_eq!(
            find_first_node_type(foo, "SuperExprSyntax").unwrap()["children"][0]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.super)"
        );
    }

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
    fn emits_function_parameter_external_labels() {
        let source = "func handle(_ gesture: UIScreenEdgePanGestureRecognizer) {}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let parameter_list = find_first_node_type(&value, "FunctionParameterListSyntax").unwrap();
        let parameters = &parameter_list["children"];
        assert_eq!(parameters.as_array().unwrap().len(), 1);
        let parameter = &parameters[0];
        assert_eq!(parameter["children"][2]["name"], "firstName");
        assert_eq!(parameter["children"][2]["tokenKind"], "wildcard");
        assert_eq!(parameter["children"][3]["name"], "secondName");
        assert_eq!(
            parameter["children"][3]["tokenKind"],
            "identifier(\"gesture\")"
        );
        assert_eq!(parameter["children"][4]["tokenKind"], "colon");
        assert_eq!(parameter["children"][5]["nodeType"], "IdentifierTypeSyntax");
    }

    #[test]
    fn emits_initializer_and_deinitializer_declarations() {
        let source =
            "class Foo {\n  init!(int: Int) {}\n  init?(text: String) {}\n  deinit {}\n  deinit\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let initializers = find_node_types(&value, "InitializerDeclSyntax");
        assert_eq!(initializers.len(), 2);
        assert_eq!(
            initializers[0]["children"][2]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.init)"
        );
        assert_eq!(
            initializers[0]["children"][3]["tokenKind"],
            "exclamationMark"
        );
        assert_eq!(
            initializers[0]["children"][4]["nodeType"],
            "FunctionSignatureSyntax"
        );
        assert_eq!(
            initializers[0]["children"][5]["nodeType"],
            "CodeBlockSyntax"
        );
        assert_eq!(
            initializers[1]["children"][3]["tokenKind"],
            "postfixQuestionMark"
        );

        let deinitializer = find_first_node_type(&value, "DeinitializerDeclSyntax").unwrap();
        assert_eq!(
            deinitializer["children"][2]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.deinit)"
        );
        assert_eq!(deinitializer["children"][3]["nodeType"], "CodeBlockSyntax");
        assert_eq!(find_node_types(&value, "DeinitializerDeclSyntax").len(), 1);
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
    fn emits_actor_declarations_and_members() {
        let source = "actor MyActor {\n  init() {}\n  func hello() {}\n  func foo(x: String) -> Int { return 0 }\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let actor_decl = find_first_node_type(&value, "ActorDeclSyntax").unwrap();
        assert_eq!(
            actor_decl["children"][2]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.actor)"
        );
        assert_eq!(
            actor_decl["children"][3]["tokenKind"],
            "identifier(\"MyActor\")"
        );

        let member_block = actor_decl["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|child| child["name"] == "memberBlock")
            .unwrap();
        let members = &member_block["children"][1]["children"];
        assert_eq!(members.as_array().unwrap().len(), 3);
        assert_eq!(
            members[0]["children"][0]["nodeType"],
            "InitializerDeclSyntax"
        );
        assert_eq!(members[1]["children"][0]["nodeType"], "FunctionDeclSyntax");
        assert_eq!(members[2]["children"][0]["nodeType"], "FunctionDeclSyntax");
    }

    #[test]
    fn emits_extension_declarations() {
        let source = "public extension Foo: Bar, Baz {\n  var d: Int { return 1 }\n  func someFooFunc() {}\n}\n";
        let value = parse_source("Ext.swift", "/tmp/Ext.swift", source).unwrap();
        let extension_decl = find_first_node_type(&value, "ExtensionDeclSyntax").unwrap();
        assert_eq!(
            extension_decl["children"][0]["nodeType"],
            "AttributeListSyntax"
        );
        assert_eq!(
            extension_decl["children"][1]["nodeType"],
            "DeclModifierListSyntax"
        );
        assert_eq!(
            extension_decl["children"][1]["children"][0]["children"][0]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.public)"
        );
        assert_eq!(
            extension_decl["children"][2]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.extension)"
        );
        assert_eq!(extension_decl["children"][3]["name"], "extendedType");
        assert_eq!(
            extension_decl["children"][3]["children"][0]["tokenKind"],
            "identifier(\"Foo\")"
        );

        let inheritance_clause = extension_decl["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|child| child["name"] == "inheritanceClause")
            .unwrap();
        assert_eq!(inheritance_clause["nodeType"], "InheritanceClauseSyntax");
        assert_eq!(inheritance_clause["children"][0]["tokenKind"], "colon");
        let inherited_types = &inheritance_clause["children"][1]["children"];
        assert_eq!(inherited_types.as_array().unwrap().len(), 2);
        assert_eq!(
            inherited_types[0]["children"][0]["children"][0]["tokenKind"],
            "identifier(\"Bar\")"
        );
        assert_eq!(inherited_types[0]["children"][1]["tokenKind"], "comma");
        assert_eq!(
            inherited_types[1]["children"][0]["children"][0]["tokenKind"],
            "identifier(\"Baz\")"
        );

        let member_block = extension_decl["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|child| child["name"] == "memberBlock")
            .unwrap();
        let members = &member_block["children"][1]["children"];
        assert_eq!(members.as_array().unwrap().len(), 2);
        assert_eq!(members[0]["children"][0]["nodeType"], "VariableDeclSyntax");
        assert_eq!(members[1]["children"][0]["nodeType"], "FunctionDeclSyntax");
    }

    #[test]
    fn emits_subscript_declarations_with_direct_bodies() {
        let source =
            "struct TimesTable {\n  subscript(index: Int) -> Int {\n    return index\n  }\n  subscript(i: Int) -> Int\n}\n";
        let value = parse_source("Sub.swift", "/tmp/Sub.swift", source).unwrap();
        let subscripts = find_node_types(&value, "SubscriptDeclSyntax");
        assert_eq!(subscripts.len(), 2);
        let subscript = subscripts[0];
        assert_eq!(
            subscript["children"][2]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.subscript)"
        );
        assert_eq!(
            subscript["children"][3]["nodeType"],
            "FunctionParameterClauseSyntax"
        );
        assert_eq!(subscript["children"][4]["nodeType"], "ReturnClauseSyntax");
        assert_eq!(subscript["children"][5]["nodeType"], "AccessorBlockSyntax");
        assert_eq!(
            subscript["children"][5]["children"][1]["nodeType"],
            "CodeBlockItemListSyntax"
        );
        assert_eq!(
            subscript["children"][5]["children"][1]["children"][0]["children"][0]["nodeType"],
            "ReturnStmtSyntax"
        );

        let bodyless = subscripts[1];
        assert_eq!(
            bodyless["children"][3]["nodeType"],
            "FunctionParameterClauseSyntax"
        );
        assert_eq!(bodyless["children"][4]["nodeType"], "ReturnClauseSyntax");
        assert!(bodyless["children"]
            .as_array()
            .unwrap()
            .iter()
            .all(|child| child["name"] != "accessorBlock"));
    }

    #[test]
    fn emits_subscript_declarations_with_accessors() {
        let source = "struct X {\n  subscript(i: Int) -> Int {\n    get { return i }\n    mutating set(v) { stored = v }\n  }\n}\n";
        let value = parse_source("Sub.swift", "/tmp/Sub.swift", source).unwrap();
        let subscript = find_first_node_type(&value, "SubscriptDeclSyntax").unwrap();
        let accessor_block = subscript["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|child| child["name"] == "accessorBlock")
            .unwrap();
        assert_eq!(
            accessor_block["children"][1]["nodeType"],
            "AccessorDeclListSyntax"
        );
        let accessors = &accessor_block["children"][1]["children"];
        assert_eq!(accessors.as_array().unwrap().len(), 2);
        assert_eq!(
            accessors[0]["children"][1]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.get)"
        );
        assert_eq!(accessors[0]["children"][2]["nodeType"], "CodeBlockSyntax");
        assert_eq!(
            accessors[1]["children"][1]["nodeType"],
            "DeclModifierSyntax"
        );
        assert_eq!(
            accessors[1]["children"][2]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.set)"
        );
        assert_eq!(
            accessors[1]["children"][3]["nodeType"],
            "AccessorParametersSyntax"
        );
        assert_eq!(
            accessors[1]["children"][3]["children"][1]["tokenKind"],
            "identifier(\"v\")"
        );
        assert_eq!(accessors[1]["children"][4]["nodeType"], "CodeBlockSyntax");
    }

    #[test]
    fn emits_nominal_type_inheritance_clauses() {
        let source = "class Foo: Bar, Baz {}\nstruct Quux: Codable {}\n";
        let value = parse_source("Types.swift", "/tmp/Types.swift", source).unwrap();

        let class_decl = find_first_node_type(&value, "ClassDeclSyntax").unwrap();
        let class_inheritance = class_decl["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|child| child["name"] == "inheritanceClause")
            .unwrap();
        let class_inherited_types = &class_inheritance["children"][1]["children"];
        assert_eq!(class_inherited_types.as_array().unwrap().len(), 2);
        assert_eq!(
            class_inherited_types[0]["children"][0]["children"][0]["tokenKind"],
            "identifier(\"Bar\")"
        );
        assert_eq!(
            class_inherited_types[1]["children"][0]["children"][0]["tokenKind"],
            "identifier(\"Baz\")"
        );

        let struct_decl = find_first_node_type(&value, "StructDeclSyntax").unwrap();
        let struct_inheritance = struct_decl["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|child| child["name"] == "inheritanceClause")
            .unwrap();
        assert_eq!(
            struct_inheritance["children"][1]["children"][0]["children"][0]["children"][0]
                ["tokenKind"],
            "identifier(\"Codable\")"
        );
    }

    #[test]
    fn emits_enum_declarations_and_cases() {
        let source = "enum Color: Int {\n  @xyz case red = 1, green, grayscale(Int), blue = nil\n  init() { self = .red }\n}\n";
        let value = parse_source("Enum.swift", "/tmp/Enum.swift", source).unwrap();
        let enum_decl = find_first_node_type(&value, "EnumDeclSyntax").unwrap();
        assert_eq!(
            enum_decl["children"][2]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.enum)"
        );
        assert_eq!(
            enum_decl["children"][3]["tokenKind"],
            "identifier(\"Color\")"
        );

        let inheritance_clause = enum_decl["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|child| child["name"] == "inheritanceClause")
            .unwrap();
        assert_eq!(
            inheritance_clause["children"][1]["children"][0]["children"][0]["children"][0]
                ["tokenKind"],
            "identifier(\"Int\")"
        );

        let enum_case = find_first_node_type(&value, "EnumCaseDeclSyntax").unwrap();
        assert_eq!(
            enum_case["children"][0]["children"][0]["nodeType"],
            "AttributeSyntax"
        );
        assert_eq!(
            enum_case["children"][2]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.case)"
        );
        let elements = &enum_case["children"][3]["children"];
        assert_eq!(elements.as_array().unwrap().len(), 4);
        assert_eq!(
            elements[0]["children"][0]["tokenKind"],
            "identifier(\"red\")"
        );
        assert_eq!(
            elements[0]["children"][1]["nodeType"],
            "InitializerClauseSyntax"
        );
        assert_eq!(
            elements[0]["children"][1]["children"][1]["nodeType"],
            "IntegerLiteralExprSyntax"
        );
        assert_eq!(
            elements[1]["children"][0]["tokenKind"],
            "identifier(\"green\")"
        );
        assert_eq!(
            elements[2]["children"][0]["tokenKind"],
            "identifier(\"grayscale\")"
        );
        assert_eq!(
            elements[2]["children"][1]["nodeType"],
            "EnumCaseParameterClauseSyntax"
        );
        assert_eq!(
            elements[3]["children"][0]["tokenKind"],
            "identifier(\"blue\")"
        );
        assert_eq!(
            elements[3]["children"][1]["children"][1]["nodeType"],
            "NilLiteralExprSyntax"
        );

        let member_block = enum_decl["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|child| child["name"] == "memberBlock")
            .unwrap();
        let members = &member_block["children"][1]["children"];
        assert_eq!(members.as_array().unwrap().len(), 2);
        assert_eq!(members[0]["children"][0]["nodeType"], "EnumCaseDeclSyntax");
        assert_eq!(
            members[1]["children"][0]["nodeType"],
            "InitializerDeclSyntax"
        );
    }

    #[test]
    fn emits_protocol_declarations_and_members() {
        let source = "public protocol Drawable: Shape {\n  associatedtype Item: View = DefaultView\n  var area: Int { get set }\n  func draw(_ value: Item) -> Int\n  init()\n}\n";
        let value = parse_source("Protocol.swift", "/tmp/Protocol.swift", source).unwrap();
        let protocol_decl = find_first_node_type(&value, "ProtocolDeclSyntax").unwrap();
        assert_eq!(
            protocol_decl["children"][1]["children"][0]["children"][0]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.public)"
        );
        assert_eq!(
            protocol_decl["children"][2]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.protocol)"
        );
        assert_eq!(
            protocol_decl["children"][3]["tokenKind"],
            "identifier(\"Drawable\")"
        );

        let inheritance_clause = protocol_decl["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|child| child["name"] == "inheritanceClause")
            .unwrap();
        assert_eq!(
            inheritance_clause["children"][1]["children"][0]["children"][0]["children"][0]
                ["tokenKind"],
            "identifier(\"Shape\")"
        );

        let member_block = protocol_decl["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|child| child["name"] == "memberBlock")
            .unwrap();
        let members = &member_block["children"][1]["children"];
        assert_eq!(members.as_array().unwrap().len(), 4);
        assert_eq!(
            members[0]["children"][0]["nodeType"],
            "AssociatedTypeDeclSyntax"
        );
        assert_eq!(members[1]["children"][0]["nodeType"], "VariableDeclSyntax");
        assert_eq!(members[2]["children"][0]["nodeType"], "FunctionDeclSyntax");
        assert_eq!(
            members[3]["children"][0]["nodeType"],
            "InitializerDeclSyntax"
        );

        let associated_type = &members[0]["children"][0];
        assert_eq!(
            associated_type["children"][2]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.associatedtype)"
        );
        assert_eq!(
            associated_type["children"][3]["tokenKind"],
            "identifier(\"Item\")"
        );
        assert!(associated_type["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|child| child["name"] == "inheritanceClause"));
        assert!(associated_type["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|child| child["name"] == "initializer"));

        let function = &members[2]["children"][0];
        assert_eq!(
            function["children"][2]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.func)"
        );
        assert_eq!(function["children"][3]["tokenKind"], "identifier(\"draw\")");
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
