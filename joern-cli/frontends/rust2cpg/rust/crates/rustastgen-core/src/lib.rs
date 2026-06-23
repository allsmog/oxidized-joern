mod hir;

use anyhow::{Context, Result};
use hir::HirResolver;
use ra_ap_syntax::{
    AstNode, Edition, NodeOrToken, SourceFile, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken,
    TextRange,
};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub fn parse_file(input_root: &Path, file: &Path) -> Result<Value> {
    parse_file_with_sysroot(input_root, file, false)
}

pub fn parse_file_with_sysroot(input_root: &Path, file: &Path, sysroot: bool) -> Result<Value> {
    let content =
        fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))?;
    parse_source_with_sysroot(input_root, file, &content, sysroot)
}

pub fn parse_source(input_root: &Path, file: &Path, content: &str) -> Result<Value> {
    parse_source_with_sysroot(input_root, file, content, false)
}

pub fn parse_source_with_sysroot(
    input_root: &Path,
    file: &Path,
    content: &str,
    sysroot: bool,
) -> Result<Value> {
    let parse = SourceFile::parse(content, Edition::CURRENT);
    let tree = parse.tree();
    let root = tree.syntax();
    let line_index = LineIndex::new(content);
    let relative_file_path = relative_file_path(input_root, file);
    let full_file_path = full_file_path(file);
    let crate_name = crate_name_for(input_root, file);
    let module_path = module_path_for(Path::new(&relative_file_path));
    let semantic = SemanticModel::new(root, crate_name.as_deref(), sysroot);
    // Real HIR-backed resolution is gated behind the sysroot flag (the same flag
    // that enables std resolution in the heuristic). It only *fills* fields the
    // heuristic left empty, so existing output is preserved; if it cannot load
    // (no on-disk file, no sysroot, internal panic) it is simply absent.
    let hir = sysroot
        .then(|| HirResolver::try_build(file, crate_name.as_deref()))
        .flatten();

    let mut doc = Map::new();
    doc.insert("relativeFilePath".into(), Value::String(relative_file_path));
    doc.insert("fullFilePath".into(), Value::String(full_file_path));
    doc.insert("content".into(), Value::String(content.into()));
    if let Some(crate_name) = crate_name {
        doc.insert("crateName".into(), Value::String(crate_name));
    }
    if let Some(module_path) = module_path {
        doc.insert("modulePath".into(), Value::String(module_path));
    }
    doc.insert("loc".into(), json!(line_count(content)));
    doc.insert(
        "children".into(),
        Value::Array(vec![node_to_json(
            root,
            &line_index,
            &semantic,
            hir.as_ref(),
        )]),
    );
    Ok(Value::Object(doc))
}

pub fn write_json(target: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let data = serde_json::to_vec_pretty(value)?;
    fs::write(target, data).with_context(|| format!("failed to write {}", target.display()))
}

fn node_to_json(
    node: &SyntaxNode,
    line_index: &LineIndex,
    semantic: &SemanticModel,
    hir: Option<&HirResolver>,
) -> Value {
    let mut obj = base_object(kind_name(node.kind()), node.text_range(), line_index);
    let children: Vec<_> = node
        .children_with_tokens()
        .filter_map(|child| element_to_json(child, line_index, semantic, hir))
        .collect();

    if !children.is_empty() {
        obj.insert("children".into(), Value::Array(children));
    }
    enrich_node(node, &mut obj, semantic);
    enrich_node_with_hir(node, &mut obj, hir);
    Value::Object(obj)
}

fn token_to_json(token: &SyntaxToken, line_index: &LineIndex) -> Option<Value> {
    if should_skip_kind(token.kind()) {
        return None;
    }
    let mut obj = base_object(kind_name(token.kind()), token.text_range(), line_index);
    obj.insert("text".into(), Value::String(token.text().into()));
    Some(Value::Object(obj))
}

fn element_to_json(
    element: SyntaxElement,
    line_index: &LineIndex,
    semantic: &SemanticModel,
    hir: Option<&HirResolver>,
) -> Option<Value> {
    match element {
        NodeOrToken::Node(node) => {
            if should_skip_kind(node.kind()) {
                None
            } else {
                Some(node_to_json(&node, line_index, semantic, hir))
            }
        }
        NodeOrToken::Token(token) => token_to_json(&token, line_index),
    }
}

fn base_object(
    node_kind: String,
    range: ra_ap_syntax::TextRange,
    line_index: &LineIndex,
) -> Map<String, Value> {
    let start = offset_to_usize(range.start());
    let end = offset_to_usize(range.end());
    let (start_line, start_column) = line_index.line_col(start);
    let mut obj = Map::new();
    obj.insert("nodeKind".into(), Value::String(node_kind));
    obj.insert(
        "range".into(),
        json!({
            "startOffset": start,
            "endOffset": end,
            "startLine": start_line,
            "startColumn": start_column
        }),
    );
    obj
}

fn enrich_node(node: &SyntaxNode, obj: &mut Map<String, Value>, semantic: &SemanticModel) {
    match node.kind() {
        SyntaxKind::IDENT_PAT => {
            if let Some(type_full_name) = type_for_ident_pat(node, semantic) {
                obj.insert("typeFullName".into(), Value::String(type_full_name));
            }
        }
        SyntaxKind::NAME_REF => {
            if let Some(type_full_name) = type_for_name_ref(node, semantic) {
                obj.insert("typeFullName".into(), Value::String(type_full_name));
            }
        }
        SyntaxKind::LITERAL => {
            if let Some(type_full_name) =
                literal_context_type(node, semantic).or_else(|| literal_type(node))
            {
                obj.insert("typeFullName".into(), Value::String(type_full_name));
            }
        }
        SyntaxKind::BIN_EXPR
        | SyntaxKind::CALL_EXPR
        | SyntaxKind::FIELD_EXPR
        | SyntaxKind::INDEX_EXPR
        | SyntaxKind::METHOD_CALL_EXPR
        | SyntaxKind::PATH_EXPR
        | SyntaxKind::PREFIX_EXPR
        | SyntaxKind::RECORD_EXPR => {
            if let Some(type_full_name) = expr_type(node, semantic) {
                obj.insert("typeFullName".into(), Value::String(type_full_name));
            }
            if matches!(
                node.kind(),
                SyntaxKind::CALL_EXPR | SyntaxKind::METHOD_CALL_EXPR
            ) && let Some(method_full_name) = callable_method_full_name(node, semantic)
            {
                obj.insert("methodFullName".into(), Value::String(method_full_name));
            }
        }
        SyntaxKind::TUPLE_EXPR => {
            if let Some(type_full_name) = tuple_type(node, semantic) {
                obj.insert("typeFullName".into(), Value::String(type_full_name));
            }
        }
        SyntaxKind::ARRAY_EXPR => {
            if let Some(type_full_name) = array_type(node, semantic) {
                obj.insert("typeFullName".into(), Value::String(type_full_name));
            }
        }
        _ => {}
    }
}

/// Fill `typeFullName`/`methodFullName` from real HIR resolution, but only where
/// the heuristic ([`enrich_node`]) produced nothing. The heuristic stays the
/// source of truth for the cases it already covers; HIR extends coverage to
/// generics, trait methods, and standard-library APIs the heuristic cannot
/// resolve. Only the node kinds the heuristic annotates are considered, so the
/// set of nodes that may carry these fields is unchanged.
fn enrich_node_with_hir(
    node: &SyntaxNode,
    obj: &mut Map<String, Value>,
    hir: Option<&HirResolver>,
) {
    let Some(hir) = hir else {
        return;
    };
    let range = node.text_range();
    if enriches_type(node.kind())
        && !obj.contains_key("typeFullName")
        && let Some(type_full_name) = hir.type_full_name(range)
    {
        obj.insert(
            "typeFullName".into(),
            Value::String(type_full_name.to_string()),
        );
    }
    if matches!(
        node.kind(),
        SyntaxKind::CALL_EXPR | SyntaxKind::METHOD_CALL_EXPR
    ) && !obj.contains_key("methodFullName")
        && let Some(method_full_name) = hir.method_full_name(range)
    {
        obj.insert(
            "methodFullName".into(),
            Value::String(method_full_name.to_string()),
        );
    }
}

