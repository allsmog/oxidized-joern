//! Optional rust-analyzer HIR-backed resolver.
//!
//! The hand-rolled [`SemanticModel`](crate::SemanticModel) resolves a useful but
//! bounded set of types/methods from the bare syntax tree. This module augments
//! it with rust-analyzer's HIR, which performs real type inference and name
//! resolution, so generics, trait methods, and standard-library APIs beyond the
//! heuristic's hard-coded table resolve correctly.
//!
//! The resolver loads the input file as a rust-analyzer "detached file"
//! workspace with the discovered sysroot (so `String` -> `alloc::string::String`,
//! `HashMap` methods, `Option::unwrap`, etc. resolve). It then performs a single
//! [`hir::Semantics`] pass over the syntax tree, recording the resolved
//! `typeFullName`/`methodFullName` keyed by each node's [`TextRange`]. The caller
//! looks those up while serializing.
//!
//! Everything here is best-effort: building the database touches the filesystem
//! and `cargo`/`rustc` for the sysroot, and rust-analyzer queries may panic on
//! edge cases. Any failure yields an empty resolver, and the heuristic model
//! remains the source of truth (this module only *fills* fields the heuristic
//! left empty), so the JSON envelope is never degraded.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use ra_ap_hir::{
    AsAssocItem, AssocItemContainer, DisplayTarget, HirDisplay, Module, Semantics, Type, attach_db,
};
use ra_ap_ide_db::RootDatabase;
use ra_ap_ide_db::base_db;
use ra_ap_load_cargo::{LoadCargoConfig, ProcMacroServerChoice, load_workspace};
use ra_ap_paths::{AbsPathBuf, Utf8PathBuf};
use ra_ap_project_model::{CargoConfig, ManifestPath, ProjectWorkspace, RustLibSource};
use ra_ap_syntax::ast::{self, AstNode, HasArgList, HasName};
use ra_ap_syntax::{NodeOrToken, SyntaxElement, SyntaxKind, SyntaxNode, TextRange};
use ra_ap_vfs::VfsPath;
use serde_json::{Map, Value, json};

/// Resolved type/method full names keyed by the syntax node's text range.
#[derive(Default)]
pub struct HirResolver {
    type_full_names: HashMap<TextRange, String>,
    method_full_names: HashMap<TextRange, String>,
    macro_expansions: HashMap<TextRange, Value>,
}

impl HirResolver {
    /// Build a resolver for `file`, or return `None` if HIR resolution is not
    /// possible (file not on disk, sysroot/workspace load failure, or a panic
    /// inside rust-analyzer). Never panics.
    ///
    /// `crate_name` is the package name discovered from `Cargo.toml`. When the
    /// input is a single detached file, rust-analyzer names its crate after the
    /// file stem (e.g. `lib`); remapping that root segment to the real package
    /// name keeps HIR-resolved user-type paths consistent with the heuristic's
    /// `crateName::Type` convention.
    pub fn try_build(file: &Path, crate_name: Option<&str>) -> Option<Self> {
        // HIR needs a real on-disk file: it loads it through cargo/rustc and a
        // VFS. Synthetic in-memory paths (used by some unit tests) cannot be
        // resolved, so bail out early and let the heuristic handle them.
        if !file.is_file() {
            return None;
        }
        catch_unwind(AssertUnwindSafe(|| build(file, crate_name)))
            .ok()
            .flatten()
    }

    pub fn type_full_name(&self, range: TextRange) -> Option<&str> {
        self.type_full_names.get(&range).map(String::as_str)
    }

    pub fn method_full_name(&self, range: TextRange) -> Option<&str> {
        self.method_full_names.get(&range).map(String::as_str)
    }

    pub fn macro_expansion(&self, range: TextRange) -> Option<&Value> {
        self.macro_expansions.get(&range)
    }
}

/// Shared resolution context for one file: the database, the display target for
/// non-ADT types, the local crate, and the `Cargo.toml` package name used to
/// rewrite the detached-file crate's stem.
struct Ctx<'a> {
    db: &'a RootDatabase,
    display_target: Option<DisplayTarget>,
    local_crate: Option<base_db::Crate>,
    crate_name: Option<String>,
}

