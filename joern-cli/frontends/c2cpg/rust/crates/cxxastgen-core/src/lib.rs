use anyhow::{Context, Result};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser};

pub const SCHEMA_VERSION: u32 = 1;
pub const BACKEND_NAME: &str = "oxidized-cxxastgen";

thread_local! {
    /// Tallies tree-sitter node kinds that fall through to an `Unknown`/identifier
    /// fallback while lowering statements and expressions. Accumulates across every
    /// file processed in a single CLI run. The caller surfaces it as one stderr
    /// summary line via [`take_unmapped_summary`]; it must never reach stdout or the
    /// emitted JSON.
    static UNMAPPED_KINDS: RefCell<BTreeMap<String, usize>> = const { RefCell::new(BTreeMap::new()) };
}

/// Records a single tree-sitter node `kind` that could not be mapped to a
/// dedicated oxidized AST node and fell back to a generic representation.
fn record_unmapped_kind(kind: &str) {
    UNMAPPED_KINDS.with(|counts| {
        *counts.borrow_mut().entry(kind.to_string()).or_insert(0) += 1;
    });
}

/// Drains the accumulated unmapped-kind counts and renders a one-line human
/// summary, e.g. `cxxastgen: 3 unmapped node(s): comma_expression(x2), goto_label(x1)`.
/// Returns `None` when every node lowered in this run had a dedicated mapping.
/// Calling this resets the counter, so the CLI should print it exactly once at
/// the end of a run (to stderr only — never stdout or the emitted JSON).
pub fn take_unmapped_summary() -> Option<String> {
    UNMAPPED_KINDS.with(|counts| {
        let counts = std::mem::take(&mut *counts.borrow_mut());
        if counts.is_empty() {
            return None;
        }
        let total: usize = counts.values().sum();
        let details = counts
            .iter()
            .map(|(kind, count)| format!("{kind}(x{count})"))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!("cxxastgen: {total} unmapped node(s): {details}"))
    })
}

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
    #[serde(
        rename = "baseClassDeclarations",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub base_class_declarations: Vec<BaseClassDecl>,
    #[serde(rename = "usingDeclarations", skip_serializing_if = "Vec::is_empty")]
    pub using_declarations: Vec<UsingDecl>,
    pub fields: Vec<FieldDecl>,
    #[serde(rename = "nestedDeclarations")]
    pub nested_declarations: Vec<Declaration>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseClassDecl {
    pub name: String,
    pub code: String,
    pub is_virtual: bool,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsingDecl {
    pub name: String,
    pub target: String,
    pub code: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldDecl {
    pub name: String,
    pub type_name: String,
    pub semantic_type_name: String,
    pub code: String,
    pub is_static: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initializer: Option<Expression>,
    /// Phase-2 semantic engine output: the field's declared type resolved to its
    /// fully-qualified dotted name (e.g. `Core.Widget`) when the type is written
    /// explicitly and resolves trivially against the type symbol table. Builtins
    /// (`int`, `char`, ...) are kept verbatim. Left `None` for `auto`/`decltype`
    /// or ambiguous/unknown types. Additive JSON ignored by the current reader.
    #[serde(
        rename = "resolvedTypeFullName",
        skip_serializing_if = "Option::is_none"
    )]
    pub resolved_type_full_name: Option<String>,
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
    pub semantic_type_name: String,
    pub code: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(rename = "visibleLine", skip_serializing_if = "Option::is_none")]
    pub visible_line: Option<usize>,
    pub initializer: Option<Expression>,
    /// Phase-2: declared type resolved to its qualified dotted name when written
    /// explicitly and trivially resolvable; builtins kept verbatim; `None` when
    /// `auto`/ambiguous. See [`FieldDecl::resolved_type_full_name`].
    #[serde(
        rename = "resolvedTypeFullName",
        skip_serializing_if = "Option::is_none"
    )]
    pub resolved_type_full_name: Option<String>,
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
    pub semantic_return_type: String,
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
    #[serde(rename = "semanticTypeName")]
    pub semantic_type_name: String,
    #[serde(rename = "isVariadic")]
    pub is_variadic: bool,
    #[serde(rename = "hasDefault")]
    pub has_default: bool,
    pub code: String,
    pub line: usize,
    /// Phase-2: declared parameter type resolved to its qualified dotted name
    /// when written explicitly and trivially resolvable; builtins kept verbatim;
    /// `None` when `auto`/ambiguous. See [`FieldDecl::resolved_type_full_name`].
    #[serde(
        rename = "resolvedTypeFullName",
        skip_serializing_if = "Option::is_none"
    )]
    pub resolved_type_full_name: Option<String>,
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
    Unknown {
        code: String,
        line: usize,
    },
    UsingEnum {
        #[serde(rename = "typeName")]
        type_name: String,
        code: String,
        line: usize,
    },
    LocalDecl {
        name: String,
        #[serde(rename = "typeName")]
        type_name: String,
        #[serde(rename = "semanticTypeName")]
        semantic_type_name: String,
        code: String,
        line: usize,
        initializer: Option<Expression>,
        /// Phase-2: declared local type resolved to its qualified dotted name
        /// when written explicitly and trivially resolvable; builtins kept
        /// verbatim; `None` when `auto`/ambiguous. Additive JSON.
        #[serde(
            rename = "resolvedTypeFullName",
            skip_serializing_if = "Option::is_none"
        )]
        resolved_type_full_name: Option<String>,
    },
    StructuredBinding {
        #[serde(rename = "typeName")]
        type_name: String,
        code: String,
        line: usize,
        #[serde(rename = "tempName")]
        temp_name: String,
        names: Vec<String>,
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
        #[serde(skip_serializing_if = "Vec::is_empty")]
        initializer: Vec<Statement>,
        #[serde(rename = "conditionInitializer")]
        #[serde(skip_serializing_if = "Vec::is_empty")]
        condition_initializer: Vec<Statement>,
        condition: Expression,
        #[serde(rename = "thenBody")]
        then_body: Vec<Statement>,
        #[serde(rename = "elseBody")]
        else_body: Vec<Statement>,
    },
    While {
        code: String,
        line: usize,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        initializer: Vec<Statement>,
        #[serde(rename = "conditionInitializer")]
        #[serde(skip_serializing_if = "Vec::is_empty")]
        condition_initializer: Vec<Statement>,
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
        #[serde(skip_serializing_if = "Vec::is_empty")]
        initializer: Vec<Statement>,
        #[serde(rename = "conditionInitializer")]
        #[serde(skip_serializing_if = "Vec::is_empty")]
        condition_initializer: Vec<Statement>,
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
        /// Phase-2: the literal's unambiguous type (`int`, `double`, `char`,
        /// `bool`, `char*`, ...) for the trivially classifiable cases; `None`
        /// otherwise. Additive JSON ignored by the current reader.
        #[serde(
            rename = "resolvedTypeFullName",
            skip_serializing_if = "Option::is_none"
        )]
        resolved_type_full_name: Option<String>,
    },
    Binary {
        operator: String,
        code: String,
        line: usize,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    Assignment {
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
    Fold {
        operator: String,
        code: String,
        line: usize,
        left: Option<Box<Expression>>,
        right: Option<Box<Expression>>,
    },
    PackExpansion {
        code: String,
        line: usize,
        pattern: Box<Expression>,
    },
    TypeOf {
        code: String,
        line: usize,
        argument: Box<Expression>,
    },
    Cast {
        #[serde(rename = "typeName")]
        type_name: String,
        #[serde(rename = "semanticTypeName")]
        semantic_type_name: String,
        code: String,
        line: usize,
        value: Box<Expression>,
        /// Phase-2: the cast target type resolved to its qualified dotted name
        /// when trivially resolvable; builtins kept verbatim; `None` otherwise.
        /// Additive JSON ignored by the current reader.
        #[serde(
            rename = "resolvedTypeFullName",
            skip_serializing_if = "Option::is_none"
        )]
        resolved_type_full_name: Option<String>,
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
        #[serde(rename = "semanticReturnType")]
        semantic_return_type: String,
        signature: String,
        body: Vec<Statement>,
    },
    Call {
        name: String,
        code: String,
        line: usize,
        callee: Box<Expression>,
        arguments: Vec<Expression>,
        /// Phase-1 semantic engine output: the fully-qualified method name this
        /// call unambiguously resolves to, in the dotted form the Scala backend
        /// builds (e.g. `Core.Widget.render:int(int)`). Populated only when the
        /// symbol table finds exactly one *defined* candidate by qualified/simple
        /// name plus argument count; left `None` when ambiguous or unknown. This
        /// is strictly additive JSON: the hand-rolled ujson reader on the Scala
        /// side ignores unknown object fields, so the CPG is unchanged until a
        /// later, CDT-faithful phase opts in to consuming it.
        #[serde(
            rename = "resolvedMethodFullName",
            skip_serializing_if = "Option::is_none"
        )]
        resolved_method_full_name: Option<String>,
        /// Signature of the resolved candidate (e.g. `int(int)`), emitted only
        /// alongside [`Expression::Call::resolved_method_full_name`].
        #[serde(rename = "resolvedSignature", skip_serializing_if = "Option::is_none")]
        resolved_signature: Option<String>,
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
    resolve_call_targets(&mut declarations);

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
        "cc" | "cpp" | "cxx" | "cp" | "ccm" | "cxxm" | "c++" | "c++m" => SourceLanguage::Cpp,
        "h" | "hh" | "hpp" | "hxx" | "hp" | "h++" | "ipp" | "tcc" => SourceLanguage::Header,
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

// ---------------------------------------------------------------------------
// Phase-1 semantic engine: symbol table + unambiguous call resolution.
//
// A post-parse pass walks the declaration tree, records every plain
// function/method declaration with its fully-qualified (dotted) name plus
// parameter-arity and signature, and then walks every call expression. When a
// call resolves to exactly one *defined* candidate by qualified-or-simple name
// plus argument count, it stamps `resolvedMethodFullName`/`resolvedSignature`
// onto the call node. Everything here is additive: ambiguous or unknown calls
// are left untouched, and the emitted fields are simply ignored by the Scala
// reader until a later, CDT-faithful phase opts in to consuming them.
// ---------------------------------------------------------------------------

/// One resolvable function/method declaration collected by the symbol table.
#[derive(Debug, Clone)]
struct SymbolEntry {
    /// Dotted, fully-qualified name without the signature, e.g. `Core.Widget.render`.
    qualified_name: String,
    /// Trailing identifier only, e.g. `render`.
    simple_name: String,
    /// Number of declared parameters (excluding a trailing C variadic `...`).
    arity: usize,
    /// Whether any parameter is variadic (so arity is a lower bound, not exact).
    has_variadic: bool,
    /// The declaration signature verbatim, e.g. `int(int)`.
    signature: String,
    /// Whether this entry came from a definition (body present) rather than a
    /// bare prototype. Only definitions are used to resolve calls, mirroring the
    /// CDT backend which leaves prototype-only externals as bare names.
    is_definition: bool,
}

/// One user-defined type declaration (struct/class/union/enum/typedef/alias)
/// collected by the symbol table.
#[derive(Debug, Clone)]
struct TypeEntry {
    /// Dotted, fully-qualified type name, e.g. `Core.Widget`.
    qualified_name: String,
    /// Trailing identifier only, e.g. `Widget`.
    simple_name: String,
}

/// Name -> candidate declarations, keyed by both dotted-qualified and simple name.
/// Functions and user types are indexed separately.
#[derive(Debug, Default)]
struct SymbolTable {
    by_qualified_name: HashMap<String, Vec<SymbolEntry>>,
    by_simple_name: HashMap<String, Vec<SymbolEntry>>,
    types_by_qualified_name: HashMap<String, Vec<TypeEntry>>,
    types_by_simple_name: HashMap<String, Vec<TypeEntry>>,
}

impl SymbolTable {
    fn insert(&mut self, entry: SymbolEntry) {
        self.by_qualified_name
            .entry(entry.qualified_name.clone())
            .or_default()
            .push(entry.clone());
        self.by_simple_name
            .entry(entry.simple_name.clone())
            .or_default()
            .push(entry);
    }

    fn insert_type(&mut self, entry: TypeEntry) {
        self.types_by_qualified_name
            .entry(entry.qualified_name.clone())
            .or_default()
            .push(entry.clone());
        self.types_by_simple_name
            .entry(entry.simple_name.clone())
            .or_default()
            .push(entry);
    }
}

/// A function name is only treated as a resolution target when it is a plain
/// identifier path. Operator/conversion functions, out-of-line `A::B::f`
/// definitions and the synthetic `requires` helper all have bespoke
/// full-name formatting on the Scala side and are deferred to a later phase.
fn is_plain_function_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("::")
        && !name.contains('<')
        && !name.contains('~')
        && !name.contains(' ')
        && !name.contains('(')
}

/// Records `function` into `table` under the dotted `scope` (enclosing namespace
/// and class path). Skips anything that is not a plain identifier function.
fn collect_function_symbol(table: &mut SymbolTable, scope: &str, function: &FunctionDecl) {
    if !is_plain_function_name(&function.name) {
        return;
    }
    let simple_name = function.name.clone();
    let qualified_name = if scope.is_empty() {
        simple_name.clone()
    } else {
        format!("{scope}.{simple_name}")
    };
    let has_variadic = function.parameters.iter().any(|param| param.is_variadic);
    let arity = function
        .parameters
        .iter()
        .filter(|param| !param.is_variadic)
        .count();
    table.insert(SymbolEntry {
        qualified_name,
        simple_name,
        arity,
        has_variadic,
        signature: function.signature.clone(),
        is_definition: function.is_definition,
    });
}

/// Records a user-defined type named `name` (already a plain identifier) into
/// `table` under the dotted `scope`.
fn collect_type_symbol(table: &mut SymbolTable, scope: &str, name: &str) {
    let simple_name = name.trim();
    if !is_plain_type_name(simple_name) {
        return;
    }
    let qualified_name = if scope.is_empty() {
        simple_name.to_string()
    } else {
        format!("{scope}.{simple_name}")
    };
    table.insert_type(TypeEntry {
        qualified_name,
        simple_name: simple_name.to_string(),
    });
}

/// A type name is only recorded when it is a plain identifier: anonymous,
/// templated, or otherwise decorated names are skipped (deferred to a later
/// phase) to keep resolution unambiguous and format-stable.
fn is_plain_type_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("::")
        && !name.contains('<')
        && !name.contains(' ')
        && !name.contains('*')
        && !name.contains('&')
        && !name.contains('(')
        && !name.starts_with('<')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
}

/// Walks a declaration subtree, extending `scope` for namespaces and aggregates,
/// and records every plain function/method and user-defined type it finds.
fn collect_symbols(table: &mut SymbolTable, scope: &str, declarations: &[Declaration]) {
    for declaration in declarations {
        match declaration {
            Declaration::Function(function) => collect_function_symbol(table, scope, function),
            Declaration::Namespace(namespace) => {
                let nested = extend_scope(scope, &namespace.name);
                collect_symbols(table, &nested, &namespace.declarations);
            }
            Declaration::Struct(struct_decl) => {
                collect_type_symbol(table, scope, &struct_decl.name);
                let nested = extend_scope(scope, &struct_decl.name);
                collect_symbols(table, &nested, &struct_decl.nested_declarations);
            }
            Declaration::Enum(enum_decl) => {
                collect_type_symbol(table, scope, &enum_decl.name);
            }
            Declaration::Typedef(typedef) => {
                collect_type_symbol(table, scope, &typedef.name);
            }
            _ => {}
        }
    }
}

/// Joins a dotted `scope` with a `name`, normalizing any `::` the name carries.
fn extend_scope(scope: &str, name: &str) -> String {
    let normalized = name.trim().replace("::", ".");
    if normalized.is_empty() {
        scope.to_string()
    } else if scope.is_empty() {
        normalized
    } else {
        format!("{scope}.{normalized}")
    }
}

/// Resolves a call's callee to its lookup name. Returns `None` for calls whose
/// callee is not a plain identifier / qualified identifier (e.g. member access,
/// function pointers, operators), which are deferred to a later phase.
fn call_lookup_name(name: &str) -> Option<(String, String)> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.contains('<') || trimmed.contains('(') {
        return None;
    }
    let qualified = trimmed.replace("::", ".");
    let simple = qualified
        .rsplit('.')
        .next()
        .unwrap_or(&qualified)
        .to_string();
    if simple.is_empty() || simple.contains(' ') {
        return None;
    }
    Some((qualified, simple))
}

/// Finds the unique *defined* candidate for `name`/`arity`, preferring an exact
/// dotted-qualified match before falling back to a simple-name match. Returns
/// `None` whenever the match is ambiguous or unknown.
fn resolve_unique_target<'a>(
    table: &'a SymbolTable,
    name: &str,
    arity: usize,
) -> Option<&'a SymbolEntry> {
    let (qualified, simple) = call_lookup_name(name)?;
    unique_definition(table.by_qualified_name.get(&qualified), arity)
        .or_else(|| unique_definition(table.by_simple_name.get(&simple), arity))
}

/// From a candidate slice, returns the single arity-compatible definition, or
/// `None` when there are zero or several such definitions.
fn unique_definition(candidates: Option<&Vec<SymbolEntry>>, arity: usize) -> Option<&SymbolEntry> {
    let candidates = candidates?;
    let mut matches = candidates.iter().filter(|entry| {
        entry.is_definition
            && (entry.arity == arity || (entry.has_variadic && arity >= entry.arity))
    });
    let first = matches.next()?;
    match matches.next() {
        Some(_) => None,
        None => Some(first),
    }
}

/// Fundamental C/C++ builtin type spellings that are left verbatim (never
/// rewritten to a qualified name). Multi-word builtins (`unsigned int`, `long
/// long`) are handled by stripping these words during normalization.
const BUILTIN_TYPE_WORDS: &[&str] = &[
    "void",
    "bool",
    "char",
    "char8_t",
    "char16_t",
    "char32_t",
    "wchar_t",
    "short",
    "int",
    "long",
    "signed",
    "unsigned",
    "float",
    "double",
    "nullptr_t",
    "size_t",
    "ptrdiff_t",
    "int8_t",
    "int16_t",
    "int32_t",
    "int64_t",
    "uint8_t",
    "uint16_t",
    "uint32_t",
    "uint64_t",
];

/// Splits a written type into its core object spelling and the pointer/reference
/// suffix (`*`, `&`, `&&`, `[]`, possibly combined), after stripping leading
/// `struct`/`union`/`enum` keywords and cv/storage qualifiers. Returns `None`
/// when the type is not trivially resolvable (`auto`, `decltype(...)`,
/// templated, function pointer, empty, or otherwise compound).
fn split_core_type(written: &str) -> Option<(String, String)> {
    let normalized = normalize_type(written);
    let trimmed = normalized.trim();
    if trimmed.is_empty()
        || trimmed.contains('<')
        || trimmed.contains('(')
        || trimmed.contains(',')
        || trimmed.starts_with("decltype")
    {
        return None;
    }
    // Peel a trailing run of pointer/reference/array decorators.
    let mut core = trimmed;
    let mut suffix = String::new();
    loop {
        let core_trimmed = core.trim_end();
        if let Some(stripped) = core_trimmed.strip_suffix("[]") {
            suffix.insert_str(0, "[]");
            core = stripped;
        } else if let Some(stripped) = core_trimmed.strip_suffix("&&") {
            suffix.insert_str(0, "&&");
            core = stripped;
        } else if let Some(stripped) = core_trimmed.strip_suffix('&') {
            suffix.insert(0, '&');
            core = stripped;
        } else if let Some(stripped) = core_trimmed.strip_suffix('*') {
            suffix.insert(0, '*');
            core = stripped;
        } else {
            core = core_trimmed;
            break;
        }
    }
    let core = core.trim();
    if core.is_empty() || core == "auto" {
        return None;
    }
    Some((core.to_string(), suffix))
}

/// True when `core` is a single fundamental builtin type spelling (after the
/// multi-word qualifier words have been stripped by [`normalize_type`]).
fn is_builtin_type(core: &str) -> bool {
    core.split_whitespace()
        .all(|word| BUILTIN_TYPE_WORDS.contains(&word))
}

/// Looks up the unique user type matching `core`, preferring an exact dotted
/// qualified match before a unique simple-name match. Returns `None` when the
/// match is ambiguous or unknown.
fn unique_type_qualified_name(table: &SymbolTable, core: &str) -> Option<String> {
    let dotted = core.replace("::", ".");
    if let Some(entries) = table.types_by_qualified_name.get(&dotted) {
        if !entries.is_empty() {
            return Some(entries[0].qualified_name.clone());
        }
    }
    let simple = dotted.rsplit('.').next().unwrap_or(&dotted);
    let entries = table.types_by_simple_name.get(simple)?;
    let mut qualified_names: Vec<&String> =
        entries.iter().map(|entry| &entry.qualified_name).collect();
    qualified_names.sort();
    qualified_names.dedup();
    match qualified_names.as_slice() {
        [single] => Some((*single).clone()),
        _ => None,
    }
}

/// Resolves a written declaration type to a `resolvedTypeFullName` value when it
/// is trivially determinable: builtins are kept verbatim, a unique user type is
/// rewritten to its dotted qualified name, and pointer/reference/array suffixes
/// are preserved. Returns `None` for `auto`/`decltype`/templated/ambiguous types.
fn resolve_declared_type(table: &SymbolTable, written: &str) -> Option<String> {
    let (core, suffix) = split_core_type(written)?;
    if is_builtin_type(&core) {
        return Some(format!("{core}{suffix}"));
    }
    let qualified = unique_type_qualified_name(table, &core)?;
    Some(format!("{qualified}{suffix}"))
}

/// Infers the type of a literal from its source spelling for the trivially
/// unambiguous cases. Returns `None` otherwise (e.g. user-defined literals).
fn infer_literal_type(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed {
        "true" | "false" => return Some("bool".to_string()),
        "nullptr" => return Some("std.nullptr_t".to_string()),
        _ => {}
    }
    if is_string_literal_spelling(trimmed) {
        return Some("char*".to_string());
    }
    if is_char_literal_spelling(trimmed) {
        return Some("char".to_string());
    }
    if integer_literal_value(trimmed).is_some() {
        return Some("int".to_string());
    }
    if is_floating_literal_spelling(trimmed) {
        return Some("double".to_string());
    }
    None
}

/// True when `value` is spelled as a (possibly prefixed) string literal.
fn is_string_literal_spelling(value: &str) -> bool {
    [
        "\"", "u8\"", "u\"", "U\"", "L\"", "R\"", "u8R\"", "uR\"", "UR\"", "LR\"",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix))
        && value.ends_with('"')
}

/// True when `value` is spelled as a (possibly prefixed) character literal.
fn is_char_literal_spelling(value: &str) -> bool {
    ["'", "u'", "U'", "L'", "u8'"]
        .iter()
        .any(|prefix| value.starts_with(prefix))
        && value.ends_with('\'')
}

/// True when `value` is unambiguously a floating-point literal (contains a
/// decimal point or exponent and parses as a float, and is not an integer).
fn is_floating_literal_spelling(value: &str) -> bool {
    if integer_literal_value(value).is_some() {
        return false;
    }
    let body = value.trim().trim_end_matches(['f', 'F', 'l', 'L']);
    if body.is_empty() {
        return false;
    }
    let looks_floating = body.contains('.')
        || ((body.contains('e') || body.contains('E')) && !body.starts_with("0x"));
    looks_floating && body.parse::<f64>().is_ok()
}