/// Node kinds for which the heuristic emits a `typeFullName`; HIR only fills
/// these to keep the emitted-field shape identical to the heuristic-only output.
fn enriches_type(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IDENT_PAT
            | SyntaxKind::NAME_REF
            | SyntaxKind::LITERAL
            | SyntaxKind::BIN_EXPR
            | SyntaxKind::CALL_EXPR
            | SyntaxKind::FIELD_EXPR
            | SyntaxKind::INDEX_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::PATH_EXPR
            | SyntaxKind::PREFIX_EXPR
            | SyntaxKind::RECORD_EXPR
            | SyntaxKind::TUPLE_EXPR
            | SyntaxKind::ARRAY_EXPR
    )
}

fn type_for_ident_pat(ident_pat: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let parent = ident_pat.parent()?;
    match parent.kind() {
        SyntaxKind::LET_STMT | SyntaxKind::PARAM => type_node(&parent, semantic)
            .or_else(|| initializer_expr(&parent).and_then(|expr| expr_type(&expr, semantic)))
            .or_else(|| later_assignment_type(ident_pat, &parent, semantic)),
        _ => None,
    }
}

fn type_for_name_ref(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if is_type_name_ref(node) {
        return node
            .ancestors()
            .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_TYPE)
            .and_then(|path_type| type_text(&path_type, semantic))
            .or_else(|| Some(semantic.qualify_type_name(&node.text().to_string())));
    }
    path_expr_name_ref_type(node, semantic)
}

fn is_type_name_ref(node: &SyntaxNode) -> bool {
    node.ancestors()
        .skip(1)
        .any(|ancestor| matches!(ancestor.kind(), SyntaxKind::PATH_TYPE))
}

fn type_node(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    node.children()
        .find(|child| is_type_kind(child.kind()))
        .and_then(|child| type_text(&child, semantic))
}

fn initializer_expr(node: &SyntaxNode) -> Option<SyntaxNode> {
    let mut seen_eq = false;
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::EQ => seen_eq = true,
            NodeOrToken::Node(child_node) if seen_eq && is_expr_kind(child_node.kind()) => {
                return Some(child_node);
            }
            _ => {}
        }
    }
    None
}

fn later_assignment_type(
    ident_pat: &SyntaxNode,
    let_stmt: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    let name = ident_name(ident_pat)?;
    let stmt_list = let_stmt
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::STMT_LIST)?;
    let let_end = let_stmt.text_range().end();

    stmt_list
        .children()
        .filter(|child| child.text_range().start() >= let_end)
        .find_map(|stmt| assignment_type_for_name(&stmt, &name, semantic))
}

fn assignment_type_for_name(
    stmt: &SyntaxNode,
    name: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    let bin_expr = if stmt.kind() == SyntaxKind::BIN_EXPR {
        stmt.clone()
    } else {
        stmt.descendants()
            .find(|child| child.kind() == SyntaxKind::BIN_EXPR)?
    };
    if !bin_expr
        .children_with_tokens()
        .any(|child| matches!(child, NodeOrToken::Token(token) if token.kind() == SyntaxKind::EQ))
    {
        return None;
    }
    let exprs: Vec<_> = bin_expr
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .collect();
    match exprs.as_slice() {
        [lhs, rhs, ..] if path_expr_name(lhs).as_deref() == Some(name) => expr_type(rhs, semantic),
        _ => None,
    }
}

fn ident_name(ident_pat: &SyntaxNode) -> Option<String> {
    ident_pat
        .descendants_with_tokens()
        .filter_map(|element| match element {
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::IDENT => {
                Some(token.text().to_string())
            }
            _ => None,
        })
        .next()
}

fn path_expr_name(path_expr: &SyntaxNode) -> Option<String> {
    if path_expr.kind() != SyntaxKind::PATH_EXPR {
        return None;
    }
    path_expr
        .descendants()
        .find(|child| child.kind() == SyntaxKind::NAME_REF)
        .map(|node| node.text().to_string())
}

fn literal_context_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if is_array_repeat_count(node) {
        return Some("usize".into());
    }
    for ancestor in node.ancestors().skip(1) {
        if !matches!(ancestor.kind(), SyntaxKind::LET_STMT | SyntaxKind::CONST) {
            continue;
        }
        let Some(initializer) = initializer_expr(&ancestor) else {
            continue;
        };
        if range_contains(initializer.text_range(), node.text_range()) {
            return type_node(&ancestor, semantic);
        }
    }
    None
}

fn range_contains(outer: ra_ap_syntax::TextRange, inner: ra_ap_syntax::TextRange) -> bool {
    outer.start() <= inner.start() && inner.end() <= outer.end()
}

fn expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    match node.kind() {
        SyntaxKind::LITERAL => literal_context_type(node, semantic).or_else(|| literal_type(node)),
        SyntaxKind::TUPLE_EXPR => tuple_type(node, semantic),
        SyntaxKind::ARRAY_EXPR => array_type(node, semantic),
        SyntaxKind::PAREN_EXPR => {
            first_expr_child(node).and_then(|expr| expr_type(&expr, semantic))
        }
        SyntaxKind::PREFIX_EXPR => prefix_expr_type(node, semantic),
        SyntaxKind::CAST_EXPR => type_node(node, semantic),
        SyntaxKind::BLOCK_EXPR => tail_expr(node).and_then(|expr| expr_type(&expr, semantic)),
        SyntaxKind::PATH_EXPR => {
            path_expr_name(node).and_then(|name| semantic.resolve_var(node, &name))
        }
        SyntaxKind::RECORD_EXPR => record_expr_type(node, semantic),
        SyntaxKind::FIELD_EXPR => field_expr_type(node, semantic),
        SyntaxKind::INDEX_EXPR => index_expr_type(node, semantic),
        SyntaxKind::BIN_EXPR => bin_expr_type(node, semantic),
        SyntaxKind::CALL_EXPR => call_expr_type(node, semantic),
        SyntaxKind::METHOD_CALL_EXPR => method_call_expr_type(node, semantic),
        SyntaxKind::MACRO_EXPR => macro_expr_type(node, semantic),
        _ => None,
    }
}

fn first_expr_child(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.children().find(|child| is_expr_kind(child.kind()))
}

fn tail_expr(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.descendants()
        .find(|child| child.kind() == SyntaxKind::STMT_LIST)
        .and_then(|stmt_list| {
            stmt_list
                .children()
                .filter(|child| is_expr_kind(child.kind()))
                .last()
        })
}

fn literal_type(node: &SyntaxNode) -> Option<String> {
    node.children_with_tokens().find_map(|child| match child {
        NodeOrToken::Token(token) => match token.kind() {
            SyntaxKind::INT_NUMBER => Some(infer_int_type(token.text())),
            SyntaxKind::FLOAT_NUMBER => Some(infer_float_type(token.text())),
            SyntaxKind::STRING => Some("&str".into()),
            SyntaxKind::BYTE_STRING => Some(format!("&[u8; {}]", byte_string_len(token.text()))),
            SyntaxKind::C_STRING => Some("&CStr".into()),
            SyntaxKind::CHAR => Some("char".into()),
            SyntaxKind::BYTE => Some("u8".into()),
            SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW => Some("bool".into()),
            _ => None,
        },
        NodeOrToken::Node(_) => None,
    })
}