fn build(file: &Path, crate_name: Option<&str>) -> Option<HirResolver> {
    let abs = std::fs::canonicalize(file).ok()?;
    let utf8 = Utf8PathBuf::from_path_buf(abs).ok()?;
    let abs_path = AbsPathBuf::try_from(utf8).ok()?;
    let manifest = ManifestPath::try_from(abs_path.clone()).ok()?;

    let cargo_config = CargoConfig {
        sysroot: Some(RustLibSource::Discover),
        ..Default::default()
    };

    let ws = ProjectWorkspace::load_detached_file(&manifest, &cargo_config).ok()?;
    let load_config = LoadCargoConfig {
        load_out_dirs_from_check: false,
        with_proc_macro_server: ProcMacroServerChoice::None,
        prefill_caches: false,
        num_worker_threads: 1,
        proc_macro_processes: 0,
    };
    let extra_env = Default::default();
    let (db, vfs, _proc) = load_workspace(ws, &extra_env, &load_config).ok()?;

    let vfs_path = VfsPath::from(abs_path);
    let (file_id, _) = vfs.file_id(&vfs_path)?;

    let mut resolver = HirResolver::default();
    attach_db(&db, || {
        let sema = Semantics::new(&db);
        let editioned = sema.attach_first_edition(file_id);
        let source_file = sema.parse(editioned);
        let local_crate = base_db::relevant_crates(&db, file_id)
            .iter()
            .next()
            .copied();
        let ctx = Ctx {
            db: &db,
            display_target: local_crate.map(|krate| DisplayTarget::from_crate(&db, krate)),
            local_crate,
            crate_name: crate_name.map(ToOwned::to_owned),
        };
        collect(&sema, &ctx, source_file.syntax(), &mut resolver);
    });

    if resolver.type_full_names.is_empty() && resolver.method_full_names.is_empty() {
        None
    } else {
        Some(resolver)
    }
}

fn collect(
    sema: &Semantics<'_, RootDatabase>,
    ctx: &Ctx<'_>,
    root: &SyntaxNode,
    resolver: &mut HirResolver,
) {
    for node in root.descendants() {
        if let Some(expr) = ast::Expr::cast(node.clone())
            && let Some(info) = sema.type_of_expr(&expr)
            && let Some(name) = type_full_name(&info.original(), ctx)
        {
            resolver.type_full_names.insert(node.text_range(), name);
        }

        if let Some(path_expr) = ast::PathExpr::cast(node.clone())
            && let Some(name) = source_path_expr_function_type(sema, ctx, &path_expr)
        {
            resolver.type_full_names.insert(node.text_range(), name);
        }

        if node.kind() == SyntaxKind::NAME_REF
            && let Some(name) = source_final_name_ref_function_type(sema, ctx, &node)
        {
            resolver.type_full_names.insert(node.text_range(), name);
        }

        if let Some(pat) = ast::IdentPat::cast(node.clone())
            && pat.name().is_some()
            && let Some(info) = sema.type_of_pat(&ast::Pat::IdentPat(pat))
            && let Some(name) = type_full_name(&info.original(), ctx)
        {
            resolver.type_full_names.insert(node.text_range(), name);
        }

        if let Some(call) = ast::MethodCallExpr::cast(node.clone())
            && let Some(func) = sema.resolve_method_call(&call)
            && let Some(name) = callable_full_name(&func, ctx)
        {
            resolver.method_full_names.insert(node.text_range(), name);
        }

        if let Some(call) = ast::CallExpr::cast(node.clone())
            && let Some(name) = call_target_full_name(sema, ctx, &call)
        {
            resolver.method_full_names.insert(node.text_range(), name);
        }

        if let Some(macro_call) = ast::MacroCall::cast(node.clone())
            && let Some(expanded) = sema.expand_macro_call(&macro_call)
        {
            resolver.macro_expansions.insert(
                node.text_range(),
                expanded_node_to_json(sema, ctx, &expanded.value, 0),
            );
        }
    }
}

