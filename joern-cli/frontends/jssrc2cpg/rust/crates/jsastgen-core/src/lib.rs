use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tree_sitter::{Language, Node, Parser, Point, Tree};

pub type TypeMap = BTreeMap<String, String>;

pub fn parse_file(root: &Path, path: &Path) -> Result<Value> {
    let content =
        fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_source(root, path, &content)
}

pub fn parse_file_with_source(root: &Path, path: &Path) -> Result<(Value, String)> {
    let content =
        fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let ast = parse_source(root, path, &content)?;
    Ok((ast, content))
}

pub fn parse_source(root: &Path, path: &Path, source: &str) -> Result<Value> {
    let mut value = if is_vue_file(path) {
        parse_vue_source(root, path, source)?
    } else {
        let tree = parse_tree(path, source)?;
        file_json(root, path, source, &tree)
    };
    convert_spans_to_utf16(&mut value, source);
    Ok(value)
}

fn parse_tree(path: &Path, source: &str) -> Result<Tree> {
    let candidates = if contains_ts_export_assignment(source) {
        vec![
            SourceLanguage::TypeScript,
            SourceLanguage::Tsx,
            SourceLanguage::JavaScript,
        ]
    } else {
        language_candidates(path)
    };
    parse_tree_with_candidates(source, candidates.clone()).or_else(|original_error| {
        if let Some(normalized) = normalize_export_default_from_source(source) {
            parse_tree_with_candidates(&normalized, candidates).or(Err(original_error))
        } else {
            Err(original_error)
        }
    })
}

fn contains_ts_export_assignment(source: &str) -> bool {
    source.contains("export =")
}

fn parse_tree_with_candidates(source: &str, candidates: Vec<SourceLanguage>) -> Result<Tree> {
    let mut last_error = None;
    for language in candidates {
        let tree = parse_with_language(source, language)
            .with_context(|| format!("parsing as {}", language.name()))?;
        if !tree.root_node().has_error() {
            return Ok(tree);
        }
        last_error = Some(language.name());
    }

    if let Some(language) = last_error {
        bail!("parser reported syntax errors after trying {language}");
    }
    bail!("parser reported syntax errors");
}

fn normalize_export_default_from_source(source: &str) -> Option<String> {
    let mut bytes = source.as_bytes().to_vec();
    let mut changed = false;
    let mut cursor = 0;

    while let Some(relative) = source[cursor..].find("export ") {
        let export_start = cursor + relative;
        let name_start = export_start + "export ".len();
        let Some(first) = source.as_bytes().get(name_start).copied() else {
            break;
        };
        if !is_identifier_start(first) {
            cursor = name_start;
            continue;
        }

        let mut name_end = name_start + 1;
        while source
            .as_bytes()
            .get(name_end)
            .is_some_and(|byte| is_identifier_continue(*byte))
        {
            name_end += 1;
        }

        if source[name_end..].starts_with(" from ") {
            bytes[export_start + "export".len()] = b'{';
            bytes[name_end] = b'}';
            changed = true;
        }
        cursor = name_end;
    }

    changed.then(|| String::from_utf8(bytes).expect("normalized source is valid UTF-8"))
}

fn is_identifier_start(byte: u8) -> bool {
    byte == b'_' || byte == b'$' || byte.is_ascii_alphabetic()
}

fn is_identifier_continue(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit()
}

fn is_vue_file(path: &Path) -> bool {
    path.extension().and_then(|x| x.to_str()) == Some("vue")
}

fn parse_vue_source(root: &Path, path: &Path, source: &str) -> Result<Value> {
    let mut body = Vec::new();

    let script_source = masked_vue_script_source(source);
    if script_source
        .bytes()
        .any(|byte| !byte.is_ascii_whitespace())
    {
        let tree = parse_tree_with_candidates(
            &script_source,
            vec![
                SourceLanguage::TypeScript,
                SourceLanguage::Tsx,
                SourceLanguage::JavaScript,
            ],
        )
        .with_context(|| format!("parsing {} script blocks", path.display()))?;
        body.extend(program_body_json(tree.root_node(), &script_source));
    }

    let template_source = masked_vue_template_source(source);
    if template_source
        .bytes()
        .any(|byte| !byte.is_ascii_whitespace())
    {
        let tree = parse_tree_with_candidates(&template_source, vec![SourceLanguage::Tsx])
            .with_context(|| format!("parsing {} template blocks", path.display()))?;
        body.extend(program_body_json(tree.root_node(), &template_source));
    }

    body.sort_by_key(|value| {
        value
            .get("start")
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX) as usize
    });

    let program = with_span_bounds(
        "Program",
        0,
        Point { row: 0, column: 0 },
        source.len(),
        point_for_byte(source, source.len()),
        json!({
            "sourceType": "module",
            "interpreter": Value::Null,
            "directives": [],
            "body": body
        }),
    );
    let ast = with_span_bounds(
        "File",
        0,
        Point { row: 0, column: 0 },
        source.len(),
        point_for_byte(source, source.len()),
        json!({
            "program": program,
            "comments": [],
            "tokens": []
        }),
    );

    Ok(json!({
        "fullName": path.to_string_lossy(),
        "relativeName": relative_name(root, path),
        "ast": ast
    }))
}

fn parse_with_language(source: &str, language: SourceLanguage) -> Result<Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&language.language())
        .with_context(|| format!("initializing {} parser", language.name()))?;
    parser
        .parse(source, None)
        .context("parser returned no tree")
}

#[derive(Clone, Copy)]
enum SourceLanguage {
    JavaScript,
    TypeScript,
    Tsx,
}

impl SourceLanguage {
    fn language(self) -> Language {
        match self {
            SourceLanguage::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            SourceLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            SourceLanguage::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }

    fn name(self) -> &'static str {
        match self {
            SourceLanguage::JavaScript => "JavaScript",
            SourceLanguage::TypeScript => "TypeScript",
            SourceLanguage::Tsx => "TSX",
        }
    }
}

fn language_candidates(path: &Path) -> Vec<SourceLanguage> {
    match path.extension().and_then(|x| x.to_str()) {
        Some("ts") => vec![SourceLanguage::TypeScript, SourceLanguage::JavaScript],
        Some("tsx") => vec![
            SourceLanguage::Tsx,
            SourceLanguage::TypeScript,
            SourceLanguage::JavaScript,
        ],
        Some("jsx") => vec![
            SourceLanguage::JavaScript,
            SourceLanguage::Tsx,
            SourceLanguage::TypeScript,
        ],
        _ => vec![
            SourceLanguage::JavaScript,
            SourceLanguage::TypeScript,
            SourceLanguage::Tsx,
        ],
    }
}

