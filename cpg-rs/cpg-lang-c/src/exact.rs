//! Exact Joern-compatible C lowering shared by the shipped frontend and the
//! differential harness. Its output is driven to byte-for-byte parity with
//! Joern's `c2cpg`, verified by differential testing against a real Joern
//! install (see joern-parity/README.md). It reproduces Joern/x2cpg conventions:
//! operators lowered to `<operator>.*` CALL nodes, a declaration split into a
//! LOCAL plus an `<operator>.assignment` CALL, a synthetic METHOD_RETURN and
//! mirrored METHOD_PARAMETER_OUT nodes, ORDER/ARGUMENT_INDEX sequencing, and
//! simple type resolution — all emitted in Joern's canonical AST-dump format so
//! the output diffs cleanly against the oracle.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use tree_sitter::{Node, Parser};

/// Deterministic compiler inputs that affect C preprocessing. Paths are
/// source-snapshot-relative: callers must include every referenced header in
/// `sources`, so cached builds hash exactly what the frontend reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreprocessorConfig {
    pub include_paths: Vec<String>,
    pub forced_includes: Vec<String>,
    /// Command-line definitions in `NAME` or `NAME=VALUE` form.
    pub defines: Vec<String>,
}

struct PreprocessorEnvironment<'a> {
    sources: HashMap<String, &'a str>,
    config: &'a PreprocessorConfig,
}

pub fn canonical_dump_paths(paths: &[String]) -> String {
    let sources: Vec<(String, String)> = paths
        .iter()
        .map(|path| {
            let source = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("read exact C input {path}: {e}"));
            let file = std::path::Path::new(path)
                .file_name()
                .unwrap_or_else(|| panic!("input has no filename: {path}"))
                .to_string_lossy()
                .into_owned();
            (file, source)
        })
        .collect();
    canonical_dump_sources(&sources)
}

pub fn canonical_dump_sources(sources: &[(String, String)]) -> String {
    canonical_dump_sources_with_config(sources, &PreprocessorConfig::default())
}

pub fn canonical_dump_sources_with_config(
    sources: &[(String, String)],
    config: &PreprocessorConfig,
) -> String {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .unwrap();

    struct Unit {
        file: String,
        src: String,
        tree: tree_sitter::Tree,
    }
    let units: Vec<Unit> = sources
        .iter()
        .map(|(file, src)| {
            let tree = parser.parse(src, None).unwrap();
            Unit {
                file: file.clone(),
                src: src.clone(),
                tree,
            }
        })
        .collect();
    let preprocessor = PreprocessorEnvironment {
        sources: sources
            .iter()
            .map(|(path, source)| (normalize_source_path(path), source.as_str()))
            .collect(),
        config,
    };

    // Project-wide registries: defined functions (calls to these never become
    // stubs; each also gets a method TYPE_DECL) and struct definitions.
    let mut defined: Vec<String> = Vec::new();
    let mut raw_fn_decls: Vec<(String, String)> = Vec::new(); // (name, file)
    let mut struct_decls: Vec<(String, String, String)> = Vec::new(); // (tag, code, file)
    let mut used_types: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for u in &units {
        let b = u.src.as_bytes();
        let (top_level, _) =
            preprocess_translation_unit(u.tree.root_node(), b, &u.file, &preprocessor);
        for f in top_level {
            match f.kind() {
                "function_definition" => {
                    if let Some((name, _, _)) = fn_header(f, b) {
                        defined.push(name.clone());
                        raw_fn_decls.push((name, u.file.clone()));
                    }
                }
                "struct_specifier" | "union_specifier" | "enum_specifier"
                    if f.child_by_field_name("body").is_some() =>
                {
                    let tag = f
                        .child_by_field_name("name")
                        .map(|x| text(x, b).to_string())
                        .unwrap_or_default();
                    struct_decls.push((tag, esc(text(f, b)), u.file.clone()));
                }
                "type_definition" => {
                    let tag = f
                        .child_by_field_name("declarator")
                        .map(|x| text(x, b).to_string())
                        .unwrap_or_default();
                    struct_decls.push((tag, esc(text(f, b)), u.file.clone()));
                    // The typedef's underlying type registers as a used type,
                    // with its RAW source spelling (`unsigned int` is not
                    // normalised here, unlike variable types).
                    if let Some(t) = f.child_by_field_name("type") {
                        used_types.insert(normalize_type(text(t, b)));
                    }
                }
                _ => {}
            }
        }
    }
    let mut definition_counts: HashMap<String, usize> = HashMap::new();
    for (name, _) in &raw_fn_decls {
        *definition_counts.entry(name.clone()).or_default() += 1;
    }
    let ambiguous_functions: HashSet<String> = definition_counts
        .iter()
        .filter_map(|(name, &count)| (count > 1).then_some(name.clone()))
        .collect();
    let method_full = |name: &str, file: &str| {
        if ambiguous_functions.contains(name) {
            format!("{file}:{name}")
        } else {
            name.to_string()
        }
    };
    let fn_decls: Vec<(String, String)> = raw_fn_decls
        .iter()
        .map(|(name, file)| (method_full(name, file), file.clone()))
        .collect();

    // Each dump is one method subtree keyed by FULL_NAME; Joern's oracle sorts
    // all methods (user, <global> wrappers, <operator> stubs) by fullName.
    let mut dumps: Vec<(String, String)> = Vec::new();
    let mut stub_uses: HashMap<String, usize> = HashMap::new();
    let mut edges: Vec<(String, String, String)> = Vec::new();
    let mut placements: HashMap<String, Vec<(String, usize)>> = HashMap::new();
    let mut used_macros: std::collections::BTreeMap<String, (String, String, usize, String)> =
        std::collections::BTreeMap::new();
    // #include directives become IMPORT nodes that consume earlier sibling
    // slots: the file-global TYPE_DECL's ORDER is 1 + #includes.
    let include_counts: HashMap<String, usize> = units
        .iter()
        .map(|u| {
            let b = u.src.as_bytes();
            let (top_level, _) =
                preprocess_translation_unit(u.tree.root_node(), b, &u.file, &preprocessor);
            let n = top_level
                .iter()
                .filter(|f| f.kind() == "preproc_include")
                .count();
            (u.file.clone(), n)
        })
        .collect();

    for u in &units {
        let b = u.src.as_bytes();
        let root = u.tree.root_node();

        // Per-file tables (c2cpg resolves within the translation unit).
        let mut functions: HashMap<String, String> = HashMap::new();
        let mut function_full_names: HashMap<String, String> = HashMap::new();
        let mut globals: HashMap<String, String> = HashMap::new();
        let (top_level, macros) = preprocess_translation_unit(root, b, &u.file, &preprocessor);
        let mut enumerators: Vec<String> = Vec::new();
        for &f in &top_level {
            if f.kind() == "enum_specifier" {
                if let Some(body) = f.child_by_field_name("body") {
                    for e in named_children(body) {
                        if e.kind() == "enumerator" {
                            if let Some(en) = e.child_by_field_name("name") {
                                enumerators.push(text(en, b).to_string());
                            }
                        }
                    }
                }
            }
        }
        for &f in &top_level {
            match f.kind() {
                "function_definition" => {
                    if let Some((name, ret, _)) = fn_header(f, b) {
                        function_full_names.insert(name.clone(), method_full(&name, &u.file));
                        functions.insert(name, ret);
                    }
                }
                "declaration" => {
                    let base = normalize_type(
                        &f.child_by_field_name("type")
                            .map(|t| text(t, b).to_string())
                            .unwrap_or("ANY".into()),
                    );
                    for d in named_children(f) {
                        let decl = if d.kind() == "init_declarator" {
                            d.child_by_field_name("declarator")
                        } else if matches!(
                            d.kind(),
                            "identifier" | "pointer_declarator" | "array_declarator"
                        ) {
                            Some(d)
                        } else {
                            None
                        };
                        if let Some(decl) = decl {
                            if find_function_declarator(decl).is_none() {
                                let name = innermost_id(decl, b);
                                if !name.is_empty() {
                                    globals.insert(name, format!("{base}{}", decl_suffix(decl, b)));
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let mut ctx = Ctx {
            functions: &functions,
            function_full_names: &function_full_names,
            ambiguous_functions: &ambiguous_functions,
            globals: &globals,
            enumerators: &enumerators,
            macros: &macros,
            preprocessor: &preprocessor,
            used_macros: &mut used_macros,
            symbols: HashMap::new(),
            expanding_macros: HashSet::new(),
            phantoms: Vec::new(),
            stubs: &mut stub_uses,
            types: &mut used_types,
            out: String::new(),
            block: String::new(),
            line_no: 0,
            suppress_below: None,
            ctx_stack: Vec::new(),
            parent_stack: Vec::new(),
            sym_line: HashMap::new(),
            param_in_line: HashMap::new(),
            edges: &mut edges,
            placements: &mut placements,
        };

        // Standalone dump per user method, plus one per struct <clinit>.
        for &f in &top_level {
            match f.kind() {
                "function_definition" => {
                    if let Some((name, _, _)) = fn_header(f, b) {
                        let full = ctx
                            .function_full_names
                            .get(&name)
                            .cloned()
                            .unwrap_or_else(|| name.clone());
                        ctx.begin_block(&full);
                        ctx.emit_method(f, b, 0);
                        ctx.edge("SOURCE_FILE", format!("M:{full}"), format!("F:{}", u.file));
                        dumps.push((full, std::mem::take(&mut ctx.out)));
                    }
                }
                "struct_specifier" | "union_specifier" | "enum_specifier" if needs_clinit(f, b) => {
                    let tag = f
                        .child_by_field_name("name")
                        .map(|x| text(x, b).to_string())
                        .unwrap_or_default();
                    let members = count_members(f);
                    let key = format!("{tag}.<clinit>:{tag}()");
                    ctx.begin_block(&key);
                    ctx.emit_clinit(f, b, 0, members + 1);
                    ctx.edge("SOURCE_FILE", format!("M:{key}"), format!("F:{}", u.file));
                    dumps.push((key, std::mem::take(&mut ctx.out)));
                }
                _ => {}
            }
        }

        // The per-file `<global>` wrapper method.
        let gkey = format!("{}:<global>", u.file);
        ctx.begin_block(&gkey);
        ctx.emit_file_global(root, b, &u.file);
        ctx.edge("SOURCE_FILE", format!("M:{gkey}"), format!("F:{}", u.file));
        dumps.push((gkey, std::mem::take(&mut ctx.out)));
    }

    // Operator stubs and the synthetic <includes>:<global>, emitted through
    // an instrumented Ctx so they too produce addresses and edges.
    let empty_fns: HashMap<String, String> = HashMap::new();
    let empty_full_names: HashMap<String, String> = HashMap::new();
    let empty_ambiguous: HashSet<String> = HashSet::new();
    let empty_globals: HashMap<String, String> = HashMap::new();
    let mut stub_uses2: HashMap<String, usize> = HashMap::new();
    let empty_enums: Vec<String> = Vec::new();
    let empty_macros: HashMap<String, MacroDef> = HashMap::new();
    let mut sctx = Ctx {
        functions: &empty_fns,
        function_full_names: &empty_full_names,
        ambiguous_functions: &empty_ambiguous,
        globals: &empty_globals,
        enumerators: &empty_enums,
        macros: &empty_macros,
        preprocessor: &preprocessor,
        used_macros: &mut used_macros,
        symbols: HashMap::new(),
        expanding_macros: HashSet::new(),
        phantoms: Vec::new(),
        stubs: &mut stub_uses2,
        types: &mut used_types,
        out: String::new(),
        block: String::new(),
        line_no: 0,
        suppress_below: None,
        ctx_stack: Vec::new(),
        parent_stack: Vec::new(),
        sym_line: HashMap::new(),
        param_in_line: HashMap::new(),
        edges: &mut edges,
        placements: &mut placements,
    };
    let mut stub_list: Vec<(String, usize)> = stub_uses
        .into_iter()
        .filter(|(n, _)| !defined.contains(n))
        .collect();
    stub_list.sort();
    for (name, arity) in stub_list {
        sctx.begin_block(&name);
        sctx.emit_stub(&name, arity);
        dumps.push((name, std::mem::take(&mut sctx.out)));
    }
    let macro_methods: Vec<(String, (String, String, usize, String))> = sctx
        .used_macros
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (full, (name, directive, nparams, ret)) in macro_methods {
        sctx.begin_block(&full);
        sctx.emit_macro_method(&full, &name, &directive, nparams, &ret);
        dumps.push((full, std::mem::take(&mut sctx.out)));
    }
    sctx.begin_block("<includes>:<global>");
    sctx.emit_includes_global();
    sctx.edge(
        "SOURCE_FILE",
        "M:<includes>:<global>".into(),
        "F:<includes>".into(),
    );
    dumps.push(("<includes>:<global>".into(), std::mem::take(&mut sctx.out)));
    drop(sctx);

    dumps.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::new();
    for (_, d) in &dumps {
        out.push_str(d);
        out.push('\n');
    }

    // ---- non-method scaffolding nodes (NODES| section) ----
    out.push_str("NODES|META_DATA LANGUAGE=NEWC\n");
    out.push_str("NODES|FILE NAME=<includes> ORDER=1\n");
    out.push_str("NODES|FILE NAME=<unknown> ORDER=0\n");
    let mut files: Vec<&String> = units.iter().map(|u| &u.file).collect();
    files.sort();
    for f in &files {
        out.push_str(&format!("NODES|FILE NAME={f} ORDER=0\n"));
    }
    out.push_str(
        "NODES|NAMESPACE_BLOCK NAME=<global> FULL_NAME=<global> FILENAME=<unknown> ORDER=1\n",
    );
    out.push_str("NODES|NAMESPACE_BLOCK NAME=<global> FULL_NAME=<includes>:<global> FILENAME=<includes> ORDER=1\n");
    for f in &files {
        out.push_str(&format!(
            "NODES|NAMESPACE_BLOCK NAME=<global> FULL_NAME={f}:<global> FILENAME={f} ORDER=1\n"
        ));
    }
    out.push_str("NODES|NAMESPACE NAME=<global>\n");

    // TYPE_DECLs, sorted by FULL_NAME: internal structs (empty AST_PARENT_*
    // values, a c2cpg quirk), one per defined method (parented TYPE_DECL ->
    // file global), one per file <global>, and IS_EXTERNAL=true entries under
    // <includes>:<global> for every other referenced type (no ORDER).
    let struct_tags: Vec<&String> = struct_decls.iter().map(|(t, _, _)| t).collect();
    let mut tds: Vec<(String, String)> = Vec::new();
    for (tag, code, file) in &struct_decls {
        tds.push((tag.clone(), format!(
            "NODES|TYPE_DECL NAME={tag} FULL_NAME={tag} CODE={code} AST_PARENT_TYPE= AST_PARENT_FULL_NAME= FILENAME={file} ORDER=1\n"
        )));
    }
    for (name, file) in &fn_decls {
        tds.push((name.clone(), format!(
            "NODES|TYPE_DECL NAME={name} FULL_NAME={name} CODE={name} AST_PARENT_TYPE=TYPE_DECL AST_PARENT_FULL_NAME={file}:<global> FILENAME={file} ORDER=1\n"
        )));
    }
    for f in &files {
        let ord = 1 + include_counts.get(f.as_str()).copied().unwrap_or(0);
        tds.push((format!("{f}:<global>"), format!(
            "NODES|TYPE_DECL NAME=<global> FULL_NAME={f}:<global> CODE=<global> AST_PARENT_TYPE=NAMESPACE_BLOCK AST_PARENT_FULL_NAME={f}:<global> FILENAME={f} ORDER={ord}\n"
        )));
    }
    for t in &used_types {
        if struct_tags.contains(&t) || defined.contains(t) {
            continue;
        }
        tds.push((t.clone(), format!(
            "NODES|TYPE_DECL NAME={t} FULL_NAME={t} CODE={t} IS_EXTERNAL=true AST_PARENT_TYPE=NAMESPACE_BLOCK AST_PARENT_FULL_NAME=<includes>:<global> FILENAME=<includes>\n"
        )));
    }
    tds.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, l) in tds {
        out.push_str(&l);
    }
    for t in &used_types {
        out.push_str(&format!(
            "NODES|TYPE NAME={t} FULL_NAME={t} TYPE_DECL_FULL_NAME={t}\n"
        ));
    }

    // ---- EDGES section ----
    // CFG: built from the dump blocks themselves (nested METHOD/TYPE_DECL
    // subtrees are transparent there, so each CFG is generated exactly once,
    // from its home block).
    for (key, text) in &dumps {
        for (s, d) in cfg_edges_for_block(key, text) {
            edges.push(("CFG".into(), s, d));
        }
    }

    // REACHING_DEF flows (the FLOWS| section).
    let mut flows: Vec<(String, String, String)> = Vec::new();
    for (key, text) in &dumps {
        for (var, s, d) in reaching_def_flows(key, text) {
            flows.push((var, s, d));
        }
    }
    // Cross-method captured-identifier edges (DdgGenerator
    // .addEdgesToCapturedIdentifiersAndParameters): a global's identifier links
    // to its first usage in each method that captures the global scope.
    for f in captured_identifier_flows(&dumps) {
        flows.push(f);
    }

    // TYPE -> its TYPE_DECL (struct decls are walk-addressed, rest are D:).
    for t in &used_types {
        let dst = if struct_tags.contains(&t) {
            format!("TD:{t}")
        } else {
            format!("D:{t}")
        };
        edges.push(("REF".into(), format!("T:{t}"), dst));
    }
    // NAMESPACE_BLOCK -> NAMESPACE, and NAMESPACE_BLOCK -> its FILE.
    edges.push(("REF".into(), "NB:<global>".into(), "NS:<global>".into()));
    edges.push((
        "REF".into(),
        "NB:<includes>:<global>".into(),
        "NS:<global>".into(),
    ));
    edges.push((
        "SOURCE_FILE".into(),
        "NB:<global>".into(),
        "F:<unknown>".into(),
    ));
    edges.push((
        "SOURCE_FILE".into(),
        "NB:<includes>:<global>".into(),
        "F:<includes>".into(),
    ));
    for f in &files {
        edges.push((
            "REF".into(),
            format!("NB:{f}:<global>"),
            "NS:<global>".into(),
        ));
        edges.push((
            "SOURCE_FILE".into(),
            format!("NB:{f}:<global>"),
            format!("F:{f}"),
        ));
    }
    // Macro methods: SOURCE_FILE to their defining file and CONTAINS from the
    // file-global TYPE_DECL.
    for full in used_macros.keys() {
        if let Some(file) = full.split(':').next() {
            edges.push((
                "SOURCE_FILE".into(),
                format!("M:{full}"),
                format!("F:{file}"),
            ));
            edges.push((
                "CONTAINS".into(),
                format!("D:{file}:<global>"),
                format!("M:{full}"),
            ));
        }
    }
    // The per-file <global> TYPE_DECL CONTAINS the file-global METHOD and the
    // method TYPE_DECLs of that file; each FILE contains its <global>
    // TYPE_DECL, and <includes> contains its method + external TYPE_DECLs.
    for f in &files {
        edges.push((
            "CONTAINS".into(),
            format!("D:{f}:<global>"),
            format!("M:{f}:<global>"),
        ));
        edges.push((
            "CONTAINS".into(),
            format!("F:{f}"),
            format!("D:{f}:<global>"),
        ));
    }
    for (name, file) in &fn_decls {
        edges.push((
            "CONTAINS".into(),
            format!("D:{file}:<global>"),
            format!("D:{name}"),
        ));
    }
    edges.push((
        "CONTAINS".into(),
        "F:<includes>".into(),
        "M:<includes>:<global>".into(),
    ));
    for t in &used_types {
        if !struct_tags.contains(&t) && !defined.contains(t) {
            edges.push(("CONTAINS".into(), "F:<includes>".into(), format!("D:{t}")));
        }
    }
    // SOURCE_FILE for the TYPE_DECL population.
    for (tag, _, file) in &struct_decls {
        edges.push((
            "SOURCE_FILE".into(),
            format!("TD:{tag}"),
            format!("F:{file}"),
        ));
    }
    for (name, file) in &fn_decls {
        edges.push((
            "SOURCE_FILE".into(),
            format!("D:{name}"),
            format!("F:{file}"),
        ));
    }
    for f in &files {
        edges.push((
            "SOURCE_FILE".into(),
            format!("D:{f}:<global>"),
            format!("F:{f}"),
        ));
    }
    for t in &used_types {
        if !struct_tags.contains(&t) && !defined.contains(t) {
            edges.push((
                "SOURCE_FILE".into(),
                format!("D:{t}"),
                "F:<includes>".into(),
            ));
        }
    }

    // Resolve M:/TD: symbolic addresses to first placement in block-name order.
    let resolve = |a: &str| -> Option<String> {
        if a.starts_with("M:") || a.starts_with("TD:") || a.starts_with("MB:") {
            placements
                .get(a)?
                .iter()
                .min()
                .map(|(blk, idx)| format!("{blk}#{idx}"))
        } else {
            Some(a.to_string())
        }
    };
    let mut edge_lines: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (k, src, dst) in &edges {
        if let (Some(s), Some(d)) = (resolve(src), resolve(dst)) {
            edge_lines.insert(format!("{k} {s} -> {d}"));
        }
    }
    for l in &edge_lines {
        out.push_str(&format!("EDGES|{l}\n"));
    }
    let mut flow_lines: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (var, src, dst) in &flows {
        if let (Some(s), Some(d)) = (resolve(src), resolve(dst)) {
            flow_lines.insert(format!("REACHING_DEF[{var}] {s} -> {d}"));
        }
    }
    for l in &flow_lines {
        out.push_str(&format!("FLOWS|{l}\n"));
    }
    out
}

fn count_members(n: Node) -> i64 {
    let Some(body) = n.child_by_field_name("body") else {
        return 0;
    };
    named_children(body)
        .iter()
        .map(|f| match f.kind() {
            "field_declaration" => named_children(*f)
                .iter()
                .filter(|d| {
                    matches!(
                        d.kind(),
                        "field_identifier" | "pointer_declarator" | "array_declarator"
                    )
                })
                .count() as i64,
            "enumerator" => 1,
            _ => 0,
        })
        .sum()
}

/// An in-file #define. CDT expands these; invocations become INLINED calls
/// and each *used* macro also gets a METHOD whose CODE is the directive.
#[derive(Clone, Debug)]
struct MacroDef {
    params: Option<Vec<String>>, // None = object-like
    body: String,
    directive: String,
    origin: String,
}

/// Preprocess top-level nodes and collect the final active macro environment
/// in one source-ordered pass. Directives in inactive branches never leak,
/// and a later `#undef` cannot retroactively change an earlier branch choice.
fn preprocess_translation_unit<'tree>(
    root: Node<'tree>,
    b: &[u8],
    file: &str,
    environment: &PreprocessorEnvironment<'_>,
) -> (Vec<Node<'tree>>, HashMap<String, MacroDef>) {
    let mut macros = command_line_macros(environment.config, file);
    let mut active = Vec::new();
    let mut include_stack = HashSet::from([normalize_source_path(file)]);
    for forced in &environment.config.forced_includes {
        if let Some(included) = resolve_include(file, forced, false, environment) {
            apply_include_file(&included, environment, &mut macros, &mut include_stack);
        }
    }
    for child in named_children(root) {
        preprocess_child(
            child,
            b,
            file,
            environment,
            &mut macros,
            &mut active,
            &mut include_stack,
        );
    }
    (active, macros)
}

fn preprocess_child<'tree>(
    child: Node<'tree>,
    b: &[u8],
    file: &str,
    environment: &PreprocessorEnvironment<'_>,
    macros: &mut HashMap<String, MacroDef>,
    active: &mut Vec<Node<'tree>>,
    include_stack: &mut HashSet<String>,
) {
    match child.kind() {
        "preproc_def" | "preproc_function_def" => {
            if let Some((name, definition)) = macro_definition(child, b, file) {
                macros.insert(name, definition);
            }
            active.push(child);
        }
        "preproc_include" => {
            if let Some((target, angled)) = include_target(text(child, b)) {
                if let Some(included) = resolve_include(file, &target, angled, environment) {
                    apply_include_file(&included, environment, macros, include_stack);
                }
            }
            active.push(child);
        }
        "preproc_if" | "preproc_ifdef" => {
            for selected in selected_preproc_children(child, b, macros) {
                preprocess_child(
                    selected,
                    b,
                    file,
                    environment,
                    macros,
                    active,
                    include_stack,
                );
            }
        }
        "preproc_call" if text(child, b).trim_start().starts_with("#undef") => {
            if let Some(name) = text(child, b).split_whitespace().nth(1) {
                macros.remove(name);
            }
            active.push(child);
        }
        _ => active.push(child),
    }
}

fn macro_definition(node: Node, b: &[u8], file: &str) -> Option<(String, MacroDef)> {
    let name = node
        .child_by_field_name("name")
        .map(|value| text(value, b).to_string())?;
    let body = node
        .child_by_field_name("value")
        .map(|value| text(value, b).to_string())
        .unwrap_or_default();
    let params = (node.kind() == "preproc_function_def").then(|| {
        node.child_by_field_name("parameters")
            .map(|parameters| {
                let mut names: Vec<String> = named_children(parameters)
                    .iter()
                    .map(|parameter| text(*parameter, b).to_string())
                    .collect();
                // The tree-sitter C grammar keeps `...` anonymous. Model it
                // as the standard replacement parameter so variadic arity,
                // substitution, and macro-method signatures stay explicit.
                if text(parameters, b).contains("...") {
                    names.push("__VA_ARGS__".to_string());
                }
                names
            })
            .unwrap_or_default()
    });
    Some((
        name,
        MacroDef {
            params,
            body,
            directive: text(node, b).trim_end().to_string(),
            origin: normalize_source_path(file),
        },
    ))
}

fn command_line_macros(config: &PreprocessorConfig, file: &str) -> HashMap<String, MacroDef> {
    config
        .defines
        .iter()
        .filter_map(|definition| {
            let (name, body) = definition
                .split_once('=')
                .map_or((definition.as_str(), "1"), |(name, value)| (name, value));
            let name = name.trim();
            (!name.is_empty()).then(|| {
                (
                    name.to_string(),
                    MacroDef {
                        params: None,
                        body: body.to_string(),
                        directive: format!("#define {name} {body}"),
                        origin: normalize_source_path(file),
                    },
                )
            })
        })
        .collect()
}

fn include_target(directive: &str) -> Option<(String, bool)> {
    let directive = directive.trim();
    if let Some(start) = directive.find('"') {
        let rest = &directive[start + 1..];
        return rest.find('"').map(|end| (rest[..end].to_string(), false));
    }
    let start = directive.find('<')?;
    let rest = &directive[start + 1..];
    rest.find('>').map(|end| (rest[..end].to_string(), true))
}

fn resolve_include(
    current_file: &str,
    target: &str,
    angled: bool,
    environment: &PreprocessorEnvironment<'_>,
) -> Option<String> {
    let target = normalize_source_path(target);
    let mut candidates = Vec::new();
    if !angled {
        let parent = Path::new(current_file)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        candidates.push(normalize_source_path(
            &parent.join(&target).to_string_lossy(),
        ));
    }
    candidates.extend(
        environment.config.include_paths.iter().map(|include| {
            normalize_source_path(&Path::new(include).join(&target).to_string_lossy())
        }),
    );
    candidates.push(target);
    candidates
        .into_iter()
        .find(|candidate| environment.sources.contains_key(candidate))
}

fn apply_include_file(
    included: &str,
    environment: &PreprocessorEnvironment<'_>,
    macros: &mut HashMap<String, MacroDef>,
    include_stack: &mut HashSet<String>,
) {
    let included = normalize_source_path(included);
    if !include_stack.insert(included.clone()) {
        return;
    }
    let Some(source) = environment.sources.get(&included).copied() else {
        include_stack.remove(&included);
        return;
    };
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .is_err()
    {
        include_stack.remove(&included);
        return;
    }
    if let Some(tree) = parser.parse(source, None) {
        let mut ignored = Vec::new();
        for child in named_children(tree.root_node()) {
            preprocess_child(
                child,
                source.as_bytes(),
                &included,
                environment,
                macros,
                &mut ignored,
                include_stack,
            );
        }
    }
    include_stack.remove(&included);
}

fn normalize_source_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    let absolute = path.starts_with('/');
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|part| *part != "..") => {
                parts.pop();
            }
            ".." if !absolute => parts.push(part),
            ".." => {}
            _ => parts.push(part),
        }
    }
    let normalized = parts.join("/");
    if absolute {
        format!("/{normalized}")
    } else {
        normalized
    }
}

