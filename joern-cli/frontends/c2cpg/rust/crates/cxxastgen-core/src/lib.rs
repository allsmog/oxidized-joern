use anyhow::{Context, Result};
use regex::Regex;
use serde::Serialize;
use std::fs;
use std::path::Path;

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
        declarations: parse_declarations(&source),
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

fn parse_declarations(source: &str) -> Vec<Declaration> {
    let mut declarations = Vec::new();
    declarations.extend(parse_macros(source).into_iter().map(Declaration::Macro));
    declarations.extend(parse_structs(source).into_iter().map(Declaration::Struct));
    declarations.extend(parse_enums(source).into_iter().map(Declaration::Enum));
    declarations.extend(
        parse_functions(source)
            .into_iter()
            .map(Declaration::Function),
    );
    declarations.sort_by_key(declaration_line);
    declarations
}

fn declaration_line(declaration: &Declaration) -> usize {
    match declaration {
        Declaration::Macro(value) => value.line,
        Declaration::Struct(value) => value.line,
        Declaration::Enum(value) => value.line,
        Declaration::Function(value) => value.line,
    }
}

fn parse_macros(source: &str) -> Vec<MacroDecl> {
    let macro_re =
        Regex::new(r#"^\s*#\s*define\s+([A-Za-z_][A-Za-z0-9_]*)(?:\(([^)]*)\))?\s*(.*)$"#).unwrap();
    source
        .lines()
        .enumerate()
        .filter_map(|(line_index, line)| {
            let captures = macro_re.captures(line)?;
            let params = captures
                .get(2)
                .map(|value| {
                    value
                        .as_str()
                        .split(',')
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToOwned::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            Some(MacroDecl {
                name: captures.get(1).unwrap().as_str().to_string(),
                code: line.trim().to_string(),
                line: line_index + 1,
                parameters: params,
                body: captures
                    .get(3)
                    .map(|value| value.as_str().trim().to_string())
                    .unwrap_or_default(),
            })
        })
        .collect()
}

fn parse_structs(source: &str) -> Vec<StructDecl> {
    let struct_re =
        Regex::new(r#"(?s)\bstruct\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{(.*?)\}\s*;"#).unwrap();
    struct_re
        .captures_iter(source)
        .map(|captures| {
            let whole = captures.get(0).unwrap();
            StructDecl {
                name: captures.get(1).unwrap().as_str().to_string(),
                code: compact_code(whole.as_str()),
                line: line_number(source, whole.start()),
                fields: parse_fields(captures.get(2).unwrap().as_str()),
            }
        })
        .collect()
}

fn parse_fields(body: &str) -> Vec<FieldDecl> {
    body.split(';')
        .filter_map(|raw| {
            let code = raw.trim();
            if code.is_empty() {
                return None;
            }
            let (type_name, name) = split_type_and_name(code)?;
            Some(FieldDecl {
                name,
                type_name,
                code: code.to_string(),
            })
        })
        .collect()
}

fn parse_enums(source: &str) -> Vec<EnumDecl> {
    let enum_re = Regex::new(r#"(?s)\benum\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{(.*?)\}\s*;"#).unwrap();
    enum_re
        .captures_iter(source)
        .map(|captures| {
            let whole = captures.get(0).unwrap();
            EnumDecl {
                name: captures.get(1).unwrap().as_str().to_string(),
                code: compact_code(whole.as_str()),
                line: line_number(source, whole.start()),
                variants: parse_enum_variants(captures.get(2).unwrap().as_str()),
            }
        })
        .collect()
}

fn parse_enum_variants(body: &str) -> Vec<EnumVariant> {
    body.split(',')
        .filter_map(|raw| {
            let code = raw.trim();
            if code.is_empty() {
                return None;
            }
            let mut parts = code.splitn(2, '=').map(str::trim);
            Some(EnumVariant {
                name: parts.next().unwrap_or_default().to_string(),
                value: parts.next().map(ToOwned::to_owned),
                code: code.to_string(),
            })
        })
        .collect()
}

fn parse_functions(source: &str) -> Vec<FunctionDecl> {
    let function_re = Regex::new(
        r#"(?s)([A-Za-z_][A-Za-z0-9_\s\*]*?)\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)\s*\{"#,
    )
    .unwrap();
    let mut functions = Vec::new();
    for captures in function_re.captures_iter(source) {
        let whole = captures.get(0).unwrap();
        let open_brace = whole.end() - 1;
        let Some(close_brace) = find_matching_brace(source, open_brace) else {
            continue;
        };

        let start = captures.get(1).unwrap().start();
        if is_inside_type_declaration(source, start) {
            continue;
        }

        let return_type = normalize_type(captures.get(1).unwrap().as_str());
        let name = captures.get(2).unwrap().as_str().to_string();
        let params = parse_parameters(
            captures.get(3).unwrap().as_str(),
            line_number(source, whole.start()),
        );
        let body_source = &source[open_brace + 1..close_brace];
        let line = line_number(source, whole.start());
        functions.push(FunctionDecl {
            name,
            signature: signature(&return_type, &params),
            return_type,
            code: compact_code(&source[start..=close_brace]),
            line,
            parameters: params,
            body: parse_statements(body_source, line + 1),
        });
    }
    functions
}

fn is_inside_type_declaration(source: &str, offset: usize) -> bool {
    let before = &source[..offset];
    let last_semicolon = before.rfind(';').unwrap_or(0);
    let last_open_brace = before.rfind('{').unwrap_or(0);
    let scope = &before[last_semicolon.max(last_open_brace)..];
    scope.contains("struct ") || scope.contains("enum ")
}

fn parse_parameters(raw: &str, line: usize) -> Vec<ParameterDecl> {
    raw.split(',')
        .filter_map(|param| {
            let code = param.trim();
            if code.is_empty() || code == "void" {
                return None;
            }
            let (type_name, name) = split_type_and_name(code)?;
            Some(ParameterDecl {
                name,
                type_name,
                code: code.to_string(),
                line,
            })
        })
        .collect()
}

fn parse_statements(body: &str, base_line: usize) -> Vec<Statement> {
    let mut statements = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    for (index, ch) in body.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ';' if depth == 0 => {
                let raw = &body[start..index];
                let line = base_line + body[..start].bytes().filter(|byte| *byte == b'\n').count();
                if let Some(statement) = parse_statement(raw, line) {
                    statements.push(statement);
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    statements
}

fn parse_statement(raw: &str, line: usize) -> Option<Statement> {
    let code = raw.trim();
    if code.is_empty() {
        return None;
    }
    if let Some(rest) = code.strip_prefix("return") {
        let expr = rest.trim();
        return Some(Statement::Return {
            code: code.to_string(),
            line,
            expression: (!expr.is_empty()).then(|| parse_expression(expr, line)),
        });
    }
    if let Some((left, right)) = split_top_level_once(code, '=') {
        if let Some((type_name, name)) = split_type_and_name(left.trim()) {
            return Some(Statement::LocalDecl {
                name,
                type_name,
                code: code.to_string(),
                line,
                initializer: Some(parse_expression(right.trim(), line)),
            });
        }
        return Some(Statement::Assignment {
            code: code.to_string(),
            line,
            left: parse_expression(left.trim(), line),
            right: parse_expression(right.trim(), line),
        });
    }
    if let Some((type_name, name)) = split_type_and_name(code) {
        return Some(Statement::LocalDecl {
            name,
            type_name,
            code: code.to_string(),
            line,
            initializer: None,
        });
    }
    Some(Statement::Expression {
        code: code.to_string(),
        line,
        expression: parse_expression(code, line),
    })
}

fn parse_expression(raw: &str, line: usize) -> Expression {
    let code = strip_balanced_parens(raw.trim());
    if let Some((left, right)) = split_top_level_once(code, '+') {
        return Expression::Binary {
            operator: "+".into(),
            code: code.to_string(),
            line,
            left: Box::new(parse_expression(left.trim(), line)),
            right: Box::new(parse_expression(right.trim(), line)),
        };
    }
    if let Some((base, field)) = split_top_level_once(code, '.') {
        return Expression::FieldAccess {
            field: field.trim().to_string(),
            code: code.to_string(),
            line,
            base: Box::new(parse_expression(base.trim(), line)),
        };
    }
    if let Some((name, args)) = parse_call_expression(code) {
        return Expression::Call {
            name: name.to_string(),
            code: code.to_string(),
            line,
            arguments: split_arguments(args)
                .into_iter()
                .map(|arg| parse_expression(arg, line))
                .collect(),
        };
    }
    if code.chars().all(|ch| ch.is_ascii_digit()) {
        return Expression::Literal {
            value: code.to_string(),
            code: code.to_string(),
            line,
        };
    }
    Expression::Identifier {
        name: code.to_string(),
        code: code.to_string(),
        line,
    }
}

fn parse_call_expression(code: &str) -> Option<(&str, &str)> {
    let open = code.find('(')?;
    if !code.ends_with(')') {
        return None;
    }
    let name = code[..open].trim();
    if !is_identifier(name) {
        return None;
    }
    if find_matching_brace_like(code, open, '(', ')')? != code.len() - 1 {
        return None;
    }
    Some((name, &code[open + 1..code.len() - 1]))
}

fn split_arguments(raw: &str) -> Vec<&str> {
    let mut args = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    for (index, ch) in raw.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                let arg = raw[start..index].trim();
                if !arg.is_empty() {
                    args.push(arg);
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    let tail = raw[start..].trim();
    if !tail.is_empty() {
        args.push(tail);
    }
    args
}

fn split_top_level_once(raw: &str, needle: char) -> Option<(&str, &str)> {
    let mut depth = 0i32;
    for (index, ch) in raw.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ch if ch == needle && depth == 0 => {
                return Some((&raw[..index], &raw[index + ch.len_utf8()..]))
            }
            _ => {}
        }
    }
    None
}

fn split_type_and_name(raw: &str) -> Option<(String, String)> {
    let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let parts: Vec<&str> = normalized.rsplitn(2, ' ').collect();
    if parts.len() != 2 {
        return None;
    }
    let name = parts[0].trim().trim_start_matches('*');
    if !is_identifier(name) {
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

fn strip_balanced_parens(mut value: &str) -> &str {
    loop {
        let trimmed = value.trim();
        if trimmed.starts_with('(')
            && trimmed.ends_with(')')
            && find_matching_brace_like(trimmed, 0, '(', ')') == Some(trimmed.len() - 1)
        {
            value = &trimmed[1..trimmed.len() - 1];
        } else {
            return trimmed;
        }
    }
}

fn find_matching_brace(source: &str, open: usize) -> Option<usize> {
    find_matching_brace_like(source, open, '{', '}')
}

fn find_matching_brace_like(
    source: &str,
    open: usize,
    open_char: char,
    close_char: char,
) -> Option<usize> {
    let mut depth = 0i32;
    for (index, ch) in source[open..].char_indices() {
        if ch == open_char {
            depth += 1;
        } else if ch == close_char {
            depth -= 1;
            if depth == 0 {
                return Some(open + index);
            }
        }
    }
    None
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn line_number(source: &str, offset: usize) -> usize {
    source[..offset].chars().filter(|ch| *ch == '\n').count() + 1
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
            declarations: parse_declarations(
                r#"
                #define INC(x) ((x) + 1)
                enum Mode { MODE_A = 1, MODE_B = 2 };
                struct Box { int value; };
                int add(int x, int y) {
                  int total = x + y;
                  return total;
                }
                "#,
            ),
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

    fn statement_line(statement: &Statement) -> usize {
        match statement {
            Statement::LocalDecl { line, .. }
            | Statement::Assignment { line, .. }
            | Statement::Return { line, .. }
            | Statement::Expression { line, .. } => *line,
        }
    }
}
