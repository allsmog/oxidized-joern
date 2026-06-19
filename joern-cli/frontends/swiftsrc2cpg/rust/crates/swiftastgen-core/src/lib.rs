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

struct FunctionCallComponents<'tree, 'closures> {
    parent: Node<'tree>,
    callee: Node<'tree>,
    callee_suffix: Node<'tree>,
    value_arguments: Option<Node<'tree>>,
    empty_arguments_offset: usize,
    trailing_suffix: Node<'tree>,
    trailing_closures: &'closures [Node<'tree>],
    start: usize,
    end: usize,
}

#[derive(Clone, Copy)]
struct RegexListItem {
    literal_start: usize,
    literal_end: usize,
    comma: Option<(usize, usize)>,
}

struct StringLiteralSpec {
    start: usize,
    end: usize,
    opening_pounds: Option<(usize, usize)>,
    opening_quote: (usize, usize),
    closing_quote: (usize, usize),
    closing_pounds: Option<(usize, usize)>,
    segment_specs: Vec<(usize, usize, String)>,
}

struct StringLiteralNodeSpec {
    start: usize,
    end: usize,
    opening_pounds: Option<(usize, usize)>,
    opening_quote: (usize, usize),
    closing_quote: (usize, usize),
    closing_pounds: Option<(usize, usize)>,
    segments: Vec<Value>,
}

struct TernaryNodeParts<'a> {
    start: usize,
    end: usize,
    question_mark: Node<'a>,
    then_expression: Node<'a>,
    colon: Node<'a>,
    else_expression: Node<'a>,
}

struct TernaryValueParts<'a> {
    start: usize,
    end: usize,
    condition: Value,
    question_mark: Node<'a>,
    then_expression: Value,
    colon: Node<'a>,
    else_expression: Value,
}

#[derive(Clone, Copy)]
struct RecoveredSwitchCaseSlice {
    label_start: usize,
    keyword_end: usize,
    colon_start: usize,
    colon_end: usize,
    body_start: usize,
    end: usize,
    is_default: bool,
}

