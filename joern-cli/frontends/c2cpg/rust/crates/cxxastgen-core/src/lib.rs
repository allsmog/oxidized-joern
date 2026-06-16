use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashSet;
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
    GlobalVariable(GlobalVariableDecl),
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
pub struct GlobalVariableDecl {
    pub name: String,
    pub type_name: String,
    pub code: String,
    pub line: usize,
    pub initializer: Option<Expression>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionDecl {
    pub name: String,
    pub return_type: String,
    pub signature: String,
    pub is_definition: bool,
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
        operator: String,
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
    DoWhile {
        code: String,
        line: usize,
        condition: Expression,
        body: Vec<Statement>,
    },
    For {
        code: String,
        line: usize,
        initializer: Vec<Statement>,
        condition: Option<Expression>,
        update: Option<Expression>,
        body: Vec<Statement>,
    },
    Break {
        code: String,
        line: usize,
    },
    Continue {
        code: String,
        line: usize,
    },
    Goto {
        code: String,
        line: usize,
        label: String,
    },
    Label {
        code: String,
        line: usize,
        label: String,
        body: Vec<Statement>,
    },
    Switch {
        code: String,
        line: usize,
        condition: Expression,
        body: Vec<Statement>,
    },
    Case {
        code: String,
        line: usize,
        value: Option<Expression>,
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
    Unary {
        operator: String,
        code: String,
        line: usize,
        prefix: bool,
        argument: Box<Expression>,
    },
    Conditional {
        code: String,
        line: usize,
        condition: Box<Expression>,
        consequence: Option<Box<Expression>>,
        alternative: Box<Expression>,
    },
    Cast {
        #[serde(rename = "typeName")]
        type_name: String,
        code: String,
        line: usize,
        value: Box<Expression>,
    },
    SizeOf {
        code: String,
        line: usize,
        value: Option<Box<Expression>>,
        #[serde(rename = "typeName")]
        type_name: Option<String>,
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
    IndexAccess {
        code: String,
        line: usize,
        base: Box<Expression>,
        index: Box<Expression>,
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
                if let Some(function) = parse_function_declaration(child, bytes) {
                    declarations.push(Declaration::Function(function));
                }
                declarations.extend(
                    parse_global_variable_declarations(child, bytes)
                        .into_iter()
                        .map(Declaration::GlobalVariable),
                );
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
    declarations = dedupe_function_declarations(declarations);
    declarations.sort_by_key(declaration_line);
    Ok(declarations)
}

fn dedupe_function_declarations(declarations: Vec<Declaration>) -> Vec<Declaration> {
    let definitions = declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::Function(function) if function.is_definition => {
                Some((function.name.clone(), function.signature.clone()))
            }
            _ => None,
        })
        .collect::<HashSet<_>>();

    let mut seen_prototypes = HashSet::new();
    declarations
        .into_iter()
        .filter(|declaration| match declaration {
            Declaration::Function(function) if !function.is_definition => {
                let key = (function.name.clone(), function.signature.clone());
                !definitions.contains(&key) && seen_prototypes.insert(key)
            }
            _ => true,
        })
        .collect()
}

fn declaration_line(declaration: &Declaration) -> usize {
    match declaration {
        Declaration::Macro(value) => value.line,
        Declaration::Struct(value) => value.line,
        Declaration::Enum(value) => value.line,
        Declaration::GlobalVariable(value) => value.line,
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

fn parse_global_variable_declarations(node: Node, source: &[u8]) -> Vec<GlobalVariableDecl> {
    let Some(type_node) = node.child_by_field_name("type") else {
        return Vec::new();
    };
    let base_type = normalize_type(node_text(type_node, source));
    named_children(node)
        .into_iter()
        .filter(|child| *child != type_node)
        .filter(|declarator| !is_function_prototype_declarator(*declarator))
        .filter_map(|declarator| {
            let name = declarator_name(declarator, source)?;
            Some(GlobalVariableDecl {
                name,
                type_name: type_from_declarator(&base_type, declarator, source),
                code: variable_declaration_code(node, declarator, source),
                line: line(declarator),
                initializer: declarator
                    .child_by_field_name("value")
                    .map(|value| parse_expression(value, source)),
            })
        })
        .collect()
}

fn parse_function(node: Node, source: &[u8]) -> Option<FunctionDecl> {
    let type_node = node.child_by_field_name("type")?;
    let declarator = node.child_by_field_name("declarator")?;
    let body = node.child_by_field_name("body")?;
    let name = declarator_name(declarator, source)?;
    let return_type = type_from_declarator(
        &normalize_type(node_text(type_node, source)),
        declarator,
        source,
    );
    let parameters = declarator
        .child_by_field_name("parameters")
        .map(|params| parse_parameters(params, source))
        .unwrap_or_default();
    Some(FunctionDecl {
        name,
        signature: signature(&return_type, &parameters),
        return_type,
        is_definition: true,
        code: compact_code(node_text(node, source)),
        line: line(node),
        parameters,
        body: parse_statement_block(body, source),
    })
}

fn parse_function_declaration(node: Node, source: &[u8]) -> Option<FunctionDecl> {
    let type_node = node.child_by_field_name("type")?;
    let declarator = node.child_by_field_name("declarator")?;
    if !is_function_prototype_declarator(declarator) {
        return None;
    }
    let name = declarator_name(declarator, source)?;
    let return_type = type_from_declarator(
        &normalize_type(node_text(type_node, source)),
        declarator,
        source,
    );
    let parameters = declarator
        .child_by_field_name("parameters")
        .map(|params| parse_parameters(params, source))
        .unwrap_or_default();
    Some(FunctionDecl {
        name,
        signature: signature(&return_type, &parameters),
        return_type,
        is_definition: false,
        code: statement_code(node, source),
        line: line(node),
        parameters,
        body: Vec::new(),
    })
}

fn is_function_prototype_declarator(node: Node) -> bool {
    if node.kind() != "function_declarator" {
        return false;
    }
    node.child_by_field_name("declarator")
        .is_some_and(|declarator| declarator.kind() != "parenthesized_declarator")
}

fn parse_parameters(node: Node, source: &[u8]) -> Vec<ParameterDecl> {
    named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "parameter_declaration")
        .enumerate()
        .filter_map(|(index, parameter)| {
            let code = node_text(parameter, source).trim();
            if code == "void" {
                return None;
            }
            let (type_name, name) = declaration_type_and_name(parameter, source)
                .or_else(|| split_type_and_name(code))
                .or_else(|| {
                    parameter_type_without_name(parameter, source)
                        .map(|type_name| (type_name, format!("param{}", index + 1)))
                })?;
            Some(ParameterDecl {
                name,
                type_name,
                code: code.to_string(),
                line: line(parameter),
            })
        })
        .collect()
}

fn parameter_type_without_name(node: Node, source: &[u8]) -> Option<String> {
    let base_type = node
        .child_by_field_name("type")
        .map(|type_node| normalize_type(node_text(type_node, source)))?;
    Some(
        node.child_by_field_name("declarator")
            .map(|declarator| type_from_declarator(&base_type, declarator, source))
            .unwrap_or_else(|| normalize_type(node_text(node, source))),
    )
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
        "do_statement" => parse_do_statement(node, source).into_iter().collect(),
        "for_statement" => parse_for_statement(node, source).into_iter().collect(),
        "switch_statement" => parse_switch_statement(node, source).into_iter().collect(),
        "case_statement" => parse_case_statement(node, source).into_iter().collect(),
        "labeled_statement" => parse_labeled_statement(node, source).into_iter().collect(),
        "goto_statement" => vec![Statement::Goto {
            code: statement_code(node, source),
            line: line(node),
            label: node
                .child_by_field_name("label")
                .map(|label| node_text(label, source).trim().to_string())
                .unwrap_or_default(),
        }],
        "break_statement" => vec![Statement::Break {
            code: statement_code(node, source),
            line: line(node),
        }],
        "continue_statement" => vec![Statement::Continue {
            code: statement_code(node, source),
            line: line(node),
        }],
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
                type_name: type_name
                    .as_deref()
                    .map(|base_type| type_from_declarator(base_type, declarator, source))
                    .unwrap_or_else(|| {
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
                operator: operator_text(expr, source).unwrap_or("=").to_string(),
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

fn parse_do_statement(node: Node, source: &[u8]) -> Option<Statement> {
    let condition = node.child_by_field_name("condition")?;
    let body = node.child_by_field_name("body")?;
    Some(Statement::DoWhile {
        code: statement_code(node, source),
        line: line(node),
        condition: parse_expression(condition, source),
        body: parse_statement(body, source),
    })
}

fn parse_for_statement(node: Node, source: &[u8]) -> Option<Statement> {
    let body = node.child_by_field_name("body")?;
    let initializer = node
        .child_by_field_name("initializer")
        .map(|initializer| parse_for_initializer(initializer, source))
        .unwrap_or_default();
    Some(Statement::For {
        code: statement_code(node, source),
        line: line(node),
        initializer,
        condition: node
            .child_by_field_name("condition")
            .map(|condition| parse_expression(condition, source)),
        update: node
            .child_by_field_name("update")
            .map(|update| parse_expression(update, source)),
        body: parse_statement(body, source),
    })
}

fn parse_for_initializer(node: Node, source: &[u8]) -> Vec<Statement> {
    match node.kind() {
        "declaration" => parse_local_declarations(node, source),
        "expression_statement" => named_children(node)
            .into_iter()
            .next()
            .map(|expr| statement_from_expression(node, expr, source))
            .into_iter()
            .collect(),
        _ => vec![statement_from_expression(node, node, source)],
    }
}

fn parse_switch_statement(node: Node, source: &[u8]) -> Option<Statement> {
    let condition = node.child_by_field_name("condition")?;
    let body = node.child_by_field_name("body")?;
    Some(Statement::Switch {
        code: statement_code(node, source),
        line: line(node),
        condition: parse_expression(condition, source),
        body: parse_statement(body, source),
    })
}

fn parse_case_statement(node: Node, source: &[u8]) -> Option<Statement> {
    let value = node
        .child_by_field_name("value")
        .map(|value| parse_expression(value, source));
    Some(Statement::Case {
        code: case_code(node, source),
        line: line(node),
        value,
        body: named_children(node)
            .into_iter()
            .filter(|child| child.kind() != "type_definition")
            .filter(|child| node.child_by_field_name("value") != Some(*child))
            .flat_map(|child| parse_statement(child, source))
            .collect(),
    })
}

fn parse_labeled_statement(node: Node, source: &[u8]) -> Option<Statement> {
    let label = node.child_by_field_name("label")?;
    Some(Statement::Label {
        code: case_code(node, source),
        line: line(node),
        label: node_text(label, source).trim().to_string(),
        body: named_children(node)
            .into_iter()
            .filter(|child| *child != label)
            .flat_map(|child| parse_statement(child, source))
            .collect(),
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
        "unary_expression" | "update_expression" | "pointer_expression" => {
            parse_unary_expression(node, source)
        }
        "conditional_expression" => parse_conditional_expression(node, source),
        "call_expression" => parse_call_expression(node, source),
        "field_expression" => parse_field_expression(node, source),
        "subscript_expression" => parse_subscript_expression(node, source),
        "assignment_expression" => parse_assignment_expression(node, source),
        "cast_expression" => parse_cast_expression(node, source),
        "sizeof_expression" => parse_sizeof_expression(node, source),
        _ => identifier_expression(node, source),
    }
}

fn parse_binary_expression(node: Node, source: &[u8]) -> Expression {
    let operator = operator_text(node, source).unwrap_or("?");
    parse_binary_like_expression(node, source, operator)
}

fn parse_assignment_expression(node: Node, source: &[u8]) -> Expression {
    let operator = operator_text(node, source).unwrap_or("=");
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

fn parse_unary_expression(node: Node, source: &[u8]) -> Expression {
    let operator = operator_text(node, source).unwrap_or("?");
    let argument = node
        .child_by_field_name("argument")
        .or_else(|| named_children(node).into_iter().next());
    match argument {
        Some(argument) => Expression::Unary {
            operator: operator.to_string(),
            code: node_text(node, source).trim().to_string(),
            line: line(node),
            prefix: unary_operator_is_prefix(node, source, operator),
            argument: Box::new(parse_expression(argument, source)),
        },
        None => identifier_expression(node, source),
    }
}

fn parse_conditional_expression(node: Node, source: &[u8]) -> Expression {
    let condition = node.child_by_field_name("condition");
    let alternative = node.child_by_field_name("alternative");
    match (condition, alternative) {
        (Some(condition), Some(alternative)) => Expression::Conditional {
            code: node_text(node, source).trim().to_string(),
            line: line(node),
            condition: Box::new(parse_expression(condition, source)),
            consequence: node
                .child_by_field_name("consequence")
                .map(|consequence| Box::new(parse_expression(consequence, source))),
            alternative: Box::new(parse_expression(alternative, source)),
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

fn parse_subscript_expression(node: Node, source: &[u8]) -> Expression {
    let base = node
        .child_by_field_name("argument")
        .or_else(|| named_children(node).into_iter().next());
    let index = node
        .child_by_field_name("index")
        .or_else(|| named_children(node).into_iter().nth(1));
    match (base, index) {
        (Some(base), Some(index)) => Expression::IndexAccess {
            code: node_text(node, source).trim().to_string(),
            line: line(node),
            base: Box::new(parse_expression(base, source)),
            index: Box::new(parse_expression(index, source)),
        },
        _ => identifier_expression(node, source),
    }
}

fn parse_cast_expression(node: Node, source: &[u8]) -> Expression {
    let value = node.child_by_field_name("value");
    match value {
        Some(value) => Expression::Cast {
            type_name: node
                .child_by_field_name("type")
                .map(|type_node| normalize_type(node_text(type_node, source)))
                .unwrap_or_else(|| "ANY".to_string()),
            code: node_text(node, source).trim().to_string(),
            line: line(node),
            value: Box::new(parse_expression(value, source)),
        },
        None => identifier_expression(node, source),
    }
}

fn parse_sizeof_expression(node: Node, source: &[u8]) -> Expression {
    Expression::SizeOf {
        code: node_text(node, source).trim().to_string(),
        line: line(node),
        value: node
            .child_by_field_name("value")
            .map(|value| Box::new(parse_expression(value, source))),
        type_name: node
            .child_by_field_name("type")
            .map(|type_node| normalize_type(node_text(type_node, source))),
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
    let base_type = node
        .child_by_field_name("type")
        .map(|type_node| normalize_type(node_text(type_node, source)))?;
    let declarator = node.child_by_field_name("declarator")?;
    let name = declarator_name(declarator, source)?;
    let type_name = type_from_declarator(&base_type, declarator, source);
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

fn type_from_declarator(base_type: &str, declarator: Node, source: &[u8]) -> String {
    match declarator.kind() {
        "pointer_declarator" | "abstract_pointer_declarator" => declarator
            .child_by_field_name("declarator")
            .map(|child| format!("{}*", type_from_declarator(base_type, child, source)))
            .unwrap_or_else(|| format!("{base_type}*")),
        "array_declarator" | "abstract_array_declarator" => declarator
            .child_by_field_name("declarator")
            .map(|child| format!("{}[]", type_from_declarator(base_type, child, source)))
            .unwrap_or_else(|| format!("{base_type}[]")),
        "init_declarator" | "parenthesized_declarator" | "function_declarator" => declarator
            .child_by_field_name("declarator")
            .map(|child| type_from_declarator(base_type, child, source))
            .unwrap_or_else(|| base_type.to_string()),
        _ => base_type.to_string(),
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

fn unary_operator_is_prefix(node: Node, source: &[u8], operator: &str) -> bool {
    for index in 0..node.child_count() {
        if let Some(child) = node.child(index as u32) {
            let text = node_text(child, source).trim();
            if text.is_empty() {
                continue;
            }
            return !child.is_named() && text == operator;
        }
    }
    false
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

fn variable_declaration_code(declaration: Node, declarator: Node, source: &[u8]) -> String {
    let declarator_code = node_text(declarator, source)
        .split('=')
        .next()
        .unwrap_or_default()
        .trim();
    let prefix = declaration
        .child_by_field_name("type")
        .map(|type_node| {
            let declaration_text = node_text(declaration, source);
            declaration_text[..type_node.end_byte() - declaration.start_byte()]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    format!("{prefix} {declarator_code}")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn case_code(node: Node, source: &[u8]) -> String {
    let code = node_text(node, source).trim();
    code.find(':')
        .map(|index| code[..=index].trim().to_string())
        .unwrap_or_else(|| statement_code(node, source))
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

    #[test]
    fn parses_function_prototypes_and_deduplicates_forward_declarations() {
        let sample = r#"
                int external(int value);
                int external(int value);
                int unnamed(int, char *);
                int defined(int value);
                int defined(int value) {
                  return external(value);
                }
                "#;
        let declarations =
            parse_declarations(sample, SourceLanguage::C).expect("prototype sample should parse");
        let functions = declarations
            .iter()
            .map(|declaration| match declaration {
                Declaration::Function(function) => function,
                _ => panic!("expected only function declarations"),
            })
            .collect::<Vec<_>>();

        assert_eq!(functions.len(), 3);
        assert_eq!(functions[0].name, "external");
        assert!(!functions[0].is_definition);
        assert_eq!(functions[0].signature, "int(int)");
        assert!(functions[0].body.is_empty());
        assert_eq!(functions[1].name, "unnamed");
        assert!(!functions[1].is_definition);
        assert_eq!(functions[1].signature, "int(int,char*)");
        assert_eq!(functions[1].parameters[0].name, "param1");
        assert_eq!(functions[1].parameters[1].name, "param2");
        assert_eq!(functions[1].parameters[1].type_name, "char*");
        assert_eq!(functions[2].name, "defined");
        assert!(functions[2].is_definition);
        assert_eq!(functions[2].signature, "int(int)");
        assert_eq!(functions[2].body.len(), 1);
    }

    #[test]
    fn parses_global_variable_declarations() {
        let sample = r#"
                int global = 1;
                static int *ptr;
                int read() {
                  return global;
                }
                "#;
        let declarations =
            parse_declarations(sample, SourceLanguage::C).expect("global sample should parse");

        let Declaration::GlobalVariable(global) = &declarations[0] else {
            panic!("expected global variable");
        };
        assert_eq!(global.name, "global");
        assert_eq!(global.type_name, "int");
        assert_eq!(global.code, "int global");
        assert!(matches!(
            global.initializer.as_ref(),
            Some(Expression::Literal { value, .. }) if value == "1"
        ));

        let Declaration::GlobalVariable(ptr) = &declarations[1] else {
            panic!("expected pointer global variable");
        };
        assert_eq!(ptr.name, "ptr");
        assert_eq!(ptr.type_name, "int*");
        assert_eq!(ptr.code, "static int *ptr");
        assert!(ptr.initializer.is_none());

        let Declaration::Function(function) = &declarations[2] else {
            panic!("expected function declaration");
        };
        assert_eq!(function.name, "read");
    }

    #[test]
    fn parses_for_loop_jump_and_unary_index_expressions() {
        let sample = r#"
                int sum(int *xs, int n) {
                  int total = 0;
                  for (int i = 0; i < n; i++) {
                    if (!xs[i]) {
                      continue;
                    }
                    total = total + xs[i];
                  }
                  return total;
                }
                "#;
        let declarations =
            parse_declarations(sample, SourceLanguage::C).expect("for-loop sample should parse");
        let Declaration::Function(function) = &declarations[0] else {
            panic!("expected function declaration");
        };
        assert_eq!(function.name, "sum");
        assert_eq!(function.body.len(), 3);

        let Statement::For {
            initializer,
            condition,
            update,
            body,
            ..
        } = &function.body[1]
        else {
            panic!("expected for statement");
        };
        assert!(matches!(
            initializer.as_slice(),
            [Statement::LocalDecl { .. }]
        ));
        assert_binary_operator(condition.as_ref().expect("for condition"), "<");

        let Expression::Unary {
            operator,
            prefix,
            argument,
            ..
        } = update.as_ref().expect("for update")
        else {
            panic!("expected unary update expression");
        };
        assert_eq!(operator, "++");
        assert!(!prefix);
        assert!(matches!(argument.as_ref(), Expression::Identifier { name, .. } if name == "i"));

        let [Statement::If {
            condition,
            then_body,
            ..
        }, Statement::Assignment { right, .. }] = body.as_slice()
        else {
            panic!("expected if and assignment in for body");
        };
        let Expression::Unary {
            operator, argument, ..
        } = condition
        else {
            panic!("expected unary if condition");
        };
        assert_eq!(operator, "!");
        assert!(matches!(argument.as_ref(), Expression::IndexAccess { .. }));
        assert!(matches!(then_body.as_slice(), [Statement::Continue { .. }]));

        let Expression::Binary { right, .. } = right else {
            panic!("expected binary assignment rhs");
        };
        assert!(matches!(right.as_ref(), Expression::IndexAccess { .. }));
    }

    #[test]
    fn parses_switch_do_goto_and_label_statements() {
        let sample = r#"
                int route(int x) {
                retry:
                  do {
                    x = x - 1;
                  } while (x > 3);
                  switch (x) {
                    case 1:
                      goto retry;
                    default:
                      break;
                  }
                  return x;
                }
                "#;
        let declarations =
            parse_declarations(sample, SourceLanguage::C).expect("switch/goto sample should parse");
        let Declaration::Function(function) = &declarations[0] else {
            panic!("expected function declaration");
        };
        let [Statement::Label { label, body, .. }, Statement::Switch {
            condition,
            body: switch_body,
            ..
        }, Statement::Return { .. }] = function.body.as_slice()
        else {
            panic!("expected label, switch, return");
        };
        assert_eq!(label, "retry");
        assert!(matches!(body.as_slice(), [Statement::DoWhile { .. }]));
        assert!(matches!(condition, Expression::Identifier { name, .. } if name == "x"));

        let [Statement::Case {
            value: Some(Expression::Literal { value, .. }),
            body: first_case_body,
            ..
        }, Statement::Case {
            value: None,
            body: default_body,
            ..
        }] = switch_body.as_slice()
        else {
            panic!("expected case and default");
        };
        assert_eq!(value, "1");
        assert!(matches!(
            first_case_body.as_slice(),
            [Statement::Goto { label, .. }] if label == "retry"
        ));
        assert!(matches!(default_body.as_slice(), [Statement::Break { .. }]));
    }

    #[test]
    fn parses_cast_sizeof_conditional_and_compound_assignment_expressions() {
        let sample = r#"
                int score(int x) {
                  int y = (int)sizeof(x);
                  y += x > 0 ? x : -x;
                  return y;
                }
                "#;
        let declarations =
            parse_declarations(sample, SourceLanguage::C).expect("expression sample should parse");
        let Declaration::Function(function) = &declarations[0] else {
            panic!("expected function declaration");
        };

        let [Statement::LocalDecl {
            initializer: Some(initializer),
            ..
        }, Statement::Assignment {
            operator, right, ..
        }, Statement::Return { .. }] = function.body.as_slice()
        else {
            panic!("expected local, compound assignment, return");
        };
        assert_eq!(operator, "+=");

        let Expression::Cast { value, .. } = initializer else {
            panic!("expected cast initializer");
        };
        assert!(matches!(value.as_ref(), Expression::SizeOf { .. }));

        let Expression::Conditional {
            condition,
            consequence,
            alternative,
            ..
        } = right
        else {
            panic!("expected conditional expression");
        };
        assert_binary_operator(condition, ">");
        assert!(matches!(
            consequence.as_deref(),
            Some(Expression::Identifier { name, .. }) if name == "x"
        ));
        assert!(matches!(
            alternative.as_ref(),
            Expression::Unary { operator, .. } if operator == "-"
        ));
    }

    #[test]
    fn preserves_pointer_and_array_type_suffixes_from_declarators() {
        let sample = r#"
                struct Holder {
                  int *next;
                  int values[4];
                };
                int first(int *xs) {
                  int local[4];
                  int *p = xs;
                  return p[0];
                }
                "#;
        let declarations =
            parse_declarations(sample, SourceLanguage::C).expect("type suffix sample should parse");

        let Declaration::Struct(holder) = &declarations[0] else {
            panic!("expected struct declaration");
        };
        assert_eq!(holder.fields[0].type_name, "int*");
        assert_eq!(holder.fields[1].type_name, "int[]");

        let Declaration::Function(function) = &declarations[1] else {
            panic!("expected function declaration");
        };
        assert_eq!(function.parameters[0].type_name, "int*");
        assert_eq!(function.signature, "int(int*)");
        let [Statement::LocalDecl {
            name: local_name,
            type_name: local_type,
            ..
        }, Statement::LocalDecl {
            name: pointer_name,
            type_name: pointer_type,
            ..
        }, Statement::Return { .. }] = function.body.as_slice()
        else {
            panic!("expected array local, pointer local, return");
        };
        assert_eq!(local_name, "local");
        assert_eq!(local_type, "int[]");
        assert_eq!(pointer_name, "p");
        assert_eq!(pointer_type, "int*");
    }

    fn statement_line(statement: &Statement) -> usize {
        match statement {
            Statement::LocalDecl { line, .. }
            | Statement::Assignment { line, .. }
            | Statement::Return { line, .. }
            | Statement::If { line, .. }
            | Statement::While { line, .. }
            | Statement::DoWhile { line, .. }
            | Statement::For { line, .. }
            | Statement::Break { line, .. }
            | Statement::Continue { line, .. }
            | Statement::Goto { line, .. }
            | Statement::Label { line, .. }
            | Statement::Switch { line, .. }
            | Statement::Case { line, .. }
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