fn source_path_expr_function_type(
    sema: &Semantics<'_, RootDatabase>,
    ctx: &Ctx<'_>,
    path_expr: &ast::PathExpr,
) -> Option<String> {
    let path = path_expr.path()?;
    match sema.resolve_path(&path)? {
        ra_ap_hir::PathResolution::Def(ra_ap_hir::ModuleDef::Function(func)) => {
            function_type_for_path(sema, ctx, &path, &func)
        }
        _ => None,
    }
}

fn source_final_name_ref_function_type(
    sema: &Semantics<'_, RootDatabase>,
    ctx: &Ctx<'_>,
    node: &SyntaxNode,
) -> Option<String> {
    node.ancestors()
        .filter_map(ast::Path::cast)
        .find_map(|path| {
            let last_name_ref = path
                .syntax()
                .descendants()
                .filter(|child| child.kind() == SyntaxKind::NAME_REF)
                .last()?;
            if last_name_ref.text_range() != node.text_range() {
                return None;
            }
            match sema.resolve_path(&path)? {
                ra_ap_hir::PathResolution::Def(ra_ap_hir::ModuleDef::Function(func)) => {
                    function_type_for_path(sema, ctx, &path, &func)
                }
                _ => None,
            }
        })
}

fn expanded_node_to_json(
    sema: &Semantics<'_, RootDatabase>,
    ctx: &Ctx<'_>,
    node: &SyntaxNode,
    depth: usize,
) -> Value {
    let mut obj = expansion_base_object(node.kind(), node.text().to_string());
    let children = node
        .children_with_tokens()
        .filter_map(|child| expanded_element_to_json(sema, ctx, child, depth))
        .collect::<Vec<_>>();
    if !children.is_empty() {
        obj.insert("children".into(), Value::Array(children));
    }
    enrich_expanded_node(sema, ctx, node, &mut obj);
    if depth < 8
        && let Some(macro_call) = ast::MacroCall::cast(node.clone())
        && let Some(expanded) = sema.expand_macro_call(&macro_call)
    {
        let mut expansion = expanded_node_to_json(sema, ctx, &expanded.value, depth + 1);
        apply_macro_expansion_context(&macro_call, &mut expansion);
        obj.insert("macroExpansion".into(), expansion);
    }
    Value::Object(obj)
}

fn apply_macro_expansion_context(macro_call: &ast::MacroCall, expansion: &mut Value) {
    if !macro_call_is_semicolon_stmt(macro_call) {
        return;
    }
    if let Value::Object(obj) = expansion {
        if let Some(Value::Object(first_child)) = obj
            .get_mut("children")
            .and_then(Value::as_array_mut)
            .and_then(|children| children.first_mut())
        {
            first_child.insert("typeFullName".into(), Value::String("()".into()));
        } else {
            obj.insert("typeFullName".into(), Value::String("()".into()));
        }
    }
}

fn macro_call_is_semicolon_stmt(macro_call: &ast::MacroCall) -> bool {
    let Some(macro_expr) = macro_call
        .syntax()
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::MACRO_EXPR)
    else {
        return false;
    };
    let Some(expr_stmt) = macro_expr
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::EXPR_STMT)
    else {
        return false;
    };
    expr_stmt.children_with_tokens().any(
        |child| matches!(child, NodeOrToken::Token(token) if token.kind() == SyntaxKind::SEMICOLON),
    )
}

fn expanded_macro_expr_is_semicolon_stmt(node: &SyntaxNode) -> bool {
    if node.kind() != SyntaxKind::MACRO_EXPR {
        return false;
    }
    let Some(expr_stmt) = node
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::EXPR_STMT)
    else {
        return false;
    };
    expr_stmt.children_with_tokens().any(
        |child| matches!(child, NodeOrToken::Token(token) if token.kind() == SyntaxKind::SEMICOLON),
    )
}

fn expanded_element_to_json(
    sema: &Semantics<'_, RootDatabase>,
    ctx: &Ctx<'_>,
    element: SyntaxElement,
    depth: usize,
) -> Option<Value> {
    match element {
        NodeOrToken::Node(node) => {
            if should_skip_kind(node.kind()) {
                None
            } else {
                Some(expanded_node_to_json(sema, ctx, &node, depth))
            }
        }
        NodeOrToken::Token(token) => {
            if should_skip_kind(token.kind()) {
                None
            } else {
                let mut obj = expansion_base_object(token.kind(), token.text().to_string());
                obj.insert("children".into(), Value::Array(Vec::new()));
                Some(Value::Object(obj))
            }
        }
    }
}

