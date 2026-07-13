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
use ra_ap_syntax::ast::{self, AstNode, HasName};
use ra_ap_syntax::{SyntaxNode, TextRange};
use ra_ap_vfs::VfsPath;

/// Resolved type/method full names keyed by the syntax node's text range.
#[derive(Default)]
pub struct HirResolver {
    type_full_names: HashMap<TextRange, String>,
    method_full_names: HashMap<TextRange, String>,
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
    }
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
    // ADTs (structs/enums/unions, including std `Vec`/`HashMap`/`Option`) are
    // rendered through their canonical module path, with generic arguments
    // recursively resolved (e.g. `alloc::vec::Vec<u8, alloc::alloc::Global>`).
    if let Some((adt, args)) = ty.as_adt_with_args() {
        let base = canonical_adt(&adt, ctx);
        let rendered: Vec<String> = args
            .iter()
            .map(|arg| match arg {
                Some(arg_ty) => type_full_name(arg_ty, ctx).unwrap_or_else(|| "_".into()),
                None => "_".into(),
            })
            .collect();
        if rendered.is_empty() {
            return Some(base);
        }
        return Some(format!("{base}<{}>", rendered.join(", ")));
    }
    // The type of a path that names a function/closure is the callable's own
    // "fn item" type (e.g. `fn new() -> String`). That is the callee's type, not
    // a useful `typeFullName` for the referencing node, and the heuristic never
    // emits it -- so skip it and let the call expression carry its return type.
    if ty.is_fn() || ty.is_closure() {
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