fn selected_preproc_children<'tree>(
    directive: Node<'tree>,
    b: &[u8],
    macros: &HashMap<String, MacroDef>,
) -> Vec<Node<'tree>> {
    let take =
        match directive.kind() {
            "preproc_if" | "preproc_elif" => directive
                .child_by_field_name("condition")
                .is_some_and(|condition| {
                    eval_pp_node(condition, b, macros, 0, &mut HashSet::new()) != 0
                }),
            "preproc_ifdef" | "preproc_elifdef" => {
                let name = directive
                    .child_by_field_name("name")
                    .map(|value| text(value, b))
                    .unwrap_or_default();
                let negated = directive
                    .child(0)
                    .map(|token| matches!(text(token, b), "#ifndef" | "#elifndef"))
                    .unwrap_or(false);
                macros.contains_key(name) != negated
            }
            "preproc_else" => true,
            _ => false,
        };
    if !take {
        return directive
            .child_by_field_name("alternative")
            .map(|alternative| selected_preproc_children(alternative, b, macros))
            .unwrap_or_default();
    }
    let condition = directive
        .child_by_field_name("condition")
        .map(|node| node.id());
    let name = directive.child_by_field_name("name").map(|node| node.id());
    let alternative = directive
        .child_by_field_name("alternative")
        .map(|node| node.id());
    named_children(directive)
        .into_iter()
        .filter(|child| {
            ![condition, name, alternative]
                .into_iter()
                .flatten()
                .any(|excluded| excluded == child.id())
        })
        .collect()
}

/// Evaluate the integer constant-expression subset accepted by the C
/// preprocessor. Undefined identifiers are zero; object/function macros are
/// recursively expanded with a hard depth/cycle bound.
fn eval_pp_node(
    node: Node,
    b: &[u8],
    macros: &HashMap<String, MacroDef>,
    depth: usize,
    expanding: &mut HashSet<String>,
) -> i64 {
    if depth >= 64 {
        return 0;
    }
    match node.kind() {
        "number_literal" => parse_pp_integer(text(node, b)).unwrap_or(0),
        "char_literal" => parse_pp_char(text(node, b)),
        "identifier" => {
            let name = text(node, b);
            let Some(definition) = macros
                .get(name)
                .filter(|definition| definition.params.is_none())
            else {
                return 0;
            };
            if !expanding.insert(name.to_string()) {
                return 0;
            }
            let value = if definition.body.trim().is_empty() {
                1
            } else {
                eval_pp_text(&definition.body, macros, depth + 1, expanding)
            };
            expanding.remove(name);
            value
        }
        "preproc_defined" => named_children(node)
            .first()
            .map(|name| i64::from(macros.contains_key(text(*name, b))))
            .unwrap_or(0),
        "parenthesized_expression" => named_children(node)
            .first()
            .map(|child| eval_pp_node(*child, b, macros, depth + 1, expanding))
            .unwrap_or(0),
        "unary_expression" => {
            let Some(argument) = node
                .child_by_field_name("argument")
                .or_else(|| named_children(node).last().copied())
            else {
                return 0;
            };
            let value = eval_pp_node(argument, b, macros, depth + 1, expanding);
            let operator = text(node, b)[..argument.start_byte() - node.start_byte()].trim();
            match operator {
                "!" => i64::from(value == 0),
                "~" => !value,
                "-" => value.wrapping_neg(),
                "+" => value,
                _ => 0,
            }
        }
        "binary_expression" => {
            let (Some(left), Some(right)) = (
                node.child_by_field_name("left"),
                node.child_by_field_name("right"),
            ) else {
                return 0;
            };
            let lhs = eval_pp_node(left, b, macros, depth + 1, expanding);
            let operator = text(node, b)
                [left.end_byte() - node.start_byte()..right.start_byte() - node.start_byte()]
                .trim();
            if operator == "&&" && lhs == 0 {
                return 0;
            }
            if operator == "||" && lhs != 0 {
                return 1;
            }
            let rhs = eval_pp_node(right, b, macros, depth + 1, expanding);
            match operator {
                "*" => lhs.wrapping_mul(rhs),
                "/" => lhs.checked_div(rhs).unwrap_or(0),
                "%" => lhs.checked_rem(rhs).unwrap_or(0),
                "+" => lhs.wrapping_add(rhs),
                "-" => lhs.wrapping_sub(rhs),
                "<<" => lhs.wrapping_shl(rhs.clamp(0, 63) as u32),
                ">>" => lhs.wrapping_shr(rhs.clamp(0, 63) as u32),
                "<" => i64::from(lhs < rhs),
                "<=" => i64::from(lhs <= rhs),
                ">" => i64::from(lhs > rhs),
                ">=" => i64::from(lhs >= rhs),
                "==" => i64::from(lhs == rhs),
                "!=" => i64::from(lhs != rhs),
                "&" => lhs & rhs,
                "^" => lhs ^ rhs,
                "|" => lhs | rhs,
                "&&" => i64::from(lhs != 0 && rhs != 0),
                "||" => i64::from(lhs != 0 || rhs != 0),
                _ => 0,
            }
        }
        "call_expression" => {
            let Some(function) = node.child_by_field_name("function") else {
                return 0;
            };
            let name = text(function, b);
            let Some(definition) = macros.get(name) else {
                return 0;
            };
            let Some(parameters) = definition.params.as_ref() else {
                return 0;
            };
            let arguments: Vec<String> = node
                .child_by_field_name("arguments")
                .map(named_children)
                .unwrap_or_default()
                .iter()
                .map(|argument| text(*argument, b).to_string())
                .collect();
            if !expanding.insert(name.to_string()) {
                return 0;
            }
            let expanded = substitute(&definition.body, parameters, &arguments);
            let value = eval_pp_text(&expanded, macros, depth + 1, expanding);
            expanding.remove(name);
            value
        }
        _ => 0,
    }
}

fn eval_pp_text(
    expression: &str,
    macros: &HashMap<String, MacroDef>,
    depth: usize,
    expanding: &mut HashSet<String>,
) -> i64 {
    if depth >= 64 {
        return 0;
    }
    let source = format!("#if {expression}\nint __cpg_pp;\n#endif\n");
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .is_err()
    {
        return 0;
    }
    let Some(tree) = parser.parse(&source, None) else {
        return 0;
    };
    let Some(directive) = named_children(tree.root_node())
        .into_iter()
        .find(|child| child.kind() == "preproc_if")
    else {
        return 0;
    };
    directive
        .child_by_field_name("condition")
        .map(|condition| eval_pp_node(condition, source.as_bytes(), macros, depth + 1, expanding))
        .unwrap_or(0)
}

fn parse_pp_integer(token: &str) -> Option<i64> {
    let token = token.trim();
    let end = token
        .char_indices()
        .rev()
        .find(|(_, ch)| !matches!(ch, 'u' | 'U' | 'l' | 'L'))
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    let token = &token[..end];
    if let Some(value) = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))
    {
        i64::from_str_radix(value, 16).ok()
    } else if let Some(value) = token
        .strip_prefix("0b")
        .or_else(|| token.strip_prefix("0B"))
    {
        i64::from_str_radix(value, 2).ok()
    } else if token.len() > 1 && token.starts_with('0') {
        i64::from_str_radix(&token[1..], 8).ok()
    } else {
        token.parse().ok()
    }
}

fn parse_pp_char(token: &str) -> i64 {
    let Some(inner) = token
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
    else {
        return 0;
    };
    match inner {
        "\\n" => '\n' as i64,
        "\\r" => '\r' as i64,
        "\\t" => '\t' as i64,
        "\\0" => 0,
        "\\\\" => '\\' as i64,
        "\\'" => '\'' as i64,
        value => value.chars().next().map(|value| value as i64).unwrap_or(0),
    }
}

/// Per-function emission context.
struct Ctx<'a> {
    functions: &'a HashMap<String, String>,
    /// Translation-unit-local method identities. Duplicate C names are valid
    /// for `static` helpers across files; qualify those identities by file so
    /// they remain distinct and local calls resolve deterministically.
    function_full_names: &'a HashMap<String, String>,
    /// Project-wide duplicate definition names. A call without a local target
    /// stays unresolved instead of acquiring an arbitrary first candidate.
    ambiguous_functions: &'a HashSet<String>,
    globals: &'a HashMap<String, String>,
    enumerators: &'a Vec<String>,
    macros: &'a HashMap<String, MacroDef>,
    preprocessor: &'a PreprocessorEnvironment<'a>,
    // used macros: full_name -> (name, directive, nparams, ret type)
    used_macros: &'a mut std::collections::BTreeMap<String, (String, String, usize, String)>,
    symbols: HashMap<String, String>, // local/param name -> type
    /// Macro names disabled while their replacement list is being lowered.
    /// The C preprocessor leaves a recursively encountered name unexpanded;
    /// carrying that state into AST emission prevents recursive macro pairs
    /// from re-entering `emit_macro_call` until the process stack overflows.
    expanding_macros: HashSet<String>,
    // Joern's local-creation pass materialises a LOCAL at ORDER=0 atop the
    // method body BLOCK for each referenced global (CODE `<global> name`)
    // and each type name used as a sizeof(T) argument.
    phantoms: Vec<Phantom>,
    stubs: &'a mut HashMap<String, usize>,
    types: &'a mut std::collections::BTreeSet<String>,
    out: String,
    // --- edge layer (M4) ---
    // Current dump block name and 0-based line index within it; every node's
    // address is "<block>#<idx>". METHOD/TYPE_DECL nodes are addressed
    // symbolically ("M:<full>"/"TD:<full>") and resolved at the end to the
    // first placement in block-name sort order, replicating the oracle's
    // first-wins assignment across sorted method walks.
    block: String,
    line_no: usize,
    suppress_below: Option<usize>, // depth of a nested METHOD whose interior is foreign
    ctx_stack: Vec<(usize, String)>, // CONTAINS contexts: (depth, src addr)
    parent_stack: Vec<(usize, String, String, bool)>, // (depth, label, addr, inlined)
    sym_line: HashMap<String, usize>, // LOCAL/param name -> defining line idx
    param_in_line: HashMap<String, usize>,
    edges: &'a mut Vec<(String, String, String)>,
    placements: &'a mut HashMap<String, Vec<(String, usize)>>,
}

/// AST labels that receive a CONTAINS edge from their enclosing method or
/// type-decl context (Joern's ContainsEdgePass destination list — note that
/// LOCAL, parameters, METHOD_RETURN, MODIFIER and MEMBER are absent).
const CONTAINS_DST: [&str; 13] = [
    "BLOCK",
    "IDENTIFIER",
    "FIELD_IDENTIFIER",
    "RETURN",
    "METHOD",
    "TYPE_DECL",
    "CALL",
    "LITERAL",
    "METHOD_REF",
    "TYPE_REF",
    "CONTROL_STRUCTURE",
    "JUMP_TARGET",
    "UNKNOWN",
];

struct Phantom {
    name: String,
    code: String,
    ty: String,
}

/// Properties in Joern's canonical print order; only `Some` fields are emitted.
#[derive(Default)]
struct P {
    name: Option<String>,
    code: Option<String>,
    tfn: Option<String>,
    full: Option<String>,
    mfn: Option<String>,
    sig: Option<String>,
    order: Option<i64>,
    arg: Option<i64>,
    dispatch: Option<String>,
}