fn enrich_expanded_node(
    sema: &Semantics<'_, RootDatabase>,
    ctx: &Ctx<'_>,
    node: &SyntaxNode,
    obj: &mut Map<String, Value>,
) {
    if node.kind() == SyntaxKind::NAME_REF
        && let Some(name) = expanded_name_ref_type(sema, ctx, node)
    {
        obj.insert("typeFullName".into(), Value::String(name));
    }

    if let Some(name) = expanded_path_expr_function_type(sema, ctx, node) {
        obj.insert("typeFullName".into(), Value::String(name));
    } else if let Some(expr) = ast::Expr::cast(node.clone())
        && let Some(info) = sema.type_of_expr(&expr)
        && let Some(name) = type_full_name_for_expansion(&info.original(), ctx)
    {
        obj.insert(
            "typeFullName".into(),
            Value::String(expanded_adjusted_type(sema, ctx, node, name)),
        );
    }
    if expanded_macro_expr_is_semicolon_stmt(node) {
        obj.insert("typeFullName".into(), Value::String("()".into()));
    }

    if let Some(pat) = ast::IdentPat::cast(node.clone())
        && pat.name().is_some()
        && let Some(info) = sema.type_of_pat(&ast::Pat::IdentPat(pat))
        && let Some(name) = type_full_name_for_expansion(&info.original(), ctx)
    {
        obj.insert("typeFullName".into(), Value::String(name));
    }

    if let Some(call) = ast::MethodCallExpr::cast(node.clone())
        && let Some(func) = sema.resolve_method_call(&call)
        && let Some(name) = expansion_callable_full_name(&func, ctx)
    {
        obj.insert(
            "methodFullName".into(),
            Value::String(expanded_adjusted_method_full_name(&name)),
        );
    }

    if let Some(call) = ast::CallExpr::cast(node.clone())
        && let Some(name) = expansion_call_target_full_name(sema, ctx, &call)
    {
        obj.insert(
            "methodFullName".into(),
            Value::String(expanded_adjusted_method_full_name(&name)),
        );
    }
}

fn expanded_adjusted_method_full_name(name: &str) -> String {
    match name {
        "alloc::vec::from_elem" => "alloc::vec::from_elem<T>".into(),
        "alloc::boxed::Box::new_uninit" => {
            "alloc::boxed::Box<T, alloc::alloc::Global>::new_uninit".into()
        }
        "alloc::intrinsics::write_box_via_move" => {
            "alloc::intrinsics::write_box_via_move<T>".into()
        }
        "alloc::boxed::box_assume_init_into_vec_unsafe" => {
            "alloc::boxed::box_assume_init_into_vec_unsafe<T, N>".into()
        }
        "core::slice::len" => "[T]::len".into(),
        _ => name.into(),
    }
}

fn expanded_name_ref_type(
    sema: &Semantics<'_, RootDatabase>,
    ctx: &Ctx<'_>,
    node: &SyntaxNode,
) -> Option<String> {
    if matches!(
        node.text().to_string().as_str(),
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
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
    ) {
        return Some(node.text().to_string());
    }
    if node.text() == "Box"
        && node.ancestors().filter_map(ast::Path::cast).any(|path| {
            path.syntax()
                .text()
                .to_string()
                .contains("$crate::boxed::Box")
        })
    {
        return Some("alloc::boxed::Box".into());
    }
    node.ancestors()
        .filter_map(ast::Path::cast)
        .find_map(|path| {
            let last_name_ref = path
                .syntax()
                .descendants()
                .filter(|child| child.kind() == SyntaxKind::NAME_REF)
                .last()?;
            if last_name_ref.text_range() != node.text_range() {
                return None;
            }
            match sema.resolve_path(&path)? {
                ra_ap_hir::PathResolution::Def(ra_ap_hir::ModuleDef::Function(func)) => {
                    function_type_for_path(sema, ctx, &path, &func)
                }
                _ => None,
            }
        })
        .or_else(|| expanded_local_name_ref_type(sema, ctx, node))
        .or_else(|| expanded_type_name_ref_type(node))
}

