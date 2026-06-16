use anyhow::{Context, Result};
use serde::Serialize;
use std::fs;
use std::path::Path;
use tree_sitter::{Node, Parser};

pub const SCHEMA_VERSION: u32 = 1;
pub const BACKEND_NAME: &str = "oxidized-cxxastgen-scaffold";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseOptions {
    pub include_paths: Vec<String>,
    pub defines: Vec<String>,
    pub compilation_database: Option<String>,
    pub skip_function_bodies: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SourceLanguage {
    C,
    Cpp,
    Header,
    Preprocessed,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CxxAstDocument {
    pub schema_version: u32,
    pub backend: &'static str,
    pub path: String,
    pub language: SourceLanguage,
    pub source_bytes: u64,
    pub source_lines: usize,
    pub options: ParseOptions,
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Declaration {
    Macro(MacroDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    Function(FunctionDecl),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroDecl {
    pub name: String,
    pub code: String,
    pub line: usize,
    pub parameters: Vec<String>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructDecl {
    pub name: String,
    pub code: String,
    pub line: usize,
    pub fields: Vec<FieldDecl>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldDecl {
    pub name: String,
    pub type_name: String,
    pub code: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumDecl {
    pub name: String,
    pub code: String,
    pub line: usize,
    pub variants: Vec<EnumVariant>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumVariant {
    pub name: String,
    pub value: Option<String>,
    pub code: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionDecl {
    pub name: String,
    pub return_type: String,
    pub signature: String,
    pub code: String,
    pub line: usize,
    pub parameters: Vec<ParameterDecl>,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterDecl {
    pub name: String,
    pub type_name: String,
    pub code: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Statement {
    LocalDecl {
        name: String,
        #[serde(rename = "typeName")]
        type_name: String,
        code: String,
        line: usize,
        initializer: Option<Expression>,
    },
    Assignment {
        code: String,
        line: usize,
        left: Expression,
        right: Expression,
    },
    Return {
        code: String,
        line: usize,
        expression: Option<Expression>,
    },
    If {
        code: String,
        line: usize,
        condition: Expression,
        #[serde(rename = "thenBody")]
        then_body: Vec<Statement>,
        #[serde(rename = "elseBody")]
        else_body: Vec<Statement>,
    },
    While {
        code: String,
        line: usize,
        condition: Expression,
        body: Vec<Statement>,
    },
    Expression {
        code: String,
        line: usize,
        expression: Expression,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Expression {
    Identifier {
        name: String,
        code: String,
        line: usize,
    },
    Literal {
        value: String,
        code: String,
        line: usize,
    },
    Binary {
        operator: String,
        code: String,
        line: usize,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    Call {
        name: String,
        code: String,
        line: usize,
        arguments: Vec<Expression>,
    },
    FieldAccess {
        field: String,
        code: String,
        line: usize,
        base: Box<Expression>,
    },
}

pub fn parse_file(path: &Path, options: &ParseOptions) -> Result<CxxAstDocument> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read C/C++ source '{}'", path.display()))?;
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to read metadata for '{}'", path.display()))?;

    Ok(CxxAstDocument {
        schema_version: SCHEMA_VERSION,
        backend: BACKEND_NAME,
        path: normalize_path(path),
        language: language_for_path(path),
        source_bytes: metadata.len(),
        source_lines: source.lines().count(),
        options: options.clone(),
        declarations: parse_declarations(&source, language_for_path(path))?,
    })
}

pub fn write_json(path: &Path, document: &CxxAstDocument) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory '{}'", parent.display()))?;
    }

    let json =
        serde_json::to_vec_pretty(document).context("failed to serialize cxxastgen document")?;
    fs::write(path, json).with_context(|| format!("failed to write '{}'", path.display()))
}

pub fn language_for_path(path: &Path) -> SourceLanguage {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "c" => SourceLanguage::C,
        "cc" | "cpp" | "cxx" | "c++" => SourceLanguage::Cpp,
        "h" | "hh" | "hpp" | "hxx" | "ipp" => SourceLanguage::Header,
        "i" | "ii" => SourceLanguage::Preprocessed,
        _ => SourceLanguage::Unknown,
    }
}

pub fn is_cxx_input(path: &Path) -> bool {
    !matches!(language_for_path(path), SourceLanguage::Unknown)
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn parse_declarations(source: &str, language: SourceLanguage) -> Result<Vec<Declaration>> {
    let mut parser = Parser::new();
    let language = match language {
        SourceLanguage::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        _ => tree_sitter_c::LANGUAGE.into(),
    };
    parser
        .set_language(&language)
        .context("failed to configure tree-sitter language")?;
    let tree = parser
        .parse(source, None)
        .context("tree-sitter returned no parse tree")?;

    let bytes = source.as_bytes();
    let mut declarations = Vec::new();
    for child in named_children(tree.root_node()) {
        match child.kind() {
            "preproc_def" | "preproc_function_def" => {
                if let Some(declaration) = parse_macro(child, bytes) {
                    declarations.push(Declaration::Macro(declaration));
                }
            }
            "declaration" => {
                declarations.extend(parse_type_declarations(child, bytes));
            }
            "struct_specifier" => {
                if let Some(declaration) = parse_struct(child, bytes) {
                    declarations.push(Declaration::Struct(declaration));
                }
            }
            "enum_specifier" => {
                if let Some(declaration) = parse_enum(child, bytes) {
                    declarations.push(Declaration::Enum(declaration));
                }
            }
            "function_definition" => {
                if let Some(function) = parse_function(child, bytes) {
                    declarations.push(Declaration::Function(function));
                }
            }
            _ => {}
        }
    }
    declarations.sort_by_key(declaration_line);
    Ok(declarations)
}

fn declaration_line(declaration: &Declaration) -> usize {
    match declaration {
        Declaration::Macro(value) => value.line,
        Declaration::Struct(value) => value.line,
        Declaration::Enum(value) => value.line,
        Declaration::Function(value) => value.line,
    }
}

fn parse_macro(node: Node, source: &[u8]) -> Option<MacroDecl> {
    let code = node_text(node, source).trim().to_string();
    let definition = code
        .strip_prefix('#')
        .unwrap_or(&code)
        .trim_start()
        .strip_prefix("define")
        .unwrap_or(&code)
        .trim();
    let name_end = definition
        .find(|ch: char| ch == '(' || ch.is_whitespace())
        .unwrap_or(definition.len());
    let name = definition[..name_end].trim().to_string();
    if name.is_empty() {
        return None;
    }
    let rest = definition[name_end..].trim_start();
    let (parameters, body) = if rest.starts_with('(') {
        let close = rest.find(')').unwrap_or(0);
        let params = rest
            .get(1..close)
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        (
            params,
            rest.get(close + 1..).unwrap_or_default().trim().to_string(),
        )
    } else {
        (Vec::new(), rest.to_string())
    };
    Some(MacroDecl {
        name,
        code,
        line: line(node),
        parameters,
        body,
    })
}

fn parse_type_declarations(node: Node, source: &[u8]) -> Vec<Declaration> {
    descendants(node)
        .into_iter()
        .filter_map(|child| match child.kind() {
            "struct_specifier" if child.child_by_field_name("body").is_some() => {
                parse_struct(child, source).map(Declaration::Struct)
            }
            "enum_specifier" if child.child_by_field_name("body").is_some() => {
                parse_enum(child, source).map(Declaration::Enum)
            }
            _ => None,
        })
        .collect()
}

fn parse_struct(node: Node, source: &[u8]) -> Option<StructDecl> {
    let name_node = node.child_by_field_name("name")?;
    let body = node.child_by_field_name("body")?;
    Some(StructDecl {
        name: node_text(name_node, source).to_string(),
        code: compact_code(node_text(node, source)),
        line: line(node),
        fields: named_children(body)
            .into_iter()
            .filter(|child| child.kind() == "field_declaration")
            .filter_map(|field| parse_field(field, source))
            .collect(),
    })
}

fn parse_field(node: Node, source: &[u8]) -> Option<FieldDecl> {
    let code = node_text(node, source).trim().trim_end_matches(';').trim();
    let (type_name, name) =
        declaration_type_and_name(node, source).or_else(|| split_type_and_name(code))?;
    Some(FieldDecl {
        name,
        type_name,
        code: code.to_string(),
    })
}

fn parse_enum(node: Node, source: &[u8]) -> Option<EnumDecl> {
    let name_node = node.child_by_field_name("name")?;
    let body = node.child_by_field_name("body")?;
    Some(EnumDecl {
        name: node_text(name_node, source).to_string(),
        code: compact_code(node_text(node, source)),
        line: line(node),
        variants: named_children(body)
            .into_iter()
            .filter(|child| child.kind() == "enumerator")
            .filter_map(|variant| parse_enum_variant(variant, source))
            .collect(),
    })
}

fn parse_enum_variant(node: Node, source: &[u8]) -> Option<EnumVariant> {
    let name_node = node
        .child_by_field_name("name")
        .or_else(|| named_children(node).into_iter().next())?;
    let value = node.child_by_field_name("value").map(|value| {
        node_text(value, source)
            .trim()
            .trim_start_matches('=')
            .trim()
            .to_string()
    });
    Some(EnumVariant {
        name: node_text(name_node, source).to_string(),
        value,
        code: node_text(node, source).trim().to_string(),
    })
}

fn parse_function(node: Node, source: &[u8]) -> Option<FunctionDecl> {
    let type_node = node.child_by_field_name("type")?;
    let declarator = node.child_by_field_name("declarator")?;
    let body = node.child_by_field_name("body")?;
    let name = declarator_name(declarator, source)?;
    let return_type = normalize_type(node_text(type_node, source));
    let parameters = declarator
        .child_by_field_name("parameters")
        .map(|params| parse_parameters(params, source))
        .unwrap_or_default();
    Some(FunctionDecl {
        name,
        signature: signature(&return_type, &parameters),
        return_type,
        code: compact_code(node_text(node, source)),
        line: line(node),
        parameters,
        body: parse_statement_block(body, source),
    })
}

fn parse_parameters(node: Node, source: &[u8]) -> Vec<ParameterDecl> {
    named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "parameter_declaration")
        .filter_map(|parameter| {
            let code = node_text(parameter, source).trim();
            if code == "void" {
                return None;
            }
            let (type_name, name) = declaration_type_and_name(parameter, source)
                .or_else(|| split_type_and_name(code))?;
            Some(ParameterDecl {
                name,
                type_name,
                code: code.to_string(),
                line: line(parameter),
            })
        })
        .collect()
}

fn parse_statement_block(node: Node, source: &[u8]) -> Vec<Statement> {
    named_children(node)
        .into_iter()
        .flat_map(|child| parse_statement(child, source))
        .collect()
}

fn parse_statement(node: Node, source: &[u8]) -> Vec<Statement> {
    match node.kind() {
        "compound_statement" => parse_statement_block(node, source),
        "declaration" => parse_local_declarations(node, source),
        "return_statement" => vec![Statement::Return {
            code: statement_code(node, source),
            line: line(node),
            expression: named_children(node)
                .into_iter()
                .next()
                .map(|expr| parse_expression(expr, source)),
        }],
        "expression_statement" => named_children(node)
            .into_iter()
            .next()
            .map(|expr| statement_from_expression(node, expr, source))
            .into_iter()
            .collect(),
        "if_statement" => parse_if_statement(node, source).into_iter().collect(),
        "else_clause" => named_children(node)
            .into_iter()
            .flat_map(|child| parse_statement(child, source))
            .collect(),
        "while_statement" => parse_while_statement(node, source).into_iter().collect(),
        _ => vec![Statement::Expression {
            code: statement_code(node, source),
            line: line(node),
            expression: parse_expression(node, source),
        }],
    }
}

fn parse_local_declarations(node: Node, source: &[u8]) -> Vec<Statement> {
    let type_name = node
        .child_by_field_name("type")
        .map(|type_node| normalize_type(node_text(type_node, source)));
    named_children(node)
        .into_iter()
        .filter(|child| child.kind() != "primitive_type" && child.kind() != "type_identifier")
        .filter_map(|declarator| {
            let name = declarator_name(declarator, source)?;
            let initializer = declarator
                .child_by_field_name("value")
                .map(|value| parse_expression(value, source));
            Some(Statement::LocalDecl {
                name,
                type_name: type_name.clone().unwrap_or_else(|| {
                    split_type_and_name(node_text(node, source))
                        .map(|(type_name, _)| type_name)
                        .unwrap_or_default()
                }),
                code: statement_code(node, source),
                line: line(node),
                initializer,
            })
        })
        .collect()
}

fn statement_from_expression(statement: Node, expr: Node, source: &[u8]) -> Statement {
    if expr.kind() == "assignment_expression" {
        if let (Some(left), Some(right)) = (
            expr.child_by_field_name("left"),
            expr.child_by_field_name("right"),
        ) {
            return Statement::Assignment {
                code: statement_code(statement, source),
                line: line(expr),
                left: parse_expression(left, source),
                right: parse_expression(right, source),
            };
        }
    }
    Statement::Expression {
        code: statement_code(statement, source),
        line: line(expr),
        expression: parse_expression(expr, source),
    }
}

fn parse_if_statement(node: Node, source: &[u8]) -> Option<Statement> {
    let condition = node.child_by_field_name("condition")?;
    let consequence = node.child_by_field_name("consequence")?;
    let else_body = node
        .child_by_field_name("alternative")
        .map(|alternative| parse_statement(alternative, source))
        .unwrap_or_default();
    Some(Statement::If {
        code: statement_code(node, source),
        line: line(node),
        condition: parse_expression(condition, source),
        then_body: parse_statement(consequence, source),
        else_body,
    })
}

fn parse_while_statement(node: Node, source: &[u8]) -> Option<Statement> {
    let condition = node.child_by_field_name("condition")?;
    let body = node.child_by_field_name("body")?;
    Some(Statement::While {
        code: statement_code(node, source),
        line: line(node),
        condition: parse_expression(condition, source),
        body: parse_statement(body, source),
    })
}

fn parse_expression(node: Node, source: &[u8]) -> Expression {
    match node.kind() {
        "parenthesized_expression" => named_children(node)
            .into_iter()
            .next()
            .map(|child| parse_expression(child, source))
            .unwrap_or_else(|| identifier_expression(node, source)),
        "identifier" => Expression::Identifier {
            name: node_text(node, source).to_string(),
            code: node_text(node, source).to_string(),
            line: line(node),
        },
        "number_literal" | "char_literal" | "string_literal" => Expression::Literal {
            value: node_text(node, source).to_string(),
            code: node_text(node, source).to_string(),
            line: line(node),
        },
        "binary_expression" => parse_binary_expression(node, source),
        "call_expression" => parse_call_expression(node, source),
        "field_expression" => parse_field_expression(node, source),
        "assignment_expression" => parse_binary_like_expression(node, source, "="),
        _ => identifier_expression(node, source),
    }
}

fn parse_binary_expression(node: Node, source: &[u8]) -> Expression {
    let operator = operator_text(node, source).unwrap_or("?");
    parse_binary_like_expression(node, source, operator)
}

fn parse_binary_like_expression(node: Node, source: &[u8], operator: &str) -> Expression {
    let left = node
        .child_by_field_name("left")
        .or_else(|| named_children(node).into_iter().next());
    let right = node
        .child_by_field_name("right")
        .or_else(|| named_children(node).into_iter().nth(1));
    match (left, right) {
        (Some(left), Some(right)) => Expression::Binary {
            operator: operator.to_string(),
            code: node_text(node, source).trim().to_string(),
            line: line(node),
            left: Box::new(parse_expression(left, source)),
            right: Box::new(parse_expression(right, source)),
        },
        _ => identifier_expression(node, source),
    }
}

fn parse_call_expression(node: Node, source: &[u8]) -> Expression {
    let function = node.child_by_field_name("function");
    let arguments = node
        .child_by_field_name("arguments")
        .map(|args| {
            named_children(args)
                .into_iter()
                .map(|arg| parse_expression(arg, source))
                .collect()
        })
        .unwrap_or_default();
    Expression::Call {
        name: function
            .map(|function| node_text(function, source).trim().to_string())
            .unwrap_or_else(|| node_text(node, source).trim().to_string()),
        code: node_text(node, source).trim().to_string(),
        line: line(node),
        arguments,
    }
}

fn parse_field_expression(node: Node, source: &[u8]) -> Expression {
    let base = node
        .child_by_field_name("argument")
        .or_else(|| named_children(node).into_iter().next());
    let field = node
        .child_by_field_name("field")
        .or_else(|| named_children(node).into_iter().last());
    match (base, field) {
        (Some(base), Some(field)) => Expression::FieldAccess {
            field: node_text(field, source).trim().to_string(),
            code: node_text(node, source).trim().to_string(),
            line: line(node),
            base: Box::new(parse_expression(base, source)),
        },
        _ => identifier_expression(node, source),
    }
}

fn identifier_expression(node: Node, source: &[u8]) -> Expression {
    Expression::Identifier {
        name: node_text(node, source).trim().to_string(),
        code: node_text(node, source).trim().to_string(),
        line: line(node),
    }
}

fn declaration_type_and_name(node: Node, source: &[u8]) -> Option<(String, String)> {
    let type_name = node
        .child_by_field_name("type")
        .map(|type_node| normalize_type(node_text(type_node, source)))?;
    let declarator = node.child_by_field_name("declarator")?;
    let name = declarator_name(declarator, source)?;
    Some((type_name, name))
}

fn declarator_name(node: Node, source: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" | "field_identifier" | "type_identifier" => {
            Some(node_text(node, source).trim().to_string())
        }
        _ => node
            .child_by_field_name("declarator")
            .and_then(|child| declarator_name(child, source))
            .or_else(|| {
                named_children(node)
                    .into_iter()
                    .find_map(|child| declarator_name(child, source))
            }),
    }
}

fn split_type_and_name(raw: &str) -> Option<(String, String)> {
    let normalized = raw
        .trim()
        .trim_end_matches(';')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let parts: Vec<&str> = normalized.rsplitn(2, ' ').collect();
    if parts.len() != 2 {
        return None;
    }
    let name = parts[0].trim().trim_start_matches('*');
    if name.is_empty() {
        return None;
    }
    Some((normalize_type(parts[1]), name.to_string()))
}

fn normalize_type(raw: &str) -> String {
    let normalized = raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" *", "*")
        .trim()
        .to_string();
    normalized
        .strip_prefix("struct ")
        .or_else(|| normalized.strip_prefix("enum "))
        .unwrap_or(&normalized)
        .to_string()
}

fn signature(return_type: &str, params: &[ParameterDecl]) -> String {
    format!(
        "{}({})",
        return_type,
        params
            .iter()
            .map(|param| param.type_name.as_str())
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn named_children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn descendants(node: Node) -> Vec<Node> {
    let mut result = Vec::new();
    for child in named_children(node) {
        result.push(child);
        result.extend(descendants(child));
    }
    result
}

fn operator_text<'a>(node: Node, source: &'a [u8]) -> Option<&'a str> {
    for index in 0..node.child_count() {
        let child = node.child(index as u32)?;
        if !child.is_named() {
            let text = node_text(child, source).trim();
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn node_text<'a>(node: Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or_default()
}

fn statement_code(node: Node, source: &[u8]) -> String {
    node_text(node, source)
        .trim()
        .trim_end_matches(';')
        .trim()
        .to_string()
}

fn line(node: Node) -> usize {
    node.start_position().row + 1
}

fn compact_code(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_c_and_cpp_extensions() {
        assert_eq!(language_for_path(Path::new("main.c")), SourceLanguage::C);
        assert_eq!(
            language_for_path(Path::new("main.cpp")),
            SourceLanguage::Cpp
        );
        assert_eq!(
            language_for_path(Path::new("main.hxx")),
            SourceLanguage::Header
        );
        assert_eq!(
            language_for_path(Path::new("main.i")),
            SourceLanguage::Preprocessed
        );
        assert_eq!(
            language_for_path(Path::new("README.md")),
            SourceLanguage::Unknown
        );
    }

    #[test]
    fn parses_simple_c_declarations_and_statements() {
        let sample = r#"
                #define INC(x) ((x) + 1)
                enum Mode { MODE_A = 1, MODE_B = 2 };
                struct Box { int value; };
                int add(int x, int y) {
                  int total = x + y;
                  return total;
                }
                "#;
        let doc = CxxAstDocument {
            schema_version: SCHEMA_VERSION,
            backend: BACKEND_NAME,
            path: "test.c".into(),
            language: SourceLanguage::C,
            source_bytes: 0,
            source_lines: 0,
            options: ParseOptions {
                include_paths: Vec::new(),
                defines: Vec::new(),
                compilation_database: None,
                skip_function_bodies: false,
            },
            declarations: parse_declarations(sample, SourceLanguage::C)
                .expect("sample C should parse"),
        };
        assert!(matches!(doc.declarations[0], Declaration::Macro(_)));
        assert!(matches!(doc.declarations[1], Declaration::Enum(_)));
        assert!(matches!(doc.declarations[2], Declaration::Struct(_)));
        let Declaration::Function(function) = &doc.declarations[3] else {
            panic!("expected function declaration");
        };
        assert_eq!(function.name, "add");
        assert_eq!(function.signature, "int(int,int)");
        assert_eq!(function.parameters.len(), 2);
        assert_eq!(function.body.len(), 2);
        assert_eq!(statement_line(&function.body[0]), 6);
        assert_eq!(statement_line(&function.body[1]), 7);
    }

    #[test]
    fn parses_control_flow_statements_from_tree_sitter() {
        let sample = r#"
                int clamp(int x) {
                  if (x < 0) {
                    return 0;
                  } else {
                    x = 1;
                  }
                  while (x > 10) {
                    x = x - 1;
                  }
                  return x;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::C)
            .expect("control-flow sample should parse");
        let Declaration::Function(function) = &declarations[0] else {
            panic!("expected function declaration");
        };
        assert_eq!(function.name, "clamp");
        assert_eq!(function.body.len(), 3);

        let Statement::If {
            condition,
            then_body,
            else_body,
            ..
        } = &function.body[0]
        else {
            panic!("expected if statement");
        };
        assert_binary_operator(condition, "<");
        assert!(matches!(then_body.as_slice(), [Statement::Return { .. }]));
        assert!(matches!(
            else_body.as_slice(),
            [Statement::Assignment { .. }]
        ));

        let Statement::While {
            condition, body, ..
        } = &function.body[1]
        else {
            panic!("expected while statement");
        };
        assert_binary_operator(condition, ">");
        let [Statement::Assignment { right, .. }] = body.as_slice() else {
            panic!("expected assignment in while body");
        };
        assert_binary_operator(right, "-");
    }

    fn statement_line(statement: &Statement) -> usize {
        match statement {
            Statement::LocalDecl { line, .. }
            | Statement::Assignment { line, .. }
            | Statement::Return { line, .. }
            | Statement::If { line, .. }
            | Statement::While { line, .. }
            | Statement::Expression { line, .. } => *line,
        }
    }

    fn assert_binary_operator(expression: &Expression, expected: &str) {
        let Expression::Binary { operator, .. } = expression else {
            panic!("expected binary expression");
        };
        assert_eq!(operator, expected);
    }
}
