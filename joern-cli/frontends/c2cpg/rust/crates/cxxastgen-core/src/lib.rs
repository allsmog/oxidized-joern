use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
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
    pub import_header_declarations: bool,
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
    MacroUndef(MacroUndefDecl),
    Include(IncludeDecl),
    Namespace(NamespaceDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    Typedef(TypedefDecl),
    GlobalVariable(GlobalVariableDecl),
    Function(FunctionDecl),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroDecl {
    pub name: String,
    pub code: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(rename = "visibleLine", skip_serializing_if = "Option::is_none")]
    pub visible_line: Option<usize>,
    pub parameters: Vec<String>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroUndefDecl {
    pub name: String,
    pub code: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(rename = "visibleLine", skip_serializing_if = "Option::is_none")]
    pub visible_line: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncludeDecl {
    pub name: String,
    pub code: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(rename = "visibleLine", skip_serializing_if = "Option::is_none")]
    pub visible_line: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamespaceDecl {
    pub name: String,
    pub code: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(rename = "visibleLine", skip_serializing_if = "Option::is_none")]
    pub visible_line: Option<usize>,
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructDecl {
    pub name: String,
    pub code: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(rename = "visibleLine", skip_serializing_if = "Option::is_none")]
    pub visible_line: Option<usize>,
    pub base_classes: Vec<String>,
    pub fields: Vec<FieldDecl>,
    #[serde(rename = "nestedDeclarations")]
    pub nested_declarations: Vec<Declaration>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldDecl {
    pub name: String,
    pub type_name: String,
    pub code: String,
    pub is_static: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumDecl {
    pub name: String,
    pub code: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(rename = "visibleLine", skip_serializing_if = "Option::is_none")]
    pub visible_line: Option<usize>,
    pub variants: Vec<EnumVariant>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumVariant {
    pub name: String,
    pub value: Option<String>,
    pub code: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalVariableDecl {
    pub name: String,
    pub type_name: String,
    pub code: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(rename = "visibleLine", skip_serializing_if = "Option::is_none")]
    pub visible_line: Option<usize>,
    pub initializer: Option<Expression>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedefDecl {
    pub name: String,
    pub type_name: String,
    pub code: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(rename = "visibleLine", skip_serializing_if = "Option::is_none")]
    pub visible_line: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionDecl {
    pub name: String,
    pub return_type: String,
    pub signature: String,
    pub is_definition: bool,
    pub is_static: bool,
    pub is_const: bool,
    pub is_virtual: bool,
    pub code: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(rename = "visibleLine", skip_serializing_if = "Option::is_none")]
    pub visible_line: Option<usize>,
    pub parameters: Vec<ParameterDecl>,
    pub constructor_initializers: Vec<ConstructorInitializer>,
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
#[serde(rename_all = "camelCase")]
pub struct ConstructorInitializer {
    pub field: String,
    pub code: String,
    pub line: usize,
    pub arguments: Vec<Expression>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatchClause {
    pub code: String,
    pub line: usize,
    pub parameter: Option<ParameterDecl>,
    pub body: Vec<Statement>,
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
    Throw {
        code: String,
        line: usize,
        expression: Option<Expression>,
    },
    Try {
        code: String,
        line: usize,
        body: Vec<Statement>,
        catches: Vec<CatchClause>,
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
#[serde(rename_all = "camelCase")]
pub struct LambdaCapture {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    code: String,
    #[serde(rename = "captureKind")]
    capture_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    initializer: Option<Box<Expression>>,
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
    New {
        #[serde(rename = "typeName")]
        type_name: String,
        code: String,
        line: usize,
        arguments: Vec<Expression>,
        #[serde(rename = "initializerArguments")]
        initializer_arguments: Vec<Expression>,
    },
    Delete {
        code: String,
        line: usize,
        argument: Box<Expression>,
    },
    Lambda {
        code: String,
        line: usize,
        captures: Vec<LambdaCapture>,
        #[serde(rename = "isMutable")]
        is_mutable: bool,
        parameters: Vec<ParameterDecl>,
        #[serde(rename = "returnType")]
        return_type: String,
        signature: String,
        body: Vec<Statement>,
    },
    Call {
        name: String,
        code: String,
        line: usize,
        callee: Box<Expression>,
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
    InitializerList {
        code: String,
        line: usize,
        elements: Vec<Expression>,
    },
    DesignatedInitializer {
        code: String,
        line: usize,
        designator: Box<Expression>,
        value: Box<Expression>,
    },
    Designator {
        name: String,
        code: String,
        line: usize,
    },
}

#[derive(Debug, Clone)]
struct MacroBinding {
    parameters: Vec<String>,
    body: String,
}

impl MacroBinding {
    fn from_decl(declaration: &MacroDecl) -> Self {
        Self {
            parameters: declaration.parameters.clone(),
            body: declaration.body.clone(),
        }
    }
}

type MacroSymbols = HashMap<String, MacroBinding>;

pub fn parse_file(path: &Path, options: &ParseOptions) -> Result<CxxAstDocument> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read C/C++ source '{}'", path.display()))?;
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to read metadata for '{}'", path.display()))?;

    let mut declarations = synthetic_macro_declarations(&options.defines);
    let mut symbols = MacroSymbols::new();
    register_macro_symbols(&declarations, &mut symbols);
    let mut visited_headers = HashSet::new();
    declarations.extend(parse_declarations_with_context(
        &source,
        language_for_path(path),
        Some(path),
        Some(options),
        &mut symbols,
        &mut visited_headers,
    )?);
    declarations = dedupe_function_declarations(declarations);
    declarations.sort_by_key(declaration_sort_line);

    Ok(CxxAstDocument {
        schema_version: SCHEMA_VERSION,
        backend: BACKEND_NAME,
        path: normalize_path(path),
        language: language_for_path(path),
        source_bytes: metadata.len(),
        source_lines: source.lines().count(),
        options: options.clone(),
        declarations,
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

#[cfg(test)]
fn parse_declarations(source: &str, language: SourceLanguage) -> Result<Vec<Declaration>> {
    let mut symbols = MacroSymbols::new();
    let mut visited_headers = HashSet::new();
    let mut declarations = parse_declarations_with_context(
        source,
        language,
        None,
        None,
        &mut symbols,
        &mut visited_headers,
    )?;
    declarations = dedupe_function_declarations(declarations);
    declarations.sort_by_key(declaration_line);
    Ok(declarations)
}

fn parse_declarations_with_context(
    source: &str,
    language: SourceLanguage,
    source_path: Option<&Path>,
    options: Option<&ParseOptions>,
    symbols: &mut MacroSymbols,
    visited_headers: &mut HashSet<PathBuf>,
) -> Result<Vec<Declaration>> {
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
    parse_declaration_children(
        tree.root_node(),
        bytes,
        source_path,
        options,
        symbols,
        visited_headers,
    )
}

fn parse_declaration_children(
    node: Node,
    source: &[u8],
    source_path: Option<&Path>,
    options: Option<&ParseOptions>,
    symbols: &mut MacroSymbols,
    visited_headers: &mut HashSet<PathBuf>,
) -> Result<Vec<Declaration>> {
    let mut declarations = Vec::new();
    for child in named_children(node) {
        declarations.extend(parse_declaration_node(
            child,
            source,
            source_path,
            options,
            symbols,
            visited_headers,
        )?);
    }
    Ok(declarations)
}

fn parse_declaration_node(
    node: Node,
    source: &[u8],
    source_path: Option<&Path>,
    options: Option<&ParseOptions>,
    symbols: &mut MacroSymbols,
    visited_headers: &mut HashSet<PathBuf>,
) -> Result<Vec<Declaration>> {
    let mut declarations = Vec::new();
    match node.kind() {
        "preproc_include" => {
            if let Some(declaration) = parse_include(node, source) {
                if let (Some(source_path), Some(options)) = (source_path, options) {
                    if let Some(path) = resolve_include(source_path, &declaration.name, options) {
                        declarations.extend(header_declarations(
                            &path,
                            declaration.line,
                            options,
                            symbols,
                            visited_headers,
                        )?);
                    }
                }
                declarations.push(Declaration::Include(declaration));
            }
        }
        "preproc_def" | "preproc_function_def" => {
            if let Some(declaration) = parse_macro(node, source) {
                define_macro_symbol(symbols, &declaration);
                declarations.push(Declaration::Macro(declaration));
            }
        }
        "preproc_call" => {
            if let Some(declaration) = parse_macro_undef(node, source) {
                symbols.remove(&declaration.name);
                declarations.push(Declaration::MacroUndef(declaration));
            }
        }
        "preproc_if" | "preproc_ifdef" | "preproc_elif" | "preproc_elifdef" | "preproc_else" => {
            declarations.extend(parse_preproc_declarations(
                node,
                source,
                source_path,
                options,
                symbols,
                visited_headers,
            )?);
        }
        "template_declaration" => {
            declarations.extend(parse_declaration_children(
                node,
                source,
                source_path,
                options,
                symbols,
                visited_headers,
            )?);
        }
        "namespace_definition" => {
            if let Some(namespace) =
                parse_namespace(node, source, source_path, options, symbols, visited_headers)?
            {
                declarations.push(Declaration::Namespace(namespace));
            }
        }
        "declaration" => {
            declarations.extend(parse_type_declarations(node, source, symbols));
            if let Some(function) = parse_function_declaration(node, source) {
                declarations.push(Declaration::Function(function));
            }
            declarations.extend(
                parse_global_variable_declarations(node, source)
                    .into_iter()
                    .map(Declaration::GlobalVariable),
            );
        }
        "alias_declaration" => {
            if let Some(typedef) = parse_alias_declaration(node, source) {
                declarations.push(Declaration::Typedef(typedef));
            }
        }
        "type_definition" => {
            declarations.extend(
                parse_typedef_declarations(node, source)
                    .into_iter()
                    .map(Declaration::Typedef),
            );
            declarations.extend(parse_anonymous_typedef_aggregate_declarations(
                node, source, symbols,
            ));
            declarations.extend(parse_type_declarations(node, source, symbols));
        }
        "struct_specifier" | "union_specifier" | "class_specifier" => {
            if let Some(declaration) = parse_struct(node, source, symbols) {
                declarations.push(Declaration::Struct(declaration));
            }
        }
        "enum_specifier" => {
            if let Some(declaration) = parse_enum(node, source) {
                declarations.push(Declaration::Enum(declaration));
            }
        }
        "function_definition" => {
            if let Some(function) = parse_function(node, source, symbols) {
                declarations.push(Declaration::Function(function));
            }
        }
        "constructor_or_destructor_definition" => {
            if let Some(function) = parse_constructor_or_destructor(node, source, true, symbols) {
                declarations.push(Declaration::Function(function));
            }
        }
        "constructor_or_destructor_declaration" => {
            if let Some(function) = parse_constructor_or_destructor(node, source, false, symbols) {
                declarations.push(Declaration::Function(function));
            }
        }
        _ => {}
    }
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
        Declaration::MacroUndef(value) => value.line,
        Declaration::Include(value) => value.line,
        Declaration::Namespace(value) => value.line,
        Declaration::Struct(value) => value.line,
        Declaration::Enum(value) => value.line,
        Declaration::Typedef(value) => value.line,
        Declaration::GlobalVariable(value) => value.line,
        Declaration::Function(value) => value.line,
    }
}

fn declaration_visible_line(declaration: &Declaration) -> Option<usize> {
    match declaration {
        Declaration::Macro(value) => value.visible_line,
        Declaration::MacroUndef(value) => value.visible_line,
        Declaration::Include(value) => value.visible_line,
        Declaration::Namespace(value) => value.visible_line,
        Declaration::Struct(value) => value.visible_line,
        Declaration::Enum(value) => value.visible_line,
        Declaration::Typedef(value) => value.visible_line,
        Declaration::GlobalVariable(value) => value.visible_line,
        Declaration::Function(value) => value.visible_line,
    }
}

fn declaration_sort_line(declaration: &Declaration) -> usize {
    declaration_visible_line(declaration).unwrap_or_else(|| declaration_line(declaration))
}

fn parse_macro(node: Node, source: &[u8]) -> Option<MacroDecl> {
    let code = node_text(node, source).trim().to_string();
    let definition = code
        .strip_prefix('#')
        .unwrap_or(&code)
        .trim_start()
        .strip_prefix("define")
        .unwrap_or(&code)
        .trim()
        .to_string();
    macro_from_definition(&definition, code, line(node))
}

fn parse_macro_undef(node: Node, source: &[u8]) -> Option<MacroUndefDecl> {
    let directive = node
        .child_by_field_name("directive")
        .map(|directive| node_text(directive, source).trim())
        .unwrap_or_default()
        .trim_start_matches('#');
    if directive != "undef" {
        return None;
    }
    let name = node
        .child_by_field_name("argument")
        .map(|argument| node_text(argument, source).trim())
        .unwrap_or_default()
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    if name.is_empty() {
        return None;
    }
    Some(MacroUndefDecl {
        name,
        code: node_text(node, source).trim().to_string(),
        line: line(node),
        source_path: None,
        visible_line: None,
    })
}

fn synthetic_macro_declarations(defines: &[String]) -> Vec<Declaration> {
    defines
        .iter()
        .filter_map(|define| macro_from_define_option(define))
        .map(Declaration::Macro)
        .collect()
}

fn register_macro_symbols(declarations: &[Declaration], symbols: &mut MacroSymbols) {
    for declaration in declarations {
        if let Declaration::Macro(macro_decl) = declaration {
            define_macro_symbol(symbols, macro_decl);
        }
    }
}

fn define_macro_symbol(symbols: &mut MacroSymbols, declaration: &MacroDecl) {
    symbols.insert(
        declaration.name.clone(),
        MacroBinding::from_decl(declaration),
    );
}

fn header_declarations(
    header_path: &Path,
    visible_line: usize,
    options: &ParseOptions,
    symbols: &mut MacroSymbols,
    visited_headers: &mut HashSet<PathBuf>,
) -> Result<Vec<Declaration>> {
    let normalized_header = normalized_absolute_path(header_path);
    if !visited_headers.insert(normalized_header.clone()) {
        return Ok(Vec::new());
    }

    let source = fs::read_to_string(&normalized_header)
        .with_context(|| format!("failed to read included header '{}'", header_path.display()))?;
    let declarations = parse_declarations_with_context(
        &source,
        language_for_path(&normalized_header),
        Some(&normalized_header),
        Some(options),
        symbols,
        visited_headers,
    )?;
    Ok(declarations
        .into_iter()
        .filter_map(|declaration| {
            let declaration =
                annotate_header_declaration(declaration, &normalized_header, visible_line);
            if let Declaration::Macro(macro_decl) = &declaration {
                define_macro_symbol(symbols, macro_decl);
            } else if let Declaration::MacroUndef(macro_undef) = &declaration {
                symbols.remove(&macro_undef.name);
            }
            if options.import_header_declarations
                || matches!(
                    declaration,
                    Declaration::Macro(_) | Declaration::MacroUndef(_)
                )
            {
                Some(declaration)
            } else {
                None
            }
        })
        .collect())
}

fn annotate_header_declaration(
    mut declaration: Declaration,
    header_path: &Path,
    visible_line: usize,
) -> Declaration {
    let source_path = normalize_path(header_path);
    match &mut declaration {
        Declaration::Macro(value) => {
            if value.source_path.is_none() {
                value.source_path = Some(source_path);
            }
            value.visible_line = Some(visible_line);
        }
        Declaration::MacroUndef(value) => {
            if value.source_path.is_none() {
                value.source_path = Some(source_path);
            }
            value.visible_line = Some(visible_line);
        }
        Declaration::Include(value) => {
            if value.source_path.is_none() {
                value.source_path = Some(source_path);
            }
            value.visible_line = Some(visible_line);
        }
        Declaration::Namespace(value) => {
            if value.source_path.is_none() {
                value.source_path = Some(source_path.clone());
            }
            value.visible_line = Some(visible_line);
            value.declarations = value
                .declarations
                .drain(..)
                .map(|nested| annotate_header_declaration(nested, header_path, visible_line))
                .collect();
        }
        Declaration::Struct(value) => {
            if value.source_path.is_none() {
                value.source_path = Some(source_path.clone());
            }
            value.visible_line = Some(visible_line);
            value.nested_declarations = value
                .nested_declarations
                .drain(..)
                .map(|nested| annotate_header_declaration(nested, header_path, visible_line))
                .collect();
        }
        Declaration::Enum(value) => {
            if value.source_path.is_none() {
                value.source_path = Some(source_path);
            }
            value.visible_line = Some(visible_line);
        }
        Declaration::Typedef(value) => {
            if value.source_path.is_none() {
                value.source_path = Some(source_path);
            }
            value.visible_line = Some(visible_line);
        }
        Declaration::GlobalVariable(value) => {
            if value.source_path.is_none() {
                value.source_path = Some(source_path);
            }
            value.visible_line = Some(visible_line);
        }
        Declaration::Function(value) => {
            if value.source_path.is_none() {
                value.source_path = Some(source_path);
            }
            value.visible_line = Some(visible_line);
        }
    }
    declaration
}

fn parse_preproc_declarations(
    node: Node,
    source: &[u8],
    source_path: Option<&Path>,
    options: Option<&ParseOptions>,
    symbols: &mut MacroSymbols,
    visited_headers: &mut HashSet<PathBuf>,
) -> Result<Vec<Declaration>> {
    if preproc_branch_is_active(node, source, symbols) {
        parse_selected_preproc_declaration_children(
            node,
            source,
            source_path,
            options,
            symbols,
            visited_headers,
        )
    } else if let Some(alternative) = node.child_by_field_name("alternative") {
        parse_preproc_declarations(
            alternative,
            source,
            source_path,
            options,
            symbols,
            visited_headers,
        )
    } else {
        Ok(Vec::new())
    }
}

fn parse_selected_preproc_declaration_children(
    node: Node,
    source: &[u8],
    source_path: Option<&Path>,
    options: Option<&ParseOptions>,
    symbols: &mut MacroSymbols,
    visited_headers: &mut HashSet<PathBuf>,
) -> Result<Vec<Declaration>> {
    let mut declarations = Vec::new();
    for child in preproc_body_children(node) {
        declarations.extend(parse_declaration_node(
            child,
            source,
            source_path,
            options,
            symbols,
            visited_headers,
        )?);
    }
    Ok(declarations)
}

fn preproc_body_children(node: Node) -> Vec<Node> {
    let condition = node
        .child_by_field_name("condition")
        .or_else(|| node.child_by_field_name("name"));
    let alternative = node.child_by_field_name("alternative");
    named_children(node)
        .into_iter()
        .filter(|child| Some(*child) != condition && Some(*child) != alternative)
        .collect()
}

fn preproc_branch_is_active(node: Node, source: &[u8], symbols: &MacroSymbols) -> bool {
    match node.kind() {
        "preproc_else" => true,
        "preproc_ifdef" | "preproc_elifdef" => {
            let Some(name) = node.child_by_field_name("name") else {
                return false;
            };
            let is_defined = symbols.contains_key(node_text(name, source).trim());
            if preproc_directive(node, source).contains("ifndef") {
                !is_defined
            } else {
                is_defined
            }
        }
        "preproc_if" | "preproc_elif" => node
            .child_by_field_name("condition")
            .map(|condition| eval_preproc_condition(condition, source, symbols) != 0)
            .unwrap_or(false),
        _ => false,
    }
}

fn preproc_directive(node: Node, source: &[u8]) -> String {
    node_text(node, source)
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string()
}

fn eval_preproc_condition(node: Node, source: &[u8], symbols: &MacroSymbols) -> i64 {
    match node.kind() {
        "parenthesized_expression" | "condition_clause" => named_children(node)
            .into_iter()
            .next()
            .map(|child| eval_preproc_condition(child, source, symbols))
            .unwrap_or(0),
        "number_literal" => integer_literal_value(node_text(node, source)).unwrap_or(0),
        "identifier" => eval_preproc_identifier(node_text(node, source).trim(), symbols),
        "preproc_defined" => named_children(node)
            .into_iter()
            .next()
            .map(|name| symbols.contains_key(node_text(name, source).trim()) as i64)
            .unwrap_or(0),
        "unary_expression" => eval_unary_preproc_condition(node, source, symbols),
        "binary_expression" => eval_binary_preproc_condition(node, source, symbols),
        _ => 0,
    }
}

fn eval_preproc_identifier(name: &str, symbols: &MacroSymbols) -> i64 {
    let Some(binding) = symbols.get(name) else {
        return 0;
    };
    if !binding.parameters.is_empty() {
        return 1;
    }
    macro_body_integer_value(&binding.body).unwrap_or(1)
}

fn macro_body_integer_value(body: &str) -> Option<i64> {
    integer_literal_value(strip_wrapping_parentheses(body.trim()))
}

fn eval_unary_preproc_condition(node: Node, source: &[u8], symbols: &MacroSymbols) -> i64 {
    let operator = operator_text(node, source).unwrap_or_default();
    let value = node
        .child_by_field_name("argument")
        .or_else(|| named_children(node).into_iter().next())
        .map(|argument| eval_preproc_condition(argument, source, symbols))
        .unwrap_or(0);
    match operator {
        "!" | "not" => (value == 0) as i64,
        "-" => -value,
        "+" => value,
        _ => value,
    }
}

fn eval_binary_preproc_condition(node: Node, source: &[u8], symbols: &MacroSymbols) -> i64 {
    let left = node
        .child_by_field_name("left")
        .or_else(|| named_children(node).into_iter().next())
        .map(|left| eval_preproc_condition(left, source, symbols))
        .unwrap_or(0);
    let right = node
        .child_by_field_name("right")
        .or_else(|| named_children(node).into_iter().nth(1))
        .map(|right| eval_preproc_condition(right, source, symbols))
        .unwrap_or(0);
    match operator_text(node, source).unwrap_or_default() {
        "&&" | "and" => (left != 0 && right != 0) as i64,
        "||" | "or" => (left != 0 || right != 0) as i64,
        "==" => (left == right) as i64,
        "!=" => (left != right) as i64,
        ">" => (left > right) as i64,
        ">=" => (left >= right) as i64,
        "<" => (left < right) as i64,
        "<=" => (left <= right) as i64,
        "+" => left + right,
        "-" => left - right,
        "*" => left * right,
        "/" if right != 0 => left / right,
        "%" if right != 0 => left % right,
        _ => 0,
    }
}

fn integer_literal_value(value: &str) -> Option<i64> {
    let trimmed = value
        .trim()
        .trim_end_matches(|ch: char| matches!(ch, 'u' | 'U' | 'l' | 'L'));
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        i64::from_str_radix(hex, 16).ok()
    } else {
        trimmed.parse::<i64>().ok()
    }
}

fn strip_wrapping_parentheses(mut value: &str) -> &str {
    loop {
        let trimmed = value.trim();
        if !(trimmed.starts_with('(') && trimmed.ends_with(')')) {
            return trimmed;
        }
        let inner = &trimmed[1..trimmed.len() - 1];
        if parentheses_are_balanced(inner) {
            value = inner;
        } else {
            return trimmed;
        }
    }
}

fn parentheses_are_balanced(value: &str) -> bool {
    let mut depth = 0usize;
    for ch in value.chars() {
        match ch {
            '(' => depth += 1,
            ')' if depth == 0 => return false,
            ')' => depth -= 1,
            _ => {}
        }
    }
    depth == 0
}

fn resolve_include(
    source_path: &Path,
    include_name: &str,
    options: &ParseOptions,
) -> Option<PathBuf> {
    let include_path = Path::new(include_name);
    if include_path.is_absolute() && include_path.is_file() {
        return Some(include_path.to_path_buf());
    }

    let local_candidate = source_path.parent().map(|parent| parent.join(include_path));
    local_candidate
        .filter(|candidate| candidate.is_file())
        .or_else(|| {
            options
                .include_paths
                .iter()
                .map(|include_dir| Path::new(include_dir).join(include_path))
                .find(|candidate| candidate.is_file())
        })
}

fn normalized_absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
    .components()
    .collect()
}

fn macro_from_define_option(define: &str) -> Option<MacroDecl> {
    let define = define.trim();
    if define.is_empty() {
        return None;
    }

    let (definition, code) = match define.split_once('=') {
        Some((head, body)) if body.is_empty() => {
            (head.trim().to_string(), format!("#define {}", head.trim()))
        }
        Some((head, body)) => (
            format!("{} {}", head.trim(), body.trim()),
            format!("#define {} {}", head.trim(), body.trim()),
        ),
        None => (format!("{define} 1"), format!("#define {define} 1")),
    };
    macro_from_definition(&definition, code, 1)
}

fn macro_from_definition(definition: &str, code: String, line: usize) -> Option<MacroDecl> {
    let name_end = definition
        .find(|ch: char| ch == '(' || ch.is_whitespace())
        .unwrap_or(definition.len());
    let name = definition[..name_end].trim().to_string();
    if name.is_empty() {
        return None;
    }
    let rest = &definition[name_end..];
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
        (Vec::new(), rest.trim_start().to_string())
    };
    Some(MacroDecl {
        name,
        code,
        line,
        source_path: None,
        visible_line: None,
        parameters,
        body,
    })
}

fn parse_include(node: Node, source: &[u8]) -> Option<IncludeDecl> {
    let path = node.child_by_field_name("path")?;
    let code = node_text(node, source).trim().to_string();
    let name = include_name(node_text(path, source));
    if name.is_empty() {
        return None;
    }
    Some(IncludeDecl {
        name,
        code,
        line: line(node),
        source_path: None,
        visible_line: None,
    })
}

fn parse_namespace(
    node: Node,
    source: &[u8],
    source_path: Option<&Path>,
    options: Option<&ParseOptions>,
    symbols: &mut MacroSymbols,
    visited_headers: &mut HashSet<PathBuf>,
) -> Result<Option<NamespaceDecl>> {
    let body = node.child_by_field_name("body");
    let declarations = match body {
        Some(body) => parse_declaration_children(
            body,
            source,
            source_path,
            options,
            symbols,
            visited_headers,
        )?,
        None => Vec::new(),
    };
    Ok(Some(NamespaceDecl {
        name: node
            .child_by_field_name("name")
            .map(|name| node_text(name, source).trim().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "<anonymous_namespace>".to_string()),
        code: compact_code(node_text(node, source)),
        line: line(node),
        source_path: None,
        visible_line: None,
        declarations,
    }))
}

fn include_name(path: &str) -> String {
    path.trim()
        .trim_start_matches('"')
        .trim_end_matches('"')
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_string()
}

fn parse_type_declarations(
    node: Node,
    source: &[u8],
    symbols: &mut MacroSymbols,
) -> Vec<Declaration> {
    named_children(node)
        .into_iter()
        .filter_map(|child| match child.kind() {
            "struct_specifier" | "union_specifier" | "class_specifier"
                if child.child_by_field_name("body").is_some() =>
            {
                parse_struct(child, source, symbols).map(Declaration::Struct)
            }
            "enum_specifier" if child.child_by_field_name("body").is_some() => {
                parse_enum(child, source).map(Declaration::Enum)
            }
            _ => None,
        })
        .collect()
}

fn parse_struct(node: Node, source: &[u8], symbols: &mut MacroSymbols) -> Option<StructDecl> {
    let name_node = node.child_by_field_name("name")?;
    parse_struct_with_name(
        node,
        source,
        node_text(name_node, source).to_string(),
        symbols,
    )
}

fn parse_struct_with_name(
    node: Node,
    source: &[u8],
    name: String,
    symbols: &mut MacroSymbols,
) -> Option<StructDecl> {
    let body = node.child_by_field_name("body")?;
    let field_nodes = named_children(body)
        .into_iter()
        .filter(|child| child.kind() == "field_declaration")
        .collect::<Vec<_>>();
    let mut anonymous_index = 0;
    let mut nested_declarations = field_nodes
        .iter()
        .flat_map(|field| {
            parse_nested_aggregate_declaration(*field, source, &mut anonymous_index, symbols)
        })
        .collect::<Vec<_>>();
    nested_declarations.extend(
        field_nodes
            .iter()
            .filter_map(|field| parse_function_declaration(*field, source))
            .map(Declaration::Function),
    );
    nested_declarations.extend(
        named_children(body)
            .into_iter()
            .filter(|child| child.kind() == "declaration")
            .filter_map(|declaration| parse_function_declaration(declaration, source))
            .map(Declaration::Function),
    );
    nested_declarations.extend(
        named_children(body)
            .into_iter()
            .filter(|child| child.kind() == "function_definition")
            .filter_map(|function| parse_function(function, source, symbols))
            .map(Declaration::Function),
    );
    nested_declarations.extend(
        named_children(body)
            .into_iter()
            .filter(|child| {
                matches!(
                    child.kind(),
                    "constructor_or_destructor_definition"
                        | "constructor_or_destructor_declaration"
                )
            })
            .filter_map(|function| {
                parse_constructor_or_destructor(
                    function,
                    source,
                    function.kind() == "constructor_or_destructor_definition",
                    symbols,
                )
            })
            .map(Declaration::Function),
    );
    Some(StructDecl {
        name,
        code: compact_code(node_text(node, source)),
        line: line(node),
        source_path: None,
        visible_line: None,
        base_classes: parse_base_classes(node, source),
        fields: field_nodes
            .into_iter()
            .filter_map(|field| parse_field(field, source))
            .collect(),
        nested_declarations,
    })
}

fn parse_base_classes(node: Node, source: &[u8]) -> Vec<String> {
    named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "base_class_clause")
        .flat_map(|base_clause| {
            named_children(base_clause)
                .into_iter()
                .filter(|child| {
                    matches!(
                        child.kind(),
                        "type_identifier" | "qualified_identifier" | "template_type"
                    )
                })
                .map(|base| normalize_type(node_text(base, source)))
        })
        .collect()
}

fn parse_nested_aggregate_declaration(
    node: Node,
    source: &[u8],
    anonymous_index: &mut usize,
    symbols: &mut MacroSymbols,
) -> Vec<Declaration> {
    if node.kind() != "field_declaration" {
        return Vec::new();
    }
    let Some(type_node) = node.child_by_field_name("type") else {
        return Vec::new();
    };
    if type_node.child_by_field_name("body").is_none() {
        return Vec::new();
    }
    match type_node.kind() {
        "struct_specifier" | "union_specifier" | "class_specifier" => {
            let name = aggregate_name_for_field(type_node, node, source, anonymous_index);
            parse_struct_with_name(type_node, source, name, symbols)
                .map(Declaration::Struct)
                .into_iter()
                .collect()
        }
        "enum_specifier" => parse_enum(type_node, source)
            .map(Declaration::Enum)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

fn aggregate_name_for_field(
    type_node: Node,
    field_node: Node,
    source: &[u8],
    anonymous_index: &mut usize,
) -> String {
    type_node
        .child_by_field_name("name")
        .map(|name| node_text(name, source).trim().to_string())
        .or_else(|| {
            field_node
                .child_by_field_name("declarator")
                .and_then(|declarator| declarator_name(declarator, source))
        })
        .unwrap_or_else(|| {
            let name = format!("<type>{anonymous_index}");
            *anonymous_index += 1;
            name
        })
}

fn parse_field(node: Node, source: &[u8]) -> Option<FieldDecl> {
    let type_node = node.child_by_field_name("type");
    if node
        .child_by_field_name("declarator")
        .is_some_and(is_function_prototype_declarator)
    {
        return None;
    }
    if type_node.is_some_and(|type_node| type_node.child_by_field_name("body").is_some())
        && node.child_by_field_name("declarator").is_none()
    {
        return None;
    }
    let code = node_text(node, source).trim().trim_end_matches(';').trim();
    if let Some((type_name, name)) = anonymous_aggregate_field_type_and_name(node, source) {
        return Some(FieldDecl {
            name,
            type_name,
            code: code.to_string(),
            is_static: is_static_field(node, source),
        });
    }
    let (type_name, name) =
        declaration_type_and_name(node, source).or_else(|| split_type_and_name(code))?;
    Some(FieldDecl {
        name,
        type_name,
        code: code.to_string(),
        is_static: is_static_field(node, source),
    })
}

fn is_static_field(node: Node, source: &[u8]) -> bool {
    named_children(node).into_iter().any(|child| {
        child.kind() == "storage_class_specifier"
            && node_text(child, source)
                .split_whitespace()
                .any(|specifier| specifier == "static")
    })
}

fn anonymous_aggregate_field_type_and_name(node: Node, source: &[u8]) -> Option<(String, String)> {
    let type_node = node.child_by_field_name("type")?;
    if type_node.child_by_field_name("name").is_some()
        || type_node.child_by_field_name("body").is_none()
        || !matches!(type_node.kind(), "struct_specifier" | "union_specifier")
    {
        return None;
    }
    let declarator = node.child_by_field_name("declarator")?;
    let name = declarator_name(declarator, source)?;
    Some((type_from_declarator(&name, declarator, source), name))
}

fn parse_enum(node: Node, source: &[u8]) -> Option<EnumDecl> {
    let name_node = node.child_by_field_name("name")?;
    parse_enum_with_name(node, source, node_text(name_node, source).to_string())
}

fn parse_enum_with_name(node: Node, source: &[u8], name: String) -> Option<EnumDecl> {
    let body = node.child_by_field_name("body")?;
    Some(EnumDecl {
        name,
        code: compact_code(node_text(node, source)),
        line: line(node),
        source_path: None,
        visible_line: None,
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
        line: line(node),
    })
}

fn parse_typedef_declarations(node: Node, source: &[u8]) -> Vec<TypedefDecl> {
    let Some(type_node) = node.child_by_field_name("type") else {
        return Vec::new();
    };
    if is_anonymous_aggregate_type(type_node) {
        return Vec::new();
    }
    let base_type = type_name_from_type_node(type_node, source);
    named_children(node)
        .into_iter()
        .filter(|child| *child != type_node)
        .filter_map(|declarator| {
            let name = declarator_name(declarator, source)?;
            Some(TypedefDecl {
                name,
                type_name: type_from_declarator(&base_type, declarator, source),
                code: node_text(node, source).trim().to_string(),
                line: line(declarator),
                source_path: None,
                visible_line: None,
            })
        })
        .collect()
}

fn parse_alias_declaration(node: Node, source: &[u8]) -> Option<TypedefDecl> {
    let name_node = node.child_by_field_name("name")?;
    let type_node = node.child_by_field_name("type")?;
    Some(TypedefDecl {
        name: node_text(name_node, source).trim().to_string(),
        type_name: type_name_from_type_descriptor(type_node, source),
        code: node_text(node, source).trim().to_string(),
        line: line(name_node),
        source_path: None,
        visible_line: None,
    })
}

fn parse_anonymous_typedef_aggregate_declarations(
    node: Node,
    source: &[u8],
    symbols: &mut MacroSymbols,
) -> Vec<Declaration> {
    let Some(type_node) = node.child_by_field_name("type") else {
        return Vec::new();
    };
    if !is_anonymous_aggregate_type(type_node) {
        return Vec::new();
    }

    named_children(node)
        .into_iter()
        .filter(|child| *child != type_node)
        .filter_map(|declarator| {
            let alias = declarator_name(declarator, source)?;
            match type_node.kind() {
                "struct_specifier" | "union_specifier" | "class_specifier" => {
                    parse_struct_with_name(type_node, source, alias, symbols)
                        .map(Declaration::Struct)
                }
                "enum_specifier" => {
                    parse_enum_with_name(type_node, source, alias).map(Declaration::Enum)
                }
                _ => None,
            }
        })
        .collect()
}

fn parse_global_variable_declarations(node: Node, source: &[u8]) -> Vec<GlobalVariableDecl> {
    let Some(type_node) = node.child_by_field_name("type") else {
        return Vec::new();
    };
    let base_type = type_name_from_type_node(type_node, source);
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
                source_path: None,
                visible_line: None,
                initializer: declarator
                    .child_by_field_name("value")
                    .map(|value| parse_expression(value, source)),
            })
        })
        .collect()
}

fn parse_function(node: Node, source: &[u8], symbols: &mut MacroSymbols) -> Option<FunctionDecl> {
    let type_node = node.child_by_field_name("type");
    let declarator = node.child_by_field_name("declarator")?;
    let body = node.child_by_field_name("body")?;
    let name = declarator_name(declarator, source)?;
    let function_declarator = function_declarator_node(declarator).unwrap_or(declarator);
    let return_type = type_node
        .map(|type_node| {
            type_from_declarator(
                &type_name_from_type_node(type_node, source),
                declarator,
                source,
            )
        })
        .unwrap_or_else(|| "void".to_string());
    let parameters = function_declarator
        .child_by_field_name("parameters")
        .map(|params| parse_parameters(params, source))
        .unwrap_or_default();
    let constructor_initializers = parse_constructor_initializers(node, source);
    let is_const = is_const_function_declarator(function_declarator, source);
    let is_virtual = is_virtual_function(node, declarator, source);
    Some(FunctionDecl {
        name,
        signature: function_signature(&return_type, &parameters, is_const),
        return_type,
        is_definition: true,
        is_static: is_static_function(node, source),
        is_const,
        is_virtual,
        code: compact_code(node_text(node, source)),
        line: line(node),
        source_path: None,
        visible_line: None,
        parameters,
        constructor_initializers,
        body: parse_statement_block(body, source, symbols),
    })
}

fn parse_function_declaration(node: Node, source: &[u8]) -> Option<FunctionDecl> {
    let type_node = node.child_by_field_name("type");
    let declarator = node.child_by_field_name("declarator")?;
    if !is_function_prototype_declarator(declarator) {
        return None;
    }
    let name = declarator_name(declarator, source)?;
    let function_declarator = function_declarator_node(declarator).unwrap_or(declarator);
    let return_type = type_node
        .map(|type_node| {
            type_from_declarator(
                &type_name_from_type_node(type_node, source),
                declarator,
                source,
            )
        })
        .unwrap_or_else(|| "void".to_string());
    let parameters = function_declarator
        .child_by_field_name("parameters")
        .map(|params| parse_parameters(params, source))
        .unwrap_or_default();
    let is_const = is_const_function_declarator(function_declarator, source);
    let is_virtual = is_virtual_function(node, declarator, source);
    Some(FunctionDecl {
        name,
        signature: function_signature(&return_type, &parameters, is_const),
        return_type,
        is_definition: false,
        is_static: is_static_function(node, source),
        is_const,
        is_virtual,
        code: statement_code(node, source),
        line: line(node),
        source_path: None,
        visible_line: None,
        parameters,
        constructor_initializers: Vec::new(),
        body: Vec::new(),
    })
}

fn parse_constructor_or_destructor(
    node: Node,
    source: &[u8],
    is_definition: bool,
    symbols: &mut MacroSymbols,
) -> Option<FunctionDecl> {
    let declarator = node.child_by_field_name("declarator")?;
    if !is_function_prototype_declarator(declarator) {
        return None;
    }
    let name = declarator_name(declarator, source)?;
    let function_declarator = function_declarator_node(declarator).unwrap_or(declarator);
    let return_type = "void".to_string();
    let parameters = function_declarator
        .child_by_field_name("parameters")
        .map(|params| parse_parameters(params, source))
        .unwrap_or_default();
    let constructor_initializers = parse_constructor_initializers(node, source);
    let body = node
        .child_by_field_name("body")
        .map(|body| parse_statement_block(body, source, symbols))
        .unwrap_or_default();
    let is_virtual = is_virtual_function(node, declarator, source);
    Some(FunctionDecl {
        name,
        signature: function_signature(&return_type, &parameters, false),
        return_type,
        is_definition: is_definition && node.child_by_field_name("body").is_some(),
        is_static: false,
        is_const: false,
        is_virtual,
        code: if is_definition {
            compact_code(node_text(node, source))
        } else {
            statement_code(node, source)
        },
        line: line(node),
        source_path: None,
        visible_line: None,
        parameters,
        constructor_initializers,
        body,
    })
}

fn is_static_function(node: Node, source: &[u8]) -> bool {
    named_children(node).into_iter().any(|child| {
        child.kind() == "storage_class_specifier"
            && node_text(child, source)
                .split_whitespace()
                .any(|specifier| specifier == "static")
    })
}

fn is_const_function_declarator(declarator: Node, source: &[u8]) -> bool {
    named_children(declarator).into_iter().any(|child| {
        child.kind() == "type_qualifier"
            && node_text(child, source)
                .split_whitespace()
                .any(|qualifier| qualifier == "const")
    })
}

fn is_virtual_function(node: Node, declarator: Node, source: &[u8]) -> bool {
    has_named_descendant_kind(declarator, "virtual_specifier")
        || has_named_descendant_kind(node, "pure_virtual_clause")
        || header_tokens(node, source).any(|token| token == "virtual")
}

fn has_named_descendant_kind(node: Node, kind: &str) -> bool {
    named_children(node)
        .into_iter()
        .any(|child| child.kind() == kind || has_named_descendant_kind(child, kind))
}

fn header_tokens<'a>(node: Node, source: &'a [u8]) -> impl Iterator<Item = &'a str> {
    let end_byte = node
        .child_by_field_name("body")
        .map(|body| body.start_byte())
        .unwrap_or_else(|| node.end_byte());
    std::str::from_utf8(&source[node.start_byte()..end_byte])
        .unwrap_or("")
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .filter(|token| !token.is_empty())
}