fn expanded_local_name_ref_type(
    sema: &Semantics<'_, RootDatabase>,
    ctx: &Ctx<'_>,
    node: &SyntaxNode,
) -> Option<String> {
    let path_expr = node.ancestors().find_map(ast::PathExpr::cast)?;
    let last_name_ref = path_expr
        .syntax()
        .descendants()
        .filter(|child| child.kind() == SyntaxKind::NAME_REF)
        .last()?;
    if last_name_ref.text_range() != node.text_range() {
        return None;
    }
    let expr = ast::Expr::PathExpr(path_expr);
    sema.type_of_expr(&expr)
        .and_then(|info| type_full_name_for_expansion(&info.original(), ctx))
        .map(|typ| expanded_adjusted_type(sema, ctx, node, typ))
}

fn expanded_type_name_ref_type(node: &SyntaxNode) -> Option<String> {
    let path_type = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_TYPE)?;
    let last_name_ref = path_type
        .descendants()
        .filter(|child| child.kind() == SyntaxKind::NAME_REF)
        .last()?;
    if last_name_ref.text_range() != node.text_range() {
        return None;
    }
    let name = path_type.text().to_string();
    is_expanded_builtin_type_name(&name).then_some(name)
}

fn is_expanded_builtin_type_name(name: &str) -> bool {
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

fn expanded_adjusted_type(
    sema: &Semantics<'_, RootDatabase>,
    ctx: &Ctx<'_>,
    node: &SyntaxNode,
    typ: String,
) -> String {
    if typ == "!"
        && node.kind() == SyntaxKind::BLOCK_EXPR
        && node
            .parent()
            .is_some_and(|parent| parent.kind() == SyntaxKind::IF_EXPR)
    {
        return "()".into();
    }
    let typ = expanded_ref_adjusted_type(node, typ);
    let typ = expanded_method_receiver_adjusted_type(node, typ);
    let typ = expanded_self_capture_adjusted_type(ctx, node, typ);
    expanded_macro_capture_adjusted_type(sema, ctx, node, typ)
}

fn expanded_self_capture_adjusted_type(ctx: &Ctx<'_>, node: &SyntaxNode, typ: String) -> String {
    if !expanded_node_is_self_path(node) {
        return typ;
    }
    let base = typ
        .trim_start()
        .strip_prefix("&mut ")
        .or_else(|| typ.trim_start().strip_prefix('&'))
        .unwrap_or(&typ)
        .trim();
    if base.contains("::")
        || base.len() == 1
        || !base
            .chars()
            .next()
            .is_some_and(|first| first.is_uppercase())
    {
        return typ;
    }
    ctx.crate_name
        .as_ref()
        .map(|crate_name| format!("{crate_name}::{base}"))
        .unwrap_or(typ)
}

fn expanded_node_is_self_path(node: &SyntaxNode) -> bool {
    if node.text().to_string() == "self" {
        return true;
    }
    node.kind() == SyntaxKind::NAME_REF && node.text().to_string() == "self"
}

fn expanded_method_receiver_adjusted_type(node: &SyntaxNode, typ: String) -> String {
    let Some(path_expr) = expanded_receiver_path_expr_for_node(node) else {
        return typ;
    };
    let Some(method_call) = path_expr
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::METHOD_CALL_EXPR)
    else {
        return typ;
    };
    let is_receiver = method_call
        .children()
        .find(|child| child.kind() == SyntaxKind::PATH_EXPR)
        .is_some_and(|receiver| receiver.text_range() == path_expr.text_range());
    if !is_receiver {
        return typ;
    }
    let method_name = method_call
        .children()
        .find(|child| child.kind() == SyntaxKind::NAME_REF)
        .map(|name| name.text().to_string());
    match method_name.as_deref() {
        Some("contains" | "replace")
            if matches!(typ.as_str(), "&String" | "&alloc::string::String") =>
        {
            "&str".into()
        }
        _ => typ,
    }
}