fn infer_int_type(text: &str) -> String {
    const SUFFIXES: &[&str] = &[
        "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize",
    ];
    match SUFFIXES.iter().find(|suffix| text.ends_with(**suffix)) {
        Some(suffix) => (*suffix).into(),
        None => "i32".into(),
    }
}

fn infer_float_type(text: &str) -> String {
    if text.ends_with("f32") {
        "f32".into()
    } else {
        "f64".into()
    }
}

fn byte_string_len(text: &str) -> usize {
    text.strip_prefix("b\"")
        .and_then(|value| value.strip_suffix('"'))
        .map(str::len)
        .unwrap_or(0)
}

fn tuple_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let exprs: Vec<_> = node
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .collect();
    if exprs.is_empty() {
        Some("()".into())
    } else {
        let types: Vec<_> = exprs
            .iter()
            .map(|expr| expr_type(expr, semantic).unwrap_or_else(|| "ANY".into()))
            .collect();
        Some(format!("({})", types.join(", ")))
    }
}

fn array_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if let Some(declared) = literal_context_type(node, semantic) {
        return Some(declared);
    }
    let exprs: Vec<_> = node
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .collect();
    let element_type = exprs
        .first()
        .and_then(|expr| expr_type(expr, semantic))
        .unwrap_or_else(|| "ANY".into());
    if has_direct_token(node, SyntaxKind::SEMICOLON) {
        let count = exprs
            .get(1)
            .map(|expr| expr.text().to_string())
            .unwrap_or_else(|| "0".into());
        Some(format!("[{}; {}]", element_type, count))
    } else {
        Some(format!("[{}; {}]", element_type, exprs.len()))
    }
}

#[derive(Default)]
struct SemanticModel {
    crate_name: Option<String>,
    sysroot: bool,
    /// Resolved binding types keyed by the binding node's range so that two
    /// bindings with the same name (shadowing) never collide.
    variables: HashMap<TextRange, String>,
    structs: HashMap<String, StructInfo>,
    functions: HashMap<String, String>,
    /// Inherent/trait methods keyed by the Self type's full name.
    impls: HashMap<String, ImplInfo>,
}

#[derive(Default)]
struct StructInfo {
    full_name: String,
    fields: HashMap<String, String>,
    tuple_fields: Vec<String>,
}

/// Inherent/trait methods for a single Self type, keyed in the model by the
/// Self type's full name. Method name -> declared return type (`None` when the
/// method has no explicit `-> T`).
#[derive(Default)]
struct ImplInfo {
    methods: HashMap<String, Option<String>>,
}

impl SemanticModel {
    fn new(root: &SyntaxNode, crate_name: Option<&str>, sysroot: bool) -> Self {
        let mut model = Self {
            crate_name: crate_name.map(ToOwned::to_owned),
            sysroot,
            ..Self::default()
        };
        model.collect_structs(root);
        model.collect_functions(root);
        model.collect_impls(root);
        model.collect_variables(root);
        model
    }

    fn collect_structs(&mut self, root: &SyntaxNode) {
        for node in root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::STRUCT)
        {
            let Some(name) = name_child_text(&node) else {
                continue;
            };
            let full_name = self.qualify_value_name(&name);
            let mut info = StructInfo {
                full_name: full_name.clone(),
                ..StructInfo::default()
            };
            if let Some(record_fields) = direct_child(&node, SyntaxKind::RECORD_FIELD_LIST) {
                for field in record_fields
                    .children()
                    .filter(|child| child.kind() == SyntaxKind::RECORD_FIELD)
                {
                    if let (Some(field_name), Some(field_type)) =
                        (name_child_text(&field), type_node(&field, self))
                    {
                        info.fields.insert(field_name, field_type);
                    }
                }
            }
            if let Some(tuple_fields) = direct_child(&node, SyntaxKind::TUPLE_FIELD_LIST) {
                for field in tuple_fields
                    .children()
                    .filter(|child| child.kind() == SyntaxKind::TUPLE_FIELD)
                {
                    if let Some(field_type) = type_node(&field, self) {
                        info.tuple_fields.push(field_type);
                    }
                }
            }
            self.structs.insert(name.clone(), info.clone());
            self.structs.insert(full_name, info);
        }
    }

    fn collect_functions(&mut self, root: &SyntaxNode) {
        for node in root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::FN)
        {
            if let Some(name) = name_child_text(&node) {
                self.functions
                    .insert(name.clone(), self.qualify_value_name(&name));
            }
        }
    }

    fn collect_variables(&mut self, root: &SyntaxNode) {
        for node in root.descendants() {
            match node.kind() {
                SyntaxKind::PARAM => {
                    if let (Some(pat), Some(typ)) = (
                        direct_child(&node, SyntaxKind::IDENT_PAT),
                        type_node(&node, self),
                    ) {
                        self.variables.insert(pat.text_range(), typ);
                    }
                }
                SyntaxKind::LET_STMT => {
                    if let Some(pat) = direct_child(&node, SyntaxKind::IDENT_PAT) {
                        let typ = type_node(&node, self)
                            .or_else(|| {
                                initializer_expr(&node).and_then(|expr| expr_type(&expr, self))
                            })
                            .or_else(|| later_assignment_type(&pat, &node, self));
                        if let Some(typ) = typ {
                            self.variables.insert(pat.text_range(), typ);
                        }
                    }
                }
                SyntaxKind::CONST => {
                    if let (Some(name), Some(typ)) = (
                        direct_child(&node, SyntaxKind::NAME),
                        type_node(&node, self),
                    ) {
                        self.variables.insert(name.text_range(), typ);
                    }
                }
                _ => {}
            }
        }
    }

    fn collect_impls(&mut self, root: &SyntaxNode) {
        for node in root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::IMPL)
        {
            // For `impl Foo` and `impl Trait for Foo` alike the Self type is the
            // last type child (it follows the optional `for`).
            let Some(self_type) = node
                .children()
                .filter(|child| is_type_kind(child.kind()))
                .last()
                .and_then(|child| type_text(&child, self))
            else {
                continue;
            };
            let Some(items) = direct_child(&node, SyntaxKind::ASSOC_ITEM_LIST) else {
                continue;
            };
            let mut methods = HashMap::new();
            for func in items
                .children()
                .filter(|child| child.kind() == SyntaxKind::FN)
            {
                if let Some(name) = name_child_text(&func) {
                    let return_type = direct_child(&func, SyntaxKind::RET_TYPE)
                        .and_then(|ret| type_node(&ret, self));
                    methods.insert(name, return_type);
                }
            }
            self.impls
                .entry(self_type)
                .or_default()
                .methods
                .extend(methods);
        }
    }

    /// Resolve a variable reference to the type of its nearest enclosing
    /// binding, respecting lexical scope and shadowing. Walks the use site's
    /// ancestors from innermost outward; within each scope the latest binding
    /// that precedes (or, for params, encloses) the use wins.
    fn resolve_var(&self, use_site: &SyntaxNode, name: &str) -> Option<String> {
        let use_start = use_site.text_range().start();
        for scope in use_site.ancestors() {
            let mut best: Option<(TextRange, &String)> = None;
            for binding in scope_bindings(&scope, name) {
                // A `let`/`const` binding only comes into scope after its whole
                // declaration statement (so `let x = x + 1;` reads the outer
                // `x`); params, declared in the PARAM_LIST, precede the body.
                let is_param = binding
                    .parent()
                    .is_some_and(|parent| parent.kind() == SyntaxKind::PARAM);
                let visible_from = if is_param {
                    binding.text_range().start()
                } else {
                    binding
                        .parent()
                        .map(|stmt| stmt.text_range().end())
                        .unwrap_or_else(|| binding.text_range().end())
                };
                if visible_from > use_start {
                    continue;
                }
                if let Some(typ) = self.variables.get(&binding.text_range()) {
                    match best {
                        Some((range, _)) if range.start() >= binding.text_range().start() => {}
                        _ => best = Some((binding.text_range(), typ)),
                    }
                }
            }
            if let Some((_, typ)) = best {
                return Some(typ.clone());
            }
        }
        None
    }

    fn qualify_type_name(&self, name: &str) -> String {
        if is_builtin_type_name(name) || name.contains("::") {
            name.into()
        } else if self.sysroot && name == "String" {
            "alloc::string::String".into()
        } else if self.structs.contains_key(name) {
            self.qualify_value_name(name)
        } else {
            name.into()
        }
    }

    fn qualify_value_name(&self, name: &str) -> String {
        self.crate_name
            .as_ref()
            .map(|crate_name| format!("{crate_name}::{name}"))
            .unwrap_or_else(|| name.into())
    }
}