pub fn write_json(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

pub fn write_type_map(path: &Path, value: &TypeMap) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

#[derive(Clone, Debug, Default)]
pub struct TypeMapProject {
    files: BTreeMap<String, FileTypeInfo>,
    module_lookup: BTreeMap<String, String>,
    exports: BTreeMap<String, BTreeMap<String, String>>,
}

impl TypeMapProject {
    pub fn from_parsed_files(files: &[(PathBuf, Value, String)]) -> Self {
        let mut project = TypeMapProject::default();
        for (_, ast, source) in files {
            let info = collect_file_type_info(ast, source);
            for key in module_keys_for_relative_name(&info.relative_name) {
                project
                    .module_lookup
                    .entry(key)
                    .or_insert_with(|| info.relative_name.clone());
            }
            project.files.insert(info.relative_name.clone(), info);
        }

        let mut exports = BTreeMap::new();
        for (_, ast, source) in files {
            if let Some(relative_name) = relative_name_from_ast(ast) {
                let mut infer = TypeMapInfer::new(&project, relative_name, source);
                infer.infer_ast(ast);
                exports.insert(relative_name.to_string(), infer.exports);
            }
        }
        project.exports = exports;
        project
    }

    pub fn infer_type_map(&self, ast: &Value, source: &str) -> TypeMap {
        let Some(relative_name) = relative_name_from_ast(ast) else {
            return TypeMap::new();
        };
        let mut infer = TypeMapInfer::new(self, relative_name, source);
        infer.infer_ast(ast);
        infer.type_map
    }

    fn file(&self, relative_name: &str) -> Option<&FileTypeInfo> {
        self.files.get(relative_name)
    }

    fn resolve_module(&self, current_file: &FileTypeInfo, specifier: &str) -> Option<String> {
        if !specifier.starts_with('.') {
            return None;
        }

        let current_dir = current_file
            .relative_name
            .rsplit_once('/')
            .map(|(dir, _)| dir)
            .unwrap_or("");
        let joined = if current_dir.is_empty() {
            specifier.to_string()
        } else {
            format!("{current_dir}/{specifier}")
        };
        let normalized = normalize_module_path(&joined);
        module_keys_for_relative_name(&normalized)
            .into_iter()
            .chain(std::iter::once(normalized))
            .find_map(|key| self.module_lookup.get(&key).cloned())
    }

    fn class_info<'a>(&'a self, file: &'a FileTypeInfo, class_name: &str) -> Option<&'a ClassInfo> {
        file.classes.get(class_name).or_else(|| {
            self.files
                .values()
                .find_map(|candidate| candidate.classes.get(class_name))
        })
    }

    fn imported_member_type(
        &self,
        current_file: &FileTypeInfo,
        module: &str,
        member: &str,
    ) -> Option<String> {
        if let Some(relative_name) = self.resolve_module(current_file, module) {
            if let Some(exports) = self.exports.get(&relative_name).and_then(|x| x.get(member)) {
                return Some(exports.clone());
            }
            if self
                .files
                .get(&relative_name)
                .is_some_and(|info| info.classes.contains_key(member))
            {
                return Some(member.to_string());
            }
            return None;
        }
        Some(format!("{module}:{member}"))
    }
}

#[derive(Clone, Debug, Default)]
struct FileTypeInfo {
    relative_name: String,
    imports: BTreeMap<String, ImportBinding>,
    aliases: BTreeMap<String, String>,
    classes: BTreeMap<String, ClassInfo>,
}

#[derive(Clone, Debug)]
enum ImportBinding {
    Named { module: String, imported: String },
    Namespace { module: String },
    Default { module: String },
}

#[derive(Clone, Debug, Default)]
struct ClassInfo {
    methods: BTreeMap<String, String>,
    properties: BTreeMap<String, String>,
}

fn collect_file_type_info(ast: &Value, source: &str) -> FileTypeInfo {
    let relative_name = relative_name_from_ast(ast).unwrap_or("").to_string();
    let mut info = FileTypeInfo {
        relative_name,
        ..FileTypeInfo::default()
    };

    if let Some(body) = program_body(ast) {
        for statement in body {
            collect_imports(statement, &mut info);
        }
        for statement in body {
            collect_type_aliases(statement, source, &mut info);
        }
        for statement in body {
            collect_classes(statement, source, &mut info);
        }
    }

    info
}

fn collect_imports(statement: &Value, info: &mut FileTypeInfo) {
    match node_type(statement) {
        Some("ImportDeclaration") => {
            let Some(module) = string_field(statement.get("source"), "value") else {
                return;
            };
            for specifier in array_field(statement, "specifiers") {
                match node_type(specifier) {
                    Some("ImportSpecifier") => {
                        let Some(local) = name_from_node(specifier.get("local")) else {
                            continue;
                        };
                        let imported = name_from_node(specifier.get("imported"))
                            .unwrap_or_else(|| local.clone());
                        info.imports.insert(
                            local,
                            ImportBinding::Named {
                                module: module.to_string(),
                                imported,
                            },
                        );
                    }
                    Some("ImportNamespaceSpecifier") => {
                        if let Some(local) = name_from_node(specifier.get("local")) {
                            info.imports.insert(
                                local,
                                ImportBinding::Namespace {
                                    module: module.to_string(),
                                },
                            );
                        }
                    }
                    Some("ImportDefaultSpecifier") => {
                        if let Some(local) = name_from_node(specifier.get("local")) {
                            info.imports.insert(
                                local,
                                ImportBinding::Default {
                                    module: module.to_string(),
                                },
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
        Some("TSImportEqualsDeclaration") => {
            let Some(local) = name_from_node(statement.get("id")) else {
                return;
            };
            let Some(module) = statement
                .get("moduleReference")
                .and_then(|x| x.get("expression"))
                .and_then(|x| string_field(Some(x), "value"))
            else {
                return;
            };
            info.imports.insert(
                local,
                ImportBinding::Namespace {
                    module: module.to_string(),
                },
            );
        }
        _ => {}
    }
}

fn collect_type_aliases(statement: &Value, source: &str, info: &mut FileTypeInfo) {
    match node_type(statement) {
        Some("TSTypeAliasDeclaration") => {
            if let Some(name) = name_from_node(statement.get("id")) {
                let tpe =
                    type_from_annotation(statement.get("typeAnnotation"), source, &info.aliases);
                if tpe != "any" {
                    info.aliases.insert(name, tpe);
                }
            }
        }
        Some("ExportNamedDeclaration") | Some("ExportDefaultDeclaration") => {
            if let Some(declaration) = statement.get("declaration").filter(|x| !x.is_null()) {
                collect_type_aliases(declaration, source, info);
            }
        }
        _ => {}
    }
}

fn collect_classes(statement: &Value, source: &str, info: &mut FileTypeInfo) {
    let declaration = match node_type(statement) {
        Some("ClassDeclaration") | Some("ClassExpression") => Some(statement),
        Some("ExportNamedDeclaration") | Some("ExportDefaultDeclaration") => {
            statement.get("declaration").filter(|x| !x.is_null())
        }
        _ => None,
    };
    let Some(class_node) = declaration else {
        return;
    };
    if !matches!(
        node_type(class_node),
        Some("ClassDeclaration") | Some("ClassExpression")
    ) {
        return;
    }
    let Some(name) = name_from_node(class_node.get("id")) else {
        return;
    };

    let mut class_info = ClassInfo::default();
    for member in class_members(class_node) {
        if matches!(
            node_type(member),
            Some("ClassProperty") | Some("ClassPrivateProperty")
        ) {
            if let Some(member_name) = name_from_node(member.get("key")) {
                let tpe = member
                    .get("typeAnnotation")
                    .map(|annotation| type_from_annotation(Some(annotation), source, &info.aliases))
                    .or_else(|| member.get("value").map(simple_literal_type))
                    .unwrap_or_else(|| "any".to_string());
                if tpe != "any" {
                    class_info.properties.insert(member_name, tpe);
                }
            }
        }
    }
    for member in class_members(class_node) {
        if matches!(
            node_type(member),
            Some("ClassMethod") | Some("TSDeclareMethod")
        ) {
            if let Some(member_name) = name_from_node(member.get("key")) {
                let tpe = function_return_type_from_node(
                    member,
                    source,
                    &info.aliases,
                    &BTreeMap::new(),
                    Some(&class_info),
                    None,
                );
                if tpe != "any" {
                    class_info.methods.insert(member_name, tpe);
                }
            }
        }
    }

    info.classes.insert(name, class_info);
}

struct TypeMapInfer<'a> {
    project: &'a TypeMapProject,
    file: &'a FileTypeInfo,
    source: &'a str,
    env: BTreeMap<String, String>,
    type_map: TypeMap,
    exports: BTreeMap<String, String>,
}

impl<'a> TypeMapInfer<'a> {
    fn new(project: &'a TypeMapProject, relative_name: &str, source: &'a str) -> Self {
        let file = project
            .file(relative_name)
            .unwrap_or_else(|| project.files.values().next().expect("project has files"));
        let mut env = BTreeMap::new();
        for class_name in file.classes.keys() {
            env.insert(class_name.clone(), class_name.clone());
        }
        Self {
            project,
            file,
            source,
            env,
            type_map: TypeMap::new(),
            exports: BTreeMap::new(),
        }
    }

    fn infer_ast(&mut self, ast: &Value) {
        if let Some(body) = program_body(ast) {
            for statement in body {
                self.infer_statement(statement);
            }
        }
    }

    fn infer_statement(&mut self, statement: &Value) -> Vec<(String, String)> {
        match node_type(statement) {
            Some("VariableDeclaration") => array_field(statement, "declarations")
                .iter()
                .filter_map(|declarator| self.infer_variable_declarator(declarator))
                .collect(),
            Some("FunctionDeclaration") => self.infer_function_declaration(statement),
            Some("ClassDeclaration") => self.infer_class_declaration(statement),
            Some("ExportNamedDeclaration") => self.infer_export_named(statement),
            Some("ExportDefaultDeclaration") => self.infer_export_default(statement),
            Some("ExpressionStatement") => {
                if let Some(expression) = statement.get("expression") {
                    self.infer_expr(expression);
                }
                Vec::new()
            }
            Some("BlockStatement") => {
                for child in array_field(statement, "body") {
                    self.infer_statement(child);
                }
                Vec::new()
            }
            Some("ReturnStatement") => {
                if let Some(argument) = statement.get("argument").filter(|x| !x.is_null()) {
                    self.infer_expr(argument);
                }
                Vec::new()
            }
            _ => {
                self.infer_expr(statement);
                Vec::new()
            }
        }
    }

    fn infer_variable_declarator(&mut self, declarator: &Value) -> Option<(String, String)> {
        let id = declarator.get("id")?;
        let name = name_from_node(Some(id))?;
        let init = declarator.get("init").filter(|x| !x.is_null());
        let init_type = init.map(|value| self.infer_expr(value));
        let tpe = self
            .type_from_variable_annotation(declarator)
            .or(init_type)
            .unwrap_or_else(|| "any".to_string());

        self.insert_node_type(declarator, &tpe);
        self.insert_node_type(id, &tpe);
        self.env.insert(name.clone(), tpe.clone());
        Some((name, tpe))
    }

    fn infer_function_declaration(&mut self, function: &Value) -> Vec<(String, String)> {
        let return_type = self.function_return_type(function);
        self.insert_node_type(function, &return_type);
        if let Some(name) = function.get("id").and_then(|id| name_from_node(Some(id))) {
            let signature = self.function_signature(function, &return_type);
            if let Some(id) = function.get("id") {
                self.insert_node_type(id, &signature);
            }
            self.env.insert(name.clone(), signature.clone());
            vec![(name, signature)]
        } else {
            Vec::new()
        }
    }

    fn infer_class_declaration(&mut self, class_node: &Value) -> Vec<(String, String)> {
        let Some(name) = class_node.get("id").and_then(|id| name_from_node(Some(id))) else {
            return Vec::new();
        };
        self.insert_node_type(class_node, &name);
        if let Some(id) = class_node.get("id") {
            self.insert_node_type(id, &name);
        }
        self.env.insert(name.clone(), name.clone());
        for member in class_members(class_node) {
            if matches!(
                node_type(member),
                Some("ClassMethod") | Some("TSDeclareMethod")
            ) {
                let return_type = self.function_return_type(member);
                self.insert_node_type(member, &return_type);
            }
        }
        vec![(name.clone(), name)]
    }

    fn infer_export_named(&mut self, statement: &Value) -> Vec<(String, String)> {
        let mut declarations = Vec::new();
        if let Some(declaration) = statement.get("declaration").filter(|x| !x.is_null()) {
            declarations.extend(self.infer_statement(declaration));
            for (name, tpe) in &declarations {
                self.exports.insert(name.clone(), tpe.clone());
            }
        }
        for specifier in array_field(statement, "specifiers") {
            let Some(local) = name_from_node(specifier.get("local")) else {
                continue;
            };
            let exported =
                name_from_node(specifier.get("exported")).unwrap_or_else(|| local.clone());
            let tpe = statement
                .get("source")
                .and_then(|source| string_field(Some(source), "value"))
                .and_then(|module| self.project.imported_member_type(self.file, module, &local))
                .or_else(|| self.env.get(&local).cloned())
                .unwrap_or_else(|| "any".to_string());
            if tpe != "any" {
                self.exports.insert(exported, tpe);
            }
        }
        declarations
    }

    fn infer_export_default(&mut self, statement: &Value) -> Vec<(String, String)> {
        let Some(declaration) = statement.get("declaration").filter(|x| !x.is_null()) else {
            return Vec::new();
        };
        let declarations = self.infer_statement(declaration);
        if let Some((_, tpe)) = declarations.first() {
            self.exports.insert("default".to_string(), tpe.clone());
        } else {
            let tpe = self.infer_expr(declaration);
            if tpe != "any" {
                self.exports.insert("default".to_string(), tpe);
            }
        }
        declarations
    }

    fn infer_expr(&mut self, expr: &Value) -> String {
        let expression_type = node_type(expr);
        let tpe = match node_type(expr) {
            Some("StringLiteral") | Some("TemplateLiteral") | Some("TemplateElement") => {
                "string".to_string()
            }
            Some("NumericLiteral") | Some("DecimalLiteral") | Some("BigIntLiteral") => {
                "number".to_string()
            }
            Some("BooleanLiteral") => "boolean".to_string(),
            Some("NullLiteral") => "null".to_string(),
            Some("Identifier") => self.identifier_type(expr),
            Some("ThisExpression") => "this".to_string(),
            Some("ArrayExpression") => "any[]".to_string(),
            Some("ObjectExpression") => "{}".to_string(),
            Some("TSAsExpression") | Some("TSTypeAssertion") | Some("TSSatisfiesExpression") => {
                self.type_from_cast_source(expr)
                    .or_else(|| {
                        expr.get("typeAnnotation")
                            .map(|annotation| self.type_from_annotation(Some(annotation)))
                    })
                    .unwrap_or_else(|| {
                        expr.get("expression")
                            .map(|inner| self.infer_expr(inner))
                            .unwrap_or_else(|| "any".to_string())
                    })
            }
            Some("AssignmentExpression") => self.infer_assignment_expression(expr),
            Some("TSNonNullExpression") => expr
                .get("expression")
                .map(|inner| self.infer_expr(inner))
                .unwrap_or_else(|| "any".to_string()),
            Some("ArrowFunctionExpression")
            | Some("FunctionExpression")
            | Some("FunctionDeclaration")
            | Some("ClassMethod")
            | Some("ObjectMethod") => {
                let return_type = self.function_return_type(expr);
                self.insert_node_type(expr, &return_type);
                self.function_signature(expr, &return_type)
            }
            Some("CallExpression") => self.infer_call_expression(expr),
            Some("NewExpression") => self.infer_new_expression(expr),
            Some("MemberExpression") => self.infer_member_expression(expr),
            Some("BinaryExpression") | Some("LogicalExpression") => {
                self.infer_binary_expression(expr)
            }
            Some("UnaryExpression") => self.infer_unary_expression(expr),
            Some("ConditionalExpression") => {
                let consequent = expr
                    .get("consequent")
                    .map(|value| self.infer_expr(value))
                    .unwrap_or_else(|| "any".to_string());
                let alternate = expr
                    .get("alternate")
                    .map(|value| self.infer_expr(value))
                    .unwrap_or_else(|| "any".to_string());
                if consequent == alternate {
                    consequent
                } else {
                    "any".to_string()
                }
            }
            Some("AwaitExpression") => expr
                .get("argument")
                .map(|argument| self.infer_expr(argument))
                .unwrap_or_else(|| "any".to_string()),
            _ => simple_literal_type(expr),
        };
        if !matches!(
            expression_type,
            Some("ArrowFunctionExpression")
                | Some("FunctionExpression")
                | Some("FunctionDeclaration")
                | Some("ClassMethod")
                | Some("ObjectMethod")
        ) {
            self.insert_node_type(expr, &tpe);
        }
        tpe
    }

    fn identifier_type(&self, ident: &Value) -> String {
        let Some(name) = name_from_node(Some(ident)) else {
            return "any".to_string();
        };
        self.env
            .get(&name)
            .cloned()
            .or_else(|| self.type_from_import(&name))
            .or_else(|| self.file.aliases.get(&name).cloned())
            .unwrap_or_else(|| "any".to_string())
    }

    fn type_from_import(&self, name: &str) -> Option<String> {
        match self.file.imports.get(name)? {
            ImportBinding::Named { module, imported } => self
                .project
                .imported_member_type(self.file, module, imported),
            ImportBinding::Default { module } => {
                if module.starts_with('.') {
                    self.project
                        .resolve_module(self.file, module)
                        .and_then(|relative| {
                            self.project
                                .exports
                                .get(&relative)
                                .and_then(|exports| exports.get("default").cloned())
                        })
                } else {
                    Some(format!("{module}:default"))
                }
            }
            ImportBinding::Namespace { .. } => None,
        }
    }

    fn infer_call_expression(&mut self, expr: &Value) -> String {
        let Some(callee) = expr.get("callee") else {
            return "any".to_string();
        };
        match node_type(callee) {
            Some("MemberExpression") => {
                if callee
                    .get("object")
                    .and_then(|object| name_from_node(Some(object)))
                    .is_some_and(|name| name == "Math")
                {
                    return "number".to_string();
                }
                let receiver = callee
                    .get("object")
                    .map(|object| self.infer_expr(object))
                    .unwrap_or_else(|| "any".to_string());
                let property = callee
                    .get("property")
                    .and_then(|property| name_from_node(Some(property)))
                    .unwrap_or_default();
                if receiver == "__ecma.Math" || receiver == "Math" {
                    return "number".to_string();
                }
                if let Some(class_info) = self.project.class_info(self.file, &receiver) {
                    if let Some(return_type) = class_info.methods.get(&property) {
                        return return_type.clone();
                    }
                }
                if receiver != "any" && receiver != "ANY" && !property.is_empty() {
                    return format!("{receiver}:{property}:<returnValue>");
                }
                "any".to_string()
            }
            Some("Identifier") => {
                let name = name_from_node(Some(callee)).unwrap_or_default();
                match name.as_str() {
                    "Number" | "parseInt" | "parseFloat" => "number".to_string(),
                    "String" => "string".to_string(),
                    "Boolean" => "boolean".to_string(),
                    _ => self
                        .env
                        .get(&name)
                        .and_then(|tpe| return_type_from_function_signature(tpe))
                        .unwrap_or_else(|| "any".to_string()),
                }
            }
            _ => "any".to_string(),
        }
    }

    fn infer_assignment_expression(&mut self, expr: &Value) -> String {
        let right_type = expr
            .get("right")
            .map(|right| self.infer_expr(right))
            .unwrap_or_else(|| "any".to_string());
        if let Some(left) = expr.get("left") {
            if matches!(node_type(left), Some("Identifier")) {
                if let Some(name) = name_from_node(Some(left)) {
                    self.env.insert(name, right_type.clone());
                }
                self.insert_node_type(left, &right_type);
            } else {
                self.infer_expr(left);
            }
        }
        right_type
    }

    fn infer_new_expression(&mut self, expr: &Value) -> String {
        expr.get("callee")
            .map(|callee| self.constructor_type(callee))
            .unwrap_or_else(|| "any".to_string())
    }

    fn constructor_type(&mut self, callee: &Value) -> String {
        match node_type(callee) {
            Some("Identifier") => {
                let Some(name) = name_from_node(Some(callee)) else {
                    return "any".to_string();
                };
                self.type_from_import(&name).unwrap_or(name)
            }
            Some("MemberExpression") => {
                let Some(object) = callee.get("object") else {
                    return "any".to_string();
                };
                let Some(property) = callee.get("property").and_then(|p| name_from_node(Some(p)))
                else {
                    return "any".to_string();
                };
                if let Some(ImportBinding::Namespace { module }) =
                    name_from_node(Some(object)).and_then(|name| self.file.imports.get(&name))
                {
                    return self
                        .project
                        .imported_member_type(self.file, module, &property)
                        .unwrap_or(property);
                }
                property
            }
            _ => "any".to_string(),
        }
    }

    fn infer_member_expression(&mut self, expr: &Value) -> String {
        let Some(object) = expr.get("object") else {
            return "any".to_string();
        };
        let Some(property) = expr.get("property").and_then(|p| name_from_node(Some(p))) else {
            return "any".to_string();
        };
        if let Some(object_name) = name_from_node(Some(object)) {
            if object_name == "Math" {
                return "__ecma.Math".to_string();
            }
            if let Some(ImportBinding::Namespace { module }) = self.file.imports.get(&object_name) {
                return self
                    .project
                    .imported_member_type(self.file, module, &property)
                    .unwrap_or_else(|| "any".to_string());
            }
        }

        let receiver = self.infer_expr(object);
        if receiver == "this" {
            if let Some(class_info) = self.current_class_for_member(expr) {
                return class_info
                    .properties
                    .get(&property)
                    .cloned()
                    .unwrap_or_else(|| "any".to_string());
            }
        }
        if let Some(class_info) = self.project.class_info(self.file, &receiver) {
            if let Some(property_type) = class_info.properties.get(&property) {
                return property_type.clone();
            }
        }
        "any".to_string()
    }

    fn infer_binary_expression(&mut self, expr: &Value) -> String {
        let left = expr
            .get("left")
            .map(|value| self.infer_expr(value))
            .unwrap_or_else(|| "any".to_string());
        let right = expr
            .get("right")
            .map(|value| self.infer_expr(value))
            .unwrap_or_else(|| "any".to_string());
        let op = expr
            .get("operator")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if matches!(
            op,
            "==" | "===" | "!=" | "!==" | "<" | ">" | "<=" | ">=" | "&&" | "||"
        ) {
            return "boolean".to_string();
        }
        if op == "+" && (left == "string" || right == "string") {
            "string".to_string()
        } else if left == "number" && right == "number" {
            "number".to_string()
        } else {
            "any".to_string()
        }
    }

    fn infer_unary_expression(&mut self, expr: &Value) -> String {
        let operator = expr
            .get("operator")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if operator == "!" || operator == "typeof" {
            return if operator == "!" { "boolean" } else { "string" }.to_string();
        }
        expr.get("argument")
            .map(|argument| self.infer_expr(argument))
            .unwrap_or_else(|| "any".to_string())
    }

    fn function_return_type(&mut self, function: &Value) -> String {
        let aliases = self.file.aliases.clone();
        let env = self.env.clone();
        function_return_type_from_node(function, self.source, &aliases, &env, None, Some(self))
    }

    fn function_signature(&mut self, function: &Value, return_type: &str) -> String {
        let params = array_field(function, "params")
            .iter()
            .map(|param| {
                param
                    .get("typeAnnotation")
                    .map(|annotation| self.type_from_annotation(Some(annotation)))
                    .or_else(|| self.type_from_parameter_source(param))
                    .unwrap_or_else(|| "any".to_string())
            })
            .collect::<Vec<_>>();
        format!("({}) => {return_type}", params.join(", "))
    }

    fn type_from_variable_annotation(&self, declarator: &Value) -> Option<String> {
        let id = declarator.get("id")?;
        if !matches!(node_type(id), Some("Identifier")) {
            return None;
        }
        source_slice_for_node(declarator, self.source).and_then(|code| {
            let before_initializer =
                split_top_level_once(code, '=').map_or(code, |(before, _)| before);
            let (_, annotation) = split_top_level_once(before_initializer, ':')?;
            let tpe = self.type_from_text(annotation);
            (tpe != "any").then_some(tpe)
        })
    }

    fn type_from_parameter_source(&self, param: &Value) -> Option<String> {
        source_slice_for_node(param, self.source).and_then(|code| {
            let (_, annotation) = split_top_level_once(code, ':')?;
            let annotation =
                split_top_level_once(annotation, '=').map_or(annotation, |(before, _)| before);
            let tpe = self.type_from_text(annotation);
            (tpe != "any").then_some(tpe)
        })
    }

    fn type_from_cast_source(&self, expr: &Value) -> Option<String> {
        if !matches!(node_type(expr), Some("TSTypeAssertion")) {
            return None;
        }
        let code = source_slice_for_node(expr, self.source)?.trim_start();
        let rest = code.strip_prefix('<')?;
        let type_end = rest.find('>')?;
        let tpe = self.type_from_text(&rest[..type_end]);
        (tpe != "any").then_some(tpe)
    }

    fn type_from_annotation(&self, node: Option<&Value>) -> String {
        type_from_annotation(node, self.source, &self.file.aliases)
    }

    fn type_from_text(&self, text: &str) -> String {
        type_from_text(text, &self.file.aliases)
    }

    fn current_class_for_member(&self, _expr: &Value) -> Option<&ClassInfo> {
        None
    }

    fn insert_node_type(&mut self, node: &Value, tpe: &str) {
        if should_emit_type(tpe) {
            if let Some(range) = range_key(node) {
                self.type_map.insert(range, tpe.to_string());
            }
        }
    }
}

fn function_return_type_from_node(
    function: &Value,
    source: &str,
    aliases: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
    class_info: Option<&ClassInfo>,
    infer: Option<&mut TypeMapInfer<'_>>,
) -> String {
    if let Some(return_type) = function.get("returnType") {
        let tpe = type_from_annotation(Some(return_type), source, aliases);
        if tpe != "any" {
            return tpe;
        }
    }
    if matches!(node_type(function), Some("ArrowFunctionExpression"))
        && function
            .get("expression")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        if let Some(body) = function.get("body") {
            return if let Some(infer) = infer {
                infer.infer_expr(body)
            } else {
                infer_expr_without_project(body, source, aliases, env, class_info)
            };
        }
    }
    if let Some(body) = function.get("body") {
        return return_type_from_body(body, source, aliases, env, class_info, infer);
    }
    "any".to_string()
}

fn return_type_from_body(
    body: &Value,
    source: &str,
    aliases: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
    class_info: Option<&ClassInfo>,
    mut infer: Option<&mut TypeMapInfer<'_>>,
) -> String {
    match node_type(body) {
        Some("BlockStatement") => {
            for statement in array_field(body, "body") {
                if matches!(node_type(statement), Some("ReturnStatement")) {
                    if let Some(argument) = statement.get("argument").filter(|x| !x.is_null()) {
                        return if let Some(ref mut infer) = infer {
                            infer.infer_expr(argument)
                        } else {
                            infer_expr_without_project(argument, source, aliases, env, class_info)
                        };
                    }
                }
            }
            "any".to_string()
        }
        _ => {
            if let Some(ref mut infer) = infer {
                infer.infer_expr(body)
            } else {
                infer_expr_without_project(body, source, aliases, env, class_info)
            }
        }
    }
}

fn infer_expr_without_project(
    expr: &Value,
    source: &str,
    aliases: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
    class_info: Option<&ClassInfo>,
) -> String {
    match node_type(expr) {
        Some("StringLiteral") | Some("TemplateLiteral") | Some("TemplateElement") => {
            "string".to_string()
        }
        Some("NumericLiteral") | Some("DecimalLiteral") | Some("BigIntLiteral") => {
            "number".to_string()
        }
        Some("BooleanLiteral") => "boolean".to_string(),
        Some("NullLiteral") => "null".to_string(),
        Some("Identifier") => name_from_node(Some(expr))
            .and_then(|name| {
                env.get(&name)
                    .cloned()
                    .or_else(|| aliases.get(&name).cloned())
            })
            .unwrap_or_else(|| "any".to_string()),
        Some("TSAsExpression") | Some("TSTypeAssertion") | Some("TSSatisfiesExpression") => expr
            .get("typeAnnotation")
            .map(|annotation| type_from_annotation(Some(annotation), source, aliases))
            .unwrap_or_else(|| "any".to_string()),
        Some("MemberExpression") => {
            if matches!(
                expr.get("object").and_then(node_type),
                Some("ThisExpression")
            ) {
                if let Some(property) = expr.get("property").and_then(|x| name_from_node(Some(x))) {
                    if let Some(property_type) =
                        class_info.and_then(|info| info.properties.get(&property))
                    {
                        return property_type.clone();
                    }
                }
            }
            "any".to_string()
        }
        Some("CallExpression") => {
            if expr
                .get("callee")
                .and_then(|callee| callee.get("object"))
                .and_then(|object| name_from_node(Some(object)))
                .is_some_and(|name| name == "Math")
            {
                "number".to_string()
            } else {
                "any".to_string()
            }
        }
        _ => simple_literal_type(expr),
    }
}

fn return_type_from_function_signature(tpe: &str) -> Option<String> {
    tpe.split("=>").nth(1).map(|value| value.trim().to_string())
}

fn type_from_annotation(
    node: Option<&Value>,
    source: &str,
    aliases: &BTreeMap<String, String>,
) -> String {
    let Some(node) = node else {
        return "any".to_string();
    };
    match node_type(node) {
        Some("TSTypeAnnotation") | Some("TypeAnnotation") => {
            type_from_annotation(node.get("typeAnnotation"), source, aliases)
        }
        Some("TSStringKeyword") | Some("StringTypeAnnotation") => "string".to_string(),
        Some("TSNumberKeyword") | Some("NumberTypeAnnotation") => "number".to_string(),
        Some("TSBigIntKeyword") => "number".to_string(),
        Some("TSBooleanKeyword") | Some("BooleanTypeAnnotation") => "boolean".to_string(),
        Some("TSNullKeyword") | Some("NullLiteralTypeAnnotation") => "null".to_string(),
        Some("TSVoidKeyword") => "void".to_string(),
        Some("TSUndefinedKeyword") => "undefined".to_string(),
        Some("TSUnknownKeyword") => "unknown".to_string(),
        Some("TSNeverKeyword") => "never".to_string(),
        Some("TSArrayType") | Some("ArrayTypeAnnotation") => "any[]".to_string(),
        Some("TSTypeLiteral") | Some("ObjectTypeAnnotation") => "{}".to_string(),
        Some("TSLiteralType") => node
            .get("literal")
            .map(simple_literal_type)
            .unwrap_or_else(|| "any".to_string()),
        Some("TSTypeReference") | Some("GenericTypeAnnotation") => {
            let text = source_slice_for_node(node, source).unwrap_or_default();
            if !text.is_empty() {
                let tpe = type_from_text(text, aliases);
                if tpe != "any" {
                    return tpe;
                }
            }
            name_from_node(node.get("typeName").or_else(|| node.get("id")))
                .map(|name| resolve_alias(&name, aliases, 0).unwrap_or(name))
                .unwrap_or_else(|| "any".to_string())
        }
        Some("TSUnionType") => {
            let mut inferred = array_field(node, "types")
                .iter()
                .map(|child| type_from_annotation(Some(child), source, aliases))
                .filter(|tpe| !matches!(tpe.as_str(), "null" | "undefined" | "any"));
            let first = inferred.next().unwrap_or_else(|| "any".to_string());
            if inferred.all(|tpe| tpe == first) {
                first
            } else {
                "any".to_string()
            }
        }
        _ => source_slice_for_node(node, source)
            .map(|text| type_from_text(text, aliases))
            .unwrap_or_else(|| "any".to_string()),
    }
}

fn type_from_text(text: &str, aliases: &BTreeMap<String, String>) -> String {
    let trimmed = text
        .trim()
        .trim_start_matches(':')
        .trim()
        .trim_end_matches(';')
        .trim();
    if trimmed.is_empty() {
        return "any".to_string();
    }
    match trimmed {
        "string" => return "string".to_string(),
        "number" | "int" => return "number".to_string(),
        "boolean" => return "boolean".to_string(),
        "bigint" => return "number".to_string(),
        "void" => return "void".to_string(),
        "unknown" => return "unknown".to_string(),
        "never" => return "never".to_string(),
        "undefined" => return "undefined".to_string(),
        "null" => return "null".to_string(),
        "any" => return "any".to_string(),
        _ => {}
    }
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        return "string".to_string();
    }
    if matches!(trimmed, "true" | "false") {
        return "boolean".to_string();
    }
    if trimmed.parse::<f64>().is_ok() {
        return "number".to_string();
    }
    if trimmed.ends_with("[]")
        || trimmed.starts_with("Array<")
        || trimmed.starts_with("ReadonlyArray<")
    {
        return "any[]".to_string();
    }
    if trimmed.starts_with('{') {
        return "{}".to_string();
    }
    if trimmed.contains("=>") {
        return trimmed.to_string();
    }
    if let Some((outer, inner)) = split_generic_type(trimmed) {
        if matches!(
            outer,
            "Uppercase" | "Lowercase" | "Capitalize" | "Uncapitalize" | "NonNullable"
        ) {
            let inner_type = type_from_text(inner, aliases);
            return resolve_alias(&inner_type, aliases, 0).unwrap_or(inner_type);
        }
        if matches!(outer, "Array" | "ReadonlyArray" | "Readonly") {
            return "any[]".to_string();
        }
        return outer.to_string();
    }
    resolve_alias(trimmed, aliases, 0).unwrap_or_else(|| trimmed.to_string())
}

fn resolve_alias(name: &str, aliases: &BTreeMap<String, String>, depth: usize) -> Option<String> {
    if depth > 8 {
        return None;
    }
    let target = aliases.get(name)?;
    resolve_alias(target, aliases, depth + 1).or_else(|| Some(target.clone()))
}

fn split_generic_type(text: &str) -> Option<(&str, &str)> {
    let start = text.find('<')?;
    let end = text.rfind('>')?;
    if end <= start {
        return None;
    }
    Some((text[..start].trim(), text[start + 1..end].trim()))
}

fn simple_literal_type(node: &Value) -> String {
    match node_type(node) {
        Some("StringLiteral") | Some("TemplateLiteral") | Some("TemplateElement") => {
            "string".to_string()
        }
        Some("NumericLiteral") | Some("DecimalLiteral") | Some("BigIntLiteral") => {
            "number".to_string()
        }
        Some("BooleanLiteral") => "boolean".to_string(),
        Some("NullLiteral") => "null".to_string(),
        Some("ArrayExpression") => "any[]".to_string(),
        Some("ObjectExpression") => "{}".to_string(),
        _ => "any".to_string(),
    }
}

fn should_emit_type(tpe: &str) -> bool {
    !matches!(tpe, "" | "any" | "this")
}

fn relative_name_from_ast(ast: &Value) -> Option<&str> {
    ast.get("relativeName").and_then(Value::as_str)
}

fn program_body(ast: &Value) -> Option<&Vec<Value>> {
    ast.get("ast")?.get("program")?.get("body")?.as_array()
}

fn class_members(class_node: &Value) -> &[Value] {
    class_node
        .get("body")
        .and_then(|body| body.get("body"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn node_type(node: &Value) -> Option<&str> {
    node.get("type").and_then(Value::as_str)
}

fn array_field<'a>(node: &'a Value, key: &str) -> &'a [Value] {
    node.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn string_field<'a>(node: Option<&'a Value>, key: &str) -> Option<&'a str> {
    node?.get(key).and_then(Value::as_str)
}

fn name_from_node(node: Option<&Value>) -> Option<String> {
    let node = node?;
    match node_type(node) {
        Some("Identifier") | Some("PrivateName") => {
            string_field(Some(node), "name").map(str::to_string)
        }
        Some("StringLiteral") => string_field(Some(node), "value").map(str::to_string),
        Some("TSQualifiedName") => {
            let left = name_from_node(node.get("left"))?;
            let right = name_from_node(node.get("right"))?;
            Some(format!("{left}.{right}"))
        }
        _ => string_field(Some(node), "name").map(str::to_string),
    }
}

fn range_key(node: &Value) -> Option<String> {
    let start = node.get("start")?.as_u64()?;
    let end = node.get("end")?.as_u64()?;
    Some(format!("{start}:{end}"))
}

fn source_slice_for_node<'a>(node: &Value, source: &'a str) -> Option<&'a str> {
    let start = node.get("start")?.as_u64()? as usize;
    let end = node.get("end")?.as_u64()? as usize;
    source_slice_utf16(source, start, end)
}

fn source_slice_utf16(source: &str, start: usize, end: usize) -> Option<&str> {
    let start_byte = byte_offset_for_utf16(source, start);
    let end_byte = byte_offset_for_utf16(source, end);
    source.get(start_byte..end_byte)
}

fn byte_offset_for_utf16(source: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    for (byte, ch) in source.char_indices() {
        if utf16 >= offset {
            return byte;
        }
        let next = utf16 + ch.len_utf16();
        if next > offset {
            return byte;
        }
        utf16 = next;
    }
    source.len()
}

fn split_top_level_once(text: &str, needle: char) -> Option<(&str, &str)> {
    let mut angle_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut quote = None;
    for (index, ch) in text.char_indices() {
        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => quote = Some(ch),
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            _ if ch == needle
                && angle_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && paren_depth == 0 =>
            {
                return Some((&text[..index], &text[index + ch.len_utf8()..]));
            }
            _ => {}
        }
    }
    None
}

fn module_keys_for_relative_name(relative_name: &str) -> Vec<String> {
    let normalized = normalize_module_path(relative_name);
    let mut keys = vec![normalized.clone()];
    if let Some(stripped) = strip_known_js_extension(&normalized) {
        keys.push(stripped.to_string());
    }
    if let Some(stripped) = normalized
        .strip_suffix("/index.ts")
        .or_else(|| normalized.strip_suffix("/index.tsx"))
        .or_else(|| normalized.strip_suffix("/index.js"))
        .or_else(|| normalized.strip_suffix("/index.jsx"))
    {
        keys.push(stripped.to_string());
    }
    keys
}

fn strip_known_js_extension(path: &str) -> Option<&str> {
    path.strip_suffix(".ts")
        .or_else(|| path.strip_suffix(".tsx"))
        .or_else(|| path.strip_suffix(".js"))
        .or_else(|| path.strip_suffix(".jsx"))
        .or_else(|| path.strip_suffix(".mjs"))
        .or_else(|| path.strip_suffix(".cjs"))
}

fn normalize_module_path(path: &str) -> String {
    let mut parts = Vec::new();
    let normalized = path.replace('\\', "/");
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn file_json(root: &Path, path: &Path, source: &str, tree: &Tree) -> Value {
    let relative_name = relative_name(root, path);
    let program = program_json(tree.root_node(), source);
    let ast = with_span_bounds(
        "File",
        0,
        Point { row: 0, column: 0 },
        source.len(),
        point_for_byte(source, source.len()),
        json!({
            "program": program,
            "comments": [],
            "tokens": []
        }),
    );

    json!({
        "fullName": path.to_string_lossy(),
        "relativeName": relative_name,
        "ast": ast
    })
}

fn relative_name(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn program_json(root: Node, source: &str) -> Value {
    let body = program_body_json(root, source);

    with_span_bounds(
        "Program",
        0,
        Point { row: 0, column: 0 },
        source.len(),
        point_for_byte(source, source.len()),
        json!({
            "sourceType": "module",
            "interpreter": Value::Null,
            "directives": [],
            "body": body
        }),
    )
}

fn program_body_json(root: Node, source: &str) -> Vec<Value> {
    named_children(root)
        .filter(|child| !is_comment(*child))
        .map(|child| stmt_json(child, source))
        .filter(|value| !value.is_null())
        .collect::<Vec<_>>()
}

#[derive(Debug)]
struct VueBlock {
    start: usize,
    end: usize,
    content_start: usize,
    content_end: usize,
}

fn masked_vue_script_source(source: &str) -> String {
    let mut bytes = blank_source_bytes(source);
    for block in vue_blocks(source, "script") {
        bytes[block.content_start..block.content_end]
            .copy_from_slice(&source.as_bytes()[block.content_start..block.content_end]);
    }
    String::from_utf8(bytes).expect("masked source is valid UTF-8")
}

fn masked_vue_template_source(source: &str) -> String {
    let mut bytes = blank_source_bytes(source);
    for block in vue_blocks(source, "template") {
        bytes[block.start..block.end].copy_from_slice(&source.as_bytes()[block.start..block.end]);
        normalize_vue_template_syntax(&mut bytes[block.start..block.end]);
    }
    String::from_utf8(bytes).expect("masked source is valid UTF-8")
}

fn blank_source_bytes(source: &str) -> Vec<u8> {
    source
        .as_bytes()
        .iter()
        .map(|byte| match byte {
            b'\n' | b'\r' => *byte,
            _ => b' ',
        })
        .collect()
}

fn vue_blocks(source: &str, tag: &str) -> Vec<VueBlock> {
    let lower = source.to_ascii_lowercase();
    let open_pattern = format!("<{tag}");
    let close_pattern = format!("</{tag}>");
    let mut blocks = Vec::new();
    let mut cursor = 0;

    while let Some(relative_start) = lower[cursor..].find(&open_pattern) {
        let start = cursor + relative_start;
        let after_tag_name = start + open_pattern.len();
        if !lower
            .as_bytes()
            .get(after_tag_name)
            .is_none_or(|byte| is_tag_boundary(*byte))
        {
            cursor = after_tag_name;
            continue;
        }

        let Some(relative_open_end) = lower[after_tag_name..].find('>') else {
            break;
        };
        let content_start = after_tag_name + relative_open_end + 1;
        let Some(relative_close_start) = lower[content_start..].find(&close_pattern) else {
            break;
        };
        let content_end = content_start + relative_close_start;
        let end = content_end + close_pattern.len();
        blocks.push(VueBlock {
            start,
            end,
            content_start,
            content_end,
        });
        cursor = end;
    }

    blocks
}

fn is_tag_boundary(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/')
}

fn normalize_vue_template_syntax(bytes: &mut [u8]) {
    let mut index = 0;
    while index + 1 < bytes.len() {
        match (bytes[index], bytes[index + 1]) {
            (b'{', b'{') => {
                bytes[index + 1] = b' ';
                index += 2;
            }
            (b'}', b'}') => {
                bytes[index] = b' ';
                index += 2;
            }
            _ => {
                index += 1;
            }
        }
    }

    for index in 0..bytes.len() {
        if matches!(bytes[index], b':' | b'@' | b'#')
            && is_vue_shorthand_attribute_start(bytes, index)
        {
            bytes[index] = b' ';
        }
    }
}

fn is_vue_shorthand_attribute_start(bytes: &[u8], index: usize) -> bool {
    if index == 0 {
        return false;
    }
    bytes[index - 1].is_ascii_whitespace() || bytes[index - 1] == b'<'
}

fn stmt_json(node: Node, source: &str) -> Value {
    match node.kind() {
        "lexical_declaration" | "variable_declaration" => variable_declaration_json(node, source),
        "function_declaration" => function_declaration_json(node, source),
        "class_declaration" | "abstract_class_declaration" => class_declaration_json(node, source),
        "ambient_declaration" => ambient_declaration_json(node, source),
        "import_statement" => import_statement_json(node, source),
        "export_statement" => export_statement_json(node, source),
        "internal_module" | "module" => ts_module_declaration_json(node, source),
        "interface_declaration" => ts_interface_declaration_json(node, source),
        "enum_declaration" => ts_enum_declaration_json(node, source),
        "type_alias_declaration" => ts_type_alias_declaration_json(node, source),
        "statement_block" => block_statement_json(node, source),
        "return_statement" => return_statement_json(node, source),
        "if_statement" => if_statement_json(node, source),
        "with_statement" => with_statement_json(node, source),
        "while_statement" => while_statement_json(node, source),
        "do_statement" => do_while_statement_json(node, source),
        "for_statement" => for_statement_json(node, source),
        "for_in_statement" => for_in_of_statement_json(node, source),
        "switch_statement" => switch_statement_json(node, source),
        "labeled_statement" => labeled_statement_json(node, source),
        "break_statement" => jump_statement_json("BreakStatement", node, source),
        "continue_statement" => jump_statement_json("ContinueStatement", node, source),
        "try_statement" => try_statement_json(node, source),
        "throw_statement" => throw_statement_json(node, source),
        "expression_statement" => expression_statement_json(node, source),
        "empty_statement" => with_span("EmptyStatement", node, json!({})),
        _ => noop_json(node),
    }
}

fn expr_json(node: Node, source: &str) -> Value {
    match node.kind() {
        "identifier"
        | "type_identifier"
        | "property_identifier"
        | "private_property_identifier"
        | "statement_identifier"
        | "shorthand_property_identifier_pattern" => identifier_json(node, source),
        "number" => numeric_literal_json(node, source),
        "string" => string_literal_json(node, source),
        "template_string" => template_string_json(node, source),
        "true" => boolean_literal_json(node, true),
        "false" => boolean_literal_json(node, false),
        "null" => with_span("NullLiteral", node, json!({ "value": Value::Null })),
        "this" => with_span("ThisExpression", node, json!({})),
        "binary_expression" => binary_expression_json(node, source),
        "unary_expression" => unary_expression_json(node, source),
        "await_expression" => await_expression_json(node, source),
        "as_expression" => ts_as_expression_json(node, source),
        "type_assertion" => ts_type_assertion_json(node, source),
        "satisfies_expression" => ts_satisfies_expression_json(node, source),
        "assignment_expression" | "augmented_assignment_expression" => {
            assignment_expression_json(node, source)
        }
        "update_expression" => update_expression_json(node, source),
        "ternary_expression" => conditional_expression_json(node, source),
        "call_expression" => call_expression_json(node, source),
        "new_expression" => new_expression_json(node, source),
        "member_expression" => member_expression_json(node, source),
        "subscript_expression" => subscript_expression_json(node, source),
        "array" => array_expression_json(node, source),
        "object" => object_expression_json(node, source),
        "jsx_element" => jsx_element_json(node, source),
        "jsx_self_closing_element" => jsx_self_closing_element_json(node, source),
        "jsx_expression" => jsx_expression_container_json(node, source),
        "jsx_text" => with_span("JSXText", node, json!({})),
        "array_pattern" => array_pattern_json(node, source),
        "object_pattern" => object_pattern_json(node, source),
        "assignment_pattern" => assignment_pattern_json(node, source),
        "function_expression" => function_expression_json(node, source),
        "function_declaration" => function_declaration_json(node, source),
        "arrow_function" => arrow_function_json(node, source),
        "class" => class_expression_json(node, source),
        "non_null_expression" => ts_non_null_expression_json(node, source),
        "required_parameter" | "optional_parameter" => parameter_json(node, source),
        "sequence_expression" => sequence_expression_json(node, source),
        "rest_pattern" => unary_argument_json("RestElement", node, source),
        "spread_element" => unary_argument_json("SpreadElement", node, source),
        "parenthesized_expression" => node
            .named_child(0)
            .map(|child| expr_json(child, source))
            .unwrap_or_else(|| noop_json(node)),
        "predefined_type" | "type_annotation" => ts_type_json(node, source),
        _ => noop_json(node),
    }
}

fn variable_declaration_json(node: Node, source: &str) -> Value {
    let kind = declaration_kind(node, source);
    let declarations = named_children(node)
        .filter(|child| child.kind() == "variable_declarator")
        .map(|child| variable_declarator_json(child, source))
        .collect::<Vec<_>>();

    with_span(
        "VariableDeclaration",
        node,
        json!({
            "kind": kind,
            "declarations": declarations
        }),
    )
}

fn ambient_declaration_json(node: Node, source: &str) -> Value {
    match node.named_child(0).map(|child| child.kind()) {
        Some("function_declaration" | "function_signature") => {
            let function = node.named_child(0).unwrap();
            function_like_json_with_span("TSDeclareFunction", node, function, source)
        }
        Some(_) => node
            .named_child(0)
            .map(|child| stmt_json(child, source))
            .unwrap_or_else(|| noop_json(node)),
        None => noop_json(node),
    }
}

fn variable_declarator_json(node: Node, source: &str) -> Value {
    let id = field_json(node, "name", source).unwrap_or_else(|| noop_json(node));
    let init = field_json(node, "value", source).unwrap_or(Value::Null);

    with_span(
        "VariableDeclarator",
        node,
        json!({
            "id": id,
            "init": init
        }),
    )
}

fn function_declaration_json(node: Node, source: &str) -> Value {
    function_like_json("FunctionDeclaration", node, source)
}

fn function_expression_json(node: Node, source: &str) -> Value {
    function_like_json("FunctionExpression", node, source)
}

fn class_declaration_json(node: Node, source: &str) -> Value {
    class_like_json("ClassDeclaration", node, source)
}

fn class_expression_json(node: Node, source: &str) -> Value {
    class_like_json("ClassExpression", node, source)
}

fn class_like_json(kind: &str, node: Node, source: &str) -> Value {
    let id = node
        .child_by_field_name("name")
        .map(|child| identifier_json(child, source))
        .unwrap_or(Value::Null);
    let body = node
        .child_by_field_name("body")
        .map(|child| class_body_json(child, source))
        .unwrap_or_else(|| with_span("ClassBody", node, json!({ "body": [] })));
    let super_class = class_super_json(node, source).unwrap_or(Value::Null);
    let implements = class_implements_json(node, source);

    with_span(
        kind,
        node,
        json!({
            "id": id,
            "superClass": super_class,
            "body": body,
            "decorators": decorators_json(node, source),
            "implements": implements,
            "mixins": [],
            "abstract": node.kind() == "abstract_class_declaration"
                || has_named_or_keyword_child(node, source, "abstract")
        }),
    )
}

fn class_super_json(node: Node, source: &str) -> Option<Value> {
    let heritage = named_children(node).find(|child| child.kind() == "class_heritage")?;
    for child in named_children(heritage) {
        if child.kind() == "extends_clause" {
            return named_children(child)
                .find(|candidate| is_expression_like(*candidate))
                .map(|candidate| expr_json(candidate, source));
        }
        if is_expression_like(child) {
            return Some(expr_json(child, source));
        }
    }
    None
}

fn class_implements_json(node: Node, source: &str) -> Vec<Value> {
    let Some(heritage) = named_children(node).find(|child| child.kind() == "class_heritage") else {
        return Vec::new();
    };
    named_children(heritage)
        .filter(|child| child.kind() == "implements_clause")
        .flat_map(named_children)
        .filter(|child| is_type_like(*child))
        .map(|child| {
            with_span(
                "TSExpressionWithTypeArguments",
                child,
                json!({ "expression": type_name_json(child, source) }),
            )
        })
        .collect()
}

fn class_body_json(node: Node, source: &str) -> Value {
    let mut body = Vec::new();
    let mut pending_decorators = Vec::new();

    for child in named_children(node) {
        if child.kind() == "decorator" {
            pending_decorators.push(decorator_json(child, source));
            continue;
        }
        if let Some(member) = class_member_json(child, source) {
            body.push(with_decorator_values(member, &mut pending_decorators));
        }
    }

    with_span("ClassBody", node, json!({ "body": body }))
}

fn class_member_json(node: Node, source: &str) -> Option<Value> {
    match node.kind() {
        "method_definition" => Some(class_method_json(node, source)),
        "field_definition" | "public_field_definition" => Some(class_property_json(node, source)),
        "class_static_block" => Some(class_static_block_json(node, source)),
        "abstract_method_signature" | "method_signature" => {
            Some(ts_declare_method_json(node, source))
        }
        "index_signature" => Some(ts_index_signature_json(node, source)),
        _ => None,
    }
}

fn class_method_json(node: Node, source: &str) -> Value {
    let key_node = node.child_by_field_name("name").unwrap_or(node);
    let computed = key_node.kind() == "computed_property_name";
    let key = object_key_json(key_node, source);
    let params = params_json(node, source);
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| block_from_node(node));
    let return_type = node
        .child_by_field_name("return_type")
        .map(|child| ts_type_annotation_json(child, source));

    let mut fields = json!({
            "kind": object_method_kind(node, source),
            "key": key,
            "id": Value::Null,
            "params": params,
            "body": body,
            "computed": computed,
            "static": has_keyword_child(node, source, "static"),
            "generator": has_keyword_child(node, source, "*"),
            "async": has_keyword_child(node, source, "async"),
            "decorators": decorators_json(node, source)
    });
    if let Some(return_type) = return_type {
        fields = with_extra_field(fields, "returnType", return_type);
    }

    with_span("ClassMethod", node, fields)
}

fn class_property_json(node: Node, source: &str) -> Value {
    let key_node = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("property"))
        .unwrap_or(node);
    let is_private = key_node.kind() == "private_property_identifier";
    let key = if is_private {
        private_name_json(key_node, source)
    } else {
        object_key_json(key_node, source)
    };
    let computed = key_node.kind() == "computed_property_name";
    let value = node
        .child_by_field_name("value")
        .map(|child| expr_json(child, source))
        .unwrap_or(Value::Null);
    let mut fields = json!({
        "key": key,
        "value": value,
        "computed": computed,
        "static": has_keyword_child(node, source, "static"),
        "readonly": has_named_or_keyword_child(node, source, "readonly"),
        "abstract": has_named_or_keyword_child(node, source, "abstract"),
        "decorators": decorators_json(node, source)
    });
    if let Some(type_annotation) = node
        .child_by_field_name("type")
        .map(|child| ts_type_annotation_json(child, source))
    {
        fields = with_extra_field(fields, "typeAnnotation", type_annotation);
    }
    if let Some(accessibility) = accessibility_modifier(node, source) {
        fields = with_extra_field(fields, "accessibility", Value::String(accessibility));
    }

    let kind = if is_private {
        "ClassPrivateProperty"
    } else {
        "ClassProperty"
    };
    with_span_including_trailing_semicolon(kind, node, source, fields)
}

fn class_static_block_json(node: Node, source: &str) -> Value {
    let body = node
        .child_by_field_name("body")
        .map(|block| {
            named_children(block)
                .filter(|child| !is_comment(*child))
                .map(|child| stmt_json(child, source))
                .filter(|value| !value.is_null())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    with_span("StaticBlock", node, json!({ "body": body }))
}

fn ts_declare_method_json(node: Node, source: &str) -> Value {
    let mut method = ts_method_signature_json("TSDeclareMethod", node, source);
    if let Value::Object(ref mut object) = method {
        object.insert(
            "abstract".to_string(),
            Value::Bool(
                node.kind() == "abstract_method_signature"
                    || has_named_or_keyword_child(node, source, "abstract"),
            ),
        );
    }
    method
}

fn ts_method_signature_json(kind: &str, node: Node, source: &str) -> Value {
    let key_node = node.child_by_field_name("name").unwrap_or(node);
    let computed = key_node.kind() == "computed_property_name";
    let params = params_json(node, source);
    let method_kind = if node_text(key_node, source) == "constructor" {
        "constructor"
    } else {
        "method"
    };
    let mut fields = json!({
        "kind": method_kind,
        "key": object_key_json(key_node, source),
        "id": Value::Null,
        "params": params.clone(),
        "parameters": params,
        "computed": computed,
        "static": has_keyword_child(node, source, "static"),
        "generator": false,
        "async": false,
        "abstract": has_named_or_keyword_child(node, source, "abstract"),
        "decorators": decorators_json(node, source)
    });
    if let Some(return_type) = node
        .child_by_field_name("return_type")
        .map(|child| ts_type_annotation_json(child, source))
    {
        fields = with_extra_field(fields, "returnType", return_type);
    }
    if let Some(accessibility) = accessibility_modifier(node, source) {
        fields = with_extra_field(fields, "accessibility", Value::String(accessibility));
    }

    with_span_including_trailing_semicolon(kind, node, source, fields)
}

fn ts_property_signature_json(node: Node, source: &str) -> Value {
    let key_node = node.child_by_field_name("name").unwrap_or(node);
    let mut fields = json!({
        "key": object_key_json(key_node, source),
        "computed": key_node.kind() == "computed_property_name",
        "optional": has_keyword_child(node, source, "?"),
        "readonly": has_named_or_keyword_child(node, source, "readonly")
    });
    if let Some(type_annotation) = node
        .child_by_field_name("type")
        .map(|child| ts_type_annotation_json(child, source))
    {
        fields = with_extra_field(fields, "typeAnnotation", type_annotation);
    }
    if let Some(accessibility) = accessibility_modifier(node, source) {
        fields = with_extra_field(fields, "accessibility", Value::String(accessibility));
    }

    with_span_including_trailing_semicolon("TSPropertySignature", node, source, fields)
}

fn ts_call_signature_json(node: Node, source: &str) -> Value {
    let params = params_json(node, source);
    let mut fields = json!({
        "parameters": params.clone(),
        "params": params
    });
    if let Some(return_type) = node
        .child_by_field_name("return_type")
        .map(|child| ts_type_annotation_json(child, source))
    {
        fields = with_extra_field(fields, "returnType", return_type);
    }

    with_span_including_trailing_semicolon("TSCallSignatureDeclaration", node, source, fields)
}

fn ts_construct_signature_json(node: Node, source: &str) -> Value {
    let params = params_json(node, source);
    let mut fields = json!({
        "kind": "constructor",
        "parameters": params.clone(),
        "params": params
    });
    if let Some(type_annotation) = node
        .child_by_field_name("type")
        .map(|child| ts_type_annotation_json(child, source))
    {
        fields = with_extra_field(fields, "typeAnnotation", type_annotation);
    }

    with_span_including_trailing_semicolon("TSConstructSignatureDeclaration", node, source, fields)
}

fn ts_index_signature_json(node: Node, source: &str) -> Value {
    let parameter = node
        .child_by_field_name("name")
        .map(|name| {
            let mut value = identifier_json(name, source);
            if let Some(index_type) = node
                .child_by_field_name("index_type")
                .map(|child| ts_type_annotation_json(child, source))
            {
                value = with_extra_field(value, "typeAnnotation", index_type);
            }
            value
        })
        .unwrap_or_else(|| identifier_from_name(node, "index"));
    let mut fields = json!({
        "parameters": [parameter]
    });
    if let Some(type_annotation) = node
        .child_by_field_name("type")
        .map(|child| ts_type_annotation_json(child, source))
    {
        fields = with_extra_field(fields, "typeAnnotation", type_annotation);
    }
    if let Some(accessibility) = accessibility_modifier(node, source) {
        fields = with_extra_field(fields, "accessibility", Value::String(accessibility));
    }

    with_span_including_trailing_semicolon("TSIndexSignature", node, source, fields)
}

fn function_like_json(kind: &str, node: Node, source: &str) -> Value {
    function_like_json_with_span(kind, node, node, source)
}

fn function_like_json_with_span(
    kind: &str,
    span_node: Node,
    function_node: Node,
    source: &str,
) -> Value {
    let id = field_json(function_node, "name", source).unwrap_or(Value::Null);
    let params = function_node
        .child_by_field_name("parameters")
        .map(|params_node| {
            named_children(params_node)
                .filter(|child| child.kind() != "(" && child.kind() != ")")
                .map(|child| expr_json(child, source))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let body = function_node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| block_from_node(function_node));
    let return_type = function_node
        .child_by_field_name("return_type")
        .map(|child| ts_type_annotation_json(child, source));

    let mut fields = json!({
            "id": id,
            "params": params,
            "body": body,
            "generator": false,
            "async": false,
            "decorators": decorators_json(span_node, source)
    });
    if let Some(return_type) = return_type {
        fields = with_extra_field(fields, "returnType", return_type);
    }

    with_span(kind, span_node, fields)
}

fn ts_module_declaration_json(node: Node, source: &str) -> Value {
    let name_node = node.child_by_field_name("name");
    let body = node
        .child_by_field_name("body")
        .map(|child| ts_module_block_json(child, source))
        .unwrap_or(Value::Null);

    if let Some(name) = name_node.filter(|name| node_text(*name, source).contains('.')) {
        return nested_ts_module_declaration_json(node, name, body, source);
    }

    let id = name_node
        .map(|child| module_identifier_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "TSModuleDeclaration",
        node,
        json!({
            "id": id,
            "body": body,
            "declare": has_keyword_child(node, source, "declare")
        }),
    )
}

fn nested_ts_module_declaration_json(
    node: Node,
    name_node: Node,
    leaf_body: Value,
    source: &str,
) -> Value {
    let name_text = node_text(name_node, source);
    let mut cursor = name_node.start_byte();
    let mut parts = Vec::new();
    for part in name_text.split('.') {
        let start = cursor;
        let end = start + part.len();
        parts.push((part.to_string(), start, end));
        cursor = end + 1;
    }

    let mut body = leaf_body;
    for (index, (part, start, end)) in parts.into_iter().enumerate().rev() {
        let id = with_span_bounds(
            "Identifier",
            start,
            point_for_byte(source, start),
            end,
            point_for_byte(source, end),
            json!({ "name": part }),
        );
        let span_start = if index == 0 { node.start_byte() } else { start };
        body = with_span_bounds(
            "TSModuleDeclaration",
            span_start,
            point_for_byte(source, span_start),
            node.end_byte(),
            node.end_position(),
            json!({
                "id": id,
                "body": body,
                "declare": has_keyword_child(node, source, "declare")
            }),
        );
    }
    body
}

fn module_identifier_json(node: Node, source: &str) -> Value {
    match node.kind() {
        "string" => string_literal_json(node, source),
        _ => identifier_json(node, source),
    }
}

fn ts_module_block_json(node: Node, source: &str) -> Value {
    let body = named_children(node)
        .filter(|child| !is_comment(*child))
        .map(|child| stmt_json(child, source))
        .filter(|value| !value.is_null())
        .collect::<Vec<_>>();

    with_span("TSModuleBlock", node, json!({ "body": body }))
}

fn ts_interface_declaration_json(node: Node, source: &str) -> Value {
    let id = node
        .child_by_field_name("name")
        .map(|child| identifier_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let body = node
        .child_by_field_name("body")
        .map(|child| ts_interface_body_json(child, source))
        .unwrap_or_else(|| with_span("TSInterfaceBody", node, json!({ "body": [] })));
    let extends = named_children(node)
        .find(|child| child.kind() == "extends_type_clause")
        .map(|child| {
            named_children(child)
                .filter(|candidate| is_type_like(*candidate))
                .map(|candidate| {
                    with_span(
                        "TSExpressionWithTypeArguments",
                        candidate,
                        json!({ "expression": type_name_json(candidate, source) }),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    with_span(
        "TSInterfaceDeclaration",
        node,
        json!({
            "id": id,
            "body": body,
            "extends": extends
        }),
    )
}

fn ts_interface_body_json(node: Node, source: &str) -> Value {
    let body = named_children(node)
        .filter_map(|child| ts_interface_member_json(child, source))
        .collect::<Vec<_>>();

    with_span("TSInterfaceBody", node, json!({ "body": body }))
}

fn ts_interface_member_json(node: Node, source: &str) -> Option<Value> {
    match node.kind() {
        "property_signature" => Some(ts_property_signature_json(node, source)),
        "method_signature" => Some(ts_method_signature_json("TSMethodSignature", node, source)),
        "call_signature" => Some(ts_call_signature_json(node, source)),
        "construct_signature" => Some(ts_construct_signature_json(node, source)),
        "index_signature" => Some(ts_index_signature_json(node, source)),
        "export_statement" => Some(export_statement_json(node, source)),
        _ => None,
    }
}

fn ts_enum_declaration_json(node: Node, source: &str) -> Value {
    let id = node
        .child_by_field_name("name")
        .map(|child| identifier_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let members = node
        .child_by_field_name("body")
        .map(|body| {
            named_children(body)
                .filter_map(|child| ts_enum_member_json(child, source))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    with_span(
        "TSEnumDeclaration",
        node,
        json!({
            "id": id,
            "members": members
        }),
    )
}

fn ts_enum_member_json(node: Node, source: &str) -> Option<Value> {
    match node.kind() {
        "enum_assignment" => {
            let id_node = node.child_by_field_name("name").unwrap_or(node);
            let initializer = node
                .child_by_field_name("value")
                .map(|child| expr_json(child, source))
                .unwrap_or(Value::Null);
            Some(with_span(
                "TSEnumMember",
                node,
                json!({
                    "id": import_export_name_json(id_node, source),
                    "initializer": initializer
                }),
            ))
        }
        "property_identifier" | "identifier" | "string" | "number" => Some(with_span(
            "TSEnumMember",
            node,
            json!({ "id": import_export_name_json(node, source) }),
        )),
        _ => None,
    }
}

fn ts_type_alias_declaration_json(node: Node, source: &str) -> Value {
    let id = node
        .child_by_field_name("name")
        .map(|child| identifier_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let type_annotation = node
        .child_by_field_name("value")
        .map(|child| ts_type_json(child, source))
        .unwrap_or_else(|| with_span("TSAnyKeyword", node, json!({})));

    with_span(
        "TSTypeAliasDeclaration",
        node,
        json!({
            "id": id,
            "typeAnnotation": type_annotation
        }),
    )
}

fn import_statement_json(node: Node, source: &str) -> Value {
    if let Some(require_clause) =
        named_children(node).find(|child| child.kind() == "import_require_clause")
    {
        return ts_import_equals_declaration_json(node, require_clause, source);
    }

    let source_node = node.child_by_field_name("source");
    let source_value = source_node
        .map(|child| string_literal_json(child, source))
        .unwrap_or(Value::Null);
    let specifiers = named_children(node)
        .find(|child| child.kind() == "import_clause")
        .map(|child| import_specifiers_json(child, source))
        .unwrap_or_default();

    with_span(
        "ImportDeclaration",
        node,
        json!({
            "source": source_value,
            "specifiers": specifiers
        }),
    )
}

fn ts_import_equals_declaration_json(node: Node, require_clause: Node, source: &str) -> Value {
    let id = require_clause
        .named_child(0)
        .map(|child| identifier_json(child, source))
        .unwrap_or_else(|| noop_json(require_clause));
    let expression = require_clause
        .child_by_field_name("source")
        .map(|child| string_literal_json(child, source))
        .unwrap_or(Value::Null);

    with_span(
        "TSImportEqualsDeclaration",
        node,
        json!({
            "id": id,
            "moduleReference": with_span(
                "TSExternalModuleReference",
                require_clause,
                json!({ "expression": expression })
            )
        }),
    )
}

fn import_specifiers_json(node: Node, source: &str) -> Vec<Value> {
    let mut specifiers = Vec::new();
    for child in named_children(node) {
        match child.kind() {
            "identifier" => specifiers.push(with_span(
                "ImportDefaultSpecifier",
                child,
                json!({ "local": identifier_json(child, source) }),
            )),
            "named_imports" => specifiers.extend(
                named_children(child)
                    .filter(|specifier| specifier.kind() == "import_specifier")
                    .map(|specifier| import_specifier_json(specifier, source)),
            ),
            "namespace_import" => {
                let local = child
                    .named_child(0)
                    .map(|identifier| identifier_json(identifier, source))
                    .unwrap_or_else(|| noop_json(child));
                specifiers.push(with_span(
                    "ImportNamespaceSpecifier",
                    child,
                    json!({ "local": local }),
                ));
            }
            _ => {}
        }
    }
    specifiers
}

fn import_specifier_json(node: Node, source: &str) -> Value {
    let imported_node = node.child_by_field_name("name").unwrap_or(node);
    let local_node = node.child_by_field_name("alias").unwrap_or(imported_node);
    with_span(
        "ImportSpecifier",
        node,
        json!({
            "imported": import_export_name_json(imported_node, source),
            "local": import_export_name_json(local_node, source)
        }),
    )
}

fn export_statement_json(node: Node, source: &str) -> Value {
    let source_value = node
        .child_by_field_name("source")
        .map(|child| string_literal_json(child, source))
        .unwrap_or(Value::Null);
    let mut specifiers = named_children(node)
        .find(|child| child.kind() == "export_clause")
        .map(|child| export_specifiers_json(child, source))
        .unwrap_or_default();
    if let Some(namespace_export) =
        named_children(node).find(|child| child.kind() == "namespace_export")
    {
        specifiers.push(export_namespace_specifier_json(namespace_export, source));
    }

    if has_keyword_child(node, source, "=") {
        let expression = node
            .child_by_field_name("value")
            .or_else(|| named_children(node).find(|child| is_expression_like(*child)))
            .map(|child| expr_json(child, source))
            .unwrap_or_else(|| noop_json(node));
        return with_span(
            "TSExportAssignment",
            node,
            json!({ "expression": expression }),
        );
    }

    if source_value != Value::Null && specifiers.is_empty() && has_keyword_child(node, source, "*")
    {
        return with_span(
            "ExportAllDeclaration",
            node,
            json!({
                "source": source_value,
                "exported": Value::Null
            }),
        );
    }

    if has_keyword_child(node, source, "default") {
        let declaration = node
            .child_by_field_name("declaration")
            .map(|child| stmt_json(child, source))
            .or_else(|| {
                node.child_by_field_name("value")
                    .map(|child| expr_json(child, source))
            })
            .or_else(|| {
                named_children(node)
                    .find(|child| child.kind() != "export_clause")
                    .map(|child| expr_json(child, source))
            })
            .map(|declaration| with_leading_decorators(declaration, node, source))
            .unwrap_or(Value::Null);
        return with_span(
            "ExportDefaultDeclaration",
            node,
            json!({
                "declaration": declaration
            }),
        );
    }

    let declaration = node
        .child_by_field_name("declaration")
        .map(|child| stmt_json(child, source))
        .map(|declaration| with_leading_decorators(declaration, node, source))
        .unwrap_or(Value::Null);

    with_span(
        "ExportNamedDeclaration",
        node,
        json!({
            "declaration": declaration,
            "specifiers": specifiers,
            "source": source_value
        }),
    )
}

fn export_specifiers_json(node: Node, source: &str) -> Vec<Value> {
    named_children(node)
        .filter(|child| child.kind() == "export_specifier")
        .map(|child| export_specifier_json(child, source))
        .collect()
}

fn export_namespace_specifier_json(node: Node, source: &str) -> Value {
    let exported_node = named_children(node).last().unwrap_or(node);
    with_span(
        "ExportNamespaceSpecifier",
        node,
        json!({
            "exported": import_export_name_json(exported_node, source)
        }),
    )
}

fn export_specifier_json(node: Node, source: &str) -> Value {
    let local_node = node.child_by_field_name("name").unwrap_or(node);
    let exported_node = node.child_by_field_name("alias").unwrap_or(local_node);
    with_span(
        "ExportSpecifier",
        node,
        json!({
            "local": import_export_name_json(local_node, source),
            "exported": import_export_name_json(exported_node, source)
        }),
    )
}

fn import_export_name_json(node: Node, source: &str) -> Value {
    match node.kind() {
        "string" => string_literal_json(node, source),
        _ => identifier_json(node, source),
    }
}

fn block_statement_json(node: Node, source: &str) -> Value {
    let body = named_children(node)
        .filter(|child| !is_comment(*child))
        .map(|child| stmt_json(child, source))
        .filter(|value| !value.is_null())
        .collect::<Vec<_>>();

    with_span(
        "BlockStatement",
        node,
        json!({ "body": body, "directives": [] }),
    )
}

fn block_from_node(node: Node) -> Value {
    with_span(
        "BlockStatement",
        node,
        json!({ "body": [], "directives": [] }),
    )
}

fn return_statement_json(node: Node, source: &str) -> Value {
    let argument = node
        .child_by_field_name("argument")
        .or_else(|| node.named_child(0))
        .map(|child| expr_json(child, source))
        .unwrap_or(Value::Null);

    with_span("ReturnStatement", node, json!({ "argument": argument }))
}

fn if_statement_json(node: Node, source: &str) -> Value {
    let test = node
        .child_by_field_name("condition")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let consequent = node
        .child_by_field_name("consequence")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let alternate = node
        .child_by_field_name("alternative")
        .and_then(first_named_child)
        .map(|child| stmt_json(child, source))
        .unwrap_or(Value::Null);

    with_span(
        "IfStatement",
        node,
        json!({
            "test": test,
            "consequent": consequent,
            "alternate": alternate
        }),
    )
}

fn with_statement_json(node: Node, source: &str) -> Value {
    let object = node
        .child_by_field_name("object")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "WithStatement",
        node,
        json!({
            "object": object,
            "body": body
        }),
    )
}

fn while_statement_json(node: Node, source: &str) -> Value {
    let test = node
        .child_by_field_name("condition")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "WhileStatement",
        node,
        json!({
            "test": test,
            "body": body
        }),
    )
}

fn do_while_statement_json(node: Node, source: &str) -> Value {
    let test = node
        .child_by_field_name("condition")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "DoWhileStatement",
        node,
        json!({
            "test": test,
            "body": body
        }),
    )
}

fn for_statement_json(node: Node, source: &str) -> Value {
    let init = node
        .child_by_field_name("initializer")
        .and_then(|child| non_empty_stmt_or_expr_json(child, source))
        .unwrap_or(Value::Null);
    let test = node
        .child_by_field_name("condition")
        .and_then(|child| non_empty_stmt_or_expr_json(child, source))
        .unwrap_or(Value::Null);
    let update = node
        .child_by_field_name("increment")
        .and_then(|child| non_empty_stmt_or_expr_json(child, source))
        .unwrap_or(Value::Null);
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "ForStatement",
        node,
        json!({
            "init": init,
            "test": test,
            "update": update,
            "body": body
        }),
    )
}

fn for_in_of_statement_json(node: Node, source: &str) -> Value {
    let operator = node
        .child_by_field_name("operator")
        .map(|child| node_text(child, source))
        .unwrap_or_default();
    let kind = if operator == "of" {
        "ForOfStatement"
    } else {
        "ForInStatement"
    };
    let left_node = node.child_by_field_name("left");
    let left = left_node
        .map(|child| for_in_of_left_json(node, child, source))
        .unwrap_or_else(|| noop_json(node));
    let right = node
        .child_by_field_name("right")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        kind,
        node,
        json!({
            "left": left,
            "right": right,
            "body": body,
            "await": has_keyword_child(node, source, "await")
        }),
    )
}

fn for_in_of_left_json(for_node: Node, left_node: Node, source: &str) -> Value {
    let id = pattern_or_expr_json(left_node, source);
    if let Some(kind) = declaration_kind_in_for_in_of(for_node, source) {
        let declarator = with_span(
            "VariableDeclarator",
            left_node,
            json!({
                "id": id,
                "init": Value::Null
            }),
        );
        with_span(
            "VariableDeclaration",
            left_node,
            json!({
                "kind": kind,
                "declarations": [declarator]
            }),
        )
    } else {
        id
    }
}

fn switch_statement_json(node: Node, source: &str) -> Value {
    let discriminant = node
        .child_by_field_name("value")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let cases = node
        .child_by_field_name("body")
        .map(|body| {
            named_children(body)
                .filter_map(|child| switch_case_json(child, source))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    with_span(
        "SwitchStatement",
        node,
        json!({
            "discriminant": discriminant,
            "cases": cases
        }),
    )
}

fn switch_case_json(node: Node, source: &str) -> Option<Value> {
    let test_node = node.child_by_field_name("value");
    let test = test_node
        .map(|child| expr_json(child, source))
        .unwrap_or(Value::Null);
    let consequent = named_children(node)
        .filter(|child| {
            test_node.is_none_or(|test| {
                child.kind() != test.kind()
                    || child.start_byte() != test.start_byte()
                    || child.end_byte() != test.end_byte()
            })
        })
        .map(|child| stmt_json(child, source))
        .filter(|value| !value.is_null())
        .collect::<Vec<_>>();
    let colon = colon_child(node, source).unwrap_or(node);

    match node.kind() {
        "switch_case" | "switch_default" => Some(with_span_bounds(
            "SwitchCase",
            node.start_byte(),
            node.start_position(),
            colon.end_byte(),
            colon.end_position(),
            json!({
                "test": test,
                "consequent": consequent
            }),
        )),
        _ => None,
    }
}

fn labeled_statement_json(node: Node, source: &str) -> Value {
    let label = node
        .child_by_field_name("label")
        .map(|child| identifier_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "LabeledStatement",
        node,
        json!({
            "label": label,
            "body": body
        }),
    )
}

fn non_empty_stmt_or_expr_json(node: Node, source: &str) -> Option<Value> {
    if node.kind() == "empty_statement" {
        None
    } else if matches!(
        node.kind(),
        "lexical_declaration" | "variable_declaration" | "function_declaration"
    ) {
        Some(stmt_json(node, source))
    } else {
        Some(expr_json(node, source))
    }
}

fn jump_statement_json(kind: &str, node: Node, source: &str) -> Value {
    let label = node
        .child_by_field_name("label")
        .map(|child| expr_json(child, source))
        .unwrap_or(Value::Null);
    with_span(kind, node, json!({ "label": label }))
}

fn try_statement_json(node: Node, source: &str) -> Value {
    let block = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| block_from_node(node));
    let handler = node
        .child_by_field_name("handler")
        .map(|child| catch_clause_json(child, source))
        .unwrap_or(Value::Null);
    let finalizer = node
        .child_by_field_name("finalizer")
        .and_then(|finally_clause| finally_clause.child_by_field_name("body"))
        .map(|child| stmt_json(child, source))
        .unwrap_or(Value::Null);

    with_span(
        "TryStatement",
        node,
        json!({
            "block": block,
            "handler": handler,
            "finalizer": finalizer
        }),
    )
}

fn catch_clause_json(node: Node, source: &str) -> Value {
    let param = node
        .child_by_field_name("parameter")
        .map(|child| expr_json(child, source))
        .unwrap_or(Value::Null);
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| block_from_node(node));

    with_span(
        "CatchClause",
        node,
        json!({
            "param": param,
            "body": body
        }),
    )
}

fn throw_statement_json(node: Node, source: &str) -> Value {
    let argument = node
        .named_child(0)
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    with_span("ThrowStatement", node, json!({ "argument": argument }))
}

fn expression_statement_json(node: Node, source: &str) -> Value {
    if let Some(module) = node
        .named_child(0)
        .filter(|child| matches!(child.kind(), "internal_module" | "module"))
    {
        return ts_module_declaration_json(module, source);
    }

    let expression = node
        .named_child(0)
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "ExpressionStatement",
        node,
        json!({ "expression": expression }),
    )
}

fn binary_expression_json(node: Node, source: &str) -> Value {
    let left = field_json(node, "left", source).unwrap_or_else(|| noop_json(node));
    let right = field_json(node, "right", source).unwrap_or_else(|| noop_json(node));
    let operator = node
        .child_by_field_name("operator")
        .map(|child| node_text(child, source))
        .unwrap_or_else(|| infer_operator(node, source));

    with_span(
        "BinaryExpression",
        node,
        json!({
            "left": left,
            "operator": operator,
            "right": right
        }),
    )
}

fn assignment_expression_json(node: Node, source: &str) -> Value {
    let left = field_json(node, "left", source).unwrap_or_else(|| noop_json(node));
    let right = field_json(node, "right", source).unwrap_or_else(|| noop_json(node));
    let operator = node
        .child_by_field_name("operator")
        .map(|child| node_text(child, source))
        .unwrap_or_else(|| "=".to_string());

    with_span(
        "AssignmentExpression",
        node,
        json!({
            "left": left,
            "operator": operator,
            "right": right
        }),
    )
}

fn update_expression_json(node: Node, source: &str) -> Value {
    let argument = node
        .child_by_field_name("argument")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let operator = node
        .child_by_field_name("operator")
        .map(|child| node_text(child, source))
        .unwrap_or_else(|| infer_operator(node, source));
    let prefix = node
        .child(0)
        .map(|child| !child.is_named() && node_text(child, source) == operator)
        .unwrap_or(false);

    with_span(
        "UpdateExpression",
        node,
        json!({
            "argument": argument,
            "operator": operator,
            "prefix": prefix
        }),
    )
}

fn conditional_expression_json(node: Node, source: &str) -> Value {
    let test = field_json(node, "condition", source).unwrap_or_else(|| noop_json(node));
    let consequent = field_json(node, "consequence", source).unwrap_or_else(|| noop_json(node));
    let alternate = field_json(node, "alternative", source).unwrap_or_else(|| noop_json(node));

    with_span(
        "ConditionalExpression",
        node,
        json!({
            "test": test,
            "consequent": consequent,
            "alternate": alternate
        }),
    )
}

fn unary_expression_json(node: Node, source: &str) -> Value {
    let argument = node
        .child_by_field_name("argument")
        .or_else(|| node.named_child(0))
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let operator = node
        .child_by_field_name("operator")
        .map(|child| node_text(child, source))
        .unwrap_or_else(|| infer_operator(node, source));

    with_span(
        "UnaryExpression",
        node,
        json!({
            "operator": operator,
            "argument": argument,
            "prefix": true
        }),
    )
}

fn await_expression_json(node: Node, source: &str) -> Value {
    let argument = node
        .named_child(0)
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span("AwaitExpression", node, json!({ "argument": argument }))
}

fn ts_as_expression_json(node: Node, source: &str) -> Value {
    let expression = first_expression_child(node)
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let type_annotation = last_type_child(node)
        .map(|child| ts_type_json(child, source))
        .unwrap_or_else(|| with_span("TSAnyKeyword", node, json!({})));

    with_span(
        "TSAsExpression",
        node,
        json!({
            "expression": expression,
            "typeAnnotation": type_annotation
        }),
    )
}

fn ts_type_assertion_json(node: Node, source: &str) -> Value {
    let expression = first_expression_child(node)
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let type_annotation = last_type_child(node)
        .map(|child| ts_type_json(child, source))
        .or_else(|| type_assertion_type_json(node, source))
        .unwrap_or_else(|| with_span("TSAnyKeyword", node, json!({})));

    with_span(
        "TSTypeAssertion",
        node,
        json!({
            "expression": expression,
            "typeAnnotation": type_annotation
        }),
    )
}

fn type_assertion_type_json(node: Node, source: &str) -> Option<Value> {
    let text = node_text(node, source);
    let type_start = text.find('<')? + 1;
    let type_end = text[type_start..].find('>')? + type_start;
    let raw_type = &text[type_start..type_end];
    let leading_ws = raw_type.len() - raw_type.trim_start().len();
    let trailing_ws = raw_type.len() - raw_type.trim_end().len();
    let start_byte = node.start_byte() + type_start + leading_ws;
    let end_byte = node.start_byte() + type_end - trailing_ws;
    let type_text = source.get(start_byte..end_byte)?.trim();
    Some(ts_type_json_from_text(
        type_text, start_byte, end_byte, source,
    ))
}

fn ts_type_json_from_text(
    type_text: &str,
    start_byte: usize,
    end_byte: usize,
    source: &str,
) -> Value {
    let kind = match type_text {
        "any" => Some("TSAnyKeyword"),
        "bigint" => Some("TSBigIntKeyword"),
        "boolean" => Some("TSBooleanKeyword"),
        "never" => Some("TSNeverKeyword"),
        "null" => Some("TSNullKeyword"),
        "number" | "int" => Some("TSNumberKeyword"),
        "object" => Some("TSObjectKeyword"),
        "string" => Some("TSStringKeyword"),
        "symbol" => Some("TSSymbolKeyword"),
        "undefined" => Some("TSUndefinedKeyword"),
        "unknown" => Some("TSUnknownKeyword"),
        "void" => Some("TSVoidKeyword"),
        _ => None,
    };
    if let Some(kind) = kind {
        return with_span_bounds(
            kind,
            start_byte,
            point_for_byte(source, start_byte),
            end_byte,
            point_for_byte(source, end_byte),
            json!({}),
        );
    }

    with_span_bounds(
        "TSTypeReference",
        start_byte,
        point_for_byte(source, start_byte),
        end_byte,
        point_for_byte(source, end_byte),
        json!({
            "typeName": with_span_bounds(
                "Identifier",
                start_byte,
                point_for_byte(source, start_byte),
                end_byte,
                point_for_byte(source, end_byte),
                json!({ "name": type_text })
            )
        }),
    )
}

fn ts_satisfies_expression_json(node: Node, source: &str) -> Value {
    let expression = first_expression_child(node)
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let type_annotation = last_type_child(node)
        .map(|child| ts_type_json(child, source))
        .unwrap_or_else(|| with_span("TSAnyKeyword", node, json!({})));

    with_span(
        "TSSatisfiesExpression",
        node,
        json!({
            "expression": expression,
            "typeAnnotation": type_annotation
        }),
    )
}

fn ts_non_null_expression_json(node: Node, source: &str) -> Value {
    let expression = node
        .named_child(0)
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "TSNonNullExpression",
        node,
        json!({ "expression": expression }),
    )
}

fn sequence_expression_json(node: Node, source: &str) -> Value {
    let expressions = named_children(node)
        .map(|child| expr_json(child, source))
        .collect::<Vec<_>>();

    with_span(
        "SequenceExpression",
        node,
        json!({ "expressions": expressions }),
    )
}

fn parameter_json(node: Node, source: &str) -> Value {
    let parameter_property = is_parameter_property(node, source);
    let left_node = node
        .child_by_field_name("pattern")
        .or_else(|| node.child_by_field_name("name"))
        .or_else(|| node.named_child(0));
    let type_annotation = node
        .child_by_field_name("type")
        .map(|child| ts_type_annotation_json(child, source));
    let left = match left_node {
        Some(child)
            if matches!(
                child.kind(),
                "identifier" | "property_identifier" | "type_identifier"
            ) =>
        {
            let span_node = if parameter_property { child } else { node };
            identifier_json_with_span(child, span_node, source, type_annotation.clone())
        }
        Some(child) => {
            let value = pattern_json(child, source);
            if let Some(annotation) = type_annotation.clone() {
                with_extra_field(value, "typeAnnotation", annotation)
            } else {
                value
            }
        }
        None => noop_json(node),
    };

    let parameter = if let Some(right) = node.child_by_field_name("value") {
        with_span(
            "AssignmentPattern",
            node,
            json!({
                "left": left,
                "right": expr_json(right, source)
            }),
        )
    } else {
        left
    };
    let parameter = with_decorators(parameter, node, source);

    if parameter_property {
        let mut fields = json!({
            "parameter": parameter,
            "readonly": has_named_or_keyword_child(node, source, "readonly"),
            "decorators": decorators_json(node, source)
        });
        if let Some(annotation) = type_annotation {
            fields = with_extra_field(fields, "typeAnnotation", annotation);
        }
        if let Some(accessibility) = accessibility_modifier(node, source) {
            fields = with_extra_field(fields, "accessibility", Value::String(accessibility));
        }
        return with_span("TSParameterProperty", node, fields);
    }

    parameter
}

fn call_expression_json(node: Node, source: &str) -> Value {
    if node
        .child_by_field_name("arguments")
        .is_some_and(|child| child.kind() == "template_string")
    {
        return tagged_template_expression_json(node, source);
    }

    let callee = node
        .child_by_field_name("function")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let arguments = node
        .child_by_field_name("arguments")
        .map(|args_node| {
            named_children(args_node)
                .map(|child| expr_json(child, source))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    with_span(
        "CallExpression",
        node,
        json!({
            "callee": callee,
            "arguments": arguments,
            "optional": false
        }),
    )
}

fn tagged_template_expression_json(node: Node, source: &str) -> Value {
    let tag = node
        .child_by_field_name("function")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let quasi = node
        .child_by_field_name("arguments")
        .map(|child| template_literal_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "TaggedTemplateExpression",
        node,
        json!({
            "tag": tag,
            "quasi": quasi
        }),
    )
}

fn new_expression_json(node: Node, source: &str) -> Value {
    let callee = node
        .child_by_field_name("constructor")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let arguments = node
        .child_by_field_name("arguments")
        .map(|args_node| {
            named_children(args_node)
                .map(|child| expr_json(child, source))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    with_span(
        "NewExpression",
        node,
        json!({
            "callee": callee,
            "arguments": arguments
        }),
    )
}

fn member_expression_json(node: Node, source: &str) -> Value {
    let object = field_json(node, "object", source).unwrap_or_else(|| noop_json(node));
    let property = field_json(node, "property", source).unwrap_or_else(|| noop_json(node));

    with_span(
        "MemberExpression",
        node,
        json!({
            "object": object,
            "property": property,
            "computed": false,
            "optional": false
        }),
    )
}

fn subscript_expression_json(node: Node, source: &str) -> Value {
    let object = field_json(node, "object", source).unwrap_or_else(|| noop_json(node));
    let property = field_json(node, "index", source).unwrap_or_else(|| noop_json(node));

    with_span(
        "MemberExpression",
        node,
        json!({
            "object": object,
            "property": property,
            "computed": true,
            "optional": false
        }),
    )
}

fn array_expression_json(node: Node, source: &str) -> Value {
    let elements = array_elements_json(node, source, expr_json);

    with_span("ArrayExpression", node, json!({ "elements": elements }))
}

fn array_pattern_json(node: Node, source: &str) -> Value {
    let elements = array_elements_json(node, source, pattern_json);

    with_span("ArrayPattern", node, json!({ "elements": elements }))
}

fn array_elements_json(
    node: Node,
    source: &str,
    value_json: fn(Node, &str) -> Value,
) -> Vec<Value> {
    let mut elements = Vec::new();
    let mut expect_element = true;

    for index in 0..node.child_count() {
        let Some(child) = node.child(index) else {
            continue;
        };
        if child.is_named() {
            if !is_comment(child) {
                elements.push(value_json(child, source));
                expect_element = false;
            }
            continue;
        }

        if node_text(child, source) == "," {
            if expect_element {
                elements.push(Value::Null);
            }
            expect_element = true;
        }
    }

    elements
}

fn jsx_element_json(node: Node, source: &str) -> Value {
    let opening_node = node.child_by_field_name("open_tag");
    let closing_node = node.child_by_field_name("close_tag");
    let opening = opening_node
        .map(|child| jsx_opening_element_json(child, false, source))
        .unwrap_or_else(|| noop_json(node));
    let closing = closing_node
        .map(|child| jsx_closing_element_json(child, source))
        .unwrap_or(Value::Null);
    let children = jsx_element_children_json(node, opening_node, closing_node, source);

    with_span(
        "JSXElement",
        node,
        json!({
            "openingElement": opening,
            "closingElement": closing,
            "children": children
        }),
    )
}

fn jsx_element_children_json(
    node: Node,
    opening_node: Option<Node>,
    closing_node: Option<Node>,
    source: &str,
) -> Vec<Value> {
    let mut values = Vec::new();
    let mut cursor = opening_node
        .map(|child| child.end_byte())
        .unwrap_or_else(|| node.start_byte());
    let closing_start = closing_node
        .map(|child| child.start_byte())
        .unwrap_or_else(|| node.end_byte());

    for child in children(node)
        .filter(|child| !matches!(child.kind(), "jsx_opening_element" | "jsx_closing_element"))
    {
        if child.start_byte() > cursor {
            values.push(jsx_text_json_bounds(cursor, child.start_byte(), source));
        }
        let value = jsx_child_json(child, source);
        if !value.is_null() {
            values.push(value);
        }
        cursor = child.end_byte();
    }

    if closing_start > cursor {
        values.push(jsx_text_json_bounds(cursor, closing_start, source));
    }

    values
}

fn jsx_self_closing_element_json(node: Node, source: &str) -> Value {
    let opening = jsx_opening_element_json(node, true, source);
    with_span(
        "JSXElement",
        node,
        json!({
            "openingElement": opening,
            "closingElement": Value::Null,
            "children": []
        }),
    )
}

fn jsx_child_json(node: Node, source: &str) -> Value {
    match node.kind() {
        "jsx_element" => jsx_element_json(node, source),
        "jsx_self_closing_element" => jsx_self_closing_element_json(node, source),
        "jsx_expression" => jsx_expression_container_json(node, source),
        "jsx_text" | "html_character_reference" => with_span("JSXText", node, json!({})),
        _ => expr_json(node, source),
    }
}

fn jsx_text_json_bounds(start_byte: usize, end_byte: usize, source: &str) -> Value {
    with_span_bounds(
        "JSXText",
        start_byte,
        point_for_byte(source, start_byte),
        end_byte,
        point_for_byte(source, end_byte),
        json!({}),
    )
}

fn jsx_opening_element_json(node: Node, self_closing: bool, source: &str) -> Value {
    let name = node
        .child_by_field_name("name")
        .map(|child| jsx_name_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let attributes = named_children(node)
        .filter(|child| matches!(child.kind(), "jsx_attribute" | "jsx_expression"))
        .map(|child| match child.kind() {
            "jsx_attribute" => jsx_attribute_json(child, source),
            "jsx_expression" => jsx_expression_container_json(child, source),
            _ => noop_json(child),
        })
        .collect::<Vec<_>>();

    with_span(
        "JSXOpeningElement",
        node,
        json!({
            "name": name,
            "attributes": attributes,
            "selfClosing": self_closing
        }),
    )
}

fn jsx_closing_element_json(node: Node, source: &str) -> Value {
    let name = node
        .child_by_field_name("name")
        .map(|child| jsx_name_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span("JSXClosingElement", node, json!({ "name": name }))
}

fn jsx_attribute_json(node: Node, source: &str) -> Value {
    let name_node = named_children(node)
        .find(|child| {
            matches!(
                child.kind(),
                "property_identifier" | "identifier" | "jsx_namespace_name"
            )
        })
        .unwrap_or(node);
    let value = named_children(node)
        .find(|child| {
            matches!(
                child.kind(),
                "string" | "jsx_expression" | "jsx_element" | "jsx_self_closing_element"
            )
        })
        .map(|child| match child.kind() {
            "string" => string_literal_json(child, source),
            "jsx_expression" => jsx_expression_container_json(child, source),
            "jsx_element" => jsx_element_json(child, source),
            "jsx_self_closing_element" => jsx_self_closing_element_json(child, source),
            _ => noop_json(child),
        })
        .unwrap_or(Value::Null);

    with_span(
        "JSXAttribute",
        node,
        json!({
            "name": jsx_name_json(name_node, source),
            "value": value
        }),
    )
}

fn jsx_expression_container_json(node: Node, source: &str) -> Value {
    let expression = node
        .named_child(0)
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| with_span("JSXEmptyExpression", node, json!({})));

    with_span(
        "JSXExpressionContainer",
        node,
        json!({ "expression": expression }),
    )
}

fn jsx_name_json(node: Node, source: &str) -> Value {
    match node.kind() {
        "jsx_namespace_name" => with_span(
            "JSXIdentifier",
            node,
            json!({ "name": node_text(node, source) }),
        ),
        _ => with_span(
            "JSXIdentifier",
            node,
            json!({ "name": node_text(node, source) }),
        ),
    }
}

fn object_expression_json(node: Node, source: &str) -> Value {
    let properties = named_children(node)
        .filter_map(|child| object_property_json(child, source))
        .collect::<Vec<_>>();

    with_span(
        "ObjectExpression",
        node,
        json!({ "properties": properties }),
    )
}

fn object_pattern_json(node: Node, source: &str) -> Value {
    let properties = named_children(node)
        .filter_map(|child| object_pattern_property_json(child, source))
        .collect::<Vec<_>>();

    with_span("ObjectPattern", node, json!({ "properties": properties }))
}

fn object_property_json(node: Node, source: &str) -> Option<Value> {
    match node.kind() {
        "pair" => Some(object_pair_json(node, source)),
        "method_definition" => Some(object_method_json(node, source)),
        "spread_element" => Some(unary_argument_json("SpreadElement", node, source)),
        "shorthand_property_identifier" => Some(shorthand_object_property_json(node, source)),
        _ => None,
    }
}

fn object_pattern_property_json(node: Node, source: &str) -> Option<Value> {
    match node.kind() {
        "pair_pattern" => Some(object_pair_pattern_json(node, source)),
        "object_assignment_pattern" => Some(object_assignment_pattern_json(node, source)),
        "rest_pattern" => Some(unary_argument_json("RestElement", node, source)),
        "shorthand_property_identifier_pattern" => {
            Some(shorthand_object_property_json(node, source))
        }
        _ => None,
    }
}

fn object_pair_json(node: Node, source: &str) -> Value {
    let key_node = node.child_by_field_name("key").unwrap_or(node);
    let computed = key_node.kind() == "computed_property_name";
    let key = object_key_json(key_node, source);
    let value = field_json(node, "value", source).unwrap_or_else(|| noop_json(node));

    with_span(
        "ObjectProperty",
        node,
        json!({
            "key": key,
            "value": value,
            "computed": computed,
            "shorthand": false
        }),
    )
}

fn object_pair_pattern_json(node: Node, source: &str) -> Value {
    let key_node = node.child_by_field_name("key").unwrap_or(node);
    let computed = key_node.kind() == "computed_property_name";
    let key = object_key_json(key_node, source);
    let value = node
        .child_by_field_name("value")
        .map(|child| pattern_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "ObjectProperty",
        node,
        json!({
            "key": key,
            "value": value,
            "computed": computed,
            "shorthand": false
        }),
    )
}

fn object_assignment_pattern_json(node: Node, source: &str) -> Value {
    let left = node.child_by_field_name("left").unwrap_or(node);
    let right = node
        .child_by_field_name("right")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let key = match left.kind() {
        "shorthand_property_identifier_pattern" | "identifier" | "property_identifier" => {
            identifier_json(left, source)
        }
        _ => pattern_json(left, source),
    };
    let value = with_span(
        "AssignmentPattern",
        node,
        json!({
            "left": pattern_json(left, source),
            "right": right
        }),
    );

    with_span(
        "ObjectProperty",
        node,
        json!({
            "key": key,
            "value": value,
            "computed": false,
            "shorthand": true
        }),
    )
}

fn object_method_json(node: Node, source: &str) -> Value {
    let key_node = node.child_by_field_name("name").unwrap_or(node);
    let computed = key_node.kind() == "computed_property_name";
    let key = object_key_json(key_node, source);
    let params = params_json(node, source);
    let body = node
        .child_by_field_name("body")
        .map(|child| stmt_json(child, source))
        .unwrap_or_else(|| block_from_node(node));

    with_span(
        "ObjectMethod",
        node,
        json!({
            "kind": object_method_kind(node, source),
            "key": key,
            "params": params,
            "body": body,
            "computed": computed,
            "generator": has_keyword_child(node, source, "*"),
            "async": has_keyword_child(node, source, "async")
        }),
    )
}

fn object_method_kind(node: Node, source: &str) -> &'static str {
    if node
        .child_by_field_name("name")
        .is_some_and(|child| node_text(child, source) == "constructor")
    {
        "constructor"
    } else if has_keyword_child(node, source, "get") {
        "get"
    } else if has_keyword_child(node, source, "set") {
        "set"
    } else {
        "method"
    }
}

fn shorthand_object_property_json(node: Node, source: &str) -> Value {
    let identifier = identifier_json(node, source);
    with_span(
        "ObjectProperty",
        node,
        json!({
            "key": identifier.clone(),
            "value": identifier,
            "computed": false,
            "shorthand": true
        }),
    )
}

fn object_key_json(node: Node, source: &str) -> Value {
    if node.kind() == "computed_property_name" {
        return node
            .named_child(0)
            .map(|child| expr_json(child, source))
            .unwrap_or_else(|| noop_json(node));
    }
    expr_json(node, source)
}

fn assignment_pattern_json(node: Node, source: &str) -> Value {
    let left = node
        .child_by_field_name("left")
        .map(|child| pattern_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let right = node
        .child_by_field_name("right")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));

    with_span(
        "AssignmentPattern",
        node,
        json!({
            "left": left,
            "right": right
        }),
    )
}

fn arrow_function_json(node: Node, source: &str) -> Value {
    let params = arrow_params_json(node, source);
    let body_node = node.child_by_field_name("body").unwrap_or(node);
    let expression = body_node.kind() != "statement_block";
    let body = if expression {
        expr_json(body_node, source)
    } else {
        stmt_json(body_node, source)
    };

    with_span(
        "ArrowFunctionExpression",
        node,
        json!({
            "id": Value::Null,
            "params": params,
            "body": body,
            "expression": expression,
            "generator": false,
            "async": has_keyword_child(node, source, "async")
        }),
    )
}

fn arrow_params_json(node: Node, source: &str) -> Vec<Value> {
    if let Some(params_node) = node.child_by_field_name("parameters") {
        return params_from_node(params_node, source);
    }
    node.child_by_field_name("parameter")
        .map(|param| vec![expr_json(param, source)])
        .unwrap_or_default()
}

fn params_json(node: Node, source: &str) -> Vec<Value> {
    node.child_by_field_name("parameters")
        .map(|params_node| params_from_node(params_node, source))
        .unwrap_or_default()
}

fn params_from_node(node: Node, source: &str) -> Vec<Value> {
    named_children(node)
        .filter(|child| !is_comment(*child))
        .map(|child| expr_json(child, source))
        .collect()
}

fn unary_argument_json(kind: &str, node: Node, source: &str) -> Value {
    let argument = node
        .child_by_field_name("argument")
        .or_else(|| node.named_child(0))
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    let fields = if kind == "RestElement" {
        json!({
            "argument": argument,
            "typeAnnotation": array_type_annotation_json(node)
        })
    } else {
        json!({ "argument": argument })
    };
    with_span(kind, node, fields)
}

fn array_type_annotation_json(node: Node) -> Value {
    with_span(
        "TSTypeAnnotation",
        node,
        json!({
            "typeAnnotation": with_span(
                "TSArrayType",
                node,
                json!({
                    "elementType": with_span("TSAnyKeyword", node, json!({}))
                })
            )
        }),
    )
}

fn identifier_json(node: Node, source: &str) -> Value {
    identifier_json_with_span(node, node, source, None)
}

fn identifier_json_with_span(
    name_node: Node,
    span_node: Node,
    source: &str,
    type_annotation: Option<Value>,
) -> Value {
    let mut fields = json!({ "name": node_text(name_node, source) });
    if let Some(annotation) = type_annotation {
        fields = with_extra_field(fields, "typeAnnotation", annotation);
    }
    with_span("Identifier", span_node, fields)
}

fn decorators_json(node: Node, source: &str) -> Vec<Value> {
    named_children(node)
        .filter(|child| child.kind() == "decorator")
        .map(|child| decorator_json(child, source))
        .collect()
}

fn decorator_json(node: Node, source: &str) -> Value {
    let expression = named_children(node)
        .find(|child| child.kind() != "decorator")
        .map(|child| expr_json(child, source))
        .unwrap_or_else(|| noop_json(node));
    with_span("Decorator", node, json!({ "expression": expression }))
}

fn with_decorators(value: Value, node: Node, source: &str) -> Value {
    let decorators = decorators_json(node, source);
    if decorators.is_empty() {
        value
    } else {
        with_extra_field(value, "decorators", Value::Array(decorators))
    }
}

fn with_leading_decorators(value: Value, node: Node, source: &str) -> Value {
    let mut decorators = decorators_json(node, source);
    with_decorator_values(value, &mut decorators)
}

fn with_decorator_values(value: Value, decorators: &mut Vec<Value>) -> Value {
    if decorators.is_empty() {
        return value;
    }

    let mut object = match value {
        Value::Object(map) => map,
        other => return other,
    };
    if let Some(Value::Array(existing)) = object.remove("decorators") {
        decorators.extend(existing);
    }
    object.insert(
        "decorators".to_string(),
        Value::Array(std::mem::take(decorators)),
    );
    Value::Object(object)
}

fn with_extra_field(value: Value, key: &str, field_value: Value) -> Value {
    let mut object = match value {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    object.insert(key.to_string(), field_value);
    Value::Object(object)
}

fn ts_type_annotation_json(node: Node, source: &str) -> Value {
    let type_annotation = node
        .named_child(0)
        .map(|child| ts_type_json(child, source))
        .unwrap_or_else(|| with_span("TSAnyKeyword", node, json!({})));

    with_span(
        "TSTypeAnnotation",
        node,
        json!({ "typeAnnotation": type_annotation }),
    )
}

fn ts_type_json(node: Node, source: &str) -> Value {
    match node.kind() {
        "type_annotation" => ts_type_annotation_json(node, source),
        "predefined_type" => ts_predefined_type_json(node, source),
        "type_identifier" | "nested_type_identifier" | "generic_type" => with_span(
            "TSTypeReference",
            node,
            json!({ "typeName": type_name_json(node, source) }),
        ),
        "array_type" => with_span(
            "TSArrayType",
            node,
            json!({
                "elementType": node
                    .named_child(0)
                    .map(|child| ts_type_json(child, source))
                    .unwrap_or_else(|| with_span("TSAnyKeyword", node, json!({})))
            }),
        ),
        "object_type" => with_span(
            "TSTypeLiteral",
            node,
            json!({
                "members": named_children(node)
                    .filter_map(|child| ts_interface_member_json(child, source))
                    .collect::<Vec<_>>()
            }),
        ),
        "literal_type" => with_span(
            "TSLiteralType",
            node,
            json!({
                "literal": node
                    .named_child(0)
                    .map(|child| expr_json(child, source))
                    .unwrap_or_else(|| noop_json(node))
            }),
        ),
        "union_type" => with_span(
            "TSUnionType",
            node,
            json!({
                "types": named_children(node)
                    .filter(|child| is_type_like(*child))
                    .map(|child| ts_type_json(child, source))
                    .collect::<Vec<_>>()
            }),
        ),
        "intersection_type" => with_span(
            "TSIntersectionType",
            node,
            json!({
                "types": named_children(node)
                    .filter(|child| is_type_like(*child))
                    .map(|child| ts_type_json(child, source))
                    .collect::<Vec<_>>()
            }),
        ),
        _ => with_span("TSAnyKeyword", node, json!({})),
    }
}

fn ts_predefined_type_json(node: Node, source: &str) -> Value {
    let kind = match node_text(node, source).as_str() {
        "any" => "TSAnyKeyword",
        "bigint" => "TSBigIntKeyword",
        "boolean" => "TSBooleanKeyword",
        "never" => "TSNeverKeyword",
        "null" => "TSNullKeyword",
        "number" => "TSNumberKeyword",
        "object" => "TSObjectKeyword",
        "string" => "TSStringKeyword",
        "symbol" => "TSSymbolKeyword",
        "undefined" => "TSUndefinedKeyword",
        "unknown" => "TSUnknownKeyword",
        "void" => "TSVoidKeyword",
        _ => "TSAnyKeyword",
    };
    with_span(kind, node, json!({}))
}

fn numeric_literal_json(node: Node, source: &str) -> Value {
    let raw = node_text(node, source);
    let value = raw.parse::<f64>().ok().map_or(Value::Null, Value::from);
    with_span(
        "NumericLiteral",
        node,
        json!({ "value": value, "extra": { "raw": raw } }),
    )
}

fn string_literal_json(node: Node, source: &str) -> Value {
    let raw = node_text(node, source);
    let value = decode_js_string_literal(&raw);
    with_span(
        "StringLiteral",
        node,
        json!({ "value": value, "extra": { "raw": raw } }),
    )
}

fn template_string_json(node: Node, source: &str) -> Value {
    if named_children(node).any(|child| child.kind() == "template_substitution") {
        template_literal_json(node, source)
    } else {
        string_literal_json(node, source)
    }
}

fn template_literal_json(node: Node, source: &str) -> Value {
    let substitutions = named_children(node)
        .filter(|child| child.kind() == "template_substitution")
        .collect::<Vec<_>>();
    let mut quasis = Vec::with_capacity(substitutions.len() + 1);
    let mut expressions = Vec::with_capacity(substitutions.len());
    let mut quasi_start = node.start_byte().saturating_add(1);
    let content_end = node.end_byte().saturating_sub(1);

    for substitution in &substitutions {
        quasis.push(template_element_json(
            quasi_start,
            substitution.start_byte(),
            false,
            source,
        ));
        if let Some(expression) = substitution.named_child(0) {
            expressions.push(expr_json(expression, source));
        }
        quasi_start = substitution.end_byte();
    }

    quasis.push(template_element_json(
        quasi_start,
        content_end,
        true,
        source,
    ));

    with_span(
        "TemplateLiteral",
        node,
        json!({
            "expressions": expressions,
            "quasis": quasis
        }),
    )
}

fn template_element_json(start_byte: usize, end_byte: usize, tail: bool, source: &str) -> Value {
    let raw = source
        .get(start_byte..end_byte)
        .unwrap_or_default()
        .to_string();
    with_span_bounds(
        "TemplateElement",
        start_byte,
        point_for_byte(source, start_byte),
        end_byte,
        point_for_byte(source, end_byte),
        json!({
            "value": {
                "raw": raw,
                "cooked": decode_js_string_escapes(&raw)
            },
            "tail": tail
        }),
    )
}

fn decode_js_string_literal(raw: &str) -> String {
    let Some(quote) = raw.chars().next() else {
        return String::new();
    };
    if !matches!(quote, '"' | '\'' | '`') || !raw.ends_with(quote) || raw.len() < 2 {
        return raw.to_string();
    }
    let body = &raw[1..raw.len() - 1];
    decode_js_string_escapes(body)
}

fn decode_js_string_escapes(body: &str) -> String {
    let chars = body.chars().collect::<Vec<_>>();
    let mut decoded = String::with_capacity(body.len());
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        index += 1;
        if ch != '\\' {
            decoded.push(ch);
            continue;
        }

        if index >= chars.len() {
            decoded.push('\\');
            break;
        }

        let escaped = chars[index];
        index += 1;
        match escaped {
            '"' => decoded.push('"'),
            '\'' => decoded.push('\''),
            '`' => decoded.push('`'),
            '\\' => decoded.push('\\'),
            'b' => decoded.push('\u{0008}'),
            'f' => decoded.push('\u{000c}'),
            'n' => decoded.push('\n'),
            'r' => decoded.push('\r'),
            't' => decoded.push('\t'),
            'v' => decoded.push('\u{000b}'),
            '0' => decoded.push('\0'),
            '\n' => {}
            '\r' => {
                if index < chars.len() && chars[index] == '\n' {
                    index += 1;
                }
            }
            'x' if index + 2 <= chars.len()
                && chars[index..index + 2]
                    .iter()
                    .all(|c| c.is_ascii_hexdigit()) =>
            {
                if let Some(value) = decode_hex_escape(&chars[index..index + 2]) {
                    decoded.push(value);
                }
                index += 2;
            }
            'u' if index < chars.len() && chars[index] == '{' => {
                index += 1;
                let start = index;
                while index < chars.len() && chars[index] != '}' {
                    index += 1;
                }
                if index < chars.len() && chars[index] == '}' {
                    if let Some(value) = decode_hex_escape(&chars[start..index]) {
                        decoded.push(value);
                    }
                    index += 1;
                }
            }
            'u' if index + 4 <= chars.len()
                && chars[index..index + 4]
                    .iter()
                    .all(|c| c.is_ascii_hexdigit()) =>
            {
                if let Some(value) = decode_hex_escape(&chars[index..index + 4]) {
                    decoded.push(value);
                }
                index += 4;
            }
            other => decoded.push(other),
        }
    }
    decoded
}

fn decode_hex_escape(digits: &[char]) -> Option<char> {
    let value = digits
        .iter()
        .collect::<String>()
        .chars()
        .try_fold(0_u32, |acc, ch| {
            ch.to_digit(16)
                .map(|digit| acc.saturating_mul(16).saturating_add(digit))
        })?;
    char::from_u32(value)
}

fn boolean_literal_json(node: Node, value: bool) -> Value {
    with_span("BooleanLiteral", node, json!({ "value": value }))
}

fn private_name_json(node: Node, source: &str) -> Value {
    let name = node_text(node, source).trim_start_matches('#').to_string();
    let id_start = if source.as_bytes().get(node.start_byte()) == Some(&b'#') {
        node.start_byte() + 1
    } else {
        node.start_byte()
    };
    let id = with_span_bounds(
        "Identifier",
        id_start,
        point_for_byte(source, id_start),
        node.end_byte(),
        node.end_position(),
        json!({ "name": name }),
    );
    with_span("PrivateName", node, json!({ "id": id }))
}

fn identifier_from_name(node: Node, name: &str) -> Value {
    with_span("Identifier", node, json!({ "name": name }))
}

fn type_name_json(node: Node, source: &str) -> Value {
    match node.kind() {
        "generic_type" => node
            .named_child(0)
            .map(|child| type_name_json(child, source))
            .unwrap_or_else(|| identifier_json(node, source)),
        "nested_type_identifier" => {
            let text = node_text(node, source);
            let mut parts = text.split('.').filter(|part| !part.is_empty());
            let Some(first) = parts.next() else {
                return identifier_json(node, source);
            };
            parts.fold(identifier_from_name(node, first), |left, right| {
                with_span(
                    "TSQualifiedName",
                    node,
                    json!({
                        "left": left,
                        "right": identifier_from_name(node, right)
                    }),
                )
            })
        }
        _ => identifier_json(node, source),
    }
}

fn field_json(node: Node, field: &str, source: &str) -> Option<Value> {
    node.child_by_field_name(field)
        .map(|child| expr_json(child, source))
}

fn pattern_json(node: Node, source: &str) -> Value {
    match node.kind() {
        "array_pattern" => array_pattern_json(node, source),
        "object_pattern" => object_pattern_json(node, source),
        "assignment_pattern" => assignment_pattern_json(node, source),
        "rest_pattern" => unary_argument_json("RestElement", node, source),
        "parenthesized_expression" => node
            .named_child(0)
            .map(|child| pattern_json(child, source))
            .unwrap_or_else(|| noop_json(node)),
        _ => expr_json(node, source),
    }
}

fn pattern_or_expr_json(node: Node, source: &str) -> Value {
    match node.kind() {
        "array_pattern" | "object_pattern" | "assignment_pattern" | "rest_pattern" => {
            pattern_json(node, source)
        }
        _ => expr_json(node, source),
    }
}

fn declaration_kind(node: Node, source: &str) -> String {
    for index in 0..node.child_count() {
        if let Some(child) = node.child(index) {
            let text = node_text(child, source);
            if matches!(text.as_str(), "let" | "const" | "var") {
                return text;
            }
        }
    }
    "var".to_string()
}

fn declaration_kind_in_for_in_of(node: Node, source: &str) -> Option<String> {
    (0..node.child_count())
        .filter_map(|index| node.child(index))
        .filter(|child| !child.is_named())
        .map(|child| node_text(child, source))
        .find(|text| matches!(text.as_str(), "let" | "const" | "var"))
}

fn with_span(kind: &str, node: Node, fields: Value) -> Value {
    with_span_bounds(
        kind,
        node.start_byte(),
        node.start_position(),
        node.end_byte(),
        node.end_position(),
        fields,
    )
}

fn with_span_including_trailing_semicolon(
    kind: &str,
    node: Node,
    source: &str,
    fields: Value,
) -> Value {
    let end_byte = node.end_byte();
    if source
        .as_bytes()
        .get(end_byte)
        .is_some_and(|byte| *byte == b';')
    {
        let adjusted_end = end_byte + 1;
        return with_span_bounds(
            kind,
            node.start_byte(),
            node.start_position(),
            adjusted_end,
            point_for_byte(source, adjusted_end),
            fields,
        );
    }
    with_span(kind, node, fields)
}

fn with_span_bounds(
    kind: &str,
    start_byte: usize,
    start_position: Point,
    end_byte: usize,
    end_position: Point,
    fields: Value,
) -> Value {
    let mut object = match fields {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    object.insert("type".into(), Value::String(kind.into()));
    object.insert("start".into(), Value::from(start_byte));
    object.insert("end".into(), Value::from(end_byte));
    object.insert(
        "loc".into(),
        json!({
            "start": {
                "line": start_position.row + 1,
                "column": start_position.column
            },
            "end": {
                "line": end_position.row + 1,
                "column": end_position.column
            }
        }),
    );
    Value::Object(object)
}

fn noop_json(node: Node) -> Value {
    with_span("Noop", node, json!({}))
}

fn node_text(node: Node, source: &str) -> String {
    node.utf8_text(source.as_bytes()).unwrap_or("").to_string()
}

fn convert_spans_to_utf16(value: &mut Value, source: &str) {
    match value {
        Value::Object(map) => {
            if let Some(start) = map.get("start").and_then(Value::as_u64) {
                map.insert(
                    "start".into(),
                    Value::from(utf16_offset_for_byte(source, start as usize)),
                );
            }
            if let Some(end) = map.get("end").and_then(Value::as_u64) {
                map.insert(
                    "end".into(),
                    Value::from(utf16_offset_for_byte(source, end as usize)),
                );
            }
            for child in map.values_mut() {
                convert_spans_to_utf16(child, source);
            }
        }
        Value::Array(values) => {
            for child in values {
                convert_spans_to_utf16(child, source);
            }
        }
        _ => {}
    }
}

fn utf16_offset_for_byte(source: &str, byte: usize) -> usize {
    let clamped = previous_char_boundary(source, byte.min(source.len()));
    source[..clamped].encode_utf16().count()
}

fn previous_char_boundary(source: &str, byte: usize) -> usize {
    let mut boundary = byte.min(source.len());
    while boundary > 0 && !source.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

fn point_for_byte(source: &str, byte: usize) -> Point {
    let clamped = previous_char_boundary(source, byte.min(source.len()));
    let mut row = 0;
    let mut line_start = 0;
    for (index, value) in source.bytes().take(clamped).enumerate() {
        if value == b'\n' {
            row += 1;
            line_start = index + 1;
        }
    }
    Point {
        row,
        column: source[line_start..clamped].encode_utf16().count(),
    }
}

fn named_children(node: Node) -> impl Iterator<Item = Node> {
    (0..node.named_child_count()).filter_map(move |index| node.named_child(index))
}

fn children(node: Node) -> impl Iterator<Item = Node> {
    (0..node.child_count()).filter_map(move |index| node.child(index))
}

fn first_named_child(node: Node) -> Option<Node> {
    node.named_child(0)
}

fn first_expression_child(node: Node) -> Option<Node> {
    named_children(node).find(|child| is_expression_like(*child))
}

fn last_type_child(node: Node) -> Option<Node> {
    named_children(node)
        .filter(|child| is_type_like(*child))
        .last()
}

fn is_expression_like(node: Node) -> bool {
    matches!(
        node.kind(),
        "identifier"
            | "property_identifier"
            | "number"
            | "string"
            | "template_string"
            | "true"
            | "false"
            | "null"
            | "this"
            | "binary_expression"
            | "unary_expression"
            | "assignment_expression"
            | "augmented_assignment_expression"
            | "update_expression"
            | "ternary_expression"
            | "call_expression"
            | "new_expression"
            | "member_expression"
            | "subscript_expression"
            | "array"
            | "object"
            | "function_expression"
            | "arrow_function"
            | "class"
            | "non_null_expression"
            | "sequence_expression"
            | "parenthesized_expression"
            | "as_expression"
            | "type_assertion"
            | "satisfies_expression"
            | "await_expression"
    )
}

fn is_type_like(node: Node) -> bool {
    matches!(
        node.kind(),
        "predefined_type"
            | "type_identifier"
            | "nested_type_identifier"
            | "type_annotation"
            | "array_type"
            | "generic_type"
            | "union_type"
            | "intersection_type"
            | "object_type"
            | "tuple_type"
    )
}

fn is_comment(node: Node) -> bool {
    matches!(node.kind(), "comment" | "hash_bang_line")
}

fn infer_operator(node: Node, source: &str) -> String {
    for index in 0..node.child_count() {
        if let Some(child) = node.child(index) {
            if !child.is_named() {
                let text = node_text(child, source);
                if !text.trim().is_empty() {
                    return text;
                }
            }
        }
    }
    String::new()
}

fn has_keyword_child(node: Node, source: &str, keyword: &str) -> bool {
    for index in 0..node.child_count() {
        if let Some(child) = node.child(index) {
            if !child.is_named() && node_text(child, source) == keyword {
                return true;
            }
        }
    }
    false
}

fn has_named_or_keyword_child(node: Node, source: &str, keyword: &str) -> bool {
    has_keyword_child(node, source, keyword)
        || named_children(node).any(|child| node_text(child, source) == keyword)
}

fn accessibility_modifier(node: Node, source: &str) -> Option<String> {
    named_children(node)
        .find(|child| child.kind() == "accessibility_modifier")
        .map(|child| node_text(child, source))
}

fn is_parameter_property(node: Node, source: &str) -> bool {
    accessibility_modifier(node, source).is_some()
        || has_named_or_keyword_child(node, source, "readonly")
}

fn colon_child<'a>(node: Node<'a>, source: &str) -> Option<Node<'a>> {
    (0..node.child_count())
        .filter_map(|index| node.child(index))
        .find(|child| !child.is_named() && node_text(*child, source) == ":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_babel_shaped_program_for_core_javascript() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "const answer = 40 + 2;\nfunction id(x) { return x; }\nid(answer);\n",
        )
        .expect("parse succeeds");

        assert_eq!(json["relativeName"], "app.js");
        assert_eq!(json["ast"]["type"], "File");
        assert_eq!(json["ast"]["program"]["type"], "Program");
        assert_eq!(
            json["ast"]["program"]["body"][0]["type"],
            "VariableDeclaration"
        );
        assert_eq!(
            json["ast"]["program"]["body"][0]["declarations"][0]["id"]["name"],
            "answer"
        );
        assert_eq!(
            json["ast"]["program"]["body"][1]["type"],
            "FunctionDeclaration"
        );
        assert_eq!(
            json["ast"]["program"]["body"][1]["body"]["body"][0]["argument"]["name"],
            "x"
        );
        assert_eq!(
            json["ast"]["program"]["body"][2]["type"],
            "ExpressionStatement"
        );
    }

    #[test]
    fn emits_rest_parameters_as_babel_rest_elements() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json =
            parse_source(root, path, "function method(x, ...args) {}\n").expect("parse succeeds");

        let params = &json["ast"]["program"]["body"][0]["params"];
        assert_eq!(params[0]["type"], "Identifier");
        assert_eq!(params[1]["type"], "RestElement");
        assert_eq!(params[1]["argument"]["name"], "args");
        assert_eq!(
            params[1]["typeAnnotation"]["typeAnnotation"]["type"],
            "TSArrayType"
        );
    }

    #[test]
    fn emits_array_literals_as_babel_array_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "const empty = [];\nconst values = [1, two, ...rest];\n",
        )
        .expect("parse succeeds");

        let empty = &json["ast"]["program"]["body"][0]["declarations"][0]["init"];
        assert_eq!(empty["type"], "ArrayExpression");
        assert_eq!(empty["elements"].as_array().unwrap().len(), 0);

        let values = &json["ast"]["program"]["body"][1]["declarations"][0]["init"];
        assert_eq!(values["type"], "ArrayExpression");
        assert_eq!(values["elements"][0]["type"], "NumericLiteral");
        assert_eq!(values["elements"][1]["type"], "Identifier");
        assert_eq!(values["elements"][1]["name"], "two");
        assert_eq!(values["elements"][2]["type"], "SpreadElement");
        assert_eq!(values["elements"][2]["argument"]["name"], "rest");
    }

    #[test]
    fn emits_object_literals_as_babel_object_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "const x = { key1: \"value\", key2: 2, [1 + 1]: value(), shorthand, ...rest };\n",
        )
        .expect("parse succeeds");

        let object = &json["ast"]["program"]["body"][0]["declarations"][0]["init"];
        assert_eq!(object["type"], "ObjectExpression");
        assert_eq!(object["properties"].as_array().unwrap().len(), 5);

        assert_eq!(object["properties"][0]["type"], "ObjectProperty");
        assert_eq!(object["properties"][0]["key"]["name"], "key1");
        assert_eq!(object["properties"][0]["value"]["type"], "StringLiteral");
        assert_eq!(object["properties"][0]["computed"], false);

        assert_eq!(object["properties"][2]["key"]["type"], "BinaryExpression");
        assert_eq!(object["properties"][2]["computed"], true);

        assert_eq!(object["properties"][3]["key"]["name"], "shorthand");
        assert_eq!(object["properties"][3]["value"]["name"], "shorthand");
        assert_eq!(object["properties"][3]["shorthand"], true);

        assert_eq!(object["properties"][4]["type"], "SpreadElement");
        assert_eq!(object["properties"][4]["argument"]["name"], "rest");
    }

    #[test]
    fn emits_object_methods_as_babel_object_methods() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "const x = { foo(arg) { return arg; }, [bar]() {} };\n",
        )
        .expect("parse succeeds");

        let object = &json["ast"]["program"]["body"][0]["declarations"][0]["init"];
        let plain = &object["properties"][0];
        assert_eq!(plain["type"], "ObjectMethod");
        assert_eq!(plain["kind"], "method");
        assert_eq!(plain["key"]["name"], "foo");
        assert_eq!(plain["params"][0]["name"], "arg");
        assert_eq!(plain["body"]["type"], "BlockStatement");
        assert_eq!(plain["computed"], false);

        let computed = &object["properties"][1];
        assert_eq!(computed["type"], "ObjectMethod");
        assert_eq!(computed["key"]["name"], "bar");
        assert_eq!(computed["computed"], true);
    }

    #[test]
    fn emits_if_statements_and_computed_member_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json =
            parse_source(root, path, "if (d = decorators[i]) foo();\n").expect("parse succeeds");

        let if_stmt = &json["ast"]["program"]["body"][0];
        assert_eq!(if_stmt["type"], "IfStatement");
        assert_eq!(if_stmt["test"]["type"], "AssignmentExpression");
        assert_eq!(if_stmt["test"]["right"]["type"], "MemberExpression");
        assert_eq!(if_stmt["test"]["right"]["computed"], true);
        assert_eq!(if_stmt["test"]["right"]["object"]["name"], "decorators");
        assert_eq!(if_stmt["test"]["right"]["property"]["name"], "i");
        assert_eq!(if_stmt["consequent"]["type"], "ExpressionStatement");
        assert_eq!(if_stmt["alternate"], Value::Null);
    }

    #[test]
    fn emits_ternaries_as_babel_conditional_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(root, path, "x ? y : z;\n").expect("parse succeeds");

        let expression = &json["ast"]["program"]["body"][0]["expression"];
        assert_eq!(expression["type"], "ConditionalExpression");
        assert_eq!(expression["test"]["name"], "x");
        assert_eq!(expression["consequent"]["name"], "y");
        assert_eq!(expression["alternate"]["name"], "z");
    }

    #[test]
    fn emits_loops_jumps_and_augmented_assignments() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "while (x < 1) { x += 1; break; }\ndo { continue loop1; } while (ok);\n",
        )
        .expect("parse succeeds");

        let while_stmt = &json["ast"]["program"]["body"][0];
        assert_eq!(while_stmt["type"], "WhileStatement");
        assert_eq!(while_stmt["test"]["type"], "BinaryExpression");
        let assignment = &while_stmt["body"]["body"][0]["expression"];
        assert_eq!(assignment["type"], "AssignmentExpression");
        assert_eq!(assignment["operator"], "+=");
        assert_eq!(while_stmt["body"]["body"][1]["type"], "BreakStatement");

        let do_stmt = &json["ast"]["program"]["body"][1];
        assert_eq!(do_stmt["type"], "DoWhileStatement");
        assert_eq!(do_stmt["test"]["name"], "ok");
        assert_eq!(do_stmt["body"]["body"][0]["type"], "ContinueStatement");
        assert_eq!(do_stmt["body"]["body"][0]["label"]["name"], "loop1");
    }

    #[test]
    fn emits_classic_for_loops_and_update_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "for (x = 0; x < 1; x++) { z += 1; }\nfor (;;) {}\n",
        )
        .expect("parse succeeds");

        let for_stmt = &json["ast"]["program"]["body"][0];
        assert_eq!(for_stmt["type"], "ForStatement");
        assert_eq!(for_stmt["init"]["type"], "AssignmentExpression");
        assert_eq!(for_stmt["test"]["type"], "BinaryExpression");
        assert_eq!(for_stmt["update"]["type"], "UpdateExpression");
        assert_eq!(for_stmt["update"]["operator"], "++");
        assert_eq!(for_stmt["update"]["prefix"], false);
        assert_eq!(for_stmt["body"]["body"][0]["expression"]["operator"], "+=");

        let empty_for = &json["ast"]["program"]["body"][1];
        assert_eq!(empty_for["type"], "ForStatement");
        assert_eq!(empty_for["init"], Value::Null);
        assert_eq!(empty_for["test"], Value::Null);
        assert_eq!(empty_for["update"], Value::Null);
    }

    #[test]
    fn emits_for_in_of_loops_and_destructuring_patterns() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "for (var i in arr) { foo(i); }\nfor (i of arr) { foo(i); }\nfor (var {a, b, c} of obj) { foo(a, b, c); }\nfor ([x, y] of arr) {}\n",
        )
        .expect("parse succeeds");

        let for_in = &json["ast"]["program"]["body"][0];
        assert_eq!(for_in["type"], "ForInStatement");
        assert_eq!(for_in["left"]["type"], "VariableDeclaration");
        assert_eq!(for_in["left"]["kind"], "var");
        assert_eq!(for_in["left"]["declarations"][0]["id"]["name"], "i");
        assert_eq!(for_in["left"]["declarations"][0]["init"], Value::Null);
        assert_eq!(for_in["right"]["name"], "arr");

        let for_of = &json["ast"]["program"]["body"][1];
        assert_eq!(for_of["type"], "ForOfStatement");
        assert_eq!(for_of["left"]["type"], "Identifier");
        assert_eq!(for_of["left"]["name"], "i");

        let object_pattern = &json["ast"]["program"]["body"][2]["left"]["declarations"][0]["id"];
        assert_eq!(object_pattern["type"], "ObjectPattern");
        assert_eq!(object_pattern["properties"].as_array().unwrap().len(), 3);
        assert_eq!(object_pattern["properties"][0]["type"], "ObjectProperty");
        assert_eq!(object_pattern["properties"][0]["key"]["name"], "a");
        assert_eq!(object_pattern["properties"][0]["value"]["name"], "a");
        assert_eq!(object_pattern["properties"][0]["shorthand"], true);

        let array_pattern = &json["ast"]["program"]["body"][3]["left"];
        assert_eq!(array_pattern["type"], "ArrayPattern");
        assert_eq!(array_pattern["elements"][0]["name"], "x");
        assert_eq!(array_pattern["elements"][1]["name"], "y");
    }

    #[test]
    fn emits_switch_labeled_and_this_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let source =
            "loop1: while (ok) { continue loop1; }\nswitch (x) { case 1: y; default: this.z; }\n";
        let json = parse_source(root, path, source).expect("parse succeeds");

        let labeled = &json["ast"]["program"]["body"][0];
        assert_eq!(labeled["type"], "LabeledStatement");
        assert_eq!(labeled["label"]["name"], "loop1");
        assert_eq!(labeled["body"]["type"], "WhileStatement");
        assert_eq!(labeled["body"]["body"]["body"][0]["label"]["name"], "loop1");

        let switch_stmt = &json["ast"]["program"]["body"][1];
        assert_eq!(switch_stmt["type"], "SwitchStatement");
        assert_eq!(switch_stmt["discriminant"]["name"], "x");
        assert_eq!(switch_stmt["cases"].as_array().unwrap().len(), 2);

        let case_label = &switch_stmt["cases"][0];
        assert_eq!(case_label["type"], "SwitchCase");
        assert_eq!(case_label["test"]["value"], 1.0);
        assert_eq!(case_label["consequent"][0]["expression"]["name"], "y");
        assert_eq!(
            &source[case_label["start"].as_u64().unwrap() as usize
                ..case_label["end"].as_u64().unwrap() as usize],
            "case 1:"
        );

        let default_label = &switch_stmt["cases"][1];
        assert_eq!(default_label["test"], Value::Null);
        assert_eq!(
            &source[default_label["start"].as_u64().unwrap() as usize
                ..default_label["end"].as_u64().unwrap() as usize],
            "default:"
        );
        assert_eq!(
            default_label["consequent"][0]["expression"]["object"]["type"],
            "ThisExpression"
        );
    }

    #[test]
    fn emits_with_statements() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(root, path, "with (foo()) { bar(); }\nwith (baz()) qux();\n")
            .expect("parse succeeds");

        let block_with = &json["ast"]["program"]["body"][0];
        assert_eq!(block_with["type"], "WithStatement");
        assert_eq!(block_with["object"]["type"], "CallExpression");
        assert_eq!(block_with["object"]["callee"]["name"], "foo");
        assert_eq!(block_with["body"]["type"], "BlockStatement");
        assert_eq!(
            block_with["body"]["body"][0]["expression"]["callee"]["name"],
            "bar"
        );

        let statement_with = &json["ast"]["program"]["body"][1];
        assert_eq!(statement_with["type"], "WithStatement");
        assert_eq!(statement_with["object"]["callee"]["name"], "baz");
        assert_eq!(statement_with["body"]["type"], "ExpressionStatement");
        assert_eq!(
            statement_with["body"]["expression"]["callee"]["name"],
            "qux"
        );
    }

    #[test]
    fn emits_try_catch_finally_and_throw_statements() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "try { open(); } catch (err) { throw err; } finally { close(); }\n",
        )
        .expect("parse succeeds");

        let try_stmt = &json["ast"]["program"]["body"][0];
        assert_eq!(try_stmt["type"], "TryStatement");
        assert_eq!(try_stmt["block"]["type"], "BlockStatement");
        assert_eq!(try_stmt["handler"]["type"], "CatchClause");
        assert_eq!(try_stmt["handler"]["param"]["name"], "err");
        assert_eq!(
            try_stmt["handler"]["body"]["body"][0]["type"],
            "ThrowStatement"
        );
        assert_eq!(try_stmt["finalizer"]["type"], "BlockStatement");
    }

    #[test]
    fn emits_arrow_functions_as_babel_arrow_function_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "const value = () => 42;\nconst id = x => { return x; };\n",
        )
        .expect("parse succeeds");

        let value = &json["ast"]["program"]["body"][0]["declarations"][0]["init"];
        assert_eq!(value["type"], "ArrowFunctionExpression");
        assert_eq!(value["id"], Value::Null);
        assert_eq!(value["params"].as_array().unwrap().len(), 0);
        assert_eq!(value["body"]["type"], "NumericLiteral");
        assert_eq!(value["expression"], true);

        let id = &json["ast"]["program"]["body"][1]["declarations"][0]["init"];
        assert_eq!(id["type"], "ArrowFunctionExpression");
        assert_eq!(id["params"][0]["name"], "x");
        assert_eq!(id["body"]["type"], "BlockStatement");
        assert_eq!(id["expression"], false);
    }

    #[test]
    fn emits_function_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "function method() { return function foo(x) { return x; }; }\n",
        )
        .expect("parse succeeds");

        let func = &json["ast"]["program"]["body"][0]["body"]["body"][0]["argument"];
        assert_eq!(func["type"], "FunctionExpression");
        assert_eq!(func["id"]["name"], "foo");
        assert_eq!(func["params"][0]["name"], "x");
        assert_eq!(func["body"]["body"][0]["argument"]["name"], "x");
    }

    #[test]
    fn emits_typescript_non_null_expressions_with_fallback_parser() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(root, path, "const foo = bar!\n").expect("parse succeeds");

        let init = &json["ast"]["program"]["body"][0]["declarations"][0]["init"];
        assert_eq!(init["type"], "TSNonNullExpression");
        assert_eq!(init["expression"]["name"], "bar");
        assert_eq!(init["start"], 12);
        assert_eq!(init["end"], 16);
        assert_eq!(init["expression"]["start"], 12);
        assert_eq!(init["expression"]["end"], 15);
    }

    #[test]
    fn emits_typescript_parameter_wrappers_as_plain_parameters() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.ts");
        let json = parse_source(
            root,
            path,
            "const obj = { [\"someNameComputation()\"](node: Node) { foo(node); } };\n",
        )
        .expect("parse succeeds");

        let method = &json["ast"]["program"]["body"][0]["declarations"][0]["init"]["properties"][0];
        assert_eq!(method["type"], "ObjectMethod");
        assert_eq!(method["computed"], true);
        assert_eq!(method["key"]["type"], "StringLiteral");
        assert_eq!(method["key"]["value"], "someNameComputation()");
        assert_eq!(method["params"][0]["type"], "Identifier");
        assert_eq!(method["params"][0]["name"], "node");
    }

    #[test]
    fn emits_template_literals_and_tagged_templates() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "foo(`Hello ${world}!`);\nx`a ${1+1} b`;\nString.raw`../${42}\\..`;\n",
        )
        .expect("parse succeeds");

        let template = &json["ast"]["program"]["body"][0]["expression"]["arguments"][0];
        assert_eq!(template["type"], "TemplateLiteral");
        assert_eq!(template["expressions"][0]["name"], "world");
        assert_eq!(template["quasis"][0]["type"], "TemplateElement");
        assert_eq!(template["quasis"][0]["value"]["raw"], "Hello ");
        assert_eq!(template["quasis"][0]["tail"], false);
        assert_eq!(template["quasis"][1]["value"]["raw"], "!");
        assert_eq!(template["quasis"][1]["tail"], true);

        let simple_tag = &json["ast"]["program"]["body"][1]["expression"];
        assert_eq!(simple_tag["type"], "TaggedTemplateExpression");
        assert_eq!(simple_tag["tag"]["name"], "x");
        assert_eq!(simple_tag["quasi"]["quasis"][0]["value"]["raw"], "a ");
        assert_eq!(simple_tag["quasi"]["expressions"][0]["operator"], "+");
        assert_eq!(simple_tag["quasi"]["quasis"][1]["value"]["raw"], " b");

        let member_tag = &json["ast"]["program"]["body"][2]["expression"];
        assert_eq!(member_tag["type"], "TaggedTemplateExpression");
        assert_eq!(member_tag["tag"]["type"], "MemberExpression");
        assert_eq!(member_tag["tag"]["property"]["name"], "raw");
        assert_eq!(member_tag["quasi"]["quasis"][0]["value"]["raw"], "../");
        assert_eq!(member_tag["quasi"]["quasis"][1]["value"]["raw"], "\\..");
    }

    #[test]
    fn emits_sequence_and_class_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json =
            parse_source(root, path, "let x = (class Foo {}, bar())\n").expect("parse succeeds");

        let init = &json["ast"]["program"]["body"][0]["declarations"][0]["init"];
        assert_eq!(init["type"], "SequenceExpression");
        assert_eq!(init["expressions"][0]["type"], "ClassExpression");
        assert_eq!(init["expressions"][0]["id"]["name"], "Foo");
        assert_eq!(init["expressions"][0]["body"]["type"], "ClassBody");
        assert_eq!(init["expressions"][1]["type"], "CallExpression");
        assert_eq!(init["expressions"][1]["callee"]["name"], "bar");
    }

    #[test]
    fn emits_constructor_calls_as_babel_new_expressions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json =
            parse_source(root, path, "var x = new MyClass(arg1, arg2)\n").expect("parse succeeds");

        let init = &json["ast"]["program"]["body"][0]["declarations"][0]["init"];
        assert_eq!(init["type"], "NewExpression");
        assert_eq!(init["callee"]["name"], "MyClass");
        assert_eq!(init["arguments"][0]["name"], "arg1");
        assert_eq!(init["arguments"][1]["name"], "arg2");
    }

    #[test]
    fn emits_import_export_and_ts_import_equals_declarations() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.ts");
        let json = parse_source(
            root,
            path,
            "import {x as y} from \"foo\";\nimport fs = require('fs');\nexport const getApiA = () => {};\n",
        )
        .expect("parse succeeds");

        let import_decl = &json["ast"]["program"]["body"][0];
        assert_eq!(import_decl["type"], "ImportDeclaration");
        assert_eq!(import_decl["source"]["value"], "foo");
        assert_eq!(import_decl["specifiers"][0]["type"], "ImportSpecifier");
        assert_eq!(import_decl["specifiers"][0]["imported"]["name"], "x");
        assert_eq!(import_decl["specifiers"][0]["local"]["name"], "y");

        let import_equals = &json["ast"]["program"]["body"][1];
        assert_eq!(import_equals["type"], "TSImportEqualsDeclaration");
        assert_eq!(import_equals["id"]["name"], "fs");
        assert_eq!(
            import_equals["moduleReference"]["type"],
            "TSExternalModuleReference"
        );
        assert_eq!(
            import_equals["moduleReference"]["expression"]["value"],
            "fs"
        );

        let export_decl = &json["ast"]["program"]["body"][2];
        assert_eq!(export_decl["type"], "ExportNamedDeclaration");
        assert_eq!(export_decl["declaration"]["type"], "VariableDeclaration");
        assert_eq!(
            export_decl["declaration"]["declarations"][0]["id"]["name"],
            "getApiA"
        );
    }

    #[test]
    fn emits_export_assignments_star_exports_and_default_from_exports() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/exports.js");
        let json = parse_source(
            root,
            path,
            "export = foo;\nexport = function func(param) {};\nexport = class ClassA {};\nexport { import1 as name1, name3 } from \"Foo\";\nexport bar from \"Bar\";\nexport * from \"Baz\";\nexport * as B from \"Qux\";\n",
        )
        .expect("parse succeeds");
        let body = json["ast"]["program"]["body"].as_array().unwrap();

        assert_eq!(body[0]["type"], "TSExportAssignment");
        assert_eq!(body[0]["expression"]["name"], "foo");
        assert_eq!(body[1]["type"], "TSExportAssignment");
        assert_eq!(body[1]["expression"]["type"], "FunctionExpression");
        assert_eq!(body[1]["expression"]["id"]["name"], "func");
        assert_eq!(body[2]["type"], "TSExportAssignment");
        assert_eq!(body[2]["expression"]["type"], "ClassExpression");
        assert_eq!(body[2]["expression"]["id"]["name"], "ClassA");

        assert_eq!(body[3]["type"], "ExportNamedDeclaration");
        assert_eq!(body[3]["source"]["value"], "Foo");
        assert_eq!(body[3]["specifiers"][0]["local"]["name"], "import1");
        assert_eq!(body[3]["specifiers"][0]["exported"]["name"], "name1");

        assert_eq!(body[4]["type"], "ExportNamedDeclaration");
        assert_eq!(body[4]["source"]["value"], "Bar");
        assert_eq!(body[4]["specifiers"][0]["local"]["name"], "bar");
        assert_eq!(body[4]["specifiers"][0]["exported"]["name"], "bar");

        assert_eq!(body[5]["type"], "ExportAllDeclaration");
        assert_eq!(body[5]["source"]["value"], "Baz");

        assert_eq!(body[6]["type"], "ExportNamedDeclaration");
        assert_eq!(body[6]["source"]["value"], "Qux");
        assert_eq!(body[6]["specifiers"][0]["type"], "ExportNamespaceSpecifier");
        assert_eq!(body[6]["specifiers"][0]["exported"]["name"], "B");
    }

    #[test]
    fn emits_utf16_offsets_for_non_ascii_prefixes() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/utf8.js");
        let json = parse_source(root, path, "// 😼\nlogger.error()\n").expect("parse succeeds");
        let property = &json["ast"]["program"]["body"][0]["expression"]["callee"]["property"];

        assert_eq!(property["name"], "error");
        assert_eq!(property["start"], 13);
        assert_eq!(property["end"], 18);
        assert_eq!(property["loc"]["start"]["column"], 7);
        assert_eq!(property["loc"]["end"]["column"], 12);
    }

    #[test]
    fn emits_file_and_program_spans_covering_leading_trivia() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/leading.js");
        let json = parse_source(root, path, "\n// A comment\nfoo();\n").expect("parse succeeds");

        assert_eq!(json["ast"]["start"], 0);
        assert_eq!(json["ast"]["program"]["start"], 0);
        assert_eq!(json["ast"]["end"], 21);
        assert_eq!(json["ast"]["program"]["end"], 21);
        assert_eq!(json["ast"]["program"]["body"][0]["start"], 14);
    }

    #[test]
    fn emits_ts_declare_function_modules_and_expression_wrappers() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.ts");
        let json = parse_source(
            root,
            path,
            "declare function foo(arg: string): string\nmodule M { export var [a, b] = [1, 2]; }\nasync function x(foo) { await foo(); }\ndelete foo.x;\nlet y = z satisfies T;\nlet u = req.user as UserDocument;\n",
        )
        .expect("parse succeeds");

        let declare_fn = &json["ast"]["program"]["body"][0];
        assert_eq!(declare_fn["type"], "TSDeclareFunction");
        assert_eq!(declare_fn["id"]["name"], "foo");
        assert_eq!(declare_fn["params"][0]["name"], "arg");
        assert_eq!(
            declare_fn["params"][0]["typeAnnotation"]["type"],
            "TSTypeAnnotation"
        );
        assert_eq!(
            declare_fn["params"][0]["typeAnnotation"]["typeAnnotation"]["type"],
            "TSStringKeyword"
        );
        assert_eq!(
            declare_fn["returnType"]["typeAnnotation"]["type"],
            "TSStringKeyword"
        );

        let module_decl = &json["ast"]["program"]["body"][1];
        assert_eq!(module_decl["type"], "TSModuleDeclaration");
        assert_eq!(module_decl["id"]["name"], "M");
        assert_eq!(module_decl["body"]["type"], "TSModuleBlock");
        assert_eq!(
            module_decl["body"]["body"][0]["type"],
            "ExportNamedDeclaration"
        );

        let await_expr = &json["ast"]["program"]["body"][2]["body"]["body"][0]["expression"];
        assert_eq!(await_expr["type"], "AwaitExpression");
        assert_eq!(await_expr["argument"]["type"], "CallExpression");

        let delete_expr = &json["ast"]["program"]["body"][3]["expression"];
        assert_eq!(delete_expr["type"], "UnaryExpression");
        assert_eq!(delete_expr["operator"], "delete");

        let satisfies_expr = &json["ast"]["program"]["body"][4]["declarations"][0]["init"];
        assert_eq!(satisfies_expr["type"], "TSSatisfiesExpression");
        assert_eq!(satisfies_expr["expression"]["name"], "z");

        let as_expr = &json["ast"]["program"]["body"][5]["declarations"][0]["init"];
        assert_eq!(as_expr["type"], "TSAsExpression");
        assert_eq!(as_expr["expression"]["type"], "MemberExpression");
    }

    #[test]
    fn emits_class_members_ts_declarations_and_namespaces() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.ts");
        let json = parse_source(
            root,
            path,
            "abstract class Foo extends Base { static a: string; #b: number; public abstract run(): void; }\ninterface I { x: string; (value: string): boolean; }\nenum E { A = 1, B }\ntype User = { name: string; tags: string[]; };\nnamespace A.B { class C {} }\n",
        )
        .expect("parse succeeds");

        let class_decl = &json["ast"]["program"]["body"][0];
        assert_eq!(class_decl["type"], "ClassDeclaration");
        assert_eq!(class_decl["abstract"], true);
        assert_eq!(class_decl["superClass"]["name"], "Base");
        assert_eq!(class_decl["body"]["body"][0]["type"], "ClassProperty");
        assert_eq!(class_decl["body"]["body"][0]["static"], true);
        assert_eq!(
            class_decl["body"]["body"][1]["type"],
            "ClassPrivateProperty"
        );
        assert_eq!(class_decl["body"]["body"][1]["key"]["id"]["name"], "b");
        assert_eq!(class_decl["body"]["body"][2]["type"], "TSDeclareMethod");
        assert_eq!(class_decl["body"]["body"][2]["abstract"], true);

        let interface_decl = &json["ast"]["program"]["body"][1];
        assert_eq!(interface_decl["type"], "TSInterfaceDeclaration");
        assert_eq!(
            interface_decl["body"]["body"][0]["type"],
            "TSPropertySignature"
        );
        assert_eq!(
            interface_decl["body"]["body"][1]["type"],
            "TSCallSignatureDeclaration"
        );

        let enum_decl = &json["ast"]["program"]["body"][2];
        assert_eq!(enum_decl["type"], "TSEnumDeclaration");
        assert_eq!(enum_decl["members"][0]["initializer"]["value"], 1.0);
        assert_eq!(enum_decl["members"][1]["id"]["name"], "B");

        let alias_decl = &json["ast"]["program"]["body"][3];
        assert_eq!(alias_decl["type"], "TSTypeAliasDeclaration");
        assert_eq!(alias_decl["typeAnnotation"]["type"], "TSTypeLiteral");
        assert_eq!(
            alias_decl["typeAnnotation"]["members"][1]["key"]["name"],
            "tags"
        );
        assert_eq!(
            alias_decl["typeAnnotation"]["members"][1]["typeAnnotation"]["typeAnnotation"]["type"],
            "TSArrayType"
        );

        let namespace_a = &json["ast"]["program"]["body"][4];
        assert_eq!(namespace_a["type"], "TSModuleDeclaration");
        assert_eq!(namespace_a["id"]["name"], "A");
        assert_eq!(namespace_a["body"]["type"], "TSModuleDeclaration");
        assert_eq!(namespace_a["body"]["id"]["name"], "B");
    }

    #[test]
    fn emits_jsx_expression_containers_and_array_holes() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.jsx");
        let json = parse_source(
            root,
            path,
            "const View = ({ value }) => <Button onClick={() => use(value)}>TRY</Button>;\nvar [a, , b] = x;\n",
        )
        .expect("parse succeeds");

        let jsx = &json["ast"]["program"]["body"][0]["declarations"][0]["init"]["body"];
        assert_eq!(jsx["type"], "JSXElement");
        assert_eq!(jsx["openingElement"]["type"], "JSXOpeningElement");
        let on_click = &jsx["openingElement"]["attributes"][0];
        assert_eq!(on_click["type"], "JSXAttribute");
        assert_eq!(on_click["name"]["name"], "onClick");
        assert_eq!(on_click["value"]["type"], "JSXExpressionContainer");
        assert_eq!(
            on_click["value"]["expression"]["type"],
            "ArrowFunctionExpression"
        );
        assert_eq!(
            on_click["value"]["expression"]["body"]["callee"]["name"],
            "use"
        );

        let pattern = &json["ast"]["program"]["body"][1]["declarations"][0]["id"];
        assert_eq!(pattern["type"], "ArrayPattern");
        assert_eq!(pattern["elements"][0]["name"], "a");
        assert_eq!(pattern["elements"][1], Value::Null);
        assert_eq!(pattern["elements"][2]["name"], "b");
    }

    #[test]
    fn emits_vue_templates_and_script_blocks_with_original_offsets() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/App.vue");
        let source = "<template>\n  <h1>{{ msg }}</h1><img :src=\"image.url\" v-bind:alt=\"image.description\" />\n</template>\n<script lang=\"ts\">\nimport { Component, Prop, Vue } from 'vue-property-decorator';\n@Component\nexport default class HelloWorld extends Vue {\n  @Prop() private msg!: string;\n}\n</script>\n<style>h1 { color: red; }</style>\n";
        let json = parse_source(root, path, source).expect("parse succeeds");
        let body = json["ast"]["program"]["body"].as_array().unwrap();

        assert_eq!(body[0]["type"], "ExpressionStatement");
        assert_eq!(body[0]["expression"]["type"], "JSXElement");
        assert_eq!(body[1]["type"], "ImportDeclaration");
        assert_eq!(body[2]["type"], "ExportDefaultDeclaration");

        let template_children = body[0]["expression"]["children"].as_array().unwrap();
        assert!(template_children
            .iter()
            .any(|child| child["type"] == "JSXText"));
        let h1 = template_children
            .iter()
            .find(|child| child["openingElement"]["name"]["name"] == "h1")
            .unwrap();
        let moustache = h1["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|child| child["type"] == "JSXExpressionContainer")
            .unwrap();
        assert_eq!(moustache["type"], "JSXExpressionContainer");
        assert_eq!(
            &source[moustache["start"].as_u64().unwrap() as usize
                ..moustache["end"].as_u64().unwrap() as usize],
            "{{ msg }}"
        );
        assert_eq!(moustache["expression"]["name"], "msg");

        let img = template_children
            .iter()
            .find(|child| child["openingElement"]["name"]["name"] == "img")
            .unwrap();
        let shorthand_attr = &img["openingElement"]["attributes"][0];
        assert_eq!(shorthand_attr["type"], "JSXAttribute");
        assert_eq!(shorthand_attr["name"]["name"], "src");
        assert_eq!(
            &source[shorthand_attr["start"].as_u64().unwrap() as usize
                ..shorthand_attr["end"].as_u64().unwrap() as usize],
            "src=\"image.url\""
        );

        let class_decl = &body[2]["declaration"];
        assert_eq!(class_decl["type"], "ClassDeclaration");
        assert_eq!(class_decl["decorators"][0]["type"], "Decorator");
        assert_eq!(
            class_decl["decorators"][0]["expression"]["name"],
            "Component"
        );
        assert_eq!(
            class_decl["body"]["body"][0]["decorators"][0]["expression"]["callee"]["name"],
            "Prop"
        );
    }

    #[test]
    fn emits_decorators_for_classes_members_methods_and_parameters() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/decor.ts");
        let json = parse_source(
            root,
            path,
            "@a(false)\nclass Greeter {\n  @b(foo)\n  greeting: string;\n  @c()\n  greet(@d(foo=false) x: number) {}\n}\n",
        )
        .expect("parse succeeds");

        let class_decl = &json["ast"]["program"]["body"][0];
        assert_eq!(
            class_decl["decorators"][0]["expression"]["callee"]["name"],
            "a"
        );

        let property = &class_decl["body"]["body"][0];
        assert_eq!(
            property["decorators"][0]["expression"]["callee"]["name"],
            "b"
        );

        let method = &class_decl["body"]["body"][1];
        assert_eq!(method["decorators"][0]["expression"]["callee"]["name"], "c");
        assert_eq!(
            method["params"][0]["decorators"][0]["expression"]["callee"]["name"],
            "d"
        );
    }

    #[test]
    fn decodes_string_literal_values_like_babel() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/app.js");
        let json = parse_source(
            root,
            path,
            "let a = \"\\\"abc\";\nlet b = 'abc\\'';\nlet c = `abc\ndef\n`;\n",
        )
        .expect("parse succeeds");

        assert_eq!(
            json["ast"]["program"]["body"][0]["declarations"][0]["init"]["value"],
            "\"abc"
        );
        assert_eq!(
            json["ast"]["program"]["body"][1]["declarations"][0]["init"]["value"],
            "abc'"
        );
        assert_eq!(
            json["ast"]["program"]["body"][2]["declarations"][0]["init"]["value"],
            "abc\ndef\n"
        );
    }

    #[test]
    fn emits_type_map_for_typescript_inference_and_cross_file_calls() {
        let root = Path::new("/repo");
        let main_path = Path::new("/repo/main.ts");
        let dep_path = Path::new("/repo/dep.ts");
        let main_source = "const foo = () => 42;\nlet x = \"test\";\nvar y = x;\nimport * as deps from \"./dep\";\nvar z = new deps.Foo().bar();\n";
        let dep_source = "export class Foo { bar() { return \"bar\"; } }\n";
        let main_json = parse_source(root, main_path, main_source).expect("parse succeeds");
        let dep_json = parse_source(root, dep_path, dep_source).expect("parse succeeds");
        let files = vec![
            (
                main_path.to_path_buf(),
                main_json.clone(),
                main_source.to_string(),
            ),
            (dep_path.to_path_buf(), dep_json, dep_source.to_string()),
        ];

        let project = TypeMapProject::from_parsed_files(&files);
        let type_map = project.infer_type_map(&main_json, main_source);
        let body = main_json["ast"]["program"]["body"].as_array().unwrap();
        let foo_declarator = &body[0]["declarations"][0];
        let arrow_function = &foo_declarator["init"];
        let y_declarator = &body[2]["declarations"][0];
        let z_declarator = &body[4]["declarations"][0];

        assert_eq!(
            type_map
                .get(&test_range(foo_declarator))
                .map(String::as_str),
            Some("() => number")
        );
        assert_eq!(
            type_map
                .get(&test_range(arrow_function))
                .map(String::as_str),
            Some("number")
        );
        assert_eq!(
            type_map.get(&test_range(y_declarator)).map(String::as_str),
            Some("string")
        );
        assert_eq!(
            type_map.get(&test_range(z_declarator)).map(String::as_str),
            Some("string")
        );
    }

    #[test]
    fn emits_type_annotations_for_ts_angle_bracket_assertions() {
        let root = Path::new("/repo");
        let path = Path::new("/repo/cast.ts");
        let json = parse_source(
            root,
            path,
            "let imgScr: string = <string>this.imageElement;\n(<HTMLImageElement>this.imageElement).src = imgScr;\n",
        )
        .expect("parse succeeds");
        let body = json["ast"]["program"]["body"].as_array().unwrap();
        let string_assertion = &body[0]["declarations"][0]["init"];
        let html_assertion = &body[1]["expression"]["left"]["object"];

        assert_eq!(
            string_assertion["typeAnnotation"]["type"],
            "TSStringKeyword"
        );
        assert_eq!(html_assertion["typeAnnotation"]["type"], "TSTypeReference");
        assert_eq!(
            html_assertion["typeAnnotation"]["typeName"]["name"],
            "HTMLImageElement"
        );
    }

    fn test_range(node: &Value) -> String {
        format!(
            "{}:{}",
            node["start"].as_u64().unwrap(),
            node["end"].as_u64().unwrap()
        )
    }
}