fn expanded_receiver_path_expr_for_node(node: &SyntaxNode) -> Option<SyntaxNode> {
    match node.kind() {
        SyntaxKind::PATH_EXPR => Some(node.clone()),
        SyntaxKind::NAME_REF => node
            .ancestors()
            .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_EXPR),
        _ => None,
    }
}

fn expanded_ref_adjusted_type(node: &SyntaxNode, typ: String) -> String {
    if typ.trim_start().starts_with('&') {
        return typ;
    }
    let Some(path_expr) = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_EXPR)
    else {
        return typ;
    };
    let Some(ref_expr) = path_expr
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::REF_EXPR)
    else {
        return typ;
    };
    if ref_expr.children_with_tokens().any(
        |child| matches!(child, NodeOrToken::Token(token) if token.kind() == SyntaxKind::MUT_KW),
    ) {
        format!("&mut {typ}")
    } else {
        format!("&{typ}")
    }
}

fn expanded_macro_capture_adjusted_type(
    sema: &Semantics<'_, RootDatabase>,
    ctx: &Ctx<'_>,
    node: &SyntaxNode,
    typ: String,
) -> String {
    if typ.trim_start().starts_with('&')
        || !node
            .ancestors()
            .any(|ancestor| ancestor.kind() == SyntaxKind::IF_EXPR)
        || !node
            .ancestors()
            .any(|ancestor| ancestor.kind() == SyntaxKind::BIN_EXPR)
    {
        return typ;
    }
    if node.kind() == SyntaxKind::LITERAL {
        return format!("&{typ}");
    }
    let path = ast::PathExpr::cast(node.clone())
        .and_then(|path_expr| path_expr.path())
        .or_else(|| node.ancestors().filter_map(ast::Path::cast).next());
    let Some(path) = path else {
        return typ;
    };
    match sema.resolve_path(&path) {
        Some(ra_ap_hir::PathResolution::Local(local)) if local.is_param(ctx.db) => {
            format!("&{typ}")
        }
        _ => typ,
    }
}

fn expanded_path_expr_function_type(
    sema: &Semantics<'_, RootDatabase>,
    ctx: &Ctx<'_>,
    node: &SyntaxNode,
) -> Option<String> {
    let path_expr = ast::PathExpr::cast(node.clone())?;
    let path = path_expr.path()?;
    match sema.resolve_path(&path)? {
        ra_ap_hir::PathResolution::Def(ra_ap_hir::ModuleDef::Function(func)) => {
            function_type_for_path(sema, ctx, &path, &func)
        }
        _ => None,
    }
}

fn function_type_for_path(
    sema: &Semantics<'_, RootDatabase>,
    ctx: &Ctx<'_>,
    path: &ast::Path,
    func: &ra_ap_hir::Function,
) -> Option<String> {
    specialized_call_function_type(sema, ctx, path).or_else(|| function_type_full_name(func, ctx))
}

fn specialized_call_function_type(
    sema: &Semantics<'_, RootDatabase>,
    ctx: &Ctx<'_>,
    path: &ast::Path,
) -> Option<String> {
    let path_expr = path.syntax().ancestors().find_map(ast::PathExpr::cast)?;
    let call = path_expr.syntax().parent().and_then(ast::CallExpr::cast)?;
    let arg_types = call
        .arg_list()?
        .args()
        .filter_map(|arg| {
            sema.type_of_expr(&arg)
                .and_then(|info| type_full_name_for_expansion(&info.original(), ctx))
        })
        .collect::<Vec<_>>();
    let call_expr = ast::Expr::CallExpr(call);
    let ret = sema
        .type_of_expr(&call_expr)
        .and_then(|info| type_full_name_for_expansion(&info.original(), ctx))?;
    Some(format!("fn({}) -> {ret}", arg_types.join(", ")))
}