impl Clone for StructInfo {
    fn clone(&self) -> Self {
        Self {
            full_name: self.full_name.clone(),
            fields: self.fields.clone(),
            tuple_fields: self.tuple_fields.clone(),
        }
    }
}

fn type_text(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    match node.kind() {
        SyntaxKind::PATH_TYPE => path_type_text(node, semantic),
        SyntaxKind::REF_TYPE => {
            let inner = node
                .children()
                .find(|child| is_type_kind(child.kind()))
                .and_then(|child| type_text(&child, semantic))?;
            let mutability = if has_direct_token(node, SyntaxKind::MUT_KW) {
                "mut "
            } else {
                ""
            };
            Some(format!("&{mutability}{inner}"))
        }
        SyntaxKind::PTR_TYPE => {
            let inner = node
                .children()
                .find(|child| is_type_kind(child.kind()))
                .and_then(|child| type_text(&child, semantic))?;
            let qualifier = if has_direct_token(node, SyntaxKind::CONST_KW) {
                "const "
            } else {
                "mut "
            };
            Some(format!("*{qualifier}{inner}"))
        }
        SyntaxKind::SLICE_TYPE => {
            let inner = node
                .children()
                .find(|child| is_type_kind(child.kind()))
                .and_then(|child| type_text(&child, semantic))?;
            Some(format!("[{inner}]"))
        }
        SyntaxKind::ARRAY_TYPE => {
            let inner = node
                .children()
                .find(|child| is_type_kind(child.kind()))
                .and_then(|child| type_text(&child, semantic))?;
            let count = node
                .children()
                .find(|child| child.kind() == SyntaxKind::CONST_ARG)
                .map(|child| child.text().to_string())
                .unwrap_or_default();
            Some(format!("[{inner}; {count}]"))
        }
        SyntaxKind::TUPLE_TYPE => {
            let types: Vec<_> = node
                .children()
                .filter(|child| is_type_kind(child.kind()))
                .filter_map(|child| type_text(&child, semantic))
                .collect();
            Some(format!("({})", types.join(", ")))
        }
        SyntaxKind::PAREN_TYPE => node
            .children()
            .find(|child| is_type_kind(child.kind()))
            .and_then(|child| type_text(&child, semantic)),
        SyntaxKind::NEVER_TYPE => Some("!".into()),
        _ => Some(node.text().to_string()),
    }
}

fn path_expr_name_ref_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let path_expr = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_EXPR)?;
    if path_expr
        .descendants()
        .filter(|child| child.kind() == SyntaxKind::NAME_REF)
        .last()
        .as_ref()
        .is_some_and(|last| last == node)
    {
        path_expr_name(&path_expr)
            .and_then(|name| semantic.resolve_var(&path_expr, &name))
            .filter(|typ| semantic.sysroot || !is_unresolved_generic_container(typ))
    } else {
        None
    }
}

fn prefix_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let operand = first_expr_child(node)?;
    if has_direct_token(node, SyntaxKind::BANG) {
        Some("bool".into())
    } else if has_direct_token(node, SyntaxKind::STAR) {
        expr_type(&operand, semantic).and_then(|typ| {
            typ.strip_prefix("*const ")
                .or_else(|| typ.strip_prefix("*mut "))
                .map(str::to_string)
        })
    } else {
        expr_type(&operand, semantic)
    }
}

fn record_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    direct_child(node, SyntaxKind::PATH)
        .and_then(|path| {
            path.descendants()
                .find(|child| child.kind() == SyntaxKind::NAME_REF)
        })
        .map(|name| semantic.qualify_type_name(&name.text().to_string()))
}

fn field_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let base = first_expr_child(node)?;
    let base_type = expr_type(&base, semantic)?;
    let field_name = direct_child(node, SyntaxKind::NAME_REF)?.text().to_string();
    if let Some(tuple_type) = tuple_field_type(&base_type, &field_name) {
        return Some(tuple_type);
    }
    let info = semantic.structs.get(&base_type)?;
    info.fields.get(&field_name).cloned().or_else(|| {
        field_name
            .parse::<usize>()
            .ok()
            .and_then(|idx| info.tuple_fields.get(idx).cloned())
    })
}

fn index_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if let Some(generic_type) = generic_index_expr_type(node, semantic) {
        return Some(generic_type);
    }
    let base = node.children().find(|child| is_expr_kind(child.kind()))?;
    expr_type(&base, semantic).and_then(|typ| array_element_type(&typ))
}

fn bin_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if has_any_direct_token(
        node,
        &[
            SyntaxKind::EQ2,
            SyntaxKind::NEQ,
            SyntaxKind::L_ANGLE,
            SyntaxKind::R_ANGLE,
            SyntaxKind::LTEQ,
            SyntaxKind::GTEQ,
            SyntaxKind::AMP2,
            SyntaxKind::PIPE2,
        ],
    ) {
        return Some("bool".into());
    }
    node.children()
        .find(|child| is_expr_kind(child.kind()))
        .and_then(|child| expr_type(&child, semantic))
}

fn call_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let names = call_path_names(node);
    // Return type of a user-defined associated function, e.g. `Point::new()`.
    if let Some([type_name, method]) = names.as_deref()
        && let Some(ret) = user_assoc_fn_return_type(type_name, method, semantic)
    {
        return Some(ret);
    }
    if semantic.sysroot {
        if path_matches(names.as_deref(), &["String", "from"]) {
            return Some("&str".into());
        }
        if path_matches(names.as_deref(), &["String", "new"])
            || path_matches(names.as_deref(), &["String", "with_capacity"])
        {
            return Some("alloc::string::String".into());
        }
    }
    call_name(node).and_then(|name| {
        semantic
            .functions
            .get(&name)
            .and_then(|_| function_return_type(node, &name, semantic))
    })
}

fn function_return_type(
    root_call: &SyntaxNode,
    name: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    let root = root_call.ancestors().last()?;
    root.descendants()
        .find(|node| {
            node.kind() == SyntaxKind::FN && name_child_text(node).as_deref() == Some(name)
        })
        .and_then(|node| direct_child(&node, SyntaxKind::RET_TYPE))
        .and_then(|ret| type_node(&ret, semantic))
}

fn callable_method_full_name(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    match node.kind() {
        SyntaxKind::CALL_EXPR => call_method_full_name(node, semantic),
        SyntaxKind::METHOD_CALL_EXPR => method_call_method_full_name(node, semantic),
        _ => None,
    }
}