#[derive(Clone, Copy)]
struct PostfixDirectiveCallParts<'a> {
    directive: Node<'a>,
    navigation: Node<'a>,
    period: Node<'a>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DirectiveKind {
    If,
    ElseIf,
    Else,
    EndIf,
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
            if let Some((postfix_if_config, next_index)) =
                self.postfix_if_config_expr_from_nodes(&root_children, child_index)?
            {
                statement_items.push(self.code_block_item_for_value(postfix_if_config, "item"));
                child_index = next_index;
                continue;
            }
            if self.is_if_config_start(child) {
                let (if_config, next_index) =
                    self.if_config_decl_from_code_block_nodes(&root_children, child_index)?;
                statement_items.push(self.code_block_item_for_value(if_config, "item"));
                child_index = next_index;
                continue;
            }
            if self.is_recoverable_if_case_error(child) {
                let if_expr = self.recovered_if_case_expr(child)?;
                statement_items.push(self.code_block_item_for_value(if_expr, "item"));
                child_index += 1;
                continue;
            }
            if let Some(typealias_decl) = self.recovered_suppressed_typealias_decl(child)? {
                statement_items.push(self.code_block_item_for_value(typealias_decl, "item"));
                child_index = self
                    .skip_same_line_nodes(&root_children, child_index + 1, child.end_position().row)
                    .max(child_index + 1);
                continue;
            }
            if is_trivia_node(child)
                || is_ignorable_directive(child)
                || self.is_ignorable_diagnostic(child)
                || self.is_ignorable_directive_call(child)
                || self.is_ignorable_top_level_error(child)
                || self.is_ignorable_top_level_function_type_fragment(child)
                || self.is_regex_delimiter_error(child)
            {
                child_index += 1;
                continue;
            }
            if let Some(next) = root_children.get(child_index + 1).copied() {
                if child.kind() == "statement_label" {
                    let labeled_stmt = self.labeled_stmt(child, next)?;
                    statement_items.push(self.code_block_item_for_value(labeled_stmt, "item"));
                    child_index += 2;
                    continue;
                }
                if self.is_split_keyword_apply_call(child, next) {
                    let call = self.recovered_keyword_apply_call(child, next)?;
                    statement_items.push(self.code_block_item_for_value(call, "item"));
                    child_index += 2;
                    continue;
                }
                if self.is_recoverable_regex_assignment(child, next) {
                    let assignment = self.recovered_regex_assignment(child, next)?;
                    statement_items.push(self.code_block_item_for_value(assignment, "item"));
                    child_index += 2;
                    continue;
                }
                if self.is_split_copy_variable_decl(child, next) {
                    let declaration = self.recovered_copy_variable_decl(child, next)?;
                    statement_items.push(self.code_block_item_for_value(declaration, "item"));
                    child_index += 2;
                    continue;
                }
                if self.is_split_move_variable_decl(child, next) {
                    statement_items.push(self.code_block_item(child)?);
                    child_index += 2;
                    continue;
                }
                if self.is_split_bare_macro_variable_decl(child, next) {
                    let declaration = self.recovered_variable_decl(child, Some(next))?;
                    statement_items.push(self.code_block_item_for_value(declaration, "item"));
                    child_index += 2;
                    continue;
                }
            }
            if self.is_recoverable_escaped_raw_assignment(child) {
                let mut end_node = child;
                let mut next_index = child_index + 1;
                while let Some(candidate) = root_children.get(next_index).copied() {
                    if candidate.start_byte() < end_node.end_byte()
                        || (!self.is_escaped_raw_recovery_continuation(candidate)
                            && !is_trivia_node(candidate))
                    {
                        break;
                    }
                    end_node = candidate;
                    next_index += 1;
                    if self.is_escaped_raw_closing_error(candidate) {
                        break;
                    }
                }
                let assignment = self.recovered_escaped_raw_assignment(child, end_node)?;
                statement_items.push(self.code_block_item_for_value(assignment, "item"));
                child_index = next_index.max(child_index + 1);
                continue;
            }
            if self.is_recoverable_precedence_group_error(child) {
                let (decl, next_index) =
                    self.recovered_precedence_group_decl(&root_children, child_index)?;
                statement_items.push(self.code_block_item_for_value(decl, "item"));
                child_index = next_index;
                continue;
            }
            if self.is_recoverable_do_error(child) {
                let value = self.recovered_do_syntax_from_error(child)?;
                statement_items.push(self.code_block_item_for_value(value, "item"));
                child_index = self.skip_do_cast_artifacts(&root_children, child_index + 1);
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
        if node.kind() != "ERROR" {
            return false;
        }
        let trimmed = self.text(node).trim();
        (trimmed == "}" && named_children(node).all(is_trivia_node))
            || (trimmed == "async" && named_children(node).all(is_trivia_node))
            || trimmed == "?"
            || trimmed.starts_with(',')
            || self.is_standalone_attribute_error(node)
            || (!trimmed.is_empty() && trimmed.chars().all(|ch| ch == '!'))
    }

    fn skip_same_line_nodes(&self, nodes: &[Node<'a>], mut index: usize, row: usize) -> usize {
        while nodes
            .get(index)
            .is_some_and(|node| node.start_position().row == row)
        {
            index += 1;
        }
        index
    }

    fn is_ignorable_top_level_function_type_fragment(&self, node: Node<'a>) -> bool {
        if node.kind() != "call_expression" {
            return false;
        }
        let trimmed = self.text(node).trim_start();
        trimmed.starts_with("() ->")
    }

    fn is_ignorable_diagnostic(&self, node: Node<'a>) -> bool {
        node.kind() == "diagnostic" && self.text(node).trim_start().starts_with("#sourceLocation")
    }

    fn is_if_config_start(&self, node: Node<'a>) -> bool {
        self.directive_keyword_info(node)
            .is_some_and(|(kind, _, _, _)| kind == DirectiveKind::If)
    }

    fn directive_keyword_info(
        &self,
        node: Node<'a>,
    ) -> Option<(DirectiveKind, usize, usize, &'static str)> {
        if node.kind() != "directive" {
            return None;
        }
        let text = self.text(node);
        let trimmed = text.trim_start();
        let leading = text.len() - trimmed.len();
        let keyword = if starts_directive_keyword(trimmed, "#elseif") {
            (DirectiveKind::ElseIf, "#elseif", "poundElseif")
        } else if starts_directive_keyword(trimmed, "#else") {
            (DirectiveKind::Else, "#else", "poundElse")
        } else if starts_directive_keyword(trimmed, "#endif") {
            (DirectiveKind::EndIf, "#endif", "poundEndif")
        } else if starts_directive_keyword(trimmed, "#if") {
            (DirectiveKind::If, "#if", "poundIf")
        } else {
            return None;
        };
        let start = node.start_byte() + leading;
        Some((keyword.0, start, start + keyword.1.len(), keyword.2))
    }

    fn is_ignorable_directive_call(&self, node: Node<'a>) -> bool {
        node.kind() == "call_expression"
            && named_children(node)
                .next()
                .is_some_and(|child| child.kind() == "directive")
    }

    fn is_standalone_attribute_error(&self, node: Node<'a>) -> bool {
        node.kind() == "ERROR"
            && self.text(node).trim_start().starts_with('@')
            && named_children(node).all(|child| {
                child.kind() == "attribute"
                    || is_trivia_node(child)
                    || is_ignorable_directive(child)
            })
    }

    fn is_regex_delimiter_error(&self, node: Node<'a>) -> bool {
        node.kind() == "ERROR" && self.text(node).chars().all(|ch| ch == '#')
    }

    fn is_bare_macro_error(&self, node: Node<'a>) -> bool {
        node.kind() == "ERROR"
            && self.bare_macro_pound_start(node).is_some()
            && named_children(node)
                .any(|child| matches!(child.kind(), "simple_identifier" | "identifier"))
    }

    fn bare_macro_pound_start(&self, node: Node<'a>) -> Option<usize> {
        let text = self.text(node);
        let hash_offset = text.find('#')?;
        let prefix = text[..hash_offset].trim();
        if prefix.is_empty() || prefix == "=" {
            Some(node.start_byte() + hash_offset)
        } else {
            None
        }
    }

    fn is_recoverable_escaped_raw_assignment(&self, node: Node<'a>) -> bool {
        matches!(node.kind(), "assignment" | "ERROR")
            && (self.text(node).contains("\\\"\\\"") || self.text(node).contains("\\\"\""))
            && self.starts_with_escaped_raw_delimiter(node)
            && self.assignment_lhs(node).is_some()
            && self.assignment_equal(node).is_some()
    }

    fn starts_with_escaped_raw_delimiter(&self, node: Node<'a>) -> bool {
        let Some(equal) = self.assignment_equal(node) else {
            return false;
        };
        let mut rhs_start = equal.end_byte();
        while rhs_start < node.end_byte() && self.source.as_bytes()[rhs_start].is_ascii_whitespace()
        {
            rhs_start += 1;
        }
        let pounds = self.count_hashes(rhs_start, node.end_byte());
        self.source[rhs_start + pounds..node.end_byte()].starts_with("\\\"")
    }

    fn is_escaped_raw_closing_error(&self, node: Node<'a>) -> bool {
        node.kind() == "ERROR"
            && (self.text(node).contains("\\\"\\\"") || self.text(node).contains("\\\"\""))
    }

    fn is_escaped_raw_recovery_continuation(&self, node: Node<'a>) -> bool {
        matches!(
            node.kind(),
            "ERROR" | "regex_literal" | "line_string_literal"
        )
    }

    fn is_recoverable_regex_assignment(&self, assignment: Node<'a>, next: Node<'a>) -> bool {
        assignment.kind() == "assignment"
            && self.is_regex_delimiter_error(next)
            && self
                .field_child(assignment, "result")
                .is_some_and(|rhs| rhs.kind() == "regex_literal")
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

    fn labeled_stmt(&self, label: Node<'a>, statement: Node<'a>) -> Result<Value> {
        let label_text = self.text(label);
        let colon_relative = label_text
            .rfind(':')
            .context("statement label is missing ':'")?;
        let label_start = label.start_byte();
        let colon_start = label.start_byte() + colon_relative;
        let (name_start, name_end) = self.trim_offsets(label_start, colon_start);
        let statement_syntax = self.syntax_for_statement(statement)?;
        Ok(self.syntax_node(
            "LabeledStmtSyntax",
            self.range_from_offsets(label.start_byte(), statement.end_byte()),
            vec![
                self.with_name(
                    self.token_with_range(
                        &format!(
                            "identifier({})",
                            quoted_text(&self.source[name_start..name_end])
                        ),
                        self.range_from_offsets(name_start, name_end),
                    ),
                    "label",
                ),
                self.with_name(
                    self.token_with_range(
                        "colon",
                        self.range_from_offsets(colon_start, colon_start + 1),
                    ),
                    "colon",
                ),
                self.with_name(statement_syntax, "statement"),
            ],
        ))
    }

    fn syntax_for_statement(&self, node: Node<'a>) -> Result<Value> {
        match node.kind() {
            "property_declaration" => self.variable_decl(node),
            "protocol_property_declaration" => self.variable_decl(node),
            "function_declaration" => self.function_decl(node),
            "protocol_function_declaration" => self.function_decl(node),
            "typealias_declaration" => self.typealias_decl(node),
            "class_declaration" => self.nominal_type_decl(node),
            "protocol_declaration" => self.protocol_decl(node),
            "operator_declaration" => self.operator_decl(node),
            "precedence_group_declaration" => self.precedence_group_decl(node),
            "control_transfer_statement" => self.control_transfer_stmt(node),
            "call_expression" if self.is_return_do_expr_call(node) => {
                self.recovered_return_do_stmt(node)
            }
            "call_expression" if self.is_recoverable_return_call(node) => {
                self.recovered_return_call_stmt(node)
            }
            "call_expression" if self.is_do_expr_call(node) => self.do_expr(node),
            "call_expression" if self.is_defer_stmt(node) => self.defer_stmt(node),
            "do_statement" => self.do_stmt(node),
            "for_statement" => self.for_stmt(node),
            "guard_statement" => self.guard_stmt(node),
            "if_statement" => self.if_expr(node),
            "import_declaration" => self.import_decl(node),
            "repeat_while_statement" => self.repeat_stmt(node),
            "switch_statement" => self.switch_expr(node),
            "while_statement" => self.while_stmt(node),
            "ERROR" if self.is_recoverable_precedence_group_error(node) => {
                self.precedence_group_decl(node)
            }
            "ERROR" if self.is_recoverable_missing_if_error(node) => {
                self.recovered_missing_if_expr(node)
            }
            "ERROR" if self.is_recoverable_do_error(node) => {
                self.recovered_do_syntax_from_error(node)
            }
            "ERROR" if self.is_recoverable_array_expr_error(node) => {
                self.recovered_array_expr(node)
            }
            "ERROR" if self.is_recoverable_if_case_error(node) => self.recovered_if_case_expr(node),
            "ERROR" if self.is_bare_macro_error(node) => self.macro_expansion_decl(node),
            "diagnostic" => self.macro_expansion_decl(node),
            "macro_invocation" => self.macro_expansion_decl(node),
            "assignment"
            | "additive_expression"
            | "array_literal"
            | "as_expression"
            | "await_expression"
            | "boolean_literal"
            | "call_expression"
            | "check_expression"
            | "comparison_expression"
            | "conjunction_expression"
            | "constructor_expression"
            | "consume_expression"
            | "dictionary_literal"
            | "disjunction_expression"
            | "equality_expression"
            | "integer_literal"
            | "key_path_expression"
            | "lambda_literal"
            | "line_string_literal"
            | "multi_line_string_literal"
            | "multiplicative_expression"
            | "prefix_expression"
            | "raw_string_literal"
            | "range_expression"
            | "real_literal"
            | "regex_literal"
            | "nil"
            | "self_expression"
            | "special_literal"
            | "super_expression"
            | "ternary_expression"
            | "tuple_expression"
            | "try_expression"
            | "user_type"
            | "navigation_expression" => self.expr(node),
            "simple_identifier" if self.text(node) == "return" => {
                Ok(self.return_stmt_from_keyword(node))
            }
            "simple_identifier" => self.expr(node),
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
            "typealias_declaration" => self.typealias_decl(node),
            "class_declaration" => self.nominal_type_decl(node),
            "protocol_declaration" => self.protocol_decl(node),
            "operator_declaration" => self.operator_decl(node),
            "precedence_group_declaration" => self.precedence_group_decl(node),
            "call_expression" if self.is_defer_stmt(node) => self.defer_stmt(node),
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
            .or_else(|| self.unnamed_operator_decl_name(node, operator_keyword))
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

    fn unnamed_operator_decl_name(
        &self,
        node: Node<'a>,
        operator_keyword: Node<'a>,
    ) -> Option<Node<'a>> {
        children(node).find(|child| {
            child.start_byte() >= operator_keyword.end_byte()
                && child.kind() != ":"
                && !child.is_named()
                && !self.text(*child).trim().is_empty()
        })
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
            if !(is_trivia_node(member)
                || is_ignorable_directive(member)
                || self.is_ignorable_member_error(member))
            {
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

    fn typealias_decl(&self, node: Node<'a>) -> Result<Value> {
        let typealias_keyword = self
            .immediate_child_kind(node, "typealias")
            .context("typealias declaration is missing 'typealias'")?;
        let name = self
            .field_child(node, "name")
            .or_else(|| {
                named_children(node).find(|child| {
                    child.kind() == "type_identifier"
                        && child.start_byte() > typealias_keyword.end_byte()
                })
            })
            .context("typealias declaration is missing a name")?;
        let equal = children(node)
            .find(|child| child.kind() == "=" && child.start_byte() > name.end_byte())
            .context("typealias declaration is missing '='")?;
        let mut values =
            named_children(node).filter(|child| child.start_byte() >= equal.end_byte());
        let value = values
            .next()
            .context("typealias declaration is missing an initializer")?;
        let value = if value.kind() == "type_modifiers" {
            values
                .next()
                .context("typealias declaration type modifiers are missing a base type")?
        } else {
            value
        };

        Ok(self.syntax_node(
            "TypeAliasDeclSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.attribute_list(node)?, "attributes"),
                self.with_name(self.modifier_list(node), "modifiers"),
                self.with_name(
                    self.token_for_node(
                        typealias_keyword,
                        "keyword(SwiftSyntax.Keyword.typealias)",
                    ),
                    "typealiasKeyword",
                ),
                self.with_name(
                    self.token_for_node(
                        name,
                        &format!("identifier({})", quoted_text(self.text(name))),
                    ),
                    "name",
                ),
                self.with_name(self.type_initializer_clause(equal, value)?, "initializer"),
            ],
        ))
    }

    fn recovered_suppressed_typealias_decl(&self, node: Node<'a>) -> Result<Option<Value>> {
        if node.kind() != "ERROR" {
            return Ok(None);
        }

        let (start, end) = self.trim_offsets(node.start_byte(), node.end_byte());
        let text = &self.source[start..end];
        let Some(after_keyword) = text.strip_prefix("typealias") else {
            return Ok(None);
        };
        if !after_keyword
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            return Ok(None);
        }

        let keyword_end = start + "typealias".len();
        let name = named_children(node)
            .find(|child| {
                child.start_byte() >= keyword_end
                    && matches!(child.kind(), "simple_identifier" | "type_identifier")
            })
            .context("recovered typealias declaration is missing a name")?;
        let equal_start = self.source[name.end_byte()..end]
            .find('=')
            .map(|offset| name.end_byte() + offset)
            .context("recovered typealias declaration is missing '='")?;
        let value = self
            .first_descendant_type_after(node, equal_start + 1)
            .context("recovered typealias declaration is missing an initializer")?;

        Ok(Some(self.syntax_node(
            "TypeAliasDeclSyntax",
            self.range_from_offsets(start, end),
            vec![
                self.with_name(self.attribute_list(node)?, "attributes"),
                self.with_name(self.modifier_list(node), "modifiers"),
                self.with_name(
                    self.token_with_range(
                        "keyword(SwiftSyntax.Keyword.typealias)",
                        self.range_from_offsets(start, keyword_end),
                    ),
                    "typealiasKeyword",
                ),
                self.with_name(
                    self.token_for_node(
                        name,
                        &format!("identifier({})", quoted_text(self.text(name))),
                    ),
                    "name",
                ),
                self.with_name(
                    self.type_initializer_clause_from_value(
                        equal_start,
                        equal_start + 1,
                        self.type_syntax(value)?,
                    ),
                    "initializer",
                ),
            ],
        )))
    }

    fn type_initializer_clause_from_value(
        &self,
        equal_start: usize,
        equal_end: usize,
        value: Value,
    ) -> Value {
        let value_end = end_offset(&value);
        self.syntax_node(
            "TypeInitializerClauseSyntax",
            self.range_from_offsets(equal_start, value_end),
            vec![
                self.with_name(
                    self.token_with_range("equal", self.range_from_offsets(equal_start, equal_end)),
                    "equal",
                ),
                self.with_name(value, "value"),
            ],
        )
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
                self.with_name(self.type_syntax(value)?, "value"),
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
                || is_ignorable_directive(child)
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
        let value_node = self.value_field_child(node);
        let value_end = value_node.map(|value| self.recovered_value_end(node, value));
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
            binding_children.push(self.with_name(
                self.initializer_clause_with_end(
                    equal,
                    value,
                    value_end.unwrap_or(value.end_byte()),
                )?,
                "initializer",
            ));
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
                .map(|accessor_block| accessor_block.end_byte())
                .or(value_end)
                .or_else(|| type_annotation_node.map(|type_annotation| type_annotation.end_byte()))
                .unwrap_or_else(|| pattern_node.end_byte()),
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

        let declaration_end = accessor_block_node
            .map(|accessor_block| accessor_block.end_byte())
            .or(value_end)
            .or_else(|| type_annotation_node.map(|type_annotation| type_annotation.end_byte()))
            .unwrap_or_else(|| node.end_byte())
            .max(node.end_byte());

        Ok(self.syntax_node(
            "VariableDeclSyntax",
            self.range_from_offsets(node.start_byte(), declaration_end),
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

    fn is_split_copy_variable_decl(&self, declaration: Node<'a>, continuation: Node<'a>) -> bool {
        declaration.kind() == "property_declaration"
            && continuation.kind() == "ERROR"
            && self
                .field_child(declaration, "value")
                .is_some_and(|value| self.text(value) == "copy")
            && is_identifier_like_text(self.text(continuation))
    }

    fn is_split_move_variable_decl(&self, declaration: Node<'a>, continuation: Node<'a>) -> bool {
        declaration.kind() == "property_declaration"
            && continuation.kind() == "ERROR"
            && self
                .field_child(declaration, "value")
                .is_some_and(|value| self.text(value) == "_move")
            && is_identifier_like_text(self.text(continuation))
    }

    fn is_split_ownership_statement(&self, statement: Node<'a>, continuation: Node<'a>) -> bool {
        statement.kind() == "simple_identifier"
            && matches!(self.text(statement), "_move" | "_borrow")
            && continuation.kind() == "ERROR"
            && is_identifier_like_text(self.text(continuation))
    }

    fn is_split_keyword_apply_call(&self, callee: Node<'a>, arguments: Node<'a>) -> bool {
        if callee.kind() != "ERROR"
            || arguments.kind() != "tuple_expression"
            || !is_identifier_like_text(self.text(callee))
            || self.immediate_child_kind(arguments, "(").is_none()
            || self.immediate_child_kind(arguments, ")").is_none()
        {
            return false;
        }
        let between = &self.source[callee.end_byte()..arguments.start_byte()];
        !between.contains('\n') && between.chars().all(char::is_whitespace)
    }

    fn recovered_keyword_apply_call(
        &self,
        callee: Node<'a>,
        arguments_tuple: Node<'a>,
    ) -> Result<Value> {
        let left_paren = self
            .immediate_child_kind(arguments_tuple, "(")
            .context("keyword apply call arguments are missing '('")?;
        let right_paren = self
            .immediate_child_kind(arguments_tuple, ")")
            .context("keyword apply call arguments are missing ')'")?;
        Ok(self.syntax_node(
            "FunctionCallExprSyntax",
            self.range_from_offsets(callee.start_byte(), arguments_tuple.end_byte()),
            vec![
                self.with_name(self.decl_reference_expr(callee), "calledExpression"),
                self.with_name(self.token_for_node(left_paren, "leftParen"), "leftParen"),
                self.with_name(
                    self.labeled_expr_list_from_tuple_expr(
                        arguments_tuple,
                        left_paren,
                        right_paren,
                    )?,
                    "arguments",
                ),
                self.with_name(self.token_for_node(right_paren, "rightParen"), "rightParen"),
                self.with_name(
                    self.empty_collection(
                        "MultipleTrailingClosureElementListSyntax",
                        arguments_tuple.end_byte(),
                    ),
                    "additionalTrailingClosures",
                ),
            ],
        ))
    }

    fn is_split_bare_macro_variable_decl(
        &self,
        declaration: Node<'a>,
        continuation: Node<'a>,
    ) -> bool {
        declaration.kind() == "property_declaration" && self.is_bare_macro_error(continuation)
    }

    fn recovered_copy_variable_decl(
        &self,
        declaration: Node<'a>,
        continuation: Node<'a>,
    ) -> Result<Value> {
        let binding_keyword = self
            .first_descendant_any_kind(declaration, "let")
            .or_else(|| self.first_descendant_any_kind(declaration, "var"))
            .context("recovered copy declaration is missing let/var")?;
        let pattern_node = self
            .field_child(declaration, "name")
            .context("recovered copy declaration is missing a name")?;
        let copy_keyword = self
            .field_child(declaration, "value")
            .context("recovered copy declaration is missing copy keyword")?;
        let equal = self
            .immediate_child_kind(declaration, "=")
            .context("recovered copy declaration is missing '='")?;

        let mut binding_children = vec![self.with_name(self.pattern(pattern_node)?, "pattern")];
        if let Some(type_node) = self.immediate_named_child_kind(declaration, "type_annotation") {
            binding_children
                .push(self.with_name(self.type_annotation(type_node)?, "typeAnnotation"));
        }

        let value_expr = self.ownership_expr_from_parts(
            "CopyExprSyntax",
            "copyKeyword",
            copy_keyword,
            continuation,
        )?;
        let initializer = self.syntax_node(
            "InitializerClauseSyntax",
            self.range_from_offsets(equal.start_byte(), end_offset(&value_expr)),
            vec![
                self.with_name(self.token_for_node(equal, "equal"), "equal"),
                self.with_name(value_expr, "value"),
            ],
        );
        binding_children.push(self.with_name(initializer, "initializer"));

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

        Ok(self.syntax_node(
            "VariableDeclSyntax",
            self.range_from_offsets(binding_keyword.start_byte(), end_offset(&bindings)),
            vec![
                self.with_name(self.attribute_list(declaration)?, "attributes"),
                self.with_name(self.modifier_list(declaration), "modifiers"),
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
            .or_else(|| self.first_descendant_kind(node, "wildcard_pattern"))
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
            let continuation_equal = self
                .source
                .as_bytes()
                .get(continuation.start_byte()..continuation.end_byte())
                .and_then(|bytes| bytes.iter().position(|byte| *byte == b'='))
                .map(|relative| continuation.start_byte() + relative);
            let declaration_equal = children(node)
                .find(|child| child.kind() == "=" && child.start_byte() >= pattern_node.end_byte())
                .map(|equal| (equal.start_byte(), equal.end_byte()));
            let (equal_start, equal_end, strip_leading_equal) =
                if let Some(equal_start) = continuation_equal {
                    (equal_start, equal_start + 1, true)
                } else if let Some((equal_start, equal_end)) = declaration_equal {
                    (equal_start, equal_end, false)
                } else {
                    bail!("split property initializer is missing '='");
                };
            return self
                .initializer_clause_from_offsets(
                    equal_start,
                    equal_end,
                    continuation,
                    strip_leading_equal,
                )
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
            "pattern" => {
                if let Some(is_type_pattern) = self.is_type_pattern(node)? {
                    return Ok(is_type_pattern);
                }
                if let Some(binding) =
                    named_children(node).find(|child| child.kind() == "value_binding_pattern")
                {
                    return self.value_binding_pattern(node, binding);
                }
                if self.immediate_child_kind(node, "(").is_some() {
                    return self.tuple_pattern(node);
                }
                if let Some(child) = named_children(node).find(|child| {
                    matches!(
                        child.kind(),
                        "identifier" | "pattern" | "simple_identifier" | "wildcard_pattern"
                    ) || is_expression_like_node(*child)
                }) {
                    return self.pattern(child);
                }
                bail!("pattern is empty")
            }
            "identifier" | "simple_identifier" => self.identifier_pattern(node),
            "wildcard_pattern" => self.wildcard_pattern(node),
            _ if is_expression_like_node(node) => self.expression_pattern(node),
            other => bail!("unsupported Swift pattern node '{other}'"),
        }
    }

    fn is_type_pattern(&self, node: Node<'a>) -> Result<Option<Value>> {
        let (start, end) = self.trim_offsets(node.start_byte(), node.end_byte());
        let Some(rest) = self.source[start..end].strip_prefix("is") else {
            return Ok(None);
        };
        if !rest
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            return Ok(None);
        }
        let is_end = start + "is".len();
        let type_node = self
            .field_child(node, "name")
            .or_else(|| self.type_node_after(node, is_end));
        let type_syntax = if let Some(type_node) = type_node {
            self.type_syntax(type_node)?
        } else {
            let (type_start, type_end) = self.trim_offsets(is_end, end);
            self.identifier_type_from_offsets(type_start, type_end)
        };
        Ok(Some(self.syntax_node(
            "IsTypePatternSyntax",
            self.range_from_offsets(start, end),
            vec![
                self.with_name(
                    self.token_with_range(
                        "keyword(SwiftSyntax.Keyword.is)",
                        self.range_from_offsets(start, is_end),
                    ),
                    "isKeyword",
                ),
                self.with_name(type_syntax, "type"),
            ],
        )))
    }

    fn value_binding_pattern(&self, node: Node<'a>, binding: Node<'a>) -> Result<Value> {
        let binding_kind = format!("keyword(SwiftSyntax.Keyword.{})", self.text(binding));
        let inner_pattern = if let Some(left_paren) = children(node)
            .find(|child| child.kind() == "(" && child.start_byte() >= binding.end_byte())
        {
            let right_paren = children(node)
                .filter(|child| child.kind() == ")" && child.start_byte() >= left_paren.end_byte())
                .last()
                .context("value binding tuple pattern is missing ')'")?;
            self.tuple_pattern_from_parens(
                node,
                left_paren,
                right_paren,
                left_paren.start_byte(),
                right_paren.end_byte(),
            )?
        } else {
            let child = named_children(node)
                .filter(|child| child.start_byte() >= binding.end_byte())
                .find(|child| {
                    child.kind() != "value_binding_pattern" && child.kind() != "type_annotation"
                })
                .context("value binding pattern is missing its bound pattern")?;
            self.pattern(child)?
        };
        let end = end_offset(&inner_pattern);
        Ok(self.syntax_node(
            "ValueBindingPatternSyntax",
            self.range_from_offsets(binding.start_byte(), end),
            vec![
                self.with_name(
                    self.token_for_node(binding, &binding_kind),
                    "bindingSpecifier",
                ),
                self.with_name(inner_pattern, "pattern"),
            ],
        ))
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

    fn expression_pattern(&self, node: Node<'a>) -> Result<Value> {
        Ok(self.syntax_node(
            "ExpressionPatternSyntax",
            self.range_for_node(node),
            vec![self.with_name(self.expr(node)?, "expression")],
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
                self.with_name(self.type_syntax(type_node)?, "type"),
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
        self.tuple_pattern_from_parens(
            node,
            left_paren,
            right_paren,
            node.start_byte(),
            node.end_byte(),
        )
    }

    fn tuple_pattern_from_parens(
        &self,
        node: Node<'a>,
        left_paren: Node<'a>,
        right_paren: Node<'a>,
        start: usize,
        end: usize,
    ) -> Result<Value> {
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
            self.range_from_offsets(start, end),
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
        if node.kind() == "user_type" {
            return self.user_type_syntax(node);
        }
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

    fn user_type_syntax(&self, node: Node<'a>) -> Result<Value> {
        let names = self.immediate_type_identifiers(node);
        let first = names
            .first()
            .copied()
            .context("user type is missing type identifier")?;
        let type_arguments = self.immediate_named_child_kind(node, "type_arguments");

        if names.len() == 1 {
            return self.identifier_type_for_name(
                first,
                node.start_byte(),
                node.end_byte(),
                type_arguments,
            );
        }

        let mut current =
            self.identifier_type_for_name(first, first.start_byte(), first.end_byte(), None)?;
        let mut previous = first;
        for (index, name) in names.iter().copied().enumerate().skip(1) {
            let period = self
                .children_between(node, previous.end_byte(), name.start_byte())
                .into_iter()
                .find(|child| child.kind() == ".")
                .context("member type is missing '.'")?;
            let is_last = index + 1 == names.len();
            let mut children = vec![
                self.with_name(current, "baseType"),
                self.with_name(self.token_for_node(period, "period"), "period"),
                self.with_name(
                    self.token_for_node(
                        name,
                        &format!("identifier({})", quoted_text(self.text(name))),
                    ),
                    "name",
                ),
            ];
            let end = if is_last {
                if let Some(arguments) = type_arguments {
                    children.push(self.with_name(
                        self.generic_argument_clause(arguments)?,
                        "genericArgumentClause",
                    ));
                    arguments.end_byte()
                } else {
                    name.end_byte()
                }
            } else {
                name.end_byte()
            };
            current = self.syntax_node(
                "MemberTypeSyntax",
                self.range_from_offsets(node.start_byte(), end),
                children,
            );
            previous = name;
        }
        Ok(current)
    }

    fn identifier_type_for_name(
        &self,
        name: Node<'a>,
        start: usize,
        end: usize,
        generic_arguments: Option<Node<'a>>,
    ) -> Result<Value> {
        let mut children = vec![self.with_name(
            self.token_for_node(
                name,
                &format!("identifier({})", quoted_text(self.text(name))),
            ),
            "name",
        )];
        if let Some(arguments) = generic_arguments {
            children.push(self.with_name(
                self.generic_argument_clause(arguments)?,
                "genericArgumentClause",
            ));
        }
        Ok(self.syntax_node(
            "IdentifierTypeSyntax",
            self.range_from_offsets(start, end),
            children,
        ))
    }

    fn type_syntax(&self, node: Node<'a>) -> Result<Value> {
        if node.kind() == "type_modifiers" {
            if let Some(base) = self.base_type_sibling_after_modifiers(node) {
                if let Some(recovered) = self.recovered_function_type_from_modifiers(node, base)? {
                    return Ok(recovered);
                }
            }
        }

        let node = self.base_type_after_modifiers(node)?;
        match node.kind() {
            "array_type" => self.array_type(node),
            "dictionary_type" => self.dictionary_type(node),
            "existential_type" | "opaque_type" => self.some_or_any_type(node),
            "function_type" => self.function_type(node),
            "metatype" => self.metatype_type(node),
            "optional_type" => self.optional_type(node),
            "suppressed_constraint" => self.suppressed_type(node),
            "tuple_type" => self.tuple_type(node),
            "type_pack_expansion" => self.pack_expansion_type(node),
            "type_identifier" | "user_type" => self.identifier_type(node),
            "type_parameter_pack" => self.pack_element_type(node),
            "tuple_type_item" => {
                let type_node = named_children(node)
                    .next()
                    .context("tuple type item is missing a type")?;
                self.type_syntax(type_node)
            }
            other => bail!("unsupported Swift type node '{other}'"),
        }
    }

    fn base_type_after_modifiers(&self, node: Node<'a>) -> Result<Node<'a>> {
        if node.kind() != "type_modifiers" {
            return Ok(node);
        }

        if let Some(base) = self.base_type_sibling_after_modifiers(node) {
            return Ok(base);
        }

        named_children(node)
            .find(|child| is_type_syntax_node_kind(child.kind()))
            .context("type modifiers are missing a base type")
    }

    fn base_type_sibling_after_modifiers(&self, node: Node<'a>) -> Option<Node<'a>> {
        let parent = node.parent()?;
        named_children(parent).find(|child| {
            child.start_byte() >= node.end_byte() && is_type_syntax_node_kind(child.kind())
        })
    }

    fn array_type(&self, node: Node<'a>) -> Result<Value> {
        let left_square = self
            .immediate_child_kind(node, "[")
            .context("array type is missing '['")?;
        let right_square = self
            .immediate_child_kind(node, "]")
            .context("array type is missing ']'")?;
        let element = named_children(node)
            .find(|child| {
                child.start_byte() >= left_square.end_byte()
                    && child.end_byte() <= right_square.start_byte()
            })
            .context("array type is missing element type")?;

        Ok(self.syntax_node(
            "ArrayTypeSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.token_for_node(left_square, "leftSquare"), "leftSquare"),
                self.with_name(self.type_syntax(element)?, "element"),
                self.with_name(
                    self.token_for_node(right_square, "rightSquare"),
                    "rightSquare",
                ),
            ],
        ))
    }

    fn dictionary_type(&self, node: Node<'a>) -> Result<Value> {
        let left_square = self
            .immediate_child_kind(node, "[")
            .context("dictionary type is missing '['")?;
        let colon = self
            .immediate_child_kind(node, ":")
            .context("dictionary type is missing ':'")?;
        let right_square = self
            .immediate_child_kind(node, "]")
            .context("dictionary type is missing ']'")?;
        let key = named_children(node)
            .find(|child| {
                child.start_byte() >= left_square.end_byte()
                    && child.end_byte() <= colon.start_byte()
            })
            .context("dictionary type is missing key type")?;
        let value = named_children(node)
            .find(|child| {
                child.start_byte() >= colon.end_byte()
                    && child.end_byte() <= right_square.start_byte()
            })
            .context("dictionary type is missing value type")?;

        Ok(self.syntax_node(
            "DictionaryTypeSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.token_for_node(left_square, "leftSquare"), "leftSquare"),
                self.with_name(self.type_syntax(key)?, "key"),
                self.with_name(self.token_for_node(colon, "colon"), "colon"),
                self.with_name(self.type_syntax(value)?, "value"),
                self.with_name(
                    self.token_for_node(right_square, "rightSquare"),
                    "rightSquare",
                ),
            ],
        ))
    }

    fn pack_element_type(&self, node: Node<'a>) -> Result<Value> {
        let pack = self
            .first_type_child(node)
            .context("pack element type is missing pack type")?;
        Ok(self.syntax_node(
            "PackElementTypeSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.keyword_token_before_child(node, pack, "each")?,
                    "eachKeyword",
                ),
                self.with_name(self.type_syntax(pack)?, "pack"),
            ],
        ))
    }

    fn pack_expansion_type(&self, node: Node<'a>) -> Result<Value> {
        let repetition_pattern = self
            .first_type_child(node)
            .context("pack expansion type is missing repetition pattern")?;
        Ok(self.syntax_node(
            "PackExpansionTypeSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.keyword_token_before_child(node, repetition_pattern, "repeat")?,
                    "repeatKeyword",
                ),
                self.with_name(self.type_syntax(repetition_pattern)?, "repetitionPattern"),
            ],
        ))
    }

    fn some_or_any_type(&self, node: Node<'a>) -> Result<Value> {
        let constraint = self
            .first_type_child(node)
            .context("some/any type is missing constraint")?;
        let keyword = match node.kind() {
            "opaque_type" => "some",
            _ => "any",
        };
        Ok(self.syntax_node(
            "SomeOrAnyTypeSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.keyword_token_before_child(node, constraint, keyword)?,
                    "someOrAnySpecifier",
                ),
                self.with_name(self.type_syntax(constraint)?, "constraint"),
            ],
        ))
    }

    fn suppressed_type(&self, node: Node<'a>) -> Result<Value> {
        let suppressed_type = self
            .field_child(node, "suppressed")
            .or_else(|| self.first_type_child(node))
            .context("suppressed type is missing underlying type")?;
        let tilde_start = self.source[node.start_byte()..suppressed_type.start_byte()]
            .find('~')
            .map(|offset| node.start_byte() + offset)
            .context("suppressed type is missing '~'")?;
        Ok(self.syntax_node(
            "SuppressedTypeSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.token_with_range(
                        "prefixOperator(\"~\")",
                        self.range_from_offsets(tilde_start, tilde_start + 1),
                    ),
                    "withoutTilde",
                ),
                self.with_name(self.type_syntax(suppressed_type)?, "type"),
            ],
        ))
    }

    fn metatype_type(&self, node: Node<'a>) -> Result<Value> {
        let base_type = self
            .first_type_child(node)
            .context("metatype is missing base type")?;
        let period_start = self.source[base_type.end_byte()..node.end_byte()]
            .find('.')
            .map(|offset| base_type.end_byte() + offset)
            .context("metatype is missing '.'")?;
        let (specifier_start, specifier_end) = self.trim_offsets(period_start + 1, node.end_byte());
        if specifier_start >= specifier_end {
            bail!("metatype is missing specifier");
        }
        let specifier = &self.source[specifier_start..specifier_end];
        Ok(self.syntax_node(
            "MetatypeTypeSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.type_syntax(base_type)?, "baseType"),
                self.with_name(
                    self.token_with_range(
                        "period",
                        self.range_from_offsets(period_start, period_start + 1),
                    ),
                    "period",
                ),
                self.with_name(
                    self.token_with_range(
                        &format!("keyword(SwiftSyntax.Keyword.{specifier})"),
                        self.range_from_offsets(specifier_start, specifier_end),
                    ),
                    "metatypeSpecifier",
                ),
            ],
        ))
    }

    fn first_type_child(&self, node: Node<'a>) -> Option<Node<'a>> {
        named_children(node).find(|child| is_type_syntax_node_kind(child.kind()))
    }

    fn keyword_token_before_child(
        &self,
        node: Node<'a>,
        child: Node<'a>,
        keyword: &'static str,
    ) -> Result<Value> {
        let (_, start, end) = self
            .keyword_between(node.start_byte(), child.start_byte(), &[keyword])
            .with_context(|| format!("{} is missing '{keyword}'", node.kind()))?;
        Ok(self.token_with_range(
            &format!("keyword(SwiftSyntax.Keyword.{keyword})"),
            self.range_from_offsets(start, end),
        ))
    }

    fn optional_type(&self, node: Node<'a>) -> Result<Value> {
        let wrapped_type = named_children(node)
            .next()
            .context("optional type is missing wrapped type")?;
        let marker = children(node).find(|child| matches!(self.text(*child), "?" | "!"));
        let marker_text = marker.map(|marker| self.text(marker)).or_else(|| {
            self.text(node)
                .trim_end()
                .chars()
                .last()
                .filter(|ch| matches!(ch, '?' | '!'))
                .map(|ch| if ch == '?' { "?" } else { "!" })
        });
        let marker_text = marker_text.context("optional type is missing marker")?;
        let (node_type, marker_name, token_kind) = if marker_text == "!" {
            (
                "ImplicitlyUnwrappedOptionalTypeSyntax",
                "exclamationMark",
                "exclamationMark",
            )
        } else {
            ("OptionalTypeSyntax", "questionMark", "postfixQuestionMark")
        };
        let marker_token = marker.map_or_else(
            || {
                let marker_end = node.end_byte();
                self.token_with_range(
                    token_kind,
                    self.range_from_offsets(marker_end.saturating_sub(1), marker_end),
                )
            },
            |marker| self.token_for_node(marker, token_kind),
        );
        Ok(self.syntax_node(
            node_type,
            self.range_for_node(node),
            vec![
                self.with_name(self.type_syntax(wrapped_type)?, "wrappedType"),
                self.with_name(marker_token, marker_name),
            ],
        ))
    }

    fn function_type(&self, node: Node<'a>) -> Result<Value> {
        let parameters = self
            .function_type_parameters_node(node)
            .context("function type is missing parameters")?;
        let left_paren = self
            .immediate_child_kind(parameters, "(")
            .context("function type parameters are missing '('")?;
        let right_paren = self
            .immediate_child_kind(parameters, ")")
            .context("function type parameters are missing ')'")?;
        let return_clause = self
            .return_clause(node)?
            .context("function type is missing return clause")?;

        let mut children = vec![
            self.with_name(self.token_for_node(left_paren, "leftParen"), "leftParen"),
            self.with_name(
                self.function_type_parameter_list(parameters, left_paren, right_paren)?,
                "parameters",
            ),
            self.with_name(self.token_for_node(right_paren, "rightParen"), "rightParen"),
        ];
        if let Some(effect_specifiers) = self.type_effect_specifiers(node, right_paren)? {
            children.push(self.with_name(effect_specifiers, "effectSpecifiers"));
        }
        children.push(self.with_name(return_clause, "returnClause"));

        Ok(self.syntax_node("FunctionTypeSyntax", self.range_for_node(node), children))
    }

    fn function_type_parameters_node(&self, node: Node<'a>) -> Option<Node<'a>> {
        named_children(node).find(|child| {
            matches!(
                child.kind(),
                "tuple_type" | "lambda_function_type_parameters"
            )
        })
    }

    fn recovered_function_type_from_modifiers(
        &self,
        modifiers: Node<'a>,
        base: Node<'a>,
    ) -> Result<Option<Value>> {
        if base.kind() != "function_type" || self.function_type_parameters_node(base).is_some() {
            return Ok(None);
        }

        let Some(left_paren_start) = self.source[modifiers.start_byte()..base.start_byte()]
            .rfind('(')
            .map(|offset| modifiers.start_byte() + offset)
        else {
            return Ok(None);
        };
        let Some(right_paren_start) = self.source[left_paren_start..base.start_byte()]
            .rfind(')')
            .map(|offset| left_paren_start + offset)
        else {
            return Ok(None);
        };
        if right_paren_start <= left_paren_start {
            return Ok(None);
        }

        let arrow = self
            .immediate_child_kind(base, "->")
            .context("function type is missing '->'")?;
        let return_clause = self
            .return_clause(base)?
            .context("function type is missing return clause")?;

        let mut children = vec![
            self.with_name(
                self.token_with_range(
                    "leftParen",
                    self.range_from_offsets(left_paren_start, left_paren_start + 1),
                ),
                "leftParen",
            ),
            self.with_name(
                self.synthetic_tuple_type_element_list_from_offsets(
                    left_paren_start + 1,
                    right_paren_start,
                ),
                "parameters",
            ),
            self.with_name(
                self.token_with_range(
                    "rightParen",
                    self.range_from_offsets(right_paren_start, right_paren_start + 1),
                ),
                "rightParen",
            ),
        ];
        if let Some(effect_specifiers) =
            self.synthetic_type_effect_specifiers_between(right_paren_start + 1, arrow.start_byte())
        {
            children.push(self.with_name(effect_specifiers, "effectSpecifiers"));
        }
        children.push(self.with_name(return_clause, "returnClause"));

        let end = end_offset(children.last().expect("function type has a return clause"));
        let function_type = self.syntax_node(
            "FunctionTypeSyntax",
            self.range_from_offsets(left_paren_start, end),
            children,
        );
        Ok(Some(self.syntax_node(
            "AttributedTypeSyntax",
            self.range_from_offsets(modifiers.start_byte(), end),
            vec![
                self.with_name(
                    self.empty_collection("TypeSpecifierListSyntax", modifiers.start_byte()),
                    "specifiers",
                ),
                self.with_name(self.type_attribute_list(modifiers)?, "attributes"),
                self.with_name(
                    self.empty_collection("TypeSpecifierListSyntax", left_paren_start),
                    "lateSpecifiers",
                ),
                self.with_name(function_type, "baseType"),
            ],
        )))
    }

    fn type_attribute_list(&self, node: Node<'a>) -> Result<Value> {
        let mut attributes = Vec::new();
        for attribute in named_children(node).filter(|child| child.kind() == "attribute") {
            attributes.push(self.with_name(self.attribute(attribute)?, ""));
        }
        let range = self.covering_range_or_point(&attributes, node.start_byte());
        Ok(self.syntax_node("AttributeListSyntax", range, attributes))
    }

    fn function_type_parameter_list(
        &self,
        node: Node<'a>,
        left_paren: Node<'a>,
        right_paren: Node<'a>,
    ) -> Result<Value> {
        if node.kind() == "tuple_type" {
            return self.tuple_type_element_list(node, left_paren, right_paren);
        }

        let mut elements = Vec::new();
        for parameter in named_children(node).filter(|child| child.kind() == "lambda_parameter") {
            let trailing_comma = self.trailing_delimiter(node, parameter, ",");
            let mut item_children = Vec::new();

            if let Some(colon) = self.immediate_child_kind(parameter, ":") {
                let (first_name, second_name) = self.lambda_parameter_names(parameter)?;
                item_children.push(
                    self.with_name(self.identifier_or_wildcard_token(first_name), "firstName"),
                );
                if let Some(second_name) = second_name {
                    item_children.push(
                        self.with_name(
                            self.identifier_or_wildcard_token(second_name),
                            "secondName",
                        ),
                    );
                }
                item_children.push(self.with_name(self.token_for_node(colon, "colon"), "colon"));
            }

            let type_node = self
                .lambda_parameter_type(parameter)
                .map(|type_node| self.type_syntax(type_node))
                .unwrap_or_else(|| {
                    let name = self.lambda_parameter_name(parameter)?;
                    Ok(self.identifier_type_from_offsets(name.start_byte(), name.end_byte()))
                })?;
            item_children.push(self.with_name(type_node, "type"));

            if let Some(comma) = trailing_comma {
                item_children
                    .push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
            }
            let element_end = trailing_comma.map_or(parameter.end_byte(), |comma| comma.end_byte());
            elements.push(self.with_name(
                self.syntax_node(
                    "TupleTypeElementSyntax",
                    self.range_from_offsets(parameter.start_byte(), element_end),
                    item_children,
                ),
                "",
            ));
        }

        Ok(self.syntax_node(
            "TupleTypeElementListSyntax",
            self.range_from_offsets(left_paren.end_byte(), right_paren.start_byte()),
            elements,
        ))
    }

    fn synthetic_tuple_type_element_list_from_offsets(&self, start: usize, end: usize) -> Value {
        let mut elements = Vec::new();
        let mut element_start = start;
        let mut cursor = start;
        while cursor <= end {
            let is_comma = cursor < end && self.source.as_bytes()[cursor] == b',';
            if cursor == end || is_comma {
                let (trimmed_start, trimmed_end) = self.trim_offsets(element_start, cursor);
                if trimmed_start < trimmed_end {
                    let mut item_children = vec![self.with_name(
                        self.identifier_type_from_offsets(trimmed_start, trimmed_end),
                        "type",
                    )];
                    let element_end = if is_comma {
                        item_children.push(self.with_name(
                            self.token_with_range(
                                "comma",
                                self.range_from_offsets(cursor, cursor + 1),
                            ),
                            "trailingComma",
                        ));
                        cursor + 1
                    } else {
                        trimmed_end
                    };
                    elements.push(self.with_name(
                        self.syntax_node(
                            "TupleTypeElementSyntax",
                            self.range_from_offsets(trimmed_start, element_end),
                            item_children,
                        ),
                        "",
                    ));
                }
                element_start = cursor.saturating_add(1);
            }
            cursor += 1;
        }

        self.syntax_node(
            "TupleTypeElementListSyntax",
            self.range_from_offsets(start, end),
            elements,
        )
    }

    fn synthetic_type_effect_specifiers_between(&self, start: usize, end: usize) -> Option<Value> {
        let async_specifier = self.keyword_between(start, end, &["async", "reasync"]);
        let throws_specifier = self.keyword_between(start, end, &["throws", "rethrows"]);
        if async_specifier.is_none() && throws_specifier.is_none() {
            return None;
        }

        let mut children = Vec::new();
        if let Some((keyword, keyword_start, keyword_end)) = async_specifier {
            children.push(self.with_name(
                self.token_with_range(
                    &format!("keyword(SwiftSyntax.Keyword.{keyword})"),
                    self.range_from_offsets(keyword_start, keyword_end),
                ),
                "asyncSpecifier",
            ));
        }
        if let Some((keyword, keyword_start, keyword_end)) = throws_specifier {
            children.push(self.with_name(
                self.syntax_node(
                    "ThrowsClauseSyntax",
                    self.range_from_offsets(keyword_start, keyword_end),
                    vec![self.with_name(
                        self.token_with_range(
                            &format!("keyword(SwiftSyntax.Keyword.{keyword})"),
                            self.range_from_offsets(keyword_start, keyword_end),
                        ),
                        "throwsSpecifier",
                    )],
                ),
                "throwsClause",
            ));
        }
        let range = self.covering_range_or_point(&children, start);
        Some(self.syntax_node("TypeEffectSpecifiersSyntax", range, children))
    }

    fn keyword_between(
        &self,
        start: usize,
        end: usize,
        keywords: &[&'static str],
    ) -> Option<(&'static str, usize, usize)> {
        keywords.iter().find_map(|keyword| {
            let mut search_start = start;
            while search_start <= end {
                let relative = self.source[search_start..end].find(keyword)?;
                let keyword_start = search_start + relative;
                let keyword_end = keyword_start + keyword.len();
                if self.is_keyword_boundary(keyword_start, keyword_end) {
                    return Some((*keyword, keyword_start, keyword_end));
                }
                search_start = keyword_end;
            }
            None
        })
    }

    fn is_keyword_boundary(&self, start: usize, end: usize) -> bool {
        let before = start
            .checked_sub(1)
            .and_then(|offset| self.source.as_bytes().get(offset))
            .copied();
        let after = self.source.as_bytes().get(end).copied();
        before.is_none_or(|byte| !is_identifier_byte(byte))
            && after.is_none_or(|byte| !is_identifier_byte(byte))
    }

    fn type_effect_specifiers(
        &self,
        node: Node<'a>,
        after_parameter_clause: Node<'a>,
    ) -> Result<Option<Value>> {
        let arrow = self
            .immediate_child_kind(node, "->")
            .context("function type is missing '->'")?;
        let async_specifier = children(node).find(|child| {
            matches!(child.kind(), "async" | "reasync")
                && child.start_byte() > after_parameter_clause.end_byte()
                && child.end_byte() <= arrow.start_byte()
        });
        let throws_specifier = children(node).find(|child| {
            matches!(child.kind(), "throws" | "rethrows")
                && child.start_byte() > after_parameter_clause.end_byte()
                && child.end_byte() <= arrow.start_byte()
        });
        if async_specifier.is_none() && throws_specifier.is_none() {
            return Ok(None);
        }

        let mut children = Vec::new();
        if let Some(async_specifier) = async_specifier {
            children.push(self.with_name(
                self.token_for_node(
                    async_specifier,
                    &format!("keyword(SwiftSyntax.Keyword.{})", async_specifier.kind()),
                ),
                "asyncSpecifier",
            ));
        }
        if let Some(throws_specifier) = throws_specifier {
            let throws_clause = self.syntax_node(
                "ThrowsClauseSyntax",
                self.range_for_node(throws_specifier),
                vec![self.with_name(
                    self.token_for_node(
                        throws_specifier,
                        &format!("keyword(SwiftSyntax.Keyword.{})", throws_specifier.kind()),
                    ),
                    "throwsSpecifier",
                )],
            );
            children.push(self.with_name(throws_clause, "throwsClause"));
        }
        let range = self.covering_range_or_point(&children, after_parameter_clause.end_byte());
        Ok(Some(self.syntax_node(
            "TypeEffectSpecifiersSyntax",
            range,
            children,
        )))
    }

    fn tuple_type(&self, node: Node<'a>) -> Result<Value> {
        let left_paren = self
            .immediate_child_kind(node, "(")
            .context("tuple type is missing '('")?;
        let right_paren = self
            .immediate_child_kind(node, ")")
            .context("tuple type is missing ')'")?;

        Ok(self.syntax_node(
            "TupleTypeSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.token_for_node(left_paren, "leftParen"), "leftParen"),
                self.with_name(
                    self.tuple_type_element_list(node, left_paren, right_paren)?,
                    "elements",
                ),
                self.with_name(self.token_for_node(right_paren, "rightParen"), "rightParen"),
            ],
        ))
    }

    fn tuple_type_element_list(
        &self,
        node: Node<'a>,
        left_paren: Node<'a>,
        right_paren: Node<'a>,
    ) -> Result<Value> {
        let mut elements = Vec::new();
        for item in named_children(node).filter(|child| child.kind() == "tuple_type_item") {
            let trailing_comma = self.trailing_delimiter(node, item, ",");
            let mut item_children = Vec::new();
            if let Some(colon) = self.immediate_child_kind(item, ":") {
                let names = named_children(item)
                    .filter(|child| {
                        child.end_byte() <= colon.start_byte()
                            && matches!(
                                child.kind(),
                                "simple_identifier" | "identifier" | "wildcard_pattern"
                            )
                    })
                    .collect::<Vec<_>>();
                if let Some(first_name) = names.first().copied() {
                    item_children.push(
                        self.with_name(self.identifier_or_wildcard_token(first_name), "firstName"),
                    );
                }
                if let Some(second_name) = names.get(1).copied() {
                    item_children.push(
                        self.with_name(
                            self.identifier_or_wildcard_token(second_name),
                            "secondName",
                        ),
                    );
                }
                item_children.push(self.with_name(self.token_for_node(colon, "colon"), "colon"));
            }
            let type_node = named_children(item)
                .filter(|child| {
                    !matches!(
                        child.kind(),
                        "simple_identifier" | "identifier" | "wildcard_pattern"
                    )
                })
                .find(|child| child.kind() != "type_annotation")
                .or_else(|| named_children(item).last())
                .context("tuple type item is missing a type")?;
            item_children.push(self.with_name(self.type_syntax(type_node)?, "type"));
            if let Some(comma) = trailing_comma {
                item_children
                    .push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
            }
            let element_end = trailing_comma.map_or(item.end_byte(), |comma| comma.end_byte());
            elements.push(self.with_name(
                self.syntax_node(
                    "TupleTypeElementSyntax",
                    self.range_from_offsets(item.start_byte(), element_end),
                    item_children,
                ),
                "",
            ));
        }

        Ok(self.syntax_node(
            "TupleTypeElementListSyntax",
            self.range_from_offsets(left_paren.end_byte(), right_paren.start_byte()),
            elements,
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
        self.initializer_clause_with_end(equal, value, value.end_byte())
    }

    fn initializer_clause_with_end(
        &self,
        equal: Node<'a>,
        value: Node<'a>,
        end: usize,
    ) -> Result<Value> {
        let value_expr = if let Some(question_mark) = self.optional_chain_question_after(value, end)
        {
            let expression = self.expr(value)?;
            self.optional_chaining_expr_from_value(
                expression,
                question_mark,
                value.start_byte(),
                question_mark.end_byte(),
            )
        } else {
            self.expr(value)?
        };
        Ok(self.syntax_node(
            "InitializerClauseSyntax",
            self.range_from_offsets(equal.start_byte(), end),
            vec![
                self.with_name(self.token_for_node(equal, "equal"), "equal"),
                self.with_name(value_expr, "value"),
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
                Some(self.type_syntax(return_type)?)
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
        let direct_statement_nodes = named_children(node).collect::<Vec<_>>();
        if direct_statement_nodes
            .iter()
            .copied()
            .any(|child| self.is_recoverable_do_error(child))
        {
            return self.code_block_from_statement_nodes(
                &direct_statement_nodes,
                left_brace,
                right_brace,
            );
        }
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
        self.code_block_from_statement_nodes(&statement_nodes, left_brace, right_brace)
    }

    fn code_block_from_statement_nodes(
        &self,
        statement_nodes: &[Node<'a>],
        left_brace: Node<'a>,
        right_brace: Node<'a>,
    ) -> Result<Value> {
        let mut items = Vec::new();
        self.push_code_block_items_from_nodes(statement_nodes, &mut items)?;
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
        self.code_block_item_list_from_nodes(&statement_nodes, fallback_offset)
    }

    fn code_block_item_list_from_nodes(
        &self,
        statement_nodes: &[Node<'a>],
        fallback_offset: usize,
    ) -> Result<Value> {
        let mut items = Vec::new();
        self.push_code_block_items_from_nodes(statement_nodes, &mut items)?;
        let range = self.covering_range_or_point(&items, fallback_offset);
        Ok(self.syntax_node("CodeBlockItemListSyntax", range, items))
    }

    fn push_code_block_items_from_nodes(
        &self,
        statement_nodes: &[Node<'a>],
        items: &mut Vec<Value>,
    ) -> Result<()> {
        let mut index = 0;
        while index < statement_nodes.len() {
            let child = statement_nodes[index];
            if let Some((postfix_if_config, next_index)) =
                self.postfix_if_config_expr_from_nodes(statement_nodes, index)?
            {
                items.push(self.code_block_item_for_value(postfix_if_config, "item"));
                index = next_index;
                continue;
            }
            if self.is_if_config_start(child) {
                let (if_config, next_index) =
                    self.if_config_decl_from_code_block_nodes(statement_nodes, index)?;
                items.push(self.code_block_item_for_value(if_config, "item"));
                index = next_index;
                continue;
            }
            if is_trivia_node(child) || is_ignorable_directive(child) {
                index += 1;
                continue;
            }
            if self.is_recoverable_do_error(child) {
                let value = self.recovered_do_syntax_from_error(child)?;
                items.push(self.code_block_item_for_value(value, "item"));
                index = self.skip_do_cast_artifacts(statement_nodes, index + 1);
                continue;
            }
            if let Some(next) = statement_nodes.get(index + 1).copied() {
                if child.kind() == "statement_label" {
                    let labeled_stmt = self.labeled_stmt(child, next)?;
                    items.push(self.code_block_item_for_value(labeled_stmt, "item"));
                    index += 2;
                    continue;
                }
                if self.is_split_keyword_apply_call(child, next) {
                    let call = self.recovered_keyword_apply_call(child, next)?;
                    items.push(self.code_block_item_for_value(call, "item"));
                    index += 2;
                    continue;
                }
                if self.is_split_copy_variable_decl(child, next) {
                    let declaration = self.recovered_copy_variable_decl(child, next)?;
                    items.push(self.code_block_item_for_value(declaration, "item"));
                    index += 2;
                    continue;
                }
                if self.is_split_move_variable_decl(child, next) {
                    items.push(self.code_block_item(child)?);
                    index += 2;
                    continue;
                }
                if self.is_split_ownership_statement(child, next) {
                    items.push(self.code_block_item(child)?);
                    index += 2;
                    continue;
                }
                if self.is_split_bare_macro_variable_decl(child, next) {
                    let declaration = self.recovered_variable_decl(child, Some(next))?;
                    items.push(self.code_block_item_for_value(declaration, "item"));
                    index += 2;
                    continue;
                }
            }
            items.push(self.code_block_item(child)?);
            index += 1;
        }
        Ok(())
    }

    fn postfix_if_config_expr_from_nodes(
        &self,
        nodes: &[Node<'a>],
        base_index: usize,
    ) -> Result<Option<(Value, usize)>> {
        let Some(base) = nodes.get(base_index).copied() else {
            return Ok(None);
        };
        let Some(first_branch) = nodes.get(base_index + 1).copied() else {
            return Ok(None);
        };
        if !is_expression_like_node(base)
            || !self
                .postfix_directive_kind(first_branch)
                .is_some_and(|kind| kind == DirectiveKind::If)
        {
            return Ok(None);
        }

        let (config, next_index, trailing_call) =
            self.postfix_if_config_decl_from_nodes(nodes, base_index + 1)?;
        let config_end = end_offset(&config);
        let base_expr = self.expr(base)?;
        let postfix_if_config = self.syntax_node(
            "PostfixIfConfigExprSyntax",
            self.range_from_offsets(base.start_byte(), config_end.max(base.end_byte())),
            vec![
                self.with_name(base_expr, "base"),
                self.with_name(config, "config"),
            ],
        );

        if let Some(trailing_call) = trailing_call {
            return Ok(Some((
                self.postfix_directive_trailing_call_expr(
                    trailing_call,
                    postfix_if_config,
                    base.start_byte(),
                )?,
                next_index,
            )));
        }

        Ok(Some((postfix_if_config, next_index)))
    }

    fn postfix_if_config_decl_from_nodes(
        &self,
        nodes: &[Node<'a>],
        start_index: usize,
    ) -> Result<(Value, usize, Option<Node<'a>>)> {
        let mut clauses = Vec::new();
        let mut index = start_index;
        let pound_endif;
        let next_index;
        let trailing_call;
        loop {
            let branch_call = nodes[index];
            let parts = self
                .postfix_directive_call_parts(branch_call)
                .context("postfix if config clause is missing directive call")?;
            let (kind, _, _, _) = self
                .directive_keyword_info(parts.directive)
                .context("postfix if config clause is missing directive keyword")?;
            if !matches!(
                kind,
                DirectiveKind::If | DirectiveKind::ElseIf | DirectiveKind::Else
            ) {
                bail!("unexpected directive in postfix if config clause");
            }
            clauses.push(self.postfix_if_config_clause(parts.directive, branch_call)?);

            index += 1;
            let Some(next) = nodes.get(index).copied() else {
                bail!("postfix if config declaration is missing #endif");
            };
            if let Some(next_parts) = self.postfix_directive_call_parts(next) {
                match self.directive_keyword_info(next_parts.directive) {
                    Some((DirectiveKind::ElseIf | DirectiveKind::Else, _, _, _)) => continue,
                    Some((DirectiveKind::EndIf, _, _, _)) => {
                        pound_endif = next_parts.directive;
                        next_index = index + 1;
                        trailing_call = Some(next);
                        break;
                    }
                    _ => bail!("unexpected directive call in postfix if config declaration"),
                }
            }

            match self.directive_keyword_info(next) {
                Some((DirectiveKind::EndIf, _, _, _)) => {
                    pound_endif = next;
                    next_index = index + 1;
                    trailing_call = None;
                    break;
                }
                _ => bail!("unexpected node while parsing postfix if config declaration"),
            }
        }

        let clauses_range = self.covering_range_or_point(&clauses, nodes[start_index].end_byte());
        let clause_list = self.syntax_node("IfConfigClauseListSyntax", clauses_range, clauses);
        let (_, endif_start, endif_end, endif_kind) = self
            .directive_keyword_info(pound_endif)
            .context("postfix if config declaration is missing #endif")?;
        Ok((
            self.syntax_node(
                "IfConfigDeclSyntax",
                self.range_from_offsets(nodes[start_index].start_byte(), pound_endif.end_byte()),
                vec![
                    self.with_name(clause_list, "clauses"),
                    self.with_name(
                        self.token_with_range(
                            endif_kind,
                            self.range_from_offsets(endif_start, endif_end),
                        ),
                        "poundEndif",
                    ),
                ],
            ),
            next_index,
            trailing_call,
        ))
    }

    fn postfix_if_config_clause(
        &self,
        directive: Node<'a>,
        branch_call: Node<'a>,
    ) -> Result<Value> {
        let (kind, keyword_start, keyword_end, token_kind) = self
            .directive_keyword_info(directive)
            .context("postfix if config clause is missing directive keyword")?;
        let mut children = vec![self.with_name(
            self.token_with_range(
                token_kind,
                self.range_from_offsets(keyword_start, keyword_end),
            ),
            "poundKeyword",
        )];
        if matches!(kind, DirectiveKind::If | DirectiveKind::ElseIf) {
            if let Some(condition) = named_children(directive).next() {
                children.push(self.with_name(self.expr(condition)?, "condition"));
            }
        }

        let elements = self.postfix_directive_branch_call_expr(branch_call)?;
        let clause_end = end_offset(&elements).max(directive.end_byte());
        children.push(self.with_name(elements, "elements"));
        Ok(self.syntax_node(
            "IfConfigClauseSyntax",
            self.range_from_offsets(directive.start_byte(), clause_end),
            children,
        ))
    }

    fn postfix_directive_kind(&self, node: Node<'a>) -> Option<DirectiveKind> {
        let parts = self.postfix_directive_call_parts(node)?;
        self.directive_keyword_info(parts.directive)
            .map(|(kind, _, _, _)| kind)
    }

    fn postfix_directive_call_parts(
        &self,
        node: Node<'a>,
    ) -> Option<PostfixDirectiveCallParts<'a>> {
        if node.kind() != "call_expression" {
            return None;
        }
        let navigation = named_children(node).find(|child| child.kind() != "call_suffix")?;
        if navigation.kind() != "navigation_expression" {
            return None;
        }
        let directive = self.field_child(navigation, "target")?;
        if directive.kind() != "directive" {
            return None;
        }
        let suffix_node = self.field_child(navigation, "suffix")?;
        let period = self.immediate_child_kind(suffix_node, ".")?;
        Some(PostfixDirectiveCallParts {
            directive,
            navigation,
            period,
        })
    }

    fn postfix_directive_branch_call_expr(&self, node: Node<'a>) -> Result<Value> {
        let parts = self
            .postfix_directive_call_parts(node)
            .context("postfix branch is missing directive navigation")?;
        let member_access =
            self.directive_member_access_expr(parts.navigation, None, parts.period.start_byte())?;
        self.function_call_expr_from_called_expression(
            node,
            member_access,
            parts.period.start_byte(),
        )
    }

    fn postfix_directive_trailing_call_expr(
        &self,
        node: Node<'a>,
        base: Value,
        range_start: usize,
    ) -> Result<Value> {
        let parts = self
            .postfix_directive_call_parts(node)
            .context("postfix trailing call is missing directive navigation")?;
        let member_access =
            self.directive_member_access_expr(parts.navigation, Some(base), range_start)?;
        self.function_call_expr_from_called_expression(node, member_access, range_start)
    }

    fn directive_member_access_expr(
        &self,
        navigation: Node<'a>,
        base: Option<Value>,
        range_start: usize,
    ) -> Result<Value> {
        let suffix_node = self
            .field_child(navigation, "suffix")
            .context("directive member access is missing suffix")?;
        let suffix = self
            .field_child(suffix_node, "suffix")
            .or_else(|| named_children(suffix_node).next())
            .context("directive member access suffix is missing a name")?;
        let period = self
            .immediate_child_kind(suffix_node, ".")
            .context("directive member access is missing '.'")?;

        let mut children = Vec::new();
        if let Some(base) = base {
            children.push(self.with_name(base, "base"));
        }
        children.push(self.with_name(self.token_for_node(period, "period"), "period"));
        children.push(self.with_name(self.decl_reference_expr(suffix), "declName"));

        Ok(self.syntax_node(
            "MemberAccessExprSyntax",
            self.range_from_offsets(range_start, navigation.end_byte()),
            children,
        ))
    }

    fn function_call_expr_from_called_expression(
        &self,
        node: Node<'a>,
        called_expression: Value,
        range_start: usize,
    ) -> Result<Value> {
        let suffix = self
            .immediate_named_child_kind(node, "call_suffix")
            .context("directive call expression is missing call suffix")?;
        let mut children = vec![self.with_name(called_expression, "calledExpression")];

        if let Some(value_arguments) = self.immediate_named_child_kind(suffix, "value_arguments") {
            let left_paren = self
                .immediate_child_kind(value_arguments, "(")
                .context("directive call arguments are missing '('")?;
            let right_paren = self
                .immediate_child_kind(value_arguments, ")")
                .context("directive call arguments are missing ')'")?;
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
            self.range_from_offsets(range_start, node.end_byte()),
            children,
        ))
    }

    fn if_config_decl_from_code_block_nodes(
        &self,
        nodes: &[Node<'a>],
        start_index: usize,
    ) -> Result<(Value, usize)> {
        let start = nodes[start_index];
        if !self.is_if_config_start(start) {
            bail!("if config declaration must start with #if");
        }

        let mut clauses = Vec::new();
        let mut index = start_index;
        let pound_endif;
        let next_index;
        loop {
            let directive = nodes[index];
            let (kind, _, _, _) = self
                .directive_keyword_info(directive)
                .context("if config clause is missing directive keyword")?;
            if !matches!(
                kind,
                DirectiveKind::If | DirectiveKind::ElseIf | DirectiveKind::Else
            ) {
                bail!("unexpected directive in if config clause");
            }

            let body_start = index + 1;
            let mut body_end = body_start;
            let mut nested_depth = 0usize;
            while body_end < nodes.len() {
                if let Some((kind, _, _, _)) = self.directive_keyword_info(nodes[body_end]) {
                    match kind {
                        DirectiveKind::If => nested_depth += 1,
                        DirectiveKind::ElseIf | DirectiveKind::Else if nested_depth == 0 => break,
                        DirectiveKind::EndIf if nested_depth == 0 => break,
                        DirectiveKind::EndIf => nested_depth = nested_depth.saturating_sub(1),
                        _ => {}
                    }
                }
                body_end += 1;
            }

            clauses.push(self.if_config_clause(directive, &nodes[body_start..body_end])?);

            if body_end >= nodes.len() {
                bail!("if config declaration is missing #endif");
            }
            match self.directive_keyword_info(nodes[body_end]) {
                Some((DirectiveKind::ElseIf | DirectiveKind::Else, _, _, _)) => {
                    index = body_end;
                }
                Some((DirectiveKind::EndIf, _, _, _)) => {
                    pound_endif = nodes[body_end];
                    next_index = body_end + 1;
                    break;
                }
                _ => bail!("unexpected node while parsing if config declaration"),
            }
        }

        let clauses_range = self.covering_range_or_point(&clauses, start.end_byte());
        let clause_list = self.syntax_node("IfConfigClauseListSyntax", clauses_range, clauses);
        let (_, endif_start, endif_end, endif_kind) = self
            .directive_keyword_info(pound_endif)
            .context("if config declaration is missing #endif")?;
        Ok((
            self.syntax_node(
                "IfConfigDeclSyntax",
                self.range_from_offsets(start.start_byte(), pound_endif.end_byte()),
                vec![
                    self.with_name(clause_list, "clauses"),
                    self.with_name(
                        self.token_with_range(
                            endif_kind,
                            self.range_from_offsets(endif_start, endif_end),
                        ),
                        "poundEndif",
                    ),
                ],
            ),
            next_index,
        ))
    }

    fn if_config_clause(&self, directive: Node<'a>, body: &[Node<'a>]) -> Result<Value> {
        let (kind, keyword_start, keyword_end, token_kind) = self
            .directive_keyword_info(directive)
            .context("if config clause is missing directive keyword")?;
        let mut children = vec![self.with_name(
            self.token_with_range(
                token_kind,
                self.range_from_offsets(keyword_start, keyword_end),
            ),
            "poundKeyword",
        )];
        if matches!(kind, DirectiveKind::If | DirectiveKind::ElseIf) {
            if let Some(condition) = named_children(directive).next() {
                children.push(self.with_name(self.expr(condition)?, "condition"));
            }
        }

        let elements = self.code_block_item_list_from_nodes(body, directive.end_byte())?;
        let clause_end = end_offset(&elements).max(directive.end_byte());
        children.push(self.with_name(elements, "elements"));
        Ok(self.syntax_node(
            "IfConfigClauseSyntax",
            self.range_from_offsets(directive.start_byte(), clause_end),
            children,
        ))
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

        let mut accessor_nodes: Vec<_> = named_children(computed_property)
            .filter(|child| {
                matches!(
                    child.kind(),
                    "computed_getter" | "computed_setter" | "computed_modify"
                )
            })
            .collect();
        if accessor_nodes.is_empty() {
            if let Some(statements) =
                self.immediate_named_child_kind(computed_property, "statements")
            {
                accessor_nodes = named_children(statements)
                    .filter(|child| self.is_recovered_accessor_call(*child))
                    .collect();
            }
        }
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

    fn is_recovered_accessor_call(&self, node: Node<'a>) -> bool {
        if node.kind() != "call_expression" {
            return false;
        }
        named_children(node).next().is_some_and(|callee| {
            callee.kind() == "simple_identifier"
                && matches!(self.text(callee), "_read" | "read" | "_modify" | "modify")
                && self.first_descendant_kind(node, "lambda_literal").is_some()
        })
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
        } else if let Some(body) = self.first_descendant_kind(node, "lambda_literal") {
            children.push(self.with_name(self.code_block(body)?, "body"));
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
        if self.is_key_path_expr_tree(node) {
            return self.key_path_expr(node);
        }
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
            "as_expression" => self.as_expr(node),
            "await_expression" => self.await_expr(node),
            "check_expression" => self.is_expr(node),
            "nil_coalescing_expression" => self.nil_coalescing_expr(node),
            "additive_expression"
            | "comparison_expression"
            | "conjunction_expression"
            | "disjunction_expression"
            | "equality_expression"
            | "infix_expression"
            | "multiplicative_expression"
            | "range_expression" => self.binary_operator_expr(node),
            "array_literal" => self.array_expr(node),
            "call_expression" if self.is_do_expr_call(node) => self.do_expr(node),
            "call_expression" => self.function_call_expr(node),
            "constructor_expression" => self.constructor_expr(node),
            "consume_expression" => self.consume_expr(node),
            "dictionary_literal" => self.dictionary_expr(node),
            "key_path_expression" => self.key_path_expr(node),
            "lambda_literal" => self.closure_expr(node),
            "navigation_expression" if self.is_recoverable_prefix_slash_navigation(node) => self
                .synthetic_prefix_slash_expr_from_offsets(node.start_byte(), node.end_byte())
                .context("recoverable prefix slash expression is missing operand"),
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
            "/" => Ok(self.decl_reference_expr(node)),
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
            "multi_line_string_literal" => self.string_literal(node),
            "raw_string_literal" => self.raw_string_literal(node),
            "macro_invocation" if self.is_regex_literal_text(self.text(node)) => {
                self.regex_literal_expr_from_offsets(node.start_byte(), node.end_byte())
            }
            "macro_invocation" => self.macro_expansion_expr(node),
            "special_literal" => self.special_literal_expr(node),
            "regex_literal" => {
                if let Some(parent) = node.parent() {
                    if let Some(recovered) =
                        self.recovered_escaped_raw_string_literal(parent, node)?
                    {
                        return Ok(recovered);
                    }
                }
                self.regex_literal_expr(node)
            }
            "tuple_expression" => self.tuple_expr(node),
            "ternary_expression" => self.ternary_expr(node),
            "try_expression" => self.try_expr(node),
            "user_type" => self.constructed_type_expr(node),
            "value_pack_expansion" => self.pack_expansion_expr(node),
            "value_parameter_pack" => self.pack_element_expr(node),
            "ERROR" if self.is_recoverable_array_expr_error(node) => {
                self.recovered_array_expr(node)
            }
            "ERROR" if self.is_bare_macro_error(node) => self.macro_expansion_expr(node),
            "ERROR" if is_identifier_like_text(self.text(node)) => {
                Ok(self.decl_reference_expr(node))
            }
            other => bail!("unsupported Swift expression node '{other}'"),
        }
    }

    fn pack_element_expr(&self, node: Node<'a>) -> Result<Value> {
        let pack = self
            .first_expression_child(node)
            .context("pack element expression is missing pack expression")?;
        Ok(self.syntax_node(
            "PackElementExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.keyword_token_before_child(node, pack, "each")?,
                    "eachKeyword",
                ),
                self.with_name(self.expr(pack)?, "pack"),
            ],
        ))
    }

    fn pack_expansion_expr(&self, node: Node<'a>) -> Result<Value> {
        let repetition_pattern = self
            .first_expression_child(node)
            .context("pack expansion expression is missing repetition pattern")?;
        Ok(self.syntax_node(
            "PackExpansionExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.keyword_token_before_child(node, repetition_pattern, "repeat")?,
                    "repeatKeyword",
                ),
                self.with_name(self.expr(repetition_pattern)?, "repetitionPattern"),
            ],
        ))
    }

    fn first_expression_child(&self, node: Node<'a>) -> Option<Node<'a>> {
        named_children(node).find(|child| is_expression_like_node(*child))
    }

    fn is_split_initializer_continuation(&self, node: Node<'a>) -> bool {
        (node.kind() == "call_expression" && self.text(node).trim_start().starts_with('='))
            || self.is_bare_macro_error(node)
    }

    fn key_path_expr(&self, node: Node<'a>) -> Result<Value> {
        let backslash_start = node.start_byte()
            + self
                .text(node)
                .find('\\')
                .context("key path expression is missing backslash")?;
        Ok(self.syntax_node(
            "KeyPathExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.token_with_range(
                        "backslash",
                        self.range_from_offsets(backslash_start, backslash_start + 1),
                    ),
                    "backslash",
                ),
                self.with_name(
                    self.empty_collection("KeyPathComponentListSyntax", node.end_byte()),
                    "components",
                ),
            ],
        ))
    }

    fn is_key_path_expr_tree(&self, node: Node<'a>) -> bool {
        matches!(
            node.kind(),
            "key_path_expression" | "navigation_expression" | "call_expression"
        ) && self.text(node).trim_start().starts_with('\\')
    }

    fn expr_for_split_initializer(&self, node: Node<'a>) -> Result<Value> {
        if node.kind() == "call_expression" && self.text(node).trim_start().starts_with('=') {
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
        let mut children = vec![self.with_name(
            self.called_expression_with_optional_chaining(node, callee, suffix)?,
            "calledExpression",
        )];

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
            if let Some(recovered_cases) = self.recovered_switch_case_slices(switch_entry) {
                for recovered_case in recovered_cases {
                    cases.push(self.with_name(
                        self.recovered_switch_case(switch_entry, recovered_case)?,
                        "",
                    ));
                }
            } else {
                cases.push(self.with_name(self.switch_case(switch_entry)?, ""));
            }
        }
        if cases.is_empty() {
            if let Some(recovered_cases) = self.recovered_switch_case_slices_between(
                left_brace.end_byte(),
                right_brace.start_byte(),
            ) {
                for recovered_case in recovered_cases {
                    cases.push(
                        self.with_name(self.recovered_switch_case(node, recovered_case)?, ""),
                    );
                }
            }
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
        let label = if let Some(default_keyword) = self
            .immediate_child_kind(node, "default")
            .or_else(|| self.immediate_child_kind(node, "default_keyword"))
        {
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
            let case_items = self.switch_case_items_from_offsets(
                node,
                case_keyword.end_byte(),
                colon.start_byte(),
            )?;
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

    fn switch_case_where_clause(
        &self,
        node: Node<'a>,
        start: usize,
        end: usize,
    ) -> Result<Option<Value>> {
        let Some(where_keyword) = children(node).find(|child| {
            child.kind() == "where_keyword"
                && child.start_byte() >= start
                && child.end_byte() <= end
        }) else {
            return Ok(None);
        };
        let condition = named_children(node)
            .find(|child| {
                child.start_byte() >= where_keyword.end_byte()
                    && child.end_byte() <= end
                    && is_expression_like_node(*child)
            })
            .context("switch case where clause is missing a condition")?;
        Ok(Some(self.syntax_node(
            "WhereClauseSyntax",
            self.range_from_offsets(where_keyword.start_byte(), condition.end_byte()),
            vec![
                self.with_name(
                    self.token_for_node(where_keyword, "keyword(SwiftSyntax.Keyword.where)"),
                    "whereKeyword",
                ),
                self.with_name(self.expr(condition)?, "condition"),
            ],
        )))
    }

    fn switch_case_items_from_offsets(
        &self,
        node: Node<'a>,
        start: usize,
        end: usize,
    ) -> Result<Vec<Value>> {
        let mut case_items = Vec::new();
        let mut item_start = start;
        for comma in self.top_level_commas(start, end) {
            if let Some(item) =
                self.switch_case_item_from_source_offsets(node, item_start, comma, Some(comma))?
            {
                case_items.push(self.with_name(item, ""));
            }
            item_start = comma + 1;
        }
        if let Some(item) =
            self.switch_case_item_from_source_offsets(node, item_start, end, None)?
        {
            case_items.push(self.with_name(item, ""));
        }
        Ok(case_items)
    }

    fn switch_case_item_from_source_offsets(
        &self,
        node: Node<'a>,
        start: usize,
        end: usize,
        comma: Option<usize>,
    ) -> Result<Option<Value>> {
        let (item_start, item_end) = self.trim_offsets(start, end);
        if item_start >= item_end {
            return Ok(None);
        }

        let where_start = self.top_level_where_keyword(item_start, item_end);
        let pattern_end = where_start.unwrap_or(item_end);
        let (pattern_start, pattern_end) = self.trim_offsets(item_start, pattern_end);
        if pattern_start >= pattern_end {
            return Ok(None);
        }

        let mut item_children = vec![self.with_name(
            self.synthetic_pattern_from_offsets(pattern_start, pattern_end),
            "pattern",
        )];
        if let Some(where_start) = where_start {
            let where_clause = self
                .switch_case_where_clause(node, pattern_end, item_end)?
                .unwrap_or_else(|| self.synthetic_where_clause_from_offsets(where_start, item_end));
            item_children.push(self.with_name(where_clause, "whereClause"));
        }
        if let Some(comma_start) = comma {
            item_children.push(self.with_name(
                self.token_with_range(
                    "comma",
                    self.range_from_offsets(comma_start, comma_start + 1),
                ),
                "trailingComma",
            ));
        }
        let item_range_end = comma.map_or_else(
            || item_children.last().map(end_offset).unwrap_or(pattern_end),
            |comma_start| comma_start + 1,
        );
        Ok(Some(self.syntax_node(
            "SwitchCaseItemSyntax",
            self.range_from_offsets(pattern_start, item_range_end),
            item_children,
        )))
    }

    fn synthetic_where_clause_from_offsets(&self, where_start: usize, end: usize) -> Value {
        let where_end = where_start + "where".len();
        let (condition_start, condition_end) = self.trim_offsets(where_end, end);
        self.syntax_node(
            "WhereClauseSyntax",
            self.range_from_offsets(where_start, condition_end),
            vec![
                self.with_name(
                    self.token_with_range(
                        "keyword(SwiftSyntax.Keyword.where)",
                        self.range_from_offsets(where_start, where_end),
                    ),
                    "whereKeyword",
                ),
                self.with_name(
                    self.synthetic_expr_from_offsets(condition_start, condition_end),
                    "condition",
                ),
            ],
        )
    }

    fn top_level_where_keyword(&self, start: usize, end: usize) -> Option<usize> {
        let bytes = self.source.as_bytes();
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut angle_depth = 0usize;
        let mut offset = start;
        while offset < end {
            match bytes[offset] {
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                b'[' => bracket_depth += 1,
                b']' => bracket_depth = bracket_depth.saturating_sub(1),
                b'<' if paren_depth == 0 && bracket_depth == 0 => angle_depth += 1,
                b'>' if paren_depth == 0 && bracket_depth == 0 => {
                    angle_depth = angle_depth.saturating_sub(1)
                }
                b'w' if paren_depth == 0
                    && bracket_depth == 0
                    && angle_depth == 0
                    && self.source[offset..end].starts_with("where")
                    && self.source.as_bytes()[start..offset]
                        .last()
                        .is_some_and(|byte| byte.is_ascii_whitespace())
                    && self
                        .source
                        .as_bytes()
                        .get(offset + "where".len())
                        .is_some_and(|byte| byte.is_ascii_whitespace()) =>
                {
                    return Some(offset);
                }
                _ => {}
            }
            offset += 1;
        }
        None
    }

    fn recovered_switch_case_slices(
        &self,
        node: Node<'a>,
    ) -> Option<Vec<RecoveredSwitchCaseSlice>> {
        self.recovered_switch_case_slices_between(node.start_byte(), node.end_byte())
    }

    fn recovered_switch_case_slices_between(
        &self,
        start: usize,
        end: usize,
    ) -> Option<Vec<RecoveredSwitchCaseSlice>> {
        let mut label_starts = Vec::new();
        let mut line_start = start;
        while line_start < end {
            let line_end = self.source[line_start..end]
                .find('\n')
                .map(|relative| line_start + relative)
                .unwrap_or(end);
            if let Some((label_start, is_default)) = self.switch_label_at(line_start, line_end) {
                label_starts.push((label_start, is_default));
            }
            if line_end == end {
                break;
            }
            line_start = line_end + 1;
        }
        if label_starts.len() <= 1 {
            return None;
        }

        let mut slices = Vec::new();
        for (index, (label_start, is_default)) in label_starts.iter().copied().enumerate() {
            let end = label_starts
                .get(index + 1)
                .map(|(next_start, _)| *next_start)
                .unwrap_or(end);
            let colon_start = self.switch_label_colon(label_start, end)?;
            let keyword_end = label_start
                + if is_default {
                    "default".len()
                } else {
                    "case".len()
                };
            slices.push(RecoveredSwitchCaseSlice {
                label_start,
                keyword_end,
                colon_start,
                colon_end: colon_start + 1,
                body_start: colon_start + 1,
                end,
                is_default,
            });
        }
        Some(slices)
    }

    fn switch_label_at(&self, line_start: usize, line_end: usize) -> Option<(usize, bool)> {
        let mut start = self.skip_horizontal_whitespace(line_start, line_end);
        while self.source.get(start..line_end)?.starts_with('@') {
            let attribute_end = self.source[start..line_end]
                .bytes()
                .position(|byte| byte.is_ascii_whitespace())
                .map(|relative| start + relative)?;
            start = self.skip_horizontal_whitespace(attribute_end, line_end);
        }
        self.switch_label_kind_at(start, line_end)
            .map(|is_default| (start, is_default))
    }

    fn switch_label_kind_at(&self, start: usize, line_end: usize) -> Option<bool> {
        let text = self.source.get(start..line_end)?;
        if text.starts_with("case ") || text.starts_with("case\t") {
            Some(false)
        } else if text.starts_with("default:")
            || text.starts_with("default ")
            || text.starts_with("default\t")
        {
            Some(true)
        } else {
            None
        }
    }

    fn switch_label_colon(&self, start: usize, end: usize) -> Option<usize> {
        let line_end = self.source[start..end]
            .find('\n')
            .map(|relative| start + relative)
            .unwrap_or(end);
        self.source[start..line_end]
            .rfind(':')
            .map(|relative| start + relative)
    }

    fn recovered_switch_case(
        &self,
        node: Node<'a>,
        recovered: RecoveredSwitchCaseSlice,
    ) -> Result<Value> {
        let label = if recovered.is_default {
            self.syntax_node(
                "SwitchDefaultLabelSyntax",
                self.range_from_offsets(recovered.label_start, recovered.colon_end),
                vec![
                    self.with_name(
                        self.token_with_range(
                            "keyword(SwiftSyntax.Keyword.default)",
                            self.range_from_offsets(recovered.label_start, recovered.keyword_end),
                        ),
                        "defaultKeyword",
                    ),
                    self.with_name(
                        self.token_with_range(
                            "colon",
                            self.range_from_offsets(recovered.colon_start, recovered.colon_end),
                        ),
                        "colon",
                    ),
                ],
            )
        } else {
            let (pattern_start, pattern_end) =
                self.trim_offsets(recovered.keyword_end, recovered.colon_start);
            let case_items = if pattern_start < pattern_end {
                vec![self.with_name(
                    self.switch_case_item_from_offsets(pattern_start, pattern_end),
                    "",
                )]
            } else {
                Vec::new()
            };
            let item_range = self.covering_range_or_point(&case_items, recovered.keyword_end);
            self.syntax_node(
                "SwitchCaseLabelSyntax",
                self.range_from_offsets(recovered.label_start, recovered.colon_end),
                vec![
                    self.with_name(
                        self.token_with_range(
                            "keyword(SwiftSyntax.Keyword.case)",
                            self.range_from_offsets(recovered.label_start, recovered.keyword_end),
                        ),
                        "caseKeyword",
                    ),
                    self.with_name(
                        self.syntax_node("SwitchCaseItemListSyntax", item_range, case_items),
                        "caseItems",
                    ),
                    self.with_name(
                        self.token_with_range(
                            "colon",
                            self.range_from_offsets(recovered.colon_start, recovered.colon_end),
                        ),
                        "colon",
                    ),
                ],
            )
        };

        Ok(self.syntax_node(
            "SwitchCaseSyntax",
            self.range_from_offsets(recovered.label_start, recovered.end),
            vec![
                self.with_name(label, "label"),
                self.with_name(
                    self.code_block_item_list_from_statements_in_range(
                        node,
                        recovered.body_start,
                        recovered.end,
                        recovered.body_start,
                    )?,
                    "statements",
                ),
            ],
        ))
    }

    fn switch_case_item_from_offsets(&self, pattern_start: usize, pattern_end: usize) -> Value {
        self.syntax_node(
            "SwitchCaseItemSyntax",
            self.range_from_offsets(pattern_start, pattern_end),
            vec![self.with_name(
                self.synthetic_pattern_from_offsets(pattern_start, pattern_end),
                "pattern",
            )],
        )
    }

    fn code_block_item_list_from_statements_in_range(
        &self,
        node: Node<'a>,
        start: usize,
        end: usize,
        fallback_offset: usize,
    ) -> Result<Value> {
        let statement_nodes: Vec<_> = self
            .immediate_named_child_kind(node, "statements")
            .map(named_children)
            .into_iter()
            .flatten()
            .filter(|child| {
                !is_trivia_node(*child) && child.start_byte() >= start && child.end_byte() <= end
            })
            .collect();
        let mut items = Vec::new();
        self.push_code_block_items_from_nodes(&statement_nodes, &mut items)?;
        let range = self.covering_range_or_point(&items, fallback_offset);
        Ok(self.syntax_node("CodeBlockItemListSyntax", range, items))
    }

    fn synthetic_pattern_from_offsets(&self, start: usize, end: usize) -> Value {
        let (start, end) = self.trim_offsets(start, end);
        if start >= end {
            return self.syntax_node("MissingPatternSyntax", self.point_range(start), Vec::new());
        }
        if self.source[start..end].trim() == "_" {
            return self.wildcard_pattern_from_offsets(start, end);
        }
        if let Some(type_start) = self.synthetic_is_type_pattern_start(start, end) {
            return self.syntax_node(
                "IsTypePatternSyntax",
                self.range_from_offsets(start, end),
                vec![
                    self.with_name(
                        self.token_with_range(
                            "keyword(SwiftSyntax.Keyword.is)",
                            self.range_from_offsets(start, start + "is".len()),
                        ),
                        "isKeyword",
                    ),
                    self.with_name(self.identifier_type_from_offsets(type_start, end), "type"),
                ],
            );
        }
        if let Some((keyword, keyword_end, rest_start, rest_end)) =
            self.synthetic_binding_keyword(start, end)
        {
            let child = self.synthetic_bound_pattern_from_offsets(rest_start, rest_end);
            return self.syntax_node(
                "ValueBindingPatternSyntax",
                self.range_from_offsets(start, end_offset(&child)),
                vec![
                    self.with_name(
                        self.token_with_range(
                            &format!("keyword(SwiftSyntax.Keyword.{keyword})"),
                            self.range_from_offsets(start, keyword_end),
                        ),
                        "bindingSpecifier",
                    ),
                    self.with_name(child, "pattern"),
                ],
            );
        }
        if self.source[start..end].starts_with('(') && self.source[start..end].ends_with(')') {
            if self.synthetic_tuple_is_expression_pattern(start, end) {
                let tuple = self.synthetic_tuple_expr_from_offsets(start, end);
                return self.syntax_node(
                    "ExpressionPatternSyntax",
                    self.range_from_offsets(start, end),
                    vec![self.with_name(tuple, "expression")],
                );
            }
            return self.synthetic_tuple_pattern_from_offsets(start, end);
        }
        if is_identifier_like_text(&self.source[start..end]) {
            return self.identifier_pattern_from_offsets(start, end);
        }
        self.syntax_node(
            "ExpressionPatternSyntax",
            self.range_from_offsets(start, end),
            vec![self.with_name(self.synthetic_expr_from_offsets(start, end), "expression")],
        )
    }

    fn synthetic_bound_pattern_from_offsets(&self, start: usize, end: usize) -> Value {
        if self.source[start..end].starts_with('(') && self.source[start..end].ends_with(')') {
            self.synthetic_tuple_pattern_from_offsets(start, end)
        } else {
            self.synthetic_pattern_from_offsets(start, end)
        }
    }

    fn synthetic_is_type_pattern_start(&self, start: usize, end: usize) -> Option<usize> {
        let rest = self.source[start..end].strip_prefix("is")?;
        if !rest
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            return None;
        }
        let (type_start, type_end) = self.trim_offsets(start + "is".len(), end);
        (type_start < type_end).then_some(type_start)
    }

    fn synthetic_binding_keyword(
        &self,
        start: usize,
        end: usize,
    ) -> Option<(&'static str, usize, usize, usize)> {
        const KEYWORDS: [&str; 7] = [
            "_borrowing",
            "_consuming",
            "_mutating",
            "borrowing",
            "inout",
            "let",
            "var",
        ];
        for keyword in KEYWORDS {
            let keyword_end = start + keyword.len();
            if keyword_end > end || &self.source[start..keyword_end] != keyword {
                continue;
            }
            if keyword_end == end {
                return None;
            }
            let next = self.source.as_bytes()[keyword_end];
            if !next.is_ascii_whitespace() {
                continue;
            }
            let (rest_start, rest_end) = self.trim_offsets(keyword_end, end);
            if rest_start < rest_end {
                return Some((keyword, keyword_end, rest_start, rest_end));
            }
        }
        None
    }

    fn synthetic_tuple_pattern_from_offsets(&self, start: usize, end: usize) -> Value {
        let left_paren_end = start + 1;
        let right_paren_start = end - 1;
        let mut elements = Vec::new();
        let mut element_start = left_paren_end;
        let mut depth = 0usize;
        let bytes = self.source.as_bytes();
        let mut offset = left_paren_end;
        while offset < right_paren_start {
            match bytes[offset] {
                b'(' => depth += 1,
                b')' => depth = depth.saturating_sub(1),
                b',' if depth == 0 => {
                    self.push_synthetic_tuple_pattern_element(
                        &mut elements,
                        element_start,
                        offset,
                        Some(offset),
                    );
                    element_start = offset + 1;
                }
                _ => {}
            }
            offset += 1;
        }
        self.push_synthetic_tuple_pattern_element(
            &mut elements,
            element_start,
            right_paren_start,
            None,
        );

        self.syntax_node(
            "TuplePatternSyntax",
            self.range_from_offsets(start, end),
            vec![
                self.with_name(
                    self.token_with_range(
                        "leftParen",
                        self.range_from_offsets(start, left_paren_end),
                    ),
                    "leftParen",
                ),
                self.with_name(
                    self.syntax_node(
                        "TuplePatternElementListSyntax",
                        self.range_from_offsets(left_paren_end, right_paren_start),
                        elements,
                    ),
                    "elements",
                ),
                self.with_name(
                    self.token_with_range(
                        "rightParen",
                        self.range_from_offsets(right_paren_start, end),
                    ),
                    "rightParen",
                ),
            ],
        )
    }

    fn synthetic_tuple_is_expression_pattern(&self, start: usize, end: usize) -> bool {
        let inner_start = start + 1;
        let inner_end = end.saturating_sub(1);
        let mut element_start = inner_start;
        for comma in self.top_level_commas(inner_start, inner_end) {
            if !self.synthetic_tuple_element_is_expression(element_start, comma) {
                return false;
            }
            element_start = comma + 1;
        }
        self.synthetic_tuple_element_is_expression(element_start, inner_end)
    }

    fn synthetic_tuple_element_is_expression(&self, start: usize, end: usize) -> bool {
        let (start, end) = self.trim_offsets(start, end);
        if start >= end {
            return false;
        }
        let text = &self.source[start..end];
        if text == "_" || self.synthetic_binding_keyword(start, end).is_some() {
            return false;
        }
        let Some(rest) = text.strip_prefix("is") else {
            return true;
        };
        !rest
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_whitespace())
    }

    fn synthetic_tuple_expr_from_offsets(&self, start: usize, end: usize) -> Value {
        let left_paren_end = start + 1;
        let right_paren_start = end - 1;
        self.syntax_node(
            "TupleExprSyntax",
            self.range_from_offsets(start, end),
            vec![
                self.with_name(
                    self.token_with_range(
                        "leftParen",
                        self.range_from_offsets(start, left_paren_end),
                    ),
                    "leftParen",
                ),
                self.with_name(
                    self.synthetic_labeled_expr_list_from_offsets(
                        left_paren_end,
                        right_paren_start,
                    ),
                    "elements",
                ),
                self.with_name(
                    self.token_with_range(
                        "rightParen",
                        self.range_from_offsets(right_paren_start, end),
                    ),
                    "rightParen",
                ),
            ],
        )
    }

    fn push_synthetic_tuple_pattern_element(
        &self,
        elements: &mut Vec<Value>,
        start: usize,
        end: usize,
        comma: Option<usize>,
    ) {
        let (pattern_start, pattern_end) = self.trim_offsets(start, end);
        if pattern_start >= pattern_end {
            return;
        }
        let mut children = vec![self.with_name(
            self.synthetic_bound_pattern_from_offsets(pattern_start, pattern_end),
            "pattern",
        )];
        let element_end = if let Some(comma_start) = comma {
            children.push(self.with_name(
                self.token_with_range(
                    "comma",
                    self.range_from_offsets(comma_start, comma_start + 1),
                ),
                "trailingComma",
            ));
            comma_start + 1
        } else {
            pattern_end
        };
        elements.push(self.with_name(
            self.syntax_node(
                "TuplePatternElementSyntax",
                self.range_from_offsets(pattern_start, element_end),
                children,
            ),
            "",
        ));
    }

    fn synthetic_expr_from_offsets(&self, start: usize, end: usize) -> Value {
        if self.source[start..end].trim() == "_" {
            return self.discard_assignment_expr_from_offsets(start, end);
        }
        if let Some(prefix) = self.synthetic_prefix_slash_expr_from_offsets(start, end) {
            return prefix;
        }
        if let Some(call) = self.synthetic_function_call_expr_from_offsets(start, end) {
            return call;
        }
        if let Some(member_access) = self.synthetic_member_access_expr_from_offsets(start, end) {
            return member_access;
        }
        if let Some(tuple) = self.synthetic_parenthesized_expr_from_offsets(start, end) {
            return tuple;
        }
        let text = &self.source[start..end];
        if text.chars().all(|ch| ch.is_ascii_digit()) {
            return self.syntax_node(
                "IntegerLiteralExprSyntax",
                self.range_from_offsets(start, end),
                vec![self.with_name(
                    self.token_with_range(
                        &format!("integerLiteral({})", quoted_text(text)),
                        self.range_from_offsets(start, end),
                    ),
                    "literal",
                )],
            );
        }
        if matches!(text, "true" | "false") {
            return self.syntax_node(
                "BooleanLiteralExprSyntax",
                self.range_from_offsets(start, end),
                vec![self.with_name(
                    self.token_with_range(
                        &format!("keyword(SwiftSyntax.Keyword.{text})"),
                        self.range_from_offsets(start, end),
                    ),
                    "literal",
                )],
            );
        }
        self.syntax_node(
            "DeclReferenceExprSyntax",
            self.range_from_offsets(start, end),
            vec![self.with_name(
                self.token_with_range(
                    &format!("identifier({})", quoted_text(text)),
                    self.range_from_offsets(start, end),
                ),
                "baseName",
            )],
        )
    }

    fn synthetic_prefix_slash_expr_from_offsets(&self, start: usize, end: usize) -> Option<Value> {
        let text = &self.source[start..end];
        if end <= start + 1 || !text.starts_with('/') || self.is_regex_literal_text(text) {
            return None;
        }
        let (operand_start, operand_end) = self.trim_offsets(start + 1, end);
        if operand_start >= operand_end {
            return None;
        }
        let operand = self.synthetic_expr_from_offsets(operand_start, operand_end);
        Some(self.prefix_operator_expr_from_offsets(start, start + 1, operand, end))
    }

    fn prefix_operator_expr_from_offsets(
        &self,
        start: usize,
        operator_end: usize,
        expression: Value,
        end: usize,
    ) -> Value {
        self.syntax_node(
            "PrefixOperatorExprSyntax",
            self.range_from_offsets(start, end),
            vec![
                self.with_name(
                    self.token_with_range(
                        &format!(
                            "prefixOperator({})",
                            quoted_text(&self.source[start..operator_end])
                        ),
                        self.range_from_offsets(start, operator_end),
                    ),
                    "operator",
                ),
                self.with_name(expression, "expression"),
            ],
        )
    }

    fn synthetic_function_call_expr_from_offsets(&self, start: usize, end: usize) -> Option<Value> {
        let (left_paren_start, right_paren_start) = self.outer_call_parens(start, end)?;
        let (callee_start, callee_end) = self.trim_offsets(start, left_paren_start);
        if callee_start >= callee_end {
            return None;
        }
        Some(self.syntax_node(
            "FunctionCallExprSyntax",
            self.range_from_offsets(start, end),
            vec![
                self.with_name(
                    self.synthetic_expr_from_offsets(callee_start, callee_end),
                    "calledExpression",
                ),
                self.with_name(
                    self.token_with_range(
                        "leftParen",
                        self.range_from_offsets(left_paren_start, left_paren_start + 1),
                    ),
                    "leftParen",
                ),
                self.with_name(
                    self.synthetic_labeled_expr_list_from_offsets(
                        left_paren_start + 1,
                        right_paren_start,
                    ),
                    "arguments",
                ),
                self.with_name(
                    self.token_with_range(
                        "rightParen",
                        self.range_from_offsets(right_paren_start, right_paren_start + 1),
                    ),
                    "rightParen",
                ),
                self.with_name(
                    self.empty_collection("MultipleTrailingClosureElementListSyntax", end),
                    "additionalTrailingClosures",
                ),
            ],
        ))
    }

    fn synthetic_member_access_expr_from_offsets(&self, start: usize, end: usize) -> Option<Value> {
        let dot = self.last_top_level_dot(start, end)?;
        if dot + 1 >= end {
            return None;
        }
        let (member_start, member_end) = self.trim_offsets(dot + 1, end);
        if member_start >= member_end
            || !is_identifier_like_text(&self.source[member_start..member_end])
        {
            return None;
        }

        let mut children = Vec::new();
        let (base_start, base_end) = self.trim_offsets(start, dot);
        if base_start < base_end {
            children.push(self.with_name(
                self.synthetic_expr_from_offsets(base_start, base_end),
                "base",
            ));
        }
        children.push(self.with_name(
            self.token_with_range("period", self.range_from_offsets(dot, dot + 1)),
            "period",
        ));
        children.push(self.with_name(
            self.decl_reference_expr_from_offsets(member_start, member_end),
            "declName",
        ));

        Some(self.syntax_node(
            "MemberAccessExprSyntax",
            self.range_from_offsets(start, end),
            children,
        ))
    }

    fn synthetic_parenthesized_expr_from_offsets(&self, start: usize, end: usize) -> Option<Value> {
        let (left_paren_start, right_paren_start) = self.enclosing_parens(start, end)?;
        Some(self.syntax_node(
            "TupleExprSyntax",
            self.range_from_offsets(start, end),
            vec![
                self.with_name(
                    self.token_with_range(
                        "leftParen",
                        self.range_from_offsets(left_paren_start, left_paren_start + 1),
                    ),
                    "leftParen",
                ),
                self.with_name(
                    self.synthetic_labeled_expr_list_from_offsets(
                        left_paren_start + 1,
                        right_paren_start,
                    ),
                    "elements",
                ),
                self.with_name(
                    self.token_with_range(
                        "rightParen",
                        self.range_from_offsets(right_paren_start, right_paren_start + 1),
                    ),
                    "rightParen",
                ),
            ],
        ))
    }

    fn synthetic_labeled_expr_list_from_offsets(&self, start: usize, end: usize) -> Value {
        let mut args = Vec::new();
        let mut element_start = start;
        for comma in self.top_level_commas(start, end) {
            self.push_synthetic_labeled_expr(&mut args, element_start, comma, Some(comma));
            element_start = comma + 1;
        }
        self.push_synthetic_labeled_expr(&mut args, element_start, end, None);
        self.syntax_node(
            "LabeledExprListSyntax",
            self.range_from_offsets(start, end),
            args,
        )
    }

    fn push_synthetic_labeled_expr(
        &self,
        args: &mut Vec<Value>,
        start: usize,
        end: usize,
        comma: Option<usize>,
    ) {
        let (expr_start, expr_end) = self.trim_offsets(start, end);
        if expr_start >= expr_end {
            return;
        }
        let mut children = vec![self.with_name(
            self.synthetic_expr_from_offsets(expr_start, expr_end),
            "expression",
        )];
        let argument_end = if let Some(comma_start) = comma {
            children.push(self.with_name(
                self.token_with_range(
                    "comma",
                    self.range_from_offsets(comma_start, comma_start + 1),
                ),
                "trailingComma",
            ));
            comma_start + 1
        } else {
            expr_end
        };
        args.push(self.with_name(
            self.syntax_node(
                "LabeledExprSyntax",
                self.range_from_offsets(expr_start, argument_end),
                children,
            ),
            "",
        ));
    }

    fn outer_call_parens(&self, start: usize, end: usize) -> Option<(usize, usize)> {
        if start >= end || self.source.as_bytes().get(end.checked_sub(1)?) != Some(&b')') {
            return None;
        }
        let bytes = self.source.as_bytes();
        let mut depth = 0usize;
        let mut angle_depth = 0usize;
        let mut offset = end;
        while offset > start {
            offset -= 1;
            match bytes[offset] {
                b')' if angle_depth == 0 => depth += 1,
                b'(' if angle_depth == 0 => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return (offset > start).then_some((offset, end - 1));
                    }
                }
                b'>' if depth == 0 => angle_depth += 1,
                b'<' if depth == 0 => angle_depth = angle_depth.saturating_sub(1),
                _ => {}
            }
        }
        None
    }

    fn enclosing_parens(&self, start: usize, end: usize) -> Option<(usize, usize)> {
        if end <= start + 1
            || self.source.as_bytes().get(start) != Some(&b'(')
            || self.source.as_bytes().get(end.checked_sub(1)?) != Some(&b')')
        {
            return None;
        }

        let bytes = self.source.as_bytes();
        let mut depth = 0usize;
        let right_paren_start = end - 1;
        for (offset, byte) in bytes.iter().enumerate().take(end).skip(start) {
            match byte {
                b'(' => depth += 1,
                b')' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 && offset != right_paren_start {
                        return None;
                    }
                }
                _ => {}
            }
        }
        (depth == 0).then_some((start, right_paren_start))
    }

    fn last_top_level_dot(&self, start: usize, end: usize) -> Option<usize> {
        let bytes = self.source.as_bytes();
        let mut paren_depth = 0usize;
        let mut angle_depth = 0usize;
        let mut result = None;
        let mut offset = start;
        while offset < end {
            match bytes[offset] {
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                b'<' if paren_depth == 0 => angle_depth += 1,
                b'>' if paren_depth == 0 => angle_depth = angle_depth.saturating_sub(1),
                b'.' if paren_depth == 0 && angle_depth == 0 => result = Some(offset),
                _ => {}
            }
            offset += 1;
        }
        result
    }

    fn top_level_commas(&self, start: usize, end: usize) -> Vec<usize> {
        let bytes = self.source.as_bytes();
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut angle_depth = 0usize;
        let mut commas = Vec::new();
        let mut offset = start;
        while offset < end {
            match bytes[offset] {
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                b'[' => bracket_depth += 1,
                b']' => bracket_depth = bracket_depth.saturating_sub(1),
                b'<' if paren_depth == 0 && bracket_depth == 0 => angle_depth += 1,
                b'>' if paren_depth == 0 && bracket_depth == 0 => {
                    angle_depth = angle_depth.saturating_sub(1)
                }
                b',' if paren_depth == 0 && bracket_depth == 0 && angle_depth == 0 => {
                    commas.push(offset)
                }
                _ => {}
            }
            offset += 1;
        }
        commas
    }

    fn decl_reference_expr_from_offsets(&self, start: usize, end: usize) -> Value {
        self.syntax_node(
            "DeclReferenceExprSyntax",
            self.range_from_offsets(start, end),
            vec![self.with_name(
                self.token_with_range(
                    &format!("identifier({})", quoted_text(&self.source[start..end])),
                    self.range_from_offsets(start, end),
                ),
                "baseName",
            )],
        )
    }

    fn discard_assignment_expr_from_offsets(&self, start: usize, end: usize) -> Value {
        self.syntax_node(
            "DiscardAssignmentExprSyntax",
            self.range_from_offsets(start, end),
            vec![self.with_name(
                self.token_with_range("wildcard", self.range_from_offsets(start, end)),
                "wildcard",
            )],
        )
    }

    fn identifier_pattern_from_offsets(&self, start: usize, end: usize) -> Value {
        self.syntax_node(
            "IdentifierPatternSyntax",
            self.range_from_offsets(start, end),
            vec![self.with_name(
                self.token_with_range(
                    &format!("identifier({})", quoted_text(&self.source[start..end])),
                    self.range_from_offsets(start, end),
                ),
                "identifier",
            )],
        )
    }

    fn wildcard_pattern_from_offsets(&self, start: usize, end: usize) -> Value {
        self.syntax_node(
            "WildcardPatternSyntax",
            self.range_from_offsets(start, end),
            vec![self.with_name(
                self.token_with_range("wildcard", self.range_from_offsets(start, end)),
                "wildcard",
            )],
        )
    }

    fn array_expr(&self, node: Node<'a>) -> Result<Value> {
        let left_square = self
            .immediate_child_kind(node, "[")
            .context("array literal is missing '['")?;
        let right_square = self
            .immediate_child_kind(node, "]")
            .context("array literal is missing ']'")?;

        let elements = if let Some(regex_elements) =
            self.regex_array_element_list(left_square, right_square)?
        {
            regex_elements
        } else {
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
            self.syntax_node(
                "ArrayElementListSyntax",
                self.range_from_offsets(left_square.end_byte(), right_square.start_byte()),
                elements,
            )
        };

        Ok(self.syntax_node(
            "ArrayExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.token_for_node(left_square, "leftSquare"), "leftSquare"),
                self.with_name(elements, "elements"),
                self.with_name(
                    self.token_for_node(right_square, "rightSquare"),
                    "rightSquare",
                ),
            ],
        ))
    }

    fn empty_array_expr_from_type(&self, node: Node<'a>) -> Result<Value> {
        let left_square = self
            .immediate_child_kind(node, "[")
            .context("array type expression is missing '['")?;
        let right_square = self
            .immediate_child_kind(node, "]")
            .context("array type expression is missing ']'")?;
        Ok(self.syntax_node(
            "ArrayExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.token_for_node(left_square, "leftSquare"), "leftSquare"),
                self.with_name(
                    self.syntax_node(
                        "ArrayElementListSyntax",
                        self.point_range(left_square.end_byte()),
                        Vec::new(),
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

    fn is_recoverable_array_expr_error(&self, node: Node<'a>) -> bool {
        node.kind() == "ERROR"
            && self
                .immediate_named_child_kind(node, "array_type")
                .is_some_and(|array_type| {
                    self.immediate_child_kind(array_type, "[").is_some()
                        && self.immediate_child_kind(array_type, "]").is_some()
                        && !self.tuple_type_fragments(array_type).is_empty()
                })
    }

    fn recovered_array_expr(&self, node: Node<'a>) -> Result<Value> {
        let array_type = if node.kind() == "array_type" {
            node
        } else {
            self.immediate_named_child_kind(node, "array_type")
                .context("recovered array expression is missing array type")?
        };
        let left_square = self
            .immediate_child_kind(array_type, "[")
            .context("recovered array expression is missing '['")?;
        let right_square = self
            .immediate_child_kind(array_type, "]")
            .context("recovered array expression is missing ']'")?;
        let tuple_fragments = self.tuple_type_fragments(array_type);
        let first_tuple = tuple_fragments
            .first()
            .copied()
            .context("recovered array expression is missing tuple element")?;
        let last_tuple = tuple_fragments
            .last()
            .copied()
            .context("recovered array expression is missing tuple element")?;
        let tuple = self.recovered_tuple_expr_from_type_fragments(first_tuple, last_tuple)?;
        let element = self.with_name(
            self.syntax_node(
                "ArrayElementSyntax",
                self.range_from_offsets(first_tuple.start_byte(), last_tuple.end_byte()),
                vec![self.with_name(tuple, "expression")],
            ),
            "",
        );
        let elements = self.syntax_node(
            "ArrayElementListSyntax",
            self.range_from_offsets(left_square.end_byte(), right_square.start_byte()),
            vec![element],
        );
        Ok(self.syntax_node(
            "ArrayExprSyntax",
            self.range_from_offsets(array_type.start_byte(), array_type.end_byte()),
            vec![
                self.with_name(self.token_for_node(left_square, "leftSquare"), "leftSquare"),
                self.with_name(elements, "elements"),
                self.with_name(
                    self.token_for_node(right_square, "rightSquare"),
                    "rightSquare",
                ),
            ],
        ))
    }

    fn recovered_tuple_expr_from_type_fragments(
        &self,
        first_tuple: Node<'a>,
        last_tuple: Node<'a>,
    ) -> Result<Value> {
        let left_paren = self
            .immediate_child_kind(first_tuple, "(")
            .context("recovered tuple expression is missing '('")?;
        let right_paren = self
            .immediate_child_kind(last_tuple, ")")
            .context("recovered tuple expression is missing ')'")?;
        Ok(self.syntax_node(
            "TupleExprSyntax",
            self.range_from_offsets(first_tuple.start_byte(), last_tuple.end_byte()),
            vec![
                self.with_name(self.token_for_node(left_paren, "leftParen"), "leftParen"),
                self.with_name(
                    self.syntax_node(
                        "LabeledExprListSyntax",
                        self.range_from_offsets(left_paren.end_byte(), right_paren.start_byte()),
                        Vec::new(),
                    ),
                    "elements",
                ),
                self.with_name(self.token_for_node(right_paren, "rightParen"), "rightParen"),
            ],
        ))
    }

    fn tuple_type_fragments(&self, node: Node<'a>) -> Vec<Node<'a>> {
        named_children(node)
            .filter_map(|child| match child.kind() {
                "tuple_type" => Some(child),
                "ERROR" => self.immediate_named_child_kind(child, "tuple_type"),
                _ => None,
            })
            .collect()
    }

    fn regex_array_element_list(
        &self,
        left_square: Node<'a>,
        right_square: Node<'a>,
    ) -> Result<Option<Value>> {
        let Some(items) =
            self.regex_literal_list_items(left_square.end_byte(), right_square.start_byte())
        else {
            return Ok(None);
        };
        if items.len() < 2 {
            return Ok(None);
        }
        let elements = items
            .iter()
            .map(|item| {
                let mut element_children = vec![self.with_name(
                    self.regex_literal_expr_from_offsets(item.literal_start, item.literal_end)?,
                    "expression",
                )];
                if let Some((comma_start, comma_end)) = item.comma {
                    element_children.push(self.with_name(
                        self.token_with_range(
                            "comma",
                            self.range_from_offsets(comma_start, comma_end),
                        ),
                        "trailingComma",
                    ));
                }
                let element_end = item.comma.map_or(item.literal_end, |(_, end)| end);
                Ok(self.with_name(
                    self.syntax_node(
                        "ArrayElementSyntax",
                        self.range_from_offsets(item.literal_start, element_end),
                        element_children,
                    ),
                    "",
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(self.syntax_node(
            "ArrayElementListSyntax",
            self.range_from_offsets(left_square.end_byte(), right_square.start_byte()),
            elements,
        )))
    }

    fn regex_literal_list_items(&self, start: usize, end: usize) -> Option<Vec<RegexListItem>> {
        let mut cursor = self.skip_ascii_whitespace(start, end);
        let mut items = Vec::new();
        while cursor < end {
            let literal_start = cursor;
            let opening_pounds = self.count_hashes(cursor, end);
            cursor += opening_pounds;
            if cursor >= end || self.source.as_bytes()[cursor] != b'/' {
                return None;
            }

            let mut scan = cursor + 1;
            let mut literal_end = None;
            while scan < end {
                if self.source.as_bytes()[scan] == b'/'
                    && self.has_hashes(scan + 1, opening_pounds, end)
                {
                    let candidate_end = scan + 1 + opening_pounds;
                    let after_literal = self.skip_ascii_whitespace(candidate_end, end);
                    if after_literal == end || self.source.as_bytes()[after_literal] == b',' {
                        literal_end = Some((candidate_end, after_literal));
                        break;
                    }
                }
                scan += 1;
            }
            let (literal_end, after_literal) = literal_end?;
            let comma = if after_literal < end {
                let comma_end = after_literal + 1;
                cursor = self.skip_ascii_whitespace(comma_end, end);
                Some((after_literal, comma_end))
            } else {
                cursor = after_literal;
                None
            };
            items.push(RegexListItem {
                literal_start,
                literal_end,
                comma,
            });
        }
        (!items.is_empty()).then_some(items)
    }

    fn is_regex_literal_text(&self, text: &str) -> bool {
        let opening_pounds = text.bytes().take_while(|byte| *byte == b'#').count();
        text[opening_pounds..].starts_with('/')
            && text.rfind('/').is_some_and(|closing_slash| {
                closing_slash > opening_pounds
                    && text[closing_slash + 1..].bytes().all(|byte| byte == b'#')
                    && text[closing_slash + 1..].len() == opening_pounds
            })
    }

    fn skip_ascii_whitespace(&self, mut offset: usize, end: usize) -> usize {
        let bytes = self.source.as_bytes();
        while offset < end && bytes[offset].is_ascii_whitespace() {
            offset += 1;
        }
        offset
    }

    fn count_hashes(&self, mut offset: usize, end: usize) -> usize {
        let bytes = self.source.as_bytes();
        let start = offset;
        while offset < end && bytes[offset] == b'#' {
            offset += 1;
        }
        offset - start
    }

    fn has_hashes(&self, offset: usize, count: usize, end: usize) -> bool {
        offset + count <= end
            && self.source.as_bytes()[offset..offset + count]
                .iter()
                .all(|byte| *byte == b'#')
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
        if self.is_recoverable_prefix_slash_member_call_tuple(node) {
            if let Some(call) =
                self.synthetic_function_call_expr_from_offsets(node.start_byte(), node.end_byte())
            {
                return Ok(call);
            }
        }
        let left_paren = self
            .immediate_child_kind(node, "(")
            .context("tuple expression is missing '('")?;
        let right_paren = self
            .immediate_child_kind(node, ")")
            .context("tuple expression is missing ')'")?;
        let values = self.field_children(node, "value");
        let mut elements = Vec::new();
        for value in values {
            if self.is_recovery_bang_node(value) {
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

    fn is_recoverable_prefix_slash_member_call_tuple(&self, node: Node<'a>) -> bool {
        if node.kind() != "tuple_expression" {
            return false;
        }
        let text = self.text(node);
        text.starts_with("(/")
            && text.contains(").")
            && self
                .field_children(node, "value")
                .into_iter()
                .any(|child| child.kind() == "regex_literal")
            && named_children(node).any(|child| child.kind() == "ERROR")
    }

    fn labeled_expr_list_from_tuple_expr(
        &self,
        node: Node<'a>,
        left_paren: Node<'a>,
        right_paren: Node<'a>,
    ) -> Result<Value> {
        let labels = self.field_children(node, "name");
        let mut elements = Vec::new();
        for value in self
            .field_children(node, "value")
            .into_iter()
            .filter(|value| !self.is_recovery_bang_node(*value))
        {
            let trailing_comma = self.trailing_delimiter(node, value, ",");
            let label = labels
                .iter()
                .copied()
                .rfind(|label| label.end_byte() <= value.start_byte());
            let mut children = Vec::new();
            let element_start = if let Some(label) = label {
                let colon = self
                    .children_between(node, label.end_byte(), value.start_byte())
                    .into_iter()
                    .find(|child| child.kind() == ":")
                    .context("keyword apply call argument label is missing ':'")?;
                children.push(self.with_name(
                    self.token_for_node(
                        label,
                        &format!("identifier({})", quoted_text(self.text(label))),
                    ),
                    "label",
                ));
                children.push(self.with_name(self.token_for_node(colon, "colon"), "colon"));
                label.start_byte()
            } else {
                value.start_byte()
            };
            children.push(self.with_name(self.expr(value)?, "expression"));
            if let Some(comma) = trailing_comma {
                children.push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
            }
            let element_end = trailing_comma.map_or(value.end_byte(), |comma| comma.end_byte());
            elements.push(self.with_name(
                self.syntax_node(
                    "LabeledExprSyntax",
                    self.range_from_offsets(element_start, element_end),
                    children,
                ),
                "",
            ));
        }

        Ok(self.syntax_node(
            "LabeledExprListSyntax",
            self.range_from_offsets(left_paren.end_byte(), right_paren.start_byte()),
            elements,
        ))
    }

    fn closure_expr(&self, node: Node<'a>) -> Result<Value> {
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

        let statement_nodes = statements
            .map(named_children)
            .into_iter()
            .flatten()
            .filter(|child| !is_trivia_node(*child))
            .collect::<Vec<_>>();
        let mut statement_items = Vec::new();
        self.push_code_block_items_from_nodes(&statement_nodes, &mut statement_items)?;
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

        if let Some(captures) = self.field_child(closure, "captures") {
            children.push(self.with_name(self.closure_capture_clause(captures)?, "capture"));
        }

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
            self.range_from_offsets(
                self.field_child(closure, "captures")
                    .map_or(node.start_byte(), |captures| captures.start_byte()),
                in_keyword.end_byte(),
            ),
            children,
        ))
    }

    fn closure_capture_clause(&self, node: Node<'a>) -> Result<Value> {
        let left_square = self
            .immediate_child_kind(node, "[")
            .context("closure capture clause is missing '['")?;
        let right_square = self
            .immediate_child_kind(node, "]")
            .context("closure capture clause is missing ']'")?;
        let mut captures = Vec::new();
        for capture in named_children(node).filter(|child| child.kind() == "capture_list_item") {
            let trailing_comma = self.trailing_delimiter(node, capture, ",");
            captures.push(self.with_name(self.closure_capture(capture, trailing_comma)?, ""));
        }
        Ok(self.syntax_node(
            "ClosureCaptureClauseSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.token_for_node(left_square, "leftSquare"), "leftSquare"),
                self.with_name(
                    self.syntax_node(
                        "ClosureCaptureListSyntax",
                        self.covering_range_or_point(&captures, left_square.end_byte()),
                        captures,
                    ),
                    "items",
                ),
                self.with_name(
                    self.token_for_node(right_square, "rightSquare"),
                    "rightSquare",
                ),
            ],
        ))
    }

    fn closure_capture(&self, node: Node<'a>, trailing_comma: Option<Node<'a>>) -> Result<Value> {
        let mut children = Vec::new();
        if let Some(specifier) = self.immediate_named_child_kind(node, "ownership_modifier") {
            children.push(self.with_name(self.closure_capture_specifier(specifier), "specifier"));
        }
        let name = self
            .field_child(node, "name")
            .or_else(|| {
                named_children(node)
                    .find(|child| matches!(child.kind(), "simple_identifier" | "self_expression"))
            })
            .context("closure capture is missing a name")?;
        children.push(self.with_name(self.capture_name_token(name), "name"));
        if let Some(value) = self.field_child(node, "value") {
            let equal = self
                .immediate_child_kind(node, "=")
                .context("closure capture initializer is missing '='")?;
            children.push(self.with_name(self.initializer_clause(equal, value)?, "initializer"));
        }
        if let Some(comma) = trailing_comma {
            children.push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
        }
        Ok(self.syntax_node("ClosureCaptureSyntax", self.range_for_node(node), children))
    }

    fn closure_capture_specifier(&self, node: Node<'a>) -> Value {
        let token_kind = match self.text(node) {
            "weak" => "keyword(SwiftSyntax.Keyword.weak)",
            "unowned" => "keyword(SwiftSyntax.Keyword.unowned)",
            other => {
                return self.token_for_node(node, &format!("identifier({})", quoted_text(other)))
            }
        };
        self.syntax_node(
            "ClosureCaptureSpecifierSyntax",
            self.range_for_node(node),
            vec![self.with_name(self.token_for_node(node, token_kind), "specifier")],
        )
    }

    fn capture_name_token(&self, node: Node<'a>) -> Value {
        if self.text(node) == "self" {
            self.token_for_node(node, "keyword(SwiftSyntax.Keyword.self)")
        } else {
            self.token_for_node(
                node,
                &format!("identifier({})", quoted_text(self.text(node))),
            )
        }
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
        let (first_name, second_name) = self.lambda_parameter_names(node)?;
        let type_node = self.lambda_parameter_type(node);
        let mut children = vec![
            self.with_name(self.attribute_list(node)?, "attributes"),
            self.with_name(self.modifier_list(node), "modifiers"),
            self.with_name(
                self.token_for_node(
                    first_name,
                    &format!("identifier({})", quoted_text(self.text(first_name))),
                ),
                "firstName",
            ),
        ];
        if let Some(second_name) = second_name {
            children.push(self.with_name(
                self.token_for_node(
                    second_name,
                    &format!("identifier({})", quoted_text(self.text(second_name))),
                ),
                "secondName",
            ));
        }
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
        let (first_name, second_name) = self.lambda_parameter_names(node)?;
        Ok(second_name.unwrap_or(first_name))
    }

    fn lambda_parameter_names(&self, node: Node<'a>) -> Result<(Node<'a>, Option<Node<'a>>)> {
        let mut name_cursor = node.walk();
        let name_nodes = node
            .children_by_field_name("name", &mut name_cursor)
            .filter(|child| matches!(child.kind(), "simple_identifier" | "identifier" | "_"))
            .collect::<Vec<_>>();
        if let Some(external_name) = self.field_child(node, "external_name") {
            let second_name = name_nodes
                .iter()
                .copied()
                .find(|name| name.start_byte() >= external_name.end_byte());
            return Ok((external_name, second_name));
        }
        if let Some(name) = name_nodes.first().copied() {
            return Ok((name, None));
        }
        named_children(node)
            .find(|child| matches!(child.kind(), "simple_identifier" | "identifier" | "_"))
            .map(|name| (name, None))
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
            let base_expr = if let Some(question_mark) = self.optional_chain_question_between(
                node,
                base.end_byte(),
                suffix_node.start_byte(),
            ) {
                let expression = self.expr(base)?;
                self.optional_chaining_expr_from_value(
                    expression,
                    question_mark,
                    base.start_byte(),
                    question_mark.end_byte(),
                )
            } else {
                self.expr(base)?
            };
            children.push(self.with_name(base_expr, "base"));
        }
        children.push(self.with_name(self.token_for_node(period, "period"), "period"));
        children.push(self.with_name(self.decl_reference_expr(suffix), "declName"));

        Ok(self.syntax_node(
            "MemberAccessExprSyntax",
            self.range_for_node(node),
            children,
        ))
    }

    fn optional_chaining_expr_from_value(
        &self,
        expression: Value,
        question_mark: Node<'a>,
        start: usize,
        end: usize,
    ) -> Value {
        self.syntax_node(
            "OptionalChainingExprSyntax",
            self.range_from_offsets(start, end),
            vec![
                self.with_name(expression, "expression"),
                self.with_name(
                    self.token_for_node(question_mark, "postfixQuestionMark"),
                    "questionMark",
                ),
            ],
        )
    }

    fn called_expression_with_optional_chaining(
        &self,
        parent: Node<'a>,
        callee: Node<'a>,
        suffix: Node<'a>,
    ) -> Result<Value> {
        let expression = self.expr(callee)?;
        Ok(
            if let Some(question_mark) =
                self.optional_chain_question_between(parent, callee.end_byte(), suffix.start_byte())
            {
                self.optional_chaining_expr_from_value(
                    expression,
                    question_mark,
                    callee.start_byte(),
                    question_mark.end_byte(),
                )
            } else {
                expression
            },
        )
    }

    fn optional_chain_question_after(&self, expression: Node<'a>, end: usize) -> Option<Node<'a>> {
        let parent = expression.parent()?;
        self.optional_chain_question_between(parent, expression.end_byte(), end)
    }

    fn optional_chain_question_between(
        &self,
        parent: Node<'a>,
        start: usize,
        end: usize,
    ) -> Option<Node<'a>> {
        children(parent).find(|child| {
            child.start_byte() >= start
                && child.end_byte() <= end
                && self.is_optional_chain_question_mark(*child)
        })
    }

    fn is_optional_chain_question_mark(&self, node: Node<'a>) -> bool {
        node.kind() == "?" && self.text(node) == "?"
    }

    fn is_recoverable_prefix_slash_navigation(&self, node: Node<'a>) -> bool {
        node.kind() == "navigation_expression"
            && self.field_child(node, "target").is_some_and(|target| {
                target.kind() == "/" && target.start_byte() == node.start_byte()
            })
            && self.field_child(node, "suffix").is_some()
            && named_children(node)
                .any(|child| child.kind() == "ERROR" && is_identifier_like_text(self.text(child)))
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

        if operation.kind() == "&" || self.text(operation) == "&" {
            if is_binary_expression_kind(target.kind()) {
                let lhs = self
                    .expression_field_child(target, "lhs")
                    .or_else(|| self.expression_field_child(target, "start"))
                    .or_else(|| self.field_child(target, "lhs"))
                    .or_else(|| self.field_child(target, "start"))
                    .context("inout binary recovery is missing lhs")?;
                let op = self
                    .field_child(target, "op")
                    .context("inout binary recovery is missing operator")?;
                let rhs = self
                    .expression_field_child(target, "rhs")
                    .or_else(|| self.expression_field_child(target, "end"))
                    .or_else(|| self.field_child(target, "rhs"))
                    .or_else(|| self.field_child(target, "end"))
                    .context("inout binary recovery is missing rhs")?;
                let lhs_inout = self.in_out_expr(operation, lhs)?;
                return self.infix_operator_expr_from_values(
                    operation.start_byte(),
                    rhs.end_byte(),
                    lhs_inout,
                    op,
                    self.expr(rhs)?,
                );
            }
            return self.in_out_expr(operation, target);
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

    fn in_out_expr(&self, ampersand: Node<'a>, expression: Node<'a>) -> Result<Value> {
        Ok(self.syntax_node(
            "InOutExprSyntax",
            self.range_from_offsets(ampersand.start_byte(), expression.end_byte()),
            vec![
                self.with_name(
                    self.token_for_node(ampersand, "prefixAmpersand"),
                    "ampersand",
                ),
                self.with_name(self.expr(expression)?, "expression"),
            ],
        ))
    }

    fn binary_operator_expr(&self, node: Node<'a>) -> Result<Value> {
        if let Some(member_access) = self.recovered_generic_metatype_member_access_expr(node) {
            return Ok(member_access);
        }

        let raw_lhs = self
            .field_child(node, "lhs")
            .or_else(|| self.field_child(node, "start"))
            .context("binary expression is missing lhs")?;
        let op = self
            .field_child(node, "op")
            .context("binary expression is missing operator")?;
        let raw_rhs = self
            .field_child(node, "rhs")
            .or_else(|| self.field_child(node, "end"))
            .context("binary expression is missing rhs")?;
        if self.is_recovery_bang_node(raw_lhs) {
            return self.expr(raw_rhs);
        }
        if self.is_recovery_bang_node(raw_rhs) {
            return self.expr(raw_lhs);
        }
        let lhs = self
            .expression_field_child(node, "lhs")
            .or_else(|| self.expression_field_child(node, "start"))
            .unwrap_or(raw_lhs);
        let rhs = self
            .expression_field_child(node, "rhs")
            .or_else(|| self.expression_field_child(node, "end"))
            .unwrap_or(raw_rhs);
        if lhs.kind() == "try_expression" && lhs.start_byte() == node.start_byte() {
            let try_expression = self
                .expression_field_child(lhs, "expr")
                .context("try expression is missing expression")?;
            let expression =
                self.infix_operator_expr_from_parts(node, try_expression, op, rhs, rhs.end_byte())?;
            return self.try_expr_wrapping_value(lhs, expression);
        }
        self.infix_operator_expr_from_parts(node, lhs, op, rhs, rhs.end_byte())
    }

    fn recovered_generic_metatype_member_access_expr(&self, node: Node<'a>) -> Option<Value> {
        if node.kind() != "comparison_expression" {
            return None;
        }

        let (start, end) = self.trim_offsets(node.start_byte(), node.end_byte());
        let dot = self.last_top_level_dot(start, end)?;
        let (member_start, member_end) = self.trim_offsets(dot + 1, end);
        if member_start >= member_end || &self.source[member_start..member_end] != "self" {
            return None;
        }

        let left_angle = self.source[start..dot]
            .find('<')
            .map(|offset| start + offset)?;
        let right_angle = self.source[left_angle + 1..dot]
            .rfind('>')
            .map(|offset| left_angle + 1 + offset)?;
        if !self.source[right_angle + 1..dot].trim().is_empty() {
            return None;
        }

        let (base_start, base_end) = self.trim_offsets(start, left_angle);
        if base_start >= base_end || !is_identifier_like_text(&self.source[base_start..base_end]) {
            return None;
        }

        Some(self.syntax_node(
            "MemberAccessExprSyntax",
            self.range_from_offsets(start, end),
            vec![
                self.with_name(
                    self.decl_reference_expr_from_offsets(base_start, base_end),
                    "base",
                ),
                self.with_name(
                    self.token_with_range("period", self.range_from_offsets(dot, dot + 1)),
                    "period",
                ),
                self.with_name(
                    self.decl_reference_expr_from_offsets(member_start, member_end),
                    "declName",
                ),
            ],
        ))
    }

    fn infix_operator_expr_from_parts(
        &self,
        original: Node<'a>,
        lhs: Node<'a>,
        op: Node<'a>,
        rhs: Node<'a>,
        end: usize,
    ) -> Result<Value> {
        self.infix_operator_expr_from_values(
            lhs.start_byte().max(original.start_byte()),
            end,
            self.expr(lhs)?,
            op,
            self.expr(rhs)?,
        )
    }

    fn infix_operator_expr_from_values(
        &self,
        start: usize,
        end: usize,
        lhs: Value,
        op: Node<'a>,
        rhs: Value,
    ) -> Result<Value> {
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
            self.range_from_offsets(start, end),
            vec![
                self.with_name(lhs, "leftOperand"),
                self.with_name(operator, "operator"),
                self.with_name(rhs, "rightOperand"),
            ],
        ))
    }

    fn nil_coalescing_expr(&self, node: Node<'a>) -> Result<Value> {
        let value = self
            .expression_field_child(node, "value")
            .or_else(|| self.field_child(node, "value"))
            .context("nil coalescing expression is missing value")?;
        let if_nil = self
            .expression_field_child(node, "if_nil")
            .or_else(|| self.field_child(node, "if_nil"))
            .context("nil coalescing expression is missing fallback")?;
        let operator_start = self.source[value.end_byte()..if_nil.start_byte()]
            .find("??")
            .map(|offset| value.end_byte() + offset)
            .context("nil coalescing expression is missing '??'")?;
        let operator_end = operator_start + 2;
        let operator = self.syntax_node(
            "BinaryOperatorExprSyntax",
            self.range_from_offsets(operator_start, operator_end),
            vec![self.with_name(
                self.token_with_range(
                    "binaryOperator(\"??\")",
                    self.range_from_offsets(operator_start, operator_end),
                ),
                "operator",
            )],
        );
        Ok(self.syntax_node(
            "InfixOperatorExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.expr(value)?, "leftOperand"),
                self.with_name(operator, "operator"),
                self.with_name(self.expr(if_nil)?, "rightOperand"),
            ],
        ))
    }

    fn ternary_expr(&self, node: Node<'a>) -> Result<Value> {
        let condition = self
            .field_child(node, "condition")
            .context("ternary expression is missing condition")?;
        let then_expression = self
            .field_child(node, "if_true")
            .context("ternary expression is missing then expression")?;
        let else_expression = self
            .field_child(node, "if_false")
            .context("ternary expression is missing else expression")?;
        let question_mark = children(node)
            .find(|child| child.kind() == "?")
            .context("ternary expression is missing '?'")?;
        let colon = children(node)
            .find(|child| child.kind() == ":")
            .context("ternary expression is missing ':'")?;

        if let Some(sequence) = self.recovered_arrow_sequence_expr(
            node,
            condition,
            question_mark,
            then_expression,
            colon,
            else_expression,
        )? {
            return Ok(sequence);
        }

        if let Some(reassociated) = self.reassociated_ternary_expr(
            node,
            condition,
            question_mark,
            then_expression,
            colon,
            else_expression,
        )? {
            return Ok(reassociated);
        }

        self.ternary_expr_from_condition_value(
            self.expr(condition)?,
            TernaryNodeParts {
                start: condition.start_byte(),
                end: node.end_byte(),
                question_mark,
                then_expression,
                colon,
                else_expression,
            },
        )
    }

    fn ternary_expr_from_values(&self, parts: TernaryValueParts<'a>) -> Value {
        self.syntax_node(
            "TernaryExprSyntax",
            self.range_from_offsets(parts.start, parts.end),
            vec![
                self.with_name(parts.condition, "condition"),
                self.with_name(
                    self.token_for_node(parts.question_mark, "infixQuestionMark"),
                    "questionMark",
                ),
                self.with_name(parts.then_expression, "thenExpression"),
                self.with_name(self.token_for_node(parts.colon, "colon"), "colon"),
                self.with_name(parts.else_expression, "elseExpression"),
            ],
        )
    }

    fn ternary_expr_from_condition_value(
        &self,
        condition: Value,
        parts: TernaryNodeParts<'a>,
    ) -> Result<Value> {
        Ok(self.ternary_expr_from_values(TernaryValueParts {
            start: parts.start,
            end: parts.end,
            condition,
            question_mark: parts.question_mark,
            then_expression: self.expr(parts.then_expression)?,
            colon: parts.colon,
            else_expression: self.expr(parts.else_expression)?,
        }))
    }

    fn reassociated_ternary_expr(
        &self,
        node: Node<'a>,
        condition: Node<'a>,
        question_mark: Node<'a>,
        then_expression: Node<'a>,
        colon: Node<'a>,
        else_expression: Node<'a>,
    ) -> Result<Option<Value>> {
        if condition.kind() != "ternary_expression" || condition.start_byte() != node.start_byte() {
            return Ok(None);
        }
        let inner_condition = self
            .field_child(condition, "condition")
            .context("ternary expression is missing condition")?;
        let inner_then_expression = self
            .field_child(condition, "if_true")
            .context("ternary expression is missing then expression")?;
        let inner_else_expression = self
            .field_child(condition, "if_false")
            .context("ternary expression is missing else expression")?;
        let inner_question_mark = children(condition)
            .find(|child| child.kind() == "?")
            .context("ternary expression is missing '?'")?;
        let inner_colon = children(condition)
            .find(|child| child.kind() == ":")
            .context("ternary expression is missing ':'")?;

        let nested_else = self.ternary_expr_from_condition_value(
            self.expr(inner_else_expression)?,
            TernaryNodeParts {
                start: inner_else_expression.start_byte(),
                end: node.end_byte(),
                question_mark,
                then_expression,
                colon,
                else_expression,
            },
        )?;
        Ok(Some(self.ternary_expr_from_values(TernaryValueParts {
            start: node.start_byte(),
            end: node.end_byte(),
            condition: self.expr(inner_condition)?,
            question_mark: inner_question_mark,
            then_expression: self.expr(inner_then_expression)?,
            colon: inner_colon,
            else_expression: nested_else,
        })))
    }

    fn recovered_arrow_sequence_expr(
        &self,
        node: Node<'a>,
        condition: Node<'a>,
        question_mark: Node<'a>,
        then_expression: Node<'a>,
        colon: Node<'a>,
        else_expression: Node<'a>,
    ) -> Result<Option<Value>> {
        if condition.kind() != "as_expression" {
            return Ok(None);
        }
        let Some(check_expression) = self.expression_field_child(condition, "expr") else {
            return Ok(None);
        };
        if check_expression.kind() != "check_expression" {
            return Ok(None);
        }
        let Some(arrow_expression) = self.expression_field_child(check_expression, "target") else {
            return Ok(None);
        };
        if !self.is_arrow_artifact_expression(arrow_expression) {
            return Ok(None);
        }
        let Some(sequence_head) = self.field_child(arrow_expression, "lhs") else {
            return Ok(None);
        };
        let Some(recovered_subject) = named_children(check_expression).find(|child| {
            child.kind() == "ERROR"
                && child.start_byte() >= arrow_expression.end_byte()
                && is_identifier_like_text(self.text(*child))
        }) else {
            return Ok(None);
        };
        let is_keyword = self
            .field_child(check_expression, "op")
            .context("is expression is missing is keyword")?;
        let is_type = self
            .field_child(check_expression, "name")
            .context("is expression is missing target type")?;
        let as_operator = self
            .immediate_named_child_kind(condition, "as_operator")
            .or_else(|| named_children(condition).find(|child| child.kind() == "as_operator"))
            .context("as expression is missing as operator")?;
        let as_type = self
            .field_child(condition, "name")
            .context("as expression is missing target type")?;

        let is_value =
            self.is_expr_from_parts(check_expression, recovered_subject, is_keyword, is_type)?;
        let condition_value = self.as_expr_from_value(
            condition,
            recovered_subject.start_byte(),
            is_value,
            as_operator,
            as_type,
        )?;
        let ternary = self.ternary_expr_from_condition_value(
            condition_value,
            TernaryNodeParts {
                start: recovered_subject.start_byte(),
                end: node.end_byte(),
                question_mark,
                then_expression,
                colon,
                else_expression,
            },
        )?;
        let sequence_head = self.expr(sequence_head)?;
        let elements = self.syntax_node(
            "ExprListSyntax",
            self.range_from_offsets(arrow_expression.start_byte(), node.end_byte()),
            vec![
                self.with_name(sequence_head, ""),
                self.with_name(ternary, ""),
            ],
        );
        Ok(Some(self.syntax_node(
            "SequenceExprSyntax",
            self.range_from_offsets(arrow_expression.start_byte(), node.end_byte()),
            vec![self.with_name(elements, "elements")],
        )))
    }

    fn is_arrow_artifact_expression(&self, node: Node<'a>) -> bool {
        node.kind() == "additive_expression"
            && self
                .field_child(node, "op")
                .is_some_and(|op| self.text(op) == "-")
            && self
                .field_child(node, "rhs")
                .is_some_and(|rhs| self.text(rhs) == ">")
    }

    fn try_expr(&self, node: Node<'a>) -> Result<Value> {
        let expression = self
            .expression_field_child(node, "expr")
            .context("try expression is missing expression")?;
        let value = self.expr(expression)?;
        self.try_expr_wrapping_value(node, value)
    }

    fn try_expr_wrapping_value(&self, node: Node<'a>, expression: Value) -> Result<Value> {
        let try_operator = self
            .immediate_named_child_kind(node, "try_operator")
            .or_else(|| named_children(node).find(|child| child.kind() == "try_operator"))
            .context("try expression is missing try operator")?;
        let mut children = vec![self.with_name(
            self.token_with_range(
                "keyword(SwiftSyntax.Keyword.try)",
                self.range_from_offsets(try_operator.start_byte(), try_operator.start_byte() + 3),
            ),
            "tryKeyword",
        )];

        let operator_text = self.text(try_operator);
        if let Some(mark_offset) = operator_text.find(['?', '!']) {
            let mark_start = try_operator.start_byte() + mark_offset;
            let mark_end = mark_start + 1;
            let token_kind = if &operator_text[mark_offset..mark_offset + 1] == "?" {
                "postfixQuestionMark"
            } else {
                "exclamationMark"
            };
            children.push(self.with_name(
                self.token_with_range(token_kind, self.range_from_offsets(mark_start, mark_end)),
                "questionOrExclamationMark",
            ));
        }

        children.push(self.with_name(expression, "expression"));
        Ok(self.syntax_node(
            "TryExprSyntax",
            self.range_from_offsets(node.start_byte(), end_offset(children.last().unwrap())),
            children,
        ))
    }

    fn await_expr(&self, node: Node<'a>) -> Result<Value> {
        let await_keyword = self
            .immediate_child_kind(node, "await")
            .context("await expression is missing await keyword")?;
        let expression = self
            .expression_field_child(node, "expr")
            .context("await expression is missing expression")?;
        Ok(self.syntax_node(
            "AwaitExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.token_for_node(await_keyword, "keyword(SwiftSyntax.Keyword.await)"),
                    "awaitKeyword",
                ),
                self.with_name(self.expr(expression)?, "expression"),
            ],
        ))
    }

    fn as_expr(&self, node: Node<'a>) -> Result<Value> {
        let expression = self
            .expression_field_child(node, "expr")
            .context("as expression is missing expression")?;
        let as_operator = self
            .immediate_named_child_kind(node, "as_operator")
            .or_else(|| named_children(node).find(|child| child.kind() == "as_operator"))
            .context("as expression is missing as operator")?;
        let type_node = self
            .field_child(node, "name")
            .context("as expression is missing target type")?;

        if expression.kind() == "try_expression" && expression.start_byte() == node.start_byte() {
            let try_expression = self
                .expression_field_child(expression, "expr")
                .context("try expression is missing expression")?;
            let value = self.as_expr_from_parts(node, try_expression, as_operator, type_node)?;
            return self.try_expr_wrapping_value(expression, value);
        }

        self.as_expr_from_parts(node, expression, as_operator, type_node)
    }

    fn as_expr_from_parts(
        &self,
        node: Node<'a>,
        expression: Node<'a>,
        as_operator: Node<'a>,
        type_node: Node<'a>,
    ) -> Result<Value> {
        self.as_expr_from_value(
            node,
            expression.start_byte(),
            self.expr(expression)?,
            as_operator,
            type_node,
        )
    }

    fn as_expr_from_value(
        &self,
        node: Node<'a>,
        start: usize,
        expression: Value,
        as_operator: Node<'a>,
        type_node: Node<'a>,
    ) -> Result<Value> {
        let mut children = vec![
            self.with_name(expression, "expression"),
            self.with_name(
                self.token_with_range(
                    "keyword(SwiftSyntax.Keyword.as)",
                    self.range_from_offsets(as_operator.start_byte(), as_operator.start_byte() + 2),
                ),
                "asKeyword",
            ),
        ];
        let operator_text = self.text(as_operator);
        if let Some(mark_offset) = operator_text.find(['?', '!']) {
            let mark_start = as_operator.start_byte() + mark_offset;
            let mark_end = mark_start + 1;
            let token_kind = if &operator_text[mark_offset..mark_offset + 1] == "?" {
                "postfixQuestionMark"
            } else {
                "exclamationMark"
            };
            children.push(self.with_name(
                self.token_with_range(token_kind, self.range_from_offsets(mark_start, mark_end)),
                "questionOrExclamationMark",
            ));
        }
        children.push(self.with_name(self.type_syntax(type_node)?, "type"));
        Ok(self.syntax_node(
            "AsExprSyntax",
            self.range_from_offsets(start, node.end_byte()),
            children,
        ))
    }

    fn is_expr(&self, node: Node<'a>) -> Result<Value> {
        let expression = self
            .expression_field_child(node, "target")
            .context("is expression is missing expression")?;
        let is_keyword = self
            .field_child(node, "op")
            .context("is expression is missing is keyword")?;
        let type_node = self
            .field_child(node, "name")
            .context("is expression is missing target type")?;

        if expression.kind() == "try_expression" && expression.start_byte() == node.start_byte() {
            let try_expression = self
                .expression_field_child(expression, "expr")
                .context("try expression is missing expression")?;
            let value = self.is_expr_from_parts(node, try_expression, is_keyword, type_node)?;
            return self.try_expr_wrapping_value(expression, value);
        }

        self.is_expr_from_parts(node, expression, is_keyword, type_node)
    }

    fn is_expr_from_parts(
        &self,
        node: Node<'a>,
        expression: Node<'a>,
        is_keyword: Node<'a>,
        type_node: Node<'a>,
    ) -> Result<Value> {
        Ok(self.syntax_node(
            "IsExprSyntax",
            self.range_from_offsets(expression.start_byte(), node.end_byte()),
            vec![
                self.with_name(self.expr(expression)?, "expression"),
                self.with_name(
                    self.token_for_node(is_keyword, "keyword(SwiftSyntax.Keyword.is)"),
                    "isKeyword",
                ),
                self.with_name(self.type_syntax(type_node)?, "type"),
            ],
        ))
    }

    fn if_expr(&self, node: Node<'a>) -> Result<Value> {
        let if_keyword = self
            .immediate_child_kind(node, "if")
            .context("if expression is missing 'if'")?;
        let left_brace = self
            .nearest_child_after(node, "{", if_keyword.end_byte())
            .context("if expression body is missing '{'")?;
        let right_brace = self
            .nearest_child_after(node, "}", left_brace.end_byte())
            .context("if expression body is missing '}'")?;
        let body_statements = named_children(node).find(|child| {
            child.kind() == "statements"
                && child.start_byte() >= left_brace.end_byte()
                && child.end_byte() <= right_brace.start_byte()
        });
        let conditions = self.condition_nodes(node, if_keyword, left_brace.start_byte());
        if conditions.is_empty() {
            bail!("if expression is missing condition");
        }

        let mut children = vec![
            self.with_name(
                self.token_for_node(if_keyword, "keyword(SwiftSyntax.Keyword.if)"),
                "ifKeyword",
            ),
            self.with_name(
                self.condition_element_list_from_nodes(node, &conditions, if_keyword.end_byte())?,
                "conditions",
            ),
            self.with_name(
                self.code_block_from_statements(body_statements, left_brace, right_brace)?,
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
            } else if let Some(else_left_brace) =
                self.nearest_child_after(node, "{", else_keyword.end_byte())
            {
                let else_right_brace = self
                    .nearest_child_after(node, "}", else_left_brace.end_byte())
                    .context("if else body is missing '}'")?;
                let else_statements = named_children(node).find(|child| {
                    child.kind() == "statements"
                        && child.start_byte() >= else_left_brace.end_byte()
                        && child.end_byte() <= else_right_brace.start_byte()
                });
                children.push(self.with_name(
                    self.code_block_from_statements(
                        else_statements,
                        else_left_brace,
                        else_right_brace,
                    )?,
                    "elseBody",
                ));
            }
        }

        Ok(self.syntax_node("IfExprSyntax", self.range_for_node(node), children))
    }

    fn is_recoverable_missing_if_error(&self, node: Node<'a>) -> bool {
        node.kind() == "ERROR"
            && self.text(node).trim_start().starts_with("if ")
            && self.first_descendant_kind(node, "lambda_literal").is_some()
    }

    fn recovered_missing_if_expr(&self, node: Node<'a>) -> Result<Value> {
        let node_start = node.start_byte();
        let node_end = node.end_byte();
        let if_start = self.skip_horizontal_whitespace(node_start, node_end);
        let if_end = if_start + "if".len();
        let body = self
            .first_descendant_kind(node, "lambda_literal")
            .context("recovered missing-if expression is missing body")?;
        let (condition_start, condition_end) = self.trim_offsets(if_end, body.start_byte());
        if condition_start >= condition_end {
            bail!("recovered missing-if expression is missing condition");
        }

        let condition = self.syntax_node(
            "ConditionElementSyntax",
            self.range_from_offsets(condition_start, condition_end),
            vec![self.with_name(
                self.synthetic_expr_from_offsets(condition_start, condition_end),
                "condition",
            )],
        );
        let conditions = self.syntax_node(
            "ConditionElementListSyntax",
            self.range_from_offsets(condition_start, condition_end),
            vec![self.with_name(condition, "")],
        );

        Ok(self.syntax_node(
            "IfExprSyntax",
            self.range_from_offsets(if_start, body.end_byte()),
            vec![
                self.with_name(
                    self.token_with_range(
                        "keyword(SwiftSyntax.Keyword.if)",
                        self.range_from_offsets(if_start, if_end),
                    ),
                    "ifKeyword",
                ),
                self.with_name(conditions, "conditions"),
                self.with_name(self.code_block(body)?, "body"),
            ],
        ))
    }

    fn is_recoverable_if_case_error(&self, node: Node<'a>) -> bool {
        node.kind() == "ERROR" && self.text(node).trim_start().starts_with("if case")
    }

    fn recovered_if_case_expr(&self, node: Node<'a>) -> Result<Value> {
        let node_start = node.start_byte();
        let node_end = node.end_byte();
        let if_start = self.skip_horizontal_whitespace(node_start, node_end);
        let if_end = if_start + "if".len();
        let case_start = self.source[if_end..node_end]
            .find("case")
            .map(|offset| if_end + offset)
            .context("recovered if case expression is missing 'case'")?;
        let case_end = case_start + "case".len();
        let equal_start = self.source[case_end..node_end]
            .find('=')
            .map(|offset| case_end + offset)
            .context("recovered if case expression is missing '='")?;
        let left_brace_start = self.source[equal_start..node_end]
            .find('{')
            .map(|offset| equal_start + offset)
            .context("recovered if case expression body is missing '{'")?;
        let right_brace_start = self.source[left_brace_start..node_end]
            .rfind('}')
            .map(|offset| left_brace_start + offset)
            .context("recovered if case expression body is missing '}'")?;
        let (pattern_start, pattern_end) = self.trim_offsets(case_end, equal_start);
        let (value_start, value_end) = self.trim_offsets(equal_start + 1, left_brace_start);
        if pattern_start >= pattern_end || value_start >= value_end {
            bail!("recovered if case expression is missing pattern or initializer");
        }

        let pattern_condition = self.synthetic_matching_pattern_condition_from_offsets(
            (case_start, case_end),
            (pattern_start, pattern_end),
            (equal_start, equal_start + 1),
            (value_start, value_end),
        );
        let condition_element = self.syntax_node(
            "ConditionElementSyntax",
            self.range_from_offsets(case_start, end_offset(&pattern_condition)),
            vec![self.with_name(pattern_condition, "condition")],
        );
        let condition_element_list = self.syntax_node(
            "ConditionElementListSyntax",
            self.range_from_offsets(case_start, end_offset(&condition_element)),
            vec![self.with_name(condition_element, "")],
        );
        let body = if let Some(lambda) = self.first_descendant_kind(node, "lambda_literal") {
            self.code_block(lambda)?
        } else {
            self.synthetic_code_block_from_offsets(left_brace_start, right_brace_start + 1)
        };

        Ok(self.syntax_node(
            "IfExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.token_with_range(
                        "keyword(SwiftSyntax.Keyword.if)",
                        self.range_from_offsets(if_start, if_end),
                    ),
                    "ifKeyword",
                ),
                self.with_name(condition_element_list, "conditions"),
                self.with_name(body, "body"),
            ],
        ))
    }

    fn synthetic_matching_pattern_condition_from_offsets(
        &self,
        case_range: (usize, usize),
        pattern_range: (usize, usize),
        equal_range: (usize, usize),
        value_range: (usize, usize),
    ) -> Value {
        let (case_start, case_end) = case_range;
        let (pattern_start, pattern_end) = pattern_range;
        let (equal_start, equal_end) = equal_range;
        let (value_start, value_end) = value_range;
        let initializer = self.synthetic_initializer_clause_from_offsets(
            equal_start,
            equal_end,
            value_start,
            value_end,
        );
        self.syntax_node(
            "MatchingPatternConditionSyntax",
            self.range_from_offsets(case_start, end_offset(&initializer)),
            vec![
                self.with_name(
                    self.token_with_range(
                        "keyword(SwiftSyntax.Keyword.case)",
                        self.range_from_offsets(case_start, case_end),
                    ),
                    "caseKeyword",
                ),
                self.with_name(
                    self.synthetic_pattern_from_offsets(pattern_start, pattern_end),
                    "pattern",
                ),
                self.with_name(initializer, "initializer"),
            ],
        )
    }

    fn synthetic_initializer_clause_from_offsets(
        &self,
        equal_start: usize,
        equal_end: usize,
        value_start: usize,
        value_end: usize,
    ) -> Value {
        let value = self.synthetic_expr_from_offsets(value_start, value_end);
        self.syntax_node(
            "InitializerClauseSyntax",
            self.range_from_offsets(equal_start, end_offset(&value)),
            vec![
                self.with_name(
                    self.token_with_range("equal", self.range_from_offsets(equal_start, equal_end)),
                    "equal",
                ),
                self.with_name(value, "value"),
            ],
        )
    }

    fn synthetic_code_block_from_offsets(&self, start: usize, end: usize) -> Value {
        let left_brace_end = start + 1;
        let right_brace_start = end.saturating_sub(1);
        self.syntax_node(
            "CodeBlockSyntax",
            self.range_from_offsets(start, end),
            vec![
                self.with_name(
                    self.token_with_range(
                        "leftBrace",
                        self.range_from_offsets(start, left_brace_end),
                    ),
                    "leftBrace",
                ),
                self.with_name(
                    self.syntax_node(
                        "CodeBlockItemListSyntax",
                        self.point_range(left_brace_end),
                        Vec::new(),
                    ),
                    "statements",
                ),
                self.with_name(
                    self.token_with_range(
                        "rightBrace",
                        self.range_from_offsets(right_brace_start, end),
                    ),
                    "rightBrace",
                ),
            ],
        )
    }

    fn while_stmt(&self, node: Node<'a>) -> Result<Value> {
        let while_keyword = self
            .immediate_child_kind(node, "while")
            .context("while statement is missing 'while'")?;
        let left_brace = self
            .nearest_child_after(node, "{", while_keyword.end_byte())
            .context("while statement body is missing '{'")?;
        let right_brace = self
            .nearest_child_after(node, "}", left_brace.end_byte())
            .context("while statement body is missing '}'")?;
        let body_statements = named_children(node).find(|child| {
            child.kind() == "statements"
                && child.start_byte() >= left_brace.end_byte()
                && child.end_byte() <= right_brace.start_byte()
        });
        let conditions = self.condition_nodes(node, while_keyword, left_brace.start_byte());
        if conditions.is_empty() {
            bail!("while statement is missing condition");
        }

        Ok(self.syntax_node(
            "WhileStmtSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.token_for_node(while_keyword, "keyword(SwiftSyntax.Keyword.while)"),
                    "whileKeyword",
                ),
                self.with_name(
                    self.condition_element_list_from_nodes(
                        node,
                        &conditions,
                        while_keyword.end_byte(),
                    )?,
                    "conditions",
                ),
                self.with_name(
                    self.code_block_from_statements(body_statements, left_brace, right_brace)?,
                    "body",
                ),
            ],
        ))
    }

    fn repeat_stmt(&self, node: Node<'a>) -> Result<Value> {
        let repeat_keyword = self
            .immediate_child_kind(node, "repeat")
            .context("repeat statement is missing 'repeat'")?;
        let body_statements = named_children(node)
            .find(|child| child.kind() == "statements")
            .context("repeat statement is missing body statements")?;
        let left_brace = self
            .nearest_child_before(node, "{", body_statements.start_byte())
            .context("repeat statement body is missing '{'")?;
        let right_brace = self
            .nearest_child_after(node, "}", body_statements.end_byte())
            .context("repeat statement body is missing '}'")?;
        let while_keyword = self
            .immediate_child_kind(node, "while")
            .context("repeat statement is missing 'while'")?;
        let condition = self
            .field_child(node, "condition")
            .context("repeat statement is missing condition")?;

        Ok(self.syntax_node(
            "RepeatStmtSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.token_for_node(repeat_keyword, "keyword(SwiftSyntax.Keyword.repeat)"),
                    "repeatKeyword",
                ),
                self.with_name(
                    self.code_block_from_statements(
                        Some(body_statements),
                        left_brace,
                        right_brace,
                    )?,
                    "body",
                ),
                self.with_name(
                    self.token_for_node(while_keyword, "keyword(SwiftSyntax.Keyword.while)"),
                    "whileKeyword",
                ),
                self.with_name(self.expr(condition)?, "condition"),
            ],
        ))
    }

    fn do_stmt(&self, node: Node<'a>) -> Result<Value> {
        let do_keyword = self
            .immediate_child_kind(node, "do")
            .context("do statement is missing 'do'")?;
        let body_statements = named_children(node)
            .find(|child| child.kind() == "statements")
            .context("do statement is missing body statements")?;
        let left_brace = self
            .nearest_child_before(node, "{", body_statements.start_byte())
            .context("do statement body is missing '{'")?;
        let right_brace = self
            .nearest_child_after(node, "}", body_statements.end_byte())
            .context("do statement body is missing '}'")?;
        let catch_blocks = named_children(node)
            .filter(|child| child.kind() == "catch_block")
            .collect::<Vec<_>>();

        Ok(self.syntax_node(
            "DoStmtSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.token_for_node(do_keyword, "keyword(SwiftSyntax.Keyword.do)"),
                    "doKeyword",
                ),
                self.with_name(
                    self.code_block_from_statements(
                        Some(body_statements),
                        left_brace,
                        right_brace,
                    )?,
                    "body",
                ),
                self.with_name(
                    self.catch_clause_list_from_blocks(&catch_blocks, node.end_byte())?,
                    "catchClauses",
                ),
            ],
        ))
    }

    fn is_recoverable_do_error(&self, node: Node<'a>) -> bool {
        node.kind() == "ERROR"
            && (self.first_descendant_kind(node, "do_statement").is_some()
                || (self.text(node).trim_start().starts_with("do")
                    && self.first_descendant_kind(node, "statements").is_some()))
    }

    fn recovered_do_syntax_from_error(&self, node: Node<'a>) -> Result<Value> {
        if let Some(do_statement) = self.first_descendant_kind(node, "do_statement") {
            return self.do_stmt(do_statement);
        }
        self.recovered_do_expr_from_error(node)
    }

    fn recovered_do_expr_from_error(&self, node: Node<'a>) -> Result<Value> {
        let text = self.text(node);
        let do_relative = text
            .find("do")
            .context("recovered do expression is missing 'do'")?;
        let do_start = node.start_byte() + do_relative;
        let body_statements = self
            .first_descendant_kind(node, "statements")
            .context("recovered do expression is missing body statements")?;
        let left_brace = self
            .nearest_child_before(node, "{", body_statements.start_byte())
            .context("recovered do expression body is missing '{'")?;
        let right_brace = self
            .nearest_child_after(node, "}", body_statements.end_byte())
            .context("recovered do expression body is missing '}'")?;
        Ok(self.syntax_node(
            "DoExprSyntax",
            self.range_from_offsets(do_start, right_brace.end_byte()),
            vec![
                self.with_name(
                    self.token_with_range(
                        "keyword(SwiftSyntax.Keyword.do)",
                        self.range_from_offsets(do_start, do_start + "do".len()),
                    ),
                    "doKeyword",
                ),
                self.with_name(
                    self.code_block_from_statements(
                        Some(body_statements),
                        left_brace,
                        right_brace,
                    )?,
                    "body",
                ),
                self.with_name(
                    self.empty_collection("CatchClauseListSyntax", right_brace.end_byte()),
                    "catchClauses",
                ),
            ],
        ))
    }

    fn skip_do_cast_artifacts(&self, nodes: &[Node<'a>], start_index: usize) -> usize {
        let mut index = start_index;
        if nodes
            .get(index)
            .copied()
            .is_some_and(|node| self.is_do_cast_artifact_start(node))
        {
            index += 1;
            if nodes
                .get(index)
                .copied()
                .is_some_and(|node| node.kind() == "ERROR")
            {
                index += 1;
            }
        }
        index
    }

    fn is_do_cast_artifact_start(&self, node: Node<'a>) -> bool {
        if node.kind() == "statements" {
            return named_children(node).next().is_some_and(|child| {
                child.kind() == "simple_identifier" && self.text(child) == "as"
            });
        }
        node.kind() == "simple_identifier" && self.text(node) == "as"
    }

    fn is_do_expr_call(&self, node: Node<'a>) -> bool {
        self.do_expr_parts(node).is_some()
    }

    fn do_expr(&self, node: Node<'a>) -> Result<Value> {
        let (do_keyword, body, catch_clauses, end_offset) = self
            .do_expr_parts(node)
            .context("do expression is missing body")?;
        Ok(self.syntax_node(
            "DoExprSyntax",
            self.range_from_offsets(do_keyword.start_byte(), end_offset),
            vec![
                self.with_name(
                    self.token_for_node(do_keyword, "keyword(SwiftSyntax.Keyword.do)"),
                    "doKeyword",
                ),
                self.with_name(self.code_block(body)?, "body"),
                self.with_name(catch_clauses, "catchClauses"),
            ],
        ))
    }

    fn do_expr_parts(&self, node: Node<'a>) -> Option<(Node<'a>, Node<'a>, Value, usize)> {
        if let Some((do_keyword, body)) = self.do_call_body_parts(node) {
            return Some((
                do_keyword,
                body,
                self.empty_collection("CatchClauseListSyntax", node.end_byte()),
                node.end_byte(),
            ));
        }
        self.do_expr_with_catch_parts(node)
    }

    fn do_expr_with_catch_parts(
        &self,
        node: Node<'a>,
    ) -> Option<(Node<'a>, Node<'a>, Value, usize)> {
        if node.kind() != "call_expression" {
            return None;
        }
        let callee = named_children(node)
            .find(|child| child.kind() != "call_suffix" && child.kind() != "ERROR")?;
        let (do_keyword, body) = self.do_call_body_parts(callee)?;
        let catch_marker = named_children(node).find(|child| {
            child.kind() == "ERROR"
                && self
                    .first_descendant_any_kind(*child, "simple_identifier")
                    .is_some_and(|identifier| self.text(identifier) == "catch")
        })?;
        let suffix = self.immediate_named_child_kind(node, "call_suffix")?;
        let catch_body = named_children(suffix).find(|child| child.kind() == "lambda_literal")?;
        let catch_clause = self
            .catch_clause_from_recovered_expr(catch_marker, catch_body)
            .ok()?;
        let catch_clauses = self.syntax_node(
            "CatchClauseListSyntax",
            self.range_from_offsets(catch_marker.start_byte(), catch_body.end_byte()),
            vec![self.with_name(catch_clause, "")],
        );
        Some((do_keyword, body, catch_clauses, node.end_byte()))
    }

    fn do_call_body_parts(&self, node: Node<'a>) -> Option<(Node<'a>, Node<'a>)> {
        if node.kind() != "call_expression" {
            return None;
        }
        let callee = named_children(node).find(|child| child.kind() != "call_suffix")?;
        if callee.kind() != "simple_identifier" || self.text(callee) != "do" {
            return None;
        }
        let suffix = self.immediate_named_child_kind(node, "call_suffix")?;
        let body = named_children(suffix).find(|child| child.kind() == "lambda_literal")?;
        Some((callee, body))
    }

    fn is_return_do_expr_call(&self, node: Node<'a>) -> bool {
        self.return_do_expr_parts(node).is_some()
    }

    fn recovered_return_do_stmt(&self, node: Node<'a>) -> Result<Value> {
        let (return_keyword, do_keyword, body) = self
            .return_do_expr_parts(node)
            .context("return do statement is missing do expression")?;
        let do_expr = self.syntax_node(
            "DoExprSyntax",
            self.range_from_offsets(do_keyword.start_byte(), body.end_byte()),
            vec![
                self.with_name(
                    self.token_for_node(do_keyword, "keyword(SwiftSyntax.Keyword.do)"),
                    "doKeyword",
                ),
                self.with_name(self.code_block(body)?, "body"),
                self.with_name(
                    self.empty_collection("CatchClauseListSyntax", body.end_byte()),
                    "catchClauses",
                ),
            ],
        );
        Ok(self.syntax_node(
            "ReturnStmtSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.token_for_node(return_keyword, "keyword(SwiftSyntax.Keyword.return)"),
                    "returnKeyword",
                ),
                self.with_name(do_expr, "expression"),
            ],
        ))
    }

    fn return_do_expr_parts(&self, node: Node<'a>) -> Option<(Node<'a>, Node<'a>, Node<'a>)> {
        if node.kind() != "call_expression" {
            return None;
        }
        let return_keyword = named_children(node)
            .find(|child| child.kind() == "simple_identifier" && self.text(*child) == "return")?;
        let do_keyword = named_children(node)
            .find(|child| child.kind() == "ERROR" && self.text(*child).trim() == "do")?;
        let suffix = self.immediate_named_child_kind(node, "call_suffix")?;
        let body = named_children(suffix).find(|child| child.kind() == "lambda_literal")?;
        Some((return_keyword, do_keyword, body))
    }

    fn is_recoverable_return_call(&self, node: Node<'a>) -> bool {
        self.return_call_parts(node).is_some()
    }

    fn recovered_return_call_stmt(&self, node: Node<'a>) -> Result<Value> {
        let (return_keyword, callee, suffix, trailing_closure) = self
            .return_call_parts(node)
            .context("return call statement is missing trailing closure")?;
        let (callee_start, callee_end) = self.trim_offsets(callee.start_byte(), callee.end_byte());
        if callee_start >= callee_end {
            bail!("return call statement is missing callee");
        }
        let expression = self.syntax_node(
            "FunctionCallExprSyntax",
            self.range_from_offsets(callee_start, trailing_closure.end_byte()),
            vec![
                self.with_name(
                    self.synthetic_expr_from_offsets(callee_start, callee_end),
                    "calledExpression",
                ),
                self.with_name(
                    self.empty_collection("LabeledExprListSyntax", callee_end),
                    "arguments",
                ),
                self.with_name(self.closure_expr(trailing_closure)?, "trailingClosure"),
                self.with_name(
                    self.additional_trailing_closure_list(
                        suffix,
                        &self.trailing_closure_nodes(suffix),
                        trailing_closure.end_byte(),
                    )?,
                    "additionalTrailingClosures",
                ),
            ],
        );
        Ok(self.syntax_node(
            "ReturnStmtSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.token_for_node(return_keyword, "keyword(SwiftSyntax.Keyword.return)"),
                    "returnKeyword",
                ),
                self.with_name(expression, "expression"),
            ],
        ))
    }

    fn return_call_parts(
        &self,
        node: Node<'a>,
    ) -> Option<(Node<'a>, Node<'a>, Node<'a>, Node<'a>)> {
        if node.kind() != "call_expression" {
            return None;
        }
        let return_keyword = named_children(node)
            .find(|child| child.kind() == "simple_identifier" && self.text(*child) == "return")?;
        let suffix = self.immediate_named_child_kind(node, "call_suffix")?;
        let trailing_closure = self.trailing_closure_nodes(suffix).first().copied()?;
        let callee = named_children(node).find(|child| {
            child.start_byte() >= return_keyword.end_byte()
                && child.end_byte() <= suffix.start_byte()
                && child.kind() != "call_suffix"
                && self.text(*child).trim() != "return"
        })?;
        Some((return_keyword, callee, suffix, trailing_closure))
    }

    fn catch_clause_list_from_blocks(
        &self,
        catch_blocks: &[Node<'a>],
        fallback_offset: usize,
    ) -> Result<Value> {
        let mut clauses = Vec::new();
        for catch_block in catch_blocks {
            clauses.push(self.with_name(self.catch_clause_from_block(*catch_block)?, ""));
        }
        let range = self.covering_range_or_point(&clauses, fallback_offset);
        Ok(self.syntax_node("CatchClauseListSyntax", range, clauses))
    }

    fn catch_clause_from_block(&self, node: Node<'a>) -> Result<Value> {
        let catch_keyword = self
            .first_descendant_any_kind(node, "catch_keyword")
            .context("catch clause is missing 'catch'")?;
        let body_statements = named_children(node).find(|child| child.kind() == "statements");
        let left_brace = if let Some(body_statements) = body_statements {
            self.nearest_child_before(node, "{", body_statements.start_byte())
        } else {
            self.nearest_child_after(node, "{", catch_keyword.end_byte())
        }
        .context("catch clause body is missing '{'")?;
        let right_brace = if let Some(body_statements) = body_statements {
            self.nearest_child_after(node, "}", body_statements.end_byte())
        } else {
            children(node)
                .filter(|child| child.kind() == "}" && child.start_byte() >= left_brace.end_byte())
                .last()
        }
        .context("catch clause body is missing '}'")?;

        Ok(self.syntax_node(
            "CatchClauseSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.token_for_node(catch_keyword, "keyword(SwiftSyntax.Keyword.catch)"),
                    "catchKeyword",
                ),
                self.with_name(
                    self.catch_item_list_from_block(node, catch_keyword.end_byte())?,
                    "catchItems",
                ),
                self.with_name(
                    self.code_block_from_statements(body_statements, left_brace, right_brace)?,
                    "body",
                ),
            ],
        ))
    }

    fn catch_item_list_from_block(&self, node: Node<'a>, fallback_offset: usize) -> Result<Value> {
        let Some(where_clause_node) = self.immediate_named_child_kind(node, "where_clause") else {
            return Ok(self.empty_collection("CatchItemListSyntax", fallback_offset));
        };
        let where_clause = self.where_clause(where_clause_node)?;
        let catch_item = self.syntax_node(
            "CatchItemSyntax",
            self.range_for_node(where_clause_node),
            vec![self.with_name(where_clause, "whereClause")],
        );
        Ok(self.syntax_node(
            "CatchItemListSyntax",
            self.range_for_node(where_clause_node),
            vec![self.with_name(catch_item, "")],
        ))
    }

    fn where_clause(&self, node: Node<'a>) -> Result<Value> {
        let where_keyword = self
            .first_descendant_any_kind(node, "where_keyword")
            .context("where clause is missing 'where'")?;
        let condition = named_children(node)
            .find(|child| {
                child.start_byte() >= where_keyword.end_byte() && is_expression_like_node(*child)
            })
            .context("where clause is missing condition")?;
        Ok(self.syntax_node(
            "WhereClauseSyntax",
            self.range_from_offsets(where_keyword.start_byte(), condition.end_byte()),
            vec![
                self.with_name(
                    self.token_for_node(where_keyword, "keyword(SwiftSyntax.Keyword.where)"),
                    "whereKeyword",
                ),
                self.with_name(self.expr(condition)?, "condition"),
            ],
        ))
    }

    fn catch_clause_from_recovered_expr(
        &self,
        catch_marker: Node<'a>,
        body: Node<'a>,
    ) -> Result<Value> {
        let catch_keyword = self
            .first_descendant_any_kind(catch_marker, "simple_identifier")
            .unwrap_or(catch_marker);
        Ok(self.syntax_node(
            "CatchClauseSyntax",
            self.range_from_offsets(catch_marker.start_byte(), body.end_byte()),
            vec![
                self.with_name(
                    self.token_for_node(catch_keyword, "keyword(SwiftSyntax.Keyword.catch)"),
                    "catchKeyword",
                ),
                self.with_name(
                    self.empty_collection("CatchItemListSyntax", catch_keyword.end_byte()),
                    "catchItems",
                ),
                self.with_name(self.code_block(body)?, "body"),
            ],
        ))
    }

    fn is_defer_stmt(&self, node: Node<'a>) -> bool {
        if node.kind() != "call_expression" {
            return false;
        }
        let Some(callee) = named_children(node).next() else {
            return false;
        };
        callee.kind() == "simple_identifier"
            && self.text(callee) == "defer"
            && self.first_descendant_kind(node, "lambda_literal").is_some()
    }

    fn defer_stmt(&self, node: Node<'a>) -> Result<Value> {
        let defer_keyword = named_children(node)
            .find(|child| child.kind() == "simple_identifier" && self.text(*child) == "defer")
            .context("defer statement is missing 'defer'")?;
        let body = self
            .first_descendant_kind(node, "lambda_literal")
            .context("defer statement is missing a body")?;

        Ok(self.syntax_node(
            "DeferStmtSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.token_for_node(defer_keyword, "keyword(SwiftSyntax.Keyword.defer)"),
                    "deferKeyword",
                ),
                self.with_name(self.code_block(body)?, "body"),
            ],
        ))
    }

    fn guard_stmt(&self, node: Node<'a>) -> Result<Value> {
        let guard_keyword = self
            .immediate_child_kind(node, "guard")
            .context("guard statement is missing 'guard'")?;
        let else_keyword = self
            .immediate_child_kind(node, "else")
            .context("guard statement is missing 'else'")?;
        let left_brace = children(node)
            .find(|child| child.kind() == "{" && child.start_byte() > else_keyword.end_byte())
            .context("guard else body is missing '{'")?;
        let statements = named_children(node)
            .filter(|child| child.kind() == "statements")
            .find(|child| child.start_byte() > left_brace.end_byte());
        let right_brace = children(node)
            .filter(|child| child.kind() == "}" && child.start_byte() >= left_brace.end_byte())
            .last()
            .context("guard else body is missing '}'")?;

        Ok(self.syntax_node(
            "GuardStmtSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.token_for_node(guard_keyword, "keyword(SwiftSyntax.Keyword.guard)"),
                    "guardKeyword",
                ),
                self.with_name(
                    self.guard_condition_element_list(node, guard_keyword, else_keyword)?,
                    "conditions",
                ),
                self.with_name(
                    self.token_for_node(else_keyword, "keyword(SwiftSyntax.Keyword.else)"),
                    "elseKeyword",
                ),
                self.with_name(
                    self.code_block_from_statements(statements, left_brace, right_brace)?,
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

    fn guard_condition_element_list(
        &self,
        node: Node<'a>,
        guard_keyword: Node<'a>,
        else_keyword: Node<'a>,
    ) -> Result<Value> {
        let condition_nodes = named_children(node)
            .filter(|child| {
                child.start_byte() > guard_keyword.end_byte()
                    && child.end_byte() <= else_keyword.start_byte()
            })
            .collect::<Vec<_>>();
        if condition_nodes.is_empty() {
            bail!("guard statement is missing conditions");
        }
        self.condition_element_list_from_nodes(node, &condition_nodes, guard_keyword.end_byte())
    }

    fn condition_nodes(
        &self,
        node: Node<'a>,
        keyword: Node<'a>,
        condition_end: usize,
    ) -> Vec<Node<'a>> {
        let conditions = self
            .field_children(node, "condition")
            .into_iter()
            .filter(|child| child.is_named() && child.end_byte() <= condition_end)
            .collect::<Vec<_>>();
        if conditions.is_empty() {
            self.first_named_condition(node, keyword, condition_end)
                .into_iter()
                .collect()
        } else {
            conditions
        }
    }

    fn condition_element_list_from_nodes(
        &self,
        parent: Node<'a>,
        condition_nodes: &[Node<'a>],
        fallback_offset: usize,
    ) -> Result<Value> {
        let mut elements = Vec::new();
        let mut index = 0;
        while index < condition_nodes.len() {
            let condition = condition_nodes[index];
            if let Some((element, next_index)) =
                self.matching_pattern_condition_element(parent, condition_nodes, index)?
            {
                elements.push(self.with_name(element, ""));
                index = next_index;
                continue;
            }
            if condition.kind() == "value_binding_pattern"
                || self
                    .first_descendant_kind(condition, "value_binding_pattern")
                    .is_some()
                || self
                    .optional_binding_specifier_before(parent, condition)
                    .is_some()
            {
                let (element, next_index) =
                    self.optional_binding_condition_element(parent, condition_nodes, index)?;
                elements.push(self.with_name(element, ""));
                index = next_index;
                continue;
            }
            let trailing_comma = self.trailing_delimiter(parent, condition, ",");
            elements.push(self.with_name(self.condition_element(condition, trailing_comma)?, ""));
            index += 1;
        }

        let range = self.covering_range_or_point(&elements, fallback_offset);
        Ok(self.syntax_node("ConditionElementListSyntax", range, elements))
    }

    fn condition_element(
        &self,
        condition: Node<'a>,
        trailing_comma: Option<Node<'a>>,
    ) -> Result<Value> {
        let condition_value = if condition.kind() == "availability_condition" {
            self.availability_condition(condition)?
        } else {
            self.expr(condition)?
        };
        let mut children = vec![self.with_name(condition_value, "condition")];
        if let Some(comma) = trailing_comma {
            children.push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
        }
        let end = trailing_comma.map_or(condition.end_byte(), |comma| comma.end_byte());
        Ok(self.syntax_node(
            "ConditionElementSyntax",
            self.range_from_offsets(condition.start_byte(), end),
            children,
        ))
    }

    fn optional_binding_condition_element(
        &self,
        parent: Node<'a>,
        condition_nodes: &[Node<'a>],
        index: usize,
    ) -> Result<(Value, usize)> {
        let binding = condition_nodes[index];
        let binding_specifier = children(binding)
            .find(|child| matches!(child.kind(), "let" | "var"))
            .or_else(|| self.first_descendant_any_kind(binding, "let"))
            .or_else(|| self.first_descendant_any_kind(binding, "var"))
            .or_else(|| self.optional_binding_specifier_before(parent, binding))
            .context("optional binding condition is missing binding specifier")?;
        let condition_end = self.optional_binding_condition_boundary(parent, binding_specifier);
        let equal = children(parent).find(|child| {
            child.kind() == "="
                && child.start_byte() > binding_specifier.end_byte()
                && child.start_byte() < condition_end
        });
        let type_annotation = condition_nodes.iter().copied().find(|child| {
            child.kind() == "type_annotation"
                && child.start_byte() > binding_specifier.end_byte()
                && equal
                    .map(|equal| child.end_byte() < equal.start_byte())
                    .unwrap_or_else(|| child.end_byte() < condition_end)
        });
        let (value, next_index, pattern_boundary, trailing_delimiter_node) = if let Some(equal) =
            equal
        {
            let value_index = condition_nodes
                .iter()
                .enumerate()
                .skip(index + 1)
                .find(|(_, child)| {
                    child.start_byte() > equal.end_byte()
                        && child.start_byte() < condition_end
                        && is_expression_like_node(**child)
                })
                .map(|(index, _)| index)
                .context("optional binding condition is missing an initializer")?;
            let value = condition_nodes[value_index];
            (
                Some((equal, value)),
                value_index + 1,
                type_annotation
                    .map(|annotation| annotation.start_byte())
                    .unwrap_or_else(|| equal.start_byte()),
                value,
            )
        } else {
            let pattern_node = self
                .optional_binding_shorthand_pattern_node(parent, binding_specifier, condition_end)
                .context("optional binding condition is missing a pattern")?;
            (
                None,
                index + 1,
                type_annotation
                    .map(|annotation| annotation.start_byte())
                    .unwrap_or_else(|| pattern_node.end_byte()),
                pattern_node,
            )
        };
        let (pattern_start, pattern_end) =
            self.trim_offsets(binding_specifier.end_byte(), pattern_boundary);
        if pattern_start >= pattern_end {
            bail!("optional binding condition is missing a pattern");
        }
        let trailing_comma = self.trailing_delimiter(parent, trailing_delimiter_node, ",");

        let mut optional_children = vec![
            self.with_name(
                self.token_for_node(
                    binding_specifier,
                    &format!(
                        "keyword(SwiftSyntax.Keyword.{})",
                        self.text(binding_specifier)
                    ),
                ),
                "bindingSpecifier",
            ),
            self.with_name(
                self.synthetic_bound_pattern_from_offsets(pattern_start, pattern_end),
                "pattern",
            ),
        ];
        if let Some(type_annotation) = type_annotation {
            optional_children
                .push(self.with_name(self.type_annotation(type_annotation)?, "typeAnnotation"));
        }
        if let Some((equal, value)) = value {
            optional_children
                .push(self.with_name(self.initializer_clause(equal, value)?, "initializer"));
        }
        let optional_end = end_offset(optional_children.last().unwrap());
        let optional_binding = self.syntax_node(
            "OptionalBindingConditionSyntax",
            self.range_from_offsets(binding.start_byte(), optional_end),
            optional_children,
        );

        let mut element_children = vec![self.with_name(optional_binding, "condition")];
        if let Some(comma) = trailing_comma {
            element_children
                .push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
        }
        let element_end = trailing_comma.map_or(optional_end, |comma| comma.end_byte());
        Ok((
            self.syntax_node(
                "ConditionElementSyntax",
                self.range_from_offsets(binding.start_byte(), element_end),
                element_children,
            ),
            next_index,
        ))
    }

    fn optional_binding_condition_boundary(
        &self,
        parent: Node<'a>,
        binding_specifier: Node<'a>,
    ) -> usize {
        let body_start = self
            .nearest_child_after(parent, "{", binding_specifier.end_byte())
            .map(|brace| brace.start_byte())
            .unwrap_or_else(|| parent.end_byte());
        self.top_level_commas(binding_specifier.end_byte(), body_start)
            .into_iter()
            .next()
            .unwrap_or(body_start)
    }

    fn optional_binding_specifier_before(
        &self,
        parent: Node<'a>,
        condition: Node<'a>,
    ) -> Option<Node<'a>> {
        let specifier =
            self.last_descendant_kind_before(parent, &["let", "var"], condition.start_byte())?;
        self.top_level_commas(specifier.end_byte(), condition.start_byte())
            .is_empty()
            .then_some(specifier)
    }

    fn optional_binding_shorthand_pattern_node(
        &self,
        parent: Node<'a>,
        binding_specifier: Node<'a>,
        condition_end: usize,
    ) -> Option<Node<'a>> {
        named_children(parent).find(|child| {
            child.start_byte() >= binding_specifier.end_byte()
                && child.end_byte() <= condition_end
                && !matches!(
                    child.kind(),
                    "type_annotation"
                        | "statements"
                        | "else"
                        | "catch_block"
                        | "value_binding_pattern"
                )
                && !is_trivia_node(*child)
        })
    }

    fn matching_pattern_condition_element(
        &self,
        parent: Node<'a>,
        condition_nodes: &[Node<'a>],
        index: usize,
    ) -> Result<Option<(Value, usize)>> {
        let first_condition = condition_nodes[index];
        let Some(case_keyword) = self.case_keyword_before(parent, first_condition) else {
            return Ok(None);
        };
        let (equal, value, next_index, delimiter_node) = if first_condition.kind() == "assignment" {
            (
                self.immediate_child_kind(first_condition, "=")
                    .context("matching pattern condition is missing '='")?,
                self.field_child(first_condition, "result")
                    .context("matching pattern condition is missing initializer")?,
                index + 1,
                first_condition,
            )
        } else {
            let equal = children(parent)
                .find(|child| {
                    child.kind() == "="
                        && child.start_byte() > case_keyword.end_byte()
                        && child.start_byte() >= first_condition.start_byte()
                })
                .context("matching pattern condition is missing '='")?;
            let value_index = condition_nodes
                .iter()
                .enumerate()
                .skip(index)
                .find(|(_, child)| child.start_byte() > equal.end_byte())
                .map(|(index, _)| index)
                .context("matching pattern condition is missing initializer")?;
            (
                equal,
                condition_nodes[value_index],
                value_index + 1,
                condition_nodes[value_index],
            )
        };
        let initializer = self.initializer_clause(equal, value)?;
        let (pattern_start, pattern_end) =
            self.trim_offsets(case_keyword.end_byte(), equal.start_byte());
        if pattern_start >= pattern_end {
            bail!("matching pattern condition is missing pattern");
        }
        let pattern_condition_end = end_offset(&initializer);
        let pattern_condition = self.syntax_node(
            "MatchingPatternConditionSyntax",
            self.range_from_offsets(case_keyword.start_byte(), pattern_condition_end),
            vec![
                self.with_name(
                    self.token_with_range(
                        "keyword(SwiftSyntax.Keyword.case)",
                        self.range_from_offsets(case_keyword.start_byte(), case_keyword.end_byte()),
                    ),
                    "caseKeyword",
                ),
                self.with_name(
                    self.synthetic_pattern_from_offsets(pattern_start, pattern_end),
                    "pattern",
                ),
                self.with_name(initializer, "initializer"),
            ],
        );

        let trailing_comma = self.trailing_delimiter(parent, delimiter_node, ",");
        let mut element_children = vec![self.with_name(pattern_condition, "condition")];
        if let Some(comma) = trailing_comma {
            element_children
                .push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
        }
        let element_end = trailing_comma.map_or(pattern_condition_end, |comma| comma.end_byte());
        Ok(Some((
            self.syntax_node(
                "ConditionElementSyntax",
                self.range_from_offsets(case_keyword.start_byte(), element_end),
                element_children,
            ),
            next_index,
        )))
    }

    fn case_keyword_before(&self, parent: Node<'a>, condition: Node<'a>) -> Option<Node<'a>> {
        let last_comma_end = self.source[parent.start_byte()..condition.start_byte()]
            .rfind(',')
            .map(|offset| parent.start_byte() + offset + 1)
            .unwrap_or(parent.start_byte());
        children(parent)
            .filter(|child| child.end_byte() <= condition.start_byte())
            .filter(|child| child.start_byte() >= last_comma_end)
            .filter(|child| self.text(*child).trim() == "case")
            .last()
    }

    fn availability_condition(&self, node: Node<'a>) -> Result<Value> {
        let (start, end) = self.trim_offsets(node.start_byte(), node.end_byte());
        let text = &self.source[start..end];
        let left_paren_start = start
            + text
                .find('(')
                .context("availability condition is missing '('")?;
        let right_paren_start = start
            + text
                .rfind(')')
                .context("availability condition is missing ')'")?;
        let keyword_text = &self.source[start..left_paren_start];
        let keyword_kind = match keyword_text {
            "#available" => "poundAvailable",
            "#unavailable" => "poundUnavailable",
            other => bail!("unsupported availability keyword '{other}'"),
        };
        Ok(self.syntax_node(
            "AvailabilityConditionSyntax",
            self.range_from_offsets(start, end),
            vec![
                self.with_name(
                    self.token_with_range(
                        keyword_kind,
                        self.range_from_offsets(start, left_paren_start),
                    ),
                    "availabilityKeyword",
                ),
                self.with_name(
                    self.token_with_range(
                        "leftParen",
                        self.range_from_offsets(left_paren_start, left_paren_start + 1),
                    ),
                    "leftParen",
                ),
                self.with_name(
                    self.availability_argument_list(left_paren_start + 1, right_paren_start)?,
                    "availabilityArguments",
                ),
                self.with_name(
                    self.token_with_range(
                        "rightParen",
                        self.range_from_offsets(right_paren_start, right_paren_start + 1),
                    ),
                    "rightParen",
                ),
            ],
        ))
    }

    fn availability_argument_list(&self, start: usize, end: usize) -> Result<Value> {
        let mut elements = Vec::new();
        let mut argument_start = start;
        let mut cursor = start;
        while cursor < end {
            if self.source.as_bytes()[cursor] == b',' {
                if let Some(argument) =
                    self.availability_argument(argument_start, cursor, Some((cursor, cursor + 1)))?
                {
                    elements.push(self.with_name(argument, ""));
                }
                argument_start = cursor + 1;
            }
            cursor += 1;
        }
        if let Some(argument) = self.availability_argument(argument_start, end, None)? {
            elements.push(self.with_name(argument, ""));
        }
        let range = self.covering_range_or_point(&elements, start);
        Ok(self.syntax_node("AvailabilityArgumentListSyntax", range, elements))
    }

    fn availability_argument(
        &self,
        start: usize,
        end: usize,
        comma: Option<(usize, usize)>,
    ) -> Result<Option<Value>> {
        let (argument_start, argument_end) = self.trim_offsets(start, end);
        if argument_start >= argument_end {
            return Ok(None);
        }
        let mut children = vec![self.with_name(
            self.availability_argument_value(argument_start, argument_end)?,
            "argument",
        )];
        if let Some((comma_start, comma_end)) = comma {
            children.push(self.with_name(
                self.token_with_range("comma", self.range_from_offsets(comma_start, comma_end)),
                "trailingComma",
            ));
        }
        let node_end = comma.map_or(argument_end, |(_, comma_end)| comma_end);
        Ok(Some(self.syntax_node(
            "AvailabilityArgumentSyntax",
            self.range_from_offsets(argument_start, node_end),
            children,
        )))
    }

    fn availability_argument_value(&self, start: usize, end: usize) -> Result<Value> {
        let text = &self.source[start..end];
        if text == "*" {
            return Ok(
                self.token_with_range("binaryOperator(\"*\")", self.range_from_offsets(start, end))
            );
        }
        let Some(platform_end) = self.find_horizontal_whitespace(start, end) else {
            return Ok(self.token_with_range(
                &format!("identifier({})", quoted_text(text)),
                self.range_from_offsets(start, end),
            ));
        };
        let version_start = self.skip_horizontal_whitespace(platform_end, end);
        let mut children = vec![self.with_name(
            self.token_with_range(
                &format!(
                    "identifier({})",
                    quoted_text(&self.source[start..platform_end])
                ),
                self.range_from_offsets(start, platform_end),
            ),
            "platform",
        )];
        if version_start < end {
            children.push(self.with_name(self.version_tuple(version_start, end)?, "version"));
        }
        Ok(self.syntax_node(
            "PlatformVersionSyntax",
            self.range_from_offsets(start, end),
            children,
        ))
    }

    fn version_tuple(&self, start: usize, end: usize) -> Result<Value> {
        let text = &self.source[start..end];
        let major_len = text.find('.').unwrap_or(text.len());
        let major_end = start + major_len;
        let mut components = Vec::new();
        let mut cursor = major_end;
        while cursor < end {
            if self.source.as_bytes()[cursor] != b'.' {
                break;
            }
            let number_start = cursor + 1;
            let mut number_end = number_start;
            while number_end < end && self.source.as_bytes()[number_end].is_ascii_digit() {
                number_end += 1;
            }
            components.push(self.with_name(
                self.syntax_node(
                    "VersionComponentSyntax",
                    self.range_from_offsets(cursor, number_end),
                    vec![
                        self.with_name(
                            self.token_with_range(
                                "period",
                                self.range_from_offsets(cursor, cursor + 1),
                            ),
                            "period",
                        ),
                        self.with_name(
                            self.token_with_range(
                                &format!(
                                    "integerLiteral({})",
                                    quoted_text(&self.source[number_start..number_end])
                                ),
                                self.range_from_offsets(number_start, number_end),
                            ),
                            "number",
                        ),
                    ],
                ),
                "",
            ));
            cursor = number_end;
        }
        Ok(self.syntax_node(
            "VersionTupleSyntax",
            self.range_from_offsets(start, end),
            vec![
                self.with_name(
                    self.token_with_range(
                        &format!(
                            "integerLiteral({})",
                            quoted_text(&self.source[start..major_end])
                        ),
                        self.range_from_offsets(start, major_end),
                    ),
                    "major",
                ),
                self.with_name(
                    self.syntax_node(
                        "VersionComponentListSyntax",
                        self.covering_range_or_point(&components, major_end),
                        components,
                    ),
                    "components",
                ),
            ],
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

    fn consume_expr(&self, node: Node<'a>) -> Result<Value> {
        let keyword = self
            .immediate_child_kind(node, "consume")
            .or_else(|| self.immediate_child_kind(node, "_move"))
            .context("consume expression is missing keyword")?;
        let expression = self
            .expression_field_child(node, "expr")
            .or_else(|| named_children(node).find(|child| child.start_byte() >= keyword.end_byte()))
            .context("consume expression is missing expression")?;
        self.ownership_expr_from_parts("ConsumeExprSyntax", "consumeKeyword", keyword, expression)
    }

    fn borrow_expr_from_parts(&self, keyword: Node<'a>, expression: Node<'a>) -> Result<Value> {
        self.ownership_expr_from_parts("BorrowExprSyntax", "borrowKeyword", keyword, expression)
    }

    fn ownership_expr_from_parts(
        &self,
        node_type: &str,
        keyword_name: &str,
        keyword: Node<'a>,
        expression: Node<'a>,
    ) -> Result<Value> {
        Ok(self.syntax_node(
            node_type,
            self.range_from_offsets(keyword.start_byte(), expression.end_byte()),
            vec![
                self.with_name(
                    self.token_for_node(
                        keyword,
                        &format!("keyword(SwiftSyntax.Keyword.{})", self.text(keyword)),
                    ),
                    keyword_name,
                ),
                self.with_name(self.expr(expression)?, "expression"),
            ],
        ))
    }

    fn constructor_expr(&self, node: Node<'a>) -> Result<Value> {
        let constructed_type = self
            .field_child(node, "constructed_type")
            .context("constructor expression is missing constructed type")?;
        let suffix = self
            .immediate_named_child_kind(node, "constructor_suffix")
            .context("constructor expression is missing constructor suffix")?;
        let value_arguments = self
            .immediate_named_child_kind(suffix, "value_arguments")
            .context("constructor expression is missing value arguments")?;
        let left_paren = self
            .immediate_child_kind(value_arguments, "(")
            .context("constructor arguments are missing '('")?;
        let right_paren = self
            .immediate_child_kind(value_arguments, ")")
            .context("constructor arguments are missing ')'")?;

        Ok(self.syntax_node(
            "FunctionCallExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(
                    self.constructed_type_expr(constructed_type)?,
                    "calledExpression",
                ),
                self.with_name(self.token_for_node(left_paren, "leftParen"), "leftParen"),
                self.with_name(
                    self.labeled_expr_list(value_arguments, left_paren, right_paren)?,
                    "arguments",
                ),
                self.with_name(self.token_for_node(right_paren, "rightParen"), "rightParen"),
                self.with_name(
                    self.empty_collection(
                        "MultipleTrailingClosureElementListSyntax",
                        node.end_byte(),
                    ),
                    "additionalTrailingClosures",
                ),
            ],
        ))
    }

    fn constructed_type_expr(&self, node: Node<'a>) -> Result<Value> {
        let base = self.constructed_type_base_expr(node)?;
        let Some(type_arguments) = self.immediate_named_child_kind(node, "type_arguments") else {
            return Ok(base);
        };
        Ok(self.syntax_node(
            "GenericSpecializationExprSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(base, "expression"),
                self.with_name(
                    self.generic_argument_clause(type_arguments)?,
                    "genericArgumentClause",
                ),
            ],
        ))
    }

    fn constructed_type_base_expr(&self, node: Node<'a>) -> Result<Value> {
        if node.kind() == "array_type" {
            return self.empty_array_expr_from_type(node);
        }
        let names = self.immediate_type_identifiers(node);
        let first = names
            .first()
            .copied()
            .context("constructed type is missing type identifier")?;
        let mut current = self.decl_reference_expr(first);
        let mut previous = first;
        for name in names.into_iter().skip(1) {
            let period = self
                .children_between(node, previous.end_byte(), name.start_byte())
                .into_iter()
                .find(|child| child.kind() == ".")
                .context("constructed member type is missing '.'")?;
            current = self.syntax_node(
                "MemberAccessExprSyntax",
                self.range_from_offsets(first.start_byte(), name.end_byte()),
                vec![
                    self.with_name(current, "base"),
                    self.with_name(self.token_for_node(period, "period"), "period"),
                    self.with_name(self.decl_reference_expr(name), "declName"),
                ],
            );
            previous = name;
        }
        Ok(current)
    }

    fn generic_argument_clause(&self, node: Node<'a>) -> Result<Value> {
        let left_angle = self
            .immediate_child_kind(node, "<")
            .context("generic argument clause is missing '<'")?;
        let right_angle = self
            .immediate_child_kind(node, ">")
            .context("generic argument clause is missing '>'")?;
        let mut arguments = Vec::new();
        for argument in named_children(node).filter(|child| {
            child.start_byte() >= left_angle.end_byte()
                && child.end_byte() <= right_angle.start_byte()
        }) {
            let trailing_comma = self.trailing_delimiter(node, argument, ",");
            let argument_type = self.generic_argument_type(argument)?;
            let mut children = vec![self.with_name(self.type_syntax(argument_type)?, "argument")];
            if let Some(comma) = trailing_comma {
                children.push(self.with_name(self.token_for_node(comma, "comma"), "trailingComma"));
            }
            let argument_end = trailing_comma.map_or(argument.end_byte(), |comma| comma.end_byte());
            arguments.push(self.with_name(
                self.syntax_node(
                    "GenericArgumentSyntax",
                    self.range_from_offsets(argument.start_byte(), argument_end),
                    children,
                ),
                "",
            ));
        }
        Ok(self.syntax_node(
            "GenericArgumentClauseSyntax",
            self.range_for_node(node),
            vec![
                self.with_name(self.token_for_node(left_angle, "leftAngle"), "leftAngle"),
                self.with_name(
                    self.syntax_node(
                        "GenericArgumentListSyntax",
                        self.range_from_offsets(left_angle.end_byte(), right_angle.start_byte()),
                        arguments,
                    ),
                    "arguments",
                ),
                self.with_name(self.token_for_node(right_angle, "rightAngle"), "rightAngle"),
            ],
        ))
    }

    fn generic_argument_type(&self, argument: Node<'a>) -> Result<Node<'a>> {
        if argument.kind() != "type_parameter" {
            return Ok(argument);
        }
        named_children(argument)
            .find(|child| is_type_syntax_node_kind(child.kind()))
            .context("generic type parameter is missing a type")
    }

    fn special_literal_expr(&self, node: Node<'a>) -> Result<Value> {
        if self.text(node).starts_with('#') {
            return self.macro_expansion_expr(node);
        }
        bail!("unsupported Swift special literal '{}'", self.text(node))
    }

    fn macro_expansion_expr(&self, node: Node<'a>) -> Result<Value> {
        self.macro_expansion(node, "MacroExpansionExprSyntax", false)
    }

    fn macro_expansion_decl(&self, node: Node<'a>) -> Result<Value> {
        self.macro_expansion(node, "MacroExpansionDeclSyntax", true)
    }

    fn macro_expansion(
        &self,
        node: Node<'a>,
        node_type: &str,
        include_decl_prefix: bool,
    ) -> Result<Value> {
        let (macro_name, macro_name_end) = self.macro_name_token(node)?;
        let pound_start = self
            .bare_macro_pound_start(node)
            .unwrap_or_else(|| node.start_byte());
        let mut syntax_children = Vec::new();
        if include_decl_prefix {
            syntax_children.push(self.with_name(
                self.empty_collection("AttributeListSyntax", pound_start),
                "attributes",
            ));
            syntax_children.push(self.with_name(
                self.empty_collection("DeclModifierListSyntax", pound_start),
                "modifiers",
            ));
        }

        syntax_children.push(self.with_name(
            self.token_with_range(
                "pound",
                self.range_from_offsets(pound_start, pound_start + 1),
            ),
            "pound",
        ));
        syntax_children.push(self.with_name(macro_name, "macroName"));

        if let Some(type_parameters) = self
            .immediate_named_child_kind(node, "type_parameters")
            .or_else(|| self.immediate_named_child_kind(node, "type_arguments"))
        {
            syntax_children.push(self.with_name(
                self.generic_argument_clause(type_parameters)?,
                "genericArgumentClause",
            ));
        }

        let suffix = self.immediate_named_child_kind(node, "call_suffix");
        if node.kind() == "diagnostic" {
            if let Some((left_paren, arguments, right_paren)) =
                self.diagnostic_macro_arguments(node, macro_name_end)?
            {
                syntax_children.push(self.with_name(left_paren, "leftParen"));
                syntax_children.push(self.with_name(arguments, "arguments"));
                syntax_children.push(self.with_name(right_paren, "rightParen"));
            } else {
                syntax_children.push(self.with_name(
                    self.empty_collection("LabeledExprListSyntax", macro_name_end),
                    "arguments",
                ));
            }
        } else if let Some(suffix) = suffix {
            let trailing_closures = self.trailing_closure_nodes(suffix);

            if let Some(value_arguments) =
                self.immediate_named_child_kind(suffix, "value_arguments")
            {
                let left_paren = self
                    .immediate_child_kind(value_arguments, "(")
                    .context("macro arguments are missing '('")?;
                let right_paren = self
                    .immediate_child_kind(value_arguments, ")")
                    .context("macro arguments are missing ')'")?;
                syntax_children.push(
                    self.with_name(self.token_for_node(left_paren, "leftParen"), "leftParen"),
                );
                syntax_children.push(self.with_name(
                    self.labeled_expr_list(value_arguments, left_paren, right_paren)?,
                    "arguments",
                ));
                syntax_children.push(
                    self.with_name(self.token_for_node(right_paren, "rightParen"), "rightParen"),
                );
            } else {
                syntax_children.push(self.with_name(
                    self.empty_collection("LabeledExprListSyntax", suffix.start_byte()),
                    "arguments",
                ));
            }

            if let Some(trailing_closure) = trailing_closures.first() {
                syntax_children
                    .push(self.with_name(self.closure_expr(*trailing_closure)?, "trailingClosure"));
            }
        } else {
            syntax_children.push(self.with_name(
                self.empty_collection("LabeledExprListSyntax", macro_name_end),
                "arguments",
            ));
        }

        syntax_children.push(self.with_name(
            suffix.map_or_else(
                || {
                    Ok(self.empty_collection(
                        "MultipleTrailingClosureElementListSyntax",
                        node.end_byte(),
                    ))
                },
                |suffix| {
                    let trailing_closures = self.trailing_closure_nodes(suffix);
                    self.additional_trailing_closure_list(
                        suffix,
                        &trailing_closures,
                        node.end_byte(),
                    )
                },
            )?,
            "additionalTrailingClosures",
        ));

        Ok(self.syntax_node(
            node_type,
            self.range_from_offsets(pound_start, node.end_byte()),
            syntax_children,
        ))
    }

    fn diagnostic_macro_arguments(
        &self,
        node: Node<'a>,
        macro_name_end: usize,
    ) -> Result<Option<(Value, Value, Value)>> {
        let Some(left_relative) = self.source[macro_name_end..node.end_byte()].find('(') else {
            return Ok(None);
        };
        let left_start = macro_name_end + left_relative;
        let right_start = self.source[left_start..node.end_byte()]
            .rfind(')')
            .map(|relative| left_start + relative)
            .context("diagnostic macro arguments are missing ')'")?;
        let left_paren = self.token_with_range(
            "leftParen",
            self.range_from_offsets(left_start, left_start + 1),
        );
        let right_paren = self.token_with_range(
            "rightParen",
            self.range_from_offsets(right_start, right_start + 1),
        );

        let mut arguments = Vec::new();
        if let Some(string_start) = self.source[left_start + 1..right_start]
            .find('"')
            .map(|relative| left_start + 1 + relative)
        {
            let string_end = self.source[string_start + 1..right_start]
                .rfind('"')
                .map(|relative| string_start + 1 + relative + 1)
                .context("diagnostic string argument is missing closing quote")?;
            let literal = self.string_literal_node(StringLiteralSpec {
                start: string_start,
                end: string_end,
                opening_pounds: None,
                opening_quote: (string_start, string_start + 1),
                closing_quote: (string_end - 1, string_end),
                closing_pounds: None,
                segment_specs: vec![(
                    string_start + 1,
                    string_end - 1,
                    self.source[string_start + 1..string_end - 1].to_string(),
                )],
            });
            arguments.push(self.with_name(
                self.syntax_node(
                    "LabeledExprSyntax",
                    self.range_from_offsets(string_start, string_end),
                    vec![self.with_name(literal, "expression")],
                ),
                "",
            ));
        }

        let arguments = self.syntax_node(
            "LabeledExprListSyntax",
            self.range_from_offsets(left_start + 1, right_start),
            arguments,
        );
        Ok(Some((left_paren, arguments, right_paren)))
    }

    fn macro_name_token(&self, node: Node<'a>) -> Result<(Value, usize)> {
        if let Some(name) = named_children(node)
            .find(|child| matches!(child.kind(), "simple_identifier" | "identifier"))
        {
            return Ok((
                self.token_for_node(
                    name,
                    &format!("identifier({})", quoted_text(self.text(name))),
                ),
                name.end_byte(),
            ));
        }

        if let Some(pound_start) = self.bare_macro_pound_start(node) {
            let name_start = pound_start + 1;
            let mut name_end = name_start;
            for (offset, ch) in self.source[name_start..node.end_byte()].char_indices() {
                if ch == '_' || ch.is_alphanumeric() {
                    name_end = name_start + offset + ch.len_utf8();
                } else {
                    break;
                }
            }
            if name_end == name_start {
                bail!("macro expansion is missing a macro name");
            }
            let name = &self.source[name_start..name_end];
            return Ok((
                self.token_with_range(
                    &format!("identifier({})", quoted_text(name)),
                    self.range_from_offsets(name_start, name_end),
                ),
                name_end,
            ));
        }

        bail!("macro expansion is missing a macro name")
    }

    fn function_call_expr(&self, node: Node<'a>) -> Result<Value> {
        let callee = named_children(node)
            .find(|child| child.kind() != "call_suffix")
            .context("call expression is missing callee")?;
        let suffix = self
            .immediate_named_child_kind(node, "call_suffix")
            .context("call expression is missing call suffix")?;

        if let Some(binary_expr) = self.binary_expr_with_rhs_call_suffix(node, callee, suffix)? {
            return Ok(binary_expr);
        }

        let trailing_closures = self.trailing_closure_nodes(suffix);
        if !trailing_closures.is_empty()
            && self
                .immediate_named_child_kind(suffix, "value_arguments")
                .is_none()
        {
            if let Some((inner_callee, inner_suffix, inner_value_arguments)) =
                self.parenthesized_call_parts(callee)
            {
                return self.function_call_expr_from_components(FunctionCallComponents {
                    parent: callee,
                    callee: inner_callee,
                    callee_suffix: inner_suffix,
                    value_arguments: Some(inner_value_arguments),
                    empty_arguments_offset: inner_suffix.start_byte(),
                    trailing_suffix: suffix,
                    trailing_closures: &trailing_closures,
                    start: node.start_byte(),
                    end: node.end_byte(),
                });
            }
        }

        if let Some(value_arguments) = self.immediate_named_child_kind(suffix, "value_arguments") {
            if self.subscript_delimiters(value_arguments).is_some() {
                return self.subscript_call_expr(
                    node,
                    callee,
                    value_arguments,
                    &trailing_closures,
                    true,
                );
            }
        } else if !trailing_closures.is_empty() {
            if let Some((inner_callee, inner_value_arguments)) = self.subscript_call_parts(callee) {
                return self.subscript_call_expr(
                    node,
                    inner_callee,
                    inner_value_arguments,
                    &trailing_closures,
                    false,
                );
            }
        }

        self.function_call_expr_from_components(FunctionCallComponents {
            parent: node,
            callee,
            callee_suffix: suffix,
            value_arguments: self.immediate_named_child_kind(suffix, "value_arguments"),
            empty_arguments_offset: suffix.start_byte(),
            trailing_suffix: suffix,
            trailing_closures: &trailing_closures,
            start: node.start_byte(),
            end: node.end_byte(),
        })
    }

    fn trailing_closure_nodes(&self, suffix: Node<'a>) -> Vec<Node<'a>> {
        named_children(suffix)
            .filter(|child| child.kind() == "lambda_literal")
            .collect()
    }

    fn additional_trailing_closure_list(
        &self,
        suffix: Node<'a>,
        trailing_closures: &[Node<'a>],
        empty_offset: usize,
    ) -> Result<Value> {
        let mut elements = Vec::new();
        let mut list_start = None;
        let mut list_end = empty_offset;
        for closure in trailing_closures.iter().skip(1).copied() {
            let (element, start, end) = self.multiple_trailing_closure_element(suffix, closure)?;
            list_start.get_or_insert(start);
            list_end = end;
            elements.push(self.with_name(element, ""));
        }

        if let Some(start) = list_start {
            Ok(self.syntax_node(
                "MultipleTrailingClosureElementListSyntax",
                self.range_from_offsets(start, list_end),
                elements,
            ))
        } else {
            Ok(self.empty_collection("MultipleTrailingClosureElementListSyntax", empty_offset))
        }
    }

    fn multiple_trailing_closure_element(
        &self,
        suffix: Node<'a>,
        closure: Node<'a>,
    ) -> Result<(Value, usize, usize)> {
        let label = self
            .additional_trailing_closure_label(suffix, closure)
            .context("additional trailing closure is missing a label")?;
        let colon_start = self.source[label.end_byte()..closure.start_byte()]
            .find(':')
            .map(|offset| label.end_byte() + offset)
            .context("additional trailing closure is missing ':'")?;
        let end = closure.end_byte();
        let label_token = if self.text(label) == "_" {
            self.token_for_node(label, "wildcard")
        } else {
            self.token_for_node(
                label,
                &format!("identifier({})", quoted_text(self.text(label))),
            )
        };

        Ok((
            self.syntax_node(
                "MultipleTrailingClosureElementSyntax",
                self.range_from_offsets(label.start_byte(), end),
                vec![
                    self.with_name(label_token, "label"),
                    self.with_name(
                        self.token_with_range(
                            "colon",
                            self.range_from_offsets(colon_start, colon_start + 1),
                        ),
                        "colon",
                    ),
                    self.with_name(self.closure_expr(closure)?, "closure"),
                ],
            ),
            label.start_byte(),
            end,
        ))
    }

    fn additional_trailing_closure_label(
        &self,
        suffix: Node<'a>,
        closure: Node<'a>,
    ) -> Option<Node<'a>> {
        let label = children(suffix)
            .take_while(|child| child.start_byte() < closure.start_byte())
            .filter(|child| !is_trivia_node(*child) && child.kind() != ":")
            .last()?;
        if matches!(
            label.kind(),
            "identifier" | "simple_identifier" | "wildcard_pattern"
        ) || self.text(label) == "_"
        {
            Some(label)
        } else {
            None
        }
    }

    fn parenthesized_call_parts(&self, node: Node<'a>) -> Option<(Node<'a>, Node<'a>, Node<'a>)> {
        if node.kind() != "call_expression" {
            return None;
        }
        let callee = named_children(node).find(|child| child.kind() != "call_suffix")?;
        let suffix = self.immediate_named_child_kind(node, "call_suffix")?;
        let value_arguments = self.immediate_named_child_kind(suffix, "value_arguments")?;
        if self.subscript_delimiters(value_arguments).is_some() {
            return None;
        }
        Some((callee, suffix, value_arguments))
    }

    fn function_call_expr_from_components(
        &self,
        components: FunctionCallComponents<'a, '_>,
    ) -> Result<Value> {
        let FunctionCallComponents {
            parent,
            callee,
            callee_suffix,
            value_arguments,
            empty_arguments_offset,
            trailing_suffix,
            trailing_closures,
            start,
            end,
        } = components;

        let mut children = vec![self.with_name(
            self.called_expression_with_optional_chaining(parent, callee, callee_suffix)?,
            "calledExpression",
        )];

        if let Some(value_arguments) = value_arguments {
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
                self.empty_collection("LabeledExprListSyntax", empty_arguments_offset),
                "arguments",
            ));
        }

        if let Some(trailing_closure) = trailing_closures.first() {
            children.push(self.with_name(self.closure_expr(*trailing_closure)?, "trailingClosure"));
        }
        children.push(self.with_name(
            self.additional_trailing_closure_list(trailing_suffix, trailing_closures, end)?,
            "additionalTrailingClosures",
        ));

        Ok(self.syntax_node(
            "FunctionCallExprSyntax",
            self.range_from_offsets(start, end),
            children,
        ))
    }

    fn binary_expr_with_rhs_call_suffix(
        &self,
        node: Node<'a>,
        callee: Node<'a>,
        suffix: Node<'a>,
    ) -> Result<Option<Value>> {
        if !is_binary_expression_kind(callee.kind()) {
            return Ok(None);
        }
        let Some(value_arguments) = self.immediate_named_child_kind(suffix, "value_arguments")
        else {
            return Ok(None);
        };
        if self.subscript_delimiters(value_arguments).is_some() {
            return Ok(None);
        }

        let lhs = self
            .field_child(callee, "lhs")
            .or_else(|| self.field_child(callee, "start"))
            .context("binary expression is missing lhs")?;
        let op = self
            .field_child(callee, "op")
            .context("binary expression is missing operator")?;
        let rhs = self
            .field_child(callee, "rhs")
            .or_else(|| self.field_child(callee, "end"))
            .context("binary expression is missing rhs")?;
        let rhs_call =
            self.expr_with_call_suffix(node, rhs, suffix, rhs.start_byte(), node.end_byte())?;

        if lhs.kind() == "try_expression" && lhs.start_byte() == callee.start_byte() {
            let try_expression = self
                .expression_field_child(lhs, "expr")
                .context("try expression is missing expression")?;
            let expression = self.infix_operator_expr_from_values(
                try_expression.start_byte(),
                node.end_byte(),
                self.expr(try_expression)?,
                op,
                rhs_call,
            )?;
            return self.try_expr_wrapping_value(lhs, expression).map(Some);
        }

        self.infix_operator_expr_from_values(
            callee.start_byte(),
            node.end_byte(),
            self.expr(lhs)?,
            op,
            rhs_call,
        )
        .map(Some)
    }

    fn expr_with_call_suffix(
        &self,
        parent: Node<'a>,
        expression: Node<'a>,
        suffix: Node<'a>,
        start: usize,
        end: usize,
    ) -> Result<Value> {
        if expression.kind() == "try_expression" {
            let inner = self
                .expression_field_child(expression, "expr")
                .context("try expression is missing expression")?;
            let call = self.function_call_expr_with_suffix(
                parent,
                inner,
                suffix,
                inner.start_byte(),
                end,
            )?;
            return self.try_expr_wrapping_value(expression, call);
        }
        self.function_call_expr_with_suffix(parent, expression, suffix, start, end)
    }

    fn function_call_expr_with_suffix(
        &self,
        parent: Node<'a>,
        callee: Node<'a>,
        suffix: Node<'a>,
        start: usize,
        end: usize,
    ) -> Result<Value> {
        let trailing_closures = self.trailing_closure_nodes(suffix);
        self.function_call_expr_from_components(FunctionCallComponents {
            parent,
            callee,
            callee_suffix: suffix,
            value_arguments: self.immediate_named_child_kind(suffix, "value_arguments"),
            empty_arguments_offset: suffix.start_byte(),
            trailing_suffix: suffix,
            trailing_closures: &trailing_closures,
            start,
            end,
        })
    }

    fn subscript_call_expr(
        &self,
        node: Node<'a>,
        callee: Node<'a>,
        value_arguments: Node<'a>,
        trailing_closures: &[Node<'a>],
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
        if let Some(closure) = trailing_closures.first() {
            children.push(self.with_name(self.closure_expr(*closure)?, "trailingClosure"));
        }
        children.push(
            self.with_name(
                self.additional_trailing_closure_list(
                    self.immediate_named_child_kind(node, "call_suffix")
                        .unwrap_or(value_arguments),
                    trailing_closures,
                    node.end_byte(),
                )?,
                "additionalTrailingClosures",
            ),
        );

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
        if let Some(borrow_arguments) =
            self.borrow_labeled_expr_list(value_arguments, left_paren, right_paren)?
        {
            return Ok(borrow_arguments);
        }
        if let Some(regex_arguments) = self.regex_labeled_expr_list(left_paren, right_paren)? {
            return Ok(regex_arguments);
        }

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

    fn borrow_labeled_expr_list(
        &self,
        value_arguments: Node<'a>,
        left_paren: Node<'a>,
        right_paren: Node<'a>,
    ) -> Result<Option<Value>> {
        let children = named_children(value_arguments)
            .filter(|child| !is_trivia_node(*child))
            .collect::<Vec<_>>();
        let [argument, continuation] = children.as_slice() else {
            return Ok(None);
        };
        if argument.kind() != "value_argument" || continuation.kind() != "ERROR" {
            return Ok(None);
        }
        let Some(keyword) =
            named_children(*argument).find(|child| child.kind() != "value_argument_label")
        else {
            return Ok(None);
        };
        if self.text(keyword) != "borrow" || !is_identifier_like_text(self.text(*continuation)) {
            return Ok(None);
        }

        let expression = self.borrow_expr_from_parts(keyword, *continuation)?;
        let argument = self.with_name(
            self.syntax_node(
                "LabeledExprSyntax",
                self.range_from_offsets(keyword.start_byte(), continuation.end_byte()),
                vec![self.with_name(expression, "expression")],
            ),
            "",
        );
        Ok(Some(self.syntax_node(
            "LabeledExprListSyntax",
            self.range_from_offsets(left_paren.end_byte(), right_paren.start_byte()),
            vec![argument],
        )))
    }

    fn regex_labeled_expr_list(
        &self,
        left_paren: Node<'a>,
        right_paren: Node<'a>,
    ) -> Result<Option<Value>> {
        let Some(items) =
            self.regex_literal_list_items(left_paren.end_byte(), right_paren.start_byte())
        else {
            return Ok(None);
        };
        if items.len() < 2 {
            return Ok(None);
        }
        let args = items
            .iter()
            .map(|item| {
                let mut children = vec![self.with_name(
                    self.regex_literal_expr_from_offsets(item.literal_start, item.literal_end)?,
                    "expression",
                )];
                if let Some((comma_start, comma_end)) = item.comma {
                    children.push(self.with_name(
                        self.token_with_range(
                            "comma",
                            self.range_from_offsets(comma_start, comma_end),
                        ),
                        "trailingComma",
                    ));
                }
                let element_end = item.comma.map_or(item.literal_end, |(_, end)| end);
                Ok(self.with_name(
                    self.syntax_node(
                        "LabeledExprSyntax",
                        self.range_from_offsets(item.literal_start, element_end),
                        children,
                    ),
                    "",
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(self.syntax_node(
            "LabeledExprListSyntax",
            self.range_from_offsets(left_paren.end_byte(), right_paren.start_byte()),
            args,
        )))
    }

    fn labeled_expr(&self, node: Node<'a>, trailing_comma: Option<Node<'a>>) -> Result<Value> {
        let value = self
            .value_field_child(node)
            .or_else(|| {
                named_children(node).find(|child| {
                    child.kind() != "value_argument_label" && !self.is_recovery_bang_node(*child)
                })
            })
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
        if let Some(recovered) = self.recovered_raw_string_assignment(node)? {
            return Ok(recovered);
        }
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

    fn recovered_raw_string_assignment(&self, assignment: Node<'a>) -> Result<Option<Value>> {
        let lhs = self
            .assignment_lhs(assignment)
            .context("assignment is missing lhs")?;
        let equal = self
            .assignment_equal(assignment)
            .context("assignment is missing '='")?;
        let mut rhs_start = equal.end_byte();
        while rhs_start < assignment.end_byte()
            && self.source.as_bytes()[rhs_start].is_ascii_whitespace()
        {
            rhs_start += 1;
        }
        let mut rhs_end = assignment.end_byte();
        while rhs_end > rhs_start && self.source.as_bytes()[rhs_end - 1].is_ascii_whitespace() {
            rhs_end -= 1;
        }
        let text = &self.source[rhs_start..rhs_end];
        if !text.starts_with("#\"") {
            return Ok(None);
        }
        let Some((opening_pounds_len, quote_len, closing_quote_start)) = raw_string_bounds(text)
        else {
            return Ok(None);
        };
        let content_start = rhs_start + opening_pounds_len + quote_len;
        let content_end = rhs_start + closing_quote_start;
        let rhs = self.string_literal_node(StringLiteralSpec {
            start: rhs_start,
            end: rhs_end,
            opening_pounds: Some((rhs_start, rhs_start + opening_pounds_len)),
            opening_quote: (
                rhs_start + opening_pounds_len,
                rhs_start + opening_pounds_len + quote_len,
            ),
            closing_quote: (
                rhs_start + closing_quote_start,
                rhs_start + closing_quote_start + quote_len,
            ),
            closing_pounds: Some((rhs_end - opening_pounds_len, rhs_end)),
            segment_specs: self.raw_string_segments(content_start, content_end),
        });
        let assignment_operator = self.syntax_node(
            "AssignmentExprSyntax",
            self.range_for_node(equal),
            vec![self.with_name(self.token_for_node(equal, "equal"), "equal")],
        );
        Ok(Some(self.syntax_node(
            "InfixOperatorExprSyntax",
            self.range_for_node(assignment),
            vec![
                self.with_name(self.expr(lhs)?, "leftOperand"),
                self.with_name(assignment_operator, "operator"),
                self.with_name(rhs, "rightOperand"),
            ],
        )))
    }

    fn recovered_escaped_raw_assignment(
        &self,
        assignment: Node<'a>,
        end_node: Node<'a>,
    ) -> Result<Value> {
        let lhs = self
            .assignment_lhs(assignment)
            .context("assignment is missing lhs")?;
        let equal = self
            .assignment_equal(assignment)
            .context("assignment is missing '='")?;
        let mut rhs_start = equal.end_byte();
        while rhs_start < end_node.end_byte()
            && self.source.as_bytes()[rhs_start].is_ascii_whitespace()
        {
            rhs_start += 1;
        }
        let rhs_text = &self.source[rhs_start..end_node.end_byte()];
        let segments = self.escaped_raw_string_segments(rhs_start, rhs_text);
        let quote_start = rhs_text
            .find('"')
            .map(|offset| rhs_start + offset)
            .unwrap_or(rhs_start);
        let quote_end = rhs_text
            .rfind('"')
            .map(|offset| rhs_start + offset + 1)
            .unwrap_or(end_node.end_byte());
        let rhs = self.string_literal_node(StringLiteralSpec {
            start: rhs_start,
            end: end_node.end_byte(),
            opening_pounds: None,
            opening_quote: (quote_start, quote_start.saturating_add(1)),
            closing_quote: (quote_end.saturating_sub(1), quote_end),
            closing_pounds: None,
            segment_specs: segments,
        });
        let assignment_operator = self.syntax_node(
            "AssignmentExprSyntax",
            self.range_for_node(equal),
            vec![self.with_name(self.token_for_node(equal, "equal"), "equal")],
        );
        Ok(self.syntax_node(
            "InfixOperatorExprSyntax",
            self.range_from_offsets(assignment.start_byte(), end_node.end_byte()),
            vec![
                self.with_name(self.expr(lhs)?, "leftOperand"),
                self.with_name(assignment_operator, "operator"),
                self.with_name(rhs, "rightOperand"),
            ],
        ))
    }

    fn recovered_regex_assignment(
        &self,
        assignment: Node<'a>,
        end_node: Node<'a>,
    ) -> Result<Value> {
        let lhs = self
            .assignment_lhs(assignment)
            .context("assignment is missing lhs")?;
        let equal = self
            .assignment_equal(assignment)
            .context("assignment is missing '='")?;
        let rhs = self
            .field_child(assignment, "result")
            .context("assignment is missing rhs")?;
        let assignment_operator = self.syntax_node(
            "AssignmentExprSyntax",
            self.range_for_node(equal),
            vec![self.with_name(self.token_for_node(equal, "equal"), "equal")],
        );
        Ok(self.syntax_node(
            "InfixOperatorExprSyntax",
            self.range_from_offsets(assignment.start_byte(), end_node.end_byte()),
            vec![
                self.with_name(self.expr(lhs)?, "leftOperand"),
                self.with_name(assignment_operator, "operator"),
                self.with_name(self.expr(rhs)?, "rightOperand"),
            ],
        ))
    }

    fn assignment_lhs(&self, assignment: Node<'a>) -> Option<Node<'a>> {
        self.field_child(assignment, "target").or_else(|| {
            self.immediate_named_child_kind(assignment, "directly_assignable_expression")
        })
    }

    fn assignment_equal(&self, assignment: Node<'a>) -> Option<Node<'a>> {
        self.field_child(assignment, "operator")
            .or_else(|| self.immediate_child_kind(assignment, "="))
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

    fn return_stmt_from_keyword(&self, return_keyword: Node<'a>) -> Value {
        self.syntax_node(
            "ReturnStmtSyntax",
            self.range_for_node(return_keyword),
            vec![self.with_name(
                self.token_for_node(return_keyword, "keyword(SwiftSyntax.Keyword.return)"),
                "returnKeyword",
            )],
        )
    }

    fn control_transfer_stmt(&self, node: Node<'a>) -> Result<Value> {
        if self.first_descendant_any_kind(node, "return").is_some() {
            return self.return_stmt(node);
        }
        if self.yield_keyword_offsets(node).is_some() {
            return self.yield_stmt(node);
        }
        if let Some(break_keyword) = self.first_descendant_any_kind(node, "break") {
            return Ok(self.jump_stmt(
                "BreakStmtSyntax",
                node,
                break_keyword,
                "breakKeyword",
                "keyword(SwiftSyntax.Keyword.break)",
            ));
        }
        if let Some(continue_keyword) = self.first_descendant_any_kind(node, "continue") {
            return Ok(self.jump_stmt(
                "ContinueStmtSyntax",
                node,
                continue_keyword,
                "continueKeyword",
                "keyword(SwiftSyntax.Keyword.continue)",
            ));
        }
        bail!("unsupported Swift control transfer statement");
    }

    fn yield_stmt(&self, node: Node<'a>) -> Result<Value> {
        let (yield_start, yield_end) = self
            .yield_keyword_offsets(node)
            .context("yield statement is missing yield keyword")?;
        let mut children = vec![self.with_name(
            self.token_with_range(
                "keyword(SwiftSyntax.Keyword.yield)",
                self.range_from_offsets(yield_start, yield_end),
            ),
            "yieldKeyword",
        )];
        if let Some(expression) = self
            .field_child(node, "result")
            .or_else(|| named_children(node).find(|child| child.start_byte() >= yield_end))
        {
            children.push(self.with_name(self.expr(expression)?, "yieldedExpressions"));
        }
        let range_end = children.last().map(end_offset).unwrap_or(yield_end);
        Ok(self.syntax_node(
            "YieldStmtSyntax",
            self.range_from_offsets(yield_start, range_end),
            children,
        ))
    }

    fn yield_keyword_offsets(&self, node: Node<'a>) -> Option<(usize, usize)> {
        if let Some(keyword) = self.first_descendant_any_kind(node, "yield") {
            return Some((keyword.start_byte(), keyword.end_byte()));
        }
        let start = self.skip_horizontal_whitespace(node.start_byte(), node.end_byte());
        let end = start + "yield".len();
        (end <= node.end_byte()
            && &self.source[start..end] == "yield"
            && self
                .source
                .as_bytes()
                .get(end)
                .is_none_or(|byte| byte.is_ascii_whitespace()))
        .then_some((start, end))
    }

    fn jump_stmt(
        &self,
        node_type: &str,
        node: Node<'a>,
        keyword: Node<'a>,
        keyword_name: &str,
        token_kind: &str,
    ) -> Value {
        let label = self.control_transfer_label(node, keyword);
        let range_end = label
            .map(|label| label.end_byte())
            .unwrap_or(keyword.end_byte());
        let mut children =
            vec![self.with_name(self.token_for_node(keyword, token_kind), keyword_name)];
        if let Some(label) = label {
            children.push(self.with_name(
                self.token_for_node(
                    label,
                    &format!("identifier({})", quoted_text(self.text(label))),
                ),
                "label",
            ));
        }
        self.syntax_node(
            node_type,
            self.range_from_offsets(keyword.start_byte(), range_end),
            children,
        )
    }

    fn control_transfer_label(&self, node: Node<'a>, keyword: Node<'a>) -> Option<Node<'a>> {
        self.field_child(node, "result")
            .filter(|child| child.start_byte() >= keyword.end_byte())
            .or_else(|| {
                named_children(node).find(|child| {
                    child.start_byte() >= keyword.end_byte()
                        && matches!(child.kind(), "identifier" | "simple_identifier")
                })
            })
    }

    fn string_literal(&self, node: Node<'a>) -> Result<Value> {
        let quote_len = if self.text(node).starts_with("\"\"\"") {
            3
        } else {
            1
        };
        let segments = if quote_len == 3 {
            self.multiline_string_segment_nodes(node)
        } else {
            self.line_string_segment_nodes(node)?
        };
        Ok(
            self.string_literal_node_with_segments(StringLiteralNodeSpec {
                start: node.start_byte(),
                end: node.end_byte(),
                opening_pounds: None,
                opening_quote: (node.start_byte(), node.start_byte() + quote_len),
                closing_quote: (node.end_byte() - quote_len, node.end_byte()),
                closing_pounds: None,
                segments,
            }),
        )
    }

    fn line_string_segment_nodes(&self, node: Node<'a>) -> Result<Vec<Value>> {
        let mut segments = Vec::new();
        let mut pending_start = None;
        let mut pending_end = 0;
        let mut pending_text = String::new();

        let segment_children = named_children(node).collect::<Vec<_>>();
        for (index, child) in segment_children.iter().copied().enumerate() {
            match child.kind() {
                "line_str_text" | "str_escaped_char" => {
                    pending_start.get_or_insert(child.start_byte());
                    pending_end = child.end_byte();
                    pending_text.push_str(&normalize_escaped_raw_segment(self.text(child)));
                }
                "interpolated_expression" => {
                    self.flush_string_segment(
                        &mut segments,
                        &mut pending_start,
                        &mut pending_end,
                        &mut pending_text,
                    );
                    let expression_segment = self.expression_segment(node, child)?;
                    let expression_end = end_offset(&expression_segment);
                    segments.push(self.with_name(expression_segment, ""));
                    if segment_children
                        .get(index + 1)
                        .is_none_or(|next| next.kind() == "interpolated_expression")
                    {
                        segments.push(self.with_name(
                            self.string_segment_node(expression_end, expression_end, String::new()),
                            "",
                        ));
                    }
                }
                _ => {}
            }
        }

        self.flush_string_segment(
            &mut segments,
            &mut pending_start,
            &mut pending_end,
            &mut pending_text,
        );
        Ok(segments)
    }

    fn multiline_string_segment_nodes(&self, node: Node<'a>) -> Vec<Value> {
        let mut segments = Vec::new();
        for text in self.field_children(node, "text") {
            let content = self.text(text);
            let mut line_start = 0;
            for (offset, ch) in content.char_indices() {
                if ch == '\n' {
                    self.push_multiline_string_line(
                        &mut segments,
                        text.start_byte(),
                        content,
                        line_start,
                        offset,
                    );
                    line_start = offset + ch.len_utf8();
                }
            }
            self.push_multiline_string_line(
                &mut segments,
                text.start_byte(),
                content,
                line_start,
                content.len(),
            );
        }
        segments
    }

    fn push_multiline_string_line(
        &self,
        segments: &mut Vec<Value>,
        base_offset: usize,
        content: &str,
        line_start: usize,
        line_end: usize,
    ) {
        let mut start = line_start;
        let mut end = line_end;
        let bytes = content.as_bytes();
        while start < end && matches!(bytes[start], b' ' | b'\t' | b'\r') {
            start += 1;
        }
        while end > start && matches!(bytes[end - 1], b' ' | b'\t' | b'\r') {
            end -= 1;
        }
        if start < end {
            segments.push(self.with_name(
                self.string_segment_node(
                    base_offset + start,
                    base_offset + end,
                    content[start..end].to_string(),
                ),
                "",
            ));
        }
    }

    fn flush_string_segment(
        &self,
        segments: &mut Vec<Value>,
        pending_start: &mut Option<usize>,
        pending_end: &mut usize,
        pending_text: &mut String,
    ) {
        if let Some(start) = pending_start.take() {
            segments.push(self.with_name(
                self.string_segment_node(start, *pending_end, std::mem::take(pending_text)),
                "",
            ));
        }
    }

    fn expression_segment(&self, parent: Node<'a>, interpolation: Node<'a>) -> Result<Value> {
        let backslash_left_paren = children(parent)
            .filter(|child| child.kind() == "\\(" && child.end_byte() <= interpolation.start_byte())
            .last()
            .context("interpolated string segment is missing '\\('")?;
        let right_paren = children(parent)
            .find(|child| child.kind() == ")" && child.start_byte() >= interpolation.end_byte())
            .context("interpolated string segment is missing ')'")?;
        let expression = self
            .value_field_child(interpolation)
            .or_else(|| named_children(interpolation).next())
            .context("interpolated string segment is missing expression")?;
        let labeled_expr = self.with_name(self.labeled_expr_for_value(expression, None)?, "");
        Ok(self.syntax_node(
            "ExpressionSegmentSyntax",
            self.range_from_offsets(backslash_left_paren.start_byte(), right_paren.end_byte()),
            vec![
                self.with_name(
                    self.token_with_range(
                        "backslash",
                        self.range_from_offsets(
                            backslash_left_paren.start_byte(),
                            backslash_left_paren.start_byte() + 1,
                        ),
                    ),
                    "backslash",
                ),
                self.with_name(
                    self.token_with_range(
                        "leftParen",
                        self.range_from_offsets(
                            backslash_left_paren.start_byte() + 1,
                            backslash_left_paren.end_byte(),
                        ),
                    ),
                    "leftParen",
                ),
                self.with_name(
                    self.syntax_node(
                        "LabeledExprListSyntax",
                        self.range_from_offsets(expression.start_byte(), expression.end_byte()),
                        vec![labeled_expr],
                    ),
                    "expressions",
                ),
                self.with_name(self.token_for_node(right_paren, "rightParen"), "rightParen"),
            ],
        ))
    }

    fn raw_string_literal(&self, node: Node<'a>) -> Result<Value> {
        let text = self.text(node);
        let (opening_pounds_len, quote_len, closing_quote_start) =
            raw_string_bounds(text).context("raw string literal is missing delimiters")?;
        let content_start = node.start_byte() + opening_pounds_len + quote_len;
        let content_end = node.start_byte() + closing_quote_start;
        let segments = self.raw_string_segments(content_start, content_end);

        Ok(self.string_literal_node(StringLiteralSpec {
            start: node.start_byte(),
            end: node.end_byte(),
            opening_pounds: (opening_pounds_len > 0)
                .then_some((node.start_byte(), node.start_byte() + opening_pounds_len)),
            opening_quote: (
                node.start_byte() + opening_pounds_len,
                node.start_byte() + opening_pounds_len + quote_len,
            ),
            closing_quote: (
                node.start_byte() + closing_quote_start,
                node.start_byte() + closing_quote_start + quote_len,
            ),
            closing_pounds: (opening_pounds_len > 0)
                .then_some((node.end_byte() - opening_pounds_len, node.end_byte())),
            segment_specs: segments,
        }))
    }

    fn raw_string_segments(
        &self,
        content_start: usize,
        content_end: usize,
    ) -> Vec<(usize, usize, String)> {
        let content = &self.source[content_start..content_end];
        let mut segments = Vec::new();
        let mut start = 0;
        while let Some(relative) = content[start..].find("\\#n") {
            let end = start + relative + "\\#n".len();
            segments.push((
                content_start + start,
                content_start + end,
                content[start..end].to_string(),
            ));
            start = end;
        }
        if start < content.len() {
            segments.push((
                content_start + start,
                content_end,
                content[start..].to_string(),
            ));
        }
        segments
    }

    fn recovered_escaped_raw_string_literal(
        &self,
        assignment: Node<'a>,
        rhs: Node<'a>,
    ) -> Result<Option<Value>> {
        if assignment.kind() != "assignment" || rhs.kind() != "regex_literal" {
            return Ok(None);
        }
        let equal = self
            .field_child(assignment, "operator")
            .or_else(|| self.immediate_child_kind(assignment, "="))
            .context("assignment is missing '='")?;
        let mut start = equal.end_byte();
        while start < rhs.end_byte() && self.source.as_bytes()[start].is_ascii_whitespace() {
            start += 1;
        }
        let text = &self.source[start..rhs.end_byte()];
        if !(text.contains("\\\"\\\"") || text.contains("\\\"\"")) {
            return Ok(None);
        }
        let segments = self.escaped_raw_string_segments(start, text);
        if segments.is_empty() {
            return Ok(None);
        }
        let quote_start = self.source[start..rhs.end_byte()]
            .find('"')
            .map(|offset| start + offset)
            .unwrap_or(start);
        let quote_end = self.source[start..rhs.end_byte()]
            .rfind('"')
            .map(|offset| start + offset + 1)
            .unwrap_or(rhs.end_byte());
        Ok(Some(self.string_literal_node(StringLiteralSpec {
            start,
            end: rhs.end_byte(),
            opening_pounds: None,
            opening_quote: (quote_start, quote_start.saturating_add(1)),
            closing_quote: (quote_end.saturating_sub(1), quote_end),
            closing_pounds: None,
            segment_specs: segments,
        })))
    }

    fn escaped_raw_string_segments(
        &self,
        base_offset: usize,
        text: &str,
    ) -> Vec<(usize, usize, String)> {
        let mut segments = Vec::new();
        let mut index = 0;
        while index < text.len() {
            let rest = &text[index..];
            if rest.starts_with("\\\"\"\"") {
                let segment_start = index;
                let after_quotes = index + "\\\"\"\"".len();
                let line_end = text[after_quotes..]
                    .find('\n')
                    .map(|relative| after_quotes + relative)
                    .unwrap_or(text.len());
                let suffix = text[after_quotes..line_end].trim_end_matches('\r');
                segments.push((
                    base_offset + segment_start,
                    base_offset + line_end,
                    format!("\\\"\\\"{suffix}"),
                ));
                index = line_end.saturating_add(1);
            } else if rest.starts_with("\\\"\\\"") {
                let segment_start = index;
                let mut after_quotes = index + "\\\"\\\"".len();
                if text[after_quotes..].starts_with("\\\"") {
                    after_quotes += "\\\"".len();
                }
                let line_end = text[after_quotes..]
                    .find('\n')
                    .map(|relative| after_quotes + relative)
                    .unwrap_or(text.len());
                let suffix = text[after_quotes..line_end].trim_end_matches('\r');
                segments.push((
                    base_offset + segment_start,
                    base_offset + line_end,
                    format!("\\\"\\\"{suffix}"),
                ));
                index = line_end.saturating_add(1);
            } else if rest.starts_with("\"\"") {
                let point = base_offset + index + 1;
                segments.push((point, point, String::new()));
                index += "\"\"".len();
            } else {
                let Some(ch) = rest.chars().next() else {
                    break;
                };
                index += ch.len_utf8();
            }
        }
        segments
    }

    fn regex_literal_expr(&self, node: Node<'a>) -> Result<Value> {
        let (start, end) = self.expanded_regex_literal_range(node);
        self.regex_literal_expr_from_offsets(start, end)
    }

    fn regex_literal_expr_from_offsets(&self, start: usize, end: usize) -> Result<Value> {
        let text = &self.source[start..end];
        let opening_slash = text
            .find('/')
            .context("regex literal is missing opening slash")?;
        let closing_slash = text
            .rfind('/')
            .filter(|offset| *offset > opening_slash)
            .context("regex literal is missing closing slash")?;
        let mut children = Vec::new();
        if opening_slash > 0 {
            children.push(self.with_name(
                self.token_with_range(
                    "regexPoundDelimiter",
                    self.range_from_offsets(start, start + opening_slash),
                ),
                "openingPounds",
            ));
        }
        children.push(self.with_name(
            self.token_with_range(
                "regexSlash",
                self.range_from_offsets(start + opening_slash, start + opening_slash + 1),
            ),
            "openingSlash",
        ));
        children.push(self.with_name(
            self.token_with_range(
                &format!(
                    "regexLiteralPattern({})",
                    quoted_text(&text[opening_slash + 1..closing_slash])
                ),
                self.range_from_offsets(start + opening_slash + 1, start + closing_slash),
            ),
            "regex",
        ));
        children.push(self.with_name(
            self.token_with_range(
                "regexSlash",
                self.range_from_offsets(start + closing_slash, start + closing_slash + 1),
            ),
            "closingSlash",
        ));
        if closing_slash + 1 < text.len() {
            children.push(self.with_name(
                self.token_with_range(
                    "regexPoundDelimiter",
                    self.range_from_offsets(start + closing_slash + 1, end),
                ),
                "closingPounds",
            ));
        }
        Ok(self.syntax_node(
            "RegexLiteralExprSyntax",
            self.range_from_offsets(start, end),
            children,
        ))
    }

    fn expanded_regex_literal_range(&self, node: Node<'a>) -> (usize, usize) {
        let mut start = node.start_byte();
        let mut end = node.end_byte();
        while start > 0 && self.source.as_bytes()[start - 1] == b'#' {
            start -= 1;
        }
        while end < self.source.len() && self.source.as_bytes()[end] == b'#' {
            end += 1;
        }
        (start, end)
    }

    fn string_literal_node(&self, spec: StringLiteralSpec) -> Value {
        let StringLiteralSpec {
            start,
            end,
            opening_pounds,
            opening_quote,
            closing_quote,
            closing_pounds,
            segment_specs,
        } = spec;
        let segments = segment_specs
            .into_iter()
            .map(|(segment_start, segment_end, text)| {
                self.with_name(
                    self.string_segment_node(segment_start, segment_end, text),
                    "",
                )
            })
            .collect::<Vec<_>>();
        self.string_literal_node_with_segments(StringLiteralNodeSpec {
            start,
            end,
            opening_pounds,
            opening_quote,
            closing_quote,
            closing_pounds,
            segments,
        })
    }

    fn string_literal_node_with_segments(&self, spec: StringLiteralNodeSpec) -> Value {
        let StringLiteralNodeSpec {
            start,
            end,
            opening_pounds,
            opening_quote,
            closing_quote,
            closing_pounds,
            segments,
        } = spec;
        let quote_kind = if opening_quote.1.saturating_sub(opening_quote.0) == 3 {
            "multilineStringQuote"
        } else {
            "stringQuote"
        };
        let mut children = Vec::new();
        if let Some((pounds_start, pounds_end)) = opening_pounds {
            children.push(self.with_name(
                self.token_with_range(
                    "rawStringPoundDelimiter",
                    self.range_from_offsets(pounds_start, pounds_end),
                ),
                "openingPounds",
            ));
        }
        children.push(self.with_name(
            self.token_with_range(
                quote_kind,
                self.range_from_offsets(opening_quote.0, opening_quote.1),
            ),
            "openingQuote",
        ));
        children.push(self.with_name(
            self.syntax_node(
                "StringLiteralSegmentListSyntax",
                self.covering_range_or_point(&segments, opening_quote.1),
                segments,
            ),
            "segments",
        ));
        children.push(self.with_name(
            self.token_with_range(
                quote_kind,
                self.range_from_offsets(closing_quote.0, closing_quote.1),
            ),
            "closingQuote",
        ));
        if let Some((pounds_start, pounds_end)) = closing_pounds {
            children.push(self.with_name(
                self.token_with_range(
                    "rawStringPoundDelimiter",
                    self.range_from_offsets(pounds_start, pounds_end),
                ),
                "closingPounds",
            ));
        }
        self.syntax_node(
            "StringLiteralExprSyntax",
            self.range_from_offsets(start, end),
            children,
        )
    }

    fn string_segment_node(&self, start: usize, end: usize, text: String) -> Value {
        self.syntax_node(
            "StringSegmentSyntax",
            self.range_from_offsets(start, end),
            vec![self.with_name(
                self.token_with_range(
                    &format!("stringSegment({})", quoted_text(&text)),
                    self.range_from_offsets(start, end),
                ),
                "content",
            )],
        )
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

    fn trim_offsets(&self, mut start: usize, mut end: usize) -> (usize, usize) {
        while start < end && self.source.as_bytes()[start].is_ascii_whitespace() {
            start += 1;
        }
        while end > start && self.source.as_bytes()[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        (start, end)
    }

    fn skip_horizontal_whitespace(&self, mut start: usize, end: usize) -> usize {
        while start < end && matches!(self.source.as_bytes()[start], b' ' | b'\t') {
            start += 1;
        }
        start
    }

    fn find_horizontal_whitespace(&self, start: usize, end: usize) -> Option<usize> {
        (start..end).find(|offset| matches!(self.source.as_bytes()[*offset], b' ' | b'\t'))
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

    fn value_field_child(&self, node: Node<'a>) -> Option<Node<'a>> {
        self.field_children(node, "value")
            .into_iter()
            .find(|child| !self.is_recovery_bang_node(*child))
    }

    fn expression_field_child(&self, node: Node<'a>, field: &str) -> Option<Node<'a>> {
        self.field_children(node, field)
            .into_iter()
            .find(|child| is_expression_field_candidate(*child))
    }

    fn recovered_value_end(&self, node: Node<'a>, value: Node<'a>) -> usize {
        let mut end = value.end_byte();
        for child in self.field_children(node, "value") {
            if child.start_byte() >= end
                && (self.is_recovery_bang_node(child)
                    || self.is_optional_chain_question_mark(child))
            {
                end = child.end_byte();
            }
        }
        let mut sibling = node.next_sibling();
        while let Some(next) = sibling {
            if next.start_byte() > end {
                break;
            }
            if !(self.is_recovery_bang_node(next) || self.is_optional_chain_question_mark(next)) {
                break;
            }
            end = end.max(next.end_byte());
            sibling = next.next_sibling();
        }
        end
    }

    fn is_recovery_bang_node(&self, node: Node<'a>) -> bool {
        if node.kind() == "bang" {
            return true;
        }
        if matches!(node.kind(), "custom_operator" | "ERROR") {
            let trimmed = self.text(node).trim();
            return !trimmed.is_empty() && trimmed.chars().all(|ch| ch == '!');
        }
        false
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

    fn immediate_type_identifiers(&self, node: Node<'a>) -> Vec<Node<'a>> {
        if node.kind() == "type_identifier" {
            vec![node]
        } else {
            named_children(node)
                .filter(|child| child.kind() == "type_identifier")
                .collect()
        }
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
        ["get", "set", "_read", "read", "_modify", "modify"]
            .iter()
            .find_map(|kind| {
                self.first_descendant_any_kind(node, kind)
                    .or_else(|| self.first_descendant_with_text(node, kind))
            })
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
        condition_end: usize,
    ) -> Option<Node<'a>> {
        named_children(node).find(|child| {
            child.start_byte() > keyword.end_byte()
                && child.end_byte() <= condition_end
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

    fn last_descendant_kind_before(
        &self,
        node: Node<'a>,
        kinds: &[&str],
        end: usize,
    ) -> Option<Node<'a>> {
        let mut found = None;
        self.last_descendant_kind_before_into(node, kinds, end, &mut found);
        found
    }

    fn last_descendant_kind_before_into(
        &self,
        node: Node<'a>,
        kinds: &[&str],
        end: usize,
        found: &mut Option<Node<'a>>,
    ) {
        for child in children(node) {
            if child.start_byte() >= end {
                continue;
            }
            if kinds.contains(&child.kind()) && child.end_byte() <= end {
                *found = Some(child);
            }
            self.last_descendant_kind_before_into(child, kinds, end, found);
        }
    }

    fn first_descendant_type_after(&self, node: Node<'a>, start: usize) -> Option<Node<'a>> {
        if is_type_syntax_node_kind(node.kind()) && node.start_byte() >= start {
            return Some(node);
        }
        for child in named_children(node) {
            if child.end_byte() < start {
                continue;
            }
            if let Some(found) = self.first_descendant_type_after(child, start) {
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

    fn first_descendant_with_text(&self, node: Node<'a>, text: &str) -> Option<Node<'a>> {
        if self.text(node) == text {
            return Some(node);
        }
        for child in children(node) {
            if let Some(found) = self.first_descendant_with_text(child, text) {
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

fn is_ignorable_directive(node: Node<'_>) -> bool {
    node.kind() == "directive"
}

fn starts_directive_keyword(text: &str, keyword: &str) -> bool {
    text.strip_prefix(keyword).is_some_and(|rest| {
        rest.is_empty()
            || rest
                .as_bytes()
                .first()
                .is_some_and(|byte| byte.is_ascii_whitespace())
    })
}

fn is_expression_like_node(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "additive_expression"
            | "array_literal"
            | "as_expression"
            | "await_expression"
            | "boolean_literal"
            | "call_expression"
            | "check_expression"
            | "comparison_expression"
            | "conjunction_expression"
            | "constructor_expression"
            | "dictionary_literal"
            | "disjunction_expression"
            | "equality_expression"
            | "integer_literal"
            | "key_path_expression"
            | "lambda_literal"
            | "line_string_literal"
            | "macro_invocation"
            | "multi_line_string_literal"
            | "multiplicative_expression"
            | "navigation_expression"
            | "nil_coalescing_expression"
            | "nil"
            | "prefix_expression"
            | "raw_string_literal"
            | "range_expression"
            | "real_literal"
            | "regex_literal"
            | "self_expression"
            | "simple_identifier"
            | "special_literal"
            | "super_expression"
            | "ternary_expression"
            | "try_expression"
            | "tuple_expression"
            | "user_type"
            | "value_pack_expansion"
            | "value_parameter_pack"
    )
}

fn is_expression_field_candidate(node: Node<'_>) -> bool {
    is_expression_like_node(node)
        || matches!(
            node.kind(),
            "assignment" | "directly_assignable_expression" | "if_statement" | "switch_statement"
        )
}

fn is_binary_expression_kind(kind: &str) -> bool {
    matches!(
        kind,
        "additive_expression"
            | "comparison_expression"
            | "conjunction_expression"
            | "disjunction_expression"
            | "equality_expression"
            | "infix_expression"
            | "multiplicative_expression"
            | "range_expression"
    )
}

fn is_type_syntax_node_kind(kind: &str) -> bool {
    matches!(
        kind,
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
    if first == '$' {
        return chars.all(|ch| ch == '_' || ch.is_alphanumeric());
    }
    (first == '_' || first.is_alphabetic()) && chars.all(|ch| ch == '_' || ch.is_alphanumeric())
}

fn is_identifier_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
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

fn normalize_escaped_raw_segment(text: &str) -> String {
    if let Some(suffix) = text.strip_prefix("\\\"\\\"\\\"") {
        format!("\\\"\\\"{suffix}")
    } else if let Some(suffix) = text.strip_prefix("\\\"\"\"") {
        format!("\\\"\\\"{suffix}")
    } else {
        text.to_string()
    }
}

fn raw_string_bounds(text: &str) -> Option<(usize, usize, usize)> {
    let opening_pounds_len = text.bytes().take_while(|byte| *byte == b'#').count();
    let quote_len = if text[opening_pounds_len..].starts_with("\"\"\"") {
        3
    } else if text[opening_pounds_len..].starts_with('"') {
        1
    } else {
        return None;
    };
    let closing_quote_start = text.len().checked_sub(opening_pounds_len + quote_len)?;
    Some((opening_pounds_len, quote_len, closing_quote_start))
}

fn end_offset(value: &Value) -> usize {
    value["range"]["endOffset"].as_u64().unwrap_or_default() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_labeled_statements() {
        let source =
            "loop: while foo() {\n  continue loop\n  break loop\n}\nGronk:\nswitch x { case 42: return }\n";
        let value = parse_source("Label.swift", "/tmp/Label.swift", source).unwrap();
        let labeled = find_node_types(&value, "LabeledStmtSyntax");
        assert_eq!(labeled.len(), 2);
        assert_eq!(
            child_by_name(labeled[0], "label").unwrap()["tokenKind"],
            "identifier(\"loop\")"
        );
        assert_eq!(
            child_by_name(labeled[0], "statement").unwrap()["nodeType"],
            "WhileStmtSyntax"
        );
        assert_eq!(
            child_by_name(labeled[1], "label").unwrap()["tokenKind"],
            "identifier(\"Gronk\")"
        );
        assert_eq!(
            child_by_name(labeled[1], "statement").unwrap()["nodeType"],
            "SwitchExprSyntax"
        );
        let continue_stmt = find_first_node_type(&value, "ContinueStmtSyntax").unwrap();
        assert_eq!(source_text(source, continue_stmt), "continue loop");
        assert_eq!(
            child_by_name(continue_stmt, "label").unwrap()["tokenKind"],
            "identifier(\"loop\")"
        );
        let break_stmt = find_first_node_type(&value, "BreakStmtSyntax").unwrap();
        assert_eq!(source_text(source, break_stmt), "break loop");
        assert_eq!(
            child_by_name(break_stmt, "label").unwrap()["tokenKind"],
            "identifier(\"loop\")"
        );
    }

    #[test]
    fn emits_switch_case_items_and_where_clauses() {
        let source = "switch x {\n  case _ where x % 2 == 0, 20:\n    x = 7\n}\n";
        let value = parse_source("Switch.swift", "/tmp/Switch.swift", source).unwrap();
        let switch_case = find_first_node_type(&value, "SwitchCaseSyntax").unwrap();
        let label = child_by_name(switch_case, "label").unwrap();
        let case_items = child_by_name(label, "caseItems").unwrap();
        let items = case_items["children"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(
            child_by_name(&items[0], "pattern").unwrap()["nodeType"],
            "WildcardPatternSyntax"
        );
        assert_eq!(
            child_by_name(&items[0], "whereClause").unwrap()["children"][1]["nodeType"],
            "InfixOperatorExprSyntax"
        );
        assert_eq!(
            child_by_name(&items[1], "pattern").unwrap()["children"][0]["nodeType"],
            "IntegerLiteralExprSyntax"
        );
    }

    #[test]
    fn recovers_embedded_switch_case_labels_from_error_nodes() {
        let source = r#"
switch x {
  case var var a:
  a += 1
  case var let a:
  print(a, terminator: "")
  case var (var b):
  b += 1
  case _:
  ()
}
"#;
        let value = parse_source("Switch.swift", "/tmp/Switch.swift", source).unwrap();
        let case_labels = find_node_types(&value, "SwitchCaseSyntax")
            .into_iter()
            .map(|switch_case| source_text(source, child_by_name(switch_case, "label").unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(
            case_labels,
            vec![
                "case var var a:",
                "case var let a:",
                "case var (var b):",
                "case _:"
            ]
        );

        let patterns = find_node_types(&value, "SwitchCaseItemSyntax")
            .into_iter()
            .map(|item| {
                child_by_name(item, "pattern").unwrap()["nodeType"]
                    .as_str()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            patterns,
            vec![
                "ValueBindingPatternSyntax",
                "ValueBindingPatternSyntax",
                "ValueBindingPatternSyntax",
                "WildcardPatternSyntax"
            ]
        );
    }

    #[test]
    fn recovers_attributed_switch_case_labels_from_merged_entries() {
        let case_source = r#"
switch Whatever.Thing {
  case .Thing:
  @unknown case _:
    x = 0
}
"#;
        let value = parse_source("Switch.swift", "/tmp/Switch.swift", case_source).unwrap();
        let case_labels = find_node_types(&value, "SwitchCaseSyntax")
            .into_iter()
            .map(|switch_case| {
                source_text(case_source, child_by_name(switch_case, "label").unwrap())
            })
            .collect::<Vec<_>>();
        assert_eq!(case_labels, vec!["case .Thing:", "case _:"]);

        let default_source = r#"
switch Whatever.Thing {
  case .Thing:
  @unknown default:
    x = 0
}
"#;
        let value = parse_source("Switch.swift", "/tmp/Switch.swift", default_source).unwrap();
        let case_labels = find_node_types(&value, "SwitchCaseSyntax")
            .into_iter()
            .map(|switch_case| {
                source_text(default_source, child_by_name(switch_case, "label").unwrap())
            })
            .collect::<Vec<_>>();
        assert_eq!(case_labels, vec!["case .Thing:", "default:"]);
    }

    #[test]
    fn skips_conditional_compilation_directives_in_code_blocks() {
        let source = "class C {\n  init() {\n  #if true\n    init()\n  #endif\n  }\n}\n";
        let value = parse_source("PoundIf.swift", "/tmp/PoundIf.swift", source).unwrap();
        assert_eq!(find_node_types(&value, "InitializerDeclSyntax").len(), 1);
        let calls = find_node_types(&value, "FunctionCallExprSyntax");
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0]["children"][0]["children"][0]["tokenKind"],
            "identifier(\"init\")"
        );
    }

    #[test]
    fn emits_if_config_declarations_for_directive_blocks() {
        let source = "foo()\n#if CONFIG1\nconfig1()\n#if CONFIG2\nconfig2()\n#else\nelse2()\n#endif\n#else\nelse1()\n#endif\nbar()\n";
        let value = parse_source("PoundIf.swift", "/tmp/PoundIf.swift", source).unwrap();
        let if_configs = find_node_types(&value, "IfConfigDeclSyntax");
        assert_eq!(if_configs.len(), 2);

        let outer_clauses = child_by_name(if_configs[0], "clauses").unwrap()["children"]
            .as_array()
            .unwrap();
        assert_eq!(outer_clauses.len(), 2);
        assert_eq!(
            child_by_name(&outer_clauses[0], "poundKeyword").unwrap()["tokenKind"],
            "poundIf"
        );
        assert_eq!(
            source_text(
                source,
                child_by_name(&outer_clauses[0], "condition").unwrap()
            ),
            "CONFIG1"
        );
        assert_eq!(
            child_by_name(&outer_clauses[1], "poundKeyword").unwrap()["tokenKind"],
            "poundElse"
        );

        let root_items = child_by_name(&value, "statements").unwrap()["children"]
            .as_array()
            .unwrap();
        assert_eq!(root_items.len(), 3);
        assert_eq!(
            root_items[1]["children"][0]["nodeType"],
            "IfConfigDeclSyntax"
        );
    }

    #[test]
    fn emits_postfix_if_config_expressions() {
        let source = "foo\n#if CONFIG1\n.bar()\n#else\n.baz()\n#endif\n";
        let value = parse_source(
            "PostfixIfConfig.swift",
            "/tmp/PostfixIfConfig.swift",
            source,
        )
        .unwrap();
        let postfix = find_first_node_type(&value, "PostfixIfConfigExprSyntax").unwrap();
        assert_eq!(
            source_text(source, child_by_name(postfix, "base").unwrap()),
            "foo"
        );

        let clauses = child_by_name(child_by_name(postfix, "config").unwrap(), "clauses").unwrap()
            ["children"]
            .as_array()
            .unwrap();
        assert_eq!(clauses.len(), 2);
        assert_eq!(
            child_by_name(&clauses[0], "elements").unwrap()["nodeType"],
            "FunctionCallExprSyntax"
        );
        assert_eq!(
            source_text(
                source,
                find_node_types(&clauses[0], "FunctionCallExprSyntax")[0]
            ),
            ".bar()"
        );
        assert_eq!(
            source_text(
                source,
                find_node_types(&clauses[1], "FunctionCallExprSyntax")[0]
            ),
            ".baz()"
        );
    }

    #[test]
    fn emits_postfix_if_config_expressions_with_trailing_calls() {
        let source = "foo\n#if CONFIG1\n.bar()\n#else\n.baz()\n#endif\n.oneMore(x: 1)\n";
        let value = parse_source(
            "PostfixIfConfig.swift",
            "/tmp/PostfixIfConfig.swift",
            source,
        )
        .unwrap();
        let root_items = child_by_name(&value, "statements").unwrap()["children"]
            .as_array()
            .unwrap();
        assert_eq!(root_items.len(), 1);

        let outer_call = child_by_name(&root_items[0], "item").unwrap();
        assert_eq!(outer_call["nodeType"], "FunctionCallExprSyntax");
        let member_access = child_by_name(outer_call, "calledExpression").unwrap();
        assert_eq!(member_access["nodeType"], "MemberAccessExprSyntax");
        assert_eq!(
            source_text(source, child_by_name(member_access, "declName").unwrap()),
            "oneMore"
        );
        assert_eq!(
            child_by_name(member_access, "base").unwrap()["nodeType"],
            "PostfixIfConfigExprSyntax"
        );
    }

    #[test]
    fn skips_source_location_and_conditional_attribute_fragments() {
        let source =
            "#sourceLocation(file: \"foo\", line: 42)\n@frozen\n#if hasAttribute(foo)\n@foo\n#endif\npublic struct S2 { }\n";
        let value = parse_source("Directive.swift", "/tmp/Directive.swift", source).unwrap();
        assert!(find_node_types(&value, "FunctionCallExprSyntax").is_empty());
        let class_decl = find_first_node_type(&value, "StructDeclSyntax").unwrap();
        assert_eq!(
            child_by_name(class_decl, "name").unwrap()["tokenKind"],
            "identifier(\"S2\")"
        );
    }

    #[test]
    fn emits_do_and_repeat_control_syntax() {
        let source = "\
let x = do { 5 } catch { 0 }
func foo() { do { 5 } catch { 0 } }
func casted() { do { 6 } as Int }
do { 8 } as Int
return do { 7 }
repeat { sink() } while x < 1
";
        let value = parse_source("Do.swift", "/tmp/Do.swift", source).unwrap();

        let do_exprs = find_node_types(&value, "DoExprSyntax");
        assert_eq!(do_exprs.len(), 3);
        assert_eq!(source_text(source, do_exprs[0]), "do { 5 } catch { 0 }");
        assert_eq!(
            do_exprs[0]["children"][0]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.do)"
        );
        assert!(do_exprs
            .iter()
            .any(|node| source_text(source, node) == "do { 8 }"));

        let do_stmts = find_node_types(&value, "DoStmtSyntax");
        assert_eq!(do_stmts.len(), 2);
        assert_eq!(source_text(source, do_stmts[0]), "do { 5 } catch { 0 }");
        assert_eq!(find_node_types(&value, "CatchClauseSyntax").len(), 2);
        assert_eq!(find_node_types(&value, "ReturnStmtSyntax").len(), 1);

        let repeat_stmts = find_node_types(&value, "RepeatStmtSyntax");
        assert_eq!(repeat_stmts.len(), 1);
        assert_eq!(
            repeat_stmts[0]["children"][2]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.while)"
        );

        let do_calls = find_node_types(&value, "FunctionCallExprSyntax")
            .into_iter()
            .filter(|node| source_text(source, node).starts_with("do "))
            .count();
        assert_eq!(do_calls, 0);
    }

    #[test]
    fn emits_interpolated_string_segments() {
        let source = "\"Fixit: \\(range.debugDescription)\"\n\"\\(x)\"\n\"Foo '\\(x)' bar\"\n";
        let value = parse_source("Interpolated.swift", "/tmp/Interpolated.swift", source).unwrap();
        let literals = find_node_types(&value, "StringLiteralExprSyntax");
        assert_eq!(literals.len(), 3);

        let first_expression_segments = find_node_types(literals[0], "ExpressionSegmentSyntax");
        assert_eq!(first_expression_segments.len(), 1);
        assert_eq!(
            find_first_node_type(first_expression_segments[0], "MemberAccessExprSyntax").unwrap()
                ["range"]["startOffset"],
            10
        );
        assert_eq!(
            find_node_types(literals[1], "ExpressionSegmentSyntax").len(),
            1
        );
        assert_eq!(
            find_node_types(literals[2], "ExpressionSegmentSyntax").len(),
            1
        );
        assert_eq!(find_node_types(literals[2], "StringSegmentSyntax").len(), 2);
    }

    #[test]
    fn merges_escaped_and_multiline_string_segments() {
        let source = "\"\\\\\\\"abc\"\n\"abc\\\\\\\"\"\n\"\"\"\nabc\ndef\n\"\"\"\n";
        let value =
            parse_source("LiteralStrings.swift", "/tmp/LiteralStrings.swift", source).unwrap();
        let literals = find_node_types(&value, "StringLiteralExprSyntax");
        assert_eq!(literals.len(), 3);
        assert_eq!(find_node_types(literals[0], "StringSegmentSyntax").len(), 1);
        assert_eq!(find_node_types(literals[1], "StringSegmentSyntax").len(), 1);
        assert_eq!(find_node_types(literals[2], "StringSegmentSyntax").len(), 2);
    }

    #[test]
    fn recovers_array_literal_without_commas() {
        let value =
            parse_source("ArrayNoComma.swift", "/tmp/ArrayNoComma.swift", "[() ()]\n").unwrap();
        let array = find_first_node_type(&value, "ArrayExprSyntax").unwrap();
        assert_eq!(array["range"]["startOffset"], 0);
        assert_eq!(array["range"]["endOffset"], 7);
        let elements = array["children"][1]["children"].as_array().unwrap();
        assert_eq!(elements.len(), 1);
        let tuple = &elements[0]["children"][0];
        assert_eq!(tuple["nodeType"], "TupleExprSyntax");
        assert_eq!(tuple["range"]["startOffset"], 1);
        assert_eq!(tuple["range"]["endOffset"], 6);
        assert_eq!(tuple["children"][1]["nodeType"], "LabeledExprListSyntax");
        assert!(tuple["children"][1]["children"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn emits_nested_type_specialization_calls() {
        let value = parse_source(
            "NestedType.swift",
            "/tmp/NestedType.swift",
            "Swift.Array<Array<Foo>>()\n",
        )
        .unwrap();
        let call = find_first_node_type(&value, "FunctionCallExprSyntax").unwrap();
        assert_eq!(call["range"]["startOffset"], 0);
        assert_eq!(call["range"]["endOffset"], 25);

        let specialization = &call["children"][0];
        assert_eq!(
            specialization["nodeType"],
            "GenericSpecializationExprSyntax"
        );
        let member_access = &specialization["children"][0];
        assert_eq!(member_access["nodeType"], "MemberAccessExprSyntax");
        assert_eq!(
            member_access["children"][0]["children"][0]["tokenKind"],
            "identifier(\"Swift\")"
        );
        assert_eq!(member_access["children"][1]["tokenKind"], "period");
        assert_eq!(
            member_access["children"][2]["children"][0]["tokenKind"],
            "identifier(\"Array\")"
        );

        let outer_argument =
            &specialization["children"][1]["children"][1]["children"][0]["children"][0];
        assert_eq!(outer_argument["nodeType"], "IdentifierTypeSyntax");
        assert_eq!(
            outer_argument["children"][0]["tokenKind"],
            "identifier(\"Array\")"
        );
        let inner_argument =
            &outer_argument["children"][1]["children"][1]["children"][0]["children"][0];
        assert_eq!(
            inner_argument["children"][0]["tokenKind"],
            "identifier(\"Foo\")"
        );
    }

    #[test]
    fn recovers_keyword_apply_calls() {
        let source = "optional(x: .some(23))\noptional(x: .none)\nvar pair : (Int, Double) = makePair(a: 1, b: 2.5)\n";
        let value = parse_source("KeywordApply.swift", "/tmp/KeywordApply.swift", source).unwrap();
        let statements = value["children"][0]["children"].as_array().unwrap();
        assert_eq!(statements.len(), 3);

        let first_call = &statements[0]["children"][0];
        assert_eq!(first_call["nodeType"], "FunctionCallExprSyntax");
        assert_eq!(first_call["range"]["startOffset"], 0);
        assert_eq!(first_call["range"]["endOffset"], 22);
        assert_eq!(
            first_call["children"][0]["children"][0]["tokenKind"],
            "identifier(\"optional\")"
        );
        let first_argument = &first_call["children"][2]["children"][0];
        assert_eq!(
            first_argument["children"][0]["tokenKind"],
            "identifier(\"x\")"
        );
        assert_eq!(
            first_argument["children"][2]["nodeType"],
            "FunctionCallExprSyntax"
        );
        assert_eq!(
            first_argument["children"][2]["children"][0]["nodeType"],
            "MemberAccessExprSyntax"
        );

        let second_call = &statements[1]["children"][0];
        assert_eq!(second_call["nodeType"], "FunctionCallExprSyntax");
        assert_eq!(second_call["range"]["startOffset"], 23);
        assert_eq!(second_call["range"]["endOffset"], 41);
        let second_argument = &second_call["children"][2]["children"][0];
        assert_eq!(
            second_argument["children"][2]["nodeType"],
            "MemberAccessExprSyntax"
        );

        let declaration = &statements[2]["children"][0];
        assert_eq!(declaration["nodeType"], "VariableDeclSyntax");
        let make_pair = find_node_types(declaration, "FunctionCallExprSyntax");
        assert_eq!(make_pair.len(), 1);
        assert_eq!(
            make_pair[0]["children"][0]["children"][0]["tokenKind"],
            "identifier(\"makePair\")"
        );
    }

    #[test]
    fn emits_raw_string_literals() {
        let source = r######"
_ = #"This is a string"#
_ = #####""Zeta""#####
_ = #""Eta"\#n\#n\#n\#""#
_ = #""Iota"\n\n\n\""#
"######;
        let value = parse_source("Raw.swift", "/tmp/Raw.swift", source).unwrap();
        let segments = find_node_types(&value, "StringSegmentSyntax");
        let segment_texts = segments
            .iter()
            .filter_map(|segment| segment["children"][0]["tokenKind"].as_str())
            .collect::<Vec<_>>();
        assert!(segment_texts.contains(&"stringSegment(\"This is a string\")"));
        assert!(segment_texts.contains(&"stringSegment(\"\\\"Zeta\\\"\")"));
        assert!(segment_texts.contains(&"stringSegment(\"\\\"Eta\\\"\\\\#n\")"));
        assert!(segment_texts.contains(&"stringSegment(\"\\\\#\\\"\")"));
        assert!(segment_texts.contains(&"stringSegment(\"\\\"Iota\\\"\\\\n\\\\n\\\\n\\\\\\\"\")"));
    }

    #[test]
    fn emits_regex_literals_as_regex_syntax() {
        let value = parse_source("Regex.swift", "/tmp/Regex.swift", "##/abc/#def/##").unwrap();
        let regex_literals = find_node_types(&value, "RegexLiteralExprSyntax");
        assert_eq!(regex_literals.len(), 1);
        assert_eq!(regex_literals[0]["children"][1]["tokenKind"], "regexSlash");
        assert_eq!(
            regex_literals[0]["children"][2]["tokenKind"],
            "regexLiteralPattern(\"abc/#def\")"
        );
    }

    #[test]
    fn recovers_regex_literals_in_argument_and_array_lists() {
        let source = "foo(/abc/, #/abc/#, ##/abc/##)\nlet arr = [/abc/, #/abc/#, ##/abc/##]\n";
        let value = parse_source("Regex.swift", "/tmp/Regex.swift", source).unwrap();
        let regex_literals = find_node_types(&value, "RegexLiteralExprSyntax");
        let patterns = regex_literals
            .iter()
            .filter_map(|literal| {
                literal["children"].as_array()?.iter().find_map(|child| {
                    child["tokenKind"]
                        .as_str()
                        .filter(|token| token.starts_with("regexLiteralPattern"))
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            patterns,
            vec![
                "regexLiteralPattern(\"abc\")",
                "regexLiteralPattern(\"abc\")",
                "regexLiteralPattern(\"abc\")",
                "regexLiteralPattern(\"abc\")",
                "regexLiteralPattern(\"abc\")",
                "regexLiteralPattern(\"abc\")",
            ]
        );
        assert_eq!(find_node_types(&value, "LabeledExprSyntax").len(), 3);
        assert_eq!(find_node_types(&value, "ArrayElementSyntax").len(), 3);
    }

    #[test]
    fn recovers_ownership_expressions() {
        let source = r#"
useString(borrow global)
let _ = copy global
let _ = consume global
let _ = _move global
let _ = copy $0
"#;
        let value = parse_source("Ownership.swift", "/tmp/Ownership.swift", source).unwrap();
        assert_eq!(find_node_types(&value, "BorrowExprSyntax").len(), 1);
        assert_eq!(find_node_types(&value, "CopyExprSyntax").len(), 2);
        assert_eq!(find_node_types(&value, "ConsumeExprSyntax").len(), 1);

        let references = find_node_types(&value, "DeclReferenceExprSyntax");
        let reference_tokens = references
            .iter()
            .filter_map(|reference| reference["children"][0]["tokenKind"].as_str())
            .collect::<Vec<_>>();
        assert!(reference_tokens.contains(&"identifier(\"global\")"));
        assert!(reference_tokens.contains(&"identifier(\"_move\")"));
        assert!(reference_tokens.contains(&"identifier(\"$0\")"));
    }

    #[test]
    fn recovers_contextual_move_and_borrow_continuations() {
        let source = r#"
func foo(msg: String) {
  _move msg
  use(_move msg)
  let b = (_move self).buffer
  _borrow msg
  use(_borrow msg)
  let c = (_borrow self).buffer
}
"#;
        let value = parse_source("Ownership.swift", "/tmp/Ownership.swift", source).unwrap();
        let references = find_node_types(&value, "DeclReferenceExprSyntax");
        let reference_tokens = references
            .iter()
            .filter_map(|reference| reference["children"][0]["tokenKind"].as_str())
            .collect::<Vec<_>>();
        assert!(reference_tokens.contains(&"identifier(\"_move\")"));
        assert!(reference_tokens.contains(&"identifier(\"_borrow\")"));
        assert_eq!(find_node_types(&value, "MemberAccessExprSyntax").len(), 2);
    }

    #[test]
    fn recovers_escaped_raw_multiline_literal_segments() {
        let source = "_ = ##\\\"\\\"\\\"\n  \"\"Alpha\"\"\n  \\\"\\\"\\\"##\n";
        let value = parse_source("Raw.swift", "/tmp/Raw.swift", source).unwrap();
        let segments = find_node_types(&value, "StringSegmentSyntax");
        let segment_texts = segments
            .iter()
            .filter_map(|segment| segment["children"][0]["tokenKind"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            segment_texts,
            vec![
                "stringSegment(\"\\\\\\\"\\\\\\\"\")",
                "stringSegment(\"\")",
                "stringSegment(\"\")",
                "stringSegment(\"\\\\\\\"\\\\\\\"##\")",
            ]
        );
    }

    #[test]
    fn emits_defer_statements() {
        let source = r#"
if score < 10 {
  defer {
    print(score)
  }
  defer {
    print("The score is:")
  }
  score += 5
}
"#;
        let value = parse_source("Defer.swift", "/tmp/Defer.swift", source).unwrap();
        let defers = find_node_types(&value, "DeferStmtSyntax");
        assert_eq!(defers.len(), 2);
        assert!(defers.iter().all(|defer_stmt| {
            defer_stmt["children"][0]["tokenKind"] == "keyword(SwiftSyntax.Keyword.defer)"
                && defer_stmt["children"][1]["nodeType"] == "CodeBlockSyntax"
        }));
        let calls = find_node_types(&value, "FunctionCallExprSyntax");
        let call_names = calls
            .iter()
            .filter_map(|call| call["children"][0]["children"][0]["tokenKind"].as_str())
            .collect::<Vec<_>>();
        assert!(!call_names.contains(&"identifier(\"defer\")"));
    }

    #[test]
    fn emits_typealias_declarations() {
        let source = r#"
typealias IntPair = (Int, Int)
typealias IntTriple = (Int, Int, Int)
typealias Foo1 = Int
typealias Recovery5 = Int, Float
typealias `switch` = Int
"#;
        let value = parse_source("Typealias.swift", "/tmp/Typealias.swift", source).unwrap();
        let aliases = find_node_types(&value, "TypeAliasDeclSyntax");
        assert_eq!(aliases.len(), 5);
        assert!(aliases.iter().all(|alias| {
            alias["children"][2]["tokenKind"] == "keyword(SwiftSyntax.Keyword.typealias)"
                && alias["children"][4]["nodeType"] == "TypeInitializerClauseSyntax"
        }));

        let tuple_types = find_node_types(&value, "TupleTypeSyntax");
        assert_eq!(tuple_types.len(), 2);
        assert_eq!(
            tuple_types[0]["children"][1]["children"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            tuple_types[1]["children"][1]["children"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            aliases[2]["children"][4]["children"][1]["nodeType"],
            "IdentifierTypeSyntax"
        );
        assert_eq!(
            aliases[4]["children"][3]["tokenKind"],
            "identifier(\"`switch`\")"
        );
    }

    #[test]
    fn emits_function_typealiases() {
        let source = r#"
typealias MyAlias = (_ a: Int, _ b: Double, _ c: Bool, _ d: String) -> Bool
typealias A = @attr1 @attr2(hello) (Int) -> Void
typealias AsyncFunc2 = () async throws -> ()
typealias AsyncFuncArray = [() async throws -> ()]
"#;
        let value =
            parse_source("FunctionTypes.swift", "/tmp/FunctionTypes.swift", source).unwrap();
        let function_types = find_node_types(&value, "FunctionTypeSyntax");
        assert_eq!(function_types.len(), 4);
        let array_types = find_node_types(&value, "ArrayTypeSyntax");
        assert_eq!(array_types.len(), 1);
        assert_eq!(
            child_by_name(array_types[0], "element").unwrap()["nodeType"],
            "FunctionTypeSyntax"
        );

        assert_eq!(
            function_types[0]["children"][1]["children"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert_eq!(
            function_types[0]["children"][1]["children"][0]["children"][0]["tokenKind"],
            "wildcard"
        );
        assert_eq!(
            function_types[0]["children"][1]["children"][0]["children"][1]["tokenKind"],
            "identifier(\"a\")"
        );
        assert_eq!(
            function_types[0]["children"][3]["nodeType"],
            "ReturnClauseSyntax"
        );

        assert_eq!(
            function_types[2]["children"][3]["nodeType"],
            "TypeEffectSpecifiersSyntax"
        );
        assert_eq!(
            function_types[2]["children"][3]["children"][0]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.async)"
        );
        assert_eq!(
            function_types[2]["children"][3]["children"][1]["nodeType"],
            "ThrowsClauseSyntax"
        );
        assert_eq!(
            function_types[2]["children"][4]["children"][1]["nodeType"],
            "TupleTypeSyntax"
        );
    }

    #[test]
    fn emits_dictionary_type_syntax() {
        let source = "func bar(_ : String) async -> [[String]: Array<String>] {}\n";
        let value = parse_source(
            "DictionaryTypes.swift",
            "/tmp/DictionaryTypes.swift",
            source,
        )
        .unwrap();
        let dictionary_types = find_node_types(&value, "DictionaryTypeSyntax");
        assert_eq!(dictionary_types.len(), 1);
        assert_eq!(
            source_text(source, dictionary_types[0]),
            "[[String]: Array<String>]"
        );
        assert_eq!(
            child_by_name(dictionary_types[0], "key").unwrap()["nodeType"],
            "ArrayTypeSyntax"
        );
        assert_eq!(
            child_by_name(dictionary_types[0], "value").unwrap()["nodeType"],
            "IdentifierTypeSyntax"
        );
        assert_eq!(
            child_by_name(dictionary_types[0], "colon").unwrap()["tokenKind"],
            "colon"
        );
    }

    #[test]
    fn emits_variadic_existential_suppressed_and_metatype_syntax() {
        let source = r#"
func f1<each T>(_ x: repeat each T) -> repeat each T {}
func use<each T>(_ value: repeat each T) { _ = (repeat each value) }
func opaque() -> some P {}
let foo: any ~Copyable = 0
typealias X = ~Copyable.Type
typealias Y = ~A.B.C
typealias Z1 = ~A?
typealias Z2 = ~A<T>
let _: G<repeat each T> = G()
let _ = G< >.self
"#;
        let value = parse_source("Types.swift", "/tmp/Types.swift", source).unwrap();

        let pack_expansion = find_node_types(&value, "PackExpansionTypeSyntax")
            .into_iter()
            .find(|node| source_text(source, node) == "repeat each T")
            .unwrap();
        assert_eq!(
            child_by_name(pack_expansion, "repeatKeyword").unwrap()["tokenKind"],
            "keyword(SwiftSyntax.Keyword.repeat)"
        );
        assert_eq!(
            child_by_name(pack_expansion, "repetitionPattern").unwrap()["nodeType"],
            "PackElementTypeSyntax"
        );

        let pack_element = find_node_types(&value, "PackElementTypeSyntax")
            .into_iter()
            .find(|node| source_text(source, node) == "each T")
            .unwrap();
        assert_eq!(
            child_by_name(pack_element, "eachKeyword").unwrap()["tokenKind"],
            "keyword(SwiftSyntax.Keyword.each)"
        );
        assert_eq!(
            child_by_name(pack_element, "pack").unwrap()["nodeType"],
            "IdentifierTypeSyntax"
        );

        let pack_expansion_expr = find_node_types(&value, "PackExpansionExprSyntax")
            .into_iter()
            .find(|node| source_text(source, node) == "repeat each value")
            .unwrap();
        assert_eq!(
            child_by_name(pack_expansion_expr, "repeatKeyword").unwrap()["tokenKind"],
            "keyword(SwiftSyntax.Keyword.repeat)"
        );
        assert_eq!(
            child_by_name(pack_expansion_expr, "repetitionPattern").unwrap()["nodeType"],
            "PackElementExprSyntax"
        );

        let pack_element_expr = find_node_types(&value, "PackElementExprSyntax")
            .into_iter()
            .find(|node| source_text(source, node) == "each value")
            .unwrap();
        assert_eq!(
            child_by_name(pack_element_expr, "eachKeyword").unwrap()["tokenKind"],
            "keyword(SwiftSyntax.Keyword.each)"
        );
        assert_eq!(
            child_by_name(pack_element_expr, "pack").unwrap()["nodeType"],
            "DeclReferenceExprSyntax"
        );

        let empty_generic_member_access = find_node_types(&value, "MemberAccessExprSyntax")
            .into_iter()
            .find(|node| source_text(source, node) == "G< >.self")
            .unwrap();
        assert_eq!(
            child_by_name(empty_generic_member_access, "base").unwrap()["children"][0]["tokenKind"],
            "identifier(\"G\")"
        );
        assert_eq!(
            child_by_name(empty_generic_member_access, "declName").unwrap()["children"][0]
                ["tokenKind"],
            "identifier(\"self\")"
        );

        let some_or_any_texts = find_node_types(&value, "SomeOrAnyTypeSyntax")
            .into_iter()
            .map(|node| source_text(source, node))
            .collect::<Vec<_>>();
        assert!(some_or_any_texts.contains(&"some P"));
        assert!(some_or_any_texts.contains(&"any ~Copyable"));

        let any_type = find_node_types(&value, "SomeOrAnyTypeSyntax")
            .into_iter()
            .find(|node| source_text(source, node) == "any ~Copyable")
            .unwrap();
        assert_eq!(
            child_by_name(any_type, "someOrAnySpecifier").unwrap()["tokenKind"],
            "keyword(SwiftSyntax.Keyword.any)"
        );
        assert_eq!(
            child_by_name(any_type, "constraint").unwrap()["nodeType"],
            "SuppressedTypeSyntax"
        );

        let suppressed = find_node_types(&value, "SuppressedTypeSyntax")
            .into_iter()
            .find(|node| source_text(source, node) == "~Copyable")
            .unwrap();
        assert_eq!(
            child_by_name(suppressed, "withoutTilde").unwrap()["tokenKind"],
            "prefixOperator(\"~\")"
        );
        assert_eq!(
            child_by_name(suppressed, "type").unwrap()["nodeType"],
            "IdentifierTypeSyntax"
        );

        let metatype = find_first_node_type(&value, "MetatypeTypeSyntax").unwrap();
        assert_eq!(source_text(source, metatype), "~Copyable.Type");
        assert_eq!(
            child_by_name(metatype, "baseType").unwrap()["nodeType"],
            "SuppressedTypeSyntax"
        );
        assert_eq!(
            child_by_name(metatype, "period").unwrap()["tokenKind"],
            "period"
        );
        assert_eq!(
            child_by_name(metatype, "metatypeSpecifier").unwrap()["tokenKind"],
            "keyword(SwiftSyntax.Keyword.Type)"
        );

        let alias_names = find_node_types(&value, "TypeAliasDeclSyntax")
            .into_iter()
            .map(|node| {
                child_by_name(node, "name").unwrap()["tokenKind"]
                    .as_str()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(alias_names.contains(&"identifier(\"X\")"));
        assert!(alias_names.contains(&"identifier(\"Y\")"));
        assert!(alias_names.contains(&"identifier(\"Z1\")"));
        assert!(alias_names.contains(&"identifier(\"Z2\")"));
    }

    #[test]
    fn unwraps_type_modifiers_before_function_type_annotations() {
        let source =
            "\nlet a: (a, b) -> c\nlet a: @MainActor (a, b) async throws -> c\n() -> (\\u{feff})\n";
        let value = parse_source("ClosureTypes.swift", "/tmp/ClosureTypes.swift", source).unwrap();
        let annotations = find_node_types(&value, "TypeAnnotationSyntax");
        assert_eq!(annotations.len(), 2);

        let plain_type = child_by_name(annotations[0], "type").unwrap();
        assert_eq!(plain_type["nodeType"], "FunctionTypeSyntax");
        assert_eq!(source_text(source, plain_type), "(a, b) -> c");

        let attributed_type = child_by_name(annotations[1], "type").unwrap();
        assert_eq!(attributed_type["nodeType"], "AttributedTypeSyntax");
        assert_eq!(
            source_text(source, attributed_type),
            "@MainActor (a, b) async throws -> c"
        );
        let attributed_type = child_by_name(attributed_type, "baseType").unwrap();
        assert_eq!(attributed_type["nodeType"], "FunctionTypeSyntax");
        assert_eq!(
            source_text(source, attributed_type),
            "(a, b) async throws -> c"
        );
        assert_eq!(
            child_by_name(attributed_type, "effectSpecifiers").unwrap()["nodeType"],
            "TypeEffectSpecifiersSyntax"
        );
    }

    #[test]
    fn emits_array_function_type_constructor_calls() {
        let source = "let _ = [() async -> ()]()\nlet _ = [() async throws -> ()]()\n";
        let value = parse_source("AsyncArray.swift", "/tmp/AsyncArray.swift", source).unwrap();
        let calls = find_node_types(&value, "FunctionCallExprSyntax");
        assert_eq!(calls.len(), 2);
        assert_eq!(source_text(source, calls[0]), "[() async -> ()]()");
        assert_eq!(
            child_by_name(calls[0], "calledExpression").unwrap()["nodeType"],
            "ArrayExprSyntax"
        );
        assert_eq!(
            child_by_name(
                child_by_name(calls[0], "calledExpression").unwrap(),
                "elements"
            )
            .unwrap()["children"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(source_text(source, calls[1]), "[() async throws -> ()]()");
    }

    #[test]
    fn emits_guard_statements() {
        let source = r#"
func noConditionNoElse() {
  guard {} else {}
}
while (i <= 10) {
  guard i % 2 == 0 else {
    i = i + 1
    continue
  }
  print(i)
}
func checkAge() {
  guard let myAge = age else {
    return
  }
}
func checkJobEligibility() {
  guard age >= 18, age <= 40 else {
    return
  }
}
"#;
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let guards = find_node_types(&value, "GuardStmtSyntax");
        assert_eq!(guards.len(), 4);
        assert!(guards.iter().all(|node| {
            node["children"][0]["tokenKind"] == "keyword(SwiftSyntax.Keyword.guard)"
                && node["children"][2]["tokenKind"] == "keyword(SwiftSyntax.Keyword.else)"
                && node["children"][3]["nodeType"] == "CodeBlockSyntax"
        }));

        assert_eq!(
            guards[0]["children"][1]["children"][0]["children"][0]["nodeType"],
            "ClosureExprSyntax"
        );
        assert_eq!(
            guards[2]["children"][1]["children"][0]["children"][0]["nodeType"],
            "OptionalBindingConditionSyntax"
        );
        assert_eq!(
            guards[2]["children"][1]["children"][0]["children"][0]["children"][0]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.let)"
        );
        assert_eq!(
            guards[3]["children"][1]["children"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            guards[3]["children"][1]["children"][0]["children"][1]["tokenKind"],
            "comma"
        );
    }

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
    fn skips_leading_async_recovery_error_before_function() {
        let source = "async func asyncIncorrectly() { }\n";
        let value = parse_source("Async.swift", "/tmp/Async.swift", source).unwrap();
        let function = find_first_node_type(&value, "FunctionDeclSyntax").unwrap();
        assert_eq!(
            child_by_name(function, "name").unwrap()["tokenKind"],
            "identifier(\"asyncIncorrectly\")"
        );
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
    fn emits_multiple_trailing_closure_function_call() {
        let source =
            "func f() { routes.get(\"find\") { req in User() } onFailure: { req in Error() } }\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let call = find_node_types(&value, "FunctionCallExprSyntax")
            .into_iter()
            .find(|node| source_text(source, node).starts_with("routes.get"))
            .unwrap();

        assert_eq!(
            child_by_name(call, "arguments").unwrap()["children"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            source_text(source, child_by_name(call, "trailingClosure").unwrap()),
            "{ req in User() }"
        );

        let additional = child_by_name(call, "additionalTrailingClosures").unwrap();
        let elements = additional["children"].as_array().unwrap();
        assert_eq!(elements.len(), 1);
        assert_eq!(
            elements[0]["nodeType"],
            "MultipleTrailingClosureElementSyntax"
        );
        assert_eq!(
            child_by_name(&elements[0], "label").unwrap()["tokenKind"],
            "identifier(\"onFailure\")"
        );
        assert_eq!(
            child_by_name(&elements[0], "colon").unwrap()["tokenKind"],
            "colon"
        );
        assert_eq!(
            source_text(source, child_by_name(&elements[0], "closure").unwrap()),
            "{ req in Error() }"
        );
    }

    #[test]
    fn flattens_parenthesized_call_with_trailing_closure() {
        let source =
            "func f() {\n  let result = Helper.map(41) { value in\n    return value + 1\n  }\n}\n";
        let value = parse_source("Sources/main.swift", "/tmp/Sources/main.swift", source).unwrap();
        let calls = find_node_types(&value, "FunctionCallExprSyntax")
            .into_iter()
            .filter(|node| source_text(source, node).starts_with("Helper.map"))
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 1);

        let call = calls[0];
        assert_eq!(
            child_by_name(call, "calledExpression").unwrap()["nodeType"],
            "MemberAccessExprSyntax"
        );
        assert_eq!(
            source_text(source, child_by_name(call, "calledExpression").unwrap()),
            "Helper.map"
        );
        assert_eq!(
            child_by_name(call, "arguments").unwrap()["children"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            source_text(source, child_by_name(call, "trailingClosure").unwrap()),
            "{ value in\n    return value + 1\n  }"
        );
    }

    #[test]
    fn emits_closure_captures_and_internal_parameter_names() {
        let source = "\
let g = { [weak self, weak weakB = b] foo in
  return 0
}
_ = { (_const x: Int) in }
_ = { (_ x: MyType) in }
_ = { (x y: MyType) in }
";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let capture_clause = find_first_node_type(&value, "ClosureCaptureClauseSyntax").unwrap();
        assert_eq!(capture_clause["children"][0]["tokenKind"], "leftSquare");
        assert_eq!(capture_clause["children"][2]["tokenKind"], "rightSquare");
        let captures = find_node_types(capture_clause, "ClosureCaptureSyntax");
        assert_eq!(captures.len(), 2);
        assert_eq!(
            captures[0]["children"][1]["tokenKind"],
            "keyword(SwiftSyntax.Keyword.self)"
        );
        assert_eq!(
            captures[1]["children"][1]["tokenKind"],
            "identifier(\"weakB\")"
        );
        assert_eq!(
            captures[1]["children"][2]["nodeType"],
            "InitializerClauseSyntax"
        );

        let second_names = find_node_types(&value, "ClosureParameterSyntax")
            .iter()
            .filter_map(|parameter| {
                parameter["children"].as_array().and_then(|children| {
                    children
                        .iter()
                        .find(|child| child["name"] == "secondName")
                        .and_then(|child| child["tokenKind"].as_str())
                })
            })
            .collect::<Vec<_>>();
        assert!(second_names.contains(&"identifier(\"x\")"));
        assert!(second_names.contains(&"identifier(\"y\")"));

        let shorthand = find_first_node_type(&value, "ClosureShorthandParameterSyntax").unwrap();
        assert_eq!(shorthand["children"][0]["tokenKind"], "identifier(\"foo\")");
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
    fn emits_availability_conditions() {
        let source = r#"
if #available(OSX 10.51, *), #available(OSX 10.52, *) {}
if let _ = Optional(5), #unavailable(OSX 10.52, *) {}
if case 42 = 42, #available(iOS 8.0, *) {}
if #available(*) {}
"#;
        let value = parse_source("Availability.swift", "/tmp/Availability.swift", source).unwrap();
        let availability = find_node_types(&value, "AvailabilityConditionSyntax");
        assert_eq!(availability.len(), 5);
        assert_eq!(
            child_by_name(availability[0], "availabilityKeyword").unwrap()["tokenKind"],
            "poundAvailable"
        );
        assert_eq!(
            source_text(source, availability[0]),
            "#available(OSX 10.51, *)"
        );
        assert_eq!(
            child_by_name(availability[2], "availabilityKeyword").unwrap()["tokenKind"],
            "poundUnavailable"
        );

        let first_arguments = child_by_name(availability[0], "availabilityArguments").unwrap()
            ["children"]
            .as_array()
            .unwrap();
        assert_eq!(first_arguments.len(), 2);
        assert_eq!(source_text(source, &first_arguments[0]), "OSX 10.51,");
        assert_eq!(
            child_by_name(&first_arguments[0], "argument").unwrap()["nodeType"],
            "PlatformVersionSyntax"
        );
        assert_eq!(
            child_by_name(&first_arguments[1], "argument").unwrap()["tokenKind"],
            "binaryOperator(\"*\")"
        );

        let condition_lists = find_node_types(&value, "ConditionElementListSyntax");
        let condition_counts = condition_lists
            .iter()
            .map(|list| list["children"].as_array().unwrap().len())
            .collect::<Vec<_>>();
        assert_eq!(condition_counts, vec![2, 2, 2, 1]);

        let optional_binding =
            find_first_node_type(&value, "OptionalBindingConditionSyntax").unwrap();
        assert_eq!(
            child_by_name(optional_binding, "pattern").unwrap()["nodeType"],
            "WildcardPatternSyntax"
        );

        let matching_pattern =
            find_first_node_type(&value, "MatchingPatternConditionSyntax").unwrap();
        assert_eq!(source_text(source, matching_pattern), "case 42 = 42");
        assert_eq!(
            child_by_name(matching_pattern, "pattern").unwrap()["nodeType"],
            "ExpressionPatternSyntax"
        );
    }

    #[test]
    fn emits_matching_pattern_and_tuple_binding_conditions() {
        let source = "\
if case let (a, b) = x {}
if case (let c, 1) = x {}
if let (d, e) = x {}
if case let E<Int>.e(y) = x {}
if case let .Naught(value) = n {}
";
        let value = parse_source("Patterns.swift", "/tmp/Patterns.swift", source).unwrap();
        let matching_patterns = find_node_types(&value, "MatchingPatternConditionSyntax");
        let matching_texts = matching_patterns
            .iter()
            .map(|node| source_text(source, node))
            .collect::<Vec<_>>();
        assert_eq!(
            matching_texts,
            vec![
                "case let (a, b) = x",
                "case (let c, 1) = x",
                "case let E<Int>.e(y) = x",
                "case let .Naught(value) = n"
            ]
        );

        let first_pattern = child_by_name(matching_patterns[0], "pattern").unwrap();
        assert_eq!(first_pattern["nodeType"], "ValueBindingPatternSyntax");
        assert_eq!(
            child_by_name(first_pattern, "pattern").unwrap()["nodeType"],
            "TuplePatternSyntax"
        );
        assert_eq!(
            child_by_name(matching_patterns[1], "pattern").unwrap()["nodeType"],
            "TuplePatternSyntax"
        );

        let optional_binding =
            find_first_node_type(&value, "OptionalBindingConditionSyntax").unwrap();
        assert_eq!(
            child_by_name(optional_binding, "pattern").unwrap()["nodeType"],
            "TuplePatternSyntax"
        );

        let call_texts = find_node_types(&value, "FunctionCallExprSyntax")
            .iter()
            .map(|node| source_text(source, node))
            .collect::<Vec<_>>();
        assert!(call_texts.contains(&"E<Int>.e(y)"));
        assert!(call_texts.contains(&".Naught(value)"));
    }

    #[test]
    fn recovers_statement_suite_shapes() {
        let if_source = "if let baz {}\nif let self = self {}\n";
        let if_value = parse_source("If.swift", "/tmp/If.swift", if_source).unwrap();
        let optional_bindings = find_node_types(&if_value, "OptionalBindingConditionSyntax");
        assert_eq!(optional_bindings.len(), 2);
        assert_eq!(
            source_text(
                if_source,
                child_by_name(optional_bindings[0], "pattern").unwrap()
            ),
            "baz"
        );
        assert!(child_by_name(optional_bindings[0], "initializer").is_none());
        assert!(child_by_name(optional_bindings[1], "initializer").is_some());

        let catch_source = "do {\n  try foo()\n} catch {\n  bar()\n}\ndo { try foo() }\ncatch where (error as NSError) == NSError() {}\n";
        let catch_value = parse_source("Catch.swift", "/tmp/Catch.swift", catch_source).unwrap();
        assert_eq!(find_node_types(&catch_value, "CatchClauseSyntax").len(), 2);
        let catch_items = find_node_types(&catch_value, "CatchItemSyntax");
        assert_eq!(catch_items.len(), 1);
        let where_clause = child_by_name(catch_items[0], "whereClause").unwrap();
        assert_eq!(
            child_by_name(where_clause, "condition").unwrap()["nodeType"],
            "InfixOperatorExprSyntax"
        );

        let missing_if_source = "if _ = 42 {}\n";
        let missing_if_value =
            parse_source("MissingIf.swift", "/tmp/MissingIf.swift", missing_if_source).unwrap();
        let recovered_if = find_first_node_type(&missing_if_value, "IfExprSyntax").unwrap();
        assert_eq!(source_text(missing_if_source, recovered_if), "if _ = 42 {}");

        let return_source = "return actor\n{ return 0 }\nreturn\n";
        let return_value =
            parse_source("Return.swift", "/tmp/Return.swift", return_source).unwrap();
        let return_stmts = find_node_types(&return_value, "ReturnStmtSyntax");
        let return_texts = return_stmts
            .iter()
            .map(|node| source_text(return_source, node))
            .collect::<Vec<_>>();
        assert!(return_texts.contains(&"return actor\n{ return 0 }"));
        assert!(return_texts.contains(&"return 0"));
        assert!(return_texts.contains(&"return"));
        assert!(find_first_node_type(&return_value, "ClosureExprSyntax").is_some());

        let yield_source =
            "var x: Int {\n  _read {\n    yield &x\n  }\n}\nfunc f() -> Int {\n  yield 5\n}\n";
        let yield_value = parse_source("Yield.swift", "/tmp/Yield.swift", yield_source).unwrap();
        assert_eq!(find_node_types(&yield_value, "YieldStmtSyntax").len(), 2);
        assert_eq!(find_node_types(&yield_value, "InOutExprSyntax").len(), 1);
        let accessor = find_first_node_type(&yield_value, "AccessorDeclSyntax").unwrap();
        assert_eq!(
            child_by_name(accessor, "accessorSpecifier").unwrap()["tokenKind"],
            "keyword(SwiftSyntax.Keyword._read)"
        );
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
    fn recovers_prefix_slash_operator_expressions() {
        let source = "\
prefix operator /
prefix func / <T> (_ x: T) -> T { x }
_ = /E.e
(/E.e).foo(/0)
foo(/E.e, /E.e)
foo((/E.e), /E.e)
foo((/)(E.e), /E.e)
_ = bar(/E.e) / 2
";
        let value = parse_source("PrefixSlash.swift", "/tmp/PrefixSlash.swift", source).unwrap();

        let operator = find_first_node_type(&value, "OperatorDeclSyntax").unwrap();
        assert_eq!(
            child_by_name(operator, "name").unwrap()["tokenKind"],
            "prefixOperator(\"/\")"
        );

        let prefixes = find_node_types(&value, "PrefixOperatorExprSyntax");
        let prefix_texts = prefixes
            .iter()
            .map(|node| source_text(source, node))
            .collect::<Vec<_>>();
        assert!(prefix_texts.contains(&"/E.e"));
        assert!(prefix_texts.contains(&"/0"));
        assert!(prefixes.iter().any(|node| {
            source_text(source, node) == "/E.e"
                && child_by_name(node, "expression")
                    .is_some_and(|expression| expression["nodeType"] == "MemberAccessExprSyntax")
        }));

        let calls = find_node_types(&value, "FunctionCallExprSyntax");
        let call_texts = calls
            .iter()
            .map(|node| source_text(source, node))
            .collect::<Vec<_>>();
        assert!(call_texts.contains(&"(/E.e).foo(/0)"));
        assert!(call_texts.contains(&"foo(/E.e, /E.e)"));
        assert!(call_texts.contains(&"foo((/E.e), /E.e)"));
        assert!(call_texts.contains(&"foo((/)(E.e), /E.e)"));
        assert!(call_texts.contains(&"bar(/E.e)"));
    }

    #[test]
    fn emits_optional_chaining_expressions() {
        let source = "\
var c = a?
var d : ()? = a?.foo()
var e : (() -> A)?
var f = e?()
var g = foo?.bar ?? 0
";
        let value = parse_source("Optional.swift", "/tmp/Optional.swift", source).unwrap();

        let optional_texts = find_node_types(&value, "OptionalChainingExprSyntax")
            .iter()
            .map(|node| source_text(source, node))
            .collect::<Vec<_>>();
        assert!(optional_texts.contains(&"a?"));
        assert!(optional_texts.contains(&"e?"));

        let initializers = find_node_types(&value, "InitializerClauseSyntax");
        assert!(initializers
            .iter()
            .any(|node| source_text(source, node) == "= a?"));

        let calls = find_node_types(&value, "FunctionCallExprSyntax");
        let optional_call = calls
            .iter()
            .find(|node| source_text(source, node) == "e?()")
            .unwrap();
        assert_eq!(
            child_by_name(optional_call, "calledExpression").unwrap()["nodeType"],
            "OptionalChainingExprSyntax"
        );

        let foo_call = calls
            .iter()
            .find(|node| source_text(source, node) == "a?.foo()")
            .unwrap();
        let member = child_by_name(foo_call, "calledExpression").unwrap();
        assert_eq!(member["nodeType"], "MemberAccessExprSyntax");
        assert_eq!(
            child_by_name(member, "base").unwrap()["nodeType"],
            "OptionalChainingExprSyntax"
        );

        let nil_coalescing = find_node_types(&value, "InfixOperatorExprSyntax")
            .into_iter()
            .find(|node| source_text(source, node) == "foo?.bar ?? 0")
            .unwrap();
        assert_eq!(
            child_by_name(
                child_by_name(nil_coalescing, "operator").unwrap(),
                "operator"
            )
            .unwrap()["tokenKind"],
            "binaryOperator(\"??\")"
        );
    }

    #[test]
    fn recovers_inout_operands_in_binary_arguments() {
        let source = "let d = Data(a: &b + offset, count: &c - offset)\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let inouts = find_node_types(&value, "InOutExprSyntax");
        let inout_texts = inouts
            .iter()
            .map(|node| source_text(source, node))
            .collect::<Vec<_>>();
        assert_eq!(inout_texts, vec!["&b", "&c"]);
        assert!(inouts.iter().all(|node| {
            node["children"][0]["name"] == "ampersand"
                && node["children"][0]["tokenKind"] == "prefixAmpersand"
                && node["children"][1]["nodeType"] == "DeclReferenceExprSyntax"
        }));

        let infixes = find_node_types(&value, "InfixOperatorExprSyntax");
        let infix_texts = infixes
            .iter()
            .map(|node| source_text(source, node))
            .collect::<Vec<_>>();
        assert!(infix_texts.contains(&"&b + offset"));
        assert!(infix_texts.contains(&"&c - offset"));
    }

    #[test]
    fn emits_try_ternary_cast_await_and_constructor_expressions() {
        let bang_source = "let a = (try? foo())!!";
        let bang_value = parse_source("Bang.swift", "/tmp/Bang.swift", bang_source).unwrap();
        let first_initializer = find_node_types(&bang_value, "InitializerClauseSyntax")
            .into_iter()
            .find(|node| node["range"]["startOffset"].as_u64() == Some(6))
            .unwrap();
        assert_eq!(
            first_initializer["range"]["endOffset"].as_u64(),
            Some(bang_source.len() as u64)
        );
        assert_eq!(find_node_types(first_initializer, "TryExprSyntax").len(), 1);

        let source = "\
let b = true ? try? foo() : try? bar() + 0
let c: Int? = try? produceAny() as? Int
let d = await fetch()
let e = Foo<Int>()
let f = value is Foo
";
        let value = parse_source("Expressions.swift", "/tmp/Expressions.swift", source).unwrap();

        assert_eq!(find_node_types(&value, "TernaryExprSyntax").len(), 1);
        assert!(find_node_types(&value, "TryExprSyntax").len() >= 3);
        assert_eq!(find_node_types(&value, "AsExprSyntax").len(), 1);
        assert_eq!(find_node_types(&value, "IsExprSyntax").len(), 1);
        assert_eq!(find_node_types(&value, "AwaitExprSyntax").len(), 1);
        assert_eq!(
            find_node_types(&value, "GenericSpecializationExprSyntax").len(),
            1
        );

        let nested =
            parse_source("Nested.swift", "/tmp/Nested.swift", "a ? b : c ? d : e").unwrap();
        let outer_ternary = find_first_node_type(&nested, "TernaryExprSyntax").unwrap();
        assert_eq!(
            outer_ternary["children"][0]["children"][0]["tokenKind"],
            "identifier(\"a\")"
        );
        assert_eq!(
            outer_ternary["children"][4]["nodeType"],
            "TernaryExprSyntax"
        );
    }

    #[test]
    fn emits_macro_expansion_expressions_and_declarations() {
        let source = "\
#file == $0.path
let a = #embed(\"filename.txt\")
#Test {
  print(\"This is a test\")
}
#fancyMacro<Arg1, Arg2>(hello: \"me\")
";
        let value = parse_source("Macros.swift", "/tmp/Macros.swift", source).unwrap();
        let bare_value = parse_source(
            "BareMacro.swift",
            "/tmp/BareMacro.swift",
            "let b = #notAPound",
        )
        .unwrap();
        let diagnostic_value = parse_source(
            "Diagnostic.swift",
            "/tmp/Diagnostic.swift",
            "#error(\"Unsupported platform\")",
        )
        .unwrap();

        let expr_macros = find_node_types(&value, "MacroExpansionExprSyntax");
        let decl_macros = find_node_types(&value, "MacroExpansionDeclSyntax");
        assert_eq!(
            find_node_types(&value, "GenericArgumentClauseSyntax").len(),
            1
        );
        assert_eq!(find_node_types(&value, "ClosureExprSyntax").len(), 1);

        let macro_names = expr_macros
            .into_iter()
            .chain(decl_macros)
            .filter_map(|macro_node| {
                macro_node["children"].as_array().and_then(|children| {
                    children
                        .iter()
                        .find(|child| child["name"] == "macroName")
                        .and_then(|child| child["tokenKind"].as_str())
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(macro_names.len(), 4);
        assert!(macro_names.contains(&"identifier(\"file\")"));
        assert!(macro_names.contains(&"identifier(\"embed\")"));
        assert!(macro_names.contains(&"identifier(\"Test\")"));
        assert!(macro_names.contains(&"identifier(\"fancyMacro\")"));

        let bare_macro = find_first_node_type(&bare_value, "MacroExpansionExprSyntax").unwrap();
        let bare_macro_name = bare_macro["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|child| child["name"] == "macroName")
            .and_then(|child| child["tokenKind"].as_str());
        assert_eq!(bare_macro_name, Some("identifier(\"notAPound\")"));

        let diagnostic =
            find_first_node_type(&diagnostic_value, "MacroExpansionDeclSyntax").unwrap();
        let diagnostic_name = diagnostic["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|child| child["name"] == "macroName")
            .and_then(|child| child["tokenKind"].as_str());
        assert_eq!(diagnostic_name, Some("identifier(\"error\")"));
        assert_eq!(
            find_node_types(diagnostic, "StringLiteralExprSyntax").len(),
            1
        );
    }

    #[test]
    fn emits_key_path_expressions() {
        let source =
            "\\a.b.c\n\\ABCProtocol[100]\nchildren.filter(\\.type.defaultInitialization.isEmpty)\n";
        let value = parse_source("KeyPaths.swift", "/tmp/KeyPaths.swift", source).unwrap();
        let key_paths = find_node_types(&value, "KeyPathExprSyntax");
        let key_path_texts = key_paths
            .iter()
            .map(|node| source_text(source, node))
            .collect::<Vec<_>>();
        assert_eq!(
            key_path_texts,
            vec![
                "\\a.b.c",
                "\\ABCProtocol[100]",
                "\\.type.defaultInitialization.isEmpty"
            ]
        );
        assert!(key_paths.iter().all(|node| {
            node["children"][0]["name"] == "backslash"
                && node["children"][0]["tokenKind"] == "backslash"
                && node["children"][1]["name"] == "components"
                && node["children"][1]["nodeType"] == "KeyPathComponentListSyntax"
        }));

        let calls = find_node_types(&value, "FunctionCallExprSyntax");
        assert!(calls.iter().any(|call| {
            source_text(source, call) == "children.filter(\\.type.defaultInitialization.isEmpty)"
        }));
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

    fn child_by_name<'v>(value: &'v Value, name: &str) -> Option<&'v Value> {
        value
            .get("children")
            .and_then(Value::as_array)
            .and_then(|children| children.iter().find(|child| child["name"] == name))
    }

    fn source_text<'s>(source: &'s str, value: &Value) -> &'s str {
        let start = value["range"]["startOffset"].as_u64().unwrap() as usize;
        let end = end_offset(value);
        &source[start..end]
    }
}