fn function_type_full_name(func: &ra_ap_hir::Function, ctx: &Ctx<'_>) -> Option<String> {
    let params = func
        .params_without_self(ctx.db)
        .iter()
        .map(|param| {
            type_full_name_for_expansion(param.ty(), ctx)
                .map(|typ| normalize_expansion_type_name(&typ))
                .unwrap_or_else(|| "_".into())
        })
        .collect::<Vec<_>>();
    let ret =
        normalize_expansion_type_name(&type_full_name_for_expansion(&func.ret_type(ctx.db), ctx)?);
    Some(format!("fn({}) -> {ret}", params.join(", ")))
}

fn normalize_expansion_type_name(name: &str) -> String {
    let normalized: String = match name {
        "Arguments<'_>" | "core::fmt::Arguments<_>" | "core::fmt::Arguments<'_>" => {
            "core::fmt::Arguments<'a>".into()
        }
        "&'static str" => "&str".into(),
        _ => name.into(),
    };
    let normalized = replace_bare_expansion_type_name(&normalized, "Vec", "alloc::vec::Vec");
    replace_bare_expansion_type_name(&normalized, "Global", "alloc::alloc::Global")
}

fn replace_bare_expansion_type_name(input: &str, name: &str, qualified: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    for (idx, _) in input.match_indices(name) {
        let end = idx + name.len();
        let before = input[..idx].chars().next_back();
        let after = input[end..].chars().next();
        if !matches!(before, Some(ch) if ch.is_alphanumeric() || ch == '_' || ch == ':')
            && !matches!(after, Some(ch) if ch.is_alphanumeric() || ch == '_' || ch == ':')
        {
            out.push_str(&input[cursor..idx]);
            out.push_str(qualified);
            cursor = end;
        }
    }
    out.push_str(&input[cursor..]);
    out
}

fn expansion_base_object(kind: SyntaxKind, text: String) -> Map<String, Value> {
    let mut obj = Map::new();
    obj.insert("nodeKind".into(), Value::String(format!("{kind:?}")));
    obj.insert(
        "range".into(),
        json!({
            "startOffset": 0,
            "endOffset": 0,
            "startLine": 0,
            "startColumn": 0
        }),
    );
    obj.insert("text".into(), Value::String(text));
    obj
}

fn should_skip_kind(kind: SyntaxKind) -> bool {
    kind.is_trivia() || matches!(kind, SyntaxKind::ERROR)
}

fn expansion_call_target_full_name(
    sema: &Semantics<'_, RootDatabase>,
    ctx: &Ctx<'_>,
    call: &ast::CallExpr,
) -> Option<String> {
    let ast::Expr::PathExpr(path_expr) = call.expr()? else {
        return None;
    };
    let path = path_expr.path()?;
    match sema.resolve_path(&path)? {
        ra_ap_hir::PathResolution::Def(ra_ap_hir::ModuleDef::Function(func)) => {
            expansion_callable_full_name(&func, ctx)
        }
        _ => None,
    }
}

fn expansion_callable_full_name(func: &ra_ap_hir::Function, ctx: &Ctx<'_>) -> Option<String> {
    let name = callable_full_name(func, ctx)?;
    Some(match name.as_str() {
        "core::hint::must_use" => "core::hint::must_use<T>".into(),
        "alloc::str::replace" => "str::replace<P>".into(),
        _ => name,
    })
}

/// Resolve the callee of a plain call expression (`Foo::bar(..)`, `func(..)`) to
/// a canonical full name when it is a function/associated function.
fn call_target_full_name(
    sema: &Semantics<'_, RootDatabase>,
    ctx: &Ctx<'_>,
    call: &ast::CallExpr,
) -> Option<String> {
    let ast::Expr::PathExpr(path_expr) = call.expr()? else {
        return None;
    };
    let path = path_expr.path()?;
    match sema.resolve_path(&path)? {
        ra_ap_hir::PathResolution::Def(ra_ap_hir::ModuleDef::Function(func)) => {
            callable_full_name(&func, ctx)
        }
        _ => None,
    }
}

/// Canonical, fully qualified type name (e.g. `alloc::string::String`,
/// `std::collections::hash::map::HashMap`). ADTs are rendered through their
/// defining module path; everything else falls back to rust-analyzer's display.
fn type_full_name(ty: &Type, ctx: &Ctx<'_>) -> Option<String> {
    type_full_name_inner(ty, ctx, false)
}