fn call_method_full_name(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let names = call_path_names(node);
    // User-defined associated function, e.g. `Point::new(..)`.
    if let Some([type_name, method]) = names.as_deref()
        && let Some(full_name) = user_assoc_fn_full_name(type_name, method, semantic)
    {
        return Some(full_name);
    }
    if semantic.sysroot {
        if path_matches(names.as_deref(), &["String", "from"]) {
            return Some("core::convert::From<T>::from".into());
        }
        if path_matches(names.as_deref(), &["String", "new"]) {
            return Some("alloc::string::String::new".into());
        }
        if path_matches(names.as_deref(), &["String", "with_capacity"]) {
            return Some("alloc::string::String::with_capacity".into());
        }
        if path_matches(names.as_deref(), &["Option", "Some"]) {
            return Some("core::option::Option::Some".into());
        }
        if path_matches(names.as_deref(), &["Option", "None"]) {
            return Some("core::option::Option::None".into());
        }
        if path_matches(names.as_deref(), &["Result", "Ok"]) {
            return Some("core::result::Result::Ok".into());
        }
        if path_matches(names.as_deref(), &["Result", "Err"]) {
            return Some("core::result::Result::Err".into());
        }
    }
    call_name(node).and_then(|name| semantic.functions.get(&name).cloned())
}

/// `methodFullName` for a call to an associated function of a user-defined
/// type, e.g. `Point::new` -> `crate::Point::new`.
fn user_assoc_fn_full_name(
    type_name: &str,
    method: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    let qualified = semantic.qualify_type_name(type_name);
    let info = semantic.impls.get(&qualified)?;
    info.methods
        .contains_key(method)
        .then(|| format!("{qualified}::{method}"))
}

/// Declared return type of a user-defined associated function, with `Self`
/// resolved to the owning type's full name.
fn user_assoc_fn_return_type(
    type_name: &str,
    method: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    let qualified = semantic.qualify_type_name(type_name);
    let ret = semantic
        .impls
        .get(&qualified)?
        .methods
        .get(method)?
        .clone()?;
    Some(if ret == "Self" { qualified } else { ret })
}

fn call_name(node: &SyntaxNode) -> Option<String> {
    node.children()
        .find(|child| is_expr_kind(child.kind()))
        .and_then(|expr| path_expr_name(&expr))
}

fn call_path_names(node: &SyntaxNode) -> Option<Vec<String>> {
    node.children()
        .find(|child| is_expr_kind(child.kind()))
        .and_then(|expr| path_name_refs(&expr))
}

fn path_name_refs(path_expr: &SyntaxNode) -> Option<Vec<String>> {
    if path_expr.kind() != SyntaxKind::PATH_EXPR {
        return None;
    }
    Some(
        path_expr
            .descendants()
            .filter(|child| child.kind() == SyntaxKind::NAME_REF)
            .map(|node| node.text().to_string())
            .collect(),
    )
}

fn path_matches(actual: Option<&[String]>, expected: &[&str]) -> bool {
    actual.is_some_and(|actual| {
        actual.len() == expected.len()
            && actual
                .iter()
                .zip(expected.iter())
                .all(|(actual, expected)| actual == expected)
    })
}

fn method_call_method_full_name(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let method_name = direct_child(node, SyntaxKind::NAME_REF)?.text().to_string();
    let receiver = first_expr_child(node)?;
    let receiver_type = expr_type(&receiver, semantic);
    // User-defined methods resolve regardless of sysroot.
    if let Some(typ) = receiver_type.as_deref()
        && let Some(full_name) = user_method_full_name(typ, &method_name, semantic)
    {
        return Some(full_name);
    }
    if !semantic.sysroot {
        return None;
    }
    match (method_name.as_str(), receiver_type.as_deref()) {
        ("push", Some(typ)) if typ.starts_with("alloc::vec::Vec<") => {
            Some("alloc::vec::Vec<T, A>::push".into())
        }
        ("len", Some(typ)) if typ.starts_with("alloc::vec::Vec<") => {
            Some("alloc::vec::Vec<T, A>::len".into())
        }
        ("trim", Some(typ)) if is_string_like(typ) => Some("str::trim".into()),
        ("len", Some(typ)) if is_string_like(typ) => Some("str::len".into()),
        ("as_str", Some(typ)) if is_owned_string(typ) => {
            Some("alloc::string::String::as_str".into())
        }
        ("push_str", Some(typ)) if is_owned_string(typ) => {
            Some("alloc::string::String::push_str".into())
        }
        ("to_string", Some(typ)) if is_string_like(typ) => {
            Some("<T as alloc::string::ToString>::to_string".into())
        }
        _ => None,
    }
}

fn method_call_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let method_name = direct_child(node, SyntaxKind::NAME_REF)?.text().to_string();
    let receiver = first_expr_child(node)?;
    let receiver_type = expr_type(&receiver, semantic);
    // Return type of a user-defined method.
    if let Some(typ) = receiver_type.as_deref()
        && let Some(ret) = user_method_return_type(typ, &method_name, semantic)
    {
        return Some(ret);
    }
    if !semantic.sysroot {
        return None;
    }
    match (method_name.as_str(), receiver_type.as_deref()) {
        ("push" | "push_str", _) => Some("()".into()),
        ("trim", _) => Some("&str".into()),
        ("as_str", Some(typ)) if is_owned_string(typ) => Some("&str".into()),
        ("to_string", _) => Some("alloc::string::String".into()),
        ("len", _) => Some("usize".into()),
        _ => None,
    }
}

/// `methodFullName` for a method call whose receiver resolves to a user-defined
/// type recorded in an `impl` block.
fn user_method_full_name(
    receiver_type: &str,
    method_name: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    let self_type = receiver_type.trim_start_matches(['&', ' ']);
    let self_type = self_type.strip_prefix("mut ").unwrap_or(self_type);
    let info = semantic.impls.get(self_type)?;
    info.methods
        .contains_key(method_name)
        .then(|| format!("{self_type}::{method_name}"))
}

fn user_method_return_type(
    receiver_type: &str,
    method_name: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    let self_type = receiver_type.trim_start_matches(['&', ' ']);
    let self_type = self_type.strip_prefix("mut ").unwrap_or(self_type);
    let ret = semantic
        .impls
        .get(self_type)?
        .methods
        .get(method_name)?
        .clone()?;
    Some(if ret == "Self" {
        self_type.to_string()
    } else {
        ret
    })
}

/// `&str`, `String`, or `alloc::string::String`.
fn is_string_like(typ: &str) -> bool {
    typ == "&str" || is_owned_string(typ)
}

fn is_owned_string(typ: &str) -> bool {
    typ == "String" || typ == "alloc::string::String"
}

fn macro_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if semantic.sysroot && node.text().to_string().starts_with("vec![") {
        Some("alloc::vec::Vec<i32, alloc::alloc::Global>".into())
    } else {
        None
    }
}

fn is_array_repeat_count(node: &SyntaxNode) -> bool {
    let Some(array) = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::ARRAY_EXPR)
    else {
        return false;
    };
    if !has_direct_token(&array, SyntaxKind::SEMICOLON) {
        return false;
    }
    array
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .nth(1)
        .is_some_and(|count| range_contains(count.text_range(), node.text_range()))
}

fn direct_child(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    node.children().find(|child| child.kind() == kind)
}

fn name_child_text(node: &SyntaxNode) -> Option<String> {
    direct_child(node, SyntaxKind::NAME).map(|name| name.text().to_string())
}