impl Ctx<'_> {
    fn begin_block(&mut self, name: &str) {
        self.block = name.to_string();
        self.line_no = 0;
        self.suppress_below = None;
        self.ctx_stack.clear();
        self.parent_stack.clear();
        self.sym_line.clear();
        self.param_in_line.clear();
    }

    fn at(&self, idx: usize) -> String {
        format!("{}#{}", self.block, idx)
    }

    fn edge(&mut self, kind: &str, src: String, dst: String) {
        if self.suppress_below.is_none() {
            self.edges.push((kind.to_string(), src, dst));
        }
    }

    fn line(&mut self, depth: usize, label: &str, p: P) {
        if let Some(t) = &p.tfn {
            self.types.insert(t.clone());
        }
        // -- addressing & suppression --
        let suppressed = self.suppress_below.is_some_and(|nd| depth > nd);
        if !suppressed {
            self.suppress_below = None;
        }
        let idx = self.line_no;
        let my_addr = match (label, &p.full) {
            ("METHOD", Some(f)) => format!("M:{f}"),
            ("TYPE_DECL", Some(f)) => format!("TD:{f}"),
            _ => format!("{}#{}", self.block, idx),
        };
        while self.parent_stack.last().is_some_and(|t| t.0 >= depth) {
            self.parent_stack.pop();
        }
        while self.ctx_stack.last().is_some_and(|t| t.0 >= depth) {
            self.ctx_stack.pop();
        }
        if !suppressed {
            if let ("METHOD" | "TYPE_DECL", Some(f)) = (label, &p.full) {
                let key = if label == "METHOD" {
                    format!("M:{f}")
                } else {
                    format!("TD:{f}")
                };
                self.placements
                    .entry(key)
                    .or_default()
                    .push((self.block.clone(), idx));
            }
            if CONTAINS_DST.contains(&label) {
                if let Some((_, src)) = self.ctx_stack.last() {
                    let src = src.clone();
                    self.edges.push(("CONTAINS".into(), src, my_addr.clone()));
                }
            }
            if let Some((_, plabel, paddr, pinlined)) = self.parent_stack.last() {
                // Receivers (no ARGUMENT_INDEX) and the expansion BLOCK of an
                // INLINED macro call get no ARGUMENT edge.
                let is_expansion = *pinlined && label == "BLOCK";
                if (plabel == "CALL" && p.arg.is_some() && !is_expansion) || plabel == "RETURN" {
                    let paddr = paddr.clone();
                    self.edges.push(("ARGUMENT".into(), paddr, my_addr.clone()));
                }
            }
            if let Some(t) = &p.tfn {
                self.edges
                    .push(("EVAL_TYPE".into(), my_addr.clone(), format!("T:{t}")));
            }
            if label == "CALL" && p.dispatch.as_deref() != Some("DYNAMIC_DISPATCH") {
                if let Some(mfn) = &p.mfn {
                    if !self.ambiguous_functions.contains(mfn) || mfn.contains(':') {
                        self.edges
                            .push(("CALL".into(), my_addr.clone(), format!("M:{mfn}")));
                    }
                }
            }
            if label == "METHOD_REF" {
                if let Some(mfn) = &p.mfn {
                    self.edges
                        .push(("REF".into(), my_addr.clone(), format!("M:{mfn}")));
                }
            }
            // CDT quirk: the *arguments* of an INLINED macro call carry no
            // REF edge (identifiers inside the expansion do).
            let under_inlined_call = self
                .parent_stack
                .last()
                .is_some_and(|(_, pl, _, pi)| *pi && pl == "CALL");
            if label == "IDENTIFIER" && !under_inlined_call {
                if let Some(n) = &p.name {
                    if let Some(i) = self.sym_line.get(n) {
                        let dst = format!("{}#{}", self.block, i);
                        self.edges.push(("REF".into(), my_addr.clone(), dst));
                    }
                }
            }
            if let ("LOCAL" | "METHOD_PARAMETER_IN", Some(n)) = (label, &p.name) {
                self.sym_line.insert(n.clone(), idx);
            }
            if let ("METHOD_PARAMETER_IN", Some(n)) = (label, &p.name) {
                self.param_in_line.insert(n.clone(), idx);
            }
            if let ("METHOD_PARAMETER_OUT", Some(n)) = (label, &p.name) {
                if let Some(i) = self.param_in_line.get(n) {
                    let src = format!("{}#{}", self.block, i);
                    self.edges
                        .push(("PARAMETER_LINK".into(), src, my_addr.clone()));
                }
            }
        }
        if label == "METHOD" || label == "TYPE_DECL" {
            self.ctx_stack.push((depth, my_addr.clone()));
        }
        let inlined = p.dispatch.as_deref() == Some("INLINED");
        self.parent_stack
            .push((depth, label.to_string(), my_addr, inlined));
        self.line_no += 1;
        let mut s = format!("{}{label}", "  ".repeat(depth));
        let mut kv = |k: &str, v: &str| s.push_str(&format!(" {k}={v}"));
        if let Some(v) = &p.name {
            kv("NAME", v);
        }
        if let Some(v) = &p.code {
            // Keep the neutral transport one-node-per-line even for real
            // project constructs whose tree-sitter span crosses physical
            // lines (multiline declarations and macro arguments). Most
            // emitters already escape eagerly for Joern formatting; doing it
            // at the single output boundary closes every remaining path and
            // is idempotent for the existing corpus.
            kv("CODE", &esc(v));
        }
        if let Some(v) = &p.tfn {
            kv("TYPE_FULL_NAME", v);
        }
        if let Some(v) = &p.full {
            kv("FULL_NAME", v);
        }
        if let Some(v) = &p.mfn {
            kv("METHOD_FULL_NAME", v);
        }
        if let Some(v) = &p.sig {
            kv("SIGNATURE", v);
        }
        if let Some(v) = p.order {
            kv("ORDER", &v.to_string());
        }
        if let Some(v) = p.arg {
            kv("ARGUMENT_INDEX", &v.to_string());
        }
        if let Some(v) = &p.dispatch {
            kv("DISPATCH_TYPE", v);
        }
        self.out.push_str(&s);
        self.out.push('\n');
    }

    fn note_call(&mut self, name: &str, argc: usize) {
        let e = self.stubs.entry(name.to_string()).or_insert(0);
        if argc > *e {
            *e = argc;
        }
    }

    fn emit_method(&mut self, f: Node, b: &[u8], d: usize) {
        self.symbols.clear();
        let (name, ret, params) = fn_header(f, b).expect("function header");
        let full = self
            .function_full_names
            .get(&name)
            .cloned()
            .unwrap_or_else(|| name.clone());
        let nested = d > 0;
        let sig = format!(
            "{ret}({})",
            params
                .iter()
                .map(|p| p.ty.clone())
                .collect::<Vec<_>>()
                .join(",")
        );
        self.line(
            d,
            "METHOD",
            P {
                name: Some(name.clone()),
                code: Some(esc(text(f, b))),
                full: Some(full),
                sig: Some(sig),
                order: Some(1),
                ..Default::default()
            },
        );
        if nested {
            // A nested method's interior is addressed (and produces edges) in
            // its own standalone walk; only the METHOD line itself belongs here.
            self.suppress_below = Some(d);
        }

        // Parameters: a METHOD_PARAMETER_IN and a mirrored _OUT, sharing ORDER.
        for (i, p) in params.iter().enumerate() {
            self.symbols.insert(p.name.clone(), p.ty.clone());
            let order = (i + 1) as i64;
            for label in ["METHOD_PARAMETER_IN", "METHOD_PARAMETER_OUT"] {
                self.line(
                    d + 1,
                    label,
                    P {
                        name: Some(p.name.clone()),
                        code: Some(esc(&p.code)),
                        tfn: Some(p.ty.clone()),
                        order: Some(order),
                        ..Default::default()
                    },
                );
            }
        }

        // Body block, then the synthetic method return.
        let block_order = (params.len() + 1) as i64;
        if let Some(body) = f.child_by_field_name("body") {
            self.collect_phantoms(body, b);
            self.emit_block(body, b, block_order, d + 1);
        }
        let static_modifier = named_children(f).iter().any(|child| {
            child.kind() == "storage_class_specifier" && text(*child, b).trim() == "static"
        });
        if static_modifier {
            self.line(
                d + 1,
                "MODIFIER",
                P {
                    order: Some((params.len() + 2) as i64),
                    ..Default::default()
                },
            );
        }
        self.line(
            d + 1,
            "METHOD_RETURN",
            P {
                code: Some("RET".into()),
                tfn: Some(ret),
                order: Some((params.len() + 2 + usize::from(static_modifier)) as i64),
                ..Default::default()
            },
        );
    }

    /// A `<operator>.*` stub method. Layout mirrors Joern's stable sort by
    /// ORDER over insertion order [IN p1..pn, BLOCK(1), RET(2), OUT p1..pn].
    fn emit_stub(&mut self, name: &str, arity: usize) {
        self.line(
            0,
            "METHOD",
            P {
                name: Some(name.into()),
                full: Some(name.into()),
                order: Some(0),
                ..Default::default()
            },
        );
        let pin = |k: usize| P {
            name: Some(format!("p{k}")),
            code: Some(format!("p{k}")),
            tfn: Some("ANY".into()),
            order: Some(k as i64),
            ..Default::default()
        };
        let first = if arity == 0 { 0 } else { 1 };
        self.line(1, "METHOD_PARAMETER_IN", pin(first));
        if arity == 0 {
            self.line(1, "METHOD_PARAMETER_OUT", pin(first));
        }
        self.line(
            1,
            "BLOCK",
            P {
                tfn: Some("ANY".into()),
                order: Some(1),
                arg: Some(1),
                ..Default::default()
            },
        );
        if arity != 0 {
            self.line(1, "METHOD_PARAMETER_OUT", pin(first));
        }
        let ret = P {
            code: Some("RET".into()),
            tfn: Some("ANY".into()),
            order: Some(2),
            ..Default::default()
        };
        if arity >= 2 {
            self.line(1, "METHOD_PARAMETER_IN", pin(2));
            self.line(1, "METHOD_RETURN", ret);
            self.line(1, "METHOD_PARAMETER_OUT", pin(2));
            for k in 3..=arity {
                self.line(1, "METHOD_PARAMETER_IN", pin(k));
                self.line(1, "METHOD_PARAMETER_OUT", pin(k));
            }
        } else {
            self.line(1, "METHOD_RETURN", ret);
        }
    }

    fn emit_includes_global(&mut self) {
        self.line(
            0,
            "METHOD",
            P {
                name: Some("<global>".into()),
                code: Some("<global>".into()),
                full: Some("<includes>:<global>".into()),
                order: Some(1),
                ..Default::default()
            },
        );
        self.line(
            1,
            "BLOCK",
            P {
                tfn: Some("ANY".into()),
                order: Some(1),
                ..Default::default()
            },
        );
        self.line(
            1,
            "METHOD_RETURN",
            P {
                code: Some("RET".into()),
                tfn: Some("ANY".into()),
                order: Some(2),
                ..Default::default()
            },
        );
    }

    /// The per-file `<global>` wrapper: TYPE_DECLs and nested METHOD dumps in
    /// source order (each ORDER=1), then a BLOCK holding one slot per
    /// top-level construct in source order — TYPE_REF for a struct def, LOCAL
    /// per global object declarator, METHOD_REF per function definition;
    /// prototypes consume no slot — then METHOD_RETURN.
    fn emit_file_global(&mut self, root: Node, b: &[u8], file: &str) {
        self.line(
            0,
            "METHOD",
            P {
                name: Some("<global>".into()),
                code: Some("<global>".into()),
                full: Some(format!("{file}:<global>")),
                order: Some(1),
                ..Default::default()
            },
        );
        let (top_level, _) = preprocess_translation_unit(root, b, file, self.preprocessor);
        for &n in &top_level {
            match n.kind() {
                "struct_specifier" | "union_specifier" | "enum_specifier"
                    if n.child_by_field_name("body").is_some() =>
                {
                    self.emit_type_decl(n, b, 1);
                }
                "function_definition" => self.emit_method(n, b, 1),
                _ => {}
            }
        }
        self.line(
            1,
            "BLOCK",
            P {
                tfn: Some("ANY".into()),
                order: Some(1),
                ..Default::default()
            },
        );
        let mut slot = 1i64;
        for &n in &top_level {
            match n.kind() {
                "struct_specifier" | "union_specifier" | "enum_specifier"
                    if n.child_by_field_name("body").is_some() =>
                {
                    let name = n
                        .child_by_field_name("name")
                        .map(|x| text(x, b).to_string())
                        .unwrap_or_default();
                    self.line(
                        2,
                        "TYPE_REF",
                        P {
                            code: Some(esc(text(n, b))),
                            tfn: Some(name),
                            order: Some(slot),
                            ..Default::default()
                        },
                    );
                    slot += 1;
                }
                "type_definition" => {
                    // typedef: a TYPE_DECL *inside* the global BLOCK, CODE
                    // keeps the whole statement incl. the semicolon.
                    let name = n
                        .child_by_field_name("declarator")
                        .map(|x| text(x, b).to_string())
                        .unwrap_or_default();
                    self.line(
                        2,
                        "TYPE_DECL",
                        P {
                            name: Some(name.clone()),
                            code: Some(esc(text(n, b))),
                            full: Some(name),
                            order: Some(slot),
                            ..Default::default()
                        },
                    );
                    slot += 1;
                }
                "declaration" => {
                    // Same lowering as in a method body: LOCAL per declarator,
                    // plus an assignment CALL when initialised (`int g = 5;`).
                    // Prototypes contribute nothing and consume no slot.
                    if named_children(n)
                        .iter()
                        .any(|d| find_function_declarator(*d).is_some())
                    {
                        continue;
                    }
                    self.emit_declaration(n, b, &mut slot, 2, None);
                }
                "function_definition" => {
                    if let Some((name, _, _)) = fn_header(n, b) {
                        let full = self
                            .function_full_names
                            .get(&name)
                            .cloned()
                            .unwrap_or_else(|| name.clone());
                        self.line(
                            2,
                            "METHOD_REF",
                            P {
                                code: Some(name.clone()),
                                tfn: Some(full.clone()),
                                mfn: Some(full),
                                order: Some(slot),
                                ..Default::default()
                            },
                        );
                        slot += 1;
                    }
                }
                _ => {}
            }
        }
        self.line(
            1,
            "METHOD_RETURN",
            P {
                code: Some("RET".into()),
                tfn: Some("ANY".into()),
                order: Some(2),
                ..Default::default()
            },
        );
    }

    /// `struct T { ... }` → TYPE_DECL with one MEMBER per field (CODE is the
    /// member's declarator text: `x`, `*ptr`, `arr[4]`). If any member is a
    /// sized array, a `<clinit>` method follows the members to host the
    /// `<operator>.arrayInitializer` calls.
    fn emit_type_decl(&mut self, n: Node, b: &[u8], depth: usize) {
        let name = n
            .child_by_field_name("name")
            .map(|x| text(x, b).to_string())
            .unwrap_or_default();
        self.line(
            depth,
            "TYPE_DECL",
            P {
                name: Some(name.clone()),
                code: Some(esc(text(n, b))),
                full: Some(name.clone()),
                order: Some(1),
                ..Default::default()
            },
        );
        let Some(body) = n.child_by_field_name("body") else {
            return;
        };
        let mut order = 1i64;
        // enum: one ANY-typed MEMBER per enumerator (CODE keeps `GREEN = 5`).
        for e in named_children(body) {
            if e.kind() != "enumerator" {
                continue;
            }
            let ename = e
                .child_by_field_name("name")
                .map(|x| text(x, b).to_string())
                .unwrap_or_default();
            self.line(
                depth + 1,
                "MEMBER",
                P {
                    name: Some(ename),
                    code: Some(esc(text(e, b))),
                    tfn: Some("ANY".into()),
                    order: Some(order),
                    ..Default::default()
                },
            );
            order += 1;
        }
        for f in named_children(body) {
            if f.kind() != "field_declaration" {
                continue;
            }
            let ty = normalize_type(
                &f.child_by_field_name("type")
                    .map(|t| text(t, b).to_string())
                    .unwrap_or("ANY".into()),
            );
            for d in named_children(f) {
                if matches!(
                    d.kind(),
                    "field_identifier" | "pointer_declarator" | "array_declarator"
                ) {
                    let mname = innermost_id(d, b);
                    let key = format!("MB:{name}.{mname}");
                    let placement = (self.block.clone(), self.line_no);
                    self.placements.entry(key).or_default().push(placement);
                    self.line(
                        depth + 1,
                        "MEMBER",
                        P {
                            name: Some(mname),
                            code: Some(text(d, b).to_string()),
                            tfn: Some(format!("{ty}{}", decl_suffix(d, b))),
                            order: Some(order),
                            ..Default::default()
                        },
                    );
                    order += 1;
                }
            }
        }
        if needs_clinit(n, b) {
            self.emit_clinit(n, b, depth + 1, order);
        }
    }

    /// The synthetic `<clinit>` static initialiser Joern adds to a struct with
    /// sized-array members: a property-less BLOCK holding one
    /// `<operator>.arrayInitializer` call per sized array, two bare MODIFIERs,
    /// and a METHOD_RETURN typed as the struct.
    fn emit_clinit(&mut self, n: Node, b: &[u8], depth: usize, order: i64) {
        let tag = n
            .child_by_field_name("name")
            .map(|x| text(x, b).to_string())
            .unwrap_or_default();
        self.line(
            depth,
            "METHOD",
            P {
                name: Some("<clinit>".into()),
                code: Some("<clinit>".into()),
                full: Some(format!("{tag}.<clinit>:{tag}()")),
                order: Some(order),
                ..Default::default()
            },
        );
        if depth > 0 {
            self.suppress_below = Some(depth);
        }
        self.line(
            depth + 1,
            "BLOCK",
            P {
                order: Some(1),
                ..Default::default()
            },
        );
        let mut co = 1i64;
        if let Some(body) = n.child_by_field_name("body") {
            // enum mode: phantom ANY LOCALs (ORDER=0) for the initialised
            // enumerators, then one void assignment per initialiser.
            let inits: Vec<Node> = named_children(body)
                .into_iter()
                .filter(|e| e.kind() == "enumerator" && e.child_by_field_name("value").is_some())
                .collect();
            for e in &inits {
                let ename = e
                    .child_by_field_name("name")
                    .map(|x| text(x, b).to_string())
                    .unwrap_or_default();
                self.line(
                    depth + 2,
                    "LOCAL",
                    P {
                        name: Some(ename.clone()),
                        code: Some(ename),
                        tfn: Some("ANY".into()),
                        order: Some(0),
                        ..Default::default()
                    },
                );
            }
            for e in &inits {
                let ename = e
                    .child_by_field_name("name")
                    .map(|x| text(x, b).to_string())
                    .unwrap_or_default();
                self.note_call("<operator>.assignment", 2);
                self.line(
                    depth + 2,
                    "CALL",
                    P {
                        name: Some("<operator>.assignment".into()),
                        code: Some(esc(text(*e, b))),
                        tfn: Some("void".into()),
                        mfn: Some("<operator>.assignment".into()),
                        order: Some(co),
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                self.line(
                    depth + 3,
                    "IDENTIFIER",
                    P {
                        name: Some(ename.clone()),
                        code: Some(ename),
                        tfn: Some("ANY".into()),
                        order: Some(1),
                        arg: Some(1),
                        ..Default::default()
                    },
                );
                if let Some(v) = e.child_by_field_name("value") {
                    self.emit_expr(v, b, depth + 3, 2, Some(2));
                }
                co += 1;
            }
            for f in named_children(body) {
                if f.kind() != "field_declaration" {
                    continue;
                }
                for d in named_children(f) {
                    if d.kind() == "array_declarator" {
                        if let Some(sz) = d.child_by_field_name("size") {
                            self.note_call("<operator>.arrayInitializer", 1);
                            self.line(
                                depth + 2,
                                "CALL",
                                P {
                                    name: Some("<operator>.arrayInitializer".into()),
                                    code: Some(esc(text(d, b))),
                                    tfn: Some("ANY".into()),
                                    mfn: Some("<operator>.arrayInitializer".into()),
                                    order: Some(co),
                                    dispatch: Some("STATIC_DISPATCH".into()),
                                    ..Default::default()
                                },
                            );
                            self.emit_expr(sz, b, depth + 3, 1, Some(1));
                            co += 1;
                        }
                    }
                }
            }
        }
        self.line(
            depth + 1,
            "MODIFIER",
            P {
                order: Some(2),
                ..Default::default()
            },
        );
        self.line(
            depth + 1,
            "MODIFIER",
            P {
                order: Some(3),
                ..Default::default()
            },
        );
        self.line(
            depth + 1,
            "METHOD_RETURN",
            P {
                code: Some("RET".into()),
                tfn: Some(tag),
                order: Some(4),
                ..Default::default()
            },
        );
    }

    /// An INLINED macro invocation: CALL (NAME = macro, CODE = the original
    /// invocation text, MFN = <file>:<name>:<ret>(<nparams>), SIGNATURE too),
    /// arguments in order, then a BLOCK (ANY, ORDER/INDEX = n+1) wrapping the
    /// expansion parsed from the substituted macro body.
    #[allow(clippy::too_many_arguments)]
    fn emit_macro_call(
        &mut self,
        name: &str,
        code: &str,
        arg_nodes: &[Node],
        _site: Node,
        b: &[u8],
        depth: usize,
        order: i64,
        arg: Option<i64>,
    ) {
        let (params, body, directive) = {
            let m = &self.macros[name];
            (
                m.params.clone().unwrap_or_default(),
                m.body.clone(),
                m.directive.clone(),
            )
        };
        let arg_texts: Vec<String> = arg_nodes.iter().map(|a| text(*a, b).to_string()).collect();
        let substituted = substitute(&body, &params, &arg_texts);
        let mut expanding = HashSet::from([name.to_string()]);
        let expansion = expand_macros_text(&substituted, self.macros, 0, &mut expanding);
        let ret = expansion_type(&expansion, &self.symbols, self.functions);
        let origin = &self.macros[name].origin;
        let full = format!("{origin}:{name}:{ret}({})", params.len());
        self.used_macros.entry(full.clone()).or_insert((
            name.to_string(),
            directive,
            params.len(),
            ret.clone(),
        ));
        self.line(
            depth,
            "CALL",
            P {
                name: Some(name.to_string()),
                code: Some(code.to_string()),
                tfn: Some(ret),
                mfn: Some(full),
                sig: Some(format!(
                    "{}({})",
                    expansion_type(&expansion, &self.symbols, self.functions),
                    params.len()
                )),
                order: Some(order),
                arg,
                dispatch: Some("INLINED".into()),
                ..Default::default()
            },
        );
        let ordinary = ordinary_macro_parameters(&body, &params);
        let fixed_count = params
            .iter()
            .position(|param| param == "__VA_ARGS__")
            .unwrap_or(params.len());
        let mut visible_args = Vec::new();
        for (index, used) in ordinary.into_iter().enumerate() {
            if !used {
                continue;
            }
            if params.get(index).is_some_and(|p| p == "__VA_ARGS__") {
                visible_args.extend(arg_nodes.iter().skip(fixed_count).copied());
            } else if let Some(argument) = arg_nodes.get(index) {
                visible_args.push(*argument);
            }
        }
        for (i, a) in visible_args.iter().enumerate() {
            let k = (i + 1) as i64;
            self.emit_expr(*a, b, depth + 1, k, Some(k));
        }
        let bk = (visible_args.len() + 1) as i64;
        self.line(
            depth + 1,
            "BLOCK",
            P {
                tfn: Some("ANY".into()),
                order: Some(bk),
                arg: Some(bk),
                ..Default::default()
            },
        );
        self.expanding_macros.insert(name.to_string());
        self.emit_expansion(&expansion, depth + 2);
        self.expanding_macros.remove(name);
    }

    /// Parse a macro expansion as an expression and emit it (ORDER=1, no
    /// ARGUMENT_INDEX), exactly as CDT inlines it.
    fn emit_expansion(&mut self, expansion: &str, depth: usize) {
        let src = format!("void __m() {{ {expansion}; }}");
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_c::LANGUAGE.into())
            .unwrap();
        let Some(tree) = parser.parse(&src, None) else {
            return;
        };
        let b = src.as_bytes();
        let Some(expr) = expansion_expr_node(tree.root_node()) else {
            return;
        };
        self.emit_expr(expr, b, depth, 1, None);
    }

    /// METHOD for a used macro: CODE is the #define directive, params p1..pn,
    /// an empty ANY BLOCK after the params (no ARGUMENT_INDEX), RET typed as
    /// the expansion.
    fn emit_macro_method(
        &mut self,
        full: &str,
        name: &str,
        directive: &str,
        nparams: usize,
        ret: &str,
    ) {
        self.line(
            0,
            "METHOD",
            P {
                name: Some(name.to_string()),
                code: Some(esc(directive)),
                full: Some(full.to_string()),
                sig: Some(format!("{ret}({nparams})")),
                order: Some(1),
                ..Default::default()
            },
        );
        for k in 1..=nparams {
            let pk = P {
                name: Some(format!("p{k}")),
                code: Some(format!("p{k}")),
                tfn: Some("ANY".into()),
                order: Some(k as i64),
                ..Default::default()
            };
            self.line(1, "METHOD_PARAMETER_IN", P { ..pk });
            self.line(
                1,
                "METHOD_PARAMETER_OUT",
                P {
                    name: Some(format!("p{k}")),
                    code: Some(format!("p{k}")),
                    tfn: Some("ANY".into()),
                    order: Some(k as i64),
                    ..Default::default()
                },
            );
        }
        self.line(
            1,
            "BLOCK",
            P {
                tfn: Some("ANY".into()),
                order: Some((nparams + 1) as i64),
                ..Default::default()
            },
        );
        self.line(
            1,
            "METHOD_RETURN",
            P {
                code: Some("RET".into()),
                tfn: Some(ret.to_string()),
                order: Some((nparams + 2) as i64),
                ..Default::default()
            },
        );
    }

    /// Pre-scan the method body for names that Joern's local-creation pass
    /// materialises as ORDER=0 LOCALs: referenced globals (unless shadowed by
    /// a param or body declaration) and sizeof(T) type names.
    fn collect_phantoms(&mut self, body: Node, b: &[u8]) {
        let mut shadowed: Vec<String> = self.symbols.keys().cloned().collect();
        collect_decl_names(body, b, &mut shadowed);
        let mut seen: Vec<String> = Vec::new();
        let mut stack = vec![body];
        while let Some(n) = stack.pop() {
            match n.kind() {
                "identifier" => {
                    let name = text(n, b).to_string();
                    if !shadowed.contains(&name) && !seen.contains(&name) {
                        if let Some(ty) = self.globals.get(&name) {
                            seen.push(name.clone());
                            self.phantoms.push(Phantom {
                                code: format!("<global> {name}"),
                                ty: ty.clone(),
                                name,
                            });
                        } else if self.enumerators.contains(&name) {
                            // Enumerators phantom like globals, but plain CODE
                            // and type ANY.
                            seen.push(name.clone());
                            self.phantoms.push(Phantom {
                                code: name.clone(),
                                ty: "ANY".into(),
                                name,
                            });
                        } else if !self.macros.contains_key(&name)
                            && !self.functions.contains_key(&name)
                        {
                            // Fully unresolved identifier: phantom LOCAL with
                            // CODE `<unknown> name` (e.g. NULL).
                            seen.push(name.clone());
                            self.phantoms.push(Phantom {
                                code: format!("<unknown> {name}"),
                                ty: "ANY".into(),
                                name,
                            });
                        }
                    }
                }
                "null" => {
                    if !seen.contains(&"NULL".to_string()) {
                        seen.push("NULL".into());
                        self.phantoms.push(Phantom {
                            name: "NULL".into(),
                            code: "<unknown> NULL".into(),
                            ty: "ANY".into(),
                        });
                    }
                }
                "sizeof_expression" => {
                    if let Some(t) = n.child_by_field_name("type") {
                        let code = text(t, b).to_string();
                        let name = normalize_type(&code);
                        if !seen.contains(&name) {
                            seen.push(name.clone());
                            self.phantoms.push(Phantom {
                                name: name.clone(),
                                code,
                                ty: name,
                            });
                        }
                    }
                }
                "call_expression" => {
                    if let Some(function) = n.child_by_field_name("function") {
                        let name = text(function, b);
                        if let Some(definition) = self
                            .macros
                            .get(name)
                            .filter(|definition| definition.params.is_some())
                        {
                            let params = definition.params.as_ref().unwrap();
                            let ordinary = ordinary_macro_parameters(&definition.body, params);
                            let arguments = n
                                .child_by_field_name("arguments")
                                .map(named_children)
                                .unwrap_or_default();
                            let fixed_count = params
                                .iter()
                                .position(|param| param == "__VA_ARGS__")
                                .unwrap_or(params.len());
                            let mut visible = Vec::new();
                            for (index, used) in ordinary.into_iter().enumerate() {
                                if !used {
                                    continue;
                                }
                                if params.get(index).is_some_and(|p| p == "__VA_ARGS__") {
                                    visible.extend(arguments.iter().skip(fixed_count).copied());
                                } else if let Some(argument) = arguments.get(index) {
                                    visible.push(*argument);
                                }
                            }
                            visible.reverse();
                            stack.extend(visible);
                            continue;
                        }
                    }
                    // A plain callee name denotes an unresolved function, not
                    // a local variable. Function-pointer globals still need
                    // their ordinary phantom-local treatment.
                    if let Some(function) = n.child_by_field_name("function") {
                        let name = text(function, b);
                        if self.globals.contains_key(name) || self.symbols.contains_key(name) {
                            stack.push(function);
                        }
                    }
                    if let Some(arguments) = n.child_by_field_name("arguments") {
                        let mut children = named_children(arguments);
                        children.reverse();
                        stack.extend(children);
                    }
                    continue;
                }
                // Preprocessor structure: only the selected #if/#elif/#ifdef
                // branch contributes (directive identifiers never do).
                "preproc_if" | "preproc_ifdef" => {
                    let mut cs = selected_preproc_children(n, b, self.macros);
                    cs.reverse();
                    for c in cs {
                        stack.push(c);
                    }
                    continue;
                }
                "preproc_def" | "preproc_function_def" | "preproc_include" => continue,
                _ => {}
            }
            let mut cs = named_children(n);
            cs.reverse(); // stack pop order = document order
            for c in cs {
                stack.push(c);
            }
        }
    }

    /// Emit a BLOCK node and its statements, with a fresh child ORDER sequence.
    fn emit_block(&mut self, body: Node, b: &[u8], order: i64, depth: usize) {
        self.line(
            depth,
            "BLOCK",
            P {
                code: Some(esc(text(body, b))),
                tfn: Some("void".into()),
                order: Some(order),
                ..Default::default()
            },
        );
        for ph in std::mem::take(&mut self.phantoms) {
            self.line(
                depth + 1,
                "LOCAL",
                P {
                    name: Some(ph.name),
                    code: Some(ph.code),
                    tfn: Some(ph.ty),
                    order: Some(0),
                    ..Default::default()
                },
            );
        }
        let mut so = 1i64;
        for s in named_children(body) {
            self.emit_stmt(s, b, &mut so, depth + 1);
        }
    }

    /// A block-level statement. `order` is the running 1-based child position.
    fn emit_stmt(&mut self, n: Node, b: &[u8], order: &mut i64, depth: usize) {
        match n.kind() {
            "declaration" => self.emit_declaration(n, b, order, depth, None),
            "if_statement" => self.emit_if(n, b, order, depth),
            "for_statement" => self.emit_for(n, b, order, depth),
            "preproc_if" | "preproc_ifdef" => {
                // The selected branch splices directly into the surrounding
                // block, matching the preprocessed source seen by c2cpg.
                for child in selected_preproc_children(n, b, self.macros) {
                    self.emit_stmt(child, b, order, depth);
                }
            }
            "labeled_statement" => {
                // A label flattens like a switch case: JUMP_TARGET (CODE is
                // the whole labeled statement) then the statement as sibling.
                let o = *order;
                *order += 1;
                let lname = n
                    .child_by_field_name("label")
                    .map(|l| text(l, b).to_string())
                    .unwrap_or_default();
                self.line(
                    depth,
                    "JUMP_TARGET",
                    P {
                        name: Some(lname),
                        code: Some(esc(text(n, b))),
                        order: Some(o),
                        ..Default::default()
                    },
                );
                for c in named_children(n) {
                    if c.kind() != "statement_identifier" {
                        self.emit_stmt(c, b, order, depth);
                    }
                }
            }
            "goto_statement" => {
                let o = *order;
                *order += 1;
                self.line(
                    depth,
                    "CONTROL_STRUCTURE",
                    P {
                        code: Some(esc(text(n, b))),
                        order: Some(o),
                        ..Default::default()
                    },
                );
            }
            "break_statement" | "continue_statement" => {
                let o = *order;
                *order += 1;
                self.line(
                    depth,
                    "CONTROL_STRUCTURE",
                    P {
                        code: Some(esc(text(n, b))),
                        order: Some(o),
                        ..Default::default()
                    },
                );
            }
            "switch_statement" => {
                let o = *order;
                *order += 1;
                let cs = self.line_no;
                self.line(
                    depth,
                    "CONTROL_STRUCTURE",
                    P {
                        code: Some(esc(text(n, b))),
                        order: Some(o),
                        ..Default::default()
                    },
                );
                if let Some(cond) = n.child_by_field_name("condition") {
                    let ci = self.line_no;
                    self.emit_expr(unwrap_paren(cond), b, depth + 1, 1, None);
                    self.edge("CONDITION", self.at(cs), self.at(ci));
                }
                if let Some(body) = n.child_by_field_name("body") {
                    let bi = self.line_no;
                    self.emit_block(body, b, 2, depth + 1);
                    self.edge("TRUE_BODY", self.at(cs), self.at(bi));
                }
            }
            // Cases are flattened into the switch body's BLOCK: a JUMP_TARGET,
            // then (for `case`) the value as a bare child with no
            // ARGUMENT_INDEX, then the statements — all as siblings.
            "case_statement" => {
                let value = n.child_by_field_name("value");
                let (name, code) = match value {
                    Some(v) => ("case", format!("case {}:", text(v, b))),
                    None => ("default", "default:".to_string()),
                };
                let o = *order;
                *order += 1;
                self.line(
                    depth,
                    "JUMP_TARGET",
                    P {
                        name: Some(name.into()),
                        code: Some(esc(&code)),
                        order: Some(o),
                        ..Default::default()
                    },
                );
                if let Some(v) = value {
                    let vo = *order;
                    *order += 1;
                    self.emit_expr(v, b, depth, vo, None);
                }
                for s in named_children(n) {
                    if Some(s.id()) != value.map(|v| v.id()) {
                        self.emit_stmt(s, b, order, depth);
                    }
                }
            }
            "do_statement" => {
                let o = *order;
                *order += 1;
                // c2cpg quirk: a do-while's CODE is the entire statement,
                // trailing semicolon included (unlike while, header only).
                self.line(
                    depth,
                    "CONTROL_STRUCTURE",
                    P {
                        code: Some(esc(text(n, b))),
                        order: Some(o),
                        ..Default::default()
                    },
                );
                let cs = self.line_no - 1;
                if let Some(body) = n.child_by_field_name("body") {
                    if body.kind() == "compound_statement" {
                        let bi = self.line_no;
                        self.emit_block(body, b, 1, depth + 1);
                        self.edge("DO_BODY", self.at(cs), self.at(bi));
                    }
                }
                if let Some(cond) = n.child_by_field_name("condition") {
                    let ci = self.line_no;
                    self.emit_expr(unwrap_paren(cond), b, depth + 1, 2, None);
                    self.edge("CONDITION", self.at(cs), self.at(ci));
                }
            }
            "while_statement" => {
                let o = *order;
                *order += 1;
                let cs = self.line_no;
                let cond = n.child_by_field_name("condition");
                // c2cpg quirk: a while's CODE is just `while <cond>`, not the body.
                let code = cond
                    .map(|c| format!("while {}", text(c, b)))
                    .unwrap_or("while".into());
                self.line(
                    depth,
                    "CONTROL_STRUCTURE",
                    P {
                        code: Some(esc(&code)),
                        order: Some(o),
                        ..Default::default()
                    },
                );
                if let Some(c) = cond {
                    let ci = self.line_no;
                    self.emit_expr(unwrap_paren(c), b, depth + 1, 1, None);
                    self.edge("CONDITION", self.at(cs), self.at(ci));
                }
                if let Some(body) = n.child_by_field_name("body") {
                    if body.kind() == "compound_statement" {
                        let bi = self.line_no;
                        self.emit_block(body, b, 2, depth + 1);
                        self.edge("TRUE_BODY", self.at(cs), self.at(bi));
                    }
                }
            }
            "return_statement" => {
                let o = *order;
                *order += 1;
                self.line(
                    depth,
                    "RETURN",
                    P {
                        code: Some(esc(text(n, b))),
                        order: Some(o),
                        ..Default::default()
                    },
                );
                // The returned expression is a single child, ORDER=1, no arg index.
                if let Some(e) = named_children(n).into_iter().next() {
                    self.emit_expr(e, b, depth + 1, 1, None);
                }
            }
            "expression_statement" => {
                if let Some(e) = named_children(n).into_iter().next() {
                    let o = *order;
                    *order += 1;
                    self.emit_expr(e, b, depth, o, None);
                }
            }
            _ => {}
        }
    }

    /// An `if`/`else` → CONTROL_STRUCTURE with condition (ORDER 1), consequence
    /// BLOCK (ORDER 2), and an `else` CONTROL_STRUCTURE (ORDER 3) wrapping the
    /// alternative block — exactly as c2cpg lowers it.
    fn emit_if(&mut self, n: Node, b: &[u8], order: &mut i64, depth: usize) {
        let o = *order;
        *order += 1;
        let cs = self.line_no;
        self.line(
            depth,
            "CONTROL_STRUCTURE",
            P {
                code: Some(esc(text(n, b))),
                order: Some(o),
                ..Default::default()
            },
        );
        if let Some(cond) = n.child_by_field_name("condition") {
            let ci = self.line_no;
            self.emit_expr(unwrap_paren(cond), b, depth + 1, 1, None);
            self.edge("CONDITION", self.at(cs), self.at(ci));
        }
        if let Some(cons) = n.child_by_field_name("consequence") {
            if cons.kind() == "compound_statement" {
                let bi = self.line_no;
                self.emit_block(cons, b, 2, depth + 1);
                self.edge("TRUE_BODY", self.at(cs), self.at(bi));
            }
        }
        if let Some(alt) = n.child_by_field_name("alternative") {
            let ei = self.line_no;
            self.line(
                depth + 1,
                "CONTROL_STRUCTURE",
                P {
                    code: Some("else".into()),
                    order: Some(3),
                    ..Default::default()
                },
            );
            self.edge("FALSE_BODY", self.at(cs), self.at(ei));
            if let Some(body) = named_children(alt)
                .into_iter()
                .find(|c| c.kind() == "compound_statement")
            {
                self.emit_block(body, b, 1, depth + 2);
            } else if let Some(stmt) = named_children(alt).into_iter().next() {
                // `else if`: a synthetic CODE-less ANY BLOCK wraps the
                // nested statement.
                self.line(
                    depth + 2,
                    "BLOCK",
                    P {
                        tfn: Some("ANY".into()),
                        order: Some(1),
                        ..Default::default()
                    },
                );
                let mut so = 1i64;
                self.emit_stmt(stmt, b, &mut so, depth + 3);
            }
        }
    }

    /// A `for` → CONTROL_STRUCTURE whose CODE is rebuilt as
    /// `for (init;cond;update)` — no space after the semicolons (c2cpg quirk) —
    /// with the init declaration flattened into the structure's children and
    /// its assignment carrying ARGUMENT_INDEX=1 (another quirk; the condition,
    /// update, and body carry none).
    fn emit_for(&mut self, n: Node, b: &[u8], order: &mut i64, depth: usize) {
        let init = n.child_by_field_name("initializer");
        let cond = n.child_by_field_name("condition");
        let update = n.child_by_field_name("update");
        let part = |x: Option<Node>| {
            x.map(|c| text(c, b).trim_end_matches(';').trim().to_string())
                .unwrap_or_default()
        };
        let o = *order;
        *order += 1;
        let cs = self.line_no;
        self.line(
            depth,
            "CONTROL_STRUCTURE",
            P {
                code: Some(esc(&format!(
                    "for ({};{};{})",
                    part(init),
                    part(cond),
                    part(update)
                ))),
                order: Some(o),
                ..Default::default()
            },
        );
        let mut co = 1i64;
        if init.is_none() {
            // Empty init clause: a CODE-less ANY BLOCK placeholder, which
            // still receives the FOR_INIT edge.
            let pi = self.line_no;
            self.line(
                depth + 1,
                "BLOCK",
                P {
                    tfn: Some("ANY".into()),
                    order: Some(co),
                    ..Default::default()
                },
            );
            self.edge("FOR_INIT", self.at(cs), self.at(pi));
            co += 1;
        }
        if let Some(i) = init {
            if i.kind() == "declaration" {
                let before = self.line_no;
                self.emit_declaration(i, b, &mut co, depth + 1, Some(1));
                // FOR_INIT targets the init assignment CALL, not the LOCAL.
                if self.line_no > before + 1 {
                    self.edge("FOR_INIT", self.at(cs), self.at(before + 1));
                }
            } else {
                let ii = self.line_no;
                self.emit_expr(i, b, depth + 1, co, Some(1));
                self.edge("FOR_INIT", self.at(cs), self.at(ii));
                co += 1;
            }
        }
        if let Some(c) = cond {
            let ci = self.line_no;
            self.emit_expr(c, b, depth + 1, co, None);
            self.edge("CONDITION", self.at(cs), self.at(ci));
            co += 1;
        }
        if let Some(u) = update {
            let ui = self.line_no;
            self.emit_expr(u, b, depth + 1, co, None);
            self.edge("FOR_UPDATE", self.at(cs), self.at(ui));
            co += 1;
        }
        if let Some(body) = n.child_by_field_name("body") {
            if body.kind() == "compound_statement" {
                let bi = self.line_no;
                self.emit_block(body, b, co, depth + 1);
                self.edge("FOR_BODY", self.at(cs), self.at(bi));
            }
        }
    }

    /// A C declaration `T x = init;` → a LOCAL plus, if initialised, an
    /// `<operator>.assignment` CALL — exactly as c2cpg lowers it.
    fn emit_declaration(
        &mut self,
        n: Node,
        b: &[u8],
        order: &mut i64,
        depth: usize,
        assign_arg: Option<i64>,
    ) {
        let ty = normalize_type(
            &n.child_by_field_name("type")
                .map(|t| text(t, b).to_string())
                .unwrap_or("ANY".into()),
        );
        // CDT registers the decl-SPECIFIER type separately from the declared
        // type: `unsigned char c` also registers bare `unsigned` (pinned by
        // musl memcmp.c); a pointer decl registers its base.
        self.types.insert(specifier_type(&ty));
        // LOCAL CODE is rebuilt per declarator: the decl-specifier source text
        // (keeps `const`/`struct`/`unsigned ...` spellings the type drops)
        // plus that declarator alone — so `int a, b = 1;` yields `int a`,`int b`.
        let spec_end = n
            .child_by_field_name("type")
            .map(|t| t.end_byte())
            .unwrap_or(n.start_byte());
        let decl_code = |d: Node| {
            let spec = std::str::from_utf8(&b[n.start_byte()..spec_end]).unwrap_or("");
            esc(&format!("{spec} {}", text(d, b)))
        };
        // Pass 1: all LOCALs (musl memcmp pins that `T *a=x, *b=y;` emits
        // both LOCALs before any assignment).
        struct DeclItem<'t> {
            decl: Node<'t>,
            outer: Node<'t>,
            init: Option<Node<'t>>,
            name: String,
            full_ty: String,
            function_pointer: bool,
        }
        let mut items: Vec<DeclItem> = Vec::new();
        for d in named_children(n) {
            let (decl, init) = match d.kind() {
                "init_declarator" => (
                    d.child_by_field_name("declarator"),
                    d.child_by_field_name("value"),
                ),
                "identifier" | "pointer_declarator" | "array_declarator" => (Some(d), None),
                _ => (None, None),
            };
            let Some(decl) = decl else { continue };
            let name = innermost_id(decl, b);
            let pointer_type = function_pointer_type(&ty, decl, b);
            let full_ty = pointer_type
                .clone()
                .unwrap_or_else(|| format!("{ty}{}", decl_suffix(decl, b)));
            self.symbols.insert(name.clone(), full_ty.clone());
            let lo = *order;
            *order += 1;
            self.line(
                depth,
                "LOCAL",
                P {
                    name: Some(name.clone()),
                    code: Some(if pointer_type.is_some() {
                        esc(text(n, b))
                    } else {
                        decl_code(decl)
                    }),
                    tfn: Some(full_ty.clone()),
                    order: Some(lo),
                    ..Default::default()
                },
            );
            items.push(DeclItem {
                decl,
                outer: d,
                init,
                name,
                full_ty,
                function_pointer: pointer_type.is_some(),
            });
        }
        // Pass 2: initialiser assignments / alloc lowerings, in order.
        for it in items {
            if let Some(v) = it.init {
                let ao = *order;
                *order += 1;
                self.note_call("<operator>.assignment", 2);
                self.line(
                    depth,
                    "CALL",
                    P {
                        name: Some("<operator>.assignment".into()),
                        // CODE is the raw init_declarator text (`*l=vl`, `b = 1`).
                        code: Some(esc(text(it.outer, b))),
                        tfn: Some("void".into()),
                        mfn: Some("<operator>.assignment".into()),
                        order: Some(ao),
                        arg: assign_arg,
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                self.line(
                    depth + 1,
                    "IDENTIFIER",
                    P {
                        name: Some(it.name.clone()),
                        code: Some(if it.function_pointer {
                            String::new()
                        } else {
                            it.name.clone()
                        }),
                        tfn: Some(it.full_ty.clone()),
                        order: Some(1),
                        arg: Some(1),
                        ..Default::default()
                    },
                );
                self.emit_expr(v, b, depth + 1, 2, Some(2));
                continue;
            }
            let sizes = array_sizes(it.decl);
            if !sizes.is_empty() {
                let ao = *order;
                *order += 1;
                self.note_call("<operator>.assignment", 2);
                self.note_call("<operator>.alloc", sizes.len() + 1);
                self.line(
                    depth,
                    "CALL",
                    P {
                        name: Some("<operator>.assignment".into()),
                        code: Some(esc(text(it.decl, b))),
                        tfn: Some("void".into()),
                        mfn: Some("<operator>.assignment".into()),
                        order: Some(ao),
                        arg: assign_arg,
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                self.line(
                    depth + 1,
                    "IDENTIFIER",
                    P {
                        name: Some(it.name.clone()),
                        code: Some(it.name),
                        tfn: Some(it.full_ty.clone()),
                        order: Some(1),
                        arg: Some(1),
                        ..Default::default()
                    },
                );
                self.line(
                    depth + 1,
                    "CALL",
                    P {
                        name: Some("<operator>.alloc".into()),
                        code: Some(esc(text(it.decl, b))),
                        tfn: Some(it.full_ty.clone()),
                        mfn: Some("<operator>.alloc".into()),
                        order: Some(2),
                        arg: Some(2),
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                self.line(
                    depth + 2,
                    "IDENTIFIER",
                    P {
                        name: Some(it.full_ty.clone()),
                        code: Some(it.full_ty.clone()),
                        tfn: Some(it.full_ty),
                        order: Some(1),
                        arg: Some(1),
                        ..Default::default()
                    },
                );
                for (i, sz) in sizes.into_iter().enumerate() {
                    let k = (i + 2) as i64;
                    self.emit_expr(sz, b, depth + 2, k, Some(k));
                }
            }
        }
    }

    /// Emit an expression node with the given ORDER and optional ARGUMENT_INDEX.
    fn emit_expr(&mut self, n: Node, b: &[u8], depth: usize, order: i64, arg: Option<i64>) {
        match n.kind() {
            "binary_expression" => {
                let op = n.child(1).map(|o| text(o, b)).unwrap_or("?");
                let name = operator_name(op);
                self.note_call(&name, 2);
                self.line(
                    depth,
                    "CALL",
                    P {
                        name: Some(name.clone()),
                        code: Some(esc(text(n, b))),
                        tfn: Some("ANY".into()),
                        mfn: Some(name),
                        order: Some(order),
                        arg,
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                if let Some(l) = n.child_by_field_name("left") {
                    self.emit_expr(l, b, depth + 1, 1, Some(1));
                }
                if let Some(r) = n.child_by_field_name("right") {
                    self.emit_expr(r, b, depth + 1, 2, Some(2));
                }
            }
            "assignment_expression" => {
                // A bare assignment statement; typed ANY (unlike a declaration's
                // initialiser assignment, which c2cpg types `void`).
                let op = n
                    .child_by_field_name("operator")
                    .map(|o| text(o, b))
                    .unwrap_or("=");
                let name = assignment_name(op);
                self.note_call(&name, 2);
                self.line(
                    depth,
                    "CALL",
                    P {
                        name: Some(name.clone()),
                        code: Some(esc(text(n, b))),
                        tfn: Some("ANY".into()),
                        mfn: Some(name),
                        order: Some(order),
                        arg,
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                if let Some(l) = n.child_by_field_name("left") {
                    self.emit_expr(l, b, depth + 1, 1, Some(1));
                }
                if let Some(r) = n.child_by_field_name("right") {
                    self.emit_expr(r, b, depth + 1, 2, Some(2));
                }
            }
            "unary_expression" | "pointer_expression" => {
                let op = n.child(0).map(|o| text(o, b)).unwrap_or("?");
                let name = unary_name(op);
                self.note_call(&name, 1);
                self.line(
                    depth,
                    "CALL",
                    P {
                        name: Some(name.clone()),
                        code: Some(esc(text(n, b))),
                        tfn: Some("ANY".into()),
                        mfn: Some(name),
                        order: Some(order),
                        arg,
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                if let Some(a) = n.child_by_field_name("argument") {
                    self.emit_expr(a, b, depth + 1, 1, Some(1));
                }
            }
            "update_expression" => {
                let arg_node = n.child_by_field_name("argument");
                let op_node = n.child_by_field_name("operator");
                let op = op_node.map(|o| text(o, b)).unwrap_or("++");
                let prefix = match (op_node, arg_node) {
                    (Some(o), Some(a)) => o.start_byte() < a.start_byte(),
                    _ => false,
                };
                let name = format!(
                    "<operator>.{}{}",
                    if prefix { "pre" } else { "post" },
                    if op == "++" { "Increment" } else { "Decrement" }
                );
                self.note_call(&name, 1);
                self.line(
                    depth,
                    "CALL",
                    P {
                        name: Some(name.clone()),
                        code: Some(esc(text(n, b))),
                        tfn: Some("ANY".into()),
                        mfn: Some(name),
                        order: Some(order),
                        arg,
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                if let Some(a) = arg_node {
                    self.emit_expr(a, b, depth + 1, 1, Some(1));
                }
            }
            "conditional_expression" => {
                self.note_call("<operator>.conditional", 3);
                self.line(
                    depth,
                    "CALL",
                    P {
                        name: Some("<operator>.conditional".into()),
                        code: Some(esc(text(n, b))),
                        tfn: Some("ANY".into()),
                        mfn: Some("<operator>.conditional".into()),
                        order: Some(order),
                        arg,
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                for (i, field) in ["condition", "consequence", "alternative"]
                    .iter()
                    .enumerate()
                {
                    if let Some(c) = n.child_by_field_name(*field) {
                        let k = (i + 1) as i64;
                        self.emit_expr(c, b, depth + 1, k, Some(k));
                    }
                }
            }
            "field_expression" => {
                // `.` → fieldAccess, `->` → indirectFieldAccess; the member is
                // a FIELD_IDENTIFIER child with CODE only (no NAME).
                let op = n
                    .child_by_field_name("operator")
                    .map(|o| text(o, b))
                    .unwrap_or(".");
                let name = if op == "->" {
                    "<operator>.indirectFieldAccess".to_string()
                } else {
                    "<operator>.fieldAccess".to_string()
                };
                self.note_call(&name, 2);
                // CDT resolves `q.x` to the struct MEMBER (REF edge) only for
                // value receivers — `p->y` through a pointer stays unresolved.
                if op == "." {
                    let recv_ty = n
                        .child_by_field_name("argument")
                        .filter(|a| a.kind() == "identifier")
                        .and_then(|a| self.symbols.get(text(a, b)).cloned());
                    if let (Some(t), Some(f)) = (recv_ty, n.child_by_field_name("field")) {
                        let call_at = self.at(self.line_no);
                        self.edge("REF", call_at, format!("MB:{t}.{}", text(f, b)));
                    }
                }
                self.line(
                    depth,
                    "CALL",
                    P {
                        name: Some(name.clone()),
                        code: Some(esc(text(n, b))),
                        tfn: Some("ANY".into()),
                        mfn: Some(name),
                        order: Some(order),
                        arg,
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                if let Some(a) = n.child_by_field_name("argument") {
                    self.emit_expr(a, b, depth + 1, 1, Some(1));
                }
                if let Some(f) = n.child_by_field_name("field") {
                    self.line(
                        depth + 1,
                        "FIELD_IDENTIFIER",
                        P {
                            code: Some(text(f, b).to_string()),
                            order: Some(2),
                            arg: Some(2),
                            ..Default::default()
                        },
                    );
                }
            }
            "subscript_expression" => {
                self.note_call("<operator>.indirectIndexAccess", 2);
                self.line(
                    depth,
                    "CALL",
                    P {
                        name: Some("<operator>.indirectIndexAccess".into()),
                        code: Some(esc(text(n, b))),
                        tfn: Some("ANY".into()),
                        mfn: Some("<operator>.indirectIndexAccess".into()),
                        order: Some(order),
                        arg,
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                if let Some(a) = n.child_by_field_name("argument") {
                    self.emit_expr(a, b, depth + 1, 1, Some(1));
                }
                if let Some(i) = n.child_by_field_name("index") {
                    self.emit_expr(i, b, depth + 1, 2, Some(2));
                }
            }
            "call_expression" => {
                let callee = n.child_by_field_name("function");
                let name = callee
                    .map(|c| text(c, b).to_string())
                    .unwrap_or("<anon>".into());
                let args = n.child_by_field_name("arguments");
                let argc = args.map(|a| named_children(a).len()).unwrap_or(0);
                if self.macros.get(&name).is_some_and(|m| m.params.is_some())
                    && !self.expanding_macros.contains(&name)
                {
                    let arg_nodes: Vec<Node> = args.map(|a| named_children(a)).unwrap_or_default();
                    let code = esc(text(n, b));
                    self.emit_macro_call(&name, &code, &arg_nodes, n, b, depth, order, arg);
                    return;
                }
                if !self.functions.contains_key(&name)
                    && (self.symbols.contains_key(&name) || self.globals.contains_key(&name))
                {
                    // Call through a pointer-valued symbol: <operator>.pointerCall,
                    // DYNAMIC_DISPATCH, receiver at ORDER=1 with no
                    // ARGUMENT_INDEX, args shifted to ORDER=2.. / INDEX=1..
                    self.note_call("<operator>.pointerCall", argc);
                    let ty = self
                        .symbols
                        .get(&name)
                        .or_else(|| self.globals.get(&name))
                        .map(|value| function_pointer_return_type(value).to_string());
                    self.line(
                        depth,
                        "CALL",
                        P {
                            name: Some("<operator>.pointerCall".into()),
                            code: Some(esc(text(n, b))),
                            tfn: ty,
                            mfn: Some("<operator>.pointerCall".into()),
                            order: Some(order),
                            arg,
                            dispatch: Some("DYNAMIC_DISPATCH".into()),
                            ..Default::default()
                        },
                    );
                    if let Some(c) = callee {
                        self.emit_expr(c, b, depth + 1, 1, None);
                    }
                    if let Some(args) = args {
                        for (i, a) in named_children(args).into_iter().enumerate() {
                            self.emit_expr(a, b, depth + 1, (i + 2) as i64, Some((i + 1) as i64));
                        }
                    }
                } else {
                    let ty = self.functions.get(&name).cloned().unwrap_or("ANY".into());
                    let method_full_name = self
                        .function_full_names
                        .get(&name)
                        .cloned()
                        .unwrap_or_else(|| name.clone());
                    self.note_call(&name, argc);
                    self.line(
                        depth,
                        "CALL",
                        P {
                            name: Some(name.clone()),
                            code: Some(esc(text(n, b))),
                            tfn: Some(ty),
                            mfn: Some(method_full_name),
                            order: Some(order),
                            arg,
                            dispatch: Some("STATIC_DISPATCH".into()),
                            ..Default::default()
                        },
                    );
                    if let Some(args) = args {
                        for (i, a) in named_children(args).into_iter().enumerate() {
                            let k = (i + 1) as i64;
                            self.emit_expr(a, b, depth + 1, k, Some(k));
                        }
                    }
                }
            }
            "identifier" => {
                let name = text(n, b).to_string();
                if self.macros.get(&name).is_some_and(|m| m.params.is_none())
                    && !self.expanding_macros.contains(&name)
                {
                    self.emit_macro_call(&name, &name, &[], n, b, depth, order, arg);
                    return;
                }
                if let Some(return_type) = self.functions.get(&name) {
                    let method_full_name = self
                        .function_full_names
                        .get(&name)
                        .cloned()
                        .unwrap_or_else(|| name.clone());
                    self.line(
                        depth,
                        "METHOD_REF",
                        P {
                            code: Some(name),
                            tfn: Some(return_type.clone()),
                            mfn: Some(method_full_name),
                            order: Some(order),
                            arg,
                            ..Default::default()
                        },
                    );
                    return;
                }
                let (code, ty) = if let Some(t) = self.symbols.get(&name) {
                    (name.clone(), t.clone())
                } else if let Some(t) = self.globals.get(&name) {
                    (format!("<global> {name}"), t.clone())
                } else if self.enumerators.contains(&name) {
                    (name.clone(), "ANY".to_string())
                } else {
                    // Fully unresolved (e.g. NULL with unresolved includes).
                    (format!("<unknown> {name}"), "ANY".to_string())
                };
                self.line(
                    depth,
                    "IDENTIFIER",
                    P {
                        name: Some(name),
                        code: Some(code),
                        tfn: Some(ty),
                        order: Some(order),
                        arg,
                        ..Default::default()
                    },
                );
            }
            "number_literal" => {
                // tree-sitter folds a leading sign into the literal; Joern
                // (CDT) lowers `-1` to <operator>.minus applied to `1`.
                let t = text(n, b).to_string();
                if let Some(rest) = t.strip_prefix('-').or_else(|| t.strip_prefix('+')) {
                    let name = unary_name(&t[..1]);
                    self.note_call(&name, 1);
                    self.line(
                        depth,
                        "CALL",
                        P {
                            name: Some(name.clone()),
                            code: Some(t.clone()),
                            tfn: Some("ANY".into()),
                            mfn: Some(name),
                            order: Some(order),
                            arg,
                            dispatch: Some("STATIC_DISPATCH".into()),
                            ..Default::default()
                        },
                    );
                    self.line(
                        depth + 1,
                        "LITERAL",
                        P {
                            code: Some(rest.to_string()),
                            tfn: Some("int".into()),
                            order: Some(1),
                            arg: Some(1),
                            ..Default::default()
                        },
                    );
                } else {
                    let tfn = if t.starts_with("0x") || t.starts_with("0X") {
                        "int"
                    } else if t.ends_with('f') || t.ends_with('F') {
                        "float"
                    } else if t.contains('.') || t.contains('e') || t.contains('E') {
                        "double"
                    } else {
                        "int"
                    };
                    self.line(
                        depth,
                        "LITERAL",
                        P {
                            code: Some(t),
                            tfn: Some(tfn.into()),
                            order: Some(order),
                            arg,
                            ..Default::default()
                        },
                    );
                }
            }
            "char_literal" => {
                self.line(
                    depth,
                    "LITERAL",
                    P {
                        code: Some(text(n, b).to_string()),
                        tfn: Some("char".into()),
                        order: Some(order),
                        arg,
                        ..Default::default()
                    },
                );
            }
            "string_literal" => {
                self.line(
                    depth,
                    "LITERAL",
                    P {
                        code: Some(esc(text(n, b))),
                        tfn: Some("char*".into()),
                        order: Some(order),
                        arg,
                        ..Default::default()
                    },
                );
            }
            "cast_expression" => {
                // `(T)e` → <operator>.cast. CDT quirk: the type is the BASE
                // type only — `(char *)x` types as `char` — while the
                // TYPE_REF CODE keeps the raw descriptor text (`char *`).
                let desc = n.child_by_field_name("type");
                let raw = desc.map(|t| text(t, b).to_string()).unwrap_or("ANY".into());
                let ty = desc
                    .and_then(|t| t.child_by_field_name("type"))
                    .map(|t| normalize_type(text(t, b)))
                    .unwrap_or_else(|| normalize_type(&raw));
                self.note_call("<operator>.cast", 2);
                self.line(
                    depth,
                    "CALL",
                    P {
                        name: Some("<operator>.cast".into()),
                        code: Some(esc(text(n, b))),
                        tfn: Some(ty.clone()),
                        mfn: Some("<operator>.cast".into()),
                        order: Some(order),
                        arg,
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                self.line(
                    depth + 1,
                    "TYPE_REF",
                    P {
                        code: Some(esc(&raw)),
                        tfn: Some(ty),
                        order: Some(1),
                        arg: Some(1),
                        ..Default::default()
                    },
                );
                if let Some(v) = n.child_by_field_name("value") {
                    self.emit_expr(v, b, depth + 1, 2, Some(2));
                }
            }
            "sizeof_expression" => {
                self.note_call("<operator>.sizeOf", 1);
                self.line(
                    depth,
                    "CALL",
                    P {
                        name: Some("<operator>.sizeOf".into()),
                        code: Some(esc(text(n, b))),
                        tfn: Some("ANY".into()),
                        mfn: Some("<operator>.sizeOf".into()),
                        order: Some(order),
                        arg,
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                if let Some(t) = n.child_by_field_name("type") {
                    // sizeof(T): the type name appears as an IDENTIFIER typed
                    // as itself (and spawns the ORDER=0 phantom LOCAL).
                    let code = text(t, b).to_string();
                    let name = normalize_type(&code);
                    self.line(
                        depth + 1,
                        "IDENTIFIER",
                        P {
                            name: Some(name.clone()),
                            code: Some(code),
                            tfn: Some(name),
                            order: Some(1),
                            arg: Some(1),
                            ..Default::default()
                        },
                    );
                } else if let Some(v) = n.child_by_field_name("value") {
                    self.emit_expr(unwrap_paren(v), b, depth + 1, 1, Some(1));
                }
            }
            "comma_expression" => {
                // `(a, b)` → a CODE-less BLOCK typed ANY whose children carry
                // ORDER but no ARGUMENT_INDEX.
                self.line(
                    depth,
                    "BLOCK",
                    P {
                        tfn: Some("ANY".into()),
                        order: Some(order),
                        arg,
                        ..Default::default()
                    },
                );
                let mut parts = Vec::new();
                flatten_comma(n, &mut parts);
                for (i, part) in parts.into_iter().enumerate() {
                    self.emit_expr(part, b, depth + 1, (i + 1) as i64, None);
                }
            }
            "null" => {
                // tree-sitter parses NULL as its own node kind; with
                // unresolved includes CDT sees an unresolved identifier.
                self.line(
                    depth,
                    "IDENTIFIER",
                    P {
                        name: Some("NULL".into()),
                        code: Some("<unknown> NULL".into()),
                        tfn: Some("ANY".into()),
                        order: Some(order),
                        arg,
                        ..Default::default()
                    },
                );
            }
            "parenthesized_expression" => {
                if let Some(inner) = named_children(n).into_iter().next() {
                    self.emit_expr(inner, b, depth, order, arg);
                }
            }
            _ => {}
        }
    }
}

// --- header extraction ---

struct Param {
    name: String,
    ty: String,
    code: String,
}

/// (name, return type, params) for a function_definition.
fn fn_header(f: Node, b: &[u8]) -> Option<(String, String, Vec<Param>)> {
    let base = f
        .child_by_field_name("type")
        .map(|t| text(t, b).to_string())
        .unwrap_or("ANY".into());
    let decl = f.child_by_field_name("declarator")?;
    // `void *bsearch(...)`: pointer levels wrap the function declarator.
    let mut stars = 0;
    let mut cur = decl;
    while cur.kind() == "pointer_declarator" {
        stars += 1;
        match cur.child_by_field_name("declarator") {
            Some(c) => cur = c,
            None => break,
        }
    }
    let ret = format!("{}{}", normalize_type(&base), "*".repeat(stars));
    let fd = find_function_declarator(decl)?;
    let name = fd
        .child_by_field_name("declarator")
        .map(|d| innermost_id(d, b))?;
    let mut params = Vec::new();
    if let Some(pl) = fd.child_by_field_name("parameters") {
        for p in named_children(pl) {
            if p.kind() == "parameter_declaration" {
                let base = normalize_type(
                    &p.child_by_field_name("type")
                        .map(|t| text(t, b).to_string())
                        .unwrap_or("ANY".into()),
                );
                let decl = p.child_by_field_name("declarator");
                let ty = format!(
                    "{base}{}",
                    decl.map(|d| decl_suffix(d, b)).unwrap_or_default()
                );
                let name = decl.map(|d| innermost_id(d, b)).unwrap_or_default();
                params.push(Param {
                    name,
                    ty,
                    code: text(p, b).to_string(),
                });
            }
        }
    }
    Some((name, ret, params))
}

fn find_function_declarator(n: Node) -> Option<Node> {
    if n.kind() == "function_declarator" {
        return Some(n);
    }
    named_children(n)
        .into_iter()
        .find_map(find_function_declarator)
}

fn unwrap_paren(n: Node) -> Node {
    if n.kind() == "parenthesized_expression" {
        if let Some(inner) = named_children(n).into_iter().next() {
            return unwrap_paren(inner);
        }
    }
    n
}

/// Unary/pointer operator → Joern operator name.
fn unary_name(op: &str) -> String {
    let n = match op {
        "-" => "minus",
        "+" => "plus",
        "!" => "logicalNot",
        "~" => "not",
        "*" => "indirection",
        "&" => "addressOf",
        _ => "unknown",
    };
    format!("<operator>.{n}")
}

/// Type suffix from declarator nesting: `*` per pointer level, `[]` per array
/// level (Joern renders `int *p` as `int*` and `int vals[]` as `int[]`).
fn decl_suffix(n: Node, b: &[u8]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = n;
    loop {
        match cur.kind() {
            "pointer_declarator" => parts.push("*".into()),
            "array_declarator" => {
                // CDT keeps the size: `int grid[2][3]` types as `int[2][3]`
                // (declarator nesting is outermost-last, so reverse).
                let size = cur
                    .child_by_field_name("size")
                    .map(|sz| text(sz, b).to_string())
                    .unwrap_or_default();
                parts.push(format!("[{size}]"));
            }
            _ => break,
        }
        match cur.child_by_field_name("declarator") {
            Some(c) => cur = c,
            None => break,
        }
    }
    parts.reverse();
    parts.concat()
}

/// CDT gives local function-pointer objects a signature-shaped type such as
/// `int(*)(int,int)`, while function-pointer parameters retain only their base
/// type. Keep this helper on the declaration path so the established parameter
/// quirk remains byte-identical to Joern.
fn function_pointer_type(base: &str, declaration: Node, b: &[u8]) -> Option<String> {
    let function = find_function_declarator(declaration)?;
    let target = function.child_by_field_name("declarator")?;
    if !has_pointer_declarator(target) {
        return None;
    }
    let parameters = function
        .child_by_field_name("parameters")
        .map(named_children)
        .unwrap_or_default()
        .into_iter()
        .filter(|parameter| parameter.kind() == "parameter_declaration")
        .map(|parameter| {
            let parameter_base = parameter
                .child_by_field_name("type")
                .map(|node| normalize_type(text(node, b)))
                .unwrap_or_else(|| "ANY".to_string());
            let suffix = parameter
                .child_by_field_name("declarator")
                .map(|node| decl_suffix(node, b))
                .unwrap_or_default();
            format!("{parameter_base}{suffix}")
        })
        .collect::<Vec<_>>()
        .join(",");
    Some(format!("{base}(*)({parameters})"))
}

fn function_pointer_return_type(declared_type: &str) -> &str {
    declared_type
        .split_once("(*)")
        .map_or(declared_type, |(return_type, _)| return_type)
}

fn has_pointer_declarator(node: Node) -> bool {
    node.kind() == "pointer_declarator"
        || named_children(node).into_iter().any(has_pointer_declarator)
}

/// Size expressions of a (possibly multi-dim) array declarator, source order.
fn array_sizes<'t>(n: Node<'t>) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    let mut cur = n;
    loop {
        if cur.kind() == "array_declarator" {
            if let Some(sz) = cur.child_by_field_name("size") {
                out.push(sz);
            }
        } else if cur.kind() != "pointer_declarator" {
            break;
        }
        match cur.child_by_field_name("declarator") {
            Some(c) => cur = c,
            None => break,
        }
    }
    out.reverse();
    out
}

/// Assignment operator → Joern operator name. Joern inconsistency, pinned by
/// corpus/exprs.c: +=/-=/*=//= use the `<operator>.` prefix but the other six
/// compound assignments use plural `<operators>.`.
fn assignment_name(op: &str) -> String {
    match op {
        "=" => "<operator>.assignment".into(),
        "+=" => "<operator>.assignmentPlus".into(),
        "-=" => "<operator>.assignmentMinus".into(),
        "*=" => "<operator>.assignmentMultiplication".into(),
        "/=" => "<operator>.assignmentDivision".into(),
        "%=" => "<operators>.assignmentModulo".into(),
        "&=" => "<operators>.assignmentAnd".into(),
        "|=" => "<operators>.assignmentOr".into(),
        "^=" => "<operators>.assignmentXor".into(),
        "<<=" => "<operators>.assignmentShiftLeft".into(),
        ">>=" => "<operators>.assignmentArithmeticShiftRight".into(),
        _ => "<operator>.assignment".into(),
    }
}

/// CDT renders multi-keyword primitive types reordered and unspaced
/// (`unsigned long` → `longunsigned`, pinned by corpus/exprs.c). Extend this
/// table only with oracle-pinned combinations.
/// CDT's typeForDeclSpecifier rendering, where it diverges from the declared
/// type: `unsigned char` -> `unsigned`. Extend only with oracle pins.
fn specifier_type(normalized: &str) -> String {
    match normalized {
        "unsigned char" => "unsigned".into(),
        t => t.into(),
    }
}

fn normalize_type(base: &str) -> String {
    let t = base.trim();
    // CDT inconsistency: `struct X`/`enum X` strip the keyword but `union X`
    // concatenates to `unionX` (pinned by corpus/types2.c).
    if let Some(rest) = t.strip_prefix("union ") {
        let name = rest.split_whitespace().next().unwrap_or(rest);
        return format!("union{name}");
    }
    for tag in ["struct ", "enum "] {
        if let Some(rest) = t.strip_prefix(tag) {
            if rest.trim_start().starts_with('{') {
                // Anonymous aggregate specifiers have no stable declaration
                // name. Carrying their whole multiline body as a type name
                // corrupts the one-record-per-line transport and is not a
                // usable type identity; keep the uncertainty explicit.
                return "ANY".into();
            }
            return rest.split_whitespace().next().unwrap_or(rest).into();
        }
    }
    match t {
        "unsigned long" => "longunsigned".into(),
        t => t.into(),
    }
}

// --- C operator → Joern operator name ---
fn operator_name(op: &str) -> String {
    let n = match op {
        "+" => "addition",
        "-" => "subtraction",
        "*" => "multiplication",
        "/" => "division",
        "%" => "modulo",
        "==" => "equals",
        "!=" => "notEquals",
        "<" => "lessThan",
        ">" => "greaterThan",
        "<=" => "lessEqualsThan",
        ">=" => "greaterEqualsThan",
        "&&" => "logicalAnd",
        "||" => "logicalOr",
        "&" => "and",
        "|" => "or",
        "^" => "xor",
        "<<" => "shiftLeft",
        ">>" => "arithmeticShiftRight",
        _ => "unknown",
    };
    format!("<operator>.{n}")
}

fn flatten_comma<'t>(n: Node<'t>, out: &mut Vec<Node<'t>>) {
    if n.kind() == "comma_expression" {
        if let Some(l) = n.child_by_field_name("left") {
            flatten_comma(l, out);
        }
        if let Some(r) = n.child_by_field_name("right") {
            flatten_comma(r, out);
        }
    } else {
        out.push(n);
    }
}

fn needs_clinit(n: Node, _b: &[u8]) -> bool {
    let Some(body) = n.child_by_field_name("body") else {
        return false;
    };
    named_children(body).iter().any(|f| {
        (f.kind() == "field_declaration"
            && named_children(*f)
                .iter()
                .any(|d| d.kind() == "array_declarator" && d.child_by_field_name("size").is_some()))
            || (f.kind() == "enumerator" && f.child_by_field_name("value").is_some())
    })
}

/// C-preprocessor substitution for a single macro body. Parameters inside
/// ordinary string/character literals are untouched; `#param` stringizes the
/// raw argument, `left ## right` pastes tokens, and `__VA_ARGS__` receives all
/// arguments after the fixed parameters. Nested macro expansion is a separate
/// pass ([`expand_macros_text`]) so the outer invocation remains the only
/// INLINED call in the CPG, matching CDT/Joern.
fn substitute(body: &str, params: &[String], args: &[String]) -> String {
    const EMPTY_VARIADIC: char = '\u{1e}';
    let variadic = params.iter().position(|param| param == "__VA_ARGS__");
    let bindings: HashMap<&str, String> = params
        .iter()
        .enumerate()
        .map(|(index, parameter)| {
            let value = if Some(index) == variadic {
                args.iter()
                    .skip(index)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                args.get(index).cloned().unwrap_or_default()
            };
            (parameter.as_str(), value)
        })
        .collect();
    let mut out = String::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '"' || c == '\'' {
            let quote = bytes[i];
            out.push(c);
            i += 1;
            while i < bytes.len() {
                let current = bytes[i];
                out.push(current as char);
                i += 1;
                if current == b'\\' && i < bytes.len() {
                    out.push(bytes[i] as char);
                    i += 1;
                } else if current == quote {
                    break;
                }
            }
        } else if c == '#'
            && bytes.get(i + 1) != Some(&b'#')
            && i.checked_sub(1).and_then(|index| bytes.get(index)) != Some(&b'#')
        {
            let mut name_start = i + 1;
            while bytes.get(name_start).is_some_and(u8::is_ascii_whitespace) {
                name_start += 1;
            }
            let mut name_end = name_start;
            while bytes
                .get(name_end)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                name_end += 1;
            }
            let name = &body[name_start..name_end];
            if let Some(argument) = bindings.get(name) {
                let normalized = argument.split_whitespace().collect::<Vec<_>>().join(" ");
                out.push('"');
                out.push_str(&normalized.replace('\\', "\\\\").replace('"', "\\\""));
                out.push('"');
                i = name_end;
            } else {
                out.push('#');
                i += 1;
            }
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < bytes.len()
                && ((bytes[i] as char).is_ascii_alphanumeric() || bytes[i] == b'_')
            {
                i += 1;
            }
            let word = &body[start..i];
            if let Some(argument) = bindings.get(word) {
                if word == "__VA_ARGS__" && argument.is_empty() {
                    out.push(EMPTY_VARIADIC);
                } else {
                    out.push_str(argument);
                }
            } else {
                out.push_str(word);
            }
        } else {
            out.push(c);
            i += 1;
        }
    }

    while let Some(paste) = out.find("##") {
        let left = out[..paste].trim_end();
        let right = out[paste + 2..].trim_start();
        let mut joined = left.to_string();
        let right = if let Some(rest) = right.strip_prefix(EMPTY_VARIADIC) {
            if joined.ends_with(',') {
                joined.pop();
                joined = joined.trim_end().to_string();
            }
            rest
        } else {
            right
        };
        joined.push_str(right);
        out = joined;
    }
    out.replace(EMPTY_VARIADIC, "")
}

/// Parameters that survive as ordinary expressions in the replacement list.
/// Stringized and pasted parameters are consumed by preprocessing and do not
/// appear as argument AST children in Joern's INLINED call.
fn ordinary_macro_parameters(body: &str, params: &[String]) -> Vec<bool> {
    let mut used = vec![false; params.len()];
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes[i], b'"' | b'\'') {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else {
                    let done = bytes[i] == quote;
                    i += 1;
                    if done {
                        break;
                    }
                }
            }
            continue;
        }
        if !bytes[i].is_ascii_alphabetic() && bytes[i] != b'_' {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        let word = &body[start..i];
        let Some(index) = params.iter().position(|param| param == word) else {
            continue;
        };
        let left = body[..start].trim_end().as_bytes();
        let right = body[i..].trim_start().as_bytes();
        let stringized = left.ends_with(b"#") && !left.ends_with(b"##");
        let pasted = left.ends_with(b"##") || right.starts_with(b"##");
        if !stringized && !pasted {
            used[index] = true;
        }
    }
    used
}

/// Recursively expand object/function macros in replacement text. The active
/// macro is kept in `expanding`, reproducing the preprocessor's recursion
/// suppression and bounding malformed/cyclic definitions.
fn expand_macros_text(
    input: &str,
    macros: &HashMap<String, MacroDef>,
    depth: usize,
    expanding: &mut HashSet<String>,
) -> String {
    if depth >= 64 {
        return input.to_string();
    }
    let bytes = input.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes[i], b'"' | b'\'') {
            let quote = bytes[i];
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else {
                    let done = bytes[i] == quote;
                    i += 1;
                    if done {
                        break;
                    }
                }
            }
            out.push_str(&input[start..i]);
            continue;
        }
        if !bytes[i].is_ascii_alphabetic() && bytes[i] != b'_' {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        let name = &input[start..i];
        let Some(definition) = macros.get(name) else {
            out.push_str(name);
            continue;
        };
        if expanding.contains(name) {
            out.push_str(name);
            continue;
        }
        let (arguments, end) = if let Some(parameters) = definition.params.as_ref() {
            let mut open = i;
            while bytes.get(open).is_some_and(u8::is_ascii_whitespace) {
                open += 1;
            }
            if bytes.get(open) != Some(&b'(') {
                out.push_str(name);
                continue;
            }
            let Some((arguments, end)) = parse_macro_arguments(input, open) else {
                out.push_str(name);
                continue;
            };
            (substitute(&definition.body, parameters, &arguments), end)
        } else {
            (definition.body.clone(), i)
        };
        expanding.insert(name.to_string());
        out.push_str(&expand_macros_text(
            &arguments,
            macros,
            depth + 1,
            expanding,
        ));
        expanding.remove(name);
        i = end;
    }
    out
}

fn parse_macro_arguments(input: &str, open: usize) -> Option<(Vec<String>, usize)> {
    let bytes = input.as_bytes();
    if bytes.get(open) != Some(&b'(') {
        return None;
    }
    let mut arguments = Vec::new();
    let mut depth = 1_u32;
    let mut start = open + 1;
    let mut i = start;
    let mut quote = None;
    while i < bytes.len() {
        if let Some(delimiter) = quote {
            if bytes[i] == b'\\' {
                i = (i + 2).min(bytes.len());
                continue;
            }
            if bytes[i] == delimiter {
                quote = None;
            }
            i += 1;
            continue;
        }
        match bytes[i] {
            b'"' | b'\'' => quote = Some(bytes[i]),
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    let final_argument = input[start..i].trim();
                    if !final_argument.is_empty() || !arguments.is_empty() {
                        arguments.push(final_argument.to_string());
                    }
                    return Some((arguments, i + 1));
                }
            }
            b',' if depth == 1 => {
                arguments.push(input[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The expression node of a parsed expansion (`void __m() { <exp>; }`).
fn expansion_expr_node(root: Node) -> Option<Node> {
    let f = named_children(root)
        .into_iter()
        .find(|n| n.kind() == "function_definition")?;
    let body = f.child_by_field_name("body")?;
    let stmt = named_children(body).into_iter().next()?;
    named_children(stmt).into_iter().next()
}

/// Type CDT assigns to a macro expansion root (drives MFN/SIGNATURE/TYPE).
fn expansion_type(
    expansion: &str,
    symbols: &HashMap<String, String>,
    functions: &HashMap<String, String>,
) -> String {
    let src = format!("void __m() {{ {expansion}; }}");
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .unwrap();
    let Some(tree) = parser.parse(&src, None) else {
        return "ANY".into();
    };
    let Some(mut e) = expansion_expr_node(tree.root_node()) else {
        return "ANY".into();
    };
    let b = src.as_bytes();
    while e.kind() == "parenthesized_expression" {
        match named_children(e).into_iter().next() {
            Some(inner) => e = inner,
            None => break,
        }
    }
    match e.kind() {
        "number_literal" => {
            let t = text(e, b);
            if t.contains('.') || t.contains('e') || t.contains('E') {
                "double".into()
            } else {
                "int".into()
            }
        }
        "char_literal" => "char".into(),
        "string_literal" => "char*".into(),
        "identifier" => symbols.get(text(e, b)).cloned().unwrap_or("ANY".into()),
        "call_expression" => {
            let name = e
                .child_by_field_name("function")
                .map(|c| text(c, b).to_string())
                .unwrap_or_default();
            functions.get(&name).cloned().unwrap_or("ANY".into())
        }
        _ => "ANY".into(),
    }
}

fn collect_decl_names(n: Node, b: &[u8], out: &mut Vec<String>) {
    if n.kind() == "declaration" {
        for d in named_children(n) {
            let name = innermost_id(d, b);
            if !name.is_empty() {
                out.push(name);
            }
        }
    }
    for c in named_children(n) {
        collect_decl_names(c, b, out);
    }
}

// --- tree helpers ---
fn named_children(n: Node) -> Vec<Node> {
    let mut cur = n.walk();
    n.named_children(&mut cur).collect()
}
fn text<'a>(n: Node, b: &'a [u8]) -> &'a str {
    n.utf8_text(b).unwrap_or("")
}
fn esc(s: &str) -> String {
    s.replace('\n', "\\n").trim().to_string()
}
fn innermost_id(n: Node, b: &[u8]) -> String {
    if n.kind() == "identifier" || n.kind() == "field_identifier" {
        return text(n, b).to_string();
    }
    for c in named_children(n) {
        let r = innermost_id(c, b);
        if !r.is_empty() {
            return r;
        }
    }
    String::new()
}

// ---- CFG construction (M4 part 2) -------------------------------------
// Joern's CfgCreationPass semantics, reconstructed from the oracle and run
// over our own emitted dump blocks (which are byte-identical to Joern's, so
// line indices are shared addresses). Rules pinned by corpus/:
// - Evaluation order: call arguments in order, then the call node itself.
//   Leaves are IDENTIFIER/LITERAL/FIELD_IDENTIFIER/METHOD_REF/TYPE_REF.
// - Statement BLOCKs are transparent; an expression BLOCK (child of a CALL,
//   i.e. the comma operator) is itself a CFG node after its children.
// - LOCALs, params, MODIFIERs, JUMP-less constructs are invisible.
// - if/while/for/do: the condition's root branches; back-edges target the
//   condition's first leaf. switch: cond root -> every JUMP_TARGET (plus the
//   continuation when there is no default); case values chain after their
//   JUMP_TARGET; fallthrough is natural chaining.
// - break -> after the innermost loop/switch; continue -> loop re-entry
//   (cond for while/do, update for for-loops).
// - <operator>.conditional and the short-circuit operators branch:
//   cond/lhs root -> arm entries (or past them), arms -> the call node.
// - RETURN -> METHOD_RETURN; METHOD -> first node (src uses the symbolic
//   M:<full> address so first-wins resolution applies).

struct DNode {
    label: String,
    name: String,
    code1: String,
    fullcode: String,
    full: String,
    has_arg: bool,
    arg_index: i64,
    inlined: bool,
    code2: String,
    children: Vec<usize>,
    parent: Option<usize>,
    idx: usize,
}

/// Full CODE= property value: everything between `CODE=` and the next
/// ` UPPERCASE_KEY=` property (CODE values are C code — lowercase/operators —
/// so an uppercase-keyed `=` reliably marks the boundary).
fn extract_code(rest: &str) -> String {
    let Some(start) = rest.find(" CODE=").map(|i| i + 6) else {
        return String::new();
    };
    let tail = &rest[start..];
    let bytes = tail.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b' ' {
            // look ahead for KEY=
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j].is_ascii_uppercase() || bytes[j] == b'_') {
                j += 1;
            }
            if j > i + 1 && j < bytes.len() && bytes[j] == b'=' {
                break;
            }
        }
        i += 1;
    }
    tail[..i].to_string()
}

fn parse_dump_block(text: &str) -> Vec<DNode> {
    let mut arena: Vec<DNode> = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new(); // (depth, arena id)
    for (idx, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let depth = (line.len() - line.trim_start().len()) / 2;
        let rest = line.trim_start();
        let label = rest.split(' ').next().unwrap_or("").to_string();
        let grab = |key: &str| -> String {
            rest.find(key)
                .map(|i| {
                    rest[i + key.len()..]
                        .split(' ')
                        .next()
                        .unwrap_or("")
                        .to_string()
                })
                .unwrap_or_default()
        };
        let id = arena.len();
        while stack.last().is_some_and(|t| t.0 >= depth) {
            stack.pop();
        }
        let parent = stack.last().map(|(_, pid)| *pid);
        if let Some(pid) = parent {
            arena[pid].children.push(id);
        }
        let code2 = rest
            .find(" CODE=")
            .map(|i| {
                let mut it = rest[i + 6..].split_whitespace();
                it.next();
                it.next().unwrap_or("").trim_end_matches(';').to_string()
            })
            .unwrap_or_default();
        let arg_index = rest
            .find(" ARGUMENT_INDEX=")
            .and_then(|i| rest[i + 16..].split(' ').next())
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        arena.push(DNode {
            label,
            name: grab(" NAME="),
            code1: grab(" CODE="),
            fullcode: extract_code(rest),
            full: grab(" FULL_NAME="),
            has_arg: rest.contains(" ARGUMENT_INDEX="),
            arg_index,
            inlined: rest.contains(" DISPATCH_TYPE=INLINED"),
            code2,
            children: Vec::new(),
            parent,
            idx,
        });
        stack.push((depth, id));
    }
    arena
}

struct CfgBuilder<'a> {
    arena: &'a [DNode],
    block: &'a str,
    mret: String,
    edges: Vec<(String, String)>,
    breaks: Vec<Vec<String>>,        // per breakable construct
    continues: Vec<Vec<String>>,     // per loop
    labels: HashMap<String, String>, // label name -> JUMP_TARGET addr
}

impl CfgBuilder<'_> {
    fn addr(&self, id: usize) -> String {
        let n = &self.arena[id];
        if n.idx == 0 {
            format!("M:{}", n.full)
        } else {
            format!("{}#{}", self.block, n.idx)
        }
    }

    fn connect(&mut self, srcs: &[String], dst: &str) {
        for s in srcs {
            self.edges.push((s.clone(), dst.to_string()));
        }
    }

    /// Sequence children as statements: pending outs flow into the next
    /// construct's entry; transparent children pass outs through.
    fn seq(&mut self, ids: &[usize]) -> (Option<String>, Vec<String>) {
        let mut entry: Option<String> = None;
        let mut pending: Vec<String> = Vec::new();
        for &c in ids {
            let (e, o) = self.build(c);
            if let Some(e) = e {
                self.connect(&pending, &e);
                if entry.is_none() {
                    entry = Some(e);
                }
                pending = o;
            }
        }
        (entry, pending)
    }

    fn build(&mut self, id: usize) -> (Option<String>, Vec<String>) {
        let n = &self.arena[id];
        let me = self.addr(id);
        let kids = n.children.clone();
        match n.label.as_str() {
            "IDENTIFIER" | "LITERAL" | "FIELD_IDENTIFIER" | "METHOD_REF" | "TYPE_REF"
            | "UNKNOWN" | "JUMP_TARGET" => (Some(me.clone()), vec![me]),
            "CALL" => match self.arena[id].name.as_str() {
                "<operator>.conditional" => {
                    if kids.len() != 3 {
                        return self.build_sequential_call(&kids, &me);
                    }
                    let (e1, o1) = self.build(kids[0]);
                    let (e2, o2) = self.build(kids[1]);
                    let (e3, o3) = self.build(kids[2]);
                    if let Some(e2) = &e2 {
                        self.connect(&o1, e2);
                    }
                    if let Some(e3) = &e3 {
                        self.connect(&o1, e3);
                    }
                    self.connect(&o2, &me);
                    self.connect(&o3, &me);
                    (e1, vec![me])
                }
                "<operator>.logicalAnd" | "<operator>.logicalOr" => {
                    if kids.len() != 2 {
                        return self.build_sequential_call(&kids, &me);
                    }
                    // Short-circuit: the lhs root branches to the rhs entry
                    // and directly past it to the call node.
                    let (e1, o1) = self.build(kids[0]);
                    let (e2, o2) = self.build(kids[1]);
                    if let Some(e2) = &e2 {
                        self.connect(&o1, e2);
                    }
                    self.connect(&o1, &me);
                    self.connect(&o2, &me);
                    (e1, vec![me])
                }
                _ if self.arena[id].inlined => {
                    // INLINED macro call: args -> call -> expansion content;
                    // both the call and the expansion exit flow onward, and
                    // the expansion BLOCK itself is invisible.
                    let (split, _) = kids.split_at(kids.len().saturating_sub(1));
                    let (ae, ao) = self.seq(split);
                    self.connect(&ao, &me);
                    let mut outs = vec![me.clone()];
                    if let Some(&blk) = kids.last() {
                        let bkids = self.arena[blk].children.clone();
                        let (xe, xo) = self.seq(&bkids);
                        if let Some(xe) = &xe {
                            self.connect(std::slice::from_ref(&me), xe);
                        }
                        outs.extend(xo);
                    }
                    (ae.or(Some(me)), outs)
                }
                _ => self.build_sequential_call(&kids, &me),
            },
            "BLOCK" => {
                // An expression block (comma operator) is a child of a CALL;
                // statement blocks (incl. stub bodies, which carry a spurious
                // ARGUMENT_INDEX) are transparent.
                let is_expr = n.parent.is_some_and(|p| self.arena[p].label == "CALL");
                let (entry, outs) = self.seq(&kids);
                if is_expr {
                    self.connect(&outs, &me);
                    (entry.or(Some(me.clone())), vec![me])
                } else {
                    (entry, outs)
                }
            }
            "RETURN" => {
                let (entry, outs) = self.seq(&kids);
                self.connect(&outs, &me);
                let mret = self.mret.clone();
                self.edges.push((me.clone(), mret));
                (entry.or(Some(me)), vec![])
            }
            "CONTROL_STRUCTURE" => self.build_control(id, me, &kids),
            _ => (None, vec![]), // LOCAL, MODIFIER, METHOD, TYPE_DECL, params, ...
        }
    }

    fn build_sequential_call(
        &mut self,
        children: &[usize],
        call: &str,
    ) -> (Option<String>, Vec<String>) {
        let (entry, outs) = self.seq(children);
        self.connect(&outs, call);
        (
            entry.or_else(|| Some(call.to_string())),
            vec![call.to_string()],
        )
    }

    fn build_control(
        &mut self,
        id: usize,
        me: String,
        kids: &[usize],
    ) -> (Option<String>, Vec<String>) {
        let kind = self.arena[id]
            .code1
            .split(['(', ';', ' '])
            .next()
            .unwrap_or("")
            .to_string();
        let block_child = |b: &Self| kids.iter().copied().find(|&c| b.arena[c].label == "BLOCK");
        match kind.as_str() {
            "if" => {
                let (ce, co) = self.build(kids[0]);
                let then = block_child(self);
                let els = kids.iter().copied().find(|&c| {
                    self.arena[c].label == "CONTROL_STRUCTURE" && self.arena[c].code1 == "else"
                });
                let mut outs = Vec::new();
                if let Some(t) = then {
                    let (te, to) = self.build(t);
                    if let Some(te) = &te {
                        self.connect(&co, te);
                    }
                    outs.extend(to);
                }
                if let Some(e) = els {
                    let eb = self.arena[e]
                        .children
                        .iter()
                        .copied()
                        .find(|&c| self.arena[c].label == "BLOCK");
                    if let Some(eb) = eb {
                        let (ee, eo) = self.build(eb);
                        if let Some(ee) = &ee {
                            self.connect(&co, ee);
                        }
                        outs.extend(eo);
                    }
                } else {
                    outs.extend(co);
                }
                (ce, outs)
            }
            "while" => {
                self.breaks.push(Vec::new());
                self.continues.push(Vec::new());
                let (ce, co) = self.build(kids[0]);
                let body = block_child(self);
                if let Some(b) = body {
                    let (be, bo) = self.build(b);
                    if let Some(be) = &be {
                        self.connect(&co, be);
                    }
                    if let Some(ce) = &ce {
                        self.connect(&bo, ce);
                    }
                }
                let brs = self.breaks.pop().unwrap();
                let conts = self.continues.pop().unwrap();
                if let Some(ce) = &ce {
                    self.connect(&conts, ce);
                }
                let mut outs = co;
                outs.extend(brs);
                (ce, outs)
            }
            "do" => {
                self.breaks.push(Vec::new());
                self.continues.push(Vec::new());
                let body = block_child(self);
                let cond = kids
                    .iter()
                    .copied()
                    .find(|&c| self.arena[c].label != "BLOCK");
                let (be, bo) = body.map(|b| self.build(b)).unwrap_or((None, vec![]));
                let (ce, co) = cond.map(|c| self.build(c)).unwrap_or((None, vec![]));
                if let Some(ce) = &ce {
                    self.connect(&bo, ce);
                }
                if let Some(be) = &be {
                    self.connect(&co, be);
                }
                let brs = self.breaks.pop().unwrap();
                let conts = self.continues.pop().unwrap();
                if let Some(ce) = &ce {
                    self.connect(&conts, ce);
                }
                let mut outs = co;
                outs.extend(brs);
                (be.or(ce), outs)
            }
            "for" => {
                self.breaks.push(Vec::new());
                self.continues.push(Vec::new());
                // Positional after skipping LOCALs: [init, cond, update,
                // body?] — empty clauses are placeholder BLOCKs, and a comma
                // update is itself a BLOCK, so positions are the only truth.
                let rest: Vec<usize> = kids
                    .iter()
                    .copied()
                    .filter(|&c| self.arena[c].label != "LOCAL")
                    .collect();
                let init = rest.first().copied();
                let cond = rest.get(1).copied();
                let update = rest.get(2).copied();
                let body = rest.get(3).copied();
                let (ie, io) = init.map(|i| self.build(i)).unwrap_or((None, vec![]));
                let (ce, co) = cond.map(|c| self.build(c)).unwrap_or((None, vec![]));
                let (ue, uo) = update.map(|u| self.build(u)).unwrap_or((None, vec![]));
                let (be, bo) = body.map(|b| self.build(b)).unwrap_or((None, vec![]));
                if let Some(ce) = &ce {
                    self.connect(&io, ce);
                    self.connect(&uo, ce);
                }
                // True branch: body if present, else straight to the update
                // (musl `for (...);` empty-body loops), else the cond itself.
                if let Some(t) = be.as_ref().or(ue.as_ref()).or(ce.as_ref()) {
                    let t = t.clone();
                    self.connect(&co, &t);
                }
                if let Some(ue) = &ue {
                    self.connect(&bo, ue);
                }
                let brs = self.breaks.pop().unwrap();
                let conts = self.continues.pop().unwrap();
                if let Some(ue) = &ue {
                    self.connect(&conts, ue);
                }
                let mut outs = co;
                outs.extend(brs);
                (ie.or(ce), outs)
            }
            "switch" => {
                self.breaks.push(Vec::new());
                let (ce, co) = self.build(kids[0]);
                let body = block_child(self);
                let mut outs = Vec::new();
                let mut has_default = false;
                if let Some(b) = body {
                    let bkids = self.arena[b].children.clone();
                    for &c in &bkids {
                        if self.arena[c].label == "JUMP_TARGET" {
                            let jt = self.addr(c);
                            self.connect(&co, &jt);
                            if self.arena[c].name == "default" {
                                has_default = true;
                            }
                        }
                    }
                    // Natural chaining inside the body = fallthrough; the
                    // dispatch edges above are the only entries.
                    let (_, bo) = self.seq(&bkids);
                    outs.extend(bo);
                }
                if !has_default {
                    outs.extend(co.clone());
                }
                outs.extend(self.breaks.pop().unwrap());
                (ce, outs)
            }
            "goto" => {
                // `goto L;` -> edge to the JUMP_TARGET named L; no fallthrough.
                if let Some(t) = self.labels.get(&self.arena[id].code2).cloned() {
                    self.edges.push((me.clone(), t));
                }
                (Some(me), vec![])
            }
            "break" => {
                if let Some(b) = self.breaks.last_mut() {
                    b.push(me.clone());
                }
                (Some(me), vec![])
            }
            "continue" => {
                if let Some(c) = self.continues.last_mut() {
                    c.push(me.clone());
                }
                (Some(me), vec![])
            }
            _ => (None, vec![]), // else (handled by if), goto/label: TODO
        }
    }
}

/// CFG edges for one dump block (a method subtree).
fn cfg_edges_for_block(block: &str, text: &str) -> Vec<(String, String)> {
    let arena = parse_dump_block(text);
    if arena.is_empty() || arena[0].label != "METHOD" {
        return Vec::new();
    }
    let root_kids = arena[0].children.clone();
    let mret = root_kids
        .iter()
        .copied()
        .find(|&c| arena[c].label == "METHOD_RETURN");
    let body = root_kids
        .iter()
        .copied()
        .find(|&c| arena[c].label == "BLOCK");
    let Some(mret) = mret else { return Vec::new() };
    let mut labels: HashMap<String, String> = HashMap::new();
    for n in &arena {
        if n.label == "JUMP_TARGET" && n.name != "case" && n.name != "default" {
            labels.insert(n.name.clone(), format!("{block}#{}", n.idx));
        }
    }
    let mut b = CfgBuilder {
        arena: &arena,
        block,
        mret: format!("{block}#{}", arena[mret].idx),
        edges: Vec::new(),
        breaks: Vec::new(),
        continues: Vec::new(),
        labels,
    };
    let m_addr = format!("M:{}", arena[0].full);
    let mret_addr = b.mret.clone();
    let (entry, outs) = body.map(|x| b.build(x)).unwrap_or((None, vec![]));
    match entry {
        Some(e) => b.edges.push((m_addr, e)),
        None => b.edges.push((m_addr, mret_addr.clone())),
    }
    b.connect(&outs, &mret_addr);
    b.edges
}

// ---- Reaching definitions (M7) -----------------------------------------
// Port of Joern's ReachingDefPass + DdgGenerator, validated against the
// FLOWS| oracle section. GEN: parameters define themselves at method entry;
// a non-field-access CALL defines {itself} ∪ {its Call/Identifier arguments}.
// KILL: a call's gen kills other defs of the same variables. The dataflow is
// solved over the (already byte-identical) CFG; edges are then added by the
// six DdgGenerator routines. Exact entry/cross-arg rules are pinned by diff.

/// Index-level CFG for a block, recovered from the address-level CFG (node 0
/// is the METHOD, addressed M:full; others are `block#idx`).
fn cfg_index_edges(block: &str, text: &str, n: usize) -> Vec<(usize, usize)> {
    let prefix = format!("{block}#");
    let to_idx = |a: &str| -> Option<usize> {
        if a.starts_with("M:") {
            Some(0)
        } else {
            a.strip_prefix(&prefix)
                .and_then(|s| s.parse::<usize>().ok())
        }
    };
    cfg_edges_for_block(block, text)
        .iter()
        .filter_map(|(s, d)| Some((to_idx(s)?, to_idx(d)?)))
        .filter(|&(s, d)| s < n && d < n)
        .collect()
}

// Joern v4.0.555 `MemberAccess.isFieldAccess` — the predicate
// `ReachingDefTransferFunction.initGen` uses to EXCLUDE calls from the GEN set.
// It is far broader than literal fieldAccess: it covers every member-access
// operator plus `indirection`, `getElementPtr` and `sizeOf`. A call matching
// this generates NO definition (not even itself); it only becomes a def when
// it is an argument of a non-field-access parent call (gen'd there).
fn is_field_access(name: &str) -> bool {
    matches!(
        name,
        "<operator>.memberAccess"
            | "<operator>.indirectComputedMemberAccess"
            | "<operator>.indirectMemberAccess"
            | "<operator>.computedMemberAccess"
            | "<operator>.indirection"
            | "<operator>.fieldAccess"
            | "<operator>.indirectFieldAccess"
            | "<operator>.indexAccess"
            | "<operator>.indirectIndexAccess"
            | "<operator>.getElementPtr"
            | "<operator>.sizeOf"
    )
}

// The UsageAnalyzer `containerSet`: the narrow set of access operators whose
// isContainer/isPart base relationships are tracked. (Distinct from the broad
// `isFieldAccess` GEN filter above.)
fn is_container_access(name: &str) -> bool {
    matches!(
        name,
        "<operator>.fieldAccess"
            | "<operator>.indirectFieldAccess"
            | "<operator>.indexAccess"
            | "<operator>.indirectIndexAccess"
    )
}

// The UsageAnalyzer `indirectionAccessSet`.
fn is_indirection_access(name: &str) -> bool {
    matches!(name, "<operator>.addressOf" | "<operator>.indirection")
}

// Joern `MemberAccess.isGenericMemberAccessName` — the predicate
// `ReachingDefTransferFunction.initKill` uses to SKIP kill-set computation. A
// call matching it kills nothing (e.g. `&v` / addressOf does not redefine v).
// Differs from isFieldAccess: this set has addressOf + pointerShift but no sizeOf.
fn is_generic_member_access(name: &str) -> bool {
    matches!(
        name,
        "<operator>.memberAccess"
            | "<operator>.indirectComputedMemberAccess"
            | "<operator>.indirectMemberAccess"
            | "<operator>.computedMemberAccess"
            | "<operator>.indirection"
            | "<operator>.addressOf"
            | "<operator>.fieldAccess"
            | "<operator>.indirectFieldAccess"
            | "<operator>.indexAccess"
            | "<operator>.indirectIndexAccess"
            | "<operator>.pointerShift"
            | "<operator>.getElementPtr"
    )
}

/// Joern v4.0.555 DefaultSemantics operator flow mappings: (srcArgIdx,
/// dstArgIdx), dst -1 = return value. `None` = no explicit semantics
/// (pass-through: all flows valid). `Some(vec![])` = sizeOf (no flows).
/// Verbatim from the decompiled DefaultSemantics.operatorFlows().
fn operator_semantics(name: &str) -> Option<Vec<(i64, i64)>> {
    let compound = vec![(2, 1), (1, 1), (2, -1)];
    let access1 = vec![(1, -1)];
    let incdec = vec![(1, 1), (1, -1)];
    let v: Vec<(i64, i64)> = match name {
        "<operator>.addition" => vec![(1, -1), (2, -1)],
        "<operator>.addressOf" => access1,
        "<operator>.assignment" => vec![(2, 1), (2, -1)],
        "<operators>.assignmentAnd"
        | "<operators>.assignmentArithmeticShiftRight"
        | "<operator>.assignmentDivision"
        | "<operators>.assignmentExponentiation"
        | "<operators>.assignmentLogicalShiftRight"
        | "<operator>.assignmentMinus"
        | "<operators>.assignmentModulo"
        | "<operator>.assignmentMultiplication"
        | "<operators>.assignmentOr"
        | "<operator>.assignmentPlus"
        | "<operators>.assignmentShiftLeft"
        | "<operators>.assignmentXor" => compound,
        "<operator>.cast" => vec![(1, -1), (2, -1)],
        "<operator>.computedMemberAccess" => access1,
        "<operator>.conditional" => vec![(2, -1), (3, -1)],
        "<operator>.elvis" => vec![(1, -1), (2, -1)],
        "<operator>.notNullAssert" => access1,
        "<operator>.fieldAccess" => access1,
        "<operator>.getElementPtr" => access1,
        "<operator>.incBy" => vec![(1, 1), (2, 1), (3, 1), (4, 1)],
        "<operator>.indexAccess" => access1,
        "<operator>.indirectComputedMemberAccess" => access1,
        "<operator>.indirectFieldAccess" => access1,
        "<operator>.indirectIndexAccess" => vec![(1, -1), (2, 1)],
        "<operator>.indirectMemberAccess" => access1,
        "<operator>.indirection" => access1,
        "<operator>.memberAccess" => access1,
        "<operator>.pointerShift" => access1,
        "<operator>.postDecrement"
        | "<operator>.postIncrement"
        | "<operator>.preDecrement"
        | "<operator>.preIncrement" => incdec,
        "<operator>.sizeOf" => vec![],
        "free" => vec![(1, 1)],
        // modulo, arrayInitializer, the literals: PTF (pass-through) — and any
        // operator not listed (subtraction, multiplication, comparisons,
        // logicalAnd/Or, …) — get no explicit semantics: pass-through.
        _ => return None,
    };
    Some(v)
}

/// The variable string a node defines/uses (DdgGenerator.nodeToEdgeLabel):
/// parameters use their name, everything else its code.
fn node_var(d: &DNode) -> String {
    if d.label == "METHOD_PARAMETER_IN" || d.label == "METHOD_PARAMETER_OUT" {
        d.name.clone()
    } else if d.label == "BLOCK" && d.fullcode.is_empty() {
        // An empty expression block's `code` property renders as "<empty>".
        "<empty>".to_string()
    } else {
        d.fullcode.clone()
    }
}

/// REACHING_DEF flows for one method block: (variable, srcIdxAddr, dstIdxAddr)
/// where addresses are `block#idx` or `M:full` for the method node.
fn reaching_def_flows(block: &str, text: &str) -> Vec<(String, String, String)> {
    let arena = parse_dump_block(text);
    let n = arena.len();
    if n == 0 || arena[0].label != "METHOD" {
        return Vec::new();
    }
    let method_addr = format!("M:{}", arena[0].full);
    let addr = |i: usize| -> String {
        if i == 0 {
            method_addr.clone()
        } else {
            format!("{block}#{i}")
        }
    };

    // Own nodes only: a file-global/`<clinit>` dump embeds the full nested
    // method dumps, but reaching-def is per method — descend from the root but
    // never into a nested METHOD subtree (and exclude the nested METHOD node
    // itself; it is a separate method addressed via first-wins elsewhere).
    let own: HashSet<usize> = {
        let mut set = HashSet::new();
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            set.insert(i);
            for &c in &arena[i].children {
                if arena[c].label == "METHOD" {
                    continue; // nested method: separate, skip whole subtree
                }
                stack.push(c);
            }
        }
        set
    };

    // --- arguments / uses helpers ---
    let args_of = |c: usize| -> Vec<usize> {
        // call.argument = AST children with ARGUMENT_INDEX, minus FieldIdentifier.
        let mut v: Vec<usize> = arena[c]
            .children
            .iter()
            .copied()
            .filter(|&k| arena[k].has_arg && arena[k].label != "FIELD_IDENTIFIER")
            .collect();
        v.sort_by_key(|&k| arena[k].arg_index);
        v
    };
    let uses_of = |i: usize| -> Vec<usize> {
        match arena[i].label.as_str() {
            "CALL" => args_of(i),
            "RETURN" => arena[i].children.clone(),
            "METHOD_PARAMETER_OUT" => vec![i],
            _ => Vec::new(),
        }
    };
    let is_gen_arg = |k: usize| matches!(arena[k].label.as_str(), "CALL" | "IDENTIFIER");

    // --- GEN / KILL ---
    // def id == node index. Each def carries a variable.
    let mut def_var: HashMap<usize, String> = HashMap::new();
    let mut gen: HashMap<usize, Vec<usize>> = HashMap::new(); // node -> defs generated
                                                              // parameters
    let params: Vec<usize> = arena[0]
        .children
        .iter()
        .copied()
        .filter(|&k| arena[k].label == "METHOD_PARAMETER_IN")
        .collect();
    let mut entry_gen: Vec<usize> = Vec::new();
    for &p in &params {
        def_var.insert(p, node_var(&arena[p]));
        entry_gen.push(p);
    }
    // calls
    // All call nodes are processed by the call-site routines (Joern runs
    // addEdgesToCallSite for every Call). But GEN excludes field-access calls
    // (Joern's defsForCalls.filterNot(isFieldAccess)): such a call defines no
    // value of its own — it only becomes a def when it is itself an argument
    // of a non-field-access parent (handled by the parent's gen below).
    let calls: Vec<usize> = (0..n)
        .filter(|&i| own.contains(&i) && arena[i].label == "CALL")
        .collect();
    let gen_calls: Vec<usize> = calls
        .iter()
        .copied()
        .filter(|&c| !is_field_access(&arena[c].name))
        .collect();
    for &c in &gen_calls {
        // super.initGen: gen(call) = {call} ++ {Call|Identifier arguments}.
        // (Faithful to ReachingDefTransferFunction.initGen — no per-operator
        // arg exclusion; indirection etc. are already filtered out of gen_calls
        // by the broad isFieldAccess above.)
        let mut g = vec![c];
        def_var.insert(c, node_var(&arena[c]));
        for a in args_of(c) {
            if is_gen_arg(a) {
                def_var.insert(a, node_var(&arena[a]));
                g.push(a);
            }
        }
        gen.insert(c, g);
    }
    // OptimizedReachingDefTransferFunction.withoutLoneIdentifiers: an identifier
    // ARGUMENT whose name is (a) not a parameter or local, (b) not used in any
    // return, and (c) unique by name across all call arguments in the method, is
    // a "lone identifier" (e.g. the type operand `int[2][3]` of `<operator>.alloc`,
    // or a bare type name). Such a def is REMOVED from its call's gen set.
    let mut lone_idents: Vec<usize> = Vec::new();
    {
        let mut name_excluded: HashSet<String> =
            params.iter().map(|&p| arena[p].name.clone()).collect();
        // `i` is a node id, not just an arena index: it keys `own` too.
        #[allow(clippy::needless_range_loop)]
        for i in 0..n {
            if own.contains(&i) && arena[i].label == "LOCAL" {
                name_excluded.insert(arena[i].name.clone());
            }
        }
        for i in 0..n {
            if own.contains(&i) && arena[i].label == "RETURN" {
                let mut stack = vec![i];
                while let Some(x) = stack.pop() {
                    if arena[x].label == "IDENTIFIER" {
                        name_excluded.insert(arena[x].name.clone());
                    }
                    for &c in &arena[x].children {
                        stack.push(c);
                    }
                }
            }
        }
        let mut by_name: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
        for &c in &calls {
            for a in args_of(c) {
                if arena[a].label == "IDENTIFIER" && !name_excluded.contains(&arena[a].name) {
                    by_name
                        .entry(arena[a].name.clone())
                        .or_default()
                        .push((c, a));
                }
            }
        }
        for occ in by_name.values() {
            if occ.len() == 1 {
                let (c, a) = occ[0];
                lone_idents.push(a);
                if let Some(g) = gen.get_mut(&c) {
                    g.retain(|&x| x != a);
                }
            }
        }
    }
    // kill(call) = other defs of the variables in gen(call). initKill skips
    // calls matching isGenericMemberAccessName (addressOf/pointerShift and the
    // member accesses): `&v` gens a def of v but kills no prior v def.
    let mut kill: HashMap<usize, Vec<usize>> = HashMap::new();
    for &c in &gen_calls {
        if is_generic_member_access(&arena[c].name) {
            continue;
        }
        let vars: HashSet<String> = gen[&c].iter().map(|&d| def_var[&d].clone()).collect();
        let g: HashSet<usize> = gen[&c].iter().copied().collect();
        let k: Vec<usize> = def_var
            .iter()
            .filter(|(d, v)| !g.contains(d) && vars.contains(*v))
            .map(|(d, _)| *d)
            .collect();
        kill.insert(c, k);
    }

    // --- dataflow fixpoint over the CFG ---
    let cfg = cfg_index_edges(block, text, n);
    // Nodes that are part of this method's CFG (the reaching-def flow graph's
    // node set). Used to tell an EXPRESSION block (comma operator — in the CFG)
    // from a statement / INLINED-macro / stub body block (not in the CFG).
    let cfg_nodes: HashSet<usize> = cfg.iter().flat_map(|&(s, d)| [s, d]).collect();
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(s, d) in &cfg {
        preds[d].push(s);
    }
    // ReachingDefFlowGraph quirk (decompiled initPred): the FIRST body node's
    // predecessor is the param-chain entry (method), REPLACING its CFG preds.
    // When the loop condition is the first body node (no statements precede the
    // loop, e.g. bsearch), this drops the loop back-edges into it — so loop-body
    // defs don't flow back to the condition. The method node (0) points to the
    // first body node.
    for &(s, d) in &cfg {
        if s == 0 {
            preds[d] = vec![0];
        }
    }
    let mut out: Vec<HashSet<usize>> = vec![HashSet::new(); n];
    out[0] = entry_gen.iter().copied().collect();
    let empty: Vec<usize> = Vec::new();
    let mut changed = true;
    while changed {
        changed = false;
        for i in 0..n {
            let mut in_set: HashSet<usize> = HashSet::new();
            for &p in &preds[i] {
                in_set.extend(&out[p]);
            }
            let g = gen.get(&i).unwrap_or(&empty);
            let k = kill.get(&i).unwrap_or(&empty);
            let mut new_out: HashSet<usize> =
                in_set.iter().copied().filter(|d| !k.contains(d)).collect();
            new_out.extend(g.iter().copied());
            if i == 0 {
                new_out.extend(entry_gen.iter().copied());
            }
            if new_out != out[i] {
                out[i] = new_out;
                changed = true;
            }
        }
    }
    let in_of = |i: usize| -> HashSet<usize> {
        let mut s = HashSet::new();
        for &p in &preds[i] {
            s.extend(&out[p]);
        }
        s
    };

    // Joern's reaching-def runs over ReachingDefFlowGraph, not the raw CFG: the
    // exit and the METHOD_PARAMETER_OUTs are fed by a param-out chain whose
    // source is the single `lastActualCfgNode` (the earliest cfg-predecessor of
    // METHOD_RETURN) — NOT the union of all returns. So a post-loop `return`
    // never contributes its (bypass) param defs to the exit. For a
    // single-return method this is identical to the union.
    let exit_in: HashSet<usize> = (0..n)
        .find(|&i| own.contains(&i) && arena[i].label == "METHOD_RETURN")
        .and_then(|e| preds[e].iter().copied().filter(|&p| p < n).min())
        .map(|la| out[la].clone())
        .unwrap_or_default();

    // isUsing(use, inElem) — faithful port of UsageAnalyzer.isUsing =
    // sameVariable || isContainer || isPart || isAlias. Joern compares
    // `nodeToString(use)` (NAME for identifier/param, CODE for expression)
    // against a target string via `Option.contains`, i.e. EXACT equality.
    let is_using = |use_i: usize, def_i: usize| -> bool { rd_is_using(&arena, use_i, def_i) };

    let mut flows: Vec<(String, String, String)> = Vec::new();
    // Gate every candidate edge addEdge(from=s, to=d) through
    // isValidEdge(child=d, parent=s), exactly as DdgGenerator.addEdge does.
    let push = |var: String, s: usize, d: usize, flows: &mut Vec<(String, String, String)>| {
        if rd_valid_edge(&arena, d, s) {
            flows.push((var, addr(s), addr(d)));
        }
    };

    // method-return (exit) index
    let exit = (0..n).find(|&i| own.contains(&i) && arena[i].label == "METHOD_RETURN");

    // isDdgNode: everything EXCEPT Method, ControlStructure, FieldIdentifier,
    // JumpTarget, MethodReturn. A BLOCK is a ddg node only when it is itself a
    // CFG node — i.e. an EXPRESSION block used as a call argument (the
    // comma-operator `(b++, b+1)`); statement blocks (method/loop bodies) are
    // not in the CFG and never get def-use edges.
    let is_ddg = |i: usize| match arena[i].label.as_str() {
        "CALL"
        | "IDENTIFIER"
        | "LITERAL"
        | "RETURN"
        | "METHOD_PARAMETER_IN"
        | "METHOD_REF"
        | "TYPE_REF" => true,
        "BLOCK" => cfg_nodes.contains(&i),
        _ => false,
    };

    // usedIncomingDefs(node): map use -> defs in in(node) it uses.
    let used_incoming = |i: usize| -> Vec<(usize, Vec<usize>)> {
        let ins = in_of(i);
        uses_of(i)
            .into_iter()
            .map(|u| {
                let ds: Vec<usize> = ins.iter().copied().filter(|&d| is_using(u, d)).collect();
                (u, ds)
            })
            .collect()
    };

    // Write-only args: an argument that is a definition target but not a read
    // under its call's semantics (e.g. the LHS of plain `=`, which has flow
    // (2->1) but no (1->_)). Such args get no entry edge and are not first-loop
    // uses. A compound-assignment LHS (`a += b`, flow includes (1->1)) IS read,
    // so it is NOT write-only.
    let mut assign_lhs: HashSet<usize> = HashSet::new();
    for &c in &calls {
        if let Some(maps) = operator_semantics(&arena[c].name) {
            for a in args_of(c) {
                let idx = arena[a].arg_index;
                let used = maps.iter().any(|&(s, _)| s == idx);
                let defined = maps.iter().any(|&(_, d)| d == idx);
                if defined && !used {
                    assign_lhs.insert(a);
                }
            }
        }
    }

    // 1. addEdgesFromEntryNode: a ddg node whose usedIncomingDefs are all empty
    // (no reaching def is actually used) gets method -> node, var "". This
    // includes `return 0` (literal, no reaching def) but not `return SQR(n)`
    // (the call is a reaching def). Non-INLINED calls (function calls,
    // operators) never get an entry edge; INLINED macro calls do (when their
    // args carry no reaching def). isValidEdge in push drops write-only targets.
    // `i` is a node id, not just an arena index: it keys `own`, `assign_lhs`,
    // `is_ddg` and `used_incoming` as well.
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        if i == 0 || !own.contains(&i) || !is_ddg(i) || assign_lhs.contains(&i) {
            continue;
        }
        if arena[i].label == "CALL" && !arena[i].inlined && !args_of(i).is_empty() {
            continue;
        }
        if used_incoming(i).iter().all(|(_, ds)| ds.is_empty()) {
            push(String::new(), 0, i, &mut flows);
        }
    }

    // 2. call sites
    for &c in &calls {
        let g_set: Vec<usize> = gen.get(&c).cloned().unwrap_or_default();
        let is_gen_arg_node = |x: usize| g_set.contains(&x) && x != c;
        // first loop: reaching defs into each arg use (the assignment LHS is a
        // pure write target, not a use, so it is excluded).
        for (u, ds) in used_incoming(c) {
            if assign_lhs.contains(&u) {
                continue;
            }
            for d in ds {
                if d != u {
                    push(node_var(&arena[d]), d, u, &mut flows);
                }
            }
        }
        // second loop: every arg use -> every gen member, then filtered by
        // the call's flow SEMANTICS (Joern's EdgeValidator.isValidEdge). For a
        // call with explicit semantics, an arg->arg edge is valid iff there is
        // a flow mapping (srcArgIdx -> dstArgIdx) and arg->return iff
        // (srcArgIdx -> -1). Operators with no semantics are pass-through
        // (all edges valid); `sizeOf` has empty semantics (no flows).
        let _ = is_gen_arg_node;
        let sem = operator_semantics(&arena[c].name);
        for u in args_of(c) {
            if !is_ddg(u) {
                continue;
            }
            let u_idx = arena[u].arg_index;
            for &gnode in &g_set {
                if u == gnode {
                    continue;
                }
                // An argument always taints its call's output node. An
                // argument -> sibling-argument edge is gated by the call's
                // flow semantics (pass-through when the operator has none).
                let valid = gnode == c
                    || match &sem {
                        None => true,
                        Some(maps) => maps.contains(&(u_idx, arena[gnode].arg_index)),
                    };
                if valid {
                    push(node_var(&arena[u]), u, gnode, &mut flows);
                }
            }
        }
        // addEdgeForBlock: a block argument (a comma-operator/expression block)
        // routes its VALUE — its last AST child — into the enclosing call. If the
        // last child is an identifier, the defs it uses flow into the block, then
        // block -> call; if it is a call, that call -> block, then block -> call.
        for b in args_of(c) {
            if arena[b].label != "BLOCK" || !cfg_nodes.contains(&b) {
                continue;
            }
            let Some(&last) = arena[b].children.last() else {
                continue;
            };
            match arena[last].label.as_str() {
                "IDENTIFIER" => {
                    let ins = in_of(b);
                    let mut ds: Vec<usize> = ins
                        .iter()
                        .copied()
                        .filter(|&d| is_using(last, d))
                        .filter(|&d| matches!(arena[d].label.as_str(), "IDENTIFIER" | "CALL"))
                        .collect();
                    ds.sort();
                    let any = !ds.is_empty();
                    for d in ds {
                        push(node_var(&arena[d]), d, b, &mut flows);
                    }
                    if any {
                        push(String::new(), b, c, &mut flows);
                    }
                }
                "CALL" => {
                    push(node_var(&arena[last]), last, b, &mut flows);
                    push(String::new(), b, c, &mut flows);
                }
                _ => {}
            }
        }
    }

    // 3. returns
    for i in 0..n {
        if arena[i].label != "RETURN" || !own.contains(&i) {
            continue;
        }
        for (u, ins) in used_incoming(i) {
            push(arena[u].fullcode.clone(), u, i, &mut flows);
            for d in ins {
                if d != u {
                    push(node_var(&arena[d]), d, u, &mut flows);
                }
            }
        }
        if let Some(e) = exit {
            push("<RET>".into(), i, e, &mut flows);
        }
    }

    // 4. method parameter out
    for i in 0..n {
        if arena[i].label != "METHOD_PARAMETER_OUT" || !own.contains(&i) {
            continue;
        }
        // paramIn -> paramOut, name
        if let Some(&pin) = params.iter().find(|&&p| arena[p].name == arena[i].name) {
            push(arena[pin].name.clone(), pin, i, &mut flows);
        }
        // addEdgesToMethodParameterOut: usedIncomingDefs(paramOut) — the defs
        // live at method exit (param-out chain) that the paramOut isUsing. The
        // paramOut's own "use" is itself, so the match is isUsing(paramOut, d):
        // e.g. `*l` (indirection of l) is used by the paramOut of l, and `q.x`
        // (a write through q) by the paramOut of q.
        let mut ds: Vec<usize> = exit_in
            .iter()
            .copied()
            .filter(|&d| is_using(i, d))
            .collect();
        ds.sort();
        for d in ds {
            push(node_var(&arena[d]), d, i, &mut flows);
        }
    }

    // 5. exit node: every def in in(exit) -> exit
    if let Some(e) = exit {
        let mut v: Vec<usize> = exit_in.iter().copied().collect();
        v.sort();
        for d in v {
            push(node_var(&arena[d]), d, e, &mut flows);
        }
    }

    // 6. addEdgesFromLoneIdentifiersToExit: a lone identifier (dropped from gen
    // above) gets a direct edge to the method exit.
    if let Some(e) = exit {
        let mut v = lone_idents.clone();
        v.sort();
        for d in v {
            push(node_var(&arena[d]), d, e, &mut flows);
        }
    }

    flows
}

// DdgGenerator.addEdgesToCapturedIdentifiersAndParameters (the captured part):
// `method._identifierViaContainsOut.flatMap(identifierToFirstUsages)`. For a
// method that holds method-refs (a file `<global>`), each of its own
// identifiers (e.g. the global `g` in `g = 5`) is linked to the FIRST usage of
// the same name in every referenced (captured) method.
fn captured_identifier_flows(dumps: &[(String, String)]) -> Vec<(String, String, String)> {
    let parsed: Vec<(String, Vec<DNode>)> = dumps
        .iter()
        .map(|(k, t)| (k.clone(), parse_dump_block(t)))
        .collect();
    let mut out: Vec<(String, String, String)> = Vec::new();
    for (key, arena) in &parsed {
        if arena.is_empty() || arena[0].label != "METHOD" {
            continue;
        }
        if !key.ends_with(":<global>") {
            continue;
        }
        // own nodes (do not descend into nested METHOD subtrees).
        let mut own: Vec<usize> = Vec::new();
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            own.push(i);
            for &c in &arena[i].children {
                if arena[c].label != "METHOD" {
                    stack.push(c);
                }
            }
        }
        // methods captured via method-refs held directly by this method.
        let refs: Vec<String> = own
            .iter()
            .filter(|&&i| arena[i].label == "METHOD_REF")
            .map(|&i| arena[i].fullcode.clone())
            .collect();
        if refs.is_empty() {
            continue;
        }
        own.sort();
        for &i in &own {
            if arena[i].label != "IDENTIFIER" {
                continue;
            }
            let nm = &arena[i].name;
            for r in &refs {
                if let Some((rkey, rarena)) = parsed.iter().find(|(k, _)| k == r) {
                    // first identifier of the same name (source/AST order).
                    if let Some(j) = (0..rarena.len())
                        .find(|&j| rarena[j].label == "IDENTIFIER" && rarena[j].name == *nm)
                    {
                        out.push((
                            node_var(&arena[i]),
                            format!("{key}#{i}"),
                            format!("{rkey}#{j}"),
                        ));
                    }
                }
            }
        }
    }
    out
}

// ---- UsageAnalyzer.isUsing (v4.0.555) ----
// nodeToString: Identifier/ParamIn/ParamOut -> NAME; Expression -> CODE; else None.
fn rd_node_str(arena: &[DNode], i: usize) -> Option<String> {
    match arena[i].label.as_str() {
        "IDENTIFIER" | "METHOD_PARAMETER_IN" | "METHOD_PARAMETER_OUT" => {
            Some(arena[i].name.clone())
        }
        "METHOD" | "METHOD_RETURN" | "CONTROL_STRUCTURE" | "JUMP_TARGET" => None,
        _ => Some(arena[i].fullcode.clone()),
    }
}
// call.argument with a given ARGUMENT_INDEX (argumentOption(idx)).
fn rd_arg_at(arena: &[DNode], c: usize, idx: i64) -> Option<usize> {
    arena[c]
        .children
        .iter()
        .copied()
        .find(|&k| arena[k].has_arg && arena[k].arg_index == idx)
}
// call.argument headOption (lowest argument index; the base of an access).
fn rd_head_arg(arena: &[DNode], c: usize) -> Option<usize> {
    arena[c]
        .children
        .iter()
        .copied()
        .filter(|&k| arena[k].has_arg)
        .min_by_key(|&k| arena[k].arg_index)
}
// sameVariable(use, inElement): nodeToString(use) == { param.name | indirection
// operand code | call code | identifier name }, depending on inElement type.
fn rd_same_var(arena: &[DNode], use_i: usize, in_i: usize) -> bool {
    let us = rd_node_str(arena, use_i);
    match arena[in_i].label.as_str() {
        "METHOD_PARAMETER_IN" | "IDENTIFIER" => us.as_deref() == Some(arena[in_i].name.as_str()),
        "CALL" => {
            if is_indirection_access(&arena[in_i].name) {
                match rd_arg_at(arena, in_i, 1) {
                    Some(op) => us.as_deref() == Some(arena[op].fullcode.as_str()),
                    None => false,
                }
            } else {
                us.as_deref() == Some(arena[in_i].fullcode.as_str())
            }
        }
        _ => false,
    }
}
// isContainer(use, inElement): inElement is a container access (q.x) and its
// base equals the use (q): nodeToString(use) == nodeToString(base).
fn rd_is_container(arena: &[DNode], use_i: usize, in_i: usize) -> bool {
    if arena[in_i].label == "CALL" && is_container_access(&arena[in_i].name) {
        if let Some(base) = rd_head_arg(arena, in_i) {
            return rd_node_str(arena, use_i) == rd_node_str(arena, base);
        }
    }
    false
}
// isPart(use, inElement): use is a container access (q.x) and its base equals
// inElement (a param or identifier named q).
fn rd_is_part(arena: &[DNode], use_i: usize, in_i: usize) -> bool {
    if arena[use_i].label == "CALL" && is_container_access(&arena[use_i].name) {
        if let Some(base) = rd_head_arg(arena, use_i) {
            let bs = rd_node_str(arena, base);
            return match arena[in_i].label.as_str() {
                "METHOD_PARAMETER_IN" | "IDENTIFIER" => {
                    bs.as_deref() == Some(arena[in_i].name.as_str())
                }
                _ => false,
            };
        }
    }
    false
}
// Access-path operators: expressions whose `toTrackedBaseAndAccessPathSimple`
// yields a named base + access path (so two of them can be aliases).
fn is_access_path_call(name: &str) -> bool {
    is_container_access(name)
        || is_indirection_access(name)
        || matches!(name, "<operator>.pointerShift" | "<operator>.getElementPtr")
}
// isAlias(use, inElement): both are access-path calls with the same tracked
// base and an EXACT-matching access path — i.e. the same access expression
// (`*l` aliases `*l`, `p->y` aliases `p->y`). Approximated by equal node code.
fn rd_is_alias(arena: &[DNode], use_i: usize, in_i: usize) -> bool {
    if arena[use_i].label == "CALL"
        && arena[in_i].label == "CALL"
        && is_access_path_call(&arena[use_i].name)
        && is_access_path_call(&arena[in_i].name)
    {
        return rd_node_str(arena, use_i) == rd_node_str(arena, in_i);
    }
    false
}
fn rd_is_using(arena: &[DNode], use_i: usize, in_i: usize) -> bool {
    rd_same_var(arena, use_i, in_i)
        || rd_is_container(arena, use_i, in_i)
        || rd_is_part(arena, use_i, in_i)
        || rd_is_alias(arena, use_i, in_i)
}

// ---- EdgeValidator.isValidEdge (v4.0.555) ----
fn rd_args(arena: &[DNode], c: usize) -> Vec<usize> {
    let mut v: Vec<usize> = arena[c]
        .children
        .iter()
        .copied()
        .filter(|&k| arena[k].has_arg && arena[k].label != "FIELD_IDENTIFIER")
        .collect();
    v.sort_by_key(|&k| arena[k].arg_index);
    v
}
fn rd_in_call(arena: &[DNode], node: usize) -> Option<usize> {
    arena[node].parent.filter(|&p| arena[p].label == "CALL")
}
fn rd_is_expr(arena: &[DNode], node: usize) -> bool {
    matches!(
        arena[node].label.as_str(),
        "CALL"
            | "IDENTIFIER"
            | "LITERAL"
            | "BLOCK"
            | "FIELD_IDENTIFIER"
            | "METHOD_REF"
            | "TYPE_REF"
    )
}
fn rd_sem(arena: &[DNode], c: usize) -> Option<Vec<(i64, i64)>> {
    if arena[c].label == "CALL" {
        operator_semantics(&arena[c].name)
    } else {
        None
    }
}
fn rd_is_call_retval(arena: &[DNode], node: usize) -> bool {
    if arena[node].label != "CALL" {
        return false;
    }
    match rd_sem(arena, node) {
        Some(m) => !m.iter().any(|&(_, d)| d == -1),
        None => false,
    }
}
fn rd_is_used(arena: &[DNode], e: usize) -> bool {
    match rd_in_call(arena, e).and_then(|c| rd_sem(arena, c)) {
        Some(m) => m.iter().any(|&(s, _)| s == arena[e].arg_index),
        None => true,
    }
}
fn rd_is_defined(arena: &[DNode], e: usize) -> bool {
    match rd_in_call(arena, e).and_then(|c| rd_sem(arena, c)) {
        Some(m) => m.iter().any(|&(_, d)| d == arena[e].arg_index),
        None => true,
    }
}
fn rd_has_flow(arena: &[DNode], parent: usize, child: usize) -> bool {
    match rd_in_call(arena, parent).and_then(|c| rd_sem(arena, c)) {
        Some(m) => m.contains(&(arena[parent].arg_index, arena[child].arg_index)),
        None => true,
    }
}
fn rd_same_call(arena: &[DNode], a: usize, b: usize) -> bool {
    let ca = rd_in_call(arena, a);
    ca.is_some() && ca == rd_in_call(arena, b)
}
fn rd_valid_to_expr(arena: &[DNode], par: usize, cur: usize) -> bool {
    if rd_is_expr(arena, par) {
        let same = rd_same_call(arena, par, cur);
        (same && rd_is_used(arena, par) && rd_is_defined(arena, cur))
            || (!same && rd_is_used(arena, cur))
    } else {
        rd_is_used(arena, cur)
    }
}
fn rd_valid_edge(arena: &[DNode], child: usize, parent: usize) -> bool {
    if rd_is_expr(arena, child)
        && (rd_is_call_retval(arena, parent) || !rd_valid_to_expr(arena, parent, child))
    {
        return false;
    }
    if arena[child].label == "CALL"
        && rd_is_expr(arena, parent)
        && rd_is_call_retval(arena, child)
        && rd_args(arena, child).contains(&parent)
    {
        return false;
    }
    if rd_is_expr(arena, child) {
        if rd_is_expr(arena, parent) {
            if rd_same_call(arena, parent, child)
                && rd_is_defined(arena, child)
                && rd_is_used(arena, parent)
            {
                return rd_has_flow(arena, parent, child);
            }
            return true;
        }
        return rd_is_used(arena, child);
    }
    !rd_is_call_retval(arena, parent)
}

#[cfg(test)]
mod macro_tests {
    use super::*;

    fn function_macro(params: &[&str], body: &str) -> MacroDef {
        MacroDef {
            params: Some(params.iter().map(|value| (*value).to_string()).collect()),
            body: body.to_string(),
            directive: String::new(),
            origin: "test.c".to_string(),
        }
    }

    #[test]
    fn substitutes_stringize_paste_and_variadics() {
        assert_eq!(
            substitute("#value", &["value".into()], &["hello   world".into()]),
            "\"hello world\""
        );
        assert_eq!(
            substitute(
                "left ## right",
                &["left".into(), "right".into()],
                &["token_".into(), "value".into()]
            ),
            "token_value"
        );
        assert_eq!(
            substitute(
                "first",
                &["first".into(), "__VA_ARGS__".into()],
                &["input".into(), "11".into(), "12".into()]
            ),
            "input"
        );
        assert_eq!(
            substitute(
                "log(format, ## __VA_ARGS__)",
                &["format".into(), "__VA_ARGS__".into()],
                &["\"ok\"".into()]
            ),
            "log(\"ok\")"
        );
    }

    #[test]
    fn fully_expands_nested_macros_and_tracks_visible_arguments() {
        let macros = HashMap::from([
            ("INNER".to_string(), function_macro(&["x"], "((x) + 1)")),
            ("OUTER".to_string(), function_macro(&["x"], "INNER(x)")),
        ]);
        let mut expanding = HashSet::from(["OUTER".to_string()]);
        assert_eq!(
            expand_macros_text("INNER(input)", &macros, 0, &mut expanding),
            "((input) + 1)"
        );
        assert_eq!(ordinary_macro_parameters("#x", &["x".into()]), vec![false]);
        assert_eq!(
            ordinary_macro_parameters("left ## right", &["left".into(), "right".into()]),
            vec![false, false]
        );
        assert_eq!(
            ordinary_macro_parameters("first", &["first".into(), "__VA_ARGS__".into()]),
            vec![true, false]
        );
    }
}