fn type_full_name_for_expansion(ty: &Type, ctx: &Ctx<'_>) -> Option<String> {
    type_full_name_inner(ty, ctx, true).map(|name| normalize_expansion_type_name(&name))
}

fn type_full_name_inner(ty: &Type, ctx: &Ctx<'_>, include_fn: bool) -> Option<String> {
    // ADTs (structs/enums/unions, including std `Vec`/`HashMap`/`Option`) are
    // rendered through their canonical module path, with generic arguments
    // recursively resolved (e.g. `alloc::vec::Vec<u8, alloc::alloc::Global>`).
    if let Some((adt, args)) = ty.as_adt_with_args() {
        let base = canonical_adt(&adt, ctx);
        let rendered: Vec<String> = args
            .iter()
            .map(|arg| match arg {
                Some(arg_ty) => {
                    type_full_name_inner(arg_ty, ctx, include_fn).unwrap_or_else(|| "_".into())
                }
                None => "_".into(),
            })
            .collect();
        if rendered.is_empty() {
            return Some(base);
        }
        return Some(format!("{base}<{}>", rendered.join(", ")));
    }
    // The type of a path that names a function is the callable's own
    // "fn item" type (e.g. `fn new() -> String`). That is the callee's type, not
    // a useful `typeFullName` for the referencing node, and the heuristic never
    // emits it -- so skip it and let the call expression carry its return type.
    if !include_fn && ty.is_fn() {
        return None;
    }
    let target = ctx.display_target?;
    let rendered = ty.display(ctx.db, target).to_string();
    if rendered.is_empty() || rendered.contains("{unknown}") {
        None
    } else {
        Some(rendered)
    }
}

/// Canonical full name for a callable: `<owner>::<name>` where the owner is the
/// impl's `Self` type (for inherent/trait impls) or the trait path (for default
/// trait methods), and the module path otherwise.
fn callable_full_name(func: &ra_ap_hir::Function, ctx: &Ctx<'_>) -> Option<String> {
    let db = ctx.db;
    let name = func.name(db).as_str().to_string();
    if name.is_empty() {
        return None;
    }
    if let Some(assoc) = func.as_assoc_item(db) {
        match assoc.container(db) {
            AssocItemContainer::Impl(imp) => {
                let self_ty = imp.self_ty(db);
                if let Some(adt) = self_ty.as_adt() {
                    return Some(format!("{}::{name}", canonical_adt(&adt, ctx)));
                }
            }
            AssocItemContainer::Trait(tr) => {
                let module_path = module_path(&tr.module(db), ctx);
                let trait_name = tr.name(db).as_str().to_string();
                return Some(join_path([module_path, trait_name, name]));
            }
        }
    }
    Some(format!("{}::{name}", module_path(&func.module(db), ctx)))
}

fn canonical_adt(adt: &ra_ap_hir::Adt, ctx: &Ctx<'_>) -> String {
    let module_path = module_path(&adt.module(ctx.db), ctx);
    let name = adt.name(ctx.db).as_str().to_string();
    join_path([module_path, name])
}

/// Full `::`-joined path of a module, beginning with the crate name. For the
/// local (detached-file) crate, the file-stem crate name rust-analyzer assigns
/// is replaced with the real `Cargo.toml` package name when known.
fn module_path(module: &Module, ctx: &Ctx<'_>) -> String {
    let db = ctx.db;
    let module_crate = module.krate(db);
    let crate_name = match (&ctx.crate_name, ctx.local_crate) {
        (Some(name), Some(local)) if local == module_crate.base() => name.clone(),
        _ => module_crate
            .display_name(db)
            .map(|name| name.to_string())
            .unwrap_or_default(),
    };
    let mut segments: Vec<String> = module
        .path_to_root(db)
        .into_iter()
        .rev()
        .filter_map(|module| module.name(db).map(|name| name.as_str().to_string()))
        .collect();
    let mut path = vec![crate_name];
    path.append(&mut segments);
    join_path(path)
}

fn join_path<I: IntoIterator<Item = String>>(parts: I) -> String {
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("::")
}