/// Binding nodes (IDENT_PAT for `let`/params, NAME for `const`) that declare
/// `name` directly within `scope`. A binding belongs to a scope when its
/// declaration statement is an immediate child of that scope, so nested blocks
/// (which appear later as their own ancestor) are not double-counted.
fn scope_bindings<'a>(
    scope: &'a SyntaxNode,
    name: &'a str,
) -> impl Iterator<Item = SyntaxNode> + 'a {
    scope
        .children()
        .filter_map(move |child| match child.kind() {
            SyntaxKind::LET_STMT => direct_child(&child, SyntaxKind::IDENT_PAT)
                .filter(|pat| ident_name(pat).as_deref() == Some(name)),
            SyntaxKind::CONST => {
                direct_child(&child, SyntaxKind::NAME).filter(|n| n.text() == name)
            }
            SyntaxKind::PARAM_LIST => child.children().find_map(|param| {
                direct_child(&param, SyntaxKind::IDENT_PAT)
                    .filter(|pat| ident_name(pat).as_deref() == Some(name))
            }),
            _ => None,
        })
}

fn has_direct_token(node: &SyntaxNode, kind: SyntaxKind) -> bool {
    node.children_with_tokens()
        .any(|child| matches!(child, NodeOrToken::Token(token) if token.kind() == kind))
}

fn has_any_direct_token(node: &SyntaxNode, kinds: &[SyntaxKind]) -> bool {
    node.children_with_tokens().any(|child| {
        matches!(child, NodeOrToken::Token(token) if kinds.iter().any(|kind| *kind == token.kind()))
    })
}

fn tuple_field_type(tuple_type: &str, field_name: &str) -> Option<String> {
    let index = field_name.parse::<usize>().ok()?;
    let inner = tuple_type.strip_prefix('(')?.strip_suffix(')')?;
    inner
        .split(',')
        .map(str::trim)
        .nth(index)
        .map(str::to_string)
}

fn array_element_type(array_type: &str) -> Option<String> {
    let inner = array_type.strip_prefix('[')?;
    let inner = inner.strip_suffix(']')?;
    let element = inner.split(';').next()?.trim();
    Some(element.into())
}

fn path_type_text(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if direct_child(node, SyntaxKind::PATH).is_some_and(|path| {
        path.descendants()
            .any(|child| child.kind() == SyntaxKind::GENERIC_ARG_LIST)
    }) {
        let normalized = normalize_type_text(&node.text().to_string(), semantic);
        if semantic.sysroot && normalized.starts_with("Vec<") {
            return Some(format!("alloc::vec::{}", normalized));
        }
        return Some(normalized);
    }
    node.descendants()
        .find(|child| child.kind() == SyntaxKind::NAME_REF)
        .map(|name| semantic.qualify_type_name(&name.text().to_string()))
}

fn normalize_type_text(text: &str, semantic: &SemanticModel) -> String {
    let compact: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
    semantic
        .structs
        .keys()
        .filter(|name| !name.contains("::"))
        .fold(compact, |acc, name| {
            acc.replace(name, &semantic.qualify_type_name(name))
        })
}

fn is_unresolved_generic_container(typ: &str) -> bool {
    typ.starts_with("Vec<") || typ == "Vec"
}

fn generic_index_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let root_name = index_root_name(node)?;
    let mut typ = semantic.resolve_var(node, &root_name)?;
    for _ in 0..index_depth(node) {
        if let Some(inner) = vec_element_type(&typ) {
            typ = inner;
        } else if let Some(inner) = array_element_type(&typ) {
            typ = inner;
        } else {
            return None;
        }
    }
    (!is_unresolved_generic_container(&typ)).then_some(typ)
}

fn index_root_name(node: &SyntaxNode) -> Option<String> {
    let mut current = node.clone();
    loop {
        let base = current
            .children()
            .find(|child| is_expr_kind(child.kind()))?;
        match base.kind() {
            SyntaxKind::PATH_EXPR => return path_expr_name(&base),
            SyntaxKind::INDEX_EXPR => current = base,
            _ => return None,
        }
    }
}

fn index_depth(node: &SyntaxNode) -> usize {
    let mut depth = 0;
    let mut current = node.clone();
    loop {
        if current.kind() != SyntaxKind::INDEX_EXPR {
            return depth;
        }
        depth += 1;
        let Some(base) = current.children().find(|child| is_expr_kind(child.kind())) else {
            return depth;
        };
        current = base;
    }
}

fn vec_element_type(typ: &str) -> Option<String> {
    let inner = typ.strip_prefix("Vec<")?.strip_suffix('>')?;
    Some(inner.into())
}

fn is_builtin_type_name(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "char"
            | "f32"
            | "f64"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "str"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "()"
            | "!"
    )
}

fn is_expr_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ARRAY_EXPR
            | SyntaxKind::ASM_EXPR
            | SyntaxKind::AWAIT_EXPR
            | SyntaxKind::BIN_EXPR
            | SyntaxKind::BLOCK_EXPR
            | SyntaxKind::BREAK_EXPR
            | SyntaxKind::CALL_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::CLOSURE_EXPR
            | SyntaxKind::CONTINUE_EXPR
            | SyntaxKind::FIELD_EXPR
            | SyntaxKind::FOR_EXPR
            | SyntaxKind::FORMAT_ARGS_EXPR
            | SyntaxKind::IF_EXPR
            | SyntaxKind::INDEX_EXPR
            | SyntaxKind::LET_EXPR
            | SyntaxKind::LITERAL
            | SyntaxKind::LOOP_EXPR
            | SyntaxKind::MACRO_EXPR
            | SyntaxKind::MATCH_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::OFFSET_OF_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::PATH_EXPR
            | SyntaxKind::PREFIX_EXPR
            | SyntaxKind::RANGE_EXPR
            | SyntaxKind::RECORD_EXPR
            | SyntaxKind::REF_EXPR
            | SyntaxKind::RETURN_EXPR
            | SyntaxKind::TRY_EXPR
            | SyntaxKind::TUPLE_EXPR
            | SyntaxKind::UNDERSCORE_EXPR
            | SyntaxKind::WHILE_EXPR
            | SyntaxKind::YIELD_EXPR
    )
}

fn is_type_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ARRAY_TYPE
            | SyntaxKind::DYN_TRAIT_TYPE
            | SyntaxKind::FN_PTR_TYPE
            | SyntaxKind::FOR_TYPE
            | SyntaxKind::IMPL_TRAIT_TYPE
            | SyntaxKind::INFER_TYPE
            | SyntaxKind::MACRO_TYPE
            | SyntaxKind::NEVER_TYPE
            | SyntaxKind::PAREN_TYPE
            | SyntaxKind::PATH_TYPE
            | SyntaxKind::PTR_TYPE
            | SyntaxKind::REF_TYPE
            | SyntaxKind::SLICE_TYPE
            | SyntaxKind::TUPLE_TYPE
    )
}

fn should_skip_kind(kind: SyntaxKind) -> bool {
    kind.is_trivia() || matches!(kind, SyntaxKind::ERROR)
}

fn kind_name(kind: SyntaxKind) -> String {
    format!("{kind:?}")
}

fn offset_to_usize(offset: ra_ap_syntax::TextSize) -> usize {
    let raw: u32 = offset.into();
    raw as usize
}

fn relative_file_path(input_root: &Path, file: &Path) -> String {
    let relative = file
        .strip_prefix(input_root)
        .ok()
        .or_else(|| file.file_name().map(Path::new))
        .unwrap_or(file);
    path_to_slash_string(relative)
}