fn parse_constructor_initializers(node: Node, source: &[u8]) -> Vec<ConstructorInitializer> {
    named_children(node)
        .into_iter()
        .find(|child| child.kind() == "field_initializer_list")
        .map(|initializer_list| {
            named_children(initializer_list)
                .into_iter()
                .filter(|initializer| initializer.kind() == "field_initializer")
                .filter_map(|initializer| parse_constructor_initializer(initializer, source))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_constructor_initializer(node: Node, source: &[u8]) -> Option<ConstructorInitializer> {
    let field_node = named_children(node).into_iter().find(|child| {
        matches!(
            child.kind(),
            "field_identifier" | "qualified_identifier" | "template_method"
        )
    })?;
    let arguments = named_children(node)
        .into_iter()
        .find(|child| matches!(child.kind(), "argument_list" | "initializer_list"))
        .map(|arguments| initializer_list_elements(arguments, source))
        .unwrap_or_default();
    Some(ConstructorInitializer {
        field: node_text(field_node, source).trim().to_string(),
        code: compact_code(node_text(node, source)),
        line: line(node),
        arguments,
    })
}

fn is_function_prototype_declarator(node: Node) -> bool {
    let Some(function_declarator) = function_declarator_node(node) else {
        return false;
    };
    function_declarator
        .child_by_field_name("declarator")
        .is_some_and(|declarator| declarator.kind() != "parenthesized_declarator")
}

fn function_declarator_node(node: Node) -> Option<Node> {
    if node.kind() == "function_declarator" {
        Some(node)
    } else {
        node.child_by_field_name("declarator")
            .and_then(function_declarator_node)
            .or_else(|| {
                named_children(node)
                    .into_iter()
                    .find_map(function_declarator_node)
            })
    }
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
        .map(|type_node| type_name_from_type_node(type_node, source))?;
    Some(
        node.child_by_field_name("declarator")
            .map(|declarator| type_from_declarator(&base_type, declarator, source))
            .unwrap_or_else(|| normalize_type(node_text(node, source))),
    )
}

fn parse_statement_block(node: Node, source: &[u8], symbols: &mut MacroSymbols) -> Vec<Statement> {
    named_children(node)
        .into_iter()
        .flat_map(|child| parse_statement(child, source, symbols))
        .collect()
}

fn parse_statement(node: Node, source: &[u8], symbols: &mut MacroSymbols) -> Vec<Statement> {
    match node.kind() {
        "compound_statement" => parse_statement_block(node, source, symbols),
        "declaration" => parse_local_declarations(node, source),
        "return_statement" => vec![Statement::Return {
            code: statement_code(node, source),
            line: line(node),
            expression: named_children(node)
                .into_iter()
                .next()
                .map(|expr| parse_expression(expr, source)),
        }],
        "throw_statement" => vec![Statement::Throw {
            code: statement_code(node, source),
            line: line(node),
            expression: named_children(node)
                .into_iter()
                .next()
                .map(|expr| parse_expression(expr, source)),
        }],
        "try_statement" => parse_try_statement(node, source, symbols)
            .into_iter()
            .collect(),
        "expression_statement" => named_children(node)
            .into_iter()
            .next()
            .map(|expr| statement_from_expression(node, expr, source))
            .into_iter()
            .collect(),
        "if_statement" => parse_if_statement(node, source, symbols)
            .into_iter()
            .collect(),
        "else_clause" => named_children(node)
            .into_iter()
            .flat_map(|child| parse_statement(child, source, symbols))
            .collect(),
        "while_statement" => parse_while_statement(node, source, symbols)
            .into_iter()
            .collect(),
        "do_statement" => parse_do_statement(node, source, symbols)
            .into_iter()
            .collect(),
        "for_statement" => parse_for_statement(node, source, symbols)
            .into_iter()
            .collect(),
        "switch_statement" => parse_switch_statement(node, source, symbols)
            .into_iter()
            .collect(),
        "case_statement" => parse_case_statement(node, source, symbols)
            .into_iter()
            .collect(),
        "labeled_statement" => parse_labeled_statement(node, source, symbols)
            .into_iter()
            .collect(),
        "preproc_def" | "preproc_function_def" => {
            if let Some(macro_decl) = parse_macro(node, source) {
                define_macro_symbol(symbols, &macro_decl);
            }
            Vec::new()
        }
        "preproc_call" => {
            if let Some(macro_undef) = parse_macro_undef(node, source) {
                symbols.remove(&macro_undef.name);
            }
            Vec::new()
        }
        "preproc_if" | "preproc_ifdef" | "preproc_elif" | "preproc_elifdef" | "preproc_else" => {
            parse_preproc_statements(node, source, symbols)
        }
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

fn parse_try_statement(node: Node, source: &[u8], symbols: &mut MacroSymbols) -> Option<Statement> {
    let body = node.child_by_field_name("body")?;
    let catches = named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "catch_clause")
        .map(|catch| parse_catch_clause(catch, source, symbols))
        .collect();
    Some(Statement::Try {
        code: statement_code(node, source),
        line: line(node),
        body: parse_statement(body, source, symbols),
        catches,
    })
}

fn parse_catch_clause(node: Node, source: &[u8], symbols: &mut MacroSymbols) -> CatchClause {
    let parameter = node
        .child_by_field_name("parameters")
        .and_then(|parameters| parse_parameters(parameters, source).into_iter().next());
    let body = node
        .child_by_field_name("body")
        .map(|body| parse_statement(body, source, symbols))
        .unwrap_or_default();
    CatchClause {
        code: statement_code(node, source),
        line: line(node),
        parameter,
        body,
    }
}

fn parse_local_declarations(node: Node, source: &[u8]) -> Vec<Statement> {
    let type_node = node.child_by_field_name("type");
    let type_name = type_node.map(|type_node| type_name_from_type_node(type_node, source));
    named_children(node)
        .into_iter()
        .filter(|child| Some(*child) != type_node)
        .filter_map(|declarator| {
            let name = declarator_name(declarator, source)?;
            let initializer = declarator
                .child_by_field_name("value")
                .map(|value| parse_expression(value, source))
                .or_else(|| direct_initializer_from_declarator(declarator, source));
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

fn direct_initializer_from_declarator(declarator: Node, source: &[u8]) -> Option<Expression> {
    let function_declarator = function_declarator_node(declarator)?;
    let parameters = function_declarator.child_by_field_name("parameters")?;
    let elements = named_children(parameters);
    let elements = elements
        .into_iter()
        .map(|element| direct_initializer_element(element, source))
        .collect::<Option<Vec<_>>>()?;
    Some(Expression::InitializerList {
        code: node_text(parameters, source).trim().to_string(),
        line: line(parameters),
        elements,
    })
}

fn direct_initializer_element(node: Node, source: &[u8]) -> Option<Expression> {
    if node.kind() != "parameter_declaration" {
        return Some(parse_expression(node, source));
    }
    if node.child_by_field_name("declarator").is_some() {
        return None;
    }
    let text = node_text(node, source).trim();
    if !is_simple_identifier(text) {
        return None;
    }
    Some(Expression::Identifier {
        name: text.to_string(),
        code: text.to_string(),
        line: line(node),
    })
}

fn is_simple_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
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

fn parse_preproc_statements(
    node: Node,
    source: &[u8],
    symbols: &mut MacroSymbols,
) -> Vec<Statement> {
    if preproc_branch_is_active(node, source, symbols) {
        preproc_body_children(node)
            .into_iter()
            .flat_map(|child| parse_statement(child, source, symbols))
            .collect()
    } else if let Some(alternative) = node.child_by_field_name("alternative") {
        parse_preproc_statements(alternative, source, symbols)
    } else {
        Vec::new()
    }
}

fn parse_if_statement(node: Node, source: &[u8], symbols: &mut MacroSymbols) -> Option<Statement> {
    let condition = node.child_by_field_name("condition")?;
    let consequence = node.child_by_field_name("consequence")?;
    let else_body = node
        .child_by_field_name("alternative")
        .map(|alternative| parse_statement(alternative, source, symbols))
        .unwrap_or_default();
    Some(Statement::If {
        code: statement_code(node, source),
        line: line(node),
        condition: parse_expression(condition, source),
        then_body: parse_statement(consequence, source, symbols),
        else_body,
    })
}

fn parse_while_statement(
    node: Node,
    source: &[u8],
    symbols: &mut MacroSymbols,
) -> Option<Statement> {
    let condition = node.child_by_field_name("condition")?;
    let body = node.child_by_field_name("body")?;
    Some(Statement::While {
        code: statement_code(node, source),
        line: line(node),
        condition: parse_expression(condition, source),
        body: parse_statement(body, source, symbols),
    })
}

fn parse_do_statement(node: Node, source: &[u8], symbols: &mut MacroSymbols) -> Option<Statement> {
    let condition = node.child_by_field_name("condition")?;
    let body = node.child_by_field_name("body")?;
    Some(Statement::DoWhile {
        code: statement_code(node, source),
        line: line(node),
        condition: parse_expression(condition, source),
        body: parse_statement(body, source, symbols),
    })
}

fn parse_for_statement(node: Node, source: &[u8], symbols: &mut MacroSymbols) -> Option<Statement> {
    let body = node.child_by_field_name("body")?;
    let initializer = node
        .child_by_field_name("initializer")
        .map(|initializer| parse_for_initializer(initializer, source, symbols))
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
        body: parse_statement(body, source, symbols),
    })
}

fn parse_for_initializer(node: Node, source: &[u8], symbols: &mut MacroSymbols) -> Vec<Statement> {
    match node.kind() {
        "declaration" => parse_local_declarations(node, source),
        "preproc_if" | "preproc_ifdef" | "preproc_elif" | "preproc_elifdef" | "preproc_else" => {
            parse_preproc_statements(node, source, symbols)
        }
        "expression_statement" => named_children(node)
            .into_iter()
            .next()
            .map(|expr| statement_from_expression(node, expr, source))
            .into_iter()
            .collect(),
        _ => vec![statement_from_expression(node, node, source)],
    }
}

fn parse_switch_statement(
    node: Node,
    source: &[u8],
    symbols: &mut MacroSymbols,
) -> Option<Statement> {
    let condition = node.child_by_field_name("condition")?;
    let body = node.child_by_field_name("body")?;
    Some(Statement::Switch {
        code: statement_code(node, source),
        line: line(node),
        condition: parse_expression(condition, source),
        body: parse_statement(body, source, symbols),
    })
}

fn parse_case_statement(
    node: Node,
    source: &[u8],
    symbols: &mut MacroSymbols,
) -> Option<Statement> {
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
            .flat_map(|child| parse_statement(child, source, symbols))
            .collect(),
    })
}

fn parse_labeled_statement(
    node: Node,
    source: &[u8],
    symbols: &mut MacroSymbols,
) -> Option<Statement> {
    let label = node.child_by_field_name("label")?;
    Some(Statement::Label {
        code: case_code(node, source),
        line: line(node),
        label: node_text(label, source).trim().to_string(),
        body: named_children(node)
            .into_iter()
            .filter(|child| *child != label)
            .flat_map(|child| parse_statement(child, source, symbols))
            .collect(),
    })
}

fn parse_expression(node: Node, source: &[u8]) -> Expression {
    match node.kind() {
        "parenthesized_expression" | "condition_clause" => named_children(node)
            .into_iter()
            .next()
            .map(|child| parse_expression(child, source))
            .unwrap_or_else(|| parse_expression_text(node_text(node, source), line(node))),
        "identifier" | "this" => Expression::Identifier {
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
        "compound_literal_expression" => parse_compound_literal_expression(node, source),
        "field_expression" => parse_field_expression(node, source),
        "subscript_expression" => parse_subscript_expression(node, source),
        "assignment_expression" => parse_assignment_expression(node, source),
        "cast_expression" => parse_cast_expression(node, source),
        "sizeof_expression" => parse_sizeof_expression(node, source),
        "new_expression" => parse_new_expression(node, source),
        "delete_expression" => parse_delete_expression(node, source),
        "lambda_expression" => parse_lambda_expression(node, source),
        "argument_list" => parse_initializer_list(node, source),
        "initializer_list" => parse_initializer_list(node, source),
        "initializer_pair" => parse_initializer_pair(node, source),
        _ => identifier_expression(node, source),
    }
}

fn parse_binary_expression(node: Node, source: &[u8]) -> Expression {
    let operator = operator_text(node, source).unwrap_or("?");
    parse_binary_like_expression(node, source, operator)
}

fn parse_expression_text(raw: &str, line: usize) -> Expression {
    let code = strip_wrapping_parentheses(raw.trim());
    if let Some((name, arguments)) = parse_call_text(code, line) {
        Expression::Call {
            name: name.clone(),
            code: code.to_string(),
            line,
            callee: Box::new(Expression::Identifier {
                name: name.clone(),
                code: name,
                line,
            }),
            arguments,
        }
    } else if integer_literal_value(code).is_some() {
        Expression::Literal {
            value: code.to_string(),
            code: code.to_string(),
            line,
        }
    } else {
        Expression::Identifier {
            name: code.to_string(),
            code: code.to_string(),
            line,
        }
    }
}

fn parse_call_text(code: &str, line: usize) -> Option<(String, Vec<Expression>)> {
    if !code.ends_with(')') {
        return None;
    }
    let open_index = top_level_call_open_index(code)?;
    let name = code[..open_index].trim();
    if name.is_empty() {
        return None;
    }
    let argument_text = &code[open_index + 1..code.len() - 1];
    Some((
        name.to_string(),
        split_top_level_arguments(argument_text)
            .into_iter()
            .map(|argument| parse_expression_text(argument, line))
            .collect(),
    ))
}

fn top_level_call_open_index(code: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, ch) in code.char_indices() {
        match ch {
            '(' if depth == 0 => return Some(index),
            '(' => depth += 1,
            ')' if depth == 0 => return None,
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

fn split_top_level_arguments(arguments: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    for (index, ch) in arguments.char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth > 0 => depth -= 1,
            ',' if depth == 0 => {
                let argument = arguments[start..index].trim();
                if !argument.is_empty() {
                    result.push(argument);
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    let argument = arguments[start..].trim();
    if !argument.is_empty() {
        result.push(argument);
    }
    result
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
        callee: Box::new(
            function
                .map(|function| parse_expression(function, source))
                .unwrap_or_else(|| identifier_expression(node, source)),
        ),
        arguments,
    }
}

fn parse_compound_literal_expression(node: Node, source: &[u8]) -> Expression {
    let code = node_text(node, source).trim();
    let Some(type_node) = node.child_by_field_name("type") else {
        return identifier_expression(node, source);
    };
    let Some(value) = node.child_by_field_name("value") else {
        return identifier_expression(node, source);
    };
    if code.starts_with('(') {
        return parse_initializer_list(value, source);
    }
    let name = node_text(type_node, source).trim().to_string();
    Expression::Call {
        name: name.clone(),
        code: code.to_string(),
        line: line(node),
        callee: Box::new(Expression::Identifier {
            name: name.clone(),
            code: name,
            line: line(type_node),
        }),
        arguments: initializer_list_elements(value, source),
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
            index: Box::new(parse_subscript_index(index, source)),
        },
        _ => identifier_expression(node, source),
    }
}

fn parse_subscript_index(node: Node, source: &[u8]) -> Expression {
    named_children(node)
        .into_iter()
        .next()
        .map(|index| parse_expression(index, source))
        .unwrap_or_else(|| parse_expression(node, source))
}

fn parse_cast_expression(node: Node, source: &[u8]) -> Expression {
    let value = node.child_by_field_name("value");
    match value {
        Some(value) => Expression::Cast {
            type_name: node
                .child_by_field_name("type")
                .map(|type_node| type_name_from_type_node(type_node, source))
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
            .map(|type_node| type_name_from_type_node(type_node, source)),
    }
}

fn parse_new_expression(node: Node, source: &[u8]) -> Expression {
    let type_name = node
        .child_by_field_name("type")
        .map(|type_node| type_name_from_type_node(type_node, source))
        .unwrap_or_else(|| "ANY".to_string());
    let mut arguments = Vec::new();
    if let Some(declarator) = node.child_by_field_name("declarator") {
        arguments.extend(new_declarator_lengths(declarator, source));
    }
    let mut initializer_arguments = Vec::new();
    if let Some(argument_list) = node.child_by_field_name("arguments") {
        initializer_arguments.extend(initializer_list_elements(argument_list, source));
        arguments.extend(initializer_arguments.clone());
    }
    Expression::New {
        type_name,
        code: node_text(node, source).trim().to_string(),
        line: line(node),
        arguments,
        initializer_arguments,
    }
}

fn new_declarator_lengths(node: Node, source: &[u8]) -> Vec<Expression> {
    let mut lengths = node
        .child_by_field_name("length")
        .map(|length| vec![parse_expression(length, source)])
        .unwrap_or_default();
    lengths.extend(
        named_children(node)
            .into_iter()
            .filter(|child| child.kind() == "new_declarator")
            .flat_map(|child| new_declarator_lengths(child, source)),
    );
    lengths
}

fn parse_delete_expression(node: Node, source: &[u8]) -> Expression {
    named_children(node)
        .into_iter()
        .next()
        .map(|argument| Expression::Delete {
            code: node_text(node, source).trim().to_string(),
            line: line(node),
            argument: Box::new(parse_expression(argument, source)),
        })
        .unwrap_or_else(|| identifier_expression(node, source))
}

fn parse_lambda_expression(node: Node, source: &[u8]) -> Expression {
    let parameters = lambda_parameter_list(node)
        .map(|parameters| parse_parameters(parameters, source))
        .unwrap_or_default();
    let mut symbols = MacroSymbols::new();
    let body = node
        .child_by_field_name("body")
        .or_else(|| {
            named_children(node)
                .into_iter()
                .find(|child| child.kind() == "compound_statement")
        })
        .map(|body| parse_statement_block(body, source, &mut symbols))
        .unwrap_or_default();
    let return_type = lambda_return_type(node, source, &parameters, &body);
    Expression::Lambda {
        code: node_text(node, source).trim().to_string(),
        line: line(node),
        captures: lambda_captures(node, source),
        is_mutable: lambda_is_mutable(node, source),
        signature: signature(&return_type, &parameters),
        return_type,
        parameters,
        body,
    }
}

fn lambda_parameter_list(node: Node) -> Option<Node> {
    node.child_by_field_name("declarator")
        .and_then(|declarator| find_named_descendant_kind(declarator, "parameter_list"))
}

fn lambda_is_mutable(node: Node, source: &[u8]) -> bool {
    node.child_by_field_name("declarator")
        .map(|declarator| {
            named_descendants(declarator).into_iter().any(|child| {
                child.kind() == "type_qualifier" && node_text(child, source).trim() == "mutable"
            })
        })
        .unwrap_or(false)
}

fn lambda_return_type(
    node: Node,
    source: &[u8],
    parameters: &[ParameterDecl],
    body: &[Statement],
) -> String {
    find_lambda_trailing_return_type(node, source).unwrap_or_else(|| {
        body.iter()
            .find_map(|statement| return_statement_expression(statement))
            .map(|expression| expression_static_type(expression, parameters))
            .unwrap_or_else(|| "void".to_string())
    })
}

fn find_lambda_trailing_return_type(node: Node, source: &[u8]) -> Option<String> {
    node.child_by_field_name("declarator")
        .and_then(|declarator| find_named_descendant_kind(declarator, "trailing_return_type"))
        .and_then(|trailing_return| {
            named_children(trailing_return)
                .into_iter()
                .find(|child| child.kind() == "type_descriptor")
        })
        .map(|type_node| type_name_from_type_descriptor(type_node, source))
        .or_else(|| {
            node.child_by_field_name("type")
                .map(|type_node| type_name_from_type_node(type_node, source))
        })
        .or_else(|| {
            let body_start = node
                .child_by_field_name("body")
                .map(|body| body.start_byte())
                .unwrap_or_else(|| node.end_byte());
            named_children(node)
                .into_iter()
                .filter(|child| child.end_byte() <= body_start)
                .rev()
                .find(|child| {
                    matches!(
                        child.kind(),
                        "primitive_type"
                            | "type_identifier"
                            | "qualified_identifier"
                            | "template_type"
                            | "auto"
                    )
                })
                .map(|type_node| type_name_from_type_node(type_node, source))
        })
}

fn return_statement_expression(statement: &Statement) -> Option<&Expression> {
    match statement {
        Statement::Return {
            expression: Some(expression),
            ..
        } => Some(expression),
        Statement::Try { body, catches, .. } => body
            .iter()
            .find_map(return_statement_expression)
            .or_else(|| {
                catches
                    .iter()
                    .flat_map(|catch| catch.body.iter())
                    .find_map(return_statement_expression)
            }),
        Statement::If {
            then_body,
            else_body,
            ..
        } => then_body
            .iter()
            .chain(else_body.iter())
            .find_map(return_statement_expression),
        Statement::While { body, .. }
        | Statement::DoWhile { body, .. }
        | Statement::For { body, .. }
        | Statement::Label { body, .. }
        | Statement::Switch { body, .. }
        | Statement::Case { body, .. } => body.iter().find_map(return_statement_expression),
        _ => None,
    }
}

fn expression_static_type(expression: &Expression, parameters: &[ParameterDecl]) -> String {
    match expression {
        Expression::Literal { value, .. } if integer_literal_value(value).is_some() => {
            "int".to_string()
        }
        Expression::Identifier { name, .. } => parameters
            .iter()
            .find(|parameter| parameter.name == *name)
            .map(|parameter| parameter.type_name.clone())
            .unwrap_or_else(|| "ANY".to_string()),
        Expression::Binary { left, right, .. } => {
            let left_type = expression_static_type(left, parameters);
            let right_type = expression_static_type(right, parameters);
            if left_type == right_type {
                left_type
            } else if left_type == "int" || right_type == "int" {
                "int".to_string()
            } else {
                "ANY".to_string()
            }
        }
        _ => "ANY".to_string(),
    }
}

fn lambda_captures(node: Node, source: &[u8]) -> Vec<LambdaCapture> {
    let code = node_text(node, source);
    let line = line(node);
    let Some(open) = code.find('[') else {
        return Vec::new();
    };
    let Some(close) = code[open + 1..].find(']') else {
        return Vec::new();
    };
    let capture_text = &code[open + 1..open + 1 + close];
    let initializers = lambda_capture_initializers(node, source);
    split_top_level_arguments(capture_text)
        .into_iter()
        .filter_map(|capture| lambda_capture(capture, line, &initializers))
        .collect()
}

fn lambda_capture_initializers(node: Node, source: &[u8]) -> HashMap<String, Expression> {
    node.child_by_field_name("captures")
        .map(named_children)
        .unwrap_or_default()
        .into_iter()
        .filter(|child| child.kind() == "lambda_capture_initializer")
        .filter_map(|capture| {
            let name = capture.child_by_field_name("left")?;
            let initializer = capture.child_by_field_name("right")?;
            Some((
                node_text(name, source).trim().to_string(),
                parse_expression(initializer, source),
            ))
        })
        .collect()
}

fn lambda_capture(
    capture: &str,
    line: usize,
    initializers: &HashMap<String, Expression>,
) -> Option<LambdaCapture> {
    let raw = capture.trim();
    if raw.is_empty() {
        return None;
    }
    match raw {
        "=" => {
            return Some(LambdaCapture {
                name: None,
                code: raw.to_string(),
                capture_kind: "defaultByValue".to_string(),
                initializer: None,
            });
        }
        "&" => {
            return Some(LambdaCapture {
                name: None,
                code: raw.to_string(),
                capture_kind: "defaultByReference".to_string(),
                initializer: None,
            });
        }
        "this" => {
            return Some(LambdaCapture {
                name: Some("this".to_string()),
                code: raw.to_string(),
                capture_kind: "this".to_string(),
                initializer: None,
            });
        }
        "*this" => {
            return Some(LambdaCapture {
                name: Some("this".to_string()),
                code: raw.to_string(),
                capture_kind: "copyThis".to_string(),
                initializer: None,
            });
        }
        _ => {}
    }

    let (is_reference, rest) = raw
        .strip_prefix('&')
        .map(|rest| (true, rest.trim()))
        .unwrap_or((false, raw));
    let rest = rest.strip_prefix("...").unwrap_or(rest).trim();
    if rest == "this" {
        return Some(LambdaCapture {
            name: Some("this".to_string()),
            code: raw.to_string(),
            capture_kind: "this".to_string(),
            initializer: None,
        });
    }
    if rest == "*this" {
        return Some(LambdaCapture {
            name: Some("this".to_string()),
            code: raw.to_string(),
            capture_kind: "copyThis".to_string(),
            initializer: None,
        });
    }

    let (name, capture_kind, initializer) = rest
        .split_once('=')
        .map(|(name, initializer)| {
            (
                name.trim(),
                if is_reference {
                    "initByReference"
                } else {
                    "initByValue"
                },
                Some(Box::new(
                    initializers
                        .get(name.trim())
                        .cloned()
                        .unwrap_or_else(|| parse_expression_text(initializer.trim(), line)),
                )),
            )
        })
        .unwrap_or((
            rest,
            if is_reference {
                "explicitByReference"
            } else {
                "explicitByValue"
            },
            None,
        ));
    let name = name.trim().strip_prefix('*').unwrap_or(name.trim()).trim();
    if name.is_empty() {
        None
    } else {
        Some(LambdaCapture {
            name: Some(name.to_string()),
            code: raw.to_string(),
            capture_kind: capture_kind.to_string(),
            initializer,
        })
    }
}

fn parse_initializer_list(node: Node, source: &[u8]) -> Expression {
    Expression::InitializerList {
        code: node_text(node, source).trim().to_string(),
        line: line(node),
        elements: initializer_list_elements(node, source),
    }
}

fn initializer_list_elements(node: Node, source: &[u8]) -> Vec<Expression> {
    named_children(node)
        .into_iter()
        .map(|element| parse_expression(element, source))
        .collect()
}

fn parse_initializer_pair(node: Node, source: &[u8]) -> Expression {
    let designator = named_children(node)
        .into_iter()
        .find(|child| is_initializer_designator(*child))
        .map(|designator| parse_initializer_designator(designator, source));
    let value = node.child_by_field_name("value").or_else(|| {
        named_children(node)
            .into_iter()
            .rev()
            .find(|child| !is_initializer_designator(*child))
    });

    match (designator, value) {
        (Some(designator), Some(value)) => Expression::DesignatedInitializer {
            code: node_text(node, source).trim().to_string(),
            line: line(node),
            designator: Box::new(designator),
            value: Box::new(parse_expression(value, source)),
        },
        _ => identifier_expression(node, source),
    }
}

fn parse_initializer_designator(node: Node, source: &[u8]) -> Expression {
    match node.kind() {
        "field_designator" | "field_identifier" => {
            let field = named_children(node)
                .into_iter()
                .last()
                .map(|child| node_text(child, source))
                .unwrap_or_else(|| node_text(node, source))
                .trim()
                .trim_start_matches('.')
                .to_string();
            Expression::Designator {
                name: field.clone(),
                code: field,
                line: line(node),
            }
        }
        "subscript_designator" => named_children(node)
            .into_iter()
            .next()
            .map(|index| parse_expression(index, source))
            .unwrap_or_else(|| identifier_expression(node, source)),
        "subscript_range_designator" => {
            let start = node
                .child_by_field_name("start")
                .or_else(|| named_children(node).into_iter().next());
            let end = node
                .child_by_field_name("end")
                .or_else(|| named_children(node).into_iter().nth(1));
            Expression::InitializerList {
                code: node_text(node, source).trim().to_string(),
                line: line(node),
                elements: start
                    .into_iter()
                    .chain(end)
                    .map(|element| parse_expression(element, source))
                    .collect(),
            }
        }
        _ => parse_expression(node, source),
    }
}

fn is_initializer_designator(node: Node) -> bool {
    matches!(
        node.kind(),
        "field_designator"
            | "field_identifier"
            | "subscript_designator"
            | "subscript_range_designator"
    )
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
        .map(|type_node| type_name_from_type_node(type_node, source))?;
    let declarator = node.child_by_field_name("declarator")?;
    let name = declarator_name(declarator, source)?;
    let type_name = type_from_declarator(&base_type, declarator, source);
    Some((type_name, name))
}

fn declarator_name(node: Node, source: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier"
        | "field_identifier"
        | "type_identifier"
        | "qualified_identifier"
        | "destructor_name"
        | "operator_name" => Some(node_text(node, source).trim().to_string()),
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

fn type_name_from_type_node(node: Node, source: &[u8]) -> String {
    match node.kind() {
        "struct_specifier" | "union_specifier" | "enum_specifier" => node
            .child_by_field_name("name")
            .map(|name| normalize_type(node_text(name, source)))
            .unwrap_or_else(|| normalize_type(node_text(node, source))),
        "class_specifier" => node
            .child_by_field_name("name")
            .map(|name| normalize_type(node_text(name, source)))
            .unwrap_or_else(|| normalize_type(node_text(node, source))),
        _ => normalize_type(node_text(node, source)),
    }
}

fn type_name_from_type_descriptor(node: Node, source: &[u8]) -> String {
    let Some(type_node) = node.child_by_field_name("type") else {
        return normalize_type(node_text(node, source));
    };
    let Some(declarator) = node.child_by_field_name("declarator") else {
        return normalize_type(node_text(node, source));
    };
    let base_type = std::str::from_utf8(&source[node.start_byte()..declarator.start_byte()])
        .ok()
        .map(normalize_type)
        .filter(|base| !base.is_empty())
        .unwrap_or_else(|| type_name_from_type_node(type_node, source));
    type_from_declarator(&base_type, declarator, source)
}

fn is_anonymous_aggregate_type(node: Node) -> bool {
    matches!(
        node.kind(),
        "struct_specifier" | "union_specifier" | "enum_specifier" | "class_specifier"
    ) && node.child_by_field_name("body").is_some()
        && node.child_by_field_name("name").is_none()
}

fn type_from_declarator(base_type: &str, declarator: Node, source: &[u8]) -> String {
    match declarator.kind() {
        "pointer_declarator" | "abstract_pointer_declarator" => declarator
            .child_by_field_name("declarator")
            .map(|child| format!("{}*", type_from_declarator(base_type, child, source)))
            .unwrap_or_else(|| format!("{base_type}*")),
        "reference_declarator" | "abstract_reference_declarator" => {
            let reference = reference_operator(declarator, source);
            declarator
                .child_by_field_name("declarator")
                .map(|child| {
                    format!(
                        "{}{}",
                        type_from_declarator(base_type, child, source),
                        reference
                    )
                })
                .unwrap_or_else(|| format!("{base_type}{reference}"))
        }
        "array_declarator" | "abstract_array_declarator" => declarator
            .child_by_field_name("declarator")
            .map(|child| format!("{}[]", type_from_declarator(base_type, child, source)))
            .unwrap_or_else(|| format!("{base_type}[]")),
        "function_declarator" => child_declarator(declarator)
            .map(|child| {
                function_pointer_marker(child, source).map_or_else(
                    || type_from_declarator(base_type, child, source),
                    |marker| {
                        format!(
                            "{}({})({})",
                            base_type,
                            marker,
                            parameter_type_names(declarator, source).join(",")
                        )
                    },
                )
            })
            .unwrap_or_else(|| base_type.to_string()),
        "init_declarator" | "parenthesized_declarator" => declarator
            .child_by_field_name("declarator")
            .or_else(|| child_declarator(declarator))
            .map(|child| type_from_declarator(base_type, child, source))
            .unwrap_or_else(|| base_type.to_string()),
        _ => base_type.to_string(),
    }
}

fn reference_operator(declarator: Node, source: &[u8]) -> &'static str {
    let text = node_text(declarator, source);
    let declarator_start = declarator
        .child_by_field_name("declarator")
        .map(|child| child.start_byte())
        .unwrap_or(declarator.end_byte());
    let prefix = &text[..declarator_start.saturating_sub(declarator.start_byte())];
    if prefix.contains("&&") {
        "&&"
    } else {
        "&"
    }
}

fn function_pointer_marker(declarator: Node, source: &[u8]) -> Option<String> {
    let marker = declarator_marker(declarator, source)?;
    marker.contains('*').then_some(marker)
}

fn declarator_marker(declarator: Node, source: &[u8]) -> Option<String> {
    match declarator.kind() {
        "identifier"
        | "field_identifier"
        | "type_identifier"
        | "qualified_identifier"
        | "destructor_name"
        | "operator_name" => Some(String::new()),
        "pointer_declarator" | "abstract_pointer_declarator" => Some(format!(
            "*{}",
            child_declarator(declarator)
                .and_then(|child| declarator_marker(child, source))
                .unwrap_or_default()
        )),
        "array_declarator" | "abstract_array_declarator" => Some(format!(
            "{}[]",
            child_declarator(declarator)
                .and_then(|child| declarator_marker(child, source))
                .unwrap_or_default()
        )),
        "parenthesized_declarator" | "init_declarator" => declarator
            .child_by_field_name("declarator")
            .or_else(|| child_declarator(declarator))
            .and_then(|child| declarator_marker(child, source)),
        _ => named_children(declarator)
            .into_iter()
            .find_map(|child| declarator_marker(child, source)),
    }
}

fn parameter_type_names(function_declarator: Node, source: &[u8]) -> Vec<String> {
    function_declarator
        .child_by_field_name("parameters")
        .map(|parameters| {
            parse_parameters(parameters, source)
                .into_iter()
                .map(|parameter| parameter.type_name)
                .collect()
        })
        .unwrap_or_default()
}

fn child_declarator(node: Node) -> Option<Node> {
    node.child_by_field_name("declarator").or_else(|| {
        named_children(node)
            .into_iter()
            .find(|child| is_declarator(*child))
    })
}

fn is_declarator(node: Node) -> bool {
    matches!(
        node.kind(),
        "identifier"
            | "field_identifier"
            | "type_identifier"
            | "qualified_identifier"
            | "destructor_name"
    ) || node.kind().ends_with("_declarator")
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
        .filter(|part| !TYPE_QUALIFIERS.contains(part))
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" *", "*")
        .trim()
        .to_string();
    normalized
        .strip_prefix("struct ")
        .or_else(|| normalized.strip_prefix("union "))
        .or_else(|| normalized.strip_prefix("enum "))
        .unwrap_or(&normalized)
        .to_string()
}

const TYPE_QUALIFIERS: &[&str] = &[
    "const", "volatile", "restrict", "static", "extern", "register", "typedef",
];

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

fn function_signature(return_type: &str, params: &[ParameterDecl], is_const: bool) -> String {
    let signature = signature(return_type, params);
    if is_const {
        format!("{signature}<const>")
    } else {
        signature
    }
}

fn named_children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn named_descendants(node: Node) -> Vec<Node> {
    named_children(node)
        .into_iter()
        .flat_map(|child| {
            let mut descendants = vec![child];
            descendants.extend(named_descendants(child));
            descendants
        })
        .collect()
}

fn find_named_descendant_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    named_children(node).into_iter().find_map(|child| {
        if child.kind() == kind {
            Some(child)
        } else {
            find_named_descendant_kind(child, kind)
        }
    })
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

    fn assert_lambda_capture(
        capture: &LambdaCapture,
        name: Option<&str>,
        code: &str,
        capture_kind: &str,
        has_initializer: bool,
    ) {
        assert_eq!(capture.name.as_deref(), name);
        assert_eq!(capture.code, code);
        assert_eq!(capture.capture_kind, capture_kind);
        assert_eq!(capture.initializer.is_some(), has_initializer);
    }

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
    fn parse_file_adds_command_line_defines_as_macros() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "cxxastgen-core-defines-{}-{unique}.c",
            std::process::id()
        ));
        fs::write(&path, "int selected() { return FROM_DB; }\n").expect("write temp source");

        let document = parse_file(
            &path,
            &ParseOptions {
                include_paths: Vec::new(),
                defines: vec![
                    "FROM_DB=7".to_string(),
                    "FEATURE".to_string(),
                    "EMPTY=".to_string(),
                    "INC(x)=((x) + 1)".to_string(),
                ],
                compilation_database: None,
                skip_function_bodies: false,
                import_header_declarations: false,
            },
        )
        .expect("parse temp source");
        fs::remove_file(&path).ok();

        let [Declaration::Macro(from_db), Declaration::Macro(feature), Declaration::Macro(empty), Declaration::Macro(inc), Declaration::Function(function)] =
            document.declarations.as_slice()
        else {
            panic!("expected synthetic macros followed by the parsed function");
        };
        assert_eq!(from_db.name, "FROM_DB");
        assert_eq!(from_db.code, "#define FROM_DB 7");
        assert_eq!(from_db.body, "7");
        assert_eq!(feature.name, "FEATURE");
        assert_eq!(feature.code, "#define FEATURE 1");
        assert_eq!(feature.body, "1");
        assert_eq!(empty.name, "EMPTY");
        assert_eq!(empty.code, "#define EMPTY");
        assert_eq!(empty.body, "");
        assert_eq!(inc.name, "INC");
        assert_eq!(inc.parameters, vec!["x".to_string()]);
        assert_eq!(inc.body, "((x) + 1)");
        assert_eq!(function.name, "selected");
    }

    #[test]
    fn parse_file_imports_macros_from_included_headers() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after Unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "cxxastgen-core-includes-{}-{unique}",
            std::process::id()
        ));
        let include_dir = dir.join("include");
        fs::create_dir_all(&include_dir).expect("create include dir");
        let header = include_dir.join("feature.h");
        fs::write(&header, "#define FEATURE_VALUE 7\n").expect("write header");
        let source = dir.join("main.c");
        fs::write(
            &source,
            "#include \"feature.h\"\nint selected() { return FEATURE_VALUE; }\n",
        )
        .expect("write source");

        let document = parse_file(
            &source,
            &ParseOptions {
                include_paths: vec![normalize_path(&include_dir)],
                defines: Vec::new(),
                compilation_database: None,
                skip_function_bodies: false,
                import_header_declarations: false,
            },
        )
        .expect("parse source with include");
        fs::remove_dir_all(&dir).ok();

        let feature_macro = document
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Macro(value) if value.name == "FEATURE_VALUE" => Some(value),
                _ => None,
            })
            .expect("included macro should be imported");
        assert_eq!(feature_macro.body, "7");
        assert_eq!(feature_macro.line, 1);
        assert_eq!(feature_macro.visible_line, Some(1));
        let expected_header_path = normalize_path(&header);
        assert_eq!(
            feature_macro.source_path.as_deref(),
            Some(expected_header_path.as_str())
        );
    }

    #[test]
    fn parse_file_imports_declarations_from_included_headers_when_enabled() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after Unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "cxxastgen-core-header-decls-{}-{unique}",
            std::process::id()
        ));
        let include_dir = dir.join("include");
        fs::create_dir_all(&include_dir).expect("create include dir");
        let header = include_dir.join("snapshot_math.h");
        fs::write(
            &header,
            r#"
                struct HeaderBox { int value; };
                int header_add(int x, int y);
                int header_global;
                "#,
        )
        .expect("write header");
        let source = dir.join("main.c");
        fs::write(
            &source,
            "#include \"snapshot_math.h\"\nint use_header(struct HeaderBox box) { return header_add(box.value, header_global); }\n",
        )
        .expect("write source");

        let document = parse_file(
            &source,
            &ParseOptions {
                include_paths: vec![normalize_path(&include_dir)],
                defines: Vec::new(),
                compilation_database: Some("compile_commands.json".to_string()),
                skip_function_bodies: false,
                import_header_declarations: true,
            },
        )
        .expect("parse source with imported header declarations");
        fs::remove_dir_all(&dir).ok();

        let expected_header_path = normalize_path(&header);
        let header_struct = document
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(value) if value.name == "HeaderBox" => Some(value),
                _ => None,
            })
            .expect("included struct should be imported");
        assert_eq!(
            header_struct.source_path.as_deref(),
            Some(expected_header_path.as_str())
        );
        assert_eq!(header_struct.visible_line, Some(1));

        let header_function = document
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(value) if value.name == "header_add" => Some(value),
                _ => None,
            })
            .expect("included function prototype should be imported");
        assert_eq!(
            header_function.source_path.as_deref(),
            Some(expected_header_path.as_str())
        );
        assert!(!header_function.is_definition);

        let header_global = document
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::GlobalVariable(value) if value.name == "header_global" => Some(value),
                _ => None,
            })
            .expect("included global should be imported");
        assert_eq!(
            header_global.source_path.as_deref(),
            Some(expected_header_path.as_str())
        );
    }

    #[test]
    fn parse_file_selects_active_preprocessor_branches() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after Unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "cxxastgen-core-conditionals-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        let source = dir.join("main.c");
        fs::write(
            &source,
            r#"
                #define LOCAL
                int selected() {
                #ifdef FEATURE
                  return FEATURE_VALUE;
                #else
                  return 0;
                #endif
                }
                int local() {
                #ifndef LOCAL
                  return 0;
                #else
                  return 1;
                #endif
                }
                "#,
        )
        .expect("write conditional source");

        let document = parse_file(
            &source,
            &ParseOptions {
                include_paths: Vec::new(),
                defines: vec!["FEATURE".to_string()],
                compilation_database: None,
                skip_function_bodies: false,
                import_header_declarations: false,
            },
        )
        .expect("parse conditional source");
        fs::remove_dir_all(&dir).ok();

        let selected = document
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "selected" => Some(function),
                _ => None,
            })
            .expect("selected function should be emitted");
        let local = document
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "local" => Some(function),
                _ => None,
            })
            .expect("local function should be emitted");

        let [Statement::Return {
            expression: Some(Expression::Identifier { name, .. }),
            ..
        }] = selected.body.as_slice()
        else {
            panic!("expected selected to return the active FEATURE branch");
        };
        assert_eq!(name, "FEATURE_VALUE");

        let [Statement::Return {
            expression: Some(Expression::Literal { value, .. }),
            ..
        }] = local.body.as_slice()
        else {
            panic!("expected local to return the #else branch");
        };
        assert_eq!(value, "1");
    }

    #[test]
    fn parse_file_evaluates_macro_values_and_undefs_in_preprocessor_branches() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after Unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "cxxastgen-core-macro-values-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        fs::write(
            dir.join("feature.h"),
            "#define HEADER_VALUE 11\n#define HEADER_DROP 1\n#undef HEADER_DROP\n",
        )
        .expect("write header");
        let source = dir.join("main.c");
        fs::write(
            &source,
            r#"
                #include "feature.h"
                #define WRAPPED (7)
                #define DROP 1
                #undef DROP
                int from_header() {
                #if HEADER_VALUE == 11
                  return 11;
                #else
                  return 0;
                #endif
                }
                int header_dropped() {
                #ifdef HEADER_DROP
                  return 1;
                #else
                  return 0;
                #endif
                }
                int from_define() {
                #if FEATURE == 7
                  return 7;
                #else
                  return 0;
                #endif
                }
                int disabled() {
                #if DISABLED
                  return 1;
                #else
                  return 0;
                #endif
                }
                int wrapped() {
                #if WRAPPED == 7
                  return 7;
                #else
                  return 0;
                #endif
                }
                int dropped() {
                #ifdef DROP
                  return 1;
                #else
                  return 0;
                #endif
                }
                int statement_undef() {
                #define TEMP 1
                #undef TEMP
                #ifdef TEMP
                  return 1;
                #else
                  return 0;
                #endif
                }
                "#,
        )
        .expect("write macro value source");

        let document = parse_file(
            &source,
            &ParseOptions {
                include_paths: vec![normalize_path(&dir)],
                defines: vec!["FEATURE=7".to_string(), "DISABLED=0".to_string()],
                compilation_database: None,
                skip_function_bodies: false,
                import_header_declarations: false,
            },
        )
        .expect("parse macro value source");
        fs::remove_dir_all(&dir).ok();

        assert_eq!(function_return_literal(&document, "from_header"), "11");
        assert_eq!(function_return_literal(&document, "header_dropped"), "0");
        assert_eq!(function_return_literal(&document, "from_define"), "7");
        assert_eq!(function_return_literal(&document, "disabled"), "0");
        assert_eq!(function_return_literal(&document, "wrapped"), "7");
        assert_eq!(function_return_literal(&document, "dropped"), "0");
        assert_eq!(function_return_literal(&document, "statement_undef"), "0");
        assert!(document
            .declarations
            .iter()
            .any(|declaration| matches!(declaration, Declaration::MacroUndef(value) if value.name == "DROP")));
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
                import_header_declarations: false,
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
    fn parses_cpp_namespaces_classes_and_methods() {
        let sample = r#"
                namespace Core {
                class Widget {
                public:
                  Widget();
                  Widget(int& seed) : value(seed) {}
                  Widget(const Widget& other) : value(other.value) {}
                  ~Widget() { value = 0; }
                  int value;
                  static int instances;
                  int get() { return value; }
                  int stable() const { return value; }
                  virtual int render(int scale) { return scale; }
                  virtual int declared(int scale);
                  static int identity(int x);
                  int size() const;
                  int outside() const;
                  int operator+(const Widget& other) const { return value + other.value; }
                  Widget& operator=(const Widget& other) { value = other.value; return *this; }
                  int operator[](int index) const { return value + index; }
                };
                class Fancy : public Widget {
                public:
                  int render(int scale) override { return scale + 1; }
                  int inheritedValue() { return value + get(); }
                  int explicitThis() { return this->value + this->get(); }
                };
                class Invoker {
                public:
                  int operator()(int delta) const { return delta + 1; }
                };
                int make() { return 1; }
                }
                Core::Widget::Widget() : value(1) {}
                Core::Widget::~Widget() {}
                int Core::Widget::identity(int x) { return x; }
                int Core::Widget::outside() const { return stable(); }
                int Core::Widget::declared(int scale) { return scale; }
                int use() {
                  Core::Widget widget(7);
                  Core::Widget direct(widget);
                  Core::Widget copied = widget;
                  if (1) {
                    Core::Widget scoped(widget);
                  }
                  if (widget.get()) {
                    Core::Widget early(widget);
                    return early.get();
                  }
                  Core::Widget *ptr = &widget;
                  Core::Fancy fancy;
                  Core::Invoker invoker;
                  ptr->~Widget();
                  widget = fancy;
                  return Core::make() + widget.get() + widget.stable() + widget.outside() + widget.render(3) + widget.declared(4) + fancy.render(5) + fancy.get() + fancy.value + fancy.declared(6) + fancy.inheritedValue() + fancy.explicitThis() + (widget + fancy) + widget[2] + invoker(3);
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("sample C++ namespace and class should parse");
        let namespace = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Namespace(namespace) if namespace.name == "Core" => Some(namespace),
                _ => None,
            })
            .expect("expected Core namespace declaration");
        assert_eq!(namespace.name, "Core");

        let widget = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(struct_decl) if struct_decl.name == "Widget" => {
                    Some(struct_decl)
                }
                _ => None,
            })
            .expect("expected Widget class");
        assert_eq!(widget.fields.len(), 2);
        assert_eq!(widget.fields[0].name, "value");
        assert!(!widget.fields[0].is_static);
        assert_eq!(widget.fields[1].name, "instances");
        assert!(widget.fields[1].is_static);

        let methods = widget
            .nested_declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Function(function) => Some(function),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut method_names = methods
            .iter()
            .map(|method| method.name.as_str())
            .collect::<Vec<_>>();
        method_names.sort_unstable();
        assert_eq!(
            method_names,
            vec![
                "Widget",
                "Widget",
                "Widget",
                "declared",
                "get",
                "identity",
                "operator+",
                "operator=",
                "operator[]",
                "outside",
                "render",
                "size",
                "stable",
                "~Widget"
            ]
        );
        assert!(methods.iter().any(|method| method.name == "size"
            && method.signature == "int()<const>"
            && method.is_const
            && !method.is_definition));
        assert!(methods.iter().any(|method| method.name == "outside"
            && method.signature == "int()<const>"
            && method.is_const
            && !method.is_definition));
        assert!(methods
            .iter()
            .any(|method| method.name == "identity" && method.is_static && !method.is_definition));
        assert!(methods
            .iter()
            .any(|method| method.name == "get" && method.is_definition));
        assert!(methods.iter().any(|method| method.name == "stable"
            && method.signature == "int()<const>"
            && method.is_const
            && method.is_definition));
        assert!(methods.iter().any(|method| method.name == "render"
            && method.signature == "int(int)"
            && method.is_virtual
            && method.is_definition));
        assert!(methods.iter().any(|method| method.name == "declared"
            && method.signature == "int(int)"
            && method.is_virtual
            && !method.is_definition));
        assert!(methods.iter().any(|method| method.name == "operator+"
            && method.signature == "int(Widget&)<const>"
            && method.is_const
            && method.is_definition));
        assert!(methods.iter().any(|method| method.name == "operator="
            && method.signature == "Widget&(Widget&)"
            && !method.is_const
            && method.is_definition));
        assert!(methods.iter().any(|method| method.name == "operator[]"
            && method.signature == "int(int)<const>"
            && method.is_const
            && method.is_definition));
        assert!(methods.iter().any(|method| method.name == "Widget"
            && method.signature == "void()"
            && !method.is_definition));
        assert!(methods.iter().any(|method| method.name == "Widget"
            && method.signature == "void(int&)"
            && method.is_definition));
        assert!(methods.iter().any(|method| method.name == "Widget"
            && method.signature == "void(Widget&)"
            && method.is_definition));

        let fancy = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(struct_decl) if struct_decl.name == "Fancy" => {
                    Some(struct_decl)
                }
                _ => None,
            })
            .expect("expected Fancy class");
        assert_eq!(fancy.base_classes, vec!["Widget"]);
        assert!(fancy.nested_declarations.iter().any(|declaration| matches!(
            declaration,
            Declaration::Function(method)
                if method.name == "render" && method.signature == "int(int)" && method.is_virtual
        )));

        let invoker = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(struct_decl) if struct_decl.name == "Invoker" => {
                    Some(struct_decl)
                }
                _ => None,
            })
            .expect("expected Invoker class");
        assert!(invoker.nested_declarations.iter().any(|declaration| matches!(
            declaration,
            Declaration::Function(method)
                if method.name == "operator()" && method.signature == "int(int)<const>" && method.is_definition
        )));

        let seeded_constructor = methods
            .iter()
            .find(|method| {
                method.name == "Widget" && method.signature == "void(int&)" && method.is_definition
            })
            .expect("expected inline seeded constructor");
        assert_eq!(seeded_constructor.constructor_initializers.len(), 1);
        assert_eq!(
            seeded_constructor.constructor_initializers[0].field,
            "value"
        );
        assert_eq!(
            seeded_constructor.constructor_initializers[0].code,
            "value(seed)"
        );
        assert_eq!(
            seeded_constructor.constructor_initializers[0]
                .arguments
                .len(),
            1
        );
        match &seeded_constructor.constructor_initializers[0].arguments[0] {
            Expression::Identifier { name, .. } => assert_eq!(name, "seed"),
            other => panic!("expected seed identifier initializer argument, got {other:?}"),
        }
        assert!(methods.iter().any(|method| method.name == "~Widget"
            && method.signature == "void()"
            && method.is_definition));

        let make = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "make" => Some(function),
                _ => None,
            })
            .expect("expected namespace free function");
        assert_eq!(make.signature, "int()");

        let constructor = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "Core::Widget::Widget" => {
                    Some(function)
                }
                _ => None,
            })
            .expect("expected out-of-class constructor definition");
        assert_eq!(constructor.signature, "void()");
        assert!(constructor.is_definition);
        assert_eq!(constructor.constructor_initializers.len(), 1);
        assert_eq!(constructor.constructor_initializers[0].field, "value");
        assert_eq!(constructor.constructor_initializers[0].code, "value(1)");
        match &constructor.constructor_initializers[0].arguments[0] {
            Expression::Literal { value, .. } => assert_eq!(value, "1"),
            other => panic!("expected literal out-of-class initializer argument, got {other:?}"),
        }

        let destructor = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "Core::Widget::~Widget" => {
                    Some(function)
                }
                _ => None,
            })
            .expect("expected out-of-class destructor definition");
        assert_eq!(destructor.signature, "void()");
        assert!(destructor.is_definition);

        let identity = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "Core::Widget::identity" => {
                    Some(function)
                }
                _ => None,
            })
            .expect("expected out-of-class static method definition");
        assert_eq!(identity.signature, "int(int)");
        assert!(identity.is_definition);
        assert!(!identity.is_static);

        let outside = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "Core::Widget::outside" => {
                    Some(function)
                }
                _ => None,
            })
            .expect("expected out-of-class method definition");
        assert_eq!(outside.signature, "int()<const>");
        assert!(outside.is_const);

        let declared = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "Core::Widget::declared" => {
                    Some(function)
                }
                _ => None,
            })
            .expect("expected out-of-class virtual method definition");
        assert_eq!(declared.signature, "int(int)");
        assert!(declared.is_definition);
        assert!(!declared.is_virtual);

        let use_function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "use" => Some(function),
                _ => None,
            })
            .expect("expected use function");
        let [Statement::LocalDecl {
            type_name,
            initializer: Some(widget_initializer),
            ..
        }, Statement::LocalDecl {
            type_name: direct_type_name,
            initializer: Some(direct_initializer),
            ..
        }, Statement::LocalDecl {
            type_name: copied_type_name,
            initializer: Some(copied_initializer),
            ..
        }, Statement::If {
            then_body: scoped_then_body,
            ..
        }, Statement::If {
            then_body: early_then_body,
            ..
        }, Statement::LocalDecl {
            type_name: ptr_type_name,
            initializer: Some(ptr_initializer),
            ..
        }, Statement::LocalDecl {
            type_name: fancy_type_name,
            initializer: None,
            ..
        }, Statement::LocalDecl {
            type_name: invoker_type_name,
            initializer: None,
            ..
        }, Statement::Expression {
            expression: ptr_destructor,
            ..
        }, Statement::Assignment {
            operator,
            left,
            right,
            ..
        }, Statement::Return {
            expression: Some(return_expr),
            ..
        }] = use_function.body.as_slice()
        else {
            panic!("expected local declarations followed by return expression");
        };
        assert_eq!(type_name, "Core::Widget");
        assert_eq!(direct_type_name, "Core::Widget");
        assert_eq!(copied_type_name, "Core::Widget");
        assert_eq!(ptr_type_name, "Core::Widget*");
        assert_eq!(fancy_type_name, "Core::Fancy");
        assert_eq!(invoker_type_name, "Core::Invoker");
        assert_eq!(operator, "=");
        assert!(matches!(left, Expression::Identifier { name, .. } if name == "widget"));
        assert!(matches!(right, Expression::Identifier { name, .. } if name == "fancy"));
        let Expression::InitializerList { code, elements, .. } = widget_initializer else {
            panic!("expected constructor argument list initializer");
        };
        assert_eq!(code, "(7)");
        assert!(matches!(
            elements.as_slice(),
            [Expression::Literal { value, .. }] if value == "7"
        ));
        let Expression::InitializerList { code, elements, .. } = direct_initializer else {
            panic!("expected direct copy constructor argument list initializer");
        };
        assert_eq!(code, "(widget)");
        assert!(matches!(
            elements.as_slice(),
            [Expression::Identifier { name, .. }] if name == "widget"
        ));
        assert!(
            matches!(copied_initializer, Expression::Identifier { name, .. } if name == "widget")
        );
        assert!(matches!(
            scoped_then_body.as_slice(),
            [Statement::LocalDecl {
                name,
                type_name,
                initializer: Some(Expression::InitializerList { elements, .. }),
                ..
            }] if name == "scoped"
                && type_name == "Core::Widget"
                && matches!(elements.as_slice(), [Expression::Identifier { name, .. }] if name == "widget")
        ));
        assert!(matches!(
            early_then_body.as_slice(),
            [Statement::LocalDecl {
                name,
                type_name,
                initializer: Some(Expression::InitializerList { elements, .. }),
                ..
            }, Statement::Return {
                expression: Some(Expression::Call { name: return_name, .. }),
                ..
            }] if name == "early"
                && type_name == "Core::Widget"
                && matches!(elements.as_slice(), [Expression::Identifier { name, .. }] if name == "widget")
                && return_name == "early.get"
        ));
        assert!(matches!(
            ptr_initializer,
            Expression::Unary {
                operator,
                argument,
                ..
            } if operator == "&" && matches!(argument.as_ref(), Expression::Identifier { name, .. } if name == "widget")
        ));
        assert!(matches!(
            ptr_destructor,
            Expression::Call { name, .. } if name == "ptr->~Widget"
        ));
        let call_names = collect_call_names(return_expr);
        assert_eq!(
            call_names,
            vec![
                "Core::make",
                "widget.get",
                "widget.stable",
                "widget.outside",
                "widget.render",
                "widget.declared",
                "fancy.render",
                "fancy.get",
                "fancy.declared",
                "fancy.inheritedValue",
                "fancy.explicitThis",
                "invoker"
            ]
        );
    }

    #[test]
    fn parses_cpp_rvalue_reference_constructors() {
        let sample = r#"
                namespace Core {
                class Widget {
                public:
                  Widget();
                  Widget(const Widget& other) {}
                  Widget(Widget&& other) {}
                  ~Widget() {}
                };
                }
                Core::Widget makeWidget() {
                  Core::Widget temp;
                  return temp;
                }
                int use() {
                  Core::Widget source;
                  Core::Widget copied = source;
                  Core::Widget moved = makeWidget();
                  return 0;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("rvalue reference sample should parse");
        let namespace = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Namespace(namespace) if namespace.name == "Core" => Some(namespace),
                _ => None,
            })
            .expect("expected Core namespace declaration");
        let widget = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(struct_decl) if struct_decl.name == "Widget" => {
                    Some(struct_decl)
                }
                _ => None,
            })
            .expect("expected Widget class");
        let methods = widget
            .nested_declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Function(function) => Some(function),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(methods
            .iter()
            .any(|method| method.name == "Widget" && method.signature == "void(Widget&)"));
        assert!(methods
            .iter()
            .any(|method| method.name == "Widget" && method.signature == "void(Widget&&)"));

        let make_widget = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "makeWidget" => Some(function),
                _ => None,
            })
            .expect("expected makeWidget function");
        assert_eq!(make_widget.return_type, "Core::Widget");

        let use_function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "use" => Some(function),
                _ => None,
            })
            .expect("expected use function");
        let [Statement::LocalDecl {
            name: source_name,
            type_name: source_type,
            initializer: None,
            ..
        }, Statement::LocalDecl {
            name: copied_name,
            type_name: copied_type,
            initializer:
                Some(Expression::Identifier {
                    name: copied_initializer,
                    ..
                }),
            ..
        }, Statement::LocalDecl {
            name: moved_name,
            type_name: moved_type,
            initializer:
                Some(Expression::Call {
                    name: moved_initializer,
                    ..
                }),
            ..
        }, Statement::Return { .. }] = use_function.body.as_slice()
        else {
            panic!("expected source, copied, moved locals followed by return");
        };
        assert_eq!(source_name, "source");
        assert_eq!(source_type, "Core::Widget");
        assert_eq!(copied_name, "copied");
        assert_eq!(copied_type, "Core::Widget");
        assert_eq!(copied_initializer, "source");
        assert_eq!(moved_name, "moved");
        assert_eq!(moved_type, "Core::Widget");
        assert_eq!(moved_initializer, "makeWidget");
    }

    #[test]
    fn parses_cpp_template_declarations() {
        let sample = r#"
                namespace Core {
                template <typename T>
                T pick(T value) { return value; }
                template <typename T>
                struct Holder {
                  T value;
                  T get() { return value; }
                };
                }
                int use(Core::Holder<int> holder) {
                  return holder.get() + Core::pick(1);
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("template declaration sample should parse");
        let namespace = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Namespace(namespace) if namespace.name == "Core" => Some(namespace),
                _ => None,
            })
            .expect("expected Core namespace declaration");
        let pick = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "pick" => Some(function),
                _ => None,
            })
            .expect("expected templated pick function");
        assert_eq!(pick.signature, "T(T)");
        assert!(pick.is_definition);
        let holder = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(struct_decl) if struct_decl.name == "Holder" => {
                    Some(struct_decl)
                }
                _ => None,
            })
            .expect("expected templated Holder class");
        assert_eq!(holder.fields[0].type_name, "T");
        assert!(holder
            .nested_declarations
            .iter()
            .any(|declaration| matches!(
                declaration,
                Declaration::Function(method)
                    if method.name == "get" && method.signature == "T()" && method.is_definition
            )));
        let use_function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "use" => Some(function),
                _ => None,
            })
            .expect("expected use function");
        assert_eq!(use_function.parameters[0].type_name, "Core::Holder<int>");
    }

    #[test]
    fn parses_cpp_lambda_expressions() {
        let sample = r#"
                int use(int base) {
                  auto mapper = [base](int x) { return base + x; };
                  return mapper(2);
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("lambda expression sample should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "use" => Some(function),
                _ => None,
            })
            .expect("expected use function");
        let [Statement::LocalDecl {
            name,
            type_name,
            initializer: Some(lambda),
            ..
        }, Statement::Return {
            expression: Some(call),
            ..
        }] = function.body.as_slice()
        else {
            panic!("expected lambda local followed by return call");
        };
        assert_eq!(name, "mapper");
        assert_eq!(type_name, "auto");
        let Expression::Lambda {
            captures,
            parameters,
            return_type,
            signature,
            body,
            ..
        } = lambda
        else {
            panic!("expected lambda initializer");
        };
        assert_eq!(captures.len(), 1);
        assert_lambda_capture(&captures[0], Some("base"), "base", "explicitByValue", false);
        assert_eq!(parameters.len(), 1);
        assert_eq!(parameters[0].name, "x");
        assert_eq!(parameters[0].type_name, "int");
        assert_eq!(return_type, "int");
        assert_eq!(signature, "int(int)");
        assert!(matches!(
            body.as_slice(),
            [Statement::Return {
                expression: Some(Expression::Binary { operator, .. }),
                ..
            }] if operator == "+"
        ));
        assert!(matches!(
            call,
            Expression::Call { name, arguments, .. }
                if name == "mapper" && matches!(
                    arguments.as_slice(),
                    [Expression::Literal { value, .. }] if value == "2"
                )
        ));
    }

    #[test]
    fn parses_cpp_lambda_capture_kinds() {
        let sample = r#"
                struct Widget {
                  int value;
                  int use(int base) {
                    int delta = 1;
                    auto by_ref = [&](int x) { return base + delta + x; };
                    auto mixed = [=, &delta, this](int x) { return this->value + base + delta + x; };
                    auto copy = [snap = base + delta](int x) { return snap + x; };
                    auto copied_this = [*this]() { return value; };
                    return by_ref(1) + mixed(2) + copy(3) + copied_this();
                  }
                };
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("lambda capture-kind sample should parse");
        let method = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(struct_decl) if struct_decl.name == "Widget" => struct_decl
                    .nested_declarations
                    .iter()
                    .find_map(|nested| match nested {
                        Declaration::Function(function) if function.name == "use" => Some(function),
                        _ => None,
                    }),
                _ => None,
            })
            .expect("expected use method");
        let captures: Vec<Vec<LambdaCapture>> = method
            .body
            .iter()
            .filter_map(|statement| match statement {
                Statement::LocalDecl {
                    initializer: Some(Expression::Lambda { captures, .. }),
                    ..
                } => Some(captures.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(captures.len(), 4);
        assert_eq!(captures[0].len(), 1);
        assert_lambda_capture(&captures[0][0], None, "&", "defaultByReference", false);
        assert_eq!(captures[1].len(), 3);
        assert_lambda_capture(&captures[1][0], None, "=", "defaultByValue", false);
        assert_lambda_capture(
            &captures[1][1],
            Some("delta"),
            "&delta",
            "explicitByReference",
            false,
        );
        assert_lambda_capture(&captures[1][2], Some("this"), "this", "this", false);
        assert_eq!(captures[2].len(), 1);
        assert_lambda_capture(
            &captures[2][0],
            Some("snap"),
            "snap = base + delta",
            "initByValue",
            true,
        );
        assert!(matches!(
            captures[2][0].initializer.as_deref(),
            Some(Expression::Binary { operator, .. }) if operator == "+"
        ));
        assert_eq!(captures[3].len(), 1);
        assert_lambda_capture(&captures[3][0], Some("this"), "*this", "copyThis", false);
    }

    #[test]
    fn parses_cpp_generic_and_nested_lambdas() {
        let sample = r#"
                int use(int base) {
                  auto identity = [](auto value) { return value; };
                  auto outer = [base] {
                    auto inner = [](int x) { return x; };
                    return base;
                  };
                  auto typed = []() -> long { return 1; };
                  return identity(outer());
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("generic and nested lambda sample should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "use" => Some(function),
                _ => None,
            })
            .expect("expected use function");

        let [Statement::LocalDecl {
            initializer: Some(identity),
            ..
        }, Statement::LocalDecl {
            initializer: Some(outer),
            ..
        }, ..] = function.body.as_slice()
        else {
            panic!("expected identity and outer lambda locals");
        };

        let Expression::Lambda {
            parameters,
            return_type,
            signature,
            ..
        } = identity
        else {
            panic!("expected generic identity lambda");
        };
        assert_eq!(parameters.len(), 1);
        assert_eq!(parameters[0].name, "value");
        assert_eq!(parameters[0].type_name, "auto");
        assert_eq!(return_type, "auto");
        assert_eq!(signature, "auto(auto)");

        let Expression::Lambda {
            parameters, body, ..
        } = outer
        else {
            panic!("expected outer lambda");
        };
        assert!(
            parameters.is_empty(),
            "outer lambda must not inherit inner lambda parameters"
        );
        let nested = body.iter().find_map(|statement| match statement {
            Statement::LocalDecl {
                initializer: Some(Expression::Lambda { parameters, .. }),
                ..
            } => Some(parameters),
            _ => None,
        });
        assert!(
            matches!(nested, Some(parameters) if parameters.len() == 1 && parameters[0].name == "x")
        );
        let typed = function.body.iter().find_map(|statement| match statement {
            Statement::LocalDecl {
                name,
                initializer:
                    Some(Expression::Lambda {
                        return_type,
                        signature,
                        ..
                    }),
                ..
            } if name == "typed" => Some((return_type, signature)),
            _ => None,
        });
        assert!(
            matches!(typed, Some((return_type, signature)) if return_type == "long" && signature == "long()")
        );
    }

    #[test]
    fn parses_cpp_mutable_lambdas() {
        let sample = r#"
                int use(int seed) {
                  auto bump = [seed](int step) mutable -> int {
                    seed += step;
                    return seed;
                  };
                  auto read = [seed]() { return seed; };
                  return bump(1) + read();
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("mutable lambda sample should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "use" => Some(function),
                _ => None,
            })
            .expect("expected use function");
        let lambdas: Vec<(&str, &Expression)> = function
            .body
            .iter()
            .filter_map(|statement| match statement {
                Statement::LocalDecl {
                    name,
                    initializer: Some(lambda @ Expression::Lambda { .. }),
                    ..
                } => Some((name.as_str(), lambda)),
                _ => None,
            })
            .collect();
        assert_eq!(lambdas.len(), 2);
        let Expression::Lambda {
            is_mutable,
            return_type,
            signature,
            body,
            ..
        } = lambdas[0].1
        else {
            panic!("expected mutable bump lambda");
        };
        assert_eq!(lambdas[0].0, "bump");
        assert!(*is_mutable);
        assert_eq!(return_type, "int");
        assert_eq!(signature, "int(int)");
        assert!(matches!(
            body.as_slice(),
            [Statement::Assignment { operator, .. }, Statement::Return { .. }] if operator == "+="
        ));
        let Expression::Lambda { is_mutable, .. } = lambdas[1].1 else {
            panic!("expected read lambda");
        };
        assert_eq!(lambdas[1].0, "read");
        assert!(!*is_mutable);
    }

    #[test]
    fn parses_cpp_lambda_assigned_to_explicit_function_object_type() {
        let sample = r#"
                int use(int base) {
                  std::function<int(int)> mapper = [base](int x) -> int { return base + x; };
                  auto caller = [mapper](int y) -> int { return mapper(y); };
                  return mapper(2) + caller(3);
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("explicit function-object lambda sample should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "use" => Some(function),
                _ => None,
            })
            .expect("expected use function");
        let [Statement::LocalDecl {
            name: mapper_name,
            type_name: mapper_type,
            initializer: Some(mapper),
            ..
        }, Statement::LocalDecl {
            name: caller_name,
            type_name: caller_type,
            initializer: Some(caller),
            ..
        }, ..] = function.body.as_slice()
        else {
            panic!("expected mapper and caller locals");
        };
        assert_eq!(mapper_name, "mapper");
        assert_eq!(mapper_type, "std::function<int(int)>");
        let Expression::Lambda {
            signature,
            return_type,
            ..
        } = mapper
        else {
            panic!("expected mapper lambda initializer");
        };
        assert_eq!(signature, "int(int)");
        assert_eq!(return_type, "int");
        assert_eq!(caller_name, "caller");
        assert_eq!(caller_type, "auto");
        let Expression::Lambda { captures, body, .. } = caller else {
            panic!("expected caller lambda initializer");
        };
        assert_eq!(captures.len(), 1);
        assert_lambda_capture(
            &captures[0],
            Some("mapper"),
            "mapper",
            "explicitByValue",
            false,
        );
        assert!(matches!(
            body.as_slice(),
            [Statement::Return {
                expression: Some(Expression::Call { name, .. }),
                ..
            }] if name == "mapper"
        ));
    }

    #[test]
    fn parses_cpp_constructor_temporaries() {
        let sample = r#"
                namespace Core {
                class Widget {
                public:
                  Widget();
                  Widget(const Widget& other) {}
                  Widget(Widget&& other) {}
                  ~Widget() {}
                };
                void accept(Widget&& widget) {}
                int consume(Widget&& widget) { return 1; }
                }
                int use() {
                  Core::Widget source;
                  Core::accept(Core::Widget());
                  Core::accept(Core::Widget(source));
                  Core::Widget local = Core::Widget();
                  int result = Core::consume(Core::Widget(source));
                  return Core::consume(Core::Widget(local)) + result;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("constructor temporary sample should parse");
        let namespace = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Namespace(namespace) if namespace.name == "Core" => Some(namespace),
                _ => None,
            })
            .expect("expected Core namespace declaration");
        let accept = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "accept" => Some(function),
                _ => None,
            })
            .expect("expected accept function");
        assert_eq!(accept.signature, "void(Widget&&)");
        let consume = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "consume" => Some(function),
                _ => None,
            })
            .expect("expected consume function");
        assert_eq!(consume.signature, "int(Widget&&)");

        let use_function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "use" => Some(function),
                _ => None,
            })
            .expect("expected use function");
        let [Statement::LocalDecl {
            name: source_name,
            type_name: source_type,
            initializer: source_initializer,
            ..
        }, Statement::Expression {
            expression: default_call,
            ..
        }, Statement::Expression {
            expression: copy_call,
            ..
        }, Statement::LocalDecl {
            name: local_name,
            type_name: local_type,
            initializer:
                Some(Expression::Call {
                    name: local_initializer,
                    ..
                }),
            ..
        }, Statement::LocalDecl {
            name: result_name,
            type_name: result_type,
            initializer: Some(result_initializer),
            ..
        }, Statement::Return {
            expression: Some(return_expression),
            ..
        }] = use_function.body.as_slice()
        else {
            panic!("expected source local, constructor temporaries, consume local, and return");
        };
        assert_eq!(source_name, "source");
        assert_eq!(source_type, "Core::Widget");
        assert!(source_initializer.is_none());

        let Expression::Call {
            name: default_accept_name,
            arguments: default_arguments,
            ..
        } = default_call
        else {
            panic!("expected default temporary accept call");
        };
        assert_eq!(default_accept_name, "Core::accept");
        assert!(matches!(
            default_arguments.as_slice(),
            [Expression::Call { name, arguments, .. }] if name == "Core::Widget" && arguments.is_empty()
        ));

        let Expression::Call {
            name: copy_accept_name,
            arguments: copy_arguments,
            ..
        } = copy_call
        else {
            panic!("expected copy temporary accept call");
        };
        assert_eq!(copy_accept_name, "Core::accept");
        assert!(matches!(
            copy_arguments.as_slice(),
            [Expression::Call { name, arguments, .. }]
                if name == "Core::Widget"
                    && matches!(arguments.as_slice(), [Expression::Identifier { name, .. }] if name == "source")
        ));
        assert_eq!(local_name, "local");
        assert_eq!(local_type, "Core::Widget");
        assert_eq!(local_initializer, "Core::Widget");
        assert_eq!(result_name, "result");
        assert_eq!(result_type, "int");
        assert!(matches!(
            result_initializer,
            Expression::Call { name, arguments, .. }
                if name == "Core::consume"
                    && matches!(
                        arguments.as_slice(),
                        [Expression::Call { name, arguments, .. }]
                            if name == "Core::Widget"
                                && matches!(arguments.as_slice(), [Expression::Identifier { name, .. }] if name == "source")
                    )
        ));
        let Expression::Binary { operator, left, .. } = return_expression else {
            panic!("expected binary return expression");
        };
        assert_eq!(operator, "+");
        assert!(matches!(
            left.as_ref(),
            Expression::Call { name, arguments, .. }
                if name == "Core::consume"
                    && matches!(
                        arguments.as_slice(),
                        [Expression::Call { name, arguments, .. }]
                            if name == "Core::Widget"
                                && matches!(arguments.as_slice(), [Expression::Identifier { name, .. }] if name == "local")
                    )
        ));
    }

    #[test]
    fn parses_cpp_braced_local_initializers() {
        let sample = r#"
                namespace Core {
                class Widget {
                public:
                  Widget();
                  Widget(int seed) {}
                  ~Widget() {}
                };
                }
                int use(int seed) {
                  Core::Widget empty{};
                  Core::Widget direct{seed};
                  Core::Widget assigned = {seed};
                  return 0;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("braced local initializer sample should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "use" => Some(function),
                _ => None,
            })
            .expect("expected use function");
        let [Statement::LocalDecl {
            name: empty_name,
            initializer: Some(empty_initializer),
            ..
        }, Statement::LocalDecl {
            name: direct_name,
            initializer: Some(direct_initializer),
            ..
        }, Statement::LocalDecl {
            name: assigned_name,
            initializer: Some(assigned_initializer),
            ..
        }, Statement::Return { .. }] = function.body.as_slice()
        else {
            panic!("expected braced local declarations and return");
        };
        assert_eq!(empty_name, "empty");
        assert_eq!(direct_name, "direct");
        assert_eq!(assigned_name, "assigned");
        let Expression::InitializerList {
            code: empty_code,
            elements: empty_elements,
            ..
        } = empty_initializer
        else {
            panic!("expected empty braced initializer list");
        };
        assert_eq!(empty_code, "{}");
        assert!(empty_elements.is_empty());
        let Expression::InitializerList {
            code: direct_code,
            elements: direct_elements,
            ..
        } = direct_initializer
        else {
            panic!("expected direct braced initializer list");
        };
        assert_eq!(direct_code, "{seed}");
        assert!(matches!(
            direct_elements.as_slice(),
            [Expression::Identifier { name, .. }] if name == "seed"
        ));
        let Expression::InitializerList {
            code: assigned_code,
            elements: assigned_elements,
            ..
        } = assigned_initializer
        else {
            panic!("expected assigned braced initializer list");
        };
        assert_eq!(assigned_code, "{seed}");
        assert!(matches!(
            assigned_elements.as_slice(),
            [Expression::Identifier { name, .. }] if name == "seed"
        ));
    }

    #[test]
    fn parses_cpp_braced_constructor_temporaries() {
        let sample = r#"
                namespace Core {
                class Widget {
                public:
                  Widget();
                  Widget(const Widget& other) {}
                  Widget(Widget&& other) {}
                  ~Widget() {}
                };
                void accept(Widget&& widget) {}
                }
                int use() {
                  Core::Widget source;
                  Core::accept(Core::Widget{});
                  Core::accept(Core::Widget{source});
                  return 0;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("braced constructor temporary sample should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "use" => Some(function),
                _ => None,
            })
            .expect("expected use function");
        let [Statement::LocalDecl {
            name: source_name, ..
        }, Statement::Expression {
            expression: empty_accept,
            ..
        }, Statement::Expression {
            expression: copy_accept,
            ..
        }, Statement::Return { .. }] = function.body.as_slice()
        else {
            panic!("expected source local, braced temporary calls, and return");
        };
        assert_eq!(source_name, "source");
        assert_eq!(
            collect_call_names(empty_accept),
            vec!["Core::accept", "Core::Widget"]
        );
        assert_eq!(
            collect_call_names(copy_accept),
            vec!["Core::accept", "Core::Widget"]
        );
        let Expression::Call {
            arguments: empty_arguments,
            ..
        } = empty_accept
        else {
            panic!("expected empty accept call");
        };
        assert!(matches!(
            empty_arguments.as_slice(),
            [Expression::Call { name, arguments, .. }] if name == "Core::Widget" && arguments.is_empty()
        ));
        let Expression::Call {
            arguments: copy_arguments,
            ..
        } = copy_accept
        else {
            panic!("expected copy accept call");
        };
        assert!(matches!(
            copy_arguments.as_slice(),
            [Expression::Call { name, arguments, .. }]
                if name == "Core::Widget"
                    && matches!(arguments.as_slice(), [Expression::Identifier { name, .. }] if name == "source")
        ));
    }

    #[test]
    fn parses_cpp_constructor_temporaries_in_control_flow() {
        let sample = r#"
                namespace Core {
                class Widget {
                public:
                  Widget();
                  Widget(const Widget& other) {}
                  Widget(Widget&& other) {}
                  ~Widget() {}
                };
                int consume(Widget&& widget) { return 1; }
                }
                int flow(int n) {
                  Core::Widget source;
                  if (Core::consume(Core::Widget())) {
                    n = n + 1;
                  }
                  while (Core::consume(Core::Widget(source))) {
                    break;
                  }
                  for (; Core::consume(Core::Widget()); Core::consume(Core::Widget(source))) {
                    break;
                  }
                  switch (Core::consume(Core::Widget(source))) {
                  default:
                    break;
                  }
                  return n;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("control-flow constructor temporary sample should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "flow" => Some(function),
                _ => None,
            })
            .expect("expected flow function");
        let [Statement::LocalDecl {
            name: source_name, ..
        }, Statement::If {
            condition: if_condition,
            ..
        }, Statement::While {
            condition: while_condition,
            ..
        }, Statement::For {
            condition: Some(for_condition),
            update: Some(for_update),
            ..
        }, Statement::Switch {
            condition: switch_condition,
            ..
        }, Statement::Return { .. }] = function.body.as_slice()
        else {
            panic!("expected source local, if, while, for, switch, and return");
        };
        assert_eq!(source_name, "source");
        assert_eq!(
            collect_call_names(if_condition),
            vec!["Core::consume", "Core::Widget"]
        );
        assert_eq!(
            collect_call_names(while_condition),
            vec!["Core::consume", "Core::Widget"]
        );
        assert_eq!(
            collect_call_names(for_condition),
            vec!["Core::consume", "Core::Widget"]
        );
        assert_eq!(
            collect_call_names(for_update),
            vec!["Core::consume", "Core::Widget"]
        );
        assert_eq!(
            collect_call_names(switch_condition),
            vec!["Core::consume", "Core::Widget"]
        );
    }

    #[test]
    fn parses_cpp_constructor_temporaries_in_logical_and_conditional_expressions() {
        let sample = r#"
                namespace Core {
                class Widget {
                public:
                  Widget();
                  Widget(const Widget& other) {}
                  Widget(Widget&& other) {}
                  ~Widget() {}
                };
                int consume(Widget&& widget) { return 1; }
                }
                int mix(int n) {
                  Core::Widget source;
                  int both = Core::consume(Core::Widget()) && Core::consume(Core::Widget(source));
                  int either = Core::consume(Core::Widget(source)) || Core::consume(Core::Widget());
                  int selected = n ? Core::consume(Core::Widget()) : Core::consume(Core::Widget(source));
                  return both + either + selected;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("logical and conditional constructor temporary sample should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "mix" => Some(function),
                _ => None,
            })
            .expect("expected mix function");
        let [Statement::LocalDecl {
            name: source_name, ..
        }, Statement::LocalDecl {
            name: both_name,
            initializer: Some(both_initializer),
            ..
        }, Statement::LocalDecl {
            name: either_name,
            initializer: Some(either_initializer),
            ..
        }, Statement::LocalDecl {
            name: selected_name,
            initializer: Some(selected_initializer),
            ..
        }, Statement::Return { .. }] = function.body.as_slice()
        else {
            panic!("expected source local, logical locals, conditional local, and return");
        };
        assert_eq!(source_name, "source");
        assert_eq!(both_name, "both");
        assert_eq!(either_name, "either");
        assert_eq!(selected_name, "selected");
        assert_binary_operator(both_initializer, "&&");
        assert_binary_operator(either_initializer, "||");
        assert!(matches!(
            selected_initializer,
            Expression::Conditional { .. }
        ));
        assert_eq!(
            collect_call_names(both_initializer),
            vec![
                "Core::consume",
                "Core::Widget",
                "Core::consume",
                "Core::Widget"
            ]
        );
        assert_eq!(
            collect_call_names(either_initializer),
            vec![
                "Core::consume",
                "Core::Widget",
                "Core::consume",
                "Core::Widget"
            ]
        );
        assert_eq!(
            collect_call_names(selected_initializer),
            vec![
                "Core::consume",
                "Core::Widget",
                "Core::consume",
                "Core::Widget"
            ]
        );
    }

    #[test]
    fn parses_cpp_throw_statements_with_constructor_temporaries() {
        let sample = r#"
                namespace Core {
                class Widget {
                public:
                  Widget();
                  Widget(const Widget& other) {}
                  Widget(Widget&& other) {}
                  ~Widget() {}
                };
                int consume(Widget&& widget) { return 1; }
                }
                int fail(int n) {
                  Core::Widget source;
                  if (n) {
                    throw Core::consume(Core::Widget(source));
                  }
                  throw Core::consume(Core::Widget());
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("throw constructor temporary sample should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "fail" => Some(function),
                _ => None,
            })
            .expect("expected fail function");
        let [Statement::LocalDecl {
            name: source_name, ..
        }, Statement::If { then_body, .. }, Statement::Throw {
            expression: Some(top_level_throw),
            ..
        }] = function.body.as_slice()
        else {
            panic!("expected source local, guarded throw, and top-level throw");
        };
        assert_eq!(source_name, "source");
        let [Statement::Throw {
            expression: Some(guarded_throw),
            ..
        }] = then_body.as_slice()
        else {
            panic!("expected guarded throw body");
        };
        assert_eq!(
            collect_call_names(guarded_throw),
            vec!["Core::consume", "Core::Widget"]
        );
        assert_eq!(
            collect_call_names(top_level_throw),
            vec!["Core::consume", "Core::Widget"]
        );
    }

    #[test]
    fn parses_cpp_try_catch_statements() {
        let sample = r#"
                namespace Core {
                class Widget {
                public:
                  Widget();
                  ~Widget() {}
                };
                void handle(Widget& widget) {}
                }
                void guarded(int n) {
                  try {
                    Core::Widget local;
                    throw n;
                  } catch (Core::Widget caught) {
                    Core::handle(caught);
                  } catch (...) {
                    n = 0;
                  }
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("try-catch statement sample should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "guarded" => Some(function),
                _ => None,
            })
            .expect("expected guarded function");
        let [Statement::Try { body, catches, .. }] = function.body.as_slice() else {
            panic!("expected top-level try statement");
        };
        let [Statement::LocalDecl {
            name: local_name, ..
        }, Statement::Throw { .. }] = body.as_slice()
        else {
            panic!("expected try body local and throw");
        };
        assert_eq!(local_name, "local");
        let [typed_catch, catch_all] = catches.as_slice() else {
            panic!("expected typed catch and catch-all");
        };
        let parameter = typed_catch
            .parameter
            .as_ref()
            .expect("expected typed catch parameter");
        assert_eq!(parameter.name, "caught");
        assert_eq!(parameter.type_name, "Core::Widget");
        assert!(matches!(
            typed_catch.body.as_slice(),
            [Statement::Expression {
                expression: Expression::Call { name, .. },
                ..
            }] if name == "Core::handle"
        ));
        assert!(catch_all.parameter.is_none());
        assert!(matches!(
            catch_all.body.as_slice(),
            [Statement::Assignment { code, .. }] if code == "n = 0"
        ));
    }

    #[test]
    fn parses_cpp_new_and_delete_expressions() {
        let sample = r#"
                int *allocate(int n) {
                  int *arr = new int[n];
                  delete[] arr;
                  return arr;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("sample C++ new/delete expressions should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "allocate" => Some(function),
                _ => None,
            })
            .expect("expected allocate function");
        let [Statement::LocalDecl {
            type_name,
            initializer: Some(new_expr),
            ..
        }, Statement::Expression {
            expression: delete_expr,
            ..
        }, Statement::Return {
            expression:
                Some(Expression::Identifier {
                    name: return_name, ..
                }),
            ..
        }] = function.body.as_slice()
        else {
            panic!("expected local new, delete statement, and return");
        };
        assert_eq!(type_name, "int*");
        let Expression::New {
            type_name,
            arguments,
            ..
        } = new_expr
        else {
            panic!("expected new expression initializer");
        };
        assert_eq!(type_name, "int");
        assert!(matches!(
            arguments.as_slice(),
            [Expression::Identifier { name, .. }] if name == "n"
        ));
        let Expression::Delete { code, argument, .. } = delete_expr else {
            panic!("expected delete expression statement");
        };
        assert_eq!(code, "delete[] arr");
        assert!(matches!(argument.as_ref(), Expression::Identifier { name, .. } if name == "arr"));
        assert_eq!(return_name, "arr");
    }

    #[test]
    fn parses_cpp_heap_constructor_initializers() {
        let sample = r#"
                Core::Widget *build(Core::Widget &source) {
                  Core::Widget *one = new Core::Widget();
                  Core::Widget *two = new Core::Widget{source};
                  delete one;
                  return two;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("sample C++ heap constructor expressions should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "build" => Some(function),
                _ => None,
            })
            .expect("expected build function");
        let [Statement::LocalDecl {
            initializer: Some(default_new),
            ..
        }, Statement::LocalDecl {
            initializer: Some(braced_new),
            ..
        }, Statement::Expression {
            expression: delete_expr,
            ..
        }, Statement::Return { .. }] = function.body.as_slice()
        else {
            panic!("expected two heap locals, delete statement, and return");
        };
        let Expression::New {
            type_name,
            arguments,
            initializer_arguments,
            ..
        } = default_new
        else {
            panic!("expected default heap constructor new expression");
        };
        assert_eq!(type_name, "Core::Widget");
        assert!(arguments.is_empty());
        assert!(initializer_arguments.is_empty());

        let Expression::New {
            type_name,
            arguments,
            initializer_arguments,
            ..
        } = braced_new
        else {
            panic!("expected braced heap constructor new expression");
        };
        assert_eq!(type_name, "Core::Widget");
        assert!(matches!(
            arguments.as_slice(),
            [Expression::Identifier { name, .. }] if name == "source"
        ));
        assert!(matches!(
            initializer_arguments.as_slice(),
            [Expression::Identifier { name, .. }] if name == "source"
        ));
        assert!(
            matches!(delete_expr, Expression::Delete { argument, .. } if matches!(
                argument.as_ref(),
                Expression::Identifier { name, .. } if name == "one"
            ))
        );
    }

    #[test]
    fn parses_include_directives() {
        let sample = r#"
                #include "./folder/sub/foo.h"
                #include <io.h>
                int value;
                "#;
        let declarations =
            parse_declarations(sample, SourceLanguage::C).expect("include sample should parse");

        let [Declaration::Include(foo), Declaration::Include(io), Declaration::GlobalVariable(global)] =
            declarations.as_slice()
        else {
            panic!("expected two includes and one global");
        };
        assert_eq!(foo.name, "./folder/sub/foo.h");
        assert_eq!(foo.code, "#include \"./folder/sub/foo.h\"");
        assert_eq!(io.name, "io.h");
        assert_eq!(io.code, "#include <io.h>");
        assert_eq!(global.name, "value");
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
    fn parses_typedef_declarations() {
        let sample = r#"
                typedef const char * foo;
                typedef foo * bar;
                using baz = bar;
                using qux = const char *;
                "#;
        let declarations =
            parse_declarations(sample, SourceLanguage::Cpp).expect("typedef sample should parse");

        let Declaration::Typedef(foo) = &declarations[0] else {
            panic!("expected first typedef");
        };
        assert_eq!(foo.name, "foo");
        assert_eq!(foo.type_name, "char*");
        assert_eq!(foo.code, "typedef const char * foo;");

        let Declaration::Typedef(bar) = &declarations[1] else {
            panic!("expected second typedef");
        };
        assert_eq!(bar.name, "bar");
        assert_eq!(bar.type_name, "foo*");
        assert_eq!(bar.code, "typedef foo * bar;");

        let Declaration::Typedef(baz) = &declarations[2] else {
            panic!("expected using alias typedef");
        };
        assert_eq!(baz.name, "baz");
        assert_eq!(baz.type_name, "bar");
        assert_eq!(baz.code, "using baz = bar;");

        let Declaration::Typedef(qux) = &declarations[3] else {
            panic!("expected pointer using alias typedef");
        };
        assert_eq!(qux.name, "qux");
        assert_eq!(qux.type_name, "char*");
        assert_eq!(qux.code, "using qux = const char *;");
    }

    #[test]
    fn parses_typedef_aggregate_declarations() {
        let sample = r#"
                typedef struct foo {
                  int x;
                } abc;
                typedef struct {
                  int y;
                } Anon;
                typedef enum mode {
                  MODE_A,
                } Mode;
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::C)
            .expect("aggregate typedef sample should parse");

        let named_struct = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(value) if value.name == "foo" => Some(value),
                _ => None,
            })
            .expect("named struct should be emitted");
        assert_eq!(named_struct.fields[0].name, "x");

        let struct_typedef = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Typedef(value) if value.name == "abc" => Some(value),
                _ => None,
            })
            .expect("named struct typedef should be emitted");
        assert_eq!(struct_typedef.type_name, "foo");

        let anonymous_struct = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(value) if value.name == "Anon" => Some(value),
                _ => None,
            })
            .expect("anonymous struct typedef should become named struct");
        assert_eq!(anonymous_struct.fields[0].name, "y");

        let named_enum = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Enum(value) if value.name == "mode" => Some(value),
                _ => None,
            })
            .expect("named enum should be emitted");
        assert_eq!(named_enum.variants[0].name, "MODE_A");
        assert_eq!(named_enum.variants[0].line, 9);

        let enum_typedef = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Typedef(value) if value.name == "Mode" => Some(value),
                _ => None,
            })
            .expect("named enum typedef should be emitted");
        assert_eq!(enum_typedef.type_name, "mode");
    }

    #[test]
    fn parses_initializer_lists_and_designated_initializers() {
        let sample = r#"
                int global[] = {0, 1};
                struct Fs { int open; };
                int init(void) {
                  int local[2] = {2, 3};
                  struct Fs fs = { .open = 7 };
                  int ranged[10] = { [3 ... 9] = 15 };
                  return local[1] + fs.open + ranged[3] + global[0];
                }
                "#;
        let declarations =
            parse_declarations(sample, SourceLanguage::C).expect("initializer sample should parse");

        let global = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::GlobalVariable(value) if value.name == "global" => Some(value),
                _ => None,
            })
            .expect("global initializer should be emitted");
        let Expression::InitializerList { code, elements, .. } =
            global.initializer.as_ref().expect("global initializer")
        else {
            panic!("expected global initializer list");
        };
        assert_eq!(code, "{0, 1}");
        assert!(matches!(
            elements.as_slice(),
            [
                Expression::Literal { value: first, .. },
                Expression::Literal { value: second, .. }
            ] if first == "0" && second == "1"
        ));

        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(value) if value.name == "init" => Some(value),
                _ => None,
            })
            .expect("function should be emitted");

        let fs_initializer = function
            .body
            .iter()
            .find_map(|statement| match statement {
                Statement::LocalDecl {
                    name,
                    initializer: Some(initializer),
                    ..
                } if name == "fs" => Some(initializer),
                _ => None,
            })
            .expect("struct initializer should be emitted");
        let Expression::InitializerList { elements, .. } = fs_initializer else {
            panic!("expected struct initializer list");
        };
        let [Expression::DesignatedInitializer {
            code,
            designator,
            value,
            ..
        }] = elements.as_slice()
        else {
            panic!("expected field designated initializer");
        };
        assert_eq!(code, ".open = 7");
        assert!(matches!(
            designator.as_ref(),
            Expression::Designator { name, code, .. } if name == "open" && code == "open"
        ));
        assert!(matches!(
            value.as_ref(),
            Expression::Literal { value, .. } if value == "7"
        ));

        let ranged_initializer = function
            .body
            .iter()
            .find_map(|statement| match statement {
                Statement::LocalDecl {
                    name,
                    initializer: Some(initializer),
                    ..
                } if name == "ranged" => Some(initializer),
                _ => None,
            })
            .expect("range initializer should be emitted");
        let Expression::InitializerList { elements, .. } = ranged_initializer else {
            panic!("expected range initializer list");
        };
        let [Expression::DesignatedInitializer {
            code, designator, ..
        }] = elements.as_slice()
        else {
            panic!("expected range designated initializer");
        };
        assert_eq!(code, "[3 ... 9] = 15");
        let Expression::InitializerList {
            code: designator_code,
            elements: range_bounds,
            ..
        } = designator.as_ref()
        else {
            panic!("expected range designator");
        };
        assert_eq!(designator_code, "[3 ... 9]");
        assert!(matches!(
            range_bounds.as_slice(),
            [
                Expression::Literal { value: start, .. },
                Expression::Literal { value: end, .. }
            ] if start == "3" && end == "9"
        ));
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
        assert!(matches!(
            argument.as_ref(),
            Expression::IndexAccess {
                index, ..
            } if matches!(index.as_ref(), Expression::Identifier { name, .. } if name == "i")
        ));
        assert!(matches!(then_body.as_slice(), [Statement::Continue { .. }]));

        let Expression::Binary { right, .. } = right else {
            panic!("expected binary assignment rhs");
        };
        assert!(matches!(
            right.as_ref(),
            Expression::IndexAccess {
                index, ..
            } if matches!(index.as_ref(), Expression::Identifier { name, .. } if name == "i")
        ));
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

    #[test]
    fn parses_nested_aggregates_unions_and_bitfields() {
        let sample = r#"
                struct Outer {
                  int flags:3;
                  struct Inner {
                    int a;
                    union Choice {
                      int i;
                      char c;
                    };
                  };
                  union Storage {
                    int x;
                    char y;
                  };
                  union {
                    long promoted;
                  };
                  struct {
                    int inline_x;
                  } inline_field;
                };
                union Top {
                  int i;
                  char c;
                };
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::C)
            .expect("nested aggregate sample should parse");

        assert_eq!(declarations.len(), 2);
        let Declaration::Struct(outer) = &declarations[0] else {
            panic!("expected outer aggregate");
        };
        assert_eq!(outer.name, "Outer");
        assert_eq!(outer.fields.len(), 2);
        assert_eq!(outer.fields[0].name, "flags");
        assert_eq!(outer.fields[0].type_name, "int");
        assert_eq!(outer.fields[0].code, "int flags:3");
        assert_eq!(outer.fields[1].name, "inline_field");
        assert_eq!(outer.fields[1].type_name, "inline_field");

        let [Declaration::Struct(inner), Declaration::Struct(storage), Declaration::Struct(anonymous_union), Declaration::Struct(inline_field)] =
            outer.nested_declarations.as_slice()
        else {
            panic!("expected nested named and anonymous aggregates");
        };
        assert_eq!(inner.name, "Inner");
        assert_eq!(inner.fields[0].name, "a");
        let [Declaration::Struct(choice)] = inner.nested_declarations.as_slice() else {
            panic!("expected nested union under inner");
        };
        assert_eq!(choice.name, "Choice");
        assert_eq!(
            choice
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            vec!["i", "c"]
        );

        assert_eq!(storage.name, "Storage");
        assert_eq!(
            storage
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            vec!["x", "y"]
        );
        assert_eq!(anonymous_union.name, "<type>0");
        assert_eq!(anonymous_union.fields[0].name, "promoted");
        assert_eq!(inline_field.name, "inline_field");
        assert_eq!(inline_field.fields[0].name, "inline_x");

        let Declaration::Struct(top) = &declarations[1] else {
            panic!("expected top-level union");
        };
        assert_eq!(top.name, "Top");
        assert_eq!(top.fields.len(), 2);
    }

    #[test]
    fn parses_function_pointer_declarator_types() {
        let sample = r#"
                struct Ops {
                  int (*open)(int);
                };
                typedef int (*Callback)(int);
                int (*foo)(int, int) = { 0 };
                int (*bar[])(int, int) = { 0 };
                int invoke(int (*cb)(int), int value) {
                  struct Ops ops;
                  int (*local)(int) = cb;
                  local(value);
                  ops.open(value);
                  return cb(value);
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::C)
            .expect("function pointer sample should parse");

        let ops = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(value) if value.name == "Ops" => Some(value),
                _ => None,
            })
            .expect("struct should be emitted");
        assert_eq!(ops.fields[0].name, "open");
        assert_eq!(ops.fields[0].type_name, "int(*)(int)");

        let callback = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Typedef(value) if value.name == "Callback" => Some(value),
                _ => None,
            })
            .expect("typedef should be emitted");
        assert_eq!(callback.type_name, "int(*)(int)");

        let foo = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::GlobalVariable(value) if value.name == "foo" => Some(value),
                _ => None,
            })
            .expect("foo global should be emitted");
        assert_eq!(foo.type_name, "int(*)(int,int)");

        let bar = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::GlobalVariable(value) if value.name == "bar" => Some(value),
                _ => None,
            })
            .expect("bar global should be emitted");
        assert_eq!(bar.type_name, "int(*[])(int,int)");

        let invoke = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(value) if value.name == "invoke" => Some(value),
                _ => None,
            })
            .expect("function should be emitted");
        assert_eq!(invoke.parameters[0].name, "cb");
        assert_eq!(invoke.parameters[0].type_name, "int(*)(int)");
        assert_eq!(invoke.signature, "int(int(*)(int),int)");
        let [Statement::LocalDecl {
            name: ops_name,
            type_name: ops_type,
            ..
        }, Statement::LocalDecl {
            name, type_name, ..
        }, Statement::Expression {
            expression: local_call,
            ..
        }, Statement::Expression {
            expression: field_call,
            ..
        }, Statement::Return {
            expression: Some(return_call),
            ..
        }] = invoke.body.as_slice()
        else {
            panic!("expected locals, pointer calls, and return");
        };
        assert_eq!(ops_name, "ops");
        assert_eq!(ops_type, "Ops");
        assert_eq!(name, "local");
        assert_eq!(type_name, "int(*)(int)");

        let Expression::Call { callee, .. } = local_call else {
            panic!("expected local function pointer call");
        };
        assert!(matches!(
            callee.as_ref(),
            Expression::Identifier { name, .. } if name == "local"
        ));

        let Expression::Call { callee, .. } = field_call else {
            panic!("expected field function pointer call");
        };
        assert!(
            matches!(callee.as_ref(), Expression::FieldAccess { field, .. } if field == "open")
        );

        let Expression::Call { callee, .. } = return_call else {
            panic!("expected return function pointer call");
        };
        assert!(matches!(
            callee.as_ref(),
            Expression::Identifier { name, .. } if name == "cb"
        ));
    }

    fn function_return_literal<'a>(document: &'a CxxAstDocument, name: &str) -> &'a str {
        let function = document
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == name => Some(function),
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected function '{name}'"));
        let [Statement::Return {
            expression: Some(Expression::Literal { value, .. }),
            ..
        }] = function.body.as_slice()
        else {
            panic!("expected '{name}' to contain one literal return");
        };
        value
    }

    fn collect_call_names(expression: &Expression) -> Vec<String> {
        match expression {
            Expression::Call {
                name, arguments, ..
            } => {
                let mut calls = vec![name.clone()];
                calls.extend(arguments.iter().flat_map(collect_call_names));
                calls
            }
            Expression::Binary { left, right, .. } => {
                let mut calls = collect_call_names(left);
                calls.extend(collect_call_names(right));
                calls
            }
            Expression::Conditional {
                condition,
                consequence,
                alternative,
                ..
            } => {
                let mut calls = collect_call_names(condition);
                if let Some(consequence) = consequence {
                    calls.extend(collect_call_names(consequence));
                }
                calls.extend(collect_call_names(alternative));
                calls
            }
            Expression::Unary { argument, .. }
            | Expression::Cast {
                value: argument, ..
            }
            | Expression::Delete { argument, .. }
            | Expression::FieldAccess { base: argument, .. } => collect_call_names(argument),
            Expression::SizeOf { value, .. } => {
                value.as_deref().map(collect_call_names).unwrap_or_default()
            }
            Expression::New { arguments, .. }
            | Expression::InitializerList {
                elements: arguments,
                ..
            } => arguments.iter().flat_map(collect_call_names).collect(),
            Expression::Lambda { body, .. } => {
                body.iter().flat_map(collect_statement_call_names).collect()
            }
            Expression::IndexAccess { base, index, .. } => {
                let mut calls = collect_call_names(base);
                calls.extend(collect_call_names(index));
                calls
            }
            Expression::DesignatedInitializer {
                designator, value, ..
            } => {
                let mut calls = collect_call_names(designator);
                calls.extend(collect_call_names(value));
                calls
            }
            _ => Vec::new(),
        }
    }

    fn collect_statement_call_names(statement: &Statement) -> Vec<String> {
        match statement {
            Statement::LocalDecl { initializer, .. } => initializer
                .as_ref()
                .map(collect_call_names)
                .unwrap_or_default(),
            Statement::Assignment { left, right, .. } => {
                let mut calls = collect_call_names(left);
                calls.extend(collect_call_names(right));
                calls
            }
            Statement::Return { expression, .. } | Statement::Throw { expression, .. } => {
                expression
                    .as_ref()
                    .map(collect_call_names)
                    .unwrap_or_default()
            }
            Statement::Try { body, catches, .. } => {
                let mut calls = body
                    .iter()
                    .flat_map(collect_statement_call_names)
                    .collect::<Vec<_>>();
                calls.extend(
                    catches
                        .iter()
                        .flat_map(|catch| catch.body.iter())
                        .flat_map(collect_statement_call_names),
                );
                calls
            }
            Statement::If {
                condition,
                then_body,
                else_body,
                ..
            } => {
                let mut calls = collect_call_names(condition);
                calls.extend(then_body.iter().flat_map(collect_statement_call_names));
                calls.extend(else_body.iter().flat_map(collect_statement_call_names));
                calls
            }
            Statement::While {
                condition, body, ..
            }
            | Statement::DoWhile {
                condition, body, ..
            }
            | Statement::Switch {
                condition, body, ..
            } => {
                let mut calls = collect_call_names(condition);
                calls.extend(body.iter().flat_map(collect_statement_call_names));
                calls
            }
            Statement::For {
                initializer,
                condition,
                update,
                body,
                ..
            } => {
                let mut calls = initializer
                    .iter()
                    .flat_map(collect_statement_call_names)
                    .collect::<Vec<_>>();
                if let Some(condition) = condition {
                    calls.extend(collect_call_names(condition));
                }
                if let Some(update) = update {
                    calls.extend(collect_call_names(update));
                }
                calls.extend(body.iter().flat_map(collect_statement_call_names));
                calls
            }
            Statement::Label { body, .. } | Statement::Case { body, .. } => {
                body.iter().flat_map(collect_statement_call_names).collect()
            }
            Statement::Expression { expression, .. } => collect_call_names(expression),
            Statement::Break { .. } | Statement::Continue { .. } | Statement::Goto { .. } => {
                Vec::new()
            }
        }
    }

    fn statement_line(statement: &Statement) -> usize {
        match statement {
            Statement::LocalDecl { line, .. }
            | Statement::Assignment { line, .. }
            | Statement::Return { line, .. }
            | Statement::Throw { line, .. }
            | Statement::Try { line, .. }
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
