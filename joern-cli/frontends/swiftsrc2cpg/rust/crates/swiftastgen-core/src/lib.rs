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
    if root.has_error() {
        bail!("Swift parse contains syntax errors");
    }

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
            if child.kind() == "shebang_line" {
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

    fn code_block_item(&self, node: Node<'a>) -> Result<Value> {
        let item = self.with_name(self.syntax_for_statement(node)?, "item");
        Ok(self.syntax_node("CodeBlockItemSyntax", self.range_for_node(node), vec![item]))
    }

    fn syntax_for_statement(&self, node: Node<'a>) -> Result<Value> {
        match node.kind() {
            "property_declaration" => self.variable_decl(node),
            "function_declaration" => self.function_decl(node),
            "integer_literal" | "simple_identifier" | "line_string_literal" => self.expr(node),
            other => bail!("unsupported Swift syntax node '{other}'"),
        }
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

        let mut binding_children =
            vec![self.with_name(self.identifier_pattern(pattern_node)?, "pattern")];
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
                self.with_name(
                    self.empty_collection("AttributeListSyntax", node.start_byte()),
                    "attributes",
                ),
                self.with_name(
                    self.empty_collection("DeclModifierListSyntax", node.start_byte()),
                    "modifiers",
                ),
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
            self.with_name(
                self.empty_collection("AttributeListSyntax", node.start_byte()),
                "attributes",
            ),
            self.with_name(
                self.empty_collection("DeclModifierListSyntax", node.start_byte()),
                "modifiers",
            ),
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
                self.with_name(
                    self.empty_collection("AttributeListSyntax", node.start_byte()),
                    "attributes",
                ),
                self.with_name(
                    self.empty_collection("DeclModifierListSyntax", node.start_byte()),
                    "modifiers",
                ),
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
        let statement_nodes: Vec<_> = named_children(node)
            .find(|child| child.kind() == "statements")
            .map(named_children)
            .into_iter()
            .flatten()
            .collect();
        let mut items = Vec::new();
        for child in statement_nodes {
            items.push(self.code_block_item(child)?);
        }
        let statements_range = self.covering_range_or_point(&items, left_brace.end_byte());
        Ok(self.syntax_node(
            "CodeBlockSyntax",
            self.range_for_node(node),
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
            "line_string_literal" => self.string_literal(node),
            other => bail!("unsupported Swift expression node '{other}'"),
        }
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

    fn immediate_child_kind(&self, node: Node<'a>, kind: &str) -> Option<Node<'a>> {
        children(node).find(|child| child.kind() == kind)
    }

    fn immediate_named_child_kind(&self, node: Node<'a>, kind: &str) -> Option<Node<'a>> {
        named_children(node).find(|child| child.kind() == kind)
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
    fn emits_function_with_body() {
        let source = "func foo() {\n  let z = x\n}\n";
        let value = parse_source("Test.swift", "/tmp/Test.swift", source).unwrap();
        let function = &value["children"][0]["children"][0]["children"][0];
        assert_eq!(function["nodeType"], "FunctionDeclSyntax");
        assert_eq!(function["children"][3]["tokenKind"], "identifier(\"foo\")");
        assert_eq!(function["children"][5]["nodeType"], "CodeBlockSyntax");
    }
}