fn full_file_path(file: &Path) -> String {
    file.canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn path_to_slash_string(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn crate_name_for(input_root: &Path, file: &Path) -> Option<String> {
    let mut current = if file.is_dir() {
        file.to_path_buf()
    } else {
        file.parent().unwrap_or(file).to_path_buf()
    };

    loop {
        if !current.starts_with(input_root) {
            return None;
        }
        let cargo_toml = current.join("Cargo.toml");
        if cargo_toml.is_file() {
            return parse_package_name(&cargo_toml);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn parse_package_name(cargo_toml: &Path) -> Option<String> {
    let contents = fs::read_to_string(cargo_toml).ok()?;
    let mut in_package = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if in_package && trimmed.starts_with("name") {
            let (_, raw) = trimmed.split_once('=')?;
            return Some(raw.trim().trim_matches('"').to_string());
        }
    }
    None
}

fn module_path_for(relative_file: &Path) -> Option<String> {
    let mut parts: Vec<String> = relative_file
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if parts.first().map(String::as_str) == Some("src") {
        parts.remove(0);
    }
    let last = parts.last_mut()?;
    if last == "lib.rs" || last == "main.rs" || last == "mod.rs" {
        parts.pop();
    } else if let Some(stripped) = last.strip_suffix(".rs") {
        *last = stripped.to_string();
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("::"))
    }
}

fn line_count(content: &str) -> usize {
    content.lines().count().max(1)
}

struct LineIndex {
    line_starts: Vec<usize>,
}

impl LineIndex {
    fn new(content: &str) -> Self {
        let mut line_starts = vec![0];
        for (idx, byte) in content.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(idx + 1);
            }
        }
        Self { line_starts }
    }

    fn line_col(&self, offset: usize) -> (usize, usize) {
        let line = match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(next) => next.saturating_sub(1),
        };
        (line, offset.saturating_sub(self.line_starts[line]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn emits_source_file_and_fn_shape() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"
"#,
        )
        .unwrap();
        let file = root.join("src/lib.rs");
        fs::write(&file, "fn main(x: i32) -> i32 { x }\n").unwrap();

        let json = parse_file(root, &file).unwrap();
        assert_eq!(json["relativeFilePath"], "src/lib.rs");
        assert_eq!(json["crateName"], "demo");
        assert_eq!(json["children"][0]["nodeKind"], "SOURCE_FILE");
        assert!(contains_kind(&json["children"][0], "FN"));
        assert!(contains_kind(&json["children"][0], "PARAM_LIST"));
        assert!(contains_kind(&json["children"][0], "RET_TYPE"));
    }

    #[test]
    fn infers_untyped_ident_pat_from_literal_initializer() {
        let file = PathBuf::from("/tmp/src/lib.rs");
        let json = parse_source(
            Path::new("/tmp"),
            &file,
            "fn main() {\n  let s = \"hello\";\n  let n = 1;\n}\n",
        )
        .unwrap();
        let root = &json["children"][0];
        let ident_pats = collect_kind(root, "IDENT_PAT");
        assert_eq!(ident_pats.len(), 2);
        assert_eq!(ident_pats[0]["typeFullName"], "&str");
        assert_eq!(ident_pats[1]["typeFullName"], "i32");
    }

    #[test]
    fn emits_type_full_name_on_path_type_leaf_name_refs() {
        let file = PathBuf::from("/tmp/src/lib.rs");
        let json = parse_source(
            Path::new("/tmp"),
            &file,
            "const MAX_SIZE: usize = 1024;\nfn id(x: i32) -> i32 { x }\n",
        )
        .unwrap();
        let name_refs = collect_kind(&json["children"][0], "NAME_REF");
        let typed_refs: Vec<_> = name_refs
            .into_iter()
            .filter_map(|node| node.get("typeFullName").and_then(|value| value.as_str()))
            .collect();
        assert!(typed_refs.contains(&"usize"));
        assert!(typed_refs.contains(&"i32"));
    }

    #[test]
    fn applies_declared_type_to_initializer_literals() {
        let file = PathBuf::from("/tmp/src/lib.rs");
        let json = parse_source(
            Path::new("/tmp"),
            &file,
            "fn main() { let x: usize = 10; }\n",
        )
        .unwrap();
        let literals = collect_kind(&json["children"][0], "LITERAL");
        assert_eq!(literals[0]["typeFullName"], "usize");
    }

    #[test]
    fn infers_let_type_from_later_simple_assignment() {
        let file = PathBuf::from("/tmp/src/lib.rs");
        let json = parse_source(Path::new("/tmp"), &file, "fn main() { let x; x = 5; }\n").unwrap();
        let ident_pats = collect_kind(&json["children"][0], "IDENT_PAT");
        assert_eq!(ident_pats[0]["typeFullName"], "i32");
    }

    #[test]
    fn inner_let_shadows_outer_for_variable_reference() {
        let file = PathBuf::from("/tmp/src/lib.rs");
        // The outer `x` is `&str`; the inner block rebinds `x` to `i32`. The
        // reference inside the inner block must resolve to the inner binding,
        // while the reference after the block resolves to the outer one.
        let json = parse_source(
            Path::new("/tmp"),
            &file,
            "fn main() {\n  let x = \"hello\";\n  {\n    let x = 1;\n    let y = x;\n  }\n  let z = x;\n}\n",
        )
        .unwrap();
        let root = &json["children"][0];
        let ident_pats = collect_kind(root, "IDENT_PAT");
        // Order: outer x (&str), inner x (i32), y (from inner x), z (from outer x).
        assert_eq!(ident_pats[0]["typeFullName"], "&str");
        assert_eq!(ident_pats[1]["typeFullName"], "i32");
        assert_eq!(ident_pats[2]["typeFullName"], "i32");
        assert_eq!(ident_pats[3]["typeFullName"], "&str");
    }

    #[test]
    fn resolves_string_new_against_sysroot() {
        let file = PathBuf::from("/tmp/src/lib.rs");
        let json = parse_source_with_sysroot(
            Path::new("/tmp"),
            &file,
            "fn main() { let s = String::new(); }\n",
            true,
        )
        .unwrap();
        let root = &json["children"][0];
        let call = &collect_kind(root, "CALL_EXPR")[0];
        assert_eq!(call["typeFullName"], "alloc::string::String");
        assert_eq!(call["methodFullName"], "alloc::string::String::new");
    }

    #[test]
    fn resolves_string_push_str_against_sysroot() {
        let file = PathBuf::from("/tmp/src/lib.rs");
        let json = parse_source_with_sysroot(
            Path::new("/tmp"),
            &file,
            "fn main() { let mut s = String::new(); s.push_str(\"x\"); }\n",
            true,
        )
        .unwrap();
        let root = &json["children"][0];
        let call = &collect_kind(root, "METHOD_CALL_EXPR")[0];
        assert_eq!(call["methodFullName"], "alloc::string::String::push_str");
        assert_eq!(call["typeFullName"], "()");
    }

    #[test]
    fn resolves_user_defined_method_and_return_type() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"
"#,
        )
        .unwrap();
        let file = root.join("src/lib.rs");
        fs::write(
            &file,
            "struct Point { x: i32 }\n\
             impl Point {\n\
             \x20   fn new() -> Self { Point { x: 0 } }\n\
             \x20   fn value(&self) -> i32 { self.x }\n\
             }\n\
             fn main() {\n\
             \x20   let p = Point::new();\n\
             \x20   let v = p.value();\n\
             }\n",
        )
        .unwrap();

        let json = parse_file(root, &file).unwrap();
        let source = &json["children"][0];
        let calls = collect_kind(source, "CALL_EXPR");
        // `Point::new()` -> associated fn returning `Self`.
        let new_call = calls
            .iter()
            .find(|call| call["methodFullName"] == "demo::Point::new")
            .expect("Point::new should resolve");
        assert_eq!(new_call["typeFullName"], "demo::Point");
        // `p.value()` -> instance method returning `i32`.
        let value_call = &collect_kind(source, "METHOD_CALL_EXPR")[0];
        assert_eq!(value_call["methodFullName"], "demo::Point::value");
        assert_eq!(value_call["typeFullName"], "i32");
    }

    /// Write `source` into a fresh on-disk cargo crate named `crate_name` and
    /// run the generator with the sysroot (HIR) path enabled, returning the
    /// SOURCE_FILE node. HIR needs a real file plus a discoverable sysroot, so
    /// these tests are inherently filesystem/toolchain bound.
    fn parse_hir_crate(crate_name: &str, source: &str) -> (tempfile::TempDir, Value) {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
            ),
        )
        .unwrap();
        let file = root.join("src/lib.rs");
        fs::write(&file, source).unwrap();
        let json = parse_file_with_sysroot(root, &file, true).unwrap();
        let source_file = json["children"][0].clone();
        (dir, source_file)
    }

    fn text_of(value: &Value) -> String {
        let mut out = String::new();
        collect_text(value, &mut out);
        out
    }

    fn collect_text(value: &Value, out: &mut String) {
        if let Some(text) = value.get("text").and_then(Value::as_str) {
            out.push_str(text);
        }
        if let Some(children) = value["children"].as_array() {
            for child in children {
                collect_text(child, out);
            }
        }
    }

    /// First node of `kind` whose reconstructed source text contains `needle`.
    fn find_kind_containing<'a>(value: &'a Value, kind: &str, needle: &str) -> Option<&'a Value> {
        collect_kind(value, kind)
            .into_iter()
            .find(|node| text_of(node).contains(needle))
    }

    fn contains_kind(value: &Value, kind: &str) -> bool {
        value["nodeKind"] == kind
            || value["children"]
                .as_array()
                .is_some_and(|children| children.iter().any(|child| contains_kind(child, kind)))
    }

    fn collect_kind<'a>(value: &'a Value, kind: &str) -> Vec<&'a Value> {
        let mut out = Vec::new();
        if value["nodeKind"] == kind {
            out.push(value);
        }
        if let Some(children) = value["children"].as_array() {
            for child in children {
                out.extend(collect_kind(child, kind));
            }
        }
        out
    }

    // --- HIR-backed resolution -------------------------------------------------
    //
    // The tests below prove the rust-analyzer HIR path resolves cases the
    // hand-rolled `SemanticModel` cannot: a generic function's monomorphized
    // return type, a trait method's canonical callable, `HashMap`/`Option`
    // methods (absent from the heuristic's hard-coded table), and the inferred
    // element type of `Vec::new()`. Each requires a discoverable sysroot.

    const HIR_FIXTURE: &str = r#"use std::collections::HashMap;