/// Entry point: builds the symbol table from `declarations`, then rewrites every
/// call expression and trivially-typed declaration/expression in place.
fn resolve_call_targets(declarations: &mut [Declaration]) {
    let mut table = SymbolTable::default();
    collect_symbols(&mut table, "", declarations);
    annotate_declarations(&table, declarations);
}

fn annotate_declarations(table: &SymbolTable, declarations: &mut [Declaration]) {
    for declaration in declarations.iter_mut() {
        match declaration {
            Declaration::Function(function) => {
                for parameter in function.parameters.iter_mut() {
                    annotate_parameter(table, parameter);
                }
                for initializer in function.constructor_initializers.iter_mut() {
                    for argument in initializer.arguments.iter_mut() {
                        annotate_expression(table, argument);
                    }
                }
                annotate_statements(table, &mut function.body);
            }
            Declaration::Namespace(namespace) => {
                annotate_declarations(table, &mut namespace.declarations);
            }
            Declaration::Struct(struct_decl) => {
                for field in struct_decl.fields.iter_mut() {
                    if field.resolved_type_full_name.is_none() {
                        field.resolved_type_full_name =
                            resolve_declared_type(table, &field.type_name);
                    }
                    if let Some(initializer) = field.initializer.as_mut() {
                        annotate_expression(table, initializer);
                    }
                }
                annotate_declarations(table, &mut struct_decl.nested_declarations);
            }
            Declaration::GlobalVariable(global) => {
                if global.resolved_type_full_name.is_none() {
                    global.resolved_type_full_name =
                        resolve_declared_type(table, &global.type_name);
                }
                if let Some(initializer) = global.initializer.as_mut() {
                    annotate_expression(table, initializer);
                }
            }
            _ => {}
        }
    }
}

/// Stamps a parameter's `resolvedTypeFullName` when its written type resolves
/// trivially, then leaves it otherwise untouched.
fn annotate_parameter(table: &SymbolTable, parameter: &mut ParameterDecl) {
    if parameter.resolved_type_full_name.is_none() && !parameter.is_variadic {
        parameter.resolved_type_full_name = resolve_declared_type(table, &parameter.type_name);
    }
}

fn annotate_statements(table: &SymbolTable, statements: &mut [Statement]) {
    for statement in statements.iter_mut() {
        annotate_statement(table, statement);
    }
}

fn annotate_statement(table: &SymbolTable, statement: &mut Statement) {
    match statement {
        Statement::Unknown { .. }
        | Statement::UsingEnum { .. }
        | Statement::Break { .. }
        | Statement::Continue { .. }
        | Statement::Goto { .. } => {}
        Statement::LocalDecl {
            type_name,
            initializer,
            resolved_type_full_name,
            ..
        } => {
            if resolved_type_full_name.is_none() {
                *resolved_type_full_name = resolve_declared_type(table, type_name);
            }
            if let Some(initializer) = initializer.as_mut() {
                annotate_expression(table, initializer);
            }
        }
        Statement::StructuredBinding { initializer, .. } => {
            // Structured bindings always carry `auto`; their element types are
            // not trivially determinable, so the field is left absent.
            if let Some(initializer) = initializer.as_mut() {
                annotate_expression(table, initializer);
            }
        }
        Statement::Assignment { left, right, .. } => {
            annotate_expression(table, left);
            annotate_expression(table, right);
        }
        Statement::Return { expression, .. } | Statement::Throw { expression, .. } => {
            if let Some(expression) = expression.as_mut() {
                annotate_expression(table, expression);
            }
        }
        Statement::Try { body, catches, .. } => {
            annotate_statements(table, body);
            for catch in catches.iter_mut() {
                annotate_statements(table, &mut catch.body);
            }
        }
        Statement::If {
            initializer,
            condition_initializer,
            condition,
            then_body,
            else_body,
            ..
        } => {
            annotate_statements(table, initializer);
            annotate_statements(table, condition_initializer);
            annotate_expression(table, condition);
            annotate_statements(table, then_body);
            annotate_statements(table, else_body);
        }
        Statement::While {
            initializer,
            condition_initializer,
            condition,
            body,
            ..
        } => {
            annotate_statements(table, initializer);
            annotate_statements(table, condition_initializer);
            annotate_expression(table, condition);
            annotate_statements(table, body);
        }
        Statement::DoWhile {
            condition, body, ..
        } => {
            annotate_expression(table, condition);
            annotate_statements(table, body);
        }
        Statement::For {
            initializer,
            condition,
            update,
            body,
            ..
        } => {
            annotate_statements(table, initializer);
            if let Some(condition) = condition.as_mut() {
                annotate_expression(table, condition);
            }
            if let Some(update) = update.as_mut() {
                annotate_expression(table, update);
            }
            annotate_statements(table, body);
        }
        Statement::Label { body, .. } => annotate_statements(table, body),
        Statement::Switch {
            initializer,
            condition_initializer,
            condition,
            body,
            ..
        } => {
            annotate_statements(table, initializer);
            annotate_statements(table, condition_initializer);
            annotate_expression(table, condition);
            annotate_statements(table, body);
        }
        Statement::Case { value, body, .. } => {
            if let Some(value) = value.as_mut() {
                annotate_expression(table, value);
            }
            annotate_statements(table, body);
        }
        Statement::Expression { expression, .. } => annotate_expression(table, expression),
    }
}

fn annotate_expression(table: &SymbolTable, expression: &mut Expression) {
    match expression {
        Expression::Identifier { .. } | Expression::Designator { .. } => {}
        Expression::Literal {
            value,
            resolved_type_full_name,
            ..
        } => {
            if resolved_type_full_name.is_none() {
                *resolved_type_full_name = infer_literal_type(value);
            }
        }
        Expression::Call {
            name,
            callee,
            arguments,
            resolved_method_full_name,
            resolved_signature,
            ..
        } => {
            annotate_expression(table, callee);
            for argument in arguments.iter_mut() {
                annotate_expression(table, argument);
            }
            // Only fill genuinely empty slots; never overwrite.
            if resolved_method_full_name.is_none() {
                if let Some(entry) = resolve_unique_target(table, name, arguments.len()) {
                    *resolved_method_full_name =
                        Some(format!("{}:{}", entry.qualified_name, entry.signature));
                    *resolved_signature = Some(entry.signature.clone());
                }
            }
        }
        Expression::Binary { left, right, .. } | Expression::Assignment { left, right, .. } => {
            annotate_expression(table, left);
            annotate_expression(table, right);
        }
        Expression::Unary { argument, .. }
        | Expression::PackExpansion {
            pattern: argument, ..
        }
        | Expression::TypeOf { argument, .. }
        | Expression::Delete { argument, .. } => annotate_expression(table, argument),
        Expression::Conditional {
            condition,
            consequence,
            alternative,
            ..
        } => {
            annotate_expression(table, condition);
            if let Some(consequence) = consequence.as_mut() {
                annotate_expression(table, consequence);
            }
            annotate_expression(table, alternative);
        }
        Expression::Fold { left, right, .. } => {
            if let Some(left) = left.as_mut() {
                annotate_expression(table, left);
            }
            if let Some(right) = right.as_mut() {
                annotate_expression(table, right);
            }
        }
        Expression::Cast {
            type_name,
            value,
            resolved_type_full_name,
            ..
        } => {
            if resolved_type_full_name.is_none() {
                *resolved_type_full_name = resolve_declared_type(table, type_name);
            }
            annotate_expression(table, value);
        }
        Expression::SizeOf { value, .. } => {
            if let Some(value) = value.as_mut() {
                annotate_expression(table, value);
            }
        }
        Expression::New {
            arguments,
            initializer_arguments,
            ..
        } => {
            for argument in arguments.iter_mut().chain(initializer_arguments.iter_mut()) {
                annotate_expression(table, argument);
            }
        }
        Expression::Lambda { body, .. } => annotate_statements(table, body),
        Expression::FieldAccess { base, .. } => annotate_expression(table, base),
        Expression::IndexAccess { base, index, .. } => {
            annotate_expression(table, base);
            annotate_expression(table, index);
        }
        Expression::InitializerList { elements, .. } => {
            for element in elements.iter_mut() {
                annotate_expression(table, element);
            }
        }
        Expression::DesignatedInitializer {
            designator, value, ..
        } => {
            annotate_expression(table, designator);
            annotate_expression(table, value);
        }
    }
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
    resolve_call_targets(&mut declarations);
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
        "concept_definition" | "requires_clause" => {
            declarations.extend(
                parse_requires_expression_declarations(node, source)
                    .into_iter()
                    .map(Declaration::Function),
            );
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
            } else if let Some(function) = parse_operator_cast(node, source, false, symbols) {
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
            if let Some(function) = parse_operator_cast(node, source, true, symbols)
                .or_else(|| parse_function(node, source, symbols))
            {
                declarations.push(Declaration::Function(function));
            }
        }
        "operator_cast_definition" => {
            if let Some(function) = parse_operator_cast(node, source, true, symbols) {
                declarations.push(Declaration::Function(function));
            }
        }
        "operator_cast_declaration" => {
            if let Some(function) = parse_operator_cast(node, source, false, symbols) {
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
        // An identifier that is not a defined macro evaluates to 0 in a `#if`
        // expression, matching the C preprocessor.
        return 0;
    };
    if !binding.parameters.is_empty() {
        // Function-like macros require call syntax (`M(...)`) to expand. A bare
        // reference in a condition cannot be evaluated here; preserve the legacy
        // "treat as truthy" behavior rather than guessing a value.
        return 1;
    }
    // Object-like macro: expand its replacement list (recursively, with a
    // "blue paint" guard against self-reference) and evaluate the result.
    let mut active = HashSet::new();
    eval_object_like_macro(name, binding, symbols, &mut active).unwrap_or(1)
}

/// Evaluates an object-like macro reference inside a `#if`/`#elif` expression by
/// performing real (lexical) macro expansion of its replacement list and then
/// re-evaluating the substituted text. Returns `None` (so the caller can fall
/// back to the legacy behavior) when the body uses preprocessor features we do
/// not expand (`#`, `##`, `__VA_ARGS__`) or otherwise cannot be reduced to an
/// integer. `active` carries the set of macro names currently being expanded so
/// that recursive/self-referential macros terminate instead of looping forever.
fn eval_object_like_macro<'a>(
    name: &'a str,
    binding: &'a MacroBinding,
    symbols: &'a MacroSymbols,
    active: &mut HashSet<&'a str>,
) -> Option<i64> {
    // Fast path: the body is already (parenthesized) integer literal.
    if let Some(value) = macro_body_integer_value(&binding.body) {
        return Some(value);
    }
    // Blue-paint rule: a macro is not expanded within its own expansion.
    if !active.insert(name) {
        return None;
    }
    let expanded = expand_object_like_text(&binding.body, symbols, active);
    active.remove(name);
    let expanded = expanded?;
    if let Some(value) = macro_body_integer_value(&expanded) {
        return Some(value);
    }
    // A fully expanded replacement list must not still reference any object-like
    // macro. If it does, expansion hit a self-referential / mutually-recursive
    // cycle (the blue-paint guard left the name in place); bail so the caller
    // falls back rather than re-expanding and looping forever.
    if expanded_text_references_object_like_macro(&expanded, symbols) {
        return None;
    }
    eval_condition_text(&expanded, symbols)
}

/// Returns true when `text` still contains an identifier that names an
/// object-like macro. Used to detect unresolved expansion cycles before handing
/// the text back to the (macro-aware) condition evaluator.
fn expanded_text_references_object_like_macro(text: &str, symbols: &MacroSymbols) -> bool {
    split_preproc_identifier_tokens(text)
        .into_iter()
        .any(|token| match token {
            PreprocToken::Identifier(ident) => symbols
                .get(ident)
                .is_some_and(|binding| binding.parameters.is_empty()),
            PreprocToken::Other(_) => false,
        })
}

fn macro_body_integer_value(body: &str) -> Option<i64> {
    integer_literal_value(strip_wrapping_parentheses(body.trim()))
}

/// Tokens that signal preprocessor operators we deliberately do not expand
/// (stringize, token-paste, variadic). Macros whose replacement list uses them
/// fall back to the legacy condition handling rather than risking a wrong value.
fn body_uses_unsupported_preproc_features(body: &str) -> bool {
    body.contains('#') || body.contains("__VA_ARGS__")
}

/// Recursively substitutes object-like macros referenced inside `text`,
/// returning the fully expanded replacement text. Identifiers that are not
/// object-like macros (including function-like macro names, which require call
/// syntax) are left untouched. `active` provides the blue-paint guard shared
/// with [`eval_object_like_macro`]. Returns `None` when an unsupported
/// preprocessor feature is encountered so callers can fall back safely.
fn expand_object_like_text<'a>(
    text: &str,
    symbols: &'a MacroSymbols,
    active: &mut HashSet<&'a str>,
) -> Option<String> {
    if body_uses_unsupported_preproc_features(text) {
        record_unmapped_kind("preproc_macro_stringize_or_paste");
        return None;
    }
    let mut output = String::with_capacity(text.len());
    for token in split_preproc_identifier_tokens(text) {
        match token {
            PreprocToken::Identifier(ident) => {
                match symbols.get_key_value(ident) {
                    // Object-like macro that is not already being expanded.
                    Some((key, binding))
                        if binding.parameters.is_empty() && !active.contains(key.as_str()) =>
                    {
                        active.insert(key.as_str());
                        let nested = expand_object_like_text(&binding.body, symbols, active);
                        active.remove(key.as_str());
                        output.push_str(&nested?);
                    }
                    // Not a macro, function-like macro, or painted blue: keep it.
                    _ => output.push_str(ident),
                }
            }
            PreprocToken::Other(raw) => output.push_str(raw),
        }
    }
    Some(output)
}

enum PreprocToken<'a> {
    Identifier(&'a str),
    Other(&'a str),
}

/// Splits `text` into a sequence of identifier and non-identifier spans. This is
/// a minimal lexical scan sufficient for object-like macro substitution: it only
/// needs to recognize C identifier boundaries so that, e.g., `FOObar` is not
/// mistaken for the macro `FOO`.
fn split_preproc_identifier_tokens(text: &str) -> Vec<PreprocToken<'_>> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if is_identifier_start(bytes[index]) {
            let start = index;
            index += 1;
            while index < bytes.len() && is_identifier_continue(bytes[index]) {
                index += 1;
            }
            tokens.push(PreprocToken::Identifier(&text[start..index]));
        } else {
            let start = index;
            index += 1;
            while index < bytes.len() && !is_identifier_start(bytes[index]) {
                index += 1;
            }
            tokens.push(PreprocToken::Other(&text[start..index]));
        }
    }
    tokens
}

fn is_identifier_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

fn is_identifier_continue(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

/// Re-parses an already-macro-expanded condition expression and evaluates it
/// with the shared preprocessor evaluator. The expanded text no longer contains
/// object-like macro references, so a fresh `MacroSymbols` is unnecessary here;
/// `symbols` is still passed through so any residual `defined(...)` checks behave
/// consistently. Returns `None` if tree-sitter cannot produce a usable
/// condition node.
fn eval_condition_text(text: &str, symbols: &MacroSymbols) -> Option<i64> {
    let directive = format!("#if {text}\n#endif\n");
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_c::LANGUAGE.into()).ok()?;
    let tree = parser.parse(&directive, None)?;
    let bytes = directive.as_bytes();
    let condition = named_children(tree.root_node())
        .into_iter()
        .find(|node| node.kind() == "preproc_if")
        .and_then(|node| node.child_by_field_name("condition"))?;
    Some(eval_preproc_condition(condition, bytes, symbols))
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
    let trimmed = value.trim().trim_end_matches(['u', 'U', 'l', 'L']);
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
        Some((head, "")) => (head.trim().to_string(), format!("#define {}", head.trim())),
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
            .filter_map(|field| {
                parse_operator_cast(*field, source, false, symbols)
                    .or_else(|| parse_function_declaration(*field, source))
            })
            .map(Declaration::Function),
    );
    nested_declarations.extend(
        named_children(body)
            .into_iter()
            .filter(|child| child.kind() == "declaration")
            .filter_map(|declaration| {
                parse_function_declaration(declaration, source)
                    .or_else(|| parse_operator_cast(declaration, source, false, symbols))
            })
            .map(Declaration::Function),
    );
    nested_declarations.extend(
        named_children(body)
            .into_iter()
            .filter(|child| child.kind() == "function_definition")
            .filter_map(|function| {
                parse_operator_cast(function, source, true, symbols)
                    .or_else(|| parse_function(function, source, symbols))
            })
            .map(Declaration::Function),
    );
    nested_declarations.extend(
        named_children(body)
            .into_iter()
            .filter(|child| {
                matches!(
                    child.kind(),
                    "operator_cast_definition" | "operator_cast_declaration"
                )
            })
            .filter_map(|function| {
                parse_operator_cast(
                    function,
                    source,
                    function.kind() == "operator_cast_definition",
                    symbols,
                )
            })
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
    nested_declarations.extend(
        named_children(body)
            .into_iter()
            .filter(|child| child.kind() == "template_declaration")
            .flat_map(|template| parse_nested_template_functions(template, source, symbols))
            .map(Declaration::Function),
    );
    let base_class_declarations = parse_base_class_declarations(node, source);
    Some(StructDecl {
        name,
        code: compact_code(node_text(node, source)),
        line: line(node),
        source_path: None,
        visible_line: None,
        base_classes: base_class_declarations
            .iter()
            .map(|base| base.name.clone())
            .collect(),
        base_class_declarations,
        using_declarations: parse_using_declarations(body, source),
        fields: field_nodes
            .into_iter()
            .filter_map(|field| parse_field(field, source))
            .collect(),
        nested_declarations,
    })
}

fn parse_using_declarations(node: Node, source: &[u8]) -> Vec<UsingDecl> {
    named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "using_declaration")
        .filter_map(|child| parse_using_declaration(child, source))
        .collect()
}

fn parse_using_declaration(node: Node, source: &[u8]) -> Option<UsingDecl> {
    let code = node_text(node, source).trim().trim_end_matches(';').trim();
    let target = code.strip_prefix("using ")?.trim().to_string();
    if target.is_empty() || target.contains('=') || target.starts_with("namespace ") {
        return None;
    }
    let name = target
        .split("::")
        .last()
        .map(str::trim)
        .filter(|name| !name.is_empty())?
        .to_string();
    Some(UsingDecl {
        name,
        target,
        code: code.to_string(),
        line: line(node),
    })
}

fn parse_nested_template_functions(
    node: Node,
    source: &[u8],
    symbols: &mut MacroSymbols,
) -> Vec<FunctionDecl> {
    named_children(node)
        .into_iter()
        .flat_map(|child| match child.kind() {
            "declaration" => parse_function_declaration(child, source)
                .or_else(|| parse_operator_cast(child, source, false, symbols))
                .into_iter()
                .collect(),
            "function_definition" => parse_operator_cast(child, source, true, symbols)
                .or_else(|| parse_function(child, source, symbols))
                .into_iter()
                .collect(),
            "operator_cast_definition" => parse_operator_cast(child, source, true, symbols)
                .into_iter()
                .collect(),
            "operator_cast_declaration" => parse_operator_cast(child, source, false, symbols)
                .into_iter()
                .collect(),
            "constructor_or_destructor_definition" => {
                parse_constructor_or_destructor(child, source, true, symbols)
                    .into_iter()
                    .collect()
            }
            "constructor_or_destructor_declaration" => {
                parse_constructor_or_destructor(child, source, false, symbols)
                    .into_iter()
                    .collect()
            }
            "template_declaration" => parse_nested_template_functions(child, source, symbols),
            _ => Vec::new(),
        })
        .collect()
}

fn parse_base_class_declarations(node: Node, source: &[u8]) -> Vec<BaseClassDecl> {
    named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "base_class_clause")
        .flat_map(|base_clause| {
            let text = node_text(base_clause, source);
            split_top_level_base_classes(text.trim().trim_start_matches(':'))
                .into_iter()
                .filter_map(move |base| parse_base_class_declaration(base, line(base_clause)))
        })
        .collect()
}

fn parse_base_class_declaration(base: &str, line: usize) -> Option<BaseClassDecl> {
    let code = base.trim();
    if code.is_empty() {
        return None;
    }
    let tokens = code.split_whitespace().collect::<Vec<_>>();
    let is_virtual = tokens.contains(&"virtual");
    let name = normalize_type(
        &tokens
            .into_iter()
            .filter(|token| !BASE_CLASS_SPECIFIERS.contains(token))
            .collect::<Vec<_>>()
            .join(" "),
    );
    if name.is_empty() {
        return None;
    }
    Some(BaseClassDecl {
        name,
        code: code.to_string(),
        is_virtual,
        line,
    })
}

fn split_top_level_base_classes(base_classes: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    for (index, ch) in base_classes.char_indices() {
        match ch {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' if depth > 0 => depth -= 1,
            ',' if depth == 0 => {
                let base = base_classes[start..index].trim();
                if !base.is_empty() {
                    result.push(base);
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    let base = base_classes[start..].trim();
    if !base.is_empty() {
        result.push(base);
    }
    result
}

const BASE_CLASS_SPECIFIERS: &[&str] = &["public", "protected", "private", "virtual"];

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
            semantic_type_name: type_name.clone(),
            type_name,
            code: code.to_string(),
            is_static: is_static_field(node, source),
            initializer: field_initializer(node, source),
            resolved_type_full_name: None,
        });
    }
    let (type_name, name) =
        declaration_type_and_name(node, source).or_else(|| split_type_and_name(code))?;
    let semantic_type_name = declaration_semantic_type_and_name(node, source)
        .map(|(type_name, _)| type_name)
        .or_else(|| {
            split_type_and_name_with_declarator_preserving_cv(code).map(|(type_name, _)| type_name)
        })
        .or_else(|| split_type_and_name_preserving_cv(code).map(|(type_name, _)| type_name))
        .unwrap_or_else(|| type_name.clone());
    Some(FieldDecl {
        name,
        type_name,
        semantic_type_name,
        code: code.to_string(),
        is_static: is_static_field(node, source),
        initializer: field_initializer(node, source),
        resolved_type_full_name: None,
    })
}