trait Greeter {
    fn greet(&self) -> i32;
}

struct Robot;

impl Greeter for Robot {
    fn greet(&self) -> i32 { 42 }
}

fn identity<T>(value: T) -> T { value }

fn exercise() {
    let g = identity(7i64);
    let mut v = Vec::new();
    v.push(1u8);
    let mut m: HashMap<i32, i32> = HashMap::new();
    let got = m.get(&1);
    let opt = Some(3i16);
    let inner = opt.unwrap();
    let r = Robot;
    let greeting = r.greet();
}
"#;

    #[test]
    fn hir_resolves_generic_return_type() {
        let (_dir, source) = parse_hir_crate("hirdemo", HIR_FIXTURE);
        // `opt.unwrap()` on `Option<i16>` returns the monomorphized `i16`. The
        // heuristic has no entry for `Option::unwrap`, so it emits nothing here;
        // HIR resolves both the callable and the generic return type, and the
        // inferred type flows to the `inner` binding.
        let unwrap_call = find_kind_containing(&source, "METHOD_CALL_EXPR", "opt.unwrap()")
            .expect("unwrap call present");
        assert_eq!(
            unwrap_call["methodFullName"],
            "core::option::Option::unwrap"
        );
        assert_eq!(unwrap_call["typeFullName"], "i16");
        let inner = collect_kind(&source, "IDENT_PAT")
            .into_iter()
            .find(|node| text_of(node) == "inner")
            .expect("binding `inner` present");
        assert_eq!(inner["typeFullName"], "i16");
        // The user generic fn's call target also resolves to its canonical path.
        let identity_call = find_kind_containing(&source, "CALL_EXPR", "identity(7i64)")
            .expect("identity call present");
        assert_eq!(identity_call["methodFullName"], "hirdemo::identity");
    }

    #[test]
    fn hir_resolves_trait_method_call() {
        let (_dir, source) = parse_hir_crate("hirdemo", HIR_FIXTURE);
        let call =
            find_kind_containing(&source, "METHOD_CALL_EXPR", "r.greet()").expect("greet call");
        // Resolved through the trait impl to the user type's canonical method,
        // with the detached-file crate stem rewritten to the package name.
        assert_eq!(call["methodFullName"], "hirdemo::Robot::greet");
        assert_eq!(call["typeFullName"], "i32");
    }

    #[test]
    fn hir_resolves_hashmap_and_option_methods() {
        let (_dir, source) = parse_hir_crate("hirdemo", HIR_FIXTURE);
        // `HashMap::get` is not in the heuristic's table at all.
        let get_call =
            find_kind_containing(&source, "METHOD_CALL_EXPR", "m.get(&1)").expect("get call");
        assert_eq!(
            get_call["methodFullName"],
            "std::collections::hash::map::HashMap::get"
        );
        // `Option::unwrap` resolves where the heuristic is silent.
        let unwrap_call =
            find_kind_containing(&source, "METHOD_CALL_EXPR", "opt.unwrap()").expect("unwrap call");
        assert_eq!(
            unwrap_call["methodFullName"],
            "core::option::Option::unwrap"
        );
    }

    #[test]
    fn hir_resolves_vec_new_element_type() {
        let (_dir, source) = parse_hir_crate("hirdemo", HIR_FIXTURE);
        // The element type of `Vec::new()` is only known after unifying it with
        // the later `v.push(1u8)`; HIR renders the full monomorphized type,
        // including the allocator parameter.
        let call = find_kind_containing(&source, "CALL_EXPR", "Vec::new()").expect("Vec::new call");
        assert_eq!(
            call["typeFullName"],
            "alloc::vec::Vec<u8, alloc::alloc::Global>"
        );
    }

    #[test]
    fn heuristic_alone_cannot_resolve_what_hir_does() {
        // Same fixture without the sysroot/HIR path: the heuristic cannot type a
        // generic return, an unknown std method, or a `Vec::new()` element. This
        // pins the contrast the HIR integration is meant to close.
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"hirdemo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let file = root.join("src/lib.rs");
        fs::write(&file, HIR_FIXTURE).unwrap();

        let json = parse_file_with_sysroot(root, &file, false).unwrap();
        let source = &json["children"][0];

        let get_call =
            find_kind_containing(source, "METHOD_CALL_EXPR", "m.get(&1)").expect("get call");
        assert!(
            get_call.get("methodFullName").is_none(),
            "heuristic must not resolve HashMap::get; got {:?}",
            get_call.get("methodFullName")
        );
        let identity_call =
            find_kind_containing(source, "CALL_EXPR", "identity(7i64)").expect("identity call");
        assert_ne!(
            identity_call.get("typeFullName").and_then(Value::as_str),
            Some("i64"),
            "heuristic cannot monomorphize the generic return type"
        );
    }
}