fn field_initializer(node: Node, source: &[u8]) -> Option<Expression> {
    if let Some(default_value) = node.child_by_field_name("default_value") {
        return Some(parse_expression(default_value, source));
    }
    let declarator = node.child_by_field_name("declarator")?;
    declarator
        .child_by_field_name("value")
        .map(|value| parse_expression(value, source))
        .or_else(|| direct_initializer_from_declarator(declarator, source))
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
    let semantic_base_type = declaration_base_type_from_first_declarator_with_normalizer(
        node,
        type_node,
        source,
        normalize_type_preserving_cv,
    );
    named_children(node)
        .into_iter()
        .filter(|child| *child != type_node)
        .filter(|declarator| !is_function_prototype_declarator(*declarator))
        .filter_map(|declarator| {
            let name = declarator_name(declarator, source)?;
            Some(GlobalVariableDecl {
                name,
                type_name: type_from_declarator(&base_type, declarator, source),
                semantic_type_name: type_from_declarator(&semantic_base_type, declarator, source),
                code: variable_declaration_code(node, declarator, source),
                line: line(declarator),
                source_path: None,
                visible_line: None,
                initializer: declarator
                    .child_by_field_name("value")
                    .map(|value| parse_expression(value, source)),
                resolved_type_full_name: None,
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
    let return_type = function_return_type(type_node, declarator, function_declarator, source);
    let semantic_return_type =
        semantic_function_return_type(node, type_node, declarator, function_declarator, source);
    let parameters = function_declarator
        .child_by_field_name("parameters")
        .map(|params| parse_parameters(params, source))
        .unwrap_or_default();
    let constructor_initializers = parse_constructor_initializers(node, source);
    let is_const = if is_conversion_operator_name(&name) {
        is_const_operator_cast(declarator, function_declarator, source)
    } else {
        is_const_function_declarator(function_declarator, source)
    };
    let is_virtual = is_virtual_function(node, declarator, source);
    Some(FunctionDecl {
        name,
        signature: function_signature(&return_type, &parameters, is_const),
        return_type,
        semantic_return_type,
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
    if is_operator_cast_declaration_node(node) {
        return None;
    }
    if !is_function_prototype_declarator(declarator) {
        return None;
    }
    let name = declarator_name(declarator, source)?;
    let function_declarator = function_declarator_node(declarator).unwrap_or(declarator);
    let return_type = function_return_type(type_node, declarator, function_declarator, source);
    let semantic_return_type =
        semantic_function_return_type(node, type_node, declarator, function_declarator, source);
    let parameters = function_declarator
        .child_by_field_name("parameters")
        .map(|params| parse_parameters(params, source))
        .unwrap_or_default();
    let is_const = if is_conversion_operator_name(&name) {
        is_const_operator_cast(declarator, function_declarator, source)
    } else {
        is_const_function_declarator(function_declarator, source)
    };
    let is_virtual = is_virtual_function(node, declarator, source);
    Some(FunctionDecl {
        name,
        signature: function_signature(&return_type, &parameters, is_const),
        return_type,
        semantic_return_type,
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

fn parse_operator_cast(
    node: Node,
    source: &[u8],
    is_definition: bool,
    symbols: &mut MacroSymbols,
) -> Option<FunctionDecl> {
    let declarator = node.child_by_field_name("declarator")?;
    let operator_cast = find_named_descendant_kind(declarator, "operator_cast")
        .or_else(|| (declarator.kind() == "operator_cast").then_some(declarator))?;
    let return_type = operator_cast_return_type(operator_cast, source)?;
    let semantic_return_type = operator_cast_semantic_return_type(operator_cast, source)?;
    let name = operator_cast_name(declarator, &return_type, source);
    let function_declarator = function_declarator_node(operator_cast).unwrap_or(operator_cast);
    let parameters = function_declarator
        .child_by_field_name("parameters")
        .map(|params| parse_parameters(params, source))
        .unwrap_or_default();
    let body = node
        .child_by_field_name("body")
        .map(|body| parse_statement_block(body, source, symbols))
        .unwrap_or_default();
    let is_const = is_const_operator_cast(operator_cast, function_declarator, source);
    let is_virtual = is_virtual_function(node, declarator, source);
    Some(FunctionDecl {
        name,
        signature: function_signature(&return_type, &parameters, is_const),
        return_type,
        semantic_return_type,
        is_definition: is_definition && node.child_by_field_name("body").is_some(),
        is_static: false,
        is_const,
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
        constructor_initializers: Vec::new(),
        body,
    })
}

fn is_operator_cast_declaration_node(node: Node) -> bool {
    matches!(
        node.kind(),
        "operator_cast" | "operator_cast_declaration" | "operator_cast_definition"
    ) || find_named_descendant_kind(node, "operator_cast").is_some()
}

fn is_conversion_operator_name(name: &str) -> bool {
    name.contains("operator ")
}

fn operator_cast_return_type(operator_cast: Node, source: &[u8]) -> Option<String> {
    operator_cast_return_type_with_normalizer(operator_cast, source, normalize_type)
}

fn operator_cast_semantic_return_type(operator_cast: Node, source: &[u8]) -> Option<String> {
    operator_cast_return_type_with_normalizer(operator_cast, source, normalize_type_preserving_cv)
}

fn operator_cast_return_type_with_normalizer(
    operator_cast: Node,
    source: &[u8],
    normalizer: fn(&str) -> String,
) -> Option<String> {
    let type_node = operator_cast.child_by_field_name("type")?;
    let declarator = operator_cast.child_by_field_name("declarator");
    let base_type = operator_cast_base_type_with_normalizer(
        operator_cast,
        type_node,
        declarator,
        source,
        normalizer,
    );
    declarator
        .map(|declarator| type_from_declarator(&base_type, declarator, source))
        .or(Some(base_type))
}

fn operator_cast_base_type_with_normalizer(
    operator_cast: Node,
    type_node: Node,
    declarator: Option<Node>,
    source: &[u8],
    normalizer: fn(&str) -> String,
) -> String {
    let end_byte = declarator
        .map(|node| node.start_byte())
        .unwrap_or(type_node.end_byte());
    std::str::from_utf8(&source[operator_cast.start_byte()..end_byte])
        .ok()
        .and_then(|raw| raw.trim().strip_prefix("operator").map(str::trim))
        .map(normalizer)
        .filter(|base_type| !base_type.is_empty())
        .unwrap_or_else(|| type_name_from_type_node_with_normalizer(type_node, source, normalizer))
}

fn operator_cast_name(declarator: Node, return_type: &str, source: &[u8]) -> String {
    let code = node_text(declarator, source).trim();
    if let Some(operator_index) = code.find("operator") {
        let owner = code[..operator_index].trim_end_matches("::").trim();
        if owner.is_empty() {
            format!("operator {return_type}")
        } else {
            format!("{owner}::operator {return_type}")
        }
    } else {
        format!("operator {return_type}")
    }
}

fn parse_requires_expression_declarations(node: Node, source: &[u8]) -> Vec<FunctionDecl> {
    named_descendants(node)
        .into_iter()
        .filter(|descendant| descendant.kind() == "requires_expression")
        .map(|requires_expression| {
            let return_type = "requires".to_string();
            let parameters = requires_expression
                .child_by_field_name("parameters")
                .map(|parameters| parse_parameters(parameters, source))
                .unwrap_or_default();
            FunctionDecl {
                name: "requires".to_string(),
                signature: signature(&return_type, &parameters),
                semantic_return_type: return_type.clone(),
                return_type,
                is_definition: false,
                is_static: false,
                is_const: false,
                is_virtual: false,
                code: compact_code(node_text(requires_expression, source)),
                line: line(requires_expression),
                source_path: None,
                visible_line: None,
                parameters,
                constructor_initializers: Vec::new(),
                body: Vec::new(),
            }
        })
        .collect()
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
        semantic_return_type: return_type.clone(),
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

fn is_const_operator_cast(operator_cast: Node, function_declarator: Node, source: &[u8]) -> bool {
    let parameters_end = function_declarator
        .child_by_field_name("parameters")
        .or_else(|| find_named_descendant_kind(function_declarator, "parameter_list"))
        .map(|parameters| parameters.end_byte())
        .unwrap_or(function_declarator.end_byte());
    has_type_qualifier_after(operator_cast, "const", parameters_end, source)
}

fn has_type_qualifier_after(
    node: Node,
    qualifier: &str,
    min_start_byte: usize,
    source: &[u8],
) -> bool {
    named_children(node).into_iter().any(|child| {
        child.kind() == "type_qualifier"
            && child.start_byte() >= min_start_byte
            && node_text(child, source)
                .split_whitespace()
                .any(|token| token == qualifier)
            || has_type_qualifier_after(child, qualifier, min_start_byte, source)
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

fn function_return_type(
    type_node: Option<Node>,
    declarator: Node,
    function_declarator: Node,
    source: &[u8],
) -> String {
    function_return_type_with_normalizer(
        type_node,
        declarator,
        function_declarator,
        source,
        normalize_type,
    )
}

fn function_return_type_with_normalizer(
    type_node: Option<Node>,
    declarator: Node,
    function_declarator: Node,
    source: &[u8],
    normalizer: fn(&str) -> String,
) -> String {
    trailing_return_type_with_normalizer(function_declarator, source, normalizer)
        .or_else(|| {
            type_node.map(|type_node| {
                type_from_declarator(
                    &type_name_from_type_node_with_normalizer(type_node, source, normalizer),
                    declarator,
                    source,
                )
            })
        })
        .unwrap_or_else(|| "void".to_string())
}

fn semantic_function_return_type(
    declaration: Node,
    type_node: Option<Node>,
    declarator: Node,
    function_declarator: Node,
    source: &[u8],
) -> String {
    trailing_return_type_with_normalizer(function_declarator, source, normalize_type_preserving_cv)
        .or_else(|| {
            type_node.map(|type_node| {
                let base_type = declaration_base_type_with_normalizer(
                    declaration,
                    type_node,
                    declarator,
                    source,
                    normalize_function_return_type_preserving_cv,
                );
                type_from_declarator(&base_type, declarator, source)
            })
        })
        .unwrap_or_else(|| "void".to_string())
}

fn trailing_return_type_with_normalizer(
    declarator: Node,
    source: &[u8],
    normalizer: fn(&str) -> String,
) -> Option<String> {
    find_named_descendant_kind(declarator, "trailing_return_type")
        .and_then(|trailing_return| {
            named_children(trailing_return)
                .into_iter()
                .find(|child| child.kind() == "type_descriptor")
        })
        .map(|type_node| {
            type_name_from_type_descriptor_with_normalizer(type_node, source, normalizer)
        })
}

fn parse_parameters(node: Node, source: &[u8]) -> Vec<ParameterDecl> {
    parse_parameters_with_varargs(node, source, true)
}

fn parse_parameters_without_varargs(node: Node, source: &[u8]) -> Vec<ParameterDecl> {
    parse_parameters_with_varargs(node, source, false)
}

fn parse_parameters_with_varargs(
    node: Node,
    source: &[u8],
    include_varargs: bool,
) -> Vec<ParameterDecl> {
    let mut parameters = Vec::new();
    let mut has_classic_varargs_parameter = false;
    for (index, parameter) in named_children(node)
        .into_iter()
        .filter(|child| {
            matches!(
                child.kind(),
                "parameter_declaration"
                    | "optional_parameter_declaration"
                    | "variadic_parameter_declaration"
            )
        })
        .enumerate()
    {
        let code = node_text(parameter, source).trim();
        if code == "void" {
            continue;
        }
        let parsed = declaration_type_and_name(parameter, source)
            .or_else(|| split_type_and_name(code))
            .or_else(|| {
                parameter_type_without_name(parameter, source)
                    .map(|type_name| (type_name, format!("param{}", index + 1)))
            });
        let semantic_parsed = declaration_semantic_type_and_name(parameter, source)
            .or_else(|| split_type_and_name_preserving_cv(code))
            .or_else(|| {
                parameter_type_without_name_preserving_cv(parameter, source)
                    .map(|type_name| (type_name, format!("param{}", index + 1)))
            });
        let is_pack = parameter.kind() == "variadic_parameter_declaration"
            && parsed
                .as_ref()
                .is_some_and(|(_, name)| code.contains(&format!("... {name}")));
        let is_classic_varargs = parameter.kind() == "variadic_parameter_declaration"
            && !is_pack
            && code.ends_with("...");
        let Some((type_name, semantic_type_name, name, code, is_variadic)) = (if is_classic_varargs
        {
            has_classic_varargs_parameter = true;
            let parameter_code = code.trim_end_matches("...").trim();
            split_type_and_name_with_declarator(parameter_code).map(|(type_name, name)| {
                let semantic_type_name =
                    split_type_and_name_with_declarator_preserving_cv(parameter_code)
                        .map(|(semantic_type_name, _)| semantic_type_name)
                        .unwrap_or_else(|| type_name.clone());
                (
                    type_name,
                    semantic_type_name,
                    name,
                    parameter_code.to_string(),
                    false,
                )
            })
        } else {
            parsed.map(|(type_name, name)| {
                let semantic_type_name = semantic_parsed
                    .as_ref()
                    .map(|(semantic_type_name, _)| semantic_type_name.clone())
                    .unwrap_or_else(|| type_name.clone());
                (
                    type_name,
                    semantic_type_name,
                    name,
                    code.to_string(),
                    parameter.kind() == "variadic_parameter_declaration",
                )
            })
        }) else {
            continue;
        };
        parameters.push(ParameterDecl {
            name,
            type_name,
            semantic_type_name,
            is_variadic,
            has_default: parameter.kind() == "optional_parameter_declaration"
                || parameter.child_by_field_name("default_value").is_some(),
            code,
            line: line(parameter),
            resolved_type_full_name: None,
        });
    }
    if include_varargs
        && (parameter_list_has_varargs(node, source) || has_classic_varargs_parameter)
    {
        let index = parameters.len() + 1;
        let type_name = parameters
            .last()
            .map(|parameter| parameter.type_name.clone())
            .unwrap_or_else(|| "ANY".to_string());
        let line = parameters
            .last()
            .map(|parameter| parameter.line)
            .unwrap_or_else(|| line(node));
        let name = format!("<param>{index}");
        parameters.push(ParameterDecl {
            name: name.clone(),
            type_name,
            semantic_type_name: parameters
                .last()
                .map(|parameter| parameter.semantic_type_name.clone())
                .unwrap_or_else(|| "ANY".to_string()),
            is_variadic: true,
            has_default: false,
            code: format!("{name}..."),
            line,
            resolved_type_full_name: None,
        });
    }
    parameters
}

fn parameter_list_has_varargs(node: Node, source: &[u8]) -> bool {
    let has_standalone_ellipsis = (0..node.child_count()).any(|index| {
        node.child(index as u32)
            .is_some_and(|child| !child.is_named() && node_text(child, source).trim() == "...")
    });
    let has_parameter_suffix_ellipsis = named_children(node)
        .into_iter()
        .rfind(|child| {
            matches!(
                child.kind(),
                "parameter_declaration" | "variadic_parameter_declaration"
            )
        })
        .is_some_and(|last_parameter| {
            last_parameter.kind() != "variadic_parameter_declaration"
                && std::str::from_utf8(&source[last_parameter.end_byte()..node.end_byte()])
                    .ok()
                    .is_some_and(|suffix| suffix.contains("..."))
        });
    has_standalone_ellipsis || has_parameter_suffix_ellipsis
}

fn parameter_type_without_name(node: Node, source: &[u8]) -> Option<String> {
    parameter_type_without_name_with_normalizer(node, source, normalize_type)
}

fn parameter_type_without_name_preserving_cv(node: Node, source: &[u8]) -> Option<String> {
    parameter_type_without_name_with_normalizer(node, source, normalize_type_preserving_cv)
}

fn parameter_type_without_name_with_normalizer(
    node: Node,
    source: &[u8],
    normalizer: fn(&str) -> String,
) -> Option<String> {
    let type_node = node.child_by_field_name("type")?;
    Some(
        node.child_by_field_name("declarator")
            .map(|declarator| {
                let base_type = declaration_base_type_with_normalizer(
                    node, type_node, declarator, source, normalizer,
                );
                type_from_declarator(&base_type, declarator, source)
            })
            .unwrap_or_else(|| normalizer(node_text(node, source))),
    )
}

fn parse_statement_block(node: Node, source: &[u8], symbols: &mut MacroSymbols) -> Vec<Statement> {
    named_children(node)
        .into_iter()
        .flat_map(|child| parse_statement(child, source, symbols))
        .collect()
}

fn parse_statement(node: Node, source: &[u8], symbols: &mut MacroSymbols) -> Vec<Statement> {
    if let Some(statement) = coroutine_statement(node, source) {
        return vec![statement];
    }
    if let Some(statement) = using_enum_statement(node, source) {
        return vec![statement];
    }
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
        "seh_try_statement" => parse_seh_try_statement(node, source, symbols)
            .into_iter()
            .collect(),
        "expression_statement" => named_children(node)
            .into_iter()
            .next()
            .map(|expr| statement_from_expression(node, expr, source))
            .into_iter()
            .collect(),
        "attributed_statement" => parse_attributed_statement(node, source, symbols),
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
        "for_range_loop" => parse_for_range_loop(node, source, symbols)
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
        _ => {
            record_unmapped_kind(node.kind());
            vec![Statement::Expression {
                code: statement_code(node, source),
                line: line(node),
                expression: parse_expression(node, source),
            }]
        }
    }
}

fn coroutine_statement(node: Node, source: &[u8]) -> Option<Statement> {
    let code = statement_code(node, source);
    let normalized = code.trim_end_matches(';').trim().to_string();
    let line = line(node);
    if let Some(operand) = coroutine_keyword_operand(&normalized, "co_return") {
        Some(Statement::Return {
            code,
            line,
            expression: if operand.is_empty() {
                None
            } else {
                Some(parse_expression_text(operand, line))
            },
        })
    } else {
        coroutine_expression(&normalized, line).map(|expression| Statement::Expression {
            code,
            line,
            expression,
        })
    }
}

fn coroutine_expression(code: &str, line: usize) -> Option<Expression> {
    ["co_await", "co_yield"]
        .iter()
        .find_map(|keyword| coroutine_unary_expression(code, keyword, line))
}

fn coroutine_unary_expression(code: &str, keyword: &str, line: usize) -> Option<Expression> {
    let operand = coroutine_keyword_operand(code, keyword)?;
    if operand.is_empty() {
        return None;
    }
    Some(Expression::Unary {
        operator: keyword.to_string(),
        code: code.to_string(),
        line,
        prefix: true,
        argument: Box::new(parse_expression_text(operand, line)),
    })
}

fn coroutine_keyword_operand<'a>(code: &'a str, keyword: &str) -> Option<&'a str> {
    let code = code.trim();
    if code == keyword {
        Some("")
    } else {
        code.strip_prefix(&format!("{keyword} ")).map(str::trim)
    }
}

fn using_enum_statement(node: Node, source: &[u8]) -> Option<Statement> {
    let code = node_text(node, source).trim();
    let normalized = code.trim_end_matches(';').trim();
    normalized
        .strip_prefix("using enum ")
        .map(str::trim)
        .filter(|type_name| !type_name.is_empty())
        .map(|type_name| Statement::UsingEnum {
            type_name: normalize_type(type_name),
            code: statement_code(node, source),
            line: line(node),
        })
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

/// Lowers an MSVC structured-exception `__try`/`__except`/`__finally` statement
/// onto the existing `try` shape: the guarded block becomes the body, each
/// `__except` filter becomes a (parameterless) catch clause, and `__finally`
/// statements are appended to the body since they always execute. No new JSON
/// kind is introduced.
fn parse_seh_try_statement(
    node: Node,
    source: &[u8],
    symbols: &mut MacroSymbols,
) -> Option<Statement> {
    let body_node = node
        .child_by_field_name("body")
        .or_else(|| named_children(node).into_iter().find(is_compound_statement))?;
    let mut body = parse_statement(body_node, source, symbols);
    let mut catches = Vec::new();
    for child in named_children(node) {
        match child.kind() {
            "seh_except_clause" => {
                let except_body = child
                    .child_by_field_name("body")
                    .or_else(|| {
                        named_children(child)
                            .into_iter()
                            .find(is_compound_statement)
                    })
                    .map(|body| parse_statement(body, source, symbols))
                    .unwrap_or_default();
                catches.push(CatchClause {
                    code: statement_code(child, source),
                    line: line(child),
                    parameter: None,
                    body: except_body,
                });
            }
            "seh_finally_clause" => {
                if let Some(finally_body) = child.child_by_field_name("body").or_else(|| {
                    named_children(child)
                        .into_iter()
                        .find(is_compound_statement)
                }) {
                    body.extend(parse_statement(finally_body, source, symbols));
                }
            }
            _ => {}
        }
    }
    Some(Statement::Try {
        code: statement_code(node, source),
        line: line(node),
        body,
        catches,
    })
}

fn is_compound_statement(node: &Node) -> bool {
    node.kind() == "compound_statement"
}

fn parse_catch_clause(node: Node, source: &[u8], symbols: &mut MacroSymbols) -> CatchClause {
    let parameter = node
        .child_by_field_name("parameters")
        .and_then(|parameters| {
            parse_parameters_without_varargs(parameters, source)
                .into_iter()
                .next()
        });
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

fn parse_attributed_statement(
    node: Node,
    source: &[u8],
    symbols: &mut MacroSymbols,
) -> Vec<Statement> {
    let Some(statement_node) = named_children(node).into_iter().last() else {
        return Vec::new();
    };
    let attribute_prefix =
        std::str::from_utf8(&source[node.start_byte()..statement_node.start_byte()])
            .unwrap_or("")
            .trim();
    parse_statement(statement_node, source, symbols)
        .into_iter()
        .map(|statement| statement_with_attribute_prefix(statement, attribute_prefix))
        .collect()
}

fn statement_with_attribute_prefix(statement: Statement, attribute_prefix: &str) -> Statement {
    if attribute_prefix.is_empty() {
        return statement;
    }
    match statement {
        Statement::Case {
            code,
            line,
            value,
            body,
        } => Statement::Case {
            code: format!("{attribute_prefix} {code}"),
            line,
            value,
            body,
        },
        _ => statement,
    }
}

fn parse_local_declarations(node: Node, source: &[u8]) -> Vec<Statement> {
    let type_node = node.child_by_field_name("type");
    let type_name = type_node.map(|type_node| type_name_from_type_node(type_node, source));
    named_children(node)
        .into_iter()
        .filter(|child| Some(*child) != type_node)
        .flat_map(|declarator| {
            if let (Some(base_type), Some(binding_declarator)) = (
                type_name.as_deref(),
                structured_binding_declarator(declarator),
            ) {
                return parse_structured_binding(
                    node,
                    declarator,
                    binding_declarator,
                    base_type,
                    source,
                )
                .into_iter()
                .collect();
            }
            let Some(name) = declarator_name(declarator, source) else {
                return Vec::new();
            };
            let initializer = declarator
                .child_by_field_name("value")
                .map(|value| parse_expression(value, source))
                .or_else(|| direct_initializer_from_declarator(declarator, source));
            let type_name = type_name
                .as_deref()
                .map(|base_type| type_from_declarator(base_type, declarator, source))
                .unwrap_or_else(|| {
                    split_type_and_name(node_text(node, source))
                        .map(|(type_name, _)| type_name)
                        .unwrap_or_default()
                });
            let semantic_type_name = type_node
                .map(|type_node| {
                    let base_type = declaration_base_type_with_normalizer(
                        node,
                        type_node,
                        declarator,
                        source,
                        normalize_type_preserving_cv,
                    );
                    type_from_declarator(&base_type, declarator, source)
                })
                .unwrap_or_else(|| {
                    split_type_and_name_preserving_cv(node_text(node, source))
                        .map(|(type_name, _)| type_name)
                        .unwrap_or_else(|| type_name.clone())
                });
            Some(Statement::LocalDecl {
                name,
                type_name,
                semantic_type_name,
                code: statement_code(node, source),
                line: line(node),
                initializer,
                resolved_type_full_name: None,
            })
            .into_iter()
            .collect::<Vec<_>>()
        })
        .collect()
}

fn parse_structured_binding(
    statement: Node,
    declarator: Node,
    binding_declarator: Node,
    base_type: &str,
    source: &[u8],
) -> Option<Statement> {
    let names = structured_binding_names(binding_declarator, source);
    if names.is_empty() {
        return None;
    }
    Some(Statement::StructuredBinding {
        type_name: type_from_declarator(base_type, declarator, source),
        code: statement_code(statement, source),
        line: line(statement),
        temp_name: structured_binding_temp_name(binding_declarator),
        names,
        initializer: declarator
            .child_by_field_name("value")
            .map(|value| parse_expression(value, source)),
    })
}

fn structured_binding_declarator(node: Node) -> Option<Node> {
    if node.kind() == "structured_binding_declarator" {
        Some(node)
    } else {
        named_children(node)
            .into_iter()
            .find_map(structured_binding_declarator)
    }
}

fn structured_binding_names(node: Node, source: &[u8]) -> Vec<String> {
    named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "identifier")
        .map(|identifier| node_text(identifier, source).trim().to_string())
        .collect()
}

fn structured_binding_temp_name(node: Node) -> String {
    format!("<tmp>{}", node.start_byte())
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
    let text = node_text(node, source).trim();
    if node.child_by_field_name("declarator").is_some() {
        return direct_initializer_call_expression(text, line(node));
    }
    if !is_simple_identifier(text) {
        return None;
    }
    Some(Expression::Identifier {
        name: text.to_string(),
        code: text.to_string(),
        line: line(node),
    })
}

fn direct_initializer_call_expression(text: &str, line: usize) -> Option<Expression> {
    match parse_expression_text(text, line) {
        expression @ Expression::Call { .. } => Some(expression),
        _ => None,
    }
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
    let condition_clause = parse_condition_clause(condition, source, symbols);
    let else_body = node
        .child_by_field_name("alternative")
        .map(|alternative| parse_statement(alternative, source, symbols))
        .unwrap_or_default();
    Some(Statement::If {
        code: statement_code(node, source),
        line: line(node),
        initializer: condition_clause.initializer,
        condition_initializer: condition_clause.condition_initializer,
        condition: condition_clause.condition,
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
    let condition_clause = parse_condition_clause(condition, source, symbols);
    Some(Statement::While {
        code: statement_code(node, source),
        line: line(node),
        initializer: condition_clause.initializer,
        condition_initializer: condition_clause.condition_initializer,
        condition: condition_clause.condition,
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

fn parse_for_range_loop(
    node: Node,
    source: &[u8],
    symbols: &mut MacroSymbols,
) -> Option<Statement> {
    let body = node.child_by_field_name("body")?;
    let type_node = node.child_by_field_name("type")?;
    let declarator = node.child_by_field_name("declarator")?;
    let right = node.child_by_field_name("right")?;
    let mut initializer = node
        .child_by_field_name("initializer")
        .map(|initializer| parse_for_initializer(initializer, source, symbols))
        .unwrap_or_default();
    let base_type = type_name_from_type_node(type_node, source);
    if let Some(binding_declarator) = structured_binding_declarator(declarator) {
        initializer.push(Statement::StructuredBinding {
            type_name: type_from_declarator(&base_type, declarator, source),
            code: node_text(declarator, source).trim().to_string(),
            line: line(declarator),
            temp_name: structured_binding_temp_name(binding_declarator),
            names: structured_binding_names(binding_declarator, source),
            initializer: Some(parse_expression(right, source)),
        });
    } else {
        let name = declarator_name(declarator, source)?;
        let semantic_base_type = type_name_from_type_node_preserving_cv(type_node, source);
        initializer.push(Statement::LocalDecl {
            name,
            type_name: type_from_declarator(&base_type, declarator, source),
            semantic_type_name: type_from_declarator(&semantic_base_type, declarator, source),
            code: node_text(declarator, source).trim().to_string(),
            line: line(declarator),
            initializer: None,
            resolved_type_full_name: None,
        });
    }
    Some(Statement::For {
        code: statement_code(node, source),
        line: line(node),
        initializer,
        condition: Some(parse_expression(right, source)),
        update: None,
        body: parse_statement(body, source, symbols),
    })
}

fn parse_for_initializer(node: Node, source: &[u8], symbols: &mut MacroSymbols) -> Vec<Statement> {
    match node.kind() {
        "init_statement" => named_children(node)
            .into_iter()
            .flat_map(|child| parse_for_initializer(child, source, symbols))
            .collect(),
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
    let condition_clause = parse_condition_clause(condition, source, symbols);
    Some(Statement::Switch {
        code: statement_code(node, source),
        line: line(node),
        initializer: condition_clause.initializer,
        condition_initializer: condition_clause.condition_initializer,
        condition: condition_clause.condition,
        body: flatten_switch_cases(parse_statement(body, source, symbols)),
    })
}

struct ConditionClause {
    initializer: Vec<Statement>,
    condition_initializer: Vec<Statement>,
    condition: Expression,
}

fn parse_condition_clause(
    node: Node,
    source: &[u8],
    symbols: &mut MacroSymbols,
) -> ConditionClause {
    if node.kind() != "condition_clause" {
        return ConditionClause {
            initializer: Vec::new(),
            condition_initializer: Vec::new(),
            condition: parse_expression(node, source),
        };
    }

    let children = named_children(node);
    if children.is_empty() {
        return ConditionClause {
            initializer: Vec::new(),
            condition_initializer: Vec::new(),
            condition: parse_expression(node, source),
        };
    }

    if condition_clause_has_semicolon(node, source) && children.len() >= 2 {
        let condition = *children.last().expect("condition child");
        let initializer = children[..children.len() - 1]
            .iter()
            .flat_map(|child| parse_condition_initializer(*child, source, symbols))
            .collect::<Vec<_>>();
        let (condition_initializer, condition) =
            parse_condition_component(condition, source, symbols);
        return ConditionClause {
            initializer,
            condition_initializer,
            condition,
        };
    }

    let child = children[0];
    let (condition_initializer, condition) = parse_condition_component(child, source, symbols);
    ConditionClause {
        initializer: Vec::new(),
        condition_initializer,
        condition,
    }
}

fn parse_condition_initializer(
    node: Node,
    source: &[u8],
    symbols: &mut MacroSymbols,
) -> Vec<Statement> {
    match node.kind() {
        "init_statement" => parse_for_initializer(node, source, symbols),
        "declaration" => parse_condition_declaration(node, source),
        "expression_statement" => named_children(node)
            .into_iter()
            .next()
            .map(|expr| statement_from_expression(node, expr, source))
            .into_iter()
            .collect(),
        _ => vec![statement_from_expression(node, node, source)],
    }
}

fn parse_condition_declaration(node: Node, source: &[u8]) -> Vec<Statement> {
    if let Some(binding) = condition_structured_binding_from_text(node, source) {
        return vec![binding];
    }
    let declarations = parse_local_declarations(node, source);
    let code = node_text(node, source).trim();
    let parsed_as_single_initialized_local = matches!(
        declarations.as_slice(),
        [Statement::LocalDecl {
            initializer: Some(_),
            ..
        }]
    );
    if !code.contains('=') || parsed_as_single_initialized_local {
        declarations
    } else {
        condition_declaration_from_text(node, source)
            .into_iter()
            .collect()
    }
}

fn condition_structured_binding_from_text(node: Node, source: &[u8]) -> Option<Statement> {
    let code = node_text(node, source).trim().trim_end_matches(';');
    let (left, right) = split_top_level_assignment(code)?;
    let open = left.find('[')?;
    let close = left[open + 1..].find(']')? + open + 1;
    let type_name = left[..open].trim();
    let names = left[open + 1..close]
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if type_name.is_empty() || names.is_empty() {
        return None;
    }
    Some(Statement::StructuredBinding {
        type_name: type_name.to_string(),
        code: code.to_string(),
        line: line(node),
        temp_name: format!("<tmp>{}", node.start_byte()),
        names,
        initializer: Some(parse_expression_text(right.trim(), line(node))),
    })
}

fn condition_declaration_from_text(node: Node, source: &[u8]) -> Option<Statement> {
    let code = node_text(node, source).trim().trim_end_matches(';');
    let (left, right) = split_top_level_assignment(code)?;
    let (type_name, name) =
        split_type_and_name_with_declarator(left).or_else(|| split_type_and_name(left))?;
    let semantic_type_name = split_type_and_name_with_declarator_preserving_cv(left)
        .or_else(|| split_type_and_name_preserving_cv(left))
        .map(|(type_name, _)| type_name)
        .unwrap_or_else(|| type_name.clone());
    Some(Statement::LocalDecl {
        name,
        type_name,
        semantic_type_name,
        code: code.to_string(),
        line: line(node),
        initializer: Some(parse_expression_text(right.trim(), line(node))),
        resolved_type_full_name: None,
    })
}

fn split_top_level_assignment(code: &str) -> Option<(&str, &str)> {
    split_top_level_assignment_operator(code).map(|(left, _, right)| (left, right))
}

fn split_top_level_assignment_operator(code: &str) -> Option<(&str, &str, &str)> {
    let mut depth = 0usize;
    for (index, ch) in code.char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth > 0 => depth -= 1,
            _ if depth == 0 => {
                let rest = &code[index..];
                if let Some(operator) = assignment_operator_at(rest) {
                    if operator == "=" && !is_standalone_assignment_equals(code, index) {
                        continue;
                    }
                    let left = code[..index].trim();
                    let right = code[index + operator.len()..].trim();
                    if !left.is_empty() && !right.is_empty() && !is_operator_function_prefix(left) {
                        return Some((left, operator, right));
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn assignment_operator_at(value: &str) -> Option<&'static str> {
    [
        "<<=", ">>=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "=",
    ]
    .into_iter()
    .find(|operator| value.starts_with(operator))
}

fn is_standalone_assignment_equals(code: &str, index: usize) -> bool {
    let rest = &code[index..];
    if rest.starts_with("==") || rest.starts_with("=>") {
        return false;
    }
    !matches!(
        previous_non_whitespace_char(code, index),
        Some('!' | '<' | '>' | '=')
    )
}

fn previous_non_whitespace_char(value: &str, index: usize) -> Option<char> {
    value[..index]
        .chars()
        .rev()
        .find(|character| !character.is_whitespace())
}

fn is_operator_function_prefix(value: &str) -> bool {
    value == "operator" || value.ends_with(".operator") || value.ends_with("::operator")
}

fn parse_condition_component(
    node: Node,
    source: &[u8],
    symbols: &mut MacroSymbols,
) -> (Vec<Statement>, Expression) {
    if matches!(node.kind(), "declaration" | "init_statement") {
        let declarations = parse_condition_initializer(node, source, symbols);
        let condition = condition_expression_from_initializer(&declarations)
            .unwrap_or_else(|| parse_expression(node, source));
        (declarations, condition)
    } else {
        (Vec::new(), parse_expression(node, source))
    }
}

fn condition_expression_from_initializer(initializer: &[Statement]) -> Option<Expression> {
    initializer
        .iter()
        .rev()
        .find_map(|statement| match statement {
            Statement::LocalDecl { name, line, .. } => Some(Expression::Identifier {
                name: name.clone(),
                code: name.clone(),
                line: *line,
            }),
            Statement::StructuredBinding {
                temp_name, line, ..
            } => Some(Expression::Identifier {
                name: temp_name.clone(),
                code: temp_name.clone(),
                line: *line,
            }),
            _ => None,
        })
}

fn condition_clause_has_semicolon(node: Node, source: &[u8]) -> bool {
    node_text(node, source).contains(';')
}

fn flatten_switch_cases(statements: Vec<Statement>) -> Vec<Statement> {
    let mut flattened = Vec::new();
    for statement in statements {
        flatten_switch_case_statement(statement, &mut flattened);
    }
    flattened
}

fn flatten_switch_case_statement(statement: Statement, flattened: &mut Vec<Statement>) {
    match statement {
        Statement::Case {
            code,
            line,
            value,
            body,
        } => {
            let mut case_body = Vec::new();
            let mut nested_cases = Vec::new();
            for child in body {
                match child {
                    nested @ Statement::Case { .. } => nested_cases.push(nested),
                    other => case_body.push(other),
                }
            }
            flattened.push(Statement::Case {
                code,
                line,
                value,
                body: case_body,
            });
            for nested in nested_cases {
                flatten_switch_case_statement(nested, flattened);
            }
        }
        other => flattened.push(other),
    }
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
    if let Some(expression) = coroutine_expression(node_text(node, source), line(node)) {
        return expression;
    }
    match node.kind() {
        "parenthesized_expression" | "condition_clause" => named_children(node)
            .into_iter()
            .next()
            .map(|child| parse_expression(child, source))
            .unwrap_or_else(|| parse_expression_text(node_text(node, source), line(node))),
        "identifier" | "this" => identifier_expression(node, source),
        "number_literal"
        | "char_literal"
        | "string_literal"
        | "raw_string_literal"
        | "concatenated_string"
        | "user_defined_literal"
        | "true"
        | "false"
        | "null" => Expression::Literal {
            value: node_text(node, source).to_string(),
            code: node_text(node, source).to_string(),
            line: line(node),
            resolved_type_full_name: None,
        },
        "binary_expression" => parse_binary_expression(node, source),
        "unary_expression" | "update_expression" | "pointer_expression" => {
            parse_unary_expression(node, source)
        }
        "conditional_expression" => parse_conditional_expression(node, source),
        "fold_expression" => parse_fold_expression(node, source),
        "parameter_pack_expansion" => parse_parameter_pack_expansion(node, source),
        "decltype" => parse_decltype_expression(node, source),
        "qualified_identifier" => parse_qualified_identifier_expression(node, source),
        "call_expression" => parse_call_expression(node, source),
        "compound_literal_expression" => parse_compound_literal_expression(node, source),
        "field_expression" => parse_field_expression(node, source),
        "subscript_expression" => parse_subscript_expression(node, source),
        "assignment_expression" => parse_assignment_expression(node, source),
        "cast_expression" => parse_cast_expression(node, source),
        "sizeof_expression" | "alignof_expression" => parse_sizeof_expression(node, source),
        "offsetof_expression" => parse_offsetof_expression(node, source),
        "new_expression" => parse_new_expression(node, source),
        "delete_expression" => parse_delete_expression(node, source),
        "lambda_expression" => parse_lambda_expression(node, source),
        "argument_list" => parse_initializer_list(node, source),
        "initializer_list" => parse_initializer_list(node, source),
        "initializer_pair" => parse_initializer_pair(node, source),
        "comma_expression" => parse_comma_expression(node, source),
        _ => {
            record_unmapped_kind(node.kind());
            identifier_expression(node, source)
        }
    }
}

/// Lowers a C/C++ comma operator (`a, b, c`) onto a left-associative chain of
/// binary `,` expressions, reusing the existing `binary` JSON kind rather than
/// collapsing the whole thing into a single bogus identifier.
fn parse_comma_expression(node: Node, source: &[u8]) -> Expression {
    let mut operands = named_children(node)
        .into_iter()
        .map(|child| parse_expression(child, source));
    let Some(first) = operands.next() else {
        return identifier_expression(node, source);
    };
    operands.fold(first, |left, right| Expression::Binary {
        operator: ",".to_string(),
        code: node_text(node, source).trim().to_string(),
        line: line(node),
        left: Box::new(left),
        right: Box::new(right),
    })
}

fn parse_binary_expression(node: Node, source: &[u8]) -> Expression {
    let operator = operator_text(node, source).unwrap_or("?");
    parse_binary_like_expression(node, source, operator)
}

fn parse_expression_text(raw: &str, line: usize) -> Expression {
    let code = strip_wrapping_parentheses(raw.trim());
    if let Some(expression) = coroutine_expression(code, line) {
        return expression;
    }
    if let Some((left, operator, right)) = split_top_level_assignment_operator(code) {
        if is_initializer_designator_text(left) {
            return Expression::DesignatedInitializer {
                code: code.to_string(),
                line,
                designator: Box::new(Expression::Designator {
                    name: left.trim().to_string(),
                    code: left.trim().to_string(),
                    line,
                }),
                value: Box::new(parse_expression_text(right, line)),
            };
        }
        return Expression::Assignment {
            operator: operator.to_string(),
            code: code.to_string(),
            line,
            left: Box::new(parse_expression_text(left, line)),
            right: Box::new(parse_expression_text(right, line)),
        };
    }
    if let Some(initializer_list) = parse_initializer_list_text(code, line) {
        return initializer_list;
    }
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
            resolved_method_full_name: None,
            resolved_signature: None,
        }
    } else if literal_text_value(code) {
        Expression::Literal {
            value: code.to_string(),
            code: code.to_string(),
            line,
            resolved_type_full_name: None,
        }
    } else {
        Expression::Identifier {
            name: code.to_string(),
            code: code.to_string(),
            line,
        }
    }
}

fn parse_initializer_list_text(code: &str, line: usize) -> Option<Expression> {
    let trimmed = code.trim();
    if !(trimmed.starts_with('{') && trimmed.ends_with('}')) {
        return None;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    if !delimiters_are_balanced(inner) {
        return None;
    }
    Some(Expression::InitializerList {
        code: trimmed.to_string(),
        line,
        elements: split_top_level_arguments(inner)
            .into_iter()
            .map(|element| parse_expression_text(element, line))
            .collect(),
    })
}

fn delimiters_are_balanced(value: &str) -> bool {
    let mut stack = Vec::new();
    for ch in value.chars() {
        match ch {
            '(' | '[' | '{' => stack.push(ch),
            ')' if stack.pop() != Some('(') => return false,
            ']' if stack.pop() != Some('[') => return false,
            '}' if stack.pop() != Some('{') => return false,
            _ => {}
        }
    }
    stack.is_empty()
}

fn is_initializer_designator_text(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with('.') || trimmed.starts_with('[')
}

fn literal_text_value(value: &str) -> bool {
    let trimmed = value.trim();
    integer_literal_value(value).is_some()
        || matches!(
            trimmed,
            "true" | "false" | "TRUE" | "FALSE" | "nullptr" | "NULL"
        )
        || string_literal_text_value(trimmed)
}

fn string_literal_text_value(value: &str) -> bool {
    [
        "\"", "u8\"", "u\"", "U\"", "L\"", "R\"", "u8R\"", "uR\"", "UR\"", "LR\"", "'", "u'", "U'",
        "L'",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix))
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
    let left = node
        .child_by_field_name("left")
        .or_else(|| named_children(node).into_iter().next());
    let right = node
        .child_by_field_name("right")
        .or_else(|| named_children(node).into_iter().nth(1));
    match (left, right) {
        (Some(left), Some(right)) => Expression::Assignment {
            operator: operator.to_string(),
            code: node_text(node, source).trim().to_string(),
            line: line(node),
            left: Box::new(parse_expression(left, source)),
            right: Box::new(parse_expression(right, source)),
        },
        _ => identifier_expression(node, source),
    }
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

fn parse_fold_expression(node: Node, source: &[u8]) -> Expression {
    let left = node
        .child_by_field_name("left")
        .and_then(|left| fold_operand_expression(left, source));
    let right = node
        .child_by_field_name("right")
        .and_then(|right| fold_operand_expression(right, source));
    Expression::Fold {
        operator: node
            .child_by_field_name("operator")
            .map(|operator| node_text(operator, source).trim().to_string())
            .unwrap_or_else(|| operator_text(node, source).unwrap_or("?").to_string()),
        code: node_text(node, source).trim().to_string(),
        line: line(node),
        left: left.map(Box::new),
        right: right.map(Box::new),
    }
}

fn fold_operand_expression(node: Node, source: &[u8]) -> Option<Expression> {
    if node.kind() == "..." {
        None
    } else {
        Some(parse_expression(node, source))
    }
}

fn parse_parameter_pack_expansion(node: Node, source: &[u8]) -> Expression {
    node.child_by_field_name("pattern")
        .map(|pattern| Expression::PackExpansion {
            code: node_text(node, source).trim().to_string(),
            line: line(node),
            pattern: Box::new(parse_expression(pattern, source)),
        })
        .unwrap_or_else(|| identifier_expression(node, source))
}

fn parse_decltype_expression(node: Node, source: &[u8]) -> Expression {
    named_children(node)
        .into_iter()
        .find(|child| child.kind() != "auto")
        .map(|argument| Expression::TypeOf {
            code: node_text(node, source).trim().to_string(),
            line: line(node),
            argument: Box::new(parse_expression(argument, source)),
        })
        .unwrap_or_else(|| identifier_expression(node, source))
}

fn parse_qualified_identifier_expression(node: Node, source: &[u8]) -> Expression {
    let scope = node.child_by_field_name("scope");
    let name = node.child_by_field_name("name").or_else(|| {
        named_children(node)
            .into_iter()
            .rev()
            .find(|child| Some(*child) != scope)
    });
    match (scope, name) {
        (Some(scope), Some(name)) if scope.kind() == "decltype" => Expression::FieldAccess {
            field: node_text(name, source).trim().to_string(),
            code: node_text(node, source).trim().to_string(),
            line: line(node),
            base: Box::new(parse_decltype_expression(scope, source)),
        },
        _ => identifier_expression(node, source),
    }
}

fn parse_call_expression(node: Node, source: &[u8]) -> Expression {
    let function = node.child_by_field_name("function");
    let arguments: Vec<Expression> = node
        .child_by_field_name("arguments")
        .map(|args| {
            named_children(args)
                .into_iter()
                .map(|arg| parse_expression(arg, source))
                .collect()
        })
        .unwrap_or_default();
    if let (Some(function), [value]) = (function, arguments.as_slice()) {
        let function_code = node_text(function, source).trim();
        if let Some(type_name) = cpp_named_cast_type(function_code) {
            let semantic_type_name = cpp_named_cast_type_preserving_cv(function_code)
                .unwrap_or_else(|| type_name.clone());
            return Expression::Cast {
                type_name,
                semantic_type_name,
                code: node_text(node, source).trim().to_string(),
                line: line(node),
                value: Box::new(value.clone()),
                resolved_type_full_name: None,
            };
        }
    }
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
        resolved_method_full_name: None,
        resolved_signature: None,
    }
}

fn cpp_named_cast_type(function_code: &str) -> Option<String> {
    cpp_named_cast_type_with_normalizer(function_code, normalize_type)
}

fn cpp_named_cast_type_preserving_cv(function_code: &str) -> Option<String> {
    cpp_named_cast_type_with_normalizer(function_code, normalize_type_preserving_cv)
}

fn cpp_named_cast_type_with_normalizer(
    function_code: &str,
    normalizer: fn(&str) -> String,
) -> Option<String> {
    const NAMED_CASTS: &[&str] = &[
        "const_cast",
        "dynamic_cast",
        "reinterpret_cast",
        "static_cast",
    ];
    NAMED_CASTS
        .iter()
        .find_map(|cast| function_code.strip_prefix(cast))
        .and_then(template_argument_text)
        .map(normalizer)
}

fn template_argument_text(raw: &str) -> Option<&str> {
    let raw = raw.trim();
    if !raw.starts_with('<') {
        return None;
    }
    let mut depth = 0usize;
    for (index, character) in raw.char_indices() {
        match character {
            '<' => depth += 1,
            '>' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    return raw[index + character.len_utf8()..]
                        .trim()
                        .is_empty()
                        .then(|| raw[1..index].trim())
                        .filter(|argument| !argument.is_empty());
                }
            }
            _ => {}
        }
    }
    None
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
        resolved_method_full_name: None,
        resolved_signature: None,
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
        Some(value) => {
            let type_name = node
                .child_by_field_name("type")
                .map(|type_node| type_name_from_type_descriptor(type_node, source))
                .unwrap_or_else(|| "ANY".to_string());
            let semantic_type_name = node
                .child_by_field_name("type")
                .map(|type_node| type_name_from_type_descriptor_preserving_cv(type_node, source))
                .unwrap_or_else(|| type_name.clone());
            Expression::Cast {
                type_name,
                semantic_type_name,
                code: node_text(node, source).trim().to_string(),
                line: line(node),
                value: Box::new(parse_expression(value, source)),
                resolved_type_full_name: None,
            }
        }
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

fn parse_offsetof_expression(node: Node, source: &[u8]) -> Expression {
    let line = line(node);
    let arguments = [
        node.child_by_field_name("type")
            .map(|type_node| type_name_from_type_node(type_node, source)),
        node.child_by_field_name("member")
            .map(|member| node_text(member, source).trim().to_string()),
    ]
    .into_iter()
    .flatten()
    .map(|value| Expression::Literal {
        code: value.clone(),
        value,
        line,
        resolved_type_full_name: None,
    })
    .collect();
    Expression::Call {
        name: "offsetof".to_string(),
        code: node_text(node, source).trim().to_string(),
        line,
        callee: Box::new(Expression::Identifier {
            name: "offsetof".to_string(),
            code: "offsetof".to_string(),
            line,
        }),
        arguments,
        resolved_method_full_name: None,
        resolved_signature: None,
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
    let mut inference_parameters = parameters.clone();
    inference_parameters.extend(lambda_template_parameters(node, source));
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
    let return_type = lambda_return_type(node, source, &inference_parameters, &body);
    let semantic_return_type =
        lambda_semantic_return_type(node, source, &inference_parameters, &body);
    Expression::Lambda {
        code: node_text(node, source).trim().to_string(),
        line: line(node),
        captures: lambda_captures(node, source),
        is_mutable: lambda_is_mutable(node, source),
        signature: signature(&return_type, &parameters),
        return_type,
        semantic_return_type,
        parameters,
        body,
    }
}

fn lambda_template_parameters(node: Node, source: &[u8]) -> Vec<ParameterDecl> {
    node.child_by_field_name("template_parameters")
        .map(|parameters| parse_parameters_without_varargs(parameters, source))
        .unwrap_or_default()
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

fn lambda_semantic_return_type(
    node: Node,
    source: &[u8],
    parameters: &[ParameterDecl],
    body: &[Statement],
) -> String {
    find_lambda_trailing_return_type_with_normalizer(node, source, normalize_type_preserving_cv)
        .unwrap_or_else(|| {
            body.iter()
                .find_map(|statement| return_statement_expression(statement))
                .map(|expression| expression_static_semantic_type(expression, parameters))
                .unwrap_or_else(|| "void".to_string())
        })
}

fn find_lambda_trailing_return_type(node: Node, source: &[u8]) -> Option<String> {
    find_lambda_trailing_return_type_with_normalizer(node, source, normalize_type)
}

fn find_lambda_trailing_return_type_with_normalizer(
    node: Node,
    source: &[u8],
    normalizer: fn(&str) -> String,
) -> Option<String> {
    node.child_by_field_name("declarator")
        .and_then(|declarator| trailing_return_type_with_normalizer(declarator, source, normalizer))
        .or_else(|| {
            node.child_by_field_name("type").map(|type_node| {
                type_name_from_type_node_with_normalizer(type_node, source, normalizer)
            })
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
                .map(|type_node| {
                    type_name_from_type_node_with_normalizer(type_node, source, normalizer)
                })
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
        Expression::Assignment { left, .. } => expression_static_type(left, parameters),
        _ => "ANY".to_string(),
    }
}

fn expression_static_semantic_type(
    expression: &Expression,
    parameters: &[ParameterDecl],
) -> String {
    match expression {
        Expression::Literal { value, .. } if integer_literal_value(value).is_some() => {
            "int".to_string()
        }
        Expression::Identifier { name, .. } => parameters
            .iter()
            .find(|parameter| parameter.name == *name)
            .map(|parameter| parameter.semantic_type_name.clone())
            .unwrap_or_else(|| "ANY".to_string()),
        Expression::Binary { left, right, .. } => {
            let left_type = expression_static_semantic_type(left, parameters);
            let right_type = expression_static_semantic_type(right, parameters);
            if left_type == right_type {
                left_type
            } else if left_type == "int" || right_type == "int" {
                "int".to_string()
            } else {
                "ANY".to_string()
            }
        }
        Expression::Assignment { left, .. } => expression_static_semantic_type(left, parameters),
        _ => expression_static_type(expression, parameters),
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
    let code = node_text(node, source).trim();
    if !is_simple_identifier(code) {
        return parse_expression_text(code, line(node));
    }
    Expression::Identifier {
        name: code.to_string(),
        code: code.to_string(),
        line: line(node),
    }
}

fn declaration_type_and_name(node: Node, source: &[u8]) -> Option<(String, String)> {
    declaration_type_and_name_with_normalizer(node, source, normalize_type)
}

fn declaration_semantic_type_and_name(node: Node, source: &[u8]) -> Option<(String, String)> {
    declaration_type_and_name_with_normalizer(node, source, normalize_type_preserving_cv)
}

fn declaration_type_and_name_with_normalizer(
    node: Node,
    source: &[u8],
    normalizer: fn(&str) -> String,
) -> Option<(String, String)> {
    let type_node = node.child_by_field_name("type")?;
    let declarator = node.child_by_field_name("declarator")?;
    let name = declarator_name(declarator, source)?;
    let base_type =
        declaration_base_type_with_normalizer(node, type_node, declarator, source, normalizer);
    let type_name = type_from_declarator(&base_type, declarator, source);
    Some((type_name, name))
}

fn declaration_base_type_with_normalizer(
    declaration: Node,
    type_node: Node,
    declarator: Node,
    source: &[u8],
    normalizer: fn(&str) -> String,
) -> String {
    std::str::from_utf8(&source[declaration.start_byte()..declarator.start_byte()])
        .ok()
        .map(normalizer)
        .filter(|base| !base.is_empty())
        .unwrap_or_else(|| type_name_from_type_node_with_normalizer(type_node, source, normalizer))
}

fn declaration_base_type_from_first_declarator_with_normalizer(
    declaration: Node,
    type_node: Node,
    source: &[u8],
    normalizer: fn(&str) -> String,
) -> String {
    named_children(declaration)
        .into_iter()
        .filter(|child| *child != type_node)
        .find(|child| declarator_name(*child, source).is_some())
        .and_then(|declarator| {
            std::str::from_utf8(&source[declaration.start_byte()..declarator.start_byte()])
                .ok()
                .map(normalizer)
                .filter(|base| !base.is_empty())
        })
        .unwrap_or_else(|| type_name_from_type_node_with_normalizer(type_node, source, normalizer))
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
    type_name_from_type_node_with_normalizer(node, source, normalize_type)
}

fn type_name_from_type_node_preserving_cv(node: Node, source: &[u8]) -> String {
    type_name_from_type_node_with_normalizer(node, source, normalize_type_preserving_cv)
}

fn type_name_from_type_node_with_normalizer(
    node: Node,
    source: &[u8],
    normalizer: fn(&str) -> String,
) -> String {
    match node.kind() {
        "struct_specifier" | "union_specifier" | "enum_specifier" => node
            .child_by_field_name("name")
            .map(|name| normalizer(node_text(name, source)))
            .unwrap_or_else(|| normalizer(node_text(node, source))),
        "class_specifier" => node
            .child_by_field_name("name")
            .map(|name| normalizer(node_text(name, source)))
            .unwrap_or_else(|| normalizer(node_text(node, source))),
        _ => normalizer(node_text(node, source)),
    }
}

fn type_name_from_type_descriptor(node: Node, source: &[u8]) -> String {
    type_name_from_type_descriptor_with_normalizer(node, source, normalize_type)
}

fn type_name_from_type_descriptor_preserving_cv(node: Node, source: &[u8]) -> String {
    type_name_from_type_descriptor_with_normalizer(node, source, normalize_type_preserving_cv)
}

fn type_name_from_type_descriptor_with_normalizer(
    node: Node,
    source: &[u8],
    normalizer: fn(&str) -> String,
) -> String {
    let Some(type_node) = node.child_by_field_name("type") else {
        return normalizer(node_text(node, source));
    };
    let Some(declarator) = node.child_by_field_name("declarator") else {
        return normalizer(node_text(node, source));
    };
    let base_type = std::str::from_utf8(&source[node.start_byte()..declarator.start_byte()])
        .ok()
        .map(normalizer)
        .filter(|base| !base.is_empty())
        .unwrap_or_else(|| type_name_from_type_node_with_normalizer(type_node, source, normalizer));
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
                function_pointer_marker(child).map_or_else(
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
        "init_declarator" | "parenthesized_declarator" | "variadic_declarator" => declarator
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

fn function_pointer_marker(declarator: Node) -> Option<String> {
    let marker = declarator_marker(declarator)?;
    marker.contains('*').then_some(marker)
}

fn declarator_marker(declarator: Node) -> Option<String> {
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
                .and_then(declarator_marker)
                .unwrap_or_default()
        )),
        "array_declarator" | "abstract_array_declarator" => Some(format!(
            "{}[]",
            child_declarator(declarator)
                .and_then(declarator_marker)
                .unwrap_or_default()
        )),
        "parenthesized_declarator" | "init_declarator" | "variadic_declarator" => declarator
            .child_by_field_name("declarator")
            .or_else(|| child_declarator(declarator))
            .and_then(declarator_marker),
        _ => named_children(declarator)
            .into_iter()
            .find_map(declarator_marker),
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
    split_type_and_name_with_normalizer(raw, normalize_type)
}

fn split_type_and_name_preserving_cv(raw: &str) -> Option<(String, String)> {
    split_type_and_name_with_normalizer(raw, normalize_type_preserving_cv)
}

fn split_type_and_name_with_normalizer(
    raw: &str,
    normalizer: fn(&str) -> String,
) -> Option<(String, String)> {
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
    Some((normalizer(parts[1]), name.to_string()))
}

fn split_type_and_name_with_declarator(raw: &str) -> Option<(String, String)> {
    split_type_and_name_with_declarator_and_normalizer(raw, normalize_type)
}

fn split_type_and_name_with_declarator_preserving_cv(raw: &str) -> Option<(String, String)> {
    split_type_and_name_with_declarator_and_normalizer(raw, normalize_type_preserving_cv)
}

fn split_type_and_name_with_declarator_and_normalizer(
    raw: &str,
    normalizer: fn(&str) -> String,
) -> Option<(String, String)> {
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
    let declarator = parts[0].trim();
    let marker = declarator
        .chars()
        .take_while(|ch| matches!(ch, '*' | '&'))
        .collect::<String>();
    let name = declarator.trim_start_matches(['*', '&']);
    if name.is_empty() {
        return None;
    }
    Some((
        normalizer(&format!("{}{marker}", parts[1])),
        name.to_string(),
    ))
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

fn normalize_type_preserving_cv(raw: &str) -> String {
    let normalized = raw
        .split_whitespace()
        .filter(|part| !TYPE_STORAGE_SPECIFIERS.contains(part))
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

fn normalize_function_return_type_preserving_cv(raw: &str) -> String {
    normalize_type_preserving_cv(raw)
        .split_whitespace()
        .filter(|part| !FUNCTION_RETURN_SPECIFIERS.contains(part))
        .collect::<Vec<_>>()
        .join(" ")
}

const TYPE_QUALIFIERS: &[&str] = &[
    "const", "volatile", "restrict", "static", "extern", "register", "typedef",
];
const TYPE_STORAGE_SPECIFIERS: &[&str] = &["static", "extern", "register", "typedef"];
const FUNCTION_RETURN_SPECIFIERS: &[&str] = &[
    "inline",
    "virtual",
    "constexpr",
    "consteval",
    "friend",
    "explicit",
];

fn signature(return_type: &str, params: &[ParameterDecl]) -> String {
    format!(
        "{}({})",
        return_type,
        params
            .iter()
            .map(signature_parameter_type)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn signature_parameter_type(param: &ParameterDecl) -> &str {
    if param.is_variadic
        && param.name.starts_with("<param>")
        && param.code == format!("{}...", param.name)
    {
        "..."
    } else {
        param.type_name.as_str()
    }
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

        for extension in ["cc", "cpp", "cxx", "cp", "ccm", "cxxm", "c++", "c++m"] {
            let filename = format!("main.{extension}");
            assert_eq!(language_for_path(Path::new(&filename)), SourceLanguage::Cpp);
        }

        for extension in ["h", "hh", "hpp", "hxx", "hp", "h++", "ipp", "tcc"] {
            let filename = format!("main.{extension}");
            assert_eq!(
                language_for_path(Path::new(&filename)),
                SourceLanguage::Header
            );
        }

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

    fn macro_symbols(defines: &[(&str, &[&str], &str)]) -> MacroSymbols {
        defines
            .iter()
            .map(|(name, params, body)| {
                (
                    (*name).to_string(),
                    MacroBinding {
                        parameters: params.iter().map(|param| (*param).to_string()).collect(),
                        body: (*body).to_string(),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn object_like_macro_used_in_condition_expands_to_its_value() {
        // `#if SIZE == 4` must compare against the macro's real replacement
        // value (4), not the legacy "any defined object-like macro is 1".
        let symbols = macro_symbols(&[("SIZE", &[], "4")]);
        assert_eq!(eval_preproc_identifier("SIZE", &symbols), 4);
        assert_eq!(
            eval_condition_text("SIZE == 4", &symbols),
            Some(1),
            "SIZE should expand to 4 so the equality holds"
        );
        assert_eq!(eval_condition_text("SIZE == 5", &symbols), Some(0));
    }

    #[test]
    fn chained_object_like_macros_expand_recursively() {
        // ENABLED -> TRUE -> 1, exercising recursive object-like substitution.
        let symbols = macro_symbols(&[("ENABLED", &[], "TRUE"), ("TRUE", &[], "1")]);
        assert_eq!(eval_preproc_identifier("ENABLED", &symbols), 1);

        // A nested arithmetic body: HALF -> (FULL / 2) -> (8 / 2) -> 4.
        let arithmetic = macro_symbols(&[("HALF", &[], "(FULL / 2)"), ("FULL", &[], "8")]);
        assert_eq!(eval_preproc_identifier("HALF", &arithmetic), 4);
        assert_eq!(eval_condition_text("HALF == 4", &arithmetic), Some(1));
    }

    #[test]
    fn function_like_macro_reference_in_condition_stays_truthy() {
        // A function-like macro referenced without call syntax cannot be
        // expanded in a condition; preserve the legacy truthy fallback.
        let symbols = macro_symbols(&[("INC", &["x"], "((x) + 1)")]);
        assert_eq!(eval_preproc_identifier("INC", &symbols), 1);
    }

    #[test]
    fn self_referential_macro_expansion_is_guarded() {
        // The "blue paint" rule stops `#define A A` and mutually recursive
        // macros from looping forever; the unresolved name falls back to 1.
        let direct = macro_symbols(&[("A", &[], "A")]);
        assert_eq!(eval_preproc_identifier("A", &direct), 1);

        let mutual = macro_symbols(&[("A", &[], "B"), ("B", &[], "A")]);
        assert_eq!(eval_preproc_identifier("A", &mutual), 1);
    }

    #[test]
    fn unexpandable_macro_body_falls_back_safely() {
        // Stringize/token-paste/variadic bodies are not expanded; the evaluator
        // falls back to the legacy truthy value and records the fallback.
        let _ = take_unmapped_summary();
        let stringize = macro_symbols(&[("STR", &[], "#x")]);
        assert_eq!(eval_preproc_identifier("STR", &stringize), 1);

        let paste = macro_symbols(&[("CAT", &[], "a ## b")]);
        assert_eq!(eval_preproc_identifier("CAT", &paste), 1);

        let summary = take_unmapped_summary().expect("fallback should be tallied");
        assert!(
            summary.contains("preproc_macro_stringize_or_paste"),
            "unexpected summary: {summary}"
        );
    }

    #[test]
    fn object_like_macro_chain_selects_preprocessor_branch() {
        // End-to-end: a chained object-like macro drives `#if` branch selection
        // through parse_declarations, so the active branch's return is kept.
        let source = r#"
            #define FULL 8
            #define HALF (FULL / 2)
            int picks_branch() {
            #if HALF == 4
              return 4;
            #else
              return 0;
            #endif
            }
        "#;
        let document = CxxAstDocument {
            schema_version: SCHEMA_VERSION,
            backend: BACKEND_NAME,
            path: "macro_chain.c".into(),
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
            declarations: parse_declarations(source, SourceLanguage::C)
                .expect("macro chain source should parse"),
        };
        assert_eq!(function_return_literal(&document, "picks_branch"), "4");
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
                  operator bool() const { return value != 0; }
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
                "operator bool",
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
        assert!(methods.iter().any(|method| method.name == "operator bool"
            && method.return_type == "bool"
            && method.signature == "bool()<const>"
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
        assert_eq!(fancy.base_class_declarations.len(), 1);
        assert_eq!(fancy.base_class_declarations[0].name, "Widget");
        assert!(!fancy.base_class_declarations[0].is_virtual);
        assert!(fancy.nested_declarations.iter().any(|declaration| matches!(
            declaration,
            Declaration::Function(method)
                if method.name == "render" && method.signature == "int(int)" && method.is_virtual
        )));
        assert!(fancy.using_declarations.is_empty());

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
    fn parses_cpp_member_using_declarations() {
        let sample = r#"
                namespace Core {
                class Base {
                public:
                  int pick(int& value) { return value; }
                };
                class Derived : public Base {
                public:
                  using Base::pick;
                  int pick(int value) { return value + 1; }
                };
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("member using declaration sample should parse");
        let namespace = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Namespace(namespace) if namespace.name == "Core" => Some(namespace),
                _ => None,
            })
            .expect("expected Core namespace declaration");
        let derived = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(struct_decl) if struct_decl.name == "Derived" => {
                    Some(struct_decl)
                }
                _ => None,
            })
            .expect("expected Derived class");

        assert_eq!(derived.using_declarations.len(), 1);
        let using = &derived.using_declarations[0];
        assert_eq!(using.name, "pick");
        assert_eq!(using.target, "Base::pick");
        assert_eq!(using.code, "using Base::pick");
    }

    #[test]
    fn parses_cpp_virtual_base_declarations() {
        let sample = r#"
                template <typename T, typename U>
                class PairBase {};
                class Root {};
                class Other {};
                class Derived : public virtual Root, protected Other, private PairBase<int, float> {};
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("virtual base declaration sample should parse");
        let derived = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(struct_decl) if struct_decl.name == "Derived" => {
                    Some(struct_decl)
                }
                _ => None,
            })
            .expect("expected Derived class");

        assert_eq!(
            derived.base_classes,
            vec!["Root", "Other", "PairBase<int, float>"]
        );
        assert_eq!(derived.base_class_declarations.len(), 3);
        assert_eq!(
            derived.base_class_declarations[0].code,
            "public virtual Root"
        );
        assert!(derived.base_class_declarations[0].is_virtual);
        assert_eq!(derived.base_class_declarations[1].code, "protected Other");
        assert!(!derived.base_class_declarations[1].is_virtual);
        assert_eq!(
            derived.base_class_declarations[2].name,
            "PairBase<int, float>"
        );
        assert!(!derived.base_class_declarations[2].is_virtual);
    }

    #[test]
    fn preserves_cpp_const_semantic_types() {
        let sample = r#"
                namespace Core {
                class Meter {};
                const Meter globalMeter;
                struct Holder {
                  const Meter field;
                  Meter mutableField;
                };
                int read(const Meter& meter) {
                  const Meter& alias = meter;
                  return 0;
                }
                const Meter& pick(const Meter& meter) {
                  return meter;
                }
                auto trailing(const Meter& meter) -> const Meter& {
                  return meter;
                }
                int castRead(Meter& meter) {
                  const Meter& casted = static_cast<const Meter&>(meter);
                  auto pick = [](Meter& input) -> const Meter& { return input; };
                  return 0;
                }
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("const semantic type sample should parse");
        let namespace = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Namespace(namespace) if namespace.name == "Core" => Some(namespace),
                _ => None,
            })
            .expect("expected Core namespace declaration");
        let function = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "read" => Some(function),
                _ => None,
            })
            .expect("expected read function");
        let pick = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "pick" => Some(function),
                _ => None,
            })
            .expect("expected pick function");
        let trailing = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "trailing" => Some(function),
                _ => None,
            })
            .expect("expected trailing function");
        let cast_read = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "castRead" => Some(function),
                _ => None,
            })
            .expect("expected castRead function");
        let global = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::GlobalVariable(global) if global.name == "globalMeter" => Some(global),
                _ => None,
            })
            .expect("expected globalMeter variable");
        let holder = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(struct_decl) if struct_decl.name == "Holder" => {
                    Some(struct_decl)
                }
                _ => None,
            })
            .expect("expected Holder struct");

        assert_eq!(function.parameters[0].type_name, "Meter&");
        assert_eq!(function.parameters[0].semantic_type_name, "const Meter&");
        assert_eq!(pick.return_type, "Meter&");
        assert_eq!(pick.semantic_return_type, "const Meter&");
        assert_eq!(trailing.return_type, "Meter&");
        assert_eq!(trailing.semantic_return_type, "const Meter&");
        assert_eq!(global.type_name, "Meter");
        assert_eq!(global.semantic_type_name, "const Meter");
        assert_eq!(holder.fields[0].name, "field");
        assert_eq!(holder.fields[0].type_name, "Meter");
        assert_eq!(holder.fields[0].semantic_type_name, "const Meter");
        assert_eq!(holder.fields[1].name, "mutableField");
        assert_eq!(holder.fields[1].type_name, "Meter");
        assert_eq!(holder.fields[1].semantic_type_name, "Meter");
        assert!(matches!(
            function.body.as_slice(),
            [Statement::LocalDecl {
                name,
                type_name,
                semantic_type_name,
                ..
            }, Statement::Return { .. }] if name == "alias"
                && type_name == "Meter&"
                && semantic_type_name == "const Meter&"
        ));
        assert!(matches!(
            cast_read.body.as_slice(),
            [Statement::LocalDecl {
                name,
                type_name,
                semantic_type_name,
                initializer:
                    Some(Expression::Cast {
                        type_name: cast_type_name,
                        semantic_type_name: cast_semantic_type_name,
                        ..
                    }),
                ..
            }, Statement::LocalDecl {
                name: lambda_name,
                initializer:
                    Some(Expression::Lambda {
                        return_type: lambda_return_type,
                        semantic_return_type: lambda_semantic_return_type,
                        ..
                    }),
                ..
            }, Statement::Return { .. }] if name == "casted"
                && type_name == "Meter&"
                && semantic_type_name == "const Meter&"
                && cast_type_name == "Meter&"
                && cast_semantic_type_name == "const Meter&"
                && lambda_name == "pick"
                && lambda_return_type == "Meter&"
                && lambda_semantic_return_type == "const Meter&"
        ));
    }

    #[test]
    fn parses_cpp_default_member_initializers() {
        let sample = r#"
                struct Cell {
                  int x = 1;
                  int y{2};
                };
                struct Holder {
                  Cell cell = {3, 4};
                  static int count = 5;
                  int plain;
                };
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("default member initializer sample should parse");
        let holder = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(struct_decl) if struct_decl.name == "Holder" => {
                    Some(struct_decl)
                }
                _ => None,
            })
            .expect("expected Holder struct");
        assert_eq!(holder.fields.len(), 3);
        assert_eq!(holder.fields[0].name, "cell");
        assert!(
            matches!(
                holder.fields[0].initializer.as_ref(),
                Some(Expression::InitializerList { code, elements, .. })
                    if code == "{3, 4}" && elements.len() == 2
            ),
            "expected cell initializer list, got {:?}",
            holder.fields[0].initializer
        );
        assert_eq!(holder.fields[1].name, "count");
        assert!(holder.fields[1].is_static);
        assert!(
            matches!(
                holder.fields[1].initializer.as_ref(),
                Some(Expression::Literal { value, .. }) if value == "5"
            ),
            "expected count literal initializer, got {:?}",
            holder.fields[1].initializer
        );
        assert_eq!(holder.fields[2].name, "plain");
        assert!(holder.fields[2].initializer.is_none());

        let cell = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(struct_decl) if struct_decl.name == "Cell" => Some(struct_decl),
                _ => None,
            })
            .expect("expected Cell struct");
        assert!(
            matches!(
                cell.fields[0].initializer.as_ref(),
                Some(Expression::Literal { value, .. }) if value == "1"
            ),
            "expected x literal initializer, got {:?}",
            cell.fields[0].initializer
        );
        assert!(
            matches!(
                cell.fields[1].initializer.as_ref(),
                Some(Expression::InitializerList { code, elements, .. })
                    if code == "{2}" && matches!(elements.as_slice(), [Expression::Literal { value, .. }] if value == "2")
            ),
            "expected y initializer list, got {:?}",
            cell.fields[1].initializer
        );
    }

    #[test]
    fn parses_cpp_operator_cast_declarations_and_definitions() {
        let sample = r#"
                namespace Core {
                class Meter {};
                using MeterAlias = Meter;
                class Widget {
                public:
                  operator bool() const;
                };
                class RefWidget {
                public:
                  operator const MeterAlias&();
                };
                }
                Core::Widget::operator bool() const { return true; }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("operator cast sample should parse");
        let namespace = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Namespace(namespace) if namespace.name == "Core" => Some(namespace),
                _ => None,
            })
            .expect("expected Core namespace declaration");
        let declared = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(struct_decl) if struct_decl.name == "Widget" => struct_decl
                    .nested_declarations
                    .iter()
                    .find_map(|nested| match nested {
                        Declaration::Function(function) if function.name == "operator bool" => {
                            Some(function)
                        }
                        _ => None,
                    }),
                _ => None,
            })
            .expect("expected operator bool declaration");
        assert_eq!(declared.return_type, "bool");
        assert_eq!(declared.signature, "bool()<const>");
        assert!(declared.is_const);
        assert!(!declared.is_definition);
        let declared_ref = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(struct_decl) if struct_decl.name == "RefWidget" => struct_decl
                    .nested_declarations
                    .iter()
                    .find_map(|nested| match nested {
                        Declaration::Function(function)
                            if function.name == "operator MeterAlias&" =>
                        {
                            Some(function)
                        }
                        _ => None,
                    }),
                _ => None,
            })
            .expect("expected operator const MeterAlias& declaration");
        assert_eq!(declared_ref.return_type, "MeterAlias&");
        assert_eq!(declared_ref.semantic_return_type, "const MeterAlias&");
        assert_eq!(declared_ref.signature, "MeterAlias&()");
        assert!(!declared_ref.is_const);
        assert!(!declared_ref.is_definition);

        let defined = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) => {
                    (function.name == "Core::Widget::operator bool").then_some(function)
                }
                _ => None,
            })
            .expect("expected out-of-class operator bool definition");
        assert_eq!(defined.return_type, "bool");
        assert_eq!(defined.signature, "bool()<const>");
        assert!(defined.is_const);
        assert!(defined.is_definition);
        assert!(matches!(
            defined.body.as_slice(),
            [Statement::Return {
                expression: Some(Expression::Literal { value, .. }),
                ..
            }] if value == "true"
        ));
    }

    #[test]
    fn parses_cpp_callable_object_reference_and_pointer_calls() {
        let sample = r#"
                namespace Core {
                class Invoker {
                public:
                  int operator()(int delta) const { return delta + 1; }
                };
                }
                int use(Core::Invoker& ref, Core::Invoker* ptr) {
                  Core::Invoker local;
                  return ref(1) + (*ptr)(2) + local(3);
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("callable reference and pointer sample should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "use" => Some(function),
                _ => None,
            })
            .expect("expected use function");
        assert_eq!(function.parameters[0].type_name, "Core::Invoker&");
        assert_eq!(function.parameters[1].type_name, "Core::Invoker*");
        let [Statement::LocalDecl {
            name: local_name,
            type_name: local_type,
            ..
        }, Statement::Return {
            expression: Some(return_expression),
            ..
        }] = function.body.as_slice()
        else {
            panic!("expected local callable followed by return");
        };
        assert_eq!(local_name, "local");
        assert_eq!(local_type, "Core::Invoker");
        let call_names = collect_call_names(return_expression);
        assert_eq!(call_names, vec!["ref", "(*ptr)", "local"]);
        let dereferenced_call = find_call_by_name(return_expression, "(*ptr)")
            .expect("expected pointer-dereferenced callable call");
        assert!(matches!(
            dereferenced_call,
            Expression::Call {
                callee,
                ..
            } if matches!(
                callee.as_ref(),
                Expression::Unary {
                    operator,
                    argument,
                    ..
                } if operator == "*" && matches!(argument.as_ref(), Expression::Identifier { name, .. } if name == "ptr")
            )
        ));
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
                int withDefault(int value, int scale = 1) { return value + scale; }
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
        let with_default = namespace
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "withDefault" => Some(function),
                _ => None,
            })
            .expect("expected default-argument function");
        assert_eq!(with_default.signature, "int(int,int)");
        assert!(!with_default.parameters[0].has_default);
        assert!(with_default.parameters[1].has_default);
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
    fn parses_cpp_explicit_bool_constructor_templates() {
        let sample = r#"
                struct foo {
                  template <typename T>
                  explicit(!std::is_integral_v<T>) foo(T) {}
                };
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("explicit(bool) constructor sample should parse");
        let Declaration::Struct(struct_decl) = &declarations[0] else {
            panic!("expected struct declaration");
        };
        let constructor = struct_decl
            .nested_declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "foo" => Some(function),
                _ => None,
            })
            .expect("expected explicit(bool) constructor");
        assert_eq!(constructor.return_type, "void");
        assert_eq!(constructor.signature, "void(T)");
        assert!(constructor.is_definition);
        assert_eq!(constructor.parameters.len(), 1);
        assert_eq!(constructor.parameters[0].name, "param1");
        assert_eq!(constructor.parameters[0].type_name, "T");
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
    fn parses_cpp_constrained_lambdas() {
        let sample = r#"
                void use() {
                  auto l1 = []<my_concept T> (T v) { return v; };
                  auto l2 = []<typename T> requires my_concept<T> (T v) { return v; };
                  auto l3 = []<typename T> (T v) requires my_concept<T> { return v; };
                  auto l4 = [](my_concept auto v) { return v; };
                  auto l5 = []<my_concept auto v> () { return v; };
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("constrained lambda sample should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "use" => Some(function),
                _ => None,
            })
            .expect("expected use function");

        let lambdas = function
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
            .map(|(name, lambda)| {
                let Expression::Lambda {
                    parameters,
                    return_type,
                    signature,
                    ..
                } = lambda
                else {
                    unreachable!();
                };
                (
                    name,
                    signature.as_str(),
                    return_type.as_str(),
                    parameters
                        .iter()
                        .map(|parameter| (parameter.name.as_str(), parameter.type_name.as_str()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            lambdas,
            vec![
                ("l1", "T(T)", "T", vec![("v", "T")]),
                ("l2", "T(T)", "T", vec![("v", "T")]),
                ("l3", "T(T)", "T", vec![("v", "T")]),
                (
                    "l4",
                    "my_concept auto(my_concept auto)",
                    "my_concept auto",
                    vec![("v", "my_concept auto")]
                ),
                ("l5", "my_concept auto()", "my_concept auto", vec![]),
            ]
        );
    }

    #[test]
    fn parses_cpp_function_trailing_return_types() {
        let sample = r#"
                auto f(int x) -> long { return x; }
                auto g(int *p) -> int* { return p; }
                auto ref(int &x) -> int& { return x; }
                auto h() -> decltype(1 + 2);
                struct Widget {
                  auto size() const -> int;
                };
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("trailing return type sample should parse");
        let function_type = |name: &str| {
            declarations
                .iter()
                .find_map(|declaration| match declaration {
                    Declaration::Function(function) if function.name == name => {
                        Some((function.return_type.as_str(), function.signature.as_str()))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("expected function {name}"))
        };
        assert_eq!(function_type("f"), ("long", "long(int)"));
        assert_eq!(function_type("g"), ("int*", "int*(int*)"));
        assert_eq!(function_type("ref"), ("int&", "int&(int&)"));
        assert_eq!(function_type("h"), ("decltype(1 + 2)", "decltype(1 + 2)()"));

        let widget_size = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Struct(struct_decl) if struct_decl.name == "Widget" => struct_decl
                    .nested_declarations
                    .iter()
                    .find_map(|nested| match nested {
                        Declaration::Function(function) if function.name == "size" => {
                            Some(function)
                        }
                        _ => None,
                    }),
                _ => None,
            })
            .expect("expected Widget::size declaration");
        assert_eq!(widget_size.return_type, "int");
        assert_eq!(widget_size.signature, "int()<const>");
    }

    #[test]
    fn parses_cpp_decltype_qualified_field_access() {
        let sample = r#"
                void method() {
                  int local = 1;
                  constexpr bool is_std_array_v = decltype(local)::value;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("decltype qualified access sample should parse");
        let Declaration::Function(function) = &declarations[0] else {
            panic!("expected function declaration");
        };
        let Some(Statement::LocalDecl {
            initializer:
                Some(Expression::FieldAccess {
                    field, code, base, ..
                }),
            ..
        }) = function.body.get(1)
        else {
            panic!("expected decltype qualified field access initializer");
        };
        assert_eq!(field, "value");
        assert_eq!(code, "decltype(local)::value");
        let Expression::TypeOf { code, argument, .. } = base.as_ref() else {
            panic!("expected decltype base to parse as typeOf");
        };
        assert_eq!(code, "decltype(local)");
        assert!(matches!(
            argument.as_ref(),
            Expression::Identifier { name, code, .. } if name == "local" && code == "local"
        ));
    }

    #[test]
    fn parses_cpp_concept_requires_expression_placeholders() {
        let sample = r#"
                template <typename T>
                concept callable = requires (T f) { f(); };

                template <typename T>
                  requires requires (T x) { x + x; }
                T add(T a, T b) { return a + b; }

                template <typename T>
                  requires callable<T>
                void f(T v);
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("concept requires sample should parse");
        let functions: Vec<&FunctionDecl> = declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Function(function) => Some(function),
                _ => None,
            })
            .collect();
        assert_eq!(
            functions
                .iter()
                .map(|function| (function.name.as_str(), function.signature.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("requires", "requires(T)"),
                ("add", "T(T,T)"),
                ("f", "void(T)")
            ]
        );

        let requires = functions[0];
        assert!(!requires.is_definition);
        assert_eq!(requires.code, "requires (T f) { f(); }");
        assert_eq!(requires.parameters.len(), 1);
        assert_eq!(requires.parameters[0].name, "f");
        assert_eq!(requires.parameters[0].type_name, "T");
        assert_eq!(requires.parameters[0].code, "T f");
    }

    #[test]
    fn parses_cpp_constrained_function_declarations() {
        let sample = r#"
                template <my_concept T>
                void f1(T v);

                template <typename T>
                  requires my_concept<T>
                void f2(T v);

                template <typename T>
                void f3(T v) requires my_concept<T>;

                void f4(my_concept auto v);

                template <my_concept auto v>
                void f5();

                template <typename T>
                  requires my_concept<T>
                void f6(T);
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("constrained function declarations should parse");
        let functions = declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Function(function) => Some(function),
                _ => None,
            })
            .map(|function| {
                (
                    function.name.as_str(),
                    function.signature.as_str(),
                    function
                        .parameters
                        .iter()
                        .map(|parameter| (parameter.name.as_str(), parameter.type_name.as_str()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            functions,
            vec![
                ("f1", "void(T)", vec![("v", "T")]),
                ("f2", "void(T)", vec![("v", "T")]),
                ("f3", "void(T)", vec![("v", "T")]),
                (
                    "f4",
                    "void(my_concept auto)",
                    vec![("v", "my_concept auto")]
                ),
                ("f5", "void()", vec![]),
                ("f6", "void(T)", vec![("param1", "T")]),
            ]
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
    fn parses_cpp_parenthesized_local_initializer_calls() {
        let sample = r#"
                namespace Core {
                class Source {};
                Source makeSource();
                class Holder {
                public:
                  Holder(Source source) {}
                };
                }
                struct Cell {
                  int x;
                  int y;
                };
                struct Board {
                  Cell cell;
                  int z;
                };
                struct BoardHolder {
                  BoardHolder(Board input) {}
                };
                int use(Core::Source& source, int seed) {
                  Board target;
                  Core::Holder local(Core::makeSource());
                  BoardHolder aggregate(target = {{1, seed}, 2});
                  return 0;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("parenthesized local initializer sample should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "use" => Some(function),
                _ => None,
            })
            .expect("expected use function");
        let [Statement::LocalDecl { .. }, Statement::LocalDecl {
            name,
            initializer: Some(initializer),
            ..
        }, Statement::LocalDecl {
            name: aggregate_name,
            initializer: Some(aggregate_initializer),
            ..
        }, Statement::Return { .. }] = function.body.as_slice()
        else {
            panic!(
                "expected target, initialized locals, and return: {:#?}",
                function.body
            );
        };
        assert_eq!(name, "local");
        let Expression::InitializerList { code, elements, .. } = initializer else {
            panic!("expected direct initializer list");
        };
        assert_eq!(code, "(Core::makeSource())");
        assert!(matches!(
            elements.as_slice(),
            [Expression::Call { name, arguments, .. }] if name == "Core::makeSource" && arguments.is_empty()
        ));
        assert_eq!(aggregate_name, "aggregate");
        let Expression::InitializerList {
            code,
            elements: aggregate_elements,
            ..
        } = aggregate_initializer
        else {
            panic!("expected aggregate direct initializer list");
        };
        assert_eq!(code, "(target = {{1, seed}, 2})");
        assert!(matches!(
            aggregate_elements.as_slice(),
            [Expression::Assignment {
                operator,
                left,
                right,
                ..
            }] if operator == "="
                && matches!(left.as_ref(), Expression::Identifier { name, .. } if name == "target")
                && matches!(
                    right.as_ref(),
                    Expression::InitializerList { elements, .. }
                        if matches!(
                            elements.as_slice(),
                            [Expression::InitializerList { elements, .. }, Expression::Literal { value, .. }]
                                if value == "2"
                                    && matches!(
                                        elements.as_slice(),
                                        [Expression::Literal { value, .. }, Expression::Identifier { name, .. }]
                                            if value == "1" && name == "seed"
                                    )
                        )
                )
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
    fn parses_cpp_selection_initializers_and_condition_declarations() {
        let sample = r#"
                struct Pair {
                  int first;
                  int second;
                };
                int f();
                Pair make_pair();
                int use(int n) {
                  if (int x = f(); x) {
                    return x;
                  }
                  if (auto [first, second] = make_pair(); first) {
                    return second;
                  }
                  while (int w = f()) {
                    return w;
                  }
                  while (auto [left, right] = make_pair()) {
                    return left + right;
                  }
                  switch (int y = f(); y) {
                  case 1:
                    return y;
                  default:
                    return n;
                  }
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("selection initializer sample should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "use" => Some(function),
                _ => None,
            })
            .expect("expected use function");
        let [Statement::If {
            initializer: if_initializer,
            condition_initializer: if_condition_initializer,
            condition: if_condition,
            then_body,
            ..
        }, Statement::If {
            initializer: structured_if_initializer,
            condition_initializer: structured_if_condition_initializer,
            condition: structured_if_condition,
            then_body: structured_if_body,
            ..
        }, Statement::While {
            initializer: while_initializer,
            condition_initializer: while_condition_initializer,
            condition: while_condition,
            body: while_body,
            ..
        }, Statement::While {
            initializer: structured_while_initializer,
            condition_initializer: structured_while_condition_initializer,
            condition: structured_while_condition,
            body: structured_while_body,
            ..
        }, Statement::Switch {
            initializer: switch_initializer,
            condition_initializer: switch_condition_initializer,
            condition: switch_condition,
            body: switch_body,
            ..
        }] = function.body.as_slice()
        else {
            panic!("expected if, structured if, while, structured while, and switch");
        };

        assert!(matches!(
            if_initializer.as_slice(),
            [Statement::LocalDecl {
                name,
                initializer: Some(Expression::Call { name: call_name, .. }),
                ..
            }] if name == "x" && call_name == "f"
        ));
        assert!(if_condition_initializer.is_empty());
        assert!(matches!(if_condition, Expression::Identifier { name, .. } if name == "x"));
        assert!(matches!(then_body.as_slice(), [Statement::Return { .. }]));

        assert!(matches!(
            structured_if_initializer.as_slice(),
            [Statement::StructuredBinding {
                type_name,
                names,
                initializer: Some(Expression::Call { name: call_name, .. }),
                ..
            }] if type_name == "auto"
                && names == &vec!["first".to_string(), "second".to_string()]
                && call_name == "make_pair"
        ));
        assert!(structured_if_condition_initializer.is_empty());
        assert!(matches!(
            structured_if_condition,
            Expression::Identifier { name, .. } if name == "first"
        ));
        assert!(matches!(
            structured_if_body.as_slice(),
            [Statement::Return { .. }]
        ));

        assert!(while_initializer.is_empty());
        assert!(matches!(
            while_condition_initializer.as_slice(),
            [Statement::LocalDecl {
                name,
                initializer: Some(Expression::Call { name: call_name, .. }),
                ..
            }] if name == "w" && call_name == "f"
        ));
        assert!(matches!(while_condition, Expression::Identifier { name, .. } if name == "w"));
        assert!(matches!(while_body.as_slice(), [Statement::Return { .. }]));

        assert!(structured_while_initializer.is_empty());
        assert!(matches!(
            structured_while_condition_initializer.as_slice(),
            [Statement::StructuredBinding {
                type_name,
                names,
                initializer: Some(Expression::Call { name: call_name, .. }),
                temp_name,
                ..
            }] if type_name == "auto"
                && names == &vec!["left".to_string(), "right".to_string()]
                && call_name == "make_pair"
                && matches!(structured_while_condition, Expression::Identifier { name, .. } if name == temp_name)
        ));
        assert!(matches!(
            structured_while_body.as_slice(),
            [Statement::Return { expression: Some(Expression::Binary { operator, .. }), .. }] if operator == "+"
        ));

        assert!(matches!(
            switch_initializer.as_slice(),
            [Statement::LocalDecl {
                name,
                initializer: Some(Expression::Call { name: call_name, .. }),
                ..
            }] if name == "y" && call_name == "f"
        ));
        assert!(switch_condition_initializer.is_empty());
        assert!(matches!(switch_condition, Expression::Identifier { name, .. } if name == "y"));
        assert!(matches!(
            switch_body.as_slice(),
            [Statement::Case { .. }, Statement::Case { .. }]
        ));
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
    fn parses_cpp_likely_and_unlikely_attributed_statements() {
        let sample = r#"
                void foo() {
                  switch (n) {
                    case 1:
                      case1();
                      break;
                    [[likely]] case 2:
                      case2();
                      break;
                  }
                  if (random > 0) [[likely]] {
                    likelyIf();
                  }
                  while (unlikely_truthy_condition) [[unlikely]] {
                    unlikelyWhile();
                  }
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("likely/unlikely attributed statement sample should parse");
        let Declaration::Function(function) = &declarations[0] else {
            panic!("expected function declaration");
        };
        let [Statement::Switch {
            body: switch_body, ..
        }, Statement::If {
            condition,
            then_body,
            ..
        }, Statement::While {
            condition: while_condition,
            body: while_body,
            ..
        }] = function.body.as_slice()
        else {
            panic!("expected switch, if, and while statements");
        };

        let [Statement::Case {
            code: first_case,
            body: first_case_body,
            ..
        }, Statement::Case {
            code: likely_case,
            body: likely_case_body,
            ..
        }] = switch_body.as_slice()
        else {
            panic!("expected plain and attributed cases");
        };
        assert_eq!(first_case, "case 1:");
        assert_eq!(likely_case, "[[likely]] case 2:");
        assert!(matches!(
            first_case_body.as_slice(),
            [
                Statement::Expression {
                    expression: Expression::Call { name, .. },
                    ..
                },
                Statement::Break { .. }
            ] if name == "case1"
        ));
        assert!(matches!(
            likely_case_body.as_slice(),
            [
                Statement::Expression {
                    expression: Expression::Call { name, .. },
                    ..
                },
                Statement::Break { .. }
            ] if name == "case2"
        ));

        assert_binary_operator(condition, ">");
        assert!(matches!(
            then_body.as_slice(),
            [Statement::Expression {
                expression: Expression::Call { name, .. },
                ..
            }] if name == "likelyIf"
        ));
        assert!(matches!(
            while_condition,
            Expression::Identifier { name, .. } if name == "unlikely_truthy_condition"
        ));
        assert!(matches!(
            while_body.as_slice(),
            [Statement::Expression {
                expression: Expression::Call { name, .. },
                ..
            }] if name == "unlikelyWhile"
        ));
    }

    #[test]
    fn parses_cpp_using_enum_statements() {
        let sample = r#"
                enum class rgba_color_channel { red, green, blue, alpha };
                int to_int(rgba_color_channel channel) {
                  switch (channel) {
                    using enum rgba_color_channel;
                    case red:
                      return 1;
                    default:
                      return 0;
                  }
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("using enum sample should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "to_int" => Some(function),
                _ => None,
            })
            .expect("expected to_int function");
        let [Statement::Switch { body, .. }] = function.body.as_slice() else {
            panic!("expected switch statement");
        };
        let [Statement::UsingEnum {
            type_name, code, ..
        }, Statement::Case {
            value: Some(Expression::Identifier { name, .. }),
            ..
        }, Statement::Case { value: None, .. }] = body.as_slice()
        else {
            panic!("expected using enum followed by cases");
        };
        assert_eq!(type_name, "rgba_color_channel");
        assert_eq!(code, "using enum rgba_color_channel");
        assert_eq!(name, "red");
    }

    #[test]
    fn parses_cpp_range_based_for_loops() {
        let sample = r#"
                int sum(int *items) {
                  int total = 0;
                  for (int value : items) {
                    total += value;
                  }
                  return total;
                }
                "#;
        let declarations =
            parse_declarations(sample, SourceLanguage::Cpp).expect("range-for sample should parse");
        let Declaration::Function(function) = &declarations[0] else {
            panic!("expected function declaration");
        };
        let [Statement::LocalDecl { .. }, Statement::For {
            initializer,
            condition: Some(condition),
            update,
            body,
            ..
        }, Statement::Return { .. }] = function.body.as_slice()
        else {
            panic!("expected local, range-for, return");
        };
        assert!(update.is_none());
        assert!(matches!(condition, Expression::Identifier { name, .. } if name == "items"));
        assert!(matches!(
            initializer.as_slice(),
            [Statement::LocalDecl {
                name,
                type_name,
                initializer: None,
                ..
            }] if name == "value" && type_name == "int"
        ));
        assert!(matches!(
            body.as_slice(),
            [Statement::Assignment {
                operator,
                left,
                right,
                ..
            }] if operator == "+="
                && matches!(left, Expression::Identifier { name, .. } if name == "total")
                && matches!(right, Expression::Identifier { name, .. } if name == "value")
        ));
    }

    #[test]
    fn parses_cpp_range_based_for_loops_with_initializers() {
        let sample = r#"
                void each(int *list) {
                  for (auto v = list; auto& e : v) {
                    e += 1;
                  }
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("range-for initializer sample should parse");
        let Declaration::Function(function) = &declarations[0] else {
            panic!("expected function declaration");
        };
        let [Statement::For {
            initializer,
            condition: Some(condition),
            update,
            body,
            ..
        }] = function.body.as_slice()
        else {
            panic!("expected range-for statement");
        };
        assert!(update.is_none());
        assert!(matches!(condition, Expression::Identifier { name, .. } if name == "v"));
        assert!(matches!(
            initializer.as_slice(),
            [
                Statement::LocalDecl {
                    name: range_initializer,
                    type_name: range_initializer_type,
                    initializer: Some(Expression::Identifier { name: initializer_value, .. }),
                    ..
                },
                Statement::LocalDecl {
                    name: range_variable,
                    type_name: range_variable_type,
                    initializer: None,
                    ..
                }
            ] if range_initializer == "v"
                && range_initializer_type == "auto"
                && initializer_value == "list"
                && range_variable == "e"
                && range_variable_type == "auto&"
        ));
        assert!(matches!(
            body.as_slice(),
            [Statement::Assignment {
                operator,
                left,
                right,
                ..
            }] if operator == "+="
                && matches!(left, Expression::Identifier { name, .. } if name == "e")
                && matches!(right, Expression::Literal { value, .. } if value == "1")
        ));
    }

    #[test]
    fn parses_cpp_range_based_for_loops_with_structured_bindings() {
        let sample = r#"
                struct Pair {
                  int first;
                  int second;
                };
                int sum(Pair *pairs) {
                  int total = 0;
                  for (auto [first, second] : pairs) {
                    total += first + second;
                  }
                  return total;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("range-for structured binding sample should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "sum" => Some(function),
                _ => None,
            })
            .expect("expected sum function");
        let [Statement::LocalDecl { .. }, Statement::For {
            initializer,
            condition: Some(condition),
            update,
            body,
            ..
        }, Statement::Return { .. }] = function.body.as_slice()
        else {
            panic!("expected local, range-for, return");
        };
        assert!(update.is_none());
        assert!(matches!(condition, Expression::Identifier { name, .. } if name == "pairs"));
        assert!(matches!(
            initializer.as_slice(),
            [Statement::StructuredBinding {
                type_name,
                names,
                initializer: Some(Expression::Identifier { name, .. }),
                ..
            }] if type_name == "auto" && names == &vec!["first".to_string(), "second".to_string()] && name == "pairs"
        ));
        assert!(matches!(
            body.as_slice(),
            [Statement::Assignment { right, .. }]
                if matches!(right, Expression::Binary { operator, .. } if operator == "+")
        ));
    }

    #[test]
    fn parses_cast_sizeof_conditional_and_compound_assignment_expressions() {
        let sample = r#"
                int score(int x) {
                  int y = (int)sizeof(x);
                  int alignment = alignof(int);
                  y += x > 0 ? x : -x;
                  return y;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("expression sample should parse");
        let Declaration::Function(function) = &declarations[0] else {
            panic!("expected function declaration");
        };

        let [Statement::LocalDecl {
            initializer: Some(initializer),
            ..
        }, Statement::LocalDecl {
            initializer: Some(alignof_initializer),
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
        assert!(matches!(
            alignof_initializer,
            Expression::SizeOf {
                value: None,
                type_name: Some(type_name),
                ..
            } if type_name == "int"
        ));

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
    fn parses_raw_assignment_initializer_text_without_confusing_comparisons_or_operators() {
        let expression = parse_expression_text("target = {{1, seed}, 2}", 1);
        assert!(matches!(
            expression,
            Expression::Assignment {
                operator,
                left,
                right,
                ..
            } if operator == "="
                && matches!(left.as_ref(), Expression::Identifier { name, .. } if name == "target")
                && matches!(right.as_ref(), Expression::InitializerList { .. })
        ));

        let designated = parse_expression_text(".cell = {1, seed}", 1);
        assert!(matches!(
            designated,
            Expression::DesignatedInitializer {
                designator,
                value,
                ..
            } if matches!(designator.as_ref(), Expression::Designator { code, .. } if code == ".cell")
                && matches!(value.as_ref(), Expression::InitializerList { .. })
        ));

        assert!(matches!(
            parse_expression_text("left == right", 1),
            Expression::Identifier { .. }
        ));
        assert!(matches!(
            parse_expression_text("operator=(value)", 1),
            Expression::Call { name, .. } if name == "operator="
        ));
        assert!(matches!(
            parse_expression_text("cooperator = value", 1),
            Expression::Assignment { left, .. }
                if matches!(left.as_ref(), Expression::Identifier { name, .. } if name == "cooperator")
        ));
    }

    #[test]
    fn parses_nested_assignment_expressions() {
        let sample = r#"
                int score(int x, int y) {
                  int z = 0;
                  return (z = x) + y;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("nested assignment expression sample should parse");
        let Declaration::Function(function) = &declarations[0] else {
            panic!("expected function declaration");
        };
        let [Statement::LocalDecl { .. }, Statement::Return {
            expression: Some(return_expression),
            ..
        }] = function.body.as_slice()
        else {
            panic!("expected local declaration and return");
        };
        let Expression::Binary { left, right, .. } = return_expression else {
            panic!("expected binary return expression");
        };
        assert!(matches!(
            left.as_ref(),
            Expression::Assignment { operator, left, right, .. }
                if operator == "="
                    && matches!(left.as_ref(), Expression::Identifier { name, .. } if name == "z")
                    && matches!(right.as_ref(), Expression::Identifier { name, .. } if name == "x")
        ));
        assert!(matches!(
            right.as_ref(),
            Expression::Identifier { name, .. } if name == "y"
        ));
    }

    #[test]
    fn parses_cpp_three_way_comparison_expressions() {
        let sample = r#"
                bool foo() {
                  bool x = 1 <=> 2;
                  return x;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("three-way comparison sample should parse");
        let Declaration::Function(function) = &declarations[0] else {
            panic!("expected function declaration");
        };
        let [Statement::LocalDecl {
            initializer: Some(initializer),
            ..
        }, Statement::Return { .. }] = function.body.as_slice()
        else {
            panic!("expected local declaration and return");
        };
        assert_binary_operator(initializer, "<=>");
    }

    #[test]
    fn parses_cpp_named_cast_expressions() {
        let sample = r#"
                int casts(float x, void *ptr) {
                  int a = static_cast<int>(x);
                  int b = const_cast<int>(a);
                  int c = dynamic_cast<int>(b);
                  int d = reinterpret_cast<int>(ptr);
                  return a + b + c + d;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("named cast sample should parse");
        let Declaration::Function(function) = &declarations[0] else {
            panic!("expected function declaration");
        };
        let casts = function
            .body
            .iter()
            .filter_map(|statement| match statement {
                Statement::LocalDecl {
                    initializer: Some(initializer),
                    ..
                } => Some(initializer),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(casts.len(), 4);
        for cast in casts {
            let Expression::Cast {
                type_name, value, ..
            } = cast
            else {
                panic!("expected named cast initializer");
            };
            assert_eq!(type_name, "int");
            assert!(matches!(value.as_ref(), Expression::Identifier { .. }));
        }
    }

    #[test]
    fn parses_cpp_boolean_and_null_literals() {
        let sample = r#"
                bool flags(int *ptr) {
                  bool ok = true;
                  bool nope = false;
                  return ptr != nullptr;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("boolean and null literal sample should parse");
        let Declaration::Function(function) = &declarations[0] else {
            panic!("expected function declaration");
        };
        let [Statement::LocalDecl {
            name: ok_name,
            type_name: ok_type,
            initializer:
                Some(Expression::Literal {
                    value: ok_value, ..
                }),
            ..
        }, Statement::LocalDecl {
            name: nope_name,
            type_name: nope_type,
            initializer:
                Some(Expression::Literal {
                    value: nope_value, ..
                }),
            ..
        }, Statement::Return {
            expression: Some(Expression::Binary {
                operator, right, ..
            }),
            ..
        }] = function.body.as_slice()
        else {
            panic!("expected boolean locals and null comparison return");
        };
        assert_eq!(
            (ok_name.as_str(), ok_type.as_str(), ok_value.as_str()),
            ("ok", "bool", "true")
        );
        assert_eq!(
            (nope_name.as_str(), nope_type.as_str(), nope_value.as_str()),
            ("nope", "bool", "false")
        );
        assert_eq!(operator, "!=");
        assert!(matches!(
            right.as_ref(),
            Expression::Literal { value, .. } if value == "nullptr"
        ));
    }

    #[test]
    fn parses_cpp_extended_string_literals() {
        let sample = r#"
                const char *strings() {
                  const char *raw = R"(hello)";
                  const char *joined = "a" "b";
                  auto tagged = 42_km;
                  return raw;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("extended string literal sample should parse");
        let Declaration::Function(function) = &declarations[0] else {
            panic!("expected function declaration");
        };
        let literal_initializers = function
            .body
            .iter()
            .filter_map(|statement| match statement {
                Statement::LocalDecl {
                    name,
                    initializer: Some(Expression::Literal { value, .. }),
                    ..
                } => Some((name.as_str(), value.as_str())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            literal_initializers,
            vec![
                ("raw", "R\"(hello)\""),
                ("joined", "\"a\" \"b\""),
                ("tagged", "42_km"),
            ]
        );
    }

    #[test]
    fn parses_cpp_utf8_literals_and_const_keywords() {
        let sample = r#"
                char8_t utf8_str[] = u8"abcde";
                consteval int sqr(int n) { return n * n; }
                void chars() {
                  char x = u8'x';
                  constinit const char *c = "ready";
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("UTF-8 literal and const keyword sample should parse");
        let global = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::GlobalVariable(global) if global.name == "utf8_str" => Some(global),
                _ => None,
            })
            .expect("expected utf8_str global");
        assert_eq!(global.type_name, "char8_t[]");
        assert!(matches!(
            global.initializer.as_ref(),
            Some(Expression::Literal { value, .. }) if value == "u8\"abcde\""
        ));

        let sqr = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "sqr" => Some(function),
                _ => None,
            })
            .expect("expected consteval function");
        assert_eq!(sqr.return_type, "int");
        assert_eq!(sqr.signature, "int(int)");

        let chars = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "chars" => Some(function),
                _ => None,
            })
            .expect("expected chars function");
        let [Statement::LocalDecl {
            name: x_name,
            initializer: Some(Expression::Literal { value: x_value, .. }),
            ..
        }, Statement::LocalDecl {
            name: c_name,
            type_name: c_type,
            code: c_code,
            ..
        }] = chars.body.as_slice()
        else {
            panic!("expected UTF-8 char local and constinit local");
        };
        assert_eq!(x_name, "x");
        assert_eq!(x_value, "u8'x'");
        assert_eq!(c_name, "c");
        assert_eq!(c_type, "char*");
        assert_eq!(c_code, "constinit const char *c = \"ready\"");
    }

    #[test]
    fn parses_cpp_offsetof_expressions() {
        let sample = r#"
                struct Pair {
                  int first;
                  int second;
                };
                int offset() {
                  return offsetof(Pair, second);
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("offsetof expression sample should parse");
        let Declaration::Function(function) = &declarations[1] else {
            panic!("expected function declaration");
        };
        let [Statement::Return {
            expression: Some(Expression::Call {
                name, arguments, ..
            }),
            ..
        }] = function.body.as_slice()
        else {
            panic!("expected offsetof return call");
        };
        assert_eq!(name, "offsetof");
        assert!(matches!(
            arguments.as_slice(),
            [
                Expression::Literal { value: type_name, .. },
                Expression::Literal { value: member, .. }
            ] if type_name == "Pair" && member == "second"
        ));
    }

    #[test]
    fn parses_cpp_fold_expressions() {
        let sample = r#"
                template <typename... Args>
                bool logicalAnd(Args... args) {
                  return (true && ... && args);
                }
                template <typename... Args>
                auto sum(Args... args) {
                  return (... + args);
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("fold expression sample should parse");
        let logical_and = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "logicalAnd" => Some(function),
                _ => None,
            })
            .expect("expected logicalAnd function");
        assert_eq!(logical_and.parameters.len(), 1);
        assert_eq!(logical_and.parameters[0].name, "args");
        assert_eq!(logical_and.parameters[0].type_name, "Args");
        let [Statement::Return {
            expression:
                Some(Expression::Fold {
                    operator,
                    left: Some(left),
                    right: Some(right),
                    ..
                }),
            ..
        }] = logical_and.body.as_slice()
        else {
            panic!("expected binary fold return");
        };
        assert_eq!(operator, "&&");
        assert!(matches!(left.as_ref(), Expression::Literal { value, .. } if value == "true"));
        assert!(matches!(right.as_ref(), Expression::Identifier { name, .. } if name == "args"));

        let sum = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "sum" => Some(function),
                _ => None,
            })
            .expect("expected sum function");
        assert_eq!(sum.parameters.len(), 1);
        assert_eq!(sum.parameters[0].name, "args");
        assert_eq!(sum.parameters[0].type_name, "Args");
        let [Statement::Return {
            expression:
                Some(Expression::Fold {
                    operator,
                    left: None,
                    right: Some(right),
                    ..
                }),
            ..
        }] = sum.body.as_slice()
        else {
            panic!("expected unary fold return");
        };
        assert_eq!(operator, "+");
        assert!(matches!(right.as_ref(), Expression::Identifier { name, .. } if name == "args"));
    }

    #[test]
    fn recovers_cpp_coroutine_statements() {
        let sample = r#"
                int main() {
                  co_await x();
                  co_return y();
                }

                generator<int> range(int start, int end) {
                  while (start < end) {
                    co_yield start;
                    start++;
                  }
                }

                task<void> echo(socket s) {
                  auto data = co_await s.async_read();
                  co_await async_write(s, data);
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("coroutine recovery sample should parse");

        let main_function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "main" => Some(function),
                _ => None,
            })
            .expect("expected main function");
        let [Statement::Expression {
            expression:
                Expression::Unary {
                    operator: await_operator,
                    code: await_code,
                    argument: await_argument,
                    ..
                },
            ..
        }, Statement::Return {
            code: return_code,
            expression: Some(Expression::Call {
                name: return_name, ..
            }),
            ..
        }] = main_function.body.as_slice()
        else {
            panic!("expected structured coroutine await and return statements");
        };
        assert_eq!(await_operator, "co_await");
        assert_eq!(await_code, "co_await x()");
        assert!(matches!(await_argument.as_ref(), Expression::Call { name, .. } if name == "x"));
        assert_eq!(return_code, "co_return y()");
        assert_eq!(return_name, "y");

        let range_function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "range" => Some(function),
                _ => None,
            })
            .expect("expected range function");
        let [Statement::While { body, .. }] = range_function.body.as_slice() else {
            panic!("expected range while statement");
        };
        let [Statement::Expression {
            expression:
                Expression::Unary {
                    operator,
                    code,
                    argument,
                    ..
                },
            ..
        }, Statement::Expression { .. }] = body.as_slice()
        else {
            panic!("expected structured co_yield expression in while body");
        };
        assert_eq!(operator, "co_yield");
        assert_eq!(code, "co_yield start");
        assert!(
            matches!(argument.as_ref(), Expression::Identifier { name, .. } if name == "start")
        );

        let echo_function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "echo" => Some(function),
                _ => None,
            })
            .expect("expected echo function");
        let [Statement::LocalDecl {
            initializer:
                Some(Expression::Unary {
                    operator: local_await_operator,
                    argument: local_await_argument,
                    ..
                }),
            ..
        }, Statement::Expression {
            expression:
                Expression::Unary {
                    operator: write_await_operator,
                    code: await_code,
                    argument: write_await_argument,
                    ..
                },
            ..
        }] = echo_function.body.as_slice()
        else {
            panic!("expected structured co_await initializer and statement");
        };
        assert_eq!(local_await_operator, "co_await");
        assert!(
            matches!(local_await_argument.as_ref(), Expression::Call { name, code, .. } if name == "s.async_read" && code == "s.async_read()")
        );
        assert_eq!(write_await_operator, "co_await");
        assert_eq!(await_code, "co_await async_write(s, data)");
        assert!(
            matches!(write_await_argument.as_ref(), Expression::Call { name, code, .. } if name == "async_write" && code == "async_write(s, data)")
        );
    }

    #[test]
    fn parses_cpp_parameter_pack_expansions() {
        let sample = r#"
                void foo(int x, int*... args) {
                  foo(x, args...);
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("parameter pack expansion sample should parse");
        let Declaration::Function(function) = &declarations[0] else {
            panic!("expected function declaration");
        };
        assert_eq!(function.parameters.len(), 2);
        assert_eq!(function.parameters[1].name, "args");
        assert_eq!(function.parameters[1].type_name, "int*");
        assert!(function.parameters[1].is_variadic);
        let [Statement::Expression {
            expression: Expression::Call { arguments, .. },
            ..
        }] = function.body.as_slice()
        else {
            panic!("expected call expression statement");
        };
        assert!(matches!(
            arguments.as_slice(),
            [
                Expression::Identifier { name: first, .. },
                Expression::PackExpansion { pattern, .. }
            ] if first == "x"
                && matches!(pattern.as_ref(), Expression::Identifier { name, .. } if name == "args")
        ));
    }

    #[test]
    fn parses_cpp_classic_varargs() {
        let sample = r#"
                int foo(const char *a, ...) { return 0; }
                void bar(int x, int args...) {}
                "#;
        let declarations =
            parse_declarations(sample, SourceLanguage::Cpp).expect("varargs sample should parse");
        let functions: Vec<&FunctionDecl> = declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Function(function) => Some(function),
                _ => None,
            })
            .collect();
        assert_eq!(functions.len(), 2);

        let foo = functions[0];
        assert_eq!(foo.signature, "int(char*,...)");
        assert_eq!(foo.parameters.len(), 2);
        assert_eq!(foo.parameters[0].name, "a");
        assert_eq!(foo.parameters[0].type_name, "char*");
        assert!(!foo.parameters[0].is_variadic);
        assert_eq!(foo.parameters[1].name, "<param>2");
        assert_eq!(foo.parameters[1].code, "<param>2...");
        assert_eq!(foo.parameters[1].type_name, "char*");
        assert!(foo.parameters[1].is_variadic);

        let bar = functions[1];
        assert_eq!(bar.signature, "void(int,int,...)");
        assert_eq!(bar.parameters.len(), 3);
        assert_eq!(bar.parameters[1].name, "args");
        assert_eq!(bar.parameters[1].type_name, "int");
        assert!(!bar.parameters[1].is_variadic);
        assert_eq!(bar.parameters[2].name, "<param>3");
        assert_eq!(bar.parameters[2].code, "<param>3...");
        assert_eq!(bar.parameters[2].type_name, "int");
        assert!(bar.parameters[2].is_variadic);

        let c_declarations = parse_declarations(
            "int baz(const char *a, ...) { return 0; }",
            SourceLanguage::C,
        )
        .expect("C varargs sample should parse");
        let Declaration::Function(baz) = &c_declarations[0] else {
            panic!("expected C function declaration");
        };
        assert_eq!(baz.signature, "int(char*,...)");
        assert_eq!(baz.parameters.len(), 2);
        assert_eq!(baz.parameters[1].name, "<param>2");
        assert_eq!(baz.parameters[1].type_name, "char*");
        assert!(baz.parameters[1].is_variadic);
    }

    #[test]
    fn preserves_cpp_auto_pointer_and_reference_declarators() {
        let sample = r#"
                int refs(int x, int *ptr) {
                  auto &ref = x;
                  auto &&rref = static_cast<int>(x);
                  auto *copied = ptr;
                  auto *addressed = &x;
                  return ref + rref + *copied + *addressed;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("auto declarator sample should parse");
        let Declaration::Function(function) = &declarations[0] else {
            panic!("expected function declaration");
        };
        let local_types = function
            .body
            .iter()
            .filter_map(|statement| match statement {
                Statement::LocalDecl {
                    name, type_name, ..
                } => Some((name.as_str(), type_name.as_str())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            local_types,
            vec![
                ("ref", "auto&"),
                ("rref", "auto&&"),
                ("copied", "auto*"),
                ("addressed", "auto*"),
            ]
        );
    }

    #[test]
    fn parses_cpp_structured_binding_declarations() {
        let sample = r#"
                struct Pair {
                  int first;
                  int second;
                };
                Pair make();
                int use() {
                  auto [first, second] = make();
                  return first + second;
                }
                "#;
        let declarations = parse_declarations(sample, SourceLanguage::Cpp)
            .expect("structured binding sample should parse");
        let function = declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == "use" => Some(function),
                _ => None,
            })
            .expect("expected use function");
        let [Statement::StructuredBinding {
            type_name,
            temp_name,
            names,
            initializer: Some(initializer),
            ..
        }, Statement::Return { .. }] = function.body.as_slice()
        else {
            panic!("expected structured binding followed by return");
        };
        assert_eq!(type_name, "auto");
        assert!(temp_name.starts_with("<tmp>"));
        assert_eq!(names, &vec!["first".to_string(), "second".to_string()]);
        assert!(matches!(initializer, Expression::Call { name, .. } if name == "make"));
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
            Expression::Binary { left, right, .. } | Expression::Assignment { left, right, .. } => {
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

    fn find_call_by_name<'a>(
        expression: &'a Expression,
        expected_name: &str,
    ) -> Option<&'a Expression> {
        match expression {
            Expression::Call {
                name,
                callee,
                arguments,
                ..
            } => {
                if name == expected_name {
                    Some(expression)
                } else {
                    find_call_by_name(callee, expected_name).or_else(|| {
                        arguments
                            .iter()
                            .find_map(|argument| find_call_by_name(argument, expected_name))
                    })
                }
            }
            Expression::Binary { left, right, .. } | Expression::Assignment { left, right, .. } => {
                find_call_by_name(left, expected_name)
                    .or_else(|| find_call_by_name(right, expected_name))
            }
            Expression::Conditional {
                condition,
                consequence,
                alternative,
                ..
            } => find_call_by_name(condition, expected_name)
                .or_else(|| {
                    consequence
                        .as_deref()
                        .and_then(|consequence| find_call_by_name(consequence, expected_name))
                })
                .or_else(|| find_call_by_name(alternative, expected_name)),
            Expression::Unary { argument, .. }
            | Expression::Cast {
                value: argument, ..
            }
            | Expression::Delete { argument, .. }
            | Expression::FieldAccess { base: argument, .. } => {
                find_call_by_name(argument, expected_name)
            }
            Expression::SizeOf { value, .. } => value
                .as_deref()
                .and_then(|value| find_call_by_name(value, expected_name)),
            Expression::New { arguments, .. }
            | Expression::InitializerList {
                elements: arguments,
                ..
            } => arguments
                .iter()
                .find_map(|argument| find_call_by_name(argument, expected_name)),
            Expression::IndexAccess { base, index, .. } => find_call_by_name(base, expected_name)
                .or_else(|| find_call_by_name(index, expected_name)),
            Expression::DesignatedInitializer {
                designator, value, ..
            } => find_call_by_name(designator, expected_name)
                .or_else(|| find_call_by_name(value, expected_name)),
            Expression::Lambda { body, .. } => body.iter().find_map(|statement| match statement {
                Statement::Return {
                    expression: Some(expression),
                    ..
                }
                | Statement::Expression { expression, .. } => {
                    find_call_by_name(expression, expected_name)
                }
                _ => None,
            }),
            _ => None,
        }
    }

    fn collect_statement_call_names(statement: &Statement) -> Vec<String> {
        match statement {
            Statement::Unknown { .. } | Statement::UsingEnum { .. } => Vec::new(),
            Statement::LocalDecl { initializer, .. } => initializer
                .as_ref()
                .map(collect_call_names)
                .unwrap_or_default(),
            Statement::StructuredBinding { initializer, .. } => initializer
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
                initializer,
                condition_initializer,
                condition,
                then_body,
                else_body,
                ..
            } => {
                let mut calls = initializer
                    .iter()
                    .flat_map(collect_statement_call_names)
                    .collect::<Vec<_>>();
                calls.extend(
                    condition_initializer
                        .iter()
                        .flat_map(collect_statement_call_names),
                );
                calls.extend(collect_call_names(condition));
                calls.extend(then_body.iter().flat_map(collect_statement_call_names));
                calls.extend(else_body.iter().flat_map(collect_statement_call_names));
                calls
            }
            Statement::While {
                initializer,
                condition_initializer,
                condition,
                body,
                ..
            } => {
                let mut calls = initializer
                    .iter()
                    .flat_map(collect_statement_call_names)
                    .collect::<Vec<_>>();
                calls.extend(
                    condition_initializer
                        .iter()
                        .flat_map(collect_statement_call_names),
                );
                calls.extend(collect_call_names(condition));
                calls.extend(body.iter().flat_map(collect_statement_call_names));
                calls
            }
            Statement::DoWhile {
                condition, body, ..
            } => {
                let mut calls = collect_call_names(condition);
                calls.extend(body.iter().flat_map(collect_statement_call_names));
                calls
            }
            Statement::Switch {
                initializer,
                condition_initializer,
                condition,
                body,
                ..
            } => {
                let mut calls = initializer
                    .iter()
                    .flat_map(collect_statement_call_names)
                    .collect::<Vec<_>>();
                calls.extend(
                    condition_initializer
                        .iter()
                        .flat_map(collect_statement_call_names),
                );
                calls.extend(collect_call_names(condition));
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
            Statement::Unknown { line, .. }
            | Statement::UsingEnum { line, .. }
            | Statement::LocalDecl { line, .. }
            | Statement::StructuredBinding { line, .. }
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

    fn single_function_body(source: &str, language: SourceLanguage) -> Vec<Statement> {
        let declarations = parse_declarations(source, language).expect("source should parse");
        declarations
            .into_iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.is_definition => Some(function.body),
                _ => None,
            })
            .expect("expected a function definition")
    }

    #[test]
    fn comma_expression_lowers_to_binary_chain() {
        fn collect_literals(expression: &Expression, out: &mut Vec<String>) {
            match expression {
                Expression::Literal { value, .. } => out.push(value.clone()),
                Expression::Binary { left, right, .. } => {
                    collect_literals(left, out);
                    collect_literals(right, out);
                }
                _ => {}
            }
        }

        let body = single_function_body(
            "int f() { int x; x = (1, 2, 3); return x; }",
            SourceLanguage::C,
        );
        let Some(Statement::Assignment { right, .. }) = body
            .iter()
            .find(|statement| matches!(statement, Statement::Assignment { .. }))
        else {
            panic!("expected assignment statement, got {body:?}");
        };
        // The comma operator lowers onto the existing `binary` kind with operator
        // `,`, preserving every operand rather than collapsing to one identifier.
        assert_binary_operator(right, ",");
        let mut literals = Vec::new();
        collect_literals(right, &mut literals);
        assert_eq!(literals, vec!["1", "2", "3"]);
    }

    #[test]
    fn seh_try_statement_lowers_to_try_with_except_catch() {
        let body = single_function_body(
            "void f() { __try { g(); } __except(1) { handle(); } }",
            SourceLanguage::Cpp,
        );
        let try_statement = body
            .iter()
            .find(|statement| matches!(statement, Statement::Try { .. }))
            .expect("expected try statement");
        let Statement::Try { body, catches, .. } = try_statement else {
            unreachable!();
        };
        assert_eq!(catches.len(), 1);
        assert!(catches[0].parameter.is_none());
        // The guarded block and the `__except` body are both preserved.
        assert_eq!(
            collect_statement_call_names(try_statement),
            vec!["g", "handle"]
        );
        assert!(!body.is_empty());
    }

    #[test]
    fn seh_finally_statements_are_appended_to_try_body() {
        let body = single_function_body(
            "void f() { __try { g(); } __finally { cleanup(); } }",
            SourceLanguage::C,
        );
        let try_statement = body
            .into_iter()
            .find(|statement| matches!(statement, Statement::Try { .. }))
            .expect("expected try statement");
        let Statement::Try { body, catches, .. } = &try_statement else {
            unreachable!();
        };
        assert!(catches.is_empty());
        // `__finally` statements always run, so they are folded onto the body.
        assert_eq!(body.len(), 2);
        assert_eq!(
            collect_statement_call_names(&try_statement),
            vec!["g", "cleanup"]
        );
    }

    #[test]
    fn unmapped_node_kinds_are_tallied_in_summary() {
        // Drain any residue from earlier tests sharing this thread.
        let _ = take_unmapped_summary();
        // A GCC statement-expression (`({ ... })`) has no dedicated mapping and
        // falls through to the recorded `Statement::Expression` fallback.
        let _ = parse_declarations("int f() { int x = ({ 1; }); return x; }", SourceLanguage::C)
            .expect("source should parse");
        let summary = take_unmapped_summary().expect("expected an unmapped summary");
        assert!(
            summary.starts_with("cxxastgen: "),
            "unexpected summary: {summary}"
        );
        assert!(
            summary.contains("unmapped node(s):"),
            "unexpected summary: {summary}"
        );
        assert!(
            summary.contains("(x"),
            "summary should carry per-kind counts: {summary}"
        );
        // The counter drains on read, so a clean follow-up reports nothing.
        assert!(take_unmapped_summary().is_none());
    }

    #[test]
    fn fully_mapped_source_produces_no_unmapped_summary() {
        let _ = take_unmapped_summary();
        let _ = parse_declarations(
            "int add(int a, int b) { int total = a + b; return total; }",
            SourceLanguage::C,
        )
        .expect("source should parse");
        assert_eq!(take_unmapped_summary(), None);
    }

    // ---- Phase-1 symbol table + call resolution -------------------------------

    fn function_named<'a>(declarations: &'a [Declaration], name: &str) -> &'a FunctionDecl {
        find_function_named(declarations, name)
            .unwrap_or_else(|| panic!("expected function `{name}`"))
    }

    fn find_function_named<'a>(
        declarations: &'a [Declaration],
        name: &str,
    ) -> Option<&'a FunctionDecl> {
        declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == name => Some(function),
                Declaration::Namespace(namespace) => {
                    find_function_named(&namespace.declarations, name)
                }
                Declaration::Struct(struct_decl) => {
                    find_function_named(&struct_decl.nested_declarations, name)
                }
                _ => None,
            })
    }

    /// Returns `(resolvedMethodFullName, resolvedSignature)` for the first call
    /// of `call_name` reachable from `function`'s body.
    fn resolved_call(
        function: &FunctionDecl,
        call_name: &str,
    ) -> Option<(Option<String>, Option<String>)> {
        function.body.iter().find_map(|statement| {
            statement_expressions(statement)
                .into_iter()
                .find_map(|expression| {
                    find_call_by_name(expression, call_name).map(|call| match call {
                        Expression::Call {
                            resolved_method_full_name,
                            resolved_signature,
                            ..
                        } => (
                            resolved_method_full_name.clone(),
                            resolved_signature.clone(),
                        ),
                        _ => unreachable!("find_call_by_name returns a call"),
                    })
                })
        })
    }

    fn statement_expressions(statement: &Statement) -> Vec<&Expression> {
        match statement {
            Statement::Return { expression, .. } | Statement::Throw { expression, .. } => {
                expression.iter().collect()
            }
            Statement::LocalDecl { initializer, .. }
            | Statement::StructuredBinding { initializer, .. } => initializer.iter().collect(),
            Statement::Expression { expression, .. } => vec![expression],
            Statement::Assignment { left, right, .. } => vec![left, right],
            _ => Vec::new(),
        }
    }

    #[test]
    fn symbol_table_collects_qualified_names_and_arity() {
        let declarations = parse_declarations(
            r#"
                namespace Core {
                int add(int a, int b) { return a + b; }
                class Widget {
                public:
                  int render(int scale) { return scale; }
                };
                }
                int top(void) { return 0; }
            "#,
            SourceLanguage::Cpp,
        )
        .expect("symbol-table sample should parse");

        let mut table = SymbolTable::default();
        collect_symbols(&mut table, "", &declarations);

        let add = &table.by_qualified_name.get("Core.add").expect("Core.add")[0];
        assert_eq!(add.simple_name, "add");
        assert_eq!(add.arity, 2);
        assert!(add.is_definition);
        assert_eq!(add.signature, "int(int,int)");

        let render = &table
            .by_qualified_name
            .get("Core.Widget.render")
            .expect("Core.Widget.render")[0];
        assert_eq!(render.arity, 1);
        assert_eq!(render.signature, "int(int)");

        // Simple-name index carries every entry too.
        assert!(table.by_simple_name.contains_key("render"));
        assert!(table.by_simple_name.contains_key("top"));
    }

    #[test]
    fn resolves_unambiguous_free_function_call() {
        let declarations = parse_declarations(
            r#"
                namespace Core {
                int helper(int value) { return value + 1; }
                int run() { return helper(7); }
                }
            "#,
            SourceLanguage::Cpp,
        )
        .expect("resolved-call sample should parse");

        let run = function_named(&declarations, "run");
        assert_eq!(
            resolved_call(run, "helper"),
            Some((
                Some("Core.helper:int(int)".to_string()),
                Some("int(int)".to_string())
            ))
        );
    }

    #[test]
    fn resolves_overload_uniquely_by_arity() {
        let declarations = parse_declarations(
            r#"
                int make(int a) { return a; }
                int make(int a, int b) { return a + b; }
                int use() { return make(1, 2); }
            "#,
            SourceLanguage::Cpp,
        )
        .expect("overload sample should parse");

        let use_fn = function_named(&declarations, "use");
        let (full_name, signature) = resolved_call(use_fn, "make").expect("make call present");
        assert_eq!(full_name, Some("make:int(int,int)".to_string()));
        assert_eq!(signature, Some("int(int,int)".to_string()));
    }

    #[test]
    fn leaves_ambiguous_same_arity_overload_unresolved() {
        let declarations = parse_declarations(
            r#"
                int pick(int a) { return a; }
                int pick(double a) { return 0; }
                int use() { return pick(1); }
            "#,
            SourceLanguage::Cpp,
        )
        .expect("ambiguous sample should parse");

        let use_fn = function_named(&declarations, "use");
        // Two arity-1 definitions => ambiguous => no resolution stamped.
        assert_eq!(resolved_call(use_fn, "pick"), Some((None, None)));
    }

    #[test]
    fn does_not_resolve_prototype_only_calls() {
        // Mirrors the CDT parity behaviour: a prototype with no definition in
        // the translation unit stays a bare external name, so we must not stamp
        // a resolved full name onto its call sites.
        let declarations = parse_declarations(
            r#"
                int external(int value);
                int defined(int value) { return external(value); }
            "#,
            SourceLanguage::C,
        )
        .expect("prototype sample should parse");

        let defined = function_named(&declarations, "defined");
        assert_eq!(resolved_call(defined, "external"), Some((None, None)));
    }

    #[test]
    fn resolves_member_call_by_simple_name() {
        let declarations = parse_declarations(
            r#"
                struct Box {
                  int area(int w) { return w; }
                  int use(int w) { return area(w); }
                };
            "#,
            SourceLanguage::Cpp,
        )
        .expect("member sample should parse");

        let use_fn = function_named(&declarations, "use");
        // `area` is reachable by simple name from the unique definition in `Box`.
        assert_eq!(
            resolved_call(use_fn, "area"),
            Some((
                Some("Box.area:int(int)".to_string()),
                Some("int(int)".to_string())
            ))
        );
    }

    #[test]
    fn resolved_fields_serialize_only_when_present() {
        let resolved = Expression::Call {
            name: "f".to_string(),
            code: "f()".to_string(),
            line: 1,
            callee: Box::new(Expression::Identifier {
                name: "f".to_string(),
                code: "f".to_string(),
                line: 1,
            }),
            arguments: Vec::new(),
            resolved_method_full_name: Some("f:int()".to_string()),
            resolved_signature: Some("int()".to_string()),
        };
        let json = serde_json::to_string(&resolved).expect("serialize resolved call");
        assert!(json.contains("\"resolvedMethodFullName\":\"f:int()\""));
        assert!(json.contains("\"resolvedSignature\":\"int()\""));

        let unresolved = Expression::Call {
            name: "f".to_string(),
            code: "f()".to_string(),
            line: 1,
            callee: Box::new(Expression::Identifier {
                name: "f".to_string(),
                code: "f".to_string(),
                line: 1,
            }),
            arguments: Vec::new(),
            resolved_method_full_name: None,
            resolved_signature: None,
        };
        let json = serde_json::to_string(&unresolved).expect("serialize unresolved call");
        assert!(!json.contains("resolvedMethodFullName"));
        assert!(!json.contains("resolvedSignature"));
    }

    // ---- Phase-2 type collection + trivial type inference ---------------------

    /// Returns the `resolvedTypeFullName` of the first `LocalDecl` named `name`
    /// in `function`'s body.
    fn local_resolved_type(function: &FunctionDecl, name: &str) -> Option<Option<String>> {
        function.body.iter().find_map(|statement| match statement {
            Statement::LocalDecl {
                name: local_name,
                resolved_type_full_name,
                ..
            } if local_name == name => Some(resolved_type_full_name.clone()),
            _ => None,
        })
    }

    /// Returns the `resolvedTypeFullName` of the first literal with the given
    /// source `value` reachable from `function`'s body.
    fn literal_resolved_type(function: &FunctionDecl, value: &str) -> Option<Option<String>> {
        function.body.iter().find_map(|statement| {
            statement_expressions(statement)
                .into_iter()
                .find_map(|expression| find_literal_resolved_type(expression, value))
        })
    }

    fn find_literal_resolved_type(expression: &Expression, value: &str) -> Option<Option<String>> {
        match expression {
            Expression::Literal {
                value: literal_value,
                resolved_type_full_name,
                ..
            } if literal_value == value => Some(resolved_type_full_name.clone()),
            Expression::Cast { value: inner, .. } => find_literal_resolved_type(inner, value),
            Expression::Binary { left, right, .. } => find_literal_resolved_type(left, value)
                .or_else(|| find_literal_resolved_type(right, value)),
            _ => None,
        }
    }

    #[test]
    fn symbol_table_collects_qualified_type_names() {
        let declarations = parse_declarations(
            r#"
                namespace Core {
                struct Widget { int value; };
                enum Color { Red, Green };
                typedef Widget WAlias;
                }
                using MyInt = int;
                struct Plain {};
            "#,
            SourceLanguage::Cpp,
        )
        .expect("type-collection sample should parse");

        let mut table = SymbolTable::default();
        collect_symbols(&mut table, "", &declarations);

        assert!(table.types_by_qualified_name.contains_key("Core.Widget"));
        assert!(table.types_by_qualified_name.contains_key("Core.Color"));
        assert!(table.types_by_qualified_name.contains_key("Core.WAlias"));
        assert!(table.types_by_qualified_name.contains_key("MyInt"));
        assert!(table.types_by_qualified_name.contains_key("Plain"));
        // Simple-name index carries the trailing identifier.
        assert!(table.types_by_simple_name.contains_key("Widget"));
        assert!(table.types_by_simple_name.contains_key("Color"));
    }

    #[test]
    fn resolves_explicit_local_type_to_qualified_user_type() {
        let declarations = parse_declarations(
            r#"
                namespace Core {
                struct Widget { int value; };
                }
                int use(Core::Widget seed) {
                  Core::Widget local = seed;
                  Core::Widget* ptr = &seed;
                  int count = 0;
                  return count;
                }
            "#,
            SourceLanguage::Cpp,
        )
        .expect("explicit-type sample should parse");

        let use_fn = function_named(&declarations, "use");
        assert_eq!(
            local_resolved_type(use_fn, "local"),
            Some(Some("Core.Widget".to_string()))
        );
        // Pointer suffix is preserved on the qualified name.
        assert_eq!(
            local_resolved_type(use_fn, "ptr"),
            Some(Some("Core.Widget*".to_string()))
        );
        // Builtins are kept verbatim.
        assert_eq!(
            local_resolved_type(use_fn, "count"),
            Some(Some("int".to_string()))
        );
        // The parameter is resolved too.
        assert_eq!(
            use_fn.parameters[0].resolved_type_full_name,
            Some("Core.Widget".to_string())
        );
    }

    #[test]
    fn infers_literal_types() {
        assert_eq!(infer_literal_type("42"), Some("int".to_string()));
        assert_eq!(infer_literal_type("0x1F"), Some("int".to_string()));
        assert_eq!(infer_literal_type("3.14"), Some("double".to_string()));
        assert_eq!(infer_literal_type("1e9"), Some("double".to_string()));
        assert_eq!(infer_literal_type("2.0f"), Some("double".to_string()));
        assert_eq!(infer_literal_type("'a'"), Some("char".to_string()));
        assert_eq!(infer_literal_type("\"hi\""), Some("char*".to_string()));
        assert_eq!(infer_literal_type("true"), Some("bool".to_string()));
        assert_eq!(infer_literal_type("false"), Some("bool".to_string()));
        assert_eq!(
            infer_literal_type("nullptr"),
            Some("std.nullptr_t".to_string())
        );
        // User-defined / unrecognized literals stay unresolved.
        assert_eq!(infer_literal_type("12_km"), None);
        assert_eq!(infer_literal_type(""), None);
    }

    #[test]
    fn stamps_literal_type_on_expression() {
        let declarations = parse_declarations(
            r#"
                int use() {
                  int i = 7;
                  return i;
                }
            "#,
            SourceLanguage::Cpp,
        )
        .expect("literal sample should parse");

        let use_fn = function_named(&declarations, "use");
        assert_eq!(
            literal_resolved_type(use_fn, "7"),
            Some(Some("int".to_string()))
        );
    }

    #[test]
    fn resolves_cast_target_type() {
        let declarations = parse_declarations(
            r#"
                namespace Core {
                struct Widget { int value; };
                }
                int use(Core::Widget seed) {
                  Core::Widget w = static_cast<Core::Widget>(seed);
                  double d = 1.0;
                  int n = (int) d;
                  return n;
                }
            "#,
            SourceLanguage::Cpp,
        )
        .expect("cast sample should parse");

        let use_fn = function_named(&declarations, "use");
        let casts = collect_casts(&use_fn.body);
        // A user-type cast resolves to the qualified name; the builtin cast to `int`.
        assert!(casts.contains(&("Core.Widget".to_string())));
        assert!(casts.contains(&("int".to_string())));
    }

    fn collect_casts(statements: &[Statement]) -> Vec<String> {
        let mut out = Vec::new();
        for statement in statements {
            for expression in statement_expressions(statement) {
                collect_casts_in_expression(expression, &mut out);
            }
        }
        out
    }

    fn collect_casts_in_expression(expression: &Expression, out: &mut Vec<String>) {
        match expression {
            Expression::Cast {
                value,
                resolved_type_full_name,
                ..
            } => {
                if let Some(resolved) = resolved_type_full_name {
                    out.push(resolved.clone());
                }
                collect_casts_in_expression(value, out);
            }
            Expression::Call {
                callee, arguments, ..
            } => {
                collect_casts_in_expression(callee, out);
                for argument in arguments {
                    collect_casts_in_expression(argument, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn leaves_ambiguous_and_auto_types_unresolved() {
        let declarations = parse_declarations(
            r#"
                namespace A {
                struct Dup { int x; };
                }
                namespace B {
                struct Dup { int y; };
                }
                int use(A::Dup seed) {
                  Dup ambiguous = seed;
                  auto inferred = seed;
                  return 0;
                }
            "#,
            SourceLanguage::Cpp,
        )
        .expect("ambiguous sample should parse");

        let use_fn = function_named(&declarations, "use");
        // `Dup` matches two distinct qualified types by simple name => unresolved.
        assert_eq!(local_resolved_type(use_fn, "ambiguous"), Some(None));
        // `auto` is never trivially inferred.
        assert_eq!(local_resolved_type(use_fn, "inferred"), Some(None));
    }

    #[test]
    fn type_field_serializes_only_when_present() {
        let resolved = Expression::Literal {
            value: "1".to_string(),
            code: "1".to_string(),
            line: 1,
            resolved_type_full_name: Some("int".to_string()),
        };
        let json = serde_json::to_string(&resolved).expect("serialize resolved literal");
        assert!(json.contains("\"resolvedTypeFullName\":\"int\""));

        let unresolved = Expression::Literal {
            value: "x".to_string(),
            code: "x".to_string(),
            line: 1,
            resolved_type_full_name: None,
        };
        let json = serde_json::to_string(&unresolved).expect("serialize unresolved literal");
        assert!(!json.contains("resolvedTypeFullName"));
    }
}
