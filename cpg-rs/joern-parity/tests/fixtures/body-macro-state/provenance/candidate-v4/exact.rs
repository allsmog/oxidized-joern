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
use std::sync::Arc;
use tree_sitter::{Node, Parser};

struct SourceUnit {
    file: String,
    src: String,
    tree: tree_sitter::Tree,
}

pub fn canonical_dump_paths(paths: &[String]) -> String {
    canonical_dump_sources(&read_sources_from_paths(paths).expect("read exact C inputs"))
}

/// Retain source names relative to the common input directory. Flattening
/// filenames loses quoted include targets and conflates equal basenames.
pub fn read_sources_from_paths(paths: &[String]) -> std::io::Result<Vec<(String, String)>> {
    let paths: Vec<std::path::PathBuf> = paths
        .iter()
        .map(std::path::absolute)
        .collect::<std::io::Result<_>>()?;
    let Some(first) = paths.first() else {
        return Ok(Vec::new());
    };
    let mut root = first.parent().expect("input file parent").to_path_buf();
    for path in &paths[1..] {
        while !path.starts_with(&root) {
            if !root.pop() {
                break;
            }
        }
    }
    paths
        .iter()
        .map(|path| {
            let source = std::fs::read_to_string(path).map_err(|error| {
                std::io::Error::new(error.kind(), format!("{}: {error}", path.display()))
            })?;
            let file = path
                .strip_prefix(&root)
                .expect("common input directory")
                .to_string_lossy()
                .into_owned();
            Ok((file, source))
        })
        .collect()
}

pub fn canonical_dump_sources(sources: &[(String, String)]) -> String {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .unwrap();

    let units: Vec<SourceUnit> = sources
        .iter()
        .map(|(file, src)| {
            let tree = parser.parse(src, None).unwrap();
            SourceUnit {
                file: file.clone(),
                src: src.clone(),
                tree,
            }
        })
        .collect();

    // Project-wide registries: defined functions (calls to these never become
    // stubs; each also gets a method TYPE_DECL) and struct definitions.
    let mut defined: Vec<String> = Vec::new();
    let mut raw_fn_decls: Vec<(String, String, usize)> = Vec::new(); // (name, file, node id)
    let mut parenthesized_definitions = HashSet::new();
    let mut struct_decls: Vec<(String, String, String)> = Vec::new(); // (tag, code, file)
    let mut type_alias_full_names: HashMap<usize, String> = HashMap::new();
    let mut used_types: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for u in &units {
        let b = u.src.as_bytes();
        for f in translation_unit_items(u.tree.root_node(), u.src.as_bytes()) {
            match f.kind() {
                "function_definition" => {
                    if let Some((name, _, _)) = fn_header(f, b) {
                        if f.child_by_field_name("declarator")
                            .and_then(parenthesized_function_parts)
                            .is_some()
                        {
                            parenthesized_definitions.insert(f.id());
                            defined.push(format!("<unresolvedNamespace>.{name}"));
                        } else {
                            defined.push(name.clone());
                        }
                        raw_fn_decls.push((name, u.file.clone(), f.id()));
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
                    if let Some(aggregate) = typedef_aggregate(f) {
                        let name = aggregate_name(aggregate, b);
                        used_types.insert(name.clone());
                        struct_decls.push((
                            name.clone(),
                            aggregate_code(aggregate, b),
                            u.file.clone(),
                        ));
                        if aggregate.child_by_field_name("name").is_some() {
                            for alias in typedef_declarators(f) {
                                used_types.insert(typedef_alias_name(alias, b));
                                let suffix = decl_suffix(alias, b);
                                let prefix = if array_dimensions(alias).is_empty() {
                                    ""
                                } else {
                                    "typedef"
                                };
                                used_types.insert(format!("{prefix}{name}{suffix}"));
                                struct_decls.push((
                                    aggregate_alias_full_name(aggregate, alias, b),
                                    esc(text(f, b)),
                                    u.file.clone(),
                                ));
                            }
                        }
                        continue;
                    }
                    let aliases = typedef_declarators(f);
                    for alias in &aliases {
                        let tag = type_binding_name(*alias, b);
                        used_types.insert(tag.clone());
                        used_types.insert(typedef_underlying_type(f, *alias, b));
                        type_alias_full_names.insert(alias.id(), tag.clone());
                        struct_decls.push((tag, esc(text(f, b)), u.file.clone()));
                    }
                }
                _ => {}
            }
        }
    }
    let mut definition_files: HashMap<String, HashSet<String>> = HashMap::new();
    for (name, file, id) in &raw_fn_decls {
        if parenthesized_definitions.contains(id) {
            continue;
        }
        definition_files
            .entry(name.clone())
            .or_default()
            .insert(file.clone());
    }
    let ambiguous_functions: HashSet<String> = definition_files
        .iter()
        .filter_map(|(name, files)| (files.len() > 1).then_some(name.clone()))
        .collect();
    let method_full = |name: &str, file: &str| {
        if ambiguous_functions.contains(name) {
            format!("{file}:{name}")
        } else {
            name.to_string()
        }
    };
    let mut definition_full_names = HashMap::new();
    let mut occurrences: HashMap<String, usize> = HashMap::new();
    let fn_decls: Vec<(String, String)> = raw_fn_decls
        .iter()
        .map(|(name, file, id)| {
            let base = if parenthesized_definitions.contains(id) {
                format!("<unresolvedNamespace>.{name}")
            } else {
                method_full(name, file)
            };
            let occurrence = occurrences.entry(base.clone()).or_default();
            let full = if *occurrence == 0 {
                base
            } else {
                format!("{base}<duplicate>{}", *occurrence - 1)
            };
            *occurrence += 1;
            definition_full_names.insert(*id, full.clone());
            (full, file.clone())
        })
        .collect();

    // Function declarations remain external METHODs even when never called.
    // A definition wins over its declarations; repeated prototypes coalesce.
    let mut prototypes = std::collections::BTreeMap::new();
    let mut unknown_declaration_prefixes = HashMap::new();
    for u in &units {
        let context = body_macro_context(u.tree.root_node(), u.src.as_bytes(), &u.file, &units);
        let root = u.tree.root_node();
        let bytes = u.src.as_bytes();
        let active: HashSet<_> = context.items.iter().map(Node::id).collect();
        let declarations = translation_unit_items(root, bytes)
            .into_iter()
            .filter(|node| node.kind() != "function_definition" || active.contains(&node.id()))
            .flat_map(|node| prototype_declarations(node, bytes));
        for declaration in declarations {
            let macros = context
                .macro_states
                .get(&declaration.id())
                .or_else(|| context.body_macro_sites.at(declaration, bytes))
                .cloned()
                .unwrap_or_default();
            let declaration_macros = macros
                .iter()
                .filter(|(_, definition)| definition.params.is_none())
                .map(|(name, definition)| (name.clone(), definition.body.clone()))
                .collect::<HashMap<_, _>>();
            for (declarator, _) in prototype_header_entries(declaration, bytes) {
                let Some(resolved) =
                    resolved_function_header(declaration, declarator, bytes, &macros)
                else {
                    continue;
                };
                let header = resolved.header;
                let parenthesized = parenthesized_function_parts(declarator);
                let full = if parenthesized.is_some() {
                    format!("<unresolvedNamespace>.{}", header.0)
                } else {
                    header.0.clone()
                };
                if !defined.contains(&full) {
                    let mut code = resolved
                        .expanded_code
                        .unwrap_or_else(|| esc(text(declaration, bytes)));
                    if macro_declaration_return(declaration).is_some() {
                        let prefix = declaration
                            .child_by_field_name("type")
                            .map(|node| text(node, bytes))
                            .unwrap_or("");
                        if declaration_macros.contains_key(prefix) {
                            let mut budget = 65_536;
                            let expansion = expand_preproc_objects(
                                prefix,
                                &declaration_macros,
                                &mut HashSet::new(),
                                &mut budget,
                            );
                            let specifier = format!("{} {}", expansion.trim(), header.1)
                                .trim()
                                .to_string();
                            let declarations = prototype_header_entries(declaration, bytes)
                                .into_iter()
                                .map(|(declarator, (_, _, parameters))| {
                                    let parameter_types = parameters
                                        .iter()
                                        .map(|parameter| {
                                            substitute(
                                                &parameter.code,
                                                std::slice::from_ref(&parameter.name),
                                                &[String::new()],
                                            )
                                            .trim()
                                            .to_string()
                                        })
                                        .collect::<Vec<_>>()
                                        .join(", ");
                                    if parenthesized_function_parts(declarator).is_some() {
                                        format!("{specifier}  ()({parameter_types})")
                                    } else {
                                        format!("{specifier} ({parameter_types})")
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join(" ");
                            code = esc(&format!("{specifier} {declarations};"));
                        } else {
                            unknown_declaration_prefixes
                                .insert(declaration.id(), prefix.to_string());
                            code = esc(text(declaration, bytes)
                                .strip_prefix(prefix)
                                .unwrap_or(text(declaration, bytes))
                                .trim());
                        }
                    }
                    prototypes
                        .entry(full)
                        .or_insert_with(|| (u.file.clone(), code, header));
                }
            }
        }
    }

    // Each dump is one method subtree keyed by FULL_NAME; Joern's oracle sorts
    // all methods (user, <global> wrappers, <operator> stubs) by fullName.
    let mut dumps: Vec<(String, String)> = Vec::new();
    let mut stub_uses: HashMap<String, usize> = HashMap::new();
    let mut edges: Vec<(String, String, String)> = Vec::new();
    let mut placements: HashMap<String, Vec<(String, usize)>> = HashMap::new();
    let mut macro_method_files = HashMap::new();
    let mut expansion_control_kinds = HashMap::new();
    let mut used_macros: std::collections::BTreeMap<String, (String, String, usize, String)> =
        std::collections::BTreeMap::new();
    // #include directives become IMPORT nodes that consume earlier sibling
    // slots: the file-global TYPE_DECL's ORDER is 1 + #includes.
    let include_counts: HashMap<String, usize> = units
        .iter()
        .map(|u| {
            let n = translation_unit_items(u.tree.root_node(), u.src.as_bytes())
                .iter()
                .filter(|f| f.kind() == "preproc_include")
                .count();
            (u.file.clone(), n)
        })
        .collect();

    for u in &units {
        let context = body_macro_context(u.tree.root_node(), u.src.as_bytes(), &u.file, &units);
        let b = u.src.as_bytes();
        let root = u.tree.root_node();
        let BodyMacroContext {
            items: active_items,
            macros,
            macro_states,
            body_macro_sites,
            header_declarations,
            typedef_states,
        } = &context;
        let mut type_sites = HashMap::new();
        for item in translation_unit_items(root, b) {
            type_sites.extend(collect_type_sites(
                item,
                b,
                typedef_states.get(&item.id()).cloned().unwrap_or_default(),
                macro_states.get(&item.id()).unwrap_or(macros),
                Some(body_macro_sites),
            ));
        }
        let active_methods: HashSet<usize> = active_items
            .iter()
            .filter(|node| node.kind() == "function_definition")
            .map(Node::id)
            .collect();

        let active_declarations: HashSet<usize> = active_items
            .iter()
            .filter(|node| node.kind() == "declaration")
            .map(Node::id)
            .collect();

        // Per-file tables (c2cpg resolves within the translation unit).
        let mut functions: HashMap<String, String> = HashMap::new();
        let mut function_call_types: HashMap<String, String> = HashMap::new();
        let mut function_full_names: HashMap<String, String> = HashMap::new();
        let mut globals: HashMap<String, String> = HashMap::new();
        let mut enumerators: Vec<String> = Vec::new();
        for f in translation_unit_items(root, b) {
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
        for f in translation_unit_items(root, b) {
            match f.kind() {
                "function_definition" => {
                    let macros = macro_states.get(&f.id()).cloned().unwrap_or_default();
                    if let Some(resolved) = f
                        .child_by_field_name("declarator")
                        .and_then(|decl| resolved_function_header(f, decl, b, &macros))
                    {
                        let (name, ret, _) = resolved.header;
                        function_full_names.insert(name.clone(), method_full(&name, &u.file));
                        function_call_types.insert(name.clone(), resolved.call_type);
                        functions.insert(name, ret);
                    }
                }
                "declaration" => {
                    let macros = macro_states.get(&f.id()).cloned().unwrap_or_default();
                    for (declarator, _) in prototype_header_entries(f, b) {
                        if !active_declarations.contains(&f.id()) {
                            continue;
                        }
                        let Some(resolved) = resolved_function_header(f, declarator, b, &macros)
                        else {
                            continue;
                        };
                        let (name, ret, _) = resolved.header;
                        function_full_names
                            .entry(name.clone())
                            .or_insert_with(|| name.clone());
                        function_call_types
                            .entry(name.clone())
                            .or_insert(resolved.call_type);
                        functions.entry(name).or_insert(ret);
                    }

                    for d in named_children(f) {
                        let decl = if d.kind() == "init_declarator" {
                            d.child_by_field_name("declarator")
                        } else if matches!(
                            d.kind(),
                            "identifier"
                                | "pointer_declarator"
                                | "array_declarator"
                                | "function_declarator"
                        ) {
                            Some(d)
                        } else {
                            None
                        };
                        if let Some(decl) = decl {
                            if !is_function_declaration(decl) {
                                let name = innermost_id(decl, b);
                                if !name.is_empty() {
                                    globals.insert(name, declared_object_type(f, decl, b));
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        for declaration in header_declarations {
            let (name, ret, _) = &declaration.header;
            function_full_names
                .entry(name.clone())
                .or_insert_with(|| name.clone());
            functions.entry(name.clone()).or_insert_with(|| ret.clone());
            function_call_types
                .entry(name.clone())
                .or_insert_with(|| declaration.call_type.clone());
        }

        let mut ctx = Ctx {
            functions: &functions,
            function_call_types: &function_call_types,
            function_full_names: &function_full_names,
            definition_full_names: &definition_full_names,
            ambiguous_functions: &ambiguous_functions,
            globals: &globals,
            enumerators: &enumerators,
            macros: macros.clone(),
            macro_states,
            body_macro_sites,
            macro_uses: Vec::new(),
            macro_use_ids: HashMap::new(),
            last_mfn_span: None,
            type_sites,
            default_typedefs: TypeNameState::default(),
            unknown_declaration_prefixes: &unknown_declaration_prefixes,
            file: u.file.clone(),
            copying_macro_argument: false,
            macro_expansion_code: None,
            macro_expansion_root: None,
            recovering_expression: false,
            recovered_bindings: HashSet::new(),
            recovery_candidates: HashMap::new(),
            field_macro_codes: HashMap::new(),
            field_macro_piece: None,
            expansion_control_kinds: &mut expansion_control_kinds,
            macro_method_files: &mut macro_method_files,
            used_macros: &mut used_macros,
            symbols: HashMap::new(),
            symbol_call_types: HashMap::new(),
            method_functions: HashMap::new(),
            method_call_types: HashMap::new(),
            phantoms: Vec::new(),
            stubs: &mut stub_uses,
            types: &mut used_types,
            type_declarations: &mut struct_decls,
            type_alias_full_names: &mut type_alias_full_names,
            out: String::new(),
            block: String::new(),
            line_no: 0,
            argument_count: 0,
            suppress_below: None,
            ctx_stack: Vec::new(),
            parent_stack: Vec::new(),
            sym_line: HashMap::new(),
            param_in_line: HashMap::new(),
            edges: &mut edges,
            placements: &mut placements,
        };

        // Standalone dump per user method, plus one per struct <clinit>.
        for f in translation_unit_items(root, b) {
            ctx.macros = macro_states.get(&f.id()).cloned().unwrap_or_default();
            let aggregate = typedef_aggregate(f).unwrap_or(f);
            match f.kind() {
                "function_definition" => {
                    if let Some((name, _, _)) = fn_header(f, b) {
                        let full = ctx
                            .definition_full_names
                            .get(&f.id())
                            .cloned()
                            .unwrap_or_else(|| name.clone());
                        ctx.begin_block(&full);
                        ctx.emit_method(f, b, 0, active_methods.contains(&f.id()));
                        ctx.edge("SOURCE_FILE", format!("M:{full}"), format!("F:{}", u.file));
                        dumps.push((full, std::mem::take(&mut ctx.out)));
                    }
                }
                "struct_specifier" | "union_specifier" | "enum_specifier" | "type_definition"
                    if needs_clinit(aggregate, b) =>
                {
                    let tag = aggregate_name(aggregate, b);
                    let members = count_members(aggregate);
                    let key = format!("{tag}.<clinit>:{tag}()");
                    ctx.begin_block(&key);
                    ctx.emit_clinit(aggregate, b, 0, members + 1);
                    ctx.edge("SOURCE_FILE", format!("M:{key}"), format!("F:{}", u.file));
                    dumps.push((key, std::mem::take(&mut ctx.out)));
                }
                _ => {}
            }
        }

        // The per-file `<global>` wrapper method.
        let gkey = format!("{}:<global>", u.file);
        ctx.begin_block(&gkey);
        ctx.emit_file_global(root, b, &u.file, &active_methods);
        ctx.edge("SOURCE_FILE", format!("M:{gkey}"), format!("F:{}", u.file));
        dumps.push((gkey, std::mem::take(&mut ctx.out)));
        ctx.resolve_macro_metadata(&mut dumps);
    }

    // Operator stubs and the synthetic <includes>:<global>, emitted through
    // an instrumented Ctx so they too produce addresses and edges.
    let empty_fns: HashMap<String, String> = HashMap::new();
    let empty_full_names: HashMap<String, String> = HashMap::new();
    let empty_ambiguous: HashSet<String> = HashSet::new();
    let empty_globals: HashMap<String, String> = HashMap::new();
    let mut stub_uses2: HashMap<String, usize> = HashMap::new();
    let empty_enums: Vec<String> = Vec::new();
    let empty_macro_states = HashMap::new();
    let empty_body_macro_sites = BodyMacroSites::default();
    let mut sctx = Ctx {
        functions: &empty_fns,
        function_call_types: &empty_fns,
        function_full_names: &empty_full_names,
        definition_full_names: &definition_full_names,
        ambiguous_functions: &empty_ambiguous,
        globals: &empty_globals,
        enumerators: &empty_enums,
        macros: Arc::new(HashMap::new()),
        macro_states: &empty_macro_states,
        body_macro_sites: &empty_body_macro_sites,
        macro_uses: Vec::new(),
        macro_use_ids: HashMap::new(),
        last_mfn_span: None,
        type_sites: HashMap::new(),
        default_typedefs: TypeNameState::default(),
        unknown_declaration_prefixes: &unknown_declaration_prefixes,
        file: String::new(),
        copying_macro_argument: false,
        macro_expansion_code: None,
        macro_expansion_root: None,
        recovering_expression: false,
        recovered_bindings: HashSet::new(),
        recovery_candidates: HashMap::new(),
        field_macro_codes: HashMap::new(),
        field_macro_piece: None,
        expansion_control_kinds: &mut expansion_control_kinds,
        macro_method_files: &mut macro_method_files,
        used_macros: &mut used_macros,
        symbols: HashMap::new(),
        symbol_call_types: HashMap::new(),
        method_functions: HashMap::new(),
        method_call_types: HashMap::new(),
        phantoms: Vec::new(),
        stubs: &mut stub_uses2,
        types: &mut used_types,
        type_declarations: &mut struct_decls,
        type_alias_full_names: &mut type_alias_full_names,
        out: String::new(),
        block: String::new(),
        line_no: 0,
        argument_count: 0,
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
        .filter(|(n, _)| !defined.contains(n) && !prototypes.contains_key(n))
        .collect();
    stub_list.sort();
    for (name, arity) in stub_list {
        sctx.begin_block(&name);
        sctx.emit_stub(&name, arity);
        dumps.push((name, std::mem::take(&mut sctx.out)));
    }
    for (name, (file, code, (method_name, ret, params))) in &prototypes {
        sctx.begin_block(name);
        sctx.emit_prototype(method_name, name, code, ret, params);
        sctx.edge("SOURCE_FILE", format!("M:{name}"), format!("F:{file}"));
        sctx.edge(
            "CONTAINS",
            format!("D:{file}:<global>"),
            format!("M:{name}"),
        );
        dumps.push((name.clone(), std::mem::take(&mut sctx.out)));
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
        let order = placements
            .get(&format!("TD:{tag}"))
            .and_then(|positions| positions.iter().min())
            .and_then(|(block, index)| {
                dumps
                    .iter()
                    .find(|(name, _)| name == block)
                    .and_then(|(_, dump)| dump.lines().nth(*index))
            })
            .and_then(|line| line.rsplit_once(" ORDER="))
            .and_then(|(_, order)| order.split_whitespace().next())
            .unwrap_or("1");
        let name = tag.split("<duplicate>").next().unwrap_or(tag);
        tds.push((tag.clone(), format!(
            "NODES|TYPE_DECL NAME={name} FULL_NAME={tag} CODE={code} AST_PARENT_TYPE= AST_PARENT_FULL_NAME= FILENAME={file} ORDER={order}\n"
        )));
    }
    for (name, file) in &fn_decls {
        let declaration_code = name.split("<duplicate>").next().unwrap_or(name);
        let display_name = declaration_code
            .strip_prefix("<unresolvedNamespace>.")
            .unwrap_or(declaration_code);
        tds.push((name.clone(), format!(
            "NODES|TYPE_DECL NAME={display_name} FULL_NAME={name} CODE={declaration_code} AST_PARENT_TYPE=TYPE_DECL AST_PARENT_FULL_NAME={file}:<global> FILENAME={file} ORDER=1\n"
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
        let display_name = t.strip_prefix("<unresolvedNamespace>.").unwrap_or(t);
        out.push_str(&format!(
            "NODES|TYPE NAME={display_name} FULL_NAME={t} TYPE_DECL_FULL_NAME={t}\n"
        ));
    }

    // ---- EDGES section ----
    // CFG: built from the dump blocks themselves (nested METHOD/TYPE_DECL
    // subtrees are transparent there, so each CFG is generated exactly once,
    // from its home block).
    for (key, text) in &dumps {
        for (s, d) in cfg_edges_for_block(key, text, &expansion_control_kinds) {
            edges.push(("CFG".into(), s, d));
        }
    }

    // REACHING_DEF flows (the FLOWS| section).
    let mut flows: Vec<(String, String, String)> = Vec::new();
    for (key, text) in &dumps {
        for (var, s, d) in reaching_def_flows(key, text, &expansion_control_kinds) {
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
    // Macro identity uses the defining file; Joern registers the stub beneath
    // the first calling translation unit and uses that unit for SOURCE_FILE.
    for full in used_macros.keys() {
        if let Some(file) = macro_method_files.get(full) {
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
    for (k, src, dst) in edges {
        if let (Some(s), Some(d)) = (resolve(&src), resolve(&dst)) {
            edge_lines.insert(format!("{k} {s} -> {d}"));
        }
    }
    for l in edge_lines {
        out.push_str(&format!("EDGES|{l}\n"));
    }
    let mut flow_lines: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (var, src, dst) in flows {
        if let (Some(s), Some(d)) = (resolve(&src), resolve(&dst)) {
            flow_lines.insert(format!("REACHING_DEF[{var}] {s} -> {d}"));
        }
    }
    for l in flow_lines {
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

/// A source #define, including supplied headers. Expanded invocations become INLINED calls
/// and each *used* macro also gets a METHOD whose CODE is the directive.
#[derive(Clone)]
struct MacroDef {
    params: Option<Vec<String>>, // None = object-like
    body: String,
    directive: String,
    file: String,
}

type MacroState = Arc<HashMap<String, MacroDef>>;
type TypeNameState = Arc<HashSet<String>>;
type TypeSites = HashMap<usize, TypeNameState>;

#[derive(Clone, PartialEq, Eq)]
struct MacroMetadata {
    name: String,
    directive: String,
    file: String,
}

struct MacroUse {
    offset: usize,
    metadata: MacroMetadata,
    arity: usize,
    ret: String,
    placements: Vec<(String, std::ops::Range<usize>, String)>,
}

/// Immutable preprocessing environments at directive boundaries in one original
/// active function body in one source buffer. File-scope declarations retain
/// their separate header snapshots. Temporary parse trees have their own bytes and never use these
/// offsets. Unchanged stretches share one Arc rather than a map per AST node.
#[derive(Default)]
struct BodyMacroSites {
    source: usize,
    tree: usize,
    len: usize,
    changes: Vec<(usize, MacroState)>,
    bodies: Vec<(usize, usize)>,
    definitions: Vec<MacroMetadata>,
    expansions: Vec<(usize, usize)>,
}

fn collect_macro_expansions(
    node: Node,
    bytes: &[u8],
    macros: &MacroState,
    sites: &mut BodyMacroSites,
) {
    if node.kind() == "preproc_defined" || node.kind() == "comment" {
        return;
    }
    if let Some(function) = node.child_by_field_name("function") {
        let name = text(function, bytes);
        if let Some(definition) = macros.get(name) {
            sites.expansion(function.start_byte(), name, definition);
            return;
        }
    }
    if node.named_child_count() == 0 {
        let name = text(node, bytes);
        if let Some(definition) = macros
            .get(name)
            .filter(|definition| definition.params.is_none())
        {
            sites.expansion(node.start_byte(), name, definition);
        }
    } else {
        for child in named_children(node) {
            collect_macro_expansions(child, bytes, macros, sites);
        }
    }
}

impl BodyMacroSites {
    fn new(root: Node, bytes: &[u8]) -> Self {
        Self {
            source: bytes.as_ptr() as usize,
            tree: root.id(),
            len: bytes.len(),
            changes: Vec::new(),
            bodies: Vec::new(),
            definitions: Vec::new(),
            expansions: Vec::new(),
        }
    }

    fn expansion(&mut self, offset: usize, name: &str, definition: &MacroDef) {
        let metadata = MacroMetadata {
            name: name.to_string(),
            directive: definition.directive.clone(),
            file: definition.file.clone(),
        };
        let index = self
            .definitions
            .iter()
            .position(|prior| prior == &metadata)
            .unwrap_or_else(|| {
                self.definitions.push(metadata);
                self.definitions.len() - 1
            });
        self.expansions.push((offset, index));
    }

    fn owns_node(&self, mut node: Node) -> bool {
        while let Some(parent) = node.parent() {
            node = parent;
        }
        node.id() == self.tree
    }

    fn owns(&self, bytes: &[u8]) -> bool {
        self.source == bytes.as_ptr() as usize && self.len == bytes.len()
    }

    fn record(&mut self, bytes: &[u8], offset: usize, macros: &MacroState) {
        if self.owns(bytes)
            && self
                .changes
                .last()
                .is_none_or(|(_, prior)| !Arc::ptr_eq(prior, macros))
        {
            self.changes.push((offset, macros.clone()));
        }
    }

    fn at(&self, node: Node, bytes: &[u8]) -> Option<&MacroState> {
        if !self.owns(bytes) {
            return None;
        }
        let body = self
            .bodies
            .partition_point(|(start, _)| *start <= node.start_byte());
        if body == 0 || node.end_byte() > self.bodies[body - 1].1 {
            return None;
        }
        let index = self
            .changes
            .partition_point(|(offset, _)| *offset <= node.start_byte());
        index.checked_sub(1).map(|index| &self.changes[index].1)
    }
}

// The pinned CDT C scanner supplies these even without compiler definitions.
// Source #undef and #define directives can still remove or replace them.
const PREDEFINED_C_MACROS: [(&str, &str); 3] = [
    ("__STDC__", "1"),
    ("__STDC_VERSION__", "199901L"),
    ("__STDC_HOSTED__", "1"),
];

fn predefined_c_macros(file: &str) -> MacroState {
    Arc::new(
        PREDEFINED_C_MACROS
            .into_iter()
            .map(|(name, value)| {
                (
                    name.to_string(),
                    MacroDef {
                        params: None,
                        body: value.to_string(),
                        // Builtin expansion methods have an explicitly empty CODE.
                        directive: String::new(),
                        file: file.to_string(),
                    },
                )
            })
            .collect(),
    )
}

/// Per-function emission context.
struct Ctx<'a> {
    functions: &'a HashMap<String, String>,
    function_call_types: &'a HashMap<String, String>,
    /// Translation-unit-local method identities. Duplicate C names are valid
    /// for `static` helpers across files; qualify those identities by file so
    /// they remain distinct and local calls resolve deterministically.
    function_full_names: &'a HashMap<String, String>,
    /// Distinct declaration identities for same-name preprocessor alternatives.
    /// Calls and METHOD_REFs continue to use the first declaration's identity.
    definition_full_names: &'a HashMap<usize, String>,
    /// Project-wide duplicate definition names. A call without a local target
    /// stays unresolved instead of acquiring an arbitrary first candidate.
    ambiguous_functions: &'a HashSet<String>,
    globals: &'a HashMap<String, String>,
    enumerators: &'a Vec<String>,
    macros: MacroState,
    macro_states: &'a HashMap<usize, MacroState>,
    body_macro_sites: &'a BodyMacroSites,
    macro_uses: Vec<MacroUse>,
    macro_use_ids: HashMap<(usize, String, usize, String), usize>,
    last_mfn_span: Option<std::ops::Range<usize>>,
    type_sites: TypeSites,
    default_typedefs: TypeNameState,
    unknown_declaration_prefixes: &'a HashMap<usize, String>,
    file: String,
    copying_macro_argument: bool,
    macro_expansion_code: Option<String>,
    macro_expansion_root: Option<usize>,
    recovering_expression: bool,
    recovered_bindings: HashSet<String>,
    recovery_candidates: HashMap<usize, bool>,
    /// Original source spelling for expressions introduced by a field-token macro.
    field_macro_codes: HashMap<usize, String>,
    /// First pure replacement expression can own the macro wrapper in CDT.
    field_macro_piece: Option<(usize, String)>,
    expansion_control_kinds: &'a mut HashMap<String, String>,
    macro_method_files: &'a mut HashMap<String, String>,
    // used macros: full_name -> (name, directive, nparams, ret type)
    used_macros: &'a mut std::collections::BTreeMap<String, (String, String, usize, String)>,
    symbols: HashMap<String, String>, // local/param name -> declaration type
    symbol_call_types: HashMap<String, String>, // callable parameter -> result type
    method_functions: HashMap<String, String>, // block-scoped prototype reference types
    method_call_types: HashMap<String, String>, // independently rendered call result types
    // Joern's local-creation pass materialises a LOCAL at ORDER=0 atop the
    // method body BLOCK for each referenced global (CODE `<global> name`)
    // and each type name used as a sizeof(T) argument.
    phantoms: Vec<Phantom>,
    stubs: &'a mut HashMap<String, usize>,
    types: &'a mut std::collections::BTreeSet<String>,
    type_declarations: &'a mut Vec<(String, String, String)>,
    type_alias_full_names: &'a mut HashMap<usize, String>,
    out: String,
    // --- edge layer (M4) ---
    // Current dump block name and 0-based line index within it; every node's
    // address is "<block>#<idx>". METHOD/TYPE_DECL nodes are addressed
    // symbolically ("M:<full>"/"TD:<full>") and resolved at the end to the
    // first placement in block-name sort order, replicating the oracle's
    // first-wins assignment across sorted method walks.
    block: String,
    line_no: usize,
    argument_count: usize,
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

/// A typedef name exists only when its declaration remains valid after the
/// macros visible at that definition have expanded. An unresolved plain type
/// name is accepted by CDT, but a primitive modifier followed by a type name
/// (`unsigned UNKNOWN`) is not. Tree-sitter accepts that latter shape without
/// an ERROR node, so it needs a separate check before populating cast context.
fn valid_type_definition_names(
    node: Node,
    bytes: &[u8],
    macros: &HashMap<String, MacroDef>,
) -> Vec<String> {
    fn names(node: Node, bytes: &[u8]) -> Vec<String> {
        if node.kind() != "type_definition" || node.has_error() {
            return Vec::new();
        }
        let Some(specifier) = node.child_by_field_name("type") else {
            return Vec::new();
        };
        if specifier.kind() == "sized_type_specifier"
            && named_children(specifier)
                .iter()
                .any(|child| child.kind() == "type_identifier")
        {
            return Vec::new();
        }
        let mut cursor = node.walk();
        let bindings: Vec<_> = node
            .children_by_field_name("declarator", &mut cursor)
            .map(|decl| type_binding_name(decl, bytes))
            .collect();
        if bindings.iter().any(String::is_empty) {
            Vec::new()
        } else {
            bindings
        }
    }

    let raw = text(node, bytes);
    let Some(expanded) = expand_declaration_tokens(raw, macros, &mut HashSet::new(), &mut 65_536)
    else {
        return Vec::new();
    };
    if expanded == raw {
        return names(node, bytes);
    }
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .unwrap();
    let Some(tree) = parser.parse(&expanded, None) else {
        return Vec::new();
    };
    let root = tree.root_node();
    if root.has_error() {
        return Vec::new();
    }
    let mut cursor = root.walk();
    let mut declarations = root
        .named_children(&mut cursor)
        .filter(|child| child.kind() != "comment");
    let Some(declaration) = declarations.next() else {
        return Vec::new();
    };
    if declarations.next().is_some() {
        return Vec::new();
    }
    names(declaration, expanded.as_bytes())
}

fn type_binding_name(node: Node, bytes: &[u8]) -> String {
    if matches!(node.kind(), "identifier" | "type_identifier") {
        return text(node, bytes).to_string();
    }
    node.child_by_field_name("declarator")
        .or_else(|| {
            (node.kind() == "parenthesized_declarator")
                .then(|| node.named_child(0))
                .flatten()
        })
        .map(|child| type_binding_name(child, bytes))
        .unwrap_or_default()
}

/// A recovered parenthesized expression may contain ERROR siblings. Only a
/// single complete non-comment token can be reinterpreted as a known type.
fn sole_parenthesized_child(node: Node) -> Option<Node> {
    if node.kind() != "parenthesized_expression" || node.has_error() {
        return None;
    }
    let mut cursor = node.walk();
    let mut children = node
        .named_children(&mut cursor)
        .filter(|child| child.kind() != "comment");
    let child = children.next()?;
    children.next().is_none().then_some(child)
}

fn collect_type_sites(
    root: Node,
    bytes: &[u8],
    initial: TypeNameState,
    macros: &MacroState,
    macro_sites: Option<&BodyMacroSites>,
) -> TypeSites {
    fn shadow(types: &mut TypeNameState, name: &str) {
        if types.contains(name) {
            Arc::make_mut(types).remove(name);
        }
    }
    fn visit(
        node: Node,
        bytes: &[u8],
        types: &mut TypeNameState,
        sites: &mut TypeSites,
        macros: &MacroState,
        macro_sites: Option<&BodyMacroSites>,
    ) {
        let macros = macro_sites
            .and_then(|sites| sites.at(node, bytes))
            .unwrap_or(macros);
        if matches!(
            node.kind(),
            "call_expression" | "sizeof_expression" | "identifier" | "field_expression"
        ) {
            sites.insert(node.id(), types.clone());
        }
        match node.kind() {
            "function_definition" => {
                let mut inner = types.clone();
                // Parameter macros must shadow typedefs with the same resolved
                // names that method emission uses for this declaration.
                let header = node
                    .child_by_field_name("declarator")
                    .and_then(|decl| resolved_function_header(node, decl, bytes, macros))
                    .map(|resolved| resolved.header)
                    .or_else(|| fn_header(node, bytes));
                if let Some((_, _, params)) = header {
                    for param in params {
                        shadow(&mut inner, &param.name);
                    }
                }
                if let Some(body) = node.child_by_field_name("body") {
                    visit(body, bytes, &mut inner, sites, macros, macro_sites);
                }
            }
            "compound_statement" | "for_statement" => {
                let mut inner = types.clone();
                for child in named_children(node) {
                    visit(child, bytes, &mut inner, sites, macros, macro_sites);
                }
            }
            "preproc_if" | "preproc_ifdef" | "preproc_elif" | "preproc_elifdef"
            | "preproc_else" => {
                for child in kept_preproc_children(node, bytes, macros) {
                    visit(child, bytes, types, sites, macros, macro_sites);
                }
            }
            "enumerator" => {
                for child in named_children(node) {
                    visit(child, bytes, types, sites, macros, macro_sites);
                }
                if let Some(name) = node.child_by_field_name("name") {
                    shadow(types, text(name, bytes));
                }
            }
            "type_definition" => {
                for child in named_children(node) {
                    visit(child, bytes, types, sites, macros, macro_sites);
                }
                for name in valid_type_definition_names(node, bytes, macros) {
                    Arc::make_mut(types).insert(name);
                }
            }
            "declaration" => {
                let mut cursor = node.walk();
                let declarators: HashSet<_> = node
                    .children_by_field_name("declarator", &mut cursor)
                    .map(|node| node.id())
                    .collect();
                for child in named_children(node) {
                    if declarators.contains(&child.id()) {
                        let decl = child
                            .child_by_field_name("declarator")
                            .filter(|_| child.kind() == "init_declarator")
                            .unwrap_or(child);
                        visit(decl, bytes, types, sites, macros, macro_sites);
                        shadow(types, &type_binding_name(decl, bytes));
                        if child.kind() == "init_declarator" {
                            if let Some(value) = child.child_by_field_name("value") {
                                visit(value, bytes, types, sites, macros, macro_sites);
                            }
                        }
                    } else {
                        visit(child, bytes, types, sites, macros, macro_sites);
                    }
                }
            }
            _ => {
                for child in named_children(node) {
                    visit(child, bytes, types, sites, macros, macro_sites);
                }
            }
        }
    }
    let mut sites = HashMap::new();
    visit(
        root,
        bytes,
        &mut initial.clone(),
        &mut sites,
        macros,
        macro_sites,
    );
    sites
}

impl Ctx<'_> {
    fn resolve_macro_metadata(&mut self, dumps: &mut [(String, String)]) {
        let mut uses = std::mem::take(&mut self.macro_uses);
        uses.sort_by_key(|use_| use_.offset);
        let mut cursor = 0;
        let mut replacements: HashMap<String, Vec<(std::ops::Range<usize>, String)>> =
            HashMap::new();
        let mut call_targets = HashMap::new();
        for use_ in uses {
            let mut metadata = &use_.metadata;
            while let Some(&(offset, definition)) = self.body_macro_sites.expansions.get(cursor) {
                if offset > use_.offset {
                    break;
                }
                cursor += 1;
                let candidate = &self.body_macro_sites.definitions[definition];
                if candidate.name == use_.metadata.name {
                    metadata = candidate;
                    break;
                }
            }
            let full = format!(
                "{}:{}:{}({})",
                metadata.file, metadata.name, use_.ret, use_.arity
            );
            self.macro_method_files
                .entry(full.clone())
                .or_insert_with(|| self.file.clone());
            self.used_macros.entry(full.clone()).or_insert_with(|| {
                (
                    metadata.name.clone(),
                    metadata.directive.clone(),
                    use_.arity,
                    use_.ret.clone(),
                )
            });
            for (block, span, address) in use_.placements {
                replacements
                    .entry(block)
                    .or_default()
                    .push((span, full.clone()));
                call_targets.insert(address, format!("M:{full}"));
            }
        }
        for (block, dump) in dumps {
            if let Some(mut edits) = replacements.remove(block) {
                edits.sort_by_key(|(span, _)| std::cmp::Reverse(span.start));
                for (span, full) in edits {
                    if dump[span.clone()] != full {
                        dump.replace_range(span, &full);
                    }
                }
            }
        }
        for (kind, source, target) in &mut *self.edges {
            if kind == "CALL" {
                if let Some(resolved) = call_targets.get(source) {
                    *target = resolved.clone();
                }
            }
        }
    }

    fn source_macros_at(&self, node: Node, bytes: &[u8]) -> Option<&MacroState> {
        if self.recovering_expression
            || self.copying_macro_argument
            || self.macro_expansion_code.is_some()
        {
            return None;
        }
        self.body_macro_sites.at(node, bytes)
    }

    fn typedefs_at(&self, node: Node) -> TypeNameState {
        self.type_sites
            .get(&node.id())
            .cloned()
            .unwrap_or_else(|| self.default_typedefs.clone())
    }

    fn enter_type_tree(
        &mut self,
        root: Node,
        bytes: &[u8],
        types: TypeNameState,
    ) -> (TypeSites, TypeNameState) {
        let sites = collect_type_sites(root, bytes, types.clone(), &self.macros, None);
        (
            std::mem::replace(&mut self.type_sites, sites),
            std::mem::replace(&mut self.default_typedefs, types),
        )
    }

    fn leave_type_tree(&mut self, previous: (TypeSites, TypeNameState)) {
        (self.type_sites, self.default_typedefs) = previous;
    }

    fn typedef_cast(&self, node: Node, bytes: &[u8]) -> Option<String> {
        if node.kind() != "call_expression" || self.recovering_expression || node.has_error() {
            return None;
        }
        let function = node.child_by_field_name("function")?;
        let name = sole_parenthesized_child(function)?;
        let arguments = node.child_by_field_name("arguments")?;
        if !named_children(arguments)
            .iter()
            .any(|child| child.kind() != "comment")
        {
            return None;
        }
        if name.kind() != "identifier" {
            return None;
        }
        let spelling = text(name, bytes);
        self.typedefs_at(node)
            .contains(spelling)
            .then(|| spelling.to_string())
    }

    fn typedef_cast_identifier(&self, node: Node, bytes: &[u8]) -> Option<String> {
        let parent = node.parent()?;
        if sole_parenthesized_child(parent) != Some(node) {
            return None;
        }
        let call = parent.parent()?;
        if call.child_by_field_name("function") != Some(parent) {
            return None;
        }
        self.typedef_cast(call, bytes)
    }

    fn emit_local_typedef(&mut self, node: Node, bytes: &[u8], depth: usize, order: &mut i64) {
        for alias in typedef_declarators(node) {
            let name = type_binding_name(alias, bytes);
            let underlying = resolved_typedef_underlying_type(node, alias, bytes, &self.macros);
            self.types.insert(underlying.clone());
            if let Some(code) = &self.macro_expansion_code {
                // CDT expands a declaration macro's typedef as a LOCAL, even
                // though it remains a type binding within this expansion tree.
                let code = format!("{code} {code}");
                self.line(
                    depth,
                    "LOCAL",
                    P {
                        name: Some(name),
                        code: Some(esc(&code)),
                        tfn: Some(underlying),
                        order: Some(*order),
                        ..Default::default()
                    },
                );
            } else {
                let full = if let Some(full) = self.type_alias_full_names.get(&alias.id()) {
                    full.clone()
                } else {
                    let count = self
                        .type_declarations
                        .iter()
                        .filter(|(full, _, _)| {
                            full.split("<duplicate>").next() == Some(name.as_str())
                        })
                        .count();
                    let full = if count == 0 {
                        name.clone()
                    } else {
                        format!("{name}<duplicate>{}", count - 1)
                    };
                    self.type_alias_full_names.insert(alias.id(), full.clone());
                    self.type_declarations.push((
                        full.clone(),
                        esc(text(node, bytes)),
                        self.file.clone(),
                    ));
                    full
                };
                self.line(
                    depth,
                    "TYPE_DECL",
                    P {
                        name: Some(name),
                        code: Some(esc(text(node, bytes))),
                        full: Some(full),
                        order: Some(*order),
                        ..Default::default()
                    },
                );
            }
            *order += 1;
        }
    }

    fn sizeof_identifier(&self, node: Node, bytes: &[u8]) -> Option<Phantom> {
        if let Some(desc) = node.child_by_field_name("type") {
            return Some(sizeof_type_identifier(desc, bytes));
        }
        let value = node.child_by_field_name("value")?;
        let identifier = sole_parenthesized_child(value)?;
        let name = text(identifier, bytes);
        (identifier.kind() == "identifier" && self.typedefs_at(node).contains(name)).then(|| {
            Phantom {
                name: name.into(),
                code: name.into(),
                ty: name.into(),
            }
        })
    }

    fn begin_block(&mut self, name: &str) {
        self.block = name.to_string();
        self.line_no = 0;
        self.suppress_below = None;
        self.ctx_stack.clear();
        self.parent_stack.clear();
        self.sym_line.clear();
        self.param_in_line.clear();
        self.recovering_expression = false;
        self.recovered_bindings.clear();
        self.recovery_candidates.clear();
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
        if let Some((_, parent_label, _, parent_inlined)) = self.parent_stack.last() {
            if (parent_label == "CALL" && p.arg.is_some() && !(*parent_inlined && label == "BLOCK"))
                || parent_label == "RETURN"
            {
                self.argument_count += 1;
            }
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
            if label == "IDENTIFIER" && !under_inlined_call && !self.copying_macro_argument {
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
        let line_start = self.out.len();
        let mut mfn_span = None;
        let mut kv = |k: &str, v: &str| {
            s.push(' ');
            s.push_str(k);
            s.push('=');
            if k == "METHOD_FULL_NAME" {
                mfn_span = Some(line_start + s.len()..line_start + s.len() + v.len());
            }
            s.push_str(v);
        };
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
        self.last_mfn_span = mfn_span;
        self.out.push_str(&s);
        self.out.push('\n');
    }

    fn note_call(&mut self, name: &str, argc: usize) {
        let e = self.stubs.entry(name.to_string()).or_insert(0);
        if argc > *e {
            *e = argc;
        }
    }

    fn emit_method(&mut self, f: Node, b: &[u8], d: usize, active: bool) {
        self.macros = self.macro_states.get(&f.id()).cloned().unwrap_or_default();
        self.symbols.clear();
        self.recovered_bindings.clear();
        self.symbol_call_types.clear();
        self.method_functions.clear();
        self.method_call_types.clear();
        let resolved = resolved_function_header(
            f,
            f.child_by_field_name("declarator").unwrap(),
            b,
            &self.macros,
        )
        .expect("function header");
        let (name, ret, params) = resolved.header;
        let full = self
            .definition_full_names
            .get(&f.id())
            .cloned()
            .unwrap_or_else(|| name.clone());
        let nested = d > 0;
        let sig = format!(
            "{ret}({})",
            params
                .iter()
                .map(|p| p.signature_type().to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        self.line(
            d,
            "METHOD",
            P {
                name: Some(name.clone()),
                code: Some(resolved.expanded_code.unwrap_or_else(|| esc(text(f, b)))),
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
            if let Some(call_type) = &p.call_type {
                self.symbol_call_types
                    .insert(p.name.clone(), call_type.clone());
            }
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
            if active {
                self.collect_phantoms(body, b);
                self.emit_block(body, b, block_order, d + 1);
            } else {
                // CDT retains inactive function declarations, but their body
                // blocks contain no executable children or phantom locals.
                self.line(
                    d + 1,
                    "BLOCK",
                    P {
                        code: Some(esc(text(body, b))),
                        tfn: Some("void".into()),
                        order: Some(block_order),
                        ..Default::default()
                    },
                );
            }
        }
        let is_static = has_leading_static(text(f, b));
        if is_static {
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
                order: Some((params.len() + 2 + usize::from(is_static)) as i64),
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
        if arity == 0 {
            // Joern represents an unresolved zero-argument call with p0.
            self.line(1, "METHOD_PARAMETER_IN", pin(0));
            self.line(1, "METHOD_PARAMETER_OUT", pin(0));
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
            return;
        }
        self.line(1, "METHOD_PARAMETER_IN", pin(1));
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
        self.line(1, "METHOD_PARAMETER_OUT", pin(1));
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

    fn emit_prototype(&mut self, name: &str, full: &str, code: &str, ret: &str, params: &[Param]) {
        self.line(
            0,
            "METHOD",
            P {
                name: Some(name.into()),
                code: Some(code.into()),
                full: Some(full.into()),
                sig: Some(format!(
                    "{ret}({})",
                    params
                        .iter()
                        .map(Param::signature_type)
                        .collect::<Vec<_>>()
                        .join(",")
                )),
                order: Some(1),
                ..Default::default()
            },
        );
        for (i, param) in params.iter().enumerate() {
            for label in ["METHOD_PARAMETER_IN", "METHOD_PARAMETER_OUT"] {
                self.line(
                    1,
                    label,
                    P {
                        name: Some(param.name.clone()),
                        code: Some(esc(&param.code)),
                        tfn: Some(param.ty.clone()),
                        order: Some((i + 1) as i64),
                        ..Default::default()
                    },
                );
            }
        }
        self.line(
            1,
            "BLOCK",
            P {
                tfn: Some("ANY".into()),
                order: Some((params.len() + 1) as i64),
                ..Default::default()
            },
        );
        let is_static = has_leading_static(code);
        if is_static {
            self.line(
                1,
                "MODIFIER",
                P {
                    order: Some((params.len() + 2) as i64),
                    ..Default::default()
                },
            );
        }
        self.line(
            1,
            "METHOD_RETURN",
            P {
                code: Some("RET".into()),
                tfn: Some(ret.into()),
                order: Some((params.len() + 2 + usize::from(is_static)) as i64),
                ..Default::default()
            },
        );
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
    fn emit_file_global(
        &mut self,
        root: Node,
        b: &[u8],
        file: &str,
        active_methods: &HashSet<usize>,
    ) {
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
        for n in translation_unit_items(root, b) {
            self.macros = self.macro_states.get(&n.id()).cloned().unwrap_or_default();
            match n.kind() {
                "struct_specifier" | "union_specifier" | "enum_specifier"
                    if n.child_by_field_name("body").is_some() =>
                {
                    self.emit_type_decl(n, b, 1);
                }
                "type_definition" if typedef_aggregate(n).is_some() => {
                    let aggregate = typedef_aggregate(n).unwrap();
                    self.emit_type_decl(aggregate, b, 1);
                    for alias in typedef_declarators(n) {
                        let name = typedef_alias_name(alias, b);
                        if aggregate.child_by_field_name("name").is_some() {
                            self.line(
                                1,
                                "TYPE_DECL",
                                P {
                                    name: Some(name.clone()),
                                    code: Some(esc(text(n, b))),
                                    full: Some(aggregate_alias_full_name(aggregate, alias, b)),
                                    order: Some(1),
                                    ..Default::default()
                                },
                            );
                        } else {
                            let keyword = if !array_dimensions(alias).is_empty() {
                                "typedef"
                            } else if aggregate.kind() == "union_specifier" {
                                "union"
                            } else {
                                "struct"
                            };
                            self.line(
                                1,
                                "LOCAL",
                                P {
                                    name: Some(name),
                                    code: Some(format!(
                                        "{} {}",
                                        text(n, b)[..aggregate.end_byte() - n.start_byte()]
                                            .split_whitespace()
                                            .collect::<Vec<_>>()
                                            .join(" "),
                                        esc(text(alias, b))
                                    )),
                                    tfn: Some(format!("{keyword}{}", decl_suffix(alias, b))),
                                    order: Some(1),
                                    ..Default::default()
                                },
                            );
                        }
                    }
                }
                "function_definition" => {
                    self.emit_method(n, b, 1, active_methods.contains(&n.id()));
                }
                _ => {}
            }
        }
        self.symbols.clear();
        self.recovered_bindings.clear();
        self.symbol_call_types.clear();
        self.method_functions.clear();
        self.method_call_types.clear();
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
        for n in translation_unit_items(root, b) {
            self.macros = self.macro_states.get(&n.id()).cloned().unwrap_or_default();
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
                "type_definition" if typedef_aggregate(n).is_some() => {
                    let aggregate = typedef_aggregate(n).unwrap();
                    self.line(
                        2,
                        "TYPE_REF",
                        P {
                            code: Some(aggregate_code(aggregate, b)),
                            tfn: Some(aggregate_name(aggregate, b)),
                            order: Some(slot),
                            ..Default::default()
                        },
                    );
                    slot += 1;
                    for alias in typedef_declarators(n) {
                        let dimensions = array_dimensions(alias);
                        if dimensions.is_empty() {
                            continue;
                        }
                        let sizes: Vec<_> = dimensions.into_iter().flatten().collect();
                        self.note_call("<operator>.arrayInitializer", sizes.len());
                        self.line(
                            2,
                            "CALL",
                            P {
                                name: Some("<operator>.arrayInitializer".into()),
                                code: Some(esc(text(alias, b))),
                                tfn: Some("ANY".into()),
                                mfn: Some("<operator>.arrayInitializer".into()),
                                order: Some(slot),
                                dispatch: Some("STATIC_DISPATCH".into()),
                                ..Default::default()
                            },
                        );
                        for (i, size) in sizes.into_iter().enumerate() {
                            let index = (i + 1) as i64;
                            self.emit_expr(size, b, 3, index, Some(index));
                        }
                        slot += 1;
                    }
                }
                "type_definition" => {
                    // Each ordinary alias keeps the complete statement CODE.
                    // Function and function-pointer typedef declarators do not
                    // produce Joern alias nodes or consume a global slot.
                    for alias in typedef_declarators(n) {
                        let name = type_binding_name(alias, b);
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
                }
                "declaration" => {
                    if let Some(prefix) = self.unknown_declaration_prefixes.get(&n.id()).cloned() {
                        self.line(
                            2,
                            "UNKNOWN",
                            P {
                                code: Some(prefix),
                                order: Some(slot),
                                ..Default::default()
                            },
                        );
                        slot += 1;
                    }
                    // LOCAL per object, then an assignment when initialised.
                    // Uninitialised file-scope arrays retain their dimensions
                    // without the allocation synthesized inside methods.
                    // Prototypes contribute nothing and consume no slot.
                    self.emit_declaration(n, b, &mut slot, 2, None, true);
                }
                "function_definition" => {
                    if let Some((name, _, _)) = fn_header(n, b) {
                        let full = self
                            .definition_full_names
                            .get(&n.id())
                            .map(|full| {
                                full.split("<duplicate>").next().unwrap_or(full).to_string()
                            })
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
        let name = aggregate_name(n, b);
        self.line(
            depth,
            "TYPE_DECL",
            P {
                name: Some(name.clone()),
                code: Some(aggregate_code(n, b)),
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
                            tfn: Some(format!("{ty}{}", member_decl_suffix(d, b, &self.macros))),
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
        let tag = aggregate_name(n, b);
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
                        let sizes: Vec<_> = array_dimensions(d).into_iter().flatten().collect();
                        if !sizes.is_empty() {
                            self.note_call("<operator>.arrayInitializer", sizes.len());
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
                            for (i, size) in sizes.into_iter().enumerate() {
                                let index = (i + 1) as i64;
                                self.emit_expr(size, b, depth + 3, index, Some(index));
                            }
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
        arg_texts: &[String],
        site: Node,
        depth: usize,
        order: i64,
        arg: Option<i64>,
        replacement_override: Option<&str>,
    ) {
        let (params, body, directive, defining_file) = {
            let m = &self.macros[name];
            (
                m.params.clone().unwrap_or_default(),
                m.body.clone(),
                m.directive.clone(),
                m.file.clone(),
            )
        };
        let replacement = replacement_override
            .map(str::to_string)
            .unwrap_or_else(|| substitute(&body, &params, arg_texts));
        let expansion = expand_body_expression(
            &replacement,
            &self.macros,
            &mut HashSet::from([name.to_string()]),
            &mut 65_536,
        );
        let ret = if site
            .parent()
            .is_some_and(|parent| parent.kind() == "expression_statement")
        {
            "ANY".to_string()
        } else {
            expansion_type(&expansion, &self.symbols, self.globals)
        };
        let full = format!("{defining_file}:{name}:{ret}({})", params.len());
        let source_owned = self.body_macro_sites.owns_node(site);
        if !source_owned {
            self.macro_method_files
                .entry(full.clone())
                .or_insert_with(|| self.file.clone());
            self.used_macros.entry(full.clone()).or_insert((
                name.to_string(),
                directive.clone(),
                params.len(),
                ret.clone(),
            ));
        }
        let address = self.at(self.line_no);
        self.line(
            depth,
            "CALL",
            P {
                name: Some(name.to_string()),
                code: Some(code.to_string()),
                tfn: Some(ret.clone()),
                mfn: Some(full.clone()),
                sig: Some(format!("{ret}({})", params.len())),
                order: Some(order),
                arg,
                dispatch: Some("INLINED".into()),
                ..Default::default()
            },
        );
        if source_owned {
            let key = (site.id(), name.to_string(), params.len(), ret.clone());
            let index = *self.macro_use_ids.entry(key).or_insert_with(|| {
                self.macro_uses.push(MacroUse {
                    offset: site.start_byte(),
                    metadata: MacroMetadata {
                        name: name.to_string(),
                        directive,
                        file: defining_file,
                    },
                    arity: params.len(),
                    ret,
                    placements: Vec::new(),
                });
                self.macro_uses.len() - 1
            });
            self.macro_uses[index].placements.push((
                self.block.clone(),
                self.last_mfn_span
                    .clone()
                    .expect("generated macro call has a method name"),
                address,
            ));
        }
        let invocation_types = self.typedefs_at(site);
        let expansion_source = format!("void __m() {{ {expansion}; }}");
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_c::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&expansion_source, None).unwrap();
        let previous_types = self.enter_type_tree(
            tree.root_node(),
            expansion_source.as_bytes(),
            invocation_types,
        );
        let mut candidates = vec![tree.root_node()];
        let mut expression_nodes = Vec::new();
        while let Some(node) = candidates.pop() {
            if !matches!(node.kind(), "parenthesized_expression" | "field_identifier") {
                expression_nodes.push(node);
            }
            candidates.extend(named_children(node).into_iter().rev());
        }
        let before_args = self.argument_count;
        let mut emitted_args = 0;
        for (i, a) in arg_texts.iter().enumerate() {
            let normalized = macro_argument_match_code(a);
            let found = expression_nodes.iter().find(|node| {
                let raw = text(**node, expansion_source.as_bytes());
                if node.kind() == "identifier" {
                    if self
                        .typedef_cast_identifier(**node, expansion_source.as_bytes())
                        .is_some()
                    {
                        return raw == normalized;
                    }
                    if node.parent().is_some_and(|parent| {
                        parent.kind() == "call_expression"
                            && parent.child_by_field_name("function") == Some(**node)
                    }) {
                        return false;
                    }
                    if self.globals.contains_key(raw) {
                        format!("<global> {raw}") == normalized
                    } else if self.symbols.contains_key(raw)
                        && self.recovered_bindings.contains(raw)
                    {
                        format!("<unknown> {raw}") == normalized
                    } else if self.symbols.contains_key(raw)
                        || self.enumerators.iter().any(|name| name == raw)
                    {
                        raw == normalized
                    } else {
                        format!("<unknown> {raw}") == normalized
                    }
                } else {
                    raw == normalized
                        && matches!(
                            node.kind(),
                            "call_expression"
                                | "binary_expression"
                                | "unary_expression"
                                | "pointer_expression"
                                | "update_expression"
                                | "cast_expression"
                                | "subscript_expression"
                                | "field_expression"
                                | "conditional_expression"
                                | "number_literal"
                                | "string_literal"
                                | "char_literal"
                                | "sizeof_expression"
                        )
                }
            });
            if let Some(node) = found {
                emitted_args += 1;
                let was_copying = self.copying_macro_argument;
                self.copying_macro_argument = true;
                let macros = std::mem::replace(&mut self.macros, Arc::new(HashMap::new()));
                if let Some(ty) = self.typedef_cast_identifier(*node, expansion_source.as_bytes()) {
                    self.line(
                        depth + 1,
                        "TYPE_REF",
                        P {
                            code: Some(ty.clone()),
                            tfn: Some(ty),
                            order: Some(emitted_args),
                            arg: Some((i + 1) as i64),
                            ..Default::default()
                        },
                    );
                } else {
                    self.emit_expr(
                        *node,
                        expansion_source.as_bytes(),
                        depth + 1,
                        emitted_args,
                        Some((i + 1) as i64),
                    );
                }
                self.macros = macros;
                self.copying_macro_argument = was_copying;
            }
        }
        // MacroHandler counts descendant ARGUMENT edges in copied subtrees.
        let expansion_index = (self.argument_count - before_args) as i64 + 1;
        let compound = expansion_expr_node(tree.root_node())
            .is_some_and(|root| root.kind() == "compound_statement");
        self.line(
            depth + 1,
            "BLOCK",
            P {
                code: compound.then(|| esc(code)),
                tfn: Some(if compound { "void" } else { "ANY" }.into()),
                order: Some(emitted_args + 1),
                arg: Some(expansion_index),
                ..Default::default()
            },
        );
        let macros = std::mem::replace(&mut self.macros, Arc::new(HashMap::new()));
        let previous_code = self.macro_expansion_code.replace(code.to_string());
        self.emit_expansion(&expansion, depth + 2);
        self.macro_expansion_code = previous_code;
        self.macros = macros;
        self.leave_type_tree(previous_types);
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
        // CDT prints expanded adjacent strings as a single token, including
        // inside a larger expansion expression. Ordinary source expressions
        // retain their original token spelling in emit_expr.
        if let Some(joined) = join_expansion_strings(tree.root_node(), b) {
            if let Some(joined_tree) = parser.parse(&joined, None) {
                if let Some(joined_expr) = expansion_expr_node(joined_tree.root_node()) {
                    self.emit_expansion_root(joined_expr, joined.as_bytes(), depth);
                    return;
                }
            }
        }
        self.emit_expansion_root(expr, b, depth);
    }

    fn emit_expansion_root(&mut self, root: Node, b: &[u8], depth: usize) {
        let previous_types = self.enter_type_tree(root, b, self.default_typedefs.clone());
        let previous_root = self.macro_expansion_root.replace(root.id());
        if root.kind() == "compound_statement" {
            // The compound is the expansion BLOCK already emitted by the
            // macro handler; its statements are direct children of that block.
            self.emit_block_contents(root, b, depth);
        } else if root.kind().ends_with("_statement") || root.kind() == "declaration" {
            self.emit_stmt(root, b, &mut 1, depth);
        } else {
            self.emit_expr(root, b, depth, 1, None);
        }
        self.macro_expansion_root = previous_root;
        self.leave_type_tree(previous_types);
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
        collect_decl_names(
            body,
            b,
            &self.macros,
            Some(self.body_macro_sites),
            &mut shadowed,
        );
        let mut seen = Vec::new();
        self.walk_phantoms(body, b, &shadowed, &mut seen);
        // VariableScopeManager prepends pending references. Its first resolved
        // reference therefore determines each synthetic local's CODE and the
        // reverse-last-use order of all synthetic locals.
        let positions: HashMap<_, _> = seen
            .into_iter()
            .enumerate()
            .map(|(i, name)| (name, i))
            .collect();
        self.phantoms
            .sort_by_key(|phantom| std::cmp::Reverse(positions[&phantom.name]));
    }

    /// An unresolved token between string literals invalidates CDT's expanded
    /// expression. Declaration initializers are recovered from their original
    /// source without the preprocessor context; ordinary returns stay UNKNOWN.
    fn needs_macro_recovery(&mut self, node: Node, bytes: &[u8]) -> bool {
        if self.macros.is_empty() || self.macro_expansion_code.is_some() {
            return false;
        }
        if let Some(&recovery) = self.recovery_candidates.get(&node.id()) {
            return recovery;
        }
        let mut pending = vec![node];
        let mut has_macro = false;
        while let Some(n) = pending.pop() {
            if matches!(n.kind(), "identifier" | "field_identifier")
                && self.macros.contains_key(text(n, bytes))
            {
                has_macro = true;
                break;
            }
            pending.extend(named_children(n));
        }
        let recovery = if has_macro {
            let expanded = expand_body_expression(
                text(node, bytes),
                &self.macros,
                &mut HashSet::new(),
                &mut 65_536,
            );
            let source = format!("void __recovery() {{ {expanded}; }}");
            let mut parser = Parser::new();
            parser
                .set_language(&tree_sitter_c::LANGUAGE.into())
                .unwrap();
            let tree = parser.parse(&source, None).unwrap();
            let mut pending = vec![tree.root_node()];
            let mut invalid = false;
            while let Some(n) = pending.pop() {
                if n.kind() == "concatenated_string"
                    && named_children(n).iter().any(|c| c.kind() == "identifier")
                {
                    // CDT recovers an enclosing call as a failed expression.
                    // A bare string initializer has a different token-level
                    // recovery shape, outside this path.
                    let mut parent = n.parent();
                    while let Some(ancestor) = parent {
                        if ancestor.kind() == "argument_list" {
                            invalid = true;
                            break;
                        }
                        parent = ancestor.parent();
                    }
                    if invalid {
                        break;
                    }
                }
                pending.extend(named_children(n));
            }
            invalid
        } else {
            false
        };
        self.recovery_candidates.insert(node.id(), recovery);
        recovery
    }

    fn declaration_needs_macro_recovery(&mut self, node: Node, bytes: &[u8]) -> bool {
        // A failed for initializer is recovered as statement fragments by CDT,
        // rather than the original declaration used for block declarations.
        !node
            .parent()
            .is_some_and(|parent| parent.kind() == "for_statement")
            && named_children(node).into_iter().any(|declarator| {
                declarator
                    .child_by_field_name("value")
                    .is_some_and(|value| self.needs_macro_recovery(value, bytes))
            })
    }

    fn walk_phantoms(&mut self, body: Node, b: &[u8], shadowed: &[String], seen: &mut Vec<String>) {
        let previous_macros = self.macros.clone();
        let mut stack = vec![body];
        while let Some(n) = stack.pop() {
            if let Some(macros) = self.source_macros_at(n, b).cloned() {
                self.macros = macros;
            }
            if n.kind() == "declaration" && self.declaration_needs_macro_recovery(n, b) {
                let macros = std::mem::replace(&mut self.macros, Arc::new(HashMap::new()));
                let recovering = std::mem::replace(&mut self.recovering_expression, true);
                self.walk_phantoms(n, b, shadowed, seen);
                self.recovering_expression = recovering;
                self.macros = macros;
                continue;
            }
            if matches!(n.kind(), "return_statement" | "expression_statement")
                && self.needs_macro_recovery(n, b)
            {
                continue;
            }
            if n.parent().is_some_and(|parent| {
                matches!(
                    parent.kind(),
                    "if_statement" | "while_statement" | "do_statement"
                ) && parent.child_by_field_name("condition") == Some(n)
            }) && self.needs_macro_recovery(n, b)
            {
                continue;
            }
            let invocation = if n.kind() == "identifier" {
                self.macros
                    .get(text(n, b))
                    .filter(|m| m.params.is_none())
                    .map(|m| (text(n, b).to_string(), m.clone(), Vec::new()))
            } else if n.kind() == "call_expression" {
                n.child_by_field_name("function").and_then(|f| {
                    self.macros
                        .get(text(f, b))
                        .filter(|m| m.params.is_some())
                        .map(|m| {
                            (
                                text(f, b).to_string(),
                                m.clone(),
                                n.child_by_field_name("arguments")
                                    .and_then(|args| macro_arguments(text(args, b), 0))
                                    .map(|(args, _)| args)
                                    .unwrap_or_default(),
                            )
                        })
                })
            } else if n.kind() == "field_expression" {
                n.child_by_field_name("field").and_then(|field| {
                    let name = text(field, b);
                    self.macros
                        .get(name)
                        .filter(|m| m.params.is_none())
                        .map(|m| {
                            // Preserve the receiver so member names are not mistaken
                            // for globals while collecting introduced index operands.
                            let raw = text(n, b);
                            let start = field.start_byte() - n.start_byte();
                            let end = field.end_byte() - n.start_byte();
                            let mut definition = m.clone();
                            definition.body = format!("{}{}{}", &raw[..start], m.body, &raw[end..]);
                            (name.to_string(), definition, Vec::new())
                        })
                })
            } else {
                None
            };
            if let Some((name, definition, args)) = invocation {
                let replacement = substitute(
                    &definition.body,
                    definition.params.as_deref().unwrap_or_default(),
                    &args,
                );
                let expansion = expand_body_expression(
                    &replacement,
                    &self.macros,
                    &mut HashSet::from([name]),
                    &mut 65_536,
                );
                let source = format!("void __m() {{ {expansion}; }}");
                let mut parser = Parser::new();
                parser
                    .set_language(&tree_sitter_c::LANGUAGE.into())
                    .unwrap();
                let tree = parser.parse(&source, None).unwrap();
                if let Some(expr) = expansion_expr_node(tree.root_node()) {
                    let macros = std::mem::replace(&mut self.macros, Arc::new(HashMap::new()));
                    let mut expansion_shadowed = shadowed.to_vec();
                    collect_decl_names(
                        expr,
                        source.as_bytes(),
                        &self.macros,
                        None,
                        &mut expansion_shadowed,
                    );
                    let previous_types =
                        self.enter_type_tree(expr, source.as_bytes(), self.typedefs_at(n));
                    self.walk_phantoms(expr, source.as_bytes(), &expansion_shadowed, seen);
                    self.leave_type_tree(previous_types);
                    self.macros = macros;
                }
                continue;
            }

            if !prototype_headers(n, b).is_empty() {
                continue;
            }
            if self.typedef_cast(n, b).is_some() {
                if let Some(args) = n.child_by_field_name("arguments") {
                    stack.push(args);
                }
                continue;
            }
            match n.kind() {
                "identifier" => {
                    let name = text(n, b).to_string();
                    // A direct call target is a callable name, not an
                    // unresolved variable. Pointer-valued callees still use
                    // their declared local/global symbol below.
                    let direct_callee = n.parent().is_some_and(|parent| {
                        parent.kind() == "call_expression"
                            && parent.child_by_field_name("function") == Some(n)
                    });
                    let variable_reference = self.globals.contains_key(&name)
                        || self.enumerators.contains(&name)
                        || (!self.macros.contains_key(&name)
                            && (self.recovering_expression
                                || (!self.functions.contains_key(&name)
                                    && !self.method_functions.contains_key(&name)))
                            && !direct_callee);
                    if !shadowed.contains(&name) && variable_reference {
                        if let Some(position) = seen.iter().position(|prior| prior == &name) {
                            seen.remove(position);
                            seen.push(name.clone());
                            if self.globals.contains_key(&name) {
                                if let Some(phantom) =
                                    self.phantoms.iter_mut().find(|p| p.name == name)
                                {
                                    phantom.code = format!(
                                        "{} {name}",
                                        if self.recovering_expression {
                                            "<unknown>"
                                        } else {
                                            "<global>"
                                        }
                                    );
                                }
                            }
                            continue;
                        }
                        if let Some(ty) = self.globals.get(&name) {
                            seen.push(name.clone());
                            self.phantoms.push(Phantom {
                                code: format!(
                                    "{} {name}",
                                    if self.recovering_expression {
                                        "<unknown>"
                                    } else {
                                        "<global>"
                                    }
                                ),
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
                            && (self.recovering_expression
                                || (!self.functions.contains_key(&name)
                                    && !self.method_functions.contains_key(&name)))
                            && !direct_callee
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
                    if let Some(position) = seen.iter().position(|name| name == "NULL") {
                        seen.remove(position);
                        seen.push("NULL".into());
                    } else {
                        seen.push("NULL".into());
                        self.phantoms.push(Phantom {
                            name: "NULL".into(),
                            code: "<unknown> NULL".into(),
                            ty: "ANY".into(),
                        });
                    }
                }
                "sizeof_expression" => {
                    if let Some(phantom) = self.sizeof_identifier(n, b) {
                        if let Some(position) = seen.iter().position(|name| name == &phantom.name) {
                            seen.remove(position);
                            seen.push(phantom.name.clone());
                            if let Some(previous) =
                                self.phantoms.iter_mut().find(|p| p.name == phantom.name)
                            {
                                *previous = phantom;
                            }
                        } else {
                            seen.push(phantom.name.clone());
                            self.phantoms.push(phantom);
                        }
                        continue;
                    }
                }
                // Only the selected branch contributes pending references;
                // directive condition identifiers are not C references.
                "preproc_if" | "preproc_ifdef" | "preproc_elif" | "preproc_elifdef"
                | "preproc_else" => {
                    stack.extend(kept_preproc_children(n, b, &self.macros).into_iter().rev());
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
        self.macros = previous_macros;
    }

    /// Emit a BLOCK node and its statements, with a fresh child ORDER sequence.
    fn emit_block(&mut self, body: Node, b: &[u8], order: i64, depth: usize) {
        self.line(
            depth,
            "BLOCK",
            P {
                code: Some(esc(self
                    .macro_expansion_code
                    .as_deref()
                    .unwrap_or(text(body, b)))),
                tfn: Some("void".into()),
                order: Some(order),
                ..Default::default()
            },
        );
        self.emit_block_contents(body, b, depth + 1);
    }

    fn emit_block_contents(&mut self, body: Node, b: &[u8], depth: usize) {
        let outer_symbols = self.symbols.clone();
        let outer_recovered = self.recovered_bindings.clone();
        let outer_symbol_calls = self.symbol_call_types.clone();
        let outer_functions = self.method_functions.clone();
        let outer_call_types = self.method_call_types.clone();
        let outer_bindings = self.sym_line.clone();
        for ph in std::mem::take(&mut self.phantoms) {
            self.line(
                depth,
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
            self.emit_stmt(s, b, &mut so, depth);
        }
        self.symbols = outer_symbols;
        self.recovered_bindings = outer_recovered;
        self.symbol_call_types = outer_symbol_calls;
        self.method_functions = outer_functions;
        self.method_call_types = outer_call_types;
        self.sym_line = outer_bindings;
    }

    /// A block-level statement. `order` is the running 1-based child position.
    fn emit_stmt(&mut self, n: Node, b: &[u8], order: &mut i64, depth: usize) {
        let previous = self
            .source_macros_at(n, b)
            .cloned()
            .map(|macros| std::mem::replace(&mut self.macros, macros));
        self.emit_stmt_in_context(n, b, order, depth);
        if let Some(previous) = previous {
            self.macros = previous;
        }
    }

    fn emit_stmt_in_context(&mut self, n: Node, b: &[u8], order: &mut i64, depth: usize) {
        if n.kind() == "return_statement" && self.needs_macro_recovery(n, b) {
            self.line(
                depth,
                "UNKNOWN",
                P {
                    code: Some(esc(text(n, b))),
                    order: Some(*order),
                    ..Default::default()
                },
            );
            *order += 1;
            return;
        }
        if n.kind() == "expression_statement" && self.needs_macro_recovery(n, b) {
            return;
        }
        // Expanded controls can carry the invocation as CODE (notably a
        // do-while). Preserve their parsed kind independently for CFG lowering.
        if self.macro_expansion_code.is_some()
            && matches!(
                n.kind(),
                "if_statement"
                    | "for_statement"
                    | "while_statement"
                    | "do_statement"
                    | "switch_statement"
                    | "break_statement"
                    | "continue_statement"
                    | "goto_statement"
            )
        {
            self.expansion_control_kinds.insert(
                self.at(self.line_no),
                n.kind().trim_end_matches("_statement").to_string(),
            );
        }
        match n.kind() {
            "compound_statement" => {
                self.emit_block(n, b, *order, depth);
                *order += 1;
            }
            "declaration" => {
                self.emit_declaration(n, b, order, depth, None, false);
            }
            "type_definition" if typedef_aggregate(n).is_none() => {
                self.emit_local_typedef(n, b, depth, order);
            }
            "if_statement" => self.emit_if(n, b, order, depth),
            "for_statement" => self.emit_for(n, b, order, depth),
            "preproc_if" | "preproc_ifdef" | "preproc_elif" | "preproc_elifdef"
            | "preproc_else" => {
                for child in kept_preproc_children(n, b, &self.macros) {
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
                        code: Some(esc(self
                            .macro_expansion_code
                            .as_deref()
                            .unwrap_or(text(n, b)))),
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
                        code: Some(esc(&self
                            .macro_expansion_code
                            .as_ref()
                            .map(|code| {
                                format!(
                                    "{code}{}",
                                    if self.macro_expansion_root == Some(n.id()) {
                                        ";"
                                    } else {
                                        ""
                                    }
                                )
                            })
                            .unwrap_or_else(|| text(n, b).to_string()))),
                        order: Some(o),
                        ..Default::default()
                    },
                );
                let cs = self.line_no - 1;
                let body_start = self.line_no;
                if let Some(body) = n.child_by_field_name("body") {
                    self.emit_loop_body(body, b, depth + 1, 1, cs, "DO_BODY");
                }
                let condition_order = if self.line_no > body_start { 2 } else { 1 };
                if let Some(cond) = n.child_by_field_name("condition") {
                    let ci = self.line_no;
                    self.emit_condition(
                        cond.named_child(0).unwrap_or(cond),
                        b,
                        depth + 1,
                        condition_order,
                    );
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
                    self.emit_condition(c.named_child(0).unwrap_or(c), b, depth + 1, 1);
                    self.edge("CONDITION", self.at(cs), self.at(ci));
                }
                if let Some(body) = n.child_by_field_name("body") {
                    self.emit_loop_body(body, b, depth + 1, 2, cs, "TRUE_BODY");
                }
            }
            "return_statement" => {
                let o = *order;
                *order += 1;
                self.line(
                    depth,
                    "RETURN",
                    P {
                        code: Some(esc(&self
                            .macro_expansion_code
                            .as_ref()
                            .map(|code| {
                                format!(
                                    "{code}{}",
                                    if self.macro_expansion_root == Some(n.id()) {
                                        ";"
                                    } else {
                                        ""
                                    }
                                )
                            })
                            .unwrap_or_else(|| text(n, b).to_string()))),
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
                code: Some(esc(self
                    .macro_expansion_code
                    .as_deref()
                    .unwrap_or(text(n, b)))),
                order: Some(o),
                ..Default::default()
            },
        );
        if let Some(cond) = n.child_by_field_name("condition") {
            let ci = self.line_no;
            self.emit_condition(cond.named_child(0).unwrap_or(cond), b, depth + 1, 1);
            self.edge("CONDITION", self.at(cs), self.at(ci));
        }
        if let Some(cons) = n.child_by_field_name("consequence") {
            let bi = self.line_no;
            if cons.kind() == "compound_statement" {
                self.emit_block(cons, b, 2, depth + 1);
            } else {
                // CDT wraps a braceless consequence in a synthetic, CODE-less
                // block, just as it does for a braceless `else` body.
                self.line(
                    depth + 1,
                    "BLOCK",
                    P {
                        tfn: Some("ANY".into()),
                        order: Some(2),
                        ..Default::default()
                    },
                );
                let mut so = 1i64;
                self.emit_stmt(cons, b, &mut so, depth + 2);
            }
            self.edge("TRUE_BODY", self.at(cs), self.at(bi));
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
        let outer_symbols = self.symbols.clone();
        let outer_recovered = self.recovered_bindings.clone();
        let outer_symbol_calls = self.symbol_call_types.clone();
        let outer_functions = self.method_functions.clone();
        let outer_call_types = self.method_call_types.clone();
        let outer_bindings = self.sym_line.clone();
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
                    "for ({}{}{};{})",
                    self.macro_expansion_code
                        .as_deref()
                        .map(str::to_string)
                        .unwrap_or_else(|| part(init)),
                    if self.macro_expansion_code.is_some() {
                        ""
                    } else {
                        ";"
                    },
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
                if let Some(initializer) =
                    self.emit_declaration(i, b, &mut co, depth + 1, Some(1), false)
                {
                    self.edge("FOR_INIT", self.at(cs), self.at(initializer));
                } else {
                    // CDT reserves a single declaration's absent initializer.
                    co += 1;
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
            self.emit_condition(c, b, depth + 1, co);
            self.edge("CONDITION", self.at(cs), self.at(ci));
        }
        co += 1;
        if let Some(u) = update {
            let ui = self.line_no;
            self.emit_expr(u, b, depth + 1, co, None);
            self.edge("FOR_UPDATE", self.at(cs), self.at(ui));
        }
        co += 1;
        if let Some(body) = n.child_by_field_name("body") {
            self.emit_loop_body(body, b, depth + 1, co, cs, "FOR_BODY");
        }
        self.symbols = outer_symbols;
        self.recovered_bindings = outer_recovered;
        self.symbol_call_types = outer_symbol_calls;
        self.method_functions = outer_functions;
        self.method_call_types = outer_call_types;
        self.sym_line = outer_bindings;
    }

    /// Loop bodies retain their actual statement shape, including a direct
    /// CALL/RETURN/control node when braces are absent. An empty statement has
    /// no AST node and consequently no body edge.
    fn emit_loop_body(
        &mut self,
        body: Node,
        b: &[u8],
        depth: usize,
        mut order: i64,
        control: usize,
        role: &str,
    ) {
        let body_index = self.line_no;
        if body.kind() == "compound_statement" {
            self.emit_block(body, b, order, depth);
        } else {
            self.emit_stmt(body, b, &mut order, depth);
        }
        if self.line_no > body_index {
            self.edge(role, self.at(control), self.at(body_index));
        }
    }

    /// CDT normalizes a bare identifier truth test, but leaves comparisons,
    /// calls, arithmetic and logical operators as their existing expressions.
    fn emit_condition(&mut self, expression: Node, b: &[u8], depth: usize, order: i64) {
        let identifier = unwrap_paren(expression);
        let name = text(identifier, b);
        let recovery = self.needs_macro_recovery(expression, b);
        if !recovery && (identifier.kind() != "identifier" || self.macros.contains_key(name)) {
            self.emit_expr(expression, b, depth, order, None);
            return;
        }
        let pointer = !recovery
            && self
                .symbols
                .get(name)
                .or_else(|| self.globals.get(name))
                .is_some_and(|ty| ty.ends_with('*'));
        let zero = if pointer { "NULL" } else { "0" };
        self.types.insert("int".into());
        self.note_call("<operator>.notEquals", 2);
        self.line(
            depth,
            "CALL",
            P {
                name: Some("<operator>.notEquals".into()),
                code: Some(esc(&format!("{} != {zero}", text(expression, b)))),
                tfn: Some("int".into()),
                mfn: Some("<operator>.notEquals".into()),
                order: Some(order),
                dispatch: Some("STATIC_DISPATCH".into()),
                ..Default::default()
            },
        );
        if recovery {
            self.line(
                depth + 1,
                "UNKNOWN",
                P {
                    code: Some(esc(text(expression, b))),
                    order: Some(1),
                    arg: Some(1),
                    ..Default::default()
                },
            );
        } else {
            self.emit_expr(identifier, b, depth + 1, 1, Some(1));
        }
        self.line(
            depth + 1,
            "LITERAL",
            P {
                code: Some(zero.into()),
                tfn: Some(if pointer { "ANY" } else { "int" }.into()),
                order: Some(2),
                arg: Some(2),
                ..Default::default()
            },
        );
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
        file_scope: bool,
    ) -> Option<usize> {
        for (declarator, header) in prototype_header_entries(n, b) {
            let (name, ret, call_type) = resolved_function_header(n, declarator, b, &self.macros)
                .map(|resolved| (resolved.header.0, resolved.header.1, resolved.call_type))
                .unwrap_or_else(|| {
                    (
                        header.0,
                        header.1,
                        function_return_type(n, declarator, b, TypeRole::Expression),
                    )
                });
            self.symbols.remove(&name);
            self.symbol_call_types.remove(&name);
            self.method_call_types.insert(name.clone(), call_type);
            self.method_functions.insert(name, ret);
        }
        let ty = declaration_type(n, b, TypeRole::Declaration);
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
        }
        let mut items: Vec<DeclItem> = Vec::new();
        for d in named_children(n) {
            let (decl, init) = match d.kind() {
                "init_declarator" => (
                    d.child_by_field_name("declarator"),
                    d.child_by_field_name("value"),
                ),
                "identifier"
                | "pointer_declarator"
                | "array_declarator"
                | "function_declarator" => (Some(d), None),
                _ => (None, None),
            };
            let Some(decl) = decl else { continue };
            if is_function_declaration(decl) {
                continue;
            }
            let name = innermost_id(decl, b);
            let full_ty = declared_object_type(n, decl, b);
            let function_pointer = find_function_declarator(decl).is_some();
            self.method_functions.remove(&name);
            self.method_call_types.remove(&name);
            self.symbol_call_types.remove(&name);
            self.symbols.insert(name.clone(), full_ty.clone());
            self.recovered_bindings.remove(&name);
            let lo = *order;
            *order += 1;
            self.line(
                depth,
                "LOCAL",
                P {
                    name: Some(name.clone()),
                    code: Some(if let Some(code) = &self.macro_expansion_code {
                        esc(&if init.is_some() {
                            code.clone()
                        } else {
                            format!("{code} {code}")
                        })
                    } else if function_pointer {
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
            });
        }
        // CDT's astForInitializer additionally registers the first component
        // of a primitive object's type (`unsigned char c = 0` registers
        // `unsigned`, as pinned by memcmp.c). A declaration without an explicit
        // initializer, including an array's implicit allocation, does not take
        // that path. Check each declarator so an initialized nested pointer declarator
        // cannot register the base of an uninitialized ordinary neighbor.
        // Keep nonprimitive registration: tags also register their base via
        // independent paths (`struct Packet *p` still needs a Packet TYPE).
        let raw_type = n
            .child_by_field_name("type")
            .map(|node| text(node, b))
            .unwrap_or("ANY");
        let primitive = primitive_type(raw_type, TypeRole::Declaration).is_some();
        if items.iter().any(|item| {
            if primitive {
                item.init.is_some() && !nested_declarator_name(item.decl)
            } else {
                find_function_declarator(item.decl).is_none()
            }
        }) {
            let registered = if primitive {
                // CDT's extra decl-specifier registration uses the first word
                // of the declared spelling (`short unsigned int` -> `short`).
                ty.split_whitespace().next().unwrap_or(&ty)
            } else {
                &ty
            };
            self.types.insert(registered.to_string());
        }
        // A for declaration contributes one initializer slot after its LOCALs.
        // Multiple declarators share a synthetic block, including an empty one
        // when none has an initializer. Ordinary declarations stay flattened.
        let initializer = self.line_no;
        let grouped = assign_arg.is_some() && items.len() > 1;
        if grouped {
            self.line(
                depth,
                "BLOCK",
                P {
                    tfn: Some("ANY".into()),
                    order: Some(*order),
                    ..Default::default()
                },
            );
            *order += 1;
        }
        let depth = depth + usize::from(grouped);
        let mut group_order = 1;
        let order = if grouped { &mut group_order } else { order };
        // Pass 2: emit all array dimension/alloc lowerings before initializers.
        for it in &items {
            let dimensions = array_dimensions(it.decl);
            let sizes: Vec<_> = dimensions.iter().flatten().copied().collect();
            // Sized local arrays allocate before their explicit initializer.
            // File-scope arrays only emit dimensions when no initializer exists,
            // including an empty arrayInitializer for an unsized declaration.
            if (file_scope && it.init.is_none() && !dimensions.is_empty())
                || (!file_scope && dimensions.first().is_some_and(Option::is_some))
            {
                let ao = *order;
                *order += 1;
                // File-scope array declarations retain their dimensions in
                // an arrayInitializer. Only block-scope arrays synthesize
                // assignment -> alloc(type, dimensions); treating globals
                // as allocations also inflates the project-wide alloc stub.
                if file_scope {
                    self.note_call("<operator>.arrayInitializer", sizes.len());
                    self.line(
                        depth,
                        "CALL",
                        P {
                            name: Some("<operator>.arrayInitializer".into()),
                            code: Some(esc(text(it.outer, b))),
                            tfn: Some("ANY".into()),
                            mfn: Some("<operator>.arrayInitializer".into()),
                            order: Some(ao),
                            dispatch: Some("STATIC_DISPATCH".into()),
                            ..Default::default()
                        },
                    );
                    for (i, size) in sizes.into_iter().enumerate() {
                        let k = (i + 1) as i64;
                        self.emit_expr(size, b, depth + 1, k, Some(k));
                    }
                    continue;
                }
                self.note_call("<operator>.assignment", 2);
                self.note_call("<operator>.alloc", sizes.len() + 1);
                self.line(
                    depth,
                    "CALL",
                    P {
                        name: Some("<operator>.assignment".into()),
                        code: Some(if self.macro_expansion_code.is_some() {
                            expanded_declaration_code(n, it.decl, b)
                        } else {
                            esc(text(it.outer, b))
                        }),
                        tfn: Some("void".into()),
                        mfn: Some("<operator>.assignment".into()),
                        order: Some(ao),
                        arg: if grouped { Some(ao) } else { assign_arg },
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                self.line(
                    depth + 1,
                    "IDENTIFIER",
                    P {
                        name: Some(it.name.clone()),
                        code: Some(if let Some(code) = &self.macro_expansion_code {
                            esc(code)
                        } else if nested_declarator_name(it.decl) {
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
                self.line(
                    depth + 1,
                    "CALL",
                    P {
                        name: Some("<operator>.alloc".into()),
                        code: Some(if self.macro_expansion_code.is_some() {
                            expanded_declaration_code(n, it.decl, b)
                        } else {
                            esc(text(it.outer, b))
                        }),
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
                        tfn: Some(it.full_ty.clone()),
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
        // CDT recovers the whole declaration, including valid sibling
        // initializers, without macro expansion or expression name resolution.
        let recovery = self.declaration_needs_macro_recovery(n, b);
        // Pass 3: explicit initializers follow every declarator's allocation.
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
                        // CDT attributes expanded declarations to their macro
                        // invocation, but the initializer CALL retains only
                        // the declaration's specifier and pointer prefix.
                        code: Some(if self.macro_expansion_code.is_some() {
                            expanded_declaration_code(n, it.decl, b)
                        } else {
                            esc(text(it.outer, b))
                        }),
                        tfn: Some("void".into()),
                        mfn: Some("<operator>.assignment".into()),
                        order: Some(ao),
                        arg: if grouped { Some(ao) } else { assign_arg },
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                self.line(
                    depth + 1,
                    "IDENTIFIER",
                    P {
                        name: Some(it.name.clone()),
                        code: Some(if let Some(code) = &self.macro_expansion_code {
                            esc(code)
                        } else if nested_declarator_name(it.decl) {
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
                if recovery {
                    let macros = std::mem::replace(&mut self.macros, Arc::new(HashMap::new()));
                    let recovering = std::mem::replace(&mut self.recovering_expression, true);
                    self.emit_expr(v, b, depth + 1, 2, Some(2));
                    self.recovering_expression = recovering;
                    self.macros = macros;
                    self.recovered_bindings.insert(it.name.clone());
                } else {
                    self.emit_expr(v, b, depth + 1, 2, Some(2));
                }
            }
        }
        (self.line_no > initializer).then_some(initializer)
    }

    /// A field-token replacement changes the expression shape. CDT attributes
    /// generated access operators to the original access (p->Len), while field
    /// identifiers use expanded names. A pure replacement subexpression can own
    /// the INLINED wrapper instead of the whole access.
    fn emit_direct_field_macro(
        &mut self,
        n: Node,
        b: &[u8],
        depth: usize,
        order: i64,
        arg: Option<i64>,
    ) -> bool {
        if self.field_macro_codes.contains_key(&n.id()) {
            // This region was already rescanned with the disabled-macro set.
            return false;
        }
        let Some(field) = n.child_by_field_name("field") else {
            return false;
        };
        let name = text(field, b);
        let Some(definition) = self.macros.get(name).filter(|m| m.params.is_none()) else {
            return false;
        };
        let replacement = expand_body_expression(
            &definition.body,
            &self.macros,
            &mut HashSet::from([name.to_string()]),
            &mut 65_536,
        );
        if replacement == name {
            return false;
        }
        let original = text(n, b);
        let start = field.start_byte() - n.start_byte();
        let end = field.end_byte() - n.start_byte();
        let expression = format!("{}{}{}", &original[..start], replacement, &original[end..]);
        let prefix = "void __m() { ";
        let source = format!("{prefix}{expression}; }}");
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_c::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&source, None).unwrap();
        if tree.root_node().has_error() {
            return false;
        }
        let Some(root) = expansion_expr_node(tree.root_node()) else {
            return false;
        };
        let mut codes = HashMap::new();
        let mut piece = None;
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if node.named_child_count() > 0 && node.end_byte() > prefix.len() + start {
                codes.insert(node.id(), original.to_string());
            }
            if piece.is_none()
                && node.start_byte() >= prefix.len() + start
                && matches!(
                    node.kind(),
                    "identifier"
                        | "number_literal"
                        | "string_literal"
                        | "char_literal"
                        | "call_expression"
                        | "binary_expression"
                        | "unary_expression"
                        | "pointer_expression"
                        | "update_expression"
                        | "cast_expression"
                        | "subscript_expression"
                        | "field_expression"
                        | "conditional_expression"
                        | "sizeof_expression"
                )
            {
                piece = Some((node.id(), name.to_string()));
            }
            pending.extend(named_children(node).into_iter().rev());
        }
        let previous_types = self.enter_type_tree(root, source.as_bytes(), self.typedefs_at(n));
        let previous = std::mem::replace(&mut self.field_macro_codes, codes);
        let previous_piece = std::mem::replace(&mut self.field_macro_piece, piece);
        self.emit_expr(root, source.as_bytes(), depth, order, arg);
        self.field_macro_codes = previous;
        self.field_macro_piece = previous_piece;
        self.leave_type_tree(previous_types);
        true
    }

    fn emit_typedef_cast(
        &mut self,
        node: Node,
        bytes: &[u8],
        ty: String,
        depth: usize,
        order: i64,
        arg: Option<i64>,
    ) {
        self.note_call("<operator>.cast", 2);
        self.line(
            depth,
            "CALL",
            P {
                name: Some("<operator>.cast".into()),
                code: Some(self.expression_code(node, bytes)),
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
                code: Some(ty.clone()),
                tfn: Some(ty),
                order: Some(1),
                arg: Some(1),
                ..Default::default()
            },
        );
        if let Some(args) = node.child_by_field_name("arguments") {
            let values: Vec<_> = named_children(args)
                .into_iter()
                .filter(|child| child.kind() != "comment")
                .collect();
            if values.len() == 1 {
                self.emit_expr(values[0], bytes, depth + 1, 2, Some(2));
            } else {
                self.line(
                    depth + 1,
                    "BLOCK",
                    P {
                        tfn: Some("ANY".into()),
                        order: Some(2),
                        arg: Some(2),
                        ..Default::default()
                    },
                );
                for (i, value) in values.into_iter().enumerate() {
                    self.emit_expr(value, bytes, depth + 2, (i + 1) as i64, None);
                }
            }
        }
    }

    // Macro expansion CODE uses CDT's rendered type descriptor, while type
    // identity still comes from the original parsed cast (not its display text).
    fn expression_code(&self, node: Node, bytes: &[u8]) -> String {
        if let Some(code) = self.field_macro_codes.get(&node.id()) {
            return esc(code);
        }
        if self.macro_expansion_code.is_none() {
            return esc(text(node, bytes));
        }
        let mut pending = vec![node];
        let mut replacements = Vec::new();
        while let Some(current) = pending.pop() {
            let descriptor = (current.kind() == "cast_expression")
                .then(|| current.child_by_field_name("type"))
                .flatten();
            if let Some(desc) = descriptor {
                replacements.push((desc.byte_range(), expanded_cast_descriptor(desc, bytes)));
            }
            // A descriptor is rendered as a whole. Its array bounds can contain
            // casts of their own, whose byte ranges must not be replaced twice.
            pending.extend(
                named_children(current)
                    .into_iter()
                    .filter(|child| Some(*child) != descriptor),
            );
        }
        replacements.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        let mut code = text(node, bytes).to_string();
        for (range, replacement) in replacements {
            code.replace_range(
                range.start - node.start_byte()..range.end - node.start_byte(),
                &replacement,
            );
        }
        esc(&code)
    }

    /// Emit an expression node with the given ORDER and optional ARGUMENT_INDEX.
    fn emit_expr(&mut self, n: Node, b: &[u8], depth: usize, order: i64, arg: Option<i64>) {
        let previous = self
            .source_macros_at(n, b)
            .cloned()
            .map(|macros| std::mem::replace(&mut self.macros, macros));
        self.emit_expr_in_context(n, b, depth, order, arg);
        if let Some(previous) = previous {
            self.macros = previous;
        }
    }

    fn emit_expr_in_context(
        &mut self,
        n: Node,
        b: &[u8],
        depth: usize,
        order: i64,
        arg: Option<i64>,
    ) {
        if self
            .field_macro_piece
            .as_ref()
            .is_some_and(|(node, _)| *node == n.id())
        {
            let (_, name) = self.field_macro_piece.take().unwrap();
            self.emit_macro_call(&name, &name, &[], n, depth, order, arg, Some(text(n, b)));
            return;
        }
        if n.kind() == "field_expression" && self.emit_direct_field_macro(n, b, depth, order, arg) {
            return;
        }
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
                        code: Some(self.expression_code(n, b)),
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
                        code: Some(self.expression_code(n, b)),
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
                        code: Some(self.expression_code(n, b)),
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
                        code: Some(self.expression_code(n, b)),
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
                        code: Some(self.expression_code(n, b)),
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
                        code: Some(self.expression_code(n, b)),
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
                        code: Some(self.expression_code(n, b)),
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
                if let Some(ty) = self.typedef_cast(n, b) {
                    self.emit_typedef_cast(n, b, ty, depth, order, arg);
                    return;
                }
                let callee = n.child_by_field_name("function");
                let name = callee
                    .map(|c| text(c, b).to_string())
                    .unwrap_or("<anon>".into());
                let args = n.child_by_field_name("arguments");
                let argc = args.map(|a| named_children(a).len()).unwrap_or(0);
                if self.macros.get(&name).is_some_and(|m| m.params.is_some()) {
                    let arg_texts = args
                        .and_then(|args| macro_arguments(text(args, b), 0))
                        .map(|(args, _)| args)
                        .unwrap_or_default();
                    let code = self.expression_code(n, b);
                    self.emit_macro_call(&name, &code, &arg_texts, n, depth, order, arg, None);
                    return;
                }
                if callee.is_some_and(|callee| callee.kind() != "identifier")
                    || (!self.recovering_expression
                        && (self.symbols.contains_key(&name) || self.globals.contains_key(&name)))
                {
                    // Call through a pointer-valued symbol: <operator>.pointerCall,
                    // DYNAMIC_DISPATCH, receiver at ORDER=1 with no
                    // ARGUMENT_INDEX, args shifted to ORDER=2.. / INDEX=1..
                    self.note_call("<operator>.pointerCall", argc);
                    let receiver = callee.map(unwrap_paren);
                    let dereferenced = receiver.is_some_and(|node| node.kind() != "identifier");
                    let receiver_name = receiver
                        .and_then(|node| callable_identifier(node, b))
                        .map(|node| text(node, b))
                        .unwrap_or(&name);
                    let ty = self
                        .symbol_call_types
                        .get(receiver_name)
                        .or_else(|| {
                            self.symbols
                                .get(receiver_name)
                                .or_else(|| self.globals.get(receiver_name))
                                // Unwrapping `*` is valid for known function
                                // pointers; it must not infer a callable type
                                // from an arbitrary object or arithmetic node.
                                .filter(|ty| !dereferenced || ty.contains('('))
                        })
                        .or_else(|| self.method_call_types.get(receiver_name))
                        .or_else(|| self.function_call_types.get(receiver_name))
                        .map(|ty| ty.split('(').next().unwrap_or(ty).to_string())
                        .unwrap_or_else(|| "ANY".into());
                    self.line(
                        depth,
                        "CALL",
                        P {
                            name: Some("<operator>.pointerCall".into()),
                            code: Some(self.expression_code(n, b)),
                            tfn: Some(ty),
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
                    let ty = if self.recovering_expression {
                        "ANY".into()
                    } else {
                        self.method_call_types
                            .get(&name)
                            .or_else(|| self.function_call_types.get(&name))
                            .cloned()
                            .unwrap_or("ANY".into())
                    };
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
                            code: Some(self.expression_code(n, b)),
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
                if self.macros.get(&name).is_some_and(|m| m.params.is_none()) {
                    self.emit_macro_call(&name, &name, &[], n, depth, order, arg, None);
                    return;
                }
                if !self.recovering_expression
                    && !self.symbols.contains_key(&name)
                    && !self.globals.contains_key(&name)
                {
                    if let Some(ret) = self
                        .method_functions
                        .get(&name)
                        .or_else(|| self.functions.get(&name))
                        .cloned()
                    {
                        let full = self
                            .function_full_names
                            .get(&name)
                            .cloned()
                            .unwrap_or_else(|| name.clone());
                        self.line(
                            depth,
                            "METHOD_REF",
                            P {
                                code: Some(name),
                                tfn: Some(ret),
                                mfn: Some(full),
                                order: Some(order),
                                arg,
                                ..Default::default()
                            },
                        );
                        return;
                    }
                }
                let (code, ty) = if let Some(t) = self.symbols.get(&name) {
                    let code =
                        if self.recovering_expression || self.recovered_bindings.contains(&name) {
                            format!("<unknown> {name}")
                        } else {
                            name.clone()
                        };
                    (code, t.clone())
                } else if let Some(t) = self.globals.get(&name) {
                    (
                        format!(
                            "{} {name}",
                            if self.recovering_expression {
                                "<unknown>"
                            } else {
                                "<global>"
                            }
                        ),
                        t.clone(),
                    )
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
                            tfn: Some(numeric_literal_type(rest).into()),
                            order: Some(1),
                            arg: Some(1),
                            ..Default::default()
                        },
                    );
                } else {
                    let tfn = numeric_literal_type(&t);
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
            // CDT represents adjacent string tokens as one literal, retaining
            // the original spelling (including intervening comments/macros).
            "string_literal" | "concatenated_string" => {
                self.line(
                    depth,
                    "LITERAL",
                    P {
                        code: Some(self.expression_code(n, b)),
                        tfn: Some("char*".into()),
                        order: Some(order),
                        arg,
                        ..Default::default()
                    },
                );
            }
            "initializer_list" | "subscript_range_designator" => {
                let elements: Vec<_> = named_children(n)
                    .into_iter()
                    .filter(|child| child.kind() != "comment")
                    .collect();
                self.note_call("<operator>.arrayInitializer", elements.len());
                self.line(
                    depth,
                    "CALL",
                    P {
                        name: Some("<operator>.arrayInitializer".into()),
                        code: Some(esc(self
                            .macro_expansion_code
                            .as_deref()
                            .unwrap_or(text(n, b)))),
                        tfn: Some("ANY".into()),
                        mfn: Some("<operator>.arrayInitializer".into()),
                        order: Some(order),
                        arg,
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                for (index, element) in elements.into_iter().enumerate() {
                    let position = (index + 1) as i64;
                    self.emit_expr(element, b, depth + 1, position, Some(position));
                }
            }
            "initializer_pair" => {
                // CDT groups array designators in a synthetic block. Nested
                // indices each assign the value independently in source order.
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
                let mut cursor = n.walk();
                let designators: Vec<_> = n
                    .children_by_field_name("designator", &mut cursor)
                    .collect();
                if let Some(value) = n.child_by_field_name("value") {
                    for (index, designator) in designators.into_iter().enumerate() {
                        let target = match designator.kind() {
                            "subscript_designator" => designator.named_child(0),
                            "subscript_range_designator" => Some(designator),
                            _ => None,
                        };
                        let Some(target) = target else {
                            continue;
                        };
                        let position = (index + 1) as i64;
                        self.note_call("<operator>.assignment", 2);
                        self.line(
                            depth + 1,
                            "CALL",
                            P {
                                name: Some("<operator>.assignment".into()),
                                code: Some(self.expression_code(n, b)),
                                tfn: Some("void".into()),
                                mfn: Some("<operator>.assignment".into()),
                                order: Some(position),
                                arg: Some(position),
                                dispatch: Some("STATIC_DISPATCH".into()),
                                ..Default::default()
                            },
                        );
                        self.emit_expr(target, b, depth + 2, 1, Some(1));
                        self.emit_expr(value, b, depth + 2, 2, Some(2));
                    }
                }
            }
            "cast_expression" | "compound_literal_expression" => {
                // `(T)e` → <operator>.cast. CDT quirk: the type is the BASE
                // type only — `(char *)x` types as `char` — while the
                // TYPE_REF CODE keeps the raw descriptor text (`char *`).
                let desc = n.child_by_field_name("type");
                let raw = desc.map(|t| text(t, b).to_string()).unwrap_or("ANY".into());
                let ty = desc
                    .and_then(|t| t.child_by_field_name("type"))
                    .map(|t| {
                        if self.macro_expansion_code.is_some() {
                            primitive_type(text(t, b), TypeRole::Declaration)
                                .unwrap_or_else(|| normalize_type(text(t, b)))
                        } else {
                            normalize_type(text(t, b))
                        }
                    })
                    .unwrap_or_else(|| normalize_type(&raw));
                self.note_call("<operator>.cast", 2);
                self.line(
                    depth,
                    "CALL",
                    P {
                        name: Some("<operator>.cast".into()),
                        code: Some(self.expression_code(n, b)),
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
                        code: Some(if self.macro_expansion_code.is_some() {
                            desc.map(|desc| esc(&expanded_cast_descriptor(desc, b)))
                                .unwrap_or_else(|| esc(&raw))
                        } else {
                            esc(&raw)
                        }),
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
            "offsetof_expression" => {
                // With no external system-header expansion, CDT retains
                // offsetof as a call; tree-sitter gives it a distinct node.
                self.note_call("offsetof", 2);
                self.line(
                    depth,
                    "CALL",
                    P {
                        name: Some("offsetof".into()),
                        code: Some(self.expression_code(n, b)),
                        tfn: Some("ANY".into()),
                        mfn: Some("offsetof".into()),
                        order: Some(order),
                        arg,
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                if let Some(ty) = n.child_by_field_name("type") {
                    self.line(
                        depth + 1,
                        "IDENTIFIER",
                        P {
                            name: Some(text(ty, b).into()),
                            code: Some(text(ty, b).into()),
                            tfn: Some("ANY".into()),
                            order: Some(1),
                            arg: Some(1),
                            ..Default::default()
                        },
                    );
                }
                if let Some(member) = n.child_by_field_name("member") {
                    let name = text(member, b);
                    self.line(
                        depth + 1,
                        "IDENTIFIER",
                        P {
                            name: Some(name.into()),
                            code: Some(format!("<unknown> {name}")),
                            tfn: Some("ANY".into()),
                            order: Some(2),
                            arg: Some(2),
                            ..Default::default()
                        },
                    );
                }
            }
            "sizeof_expression" => {
                self.note_call("<operator>.sizeOf", 1);
                self.line(
                    depth,
                    "CALL",
                    P {
                        name: Some("<operator>.sizeOf".into()),
                        code: Some(self.expression_code(n, b)),
                        tfn: Some("ANY".into()),
                        mfn: Some("<operator>.sizeOf".into()),
                        order: Some(order),
                        arg,
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    },
                );
                if let Some(identifier) = self.sizeof_identifier(n, b) {
                    // Type-form sizeof shares its typed phantom identity.
                    self.line(
                        depth + 1,
                        "IDENTIFIER",
                        P {
                            name: Some(identifier.name),
                            code: Some(identifier.code),
                            tfn: Some(identifier.ty),
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
    variadic: bool,
    // Function-pointer parameters have a declaration type that omits the
    // pointer signature, but indirect calls still use the resolved return type.
    call_type: Option<String>,
}

type FunctionHeader = (String, String, Vec<Param>);

impl Param {
    fn signature_type(&self) -> &str {
        if self.variadic {
            "..."
        } else {
            &self.ty
        }
    }
}

fn prototype_headers(declaration: Node, b: &[u8]) -> Vec<FunctionHeader> {
    prototype_header_entries(declaration, b)
        .into_iter()
        .map(|(_, header)| header)
        .collect()
}

fn macro_declaration_return(declaration: Node) -> Option<Node> {
    let mut cursor = declaration.walk();
    let result = declaration
        .children_by_field_name("declarator", &mut cursor)
        .find_map(|declarator| parenthesized_function_parts(declarator).and_then(|(_, ret)| ret));
    result
}

fn prototype_header_entries<'tree>(
    declaration: Node<'tree>,
    b: &[u8],
) -> Vec<(Node<'tree>, FunctionHeader)> {
    if declaration.kind() != "declaration" {
        return Vec::new();
    }
    let mut cursor = declaration.walk();
    declaration
        .children_by_field_name("declarator", &mut cursor)
        .filter(|&decl| is_function_declaration(decl))
        .filter_map(|decl| fn_header_declarator(declaration, decl, b).map(|header| (decl, header)))
        .collect()
}

/// Execute available quoted headers in the importing translation unit's macro
/// environment. Include guards and undef/redefinitions are evaluated at each
/// inclusion; declarations retain immutable snapshots of the preceding state.
struct BodyMacroContext<'tree> {
    items: Vec<Node<'tree>>,
    macros: MacroState,
    macro_states: HashMap<usize, MacroState>,
    body_macro_sites: BodyMacroSites,
    header_declarations: Vec<HeaderDeclaration>,
    typedef_states: HashMap<usize, TypeNameState>,
}

struct HeaderDeclaration {
    header: FunctionHeader,
    call_type: String,
}

fn body_macro_context<'tree>(
    root: Node<'tree>,
    bytes: &[u8],
    file: &str,
    units: &[SourceUnit],
) -> BodyMacroContext<'tree> {
    // CDT retains file-scope declarations from inactive branches. They use
    // the macro environment at that source position, while inactive directives
    // and includes must not change the importing translation unit's state.
    fn retain_inactive_declaration_context(
        node: Node,
        bytes: &[u8],
        macros: &MacroState,
        typedefs: &TypeNameState,
        states: &mut HashMap<usize, MacroState>,
        typedef_states: &mut HashMap<usize, TypeNameState>,
    ) {
        match node.kind() {
            "declaration"
            | "type_definition"
            | "struct_specifier"
            | "union_specifier"
            | "enum_specifier"
            | "function_definition" => {
                // The declaration's header still expands in the surrounding
                // context. It does not make an inactive function body active.
                states.insert(node.id(), macros.clone());
                typedef_states.insert(node.id(), typedefs.clone());
            }
            "preproc_if" | "preproc_ifdef" | "preproc_elif" | "preproc_elifdef"
            | "preproc_else" => {
                for child in translation_unit_children(node, bytes) {
                    retain_inactive_declaration_context(
                        child,
                        bytes,
                        macros,
                        typedefs,
                        states,
                        typedef_states,
                    );
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn collect<'tree>(
        node: Node<'tree>,
        bytes: &[u8],
        file: &str,
        units: &[SourceUnit],
        visiting: &mut HashSet<String>,
        once: &mut HashSet<String>,
        macros: &mut MacroState,
        items: &mut Vec<Node<'tree>>,
        states: &mut HashMap<usize, MacroState>,
        header_declarations: &mut Vec<HeaderDeclaration>,
        typedefs: &mut TypeNameState,
        typedef_states: &mut HashMap<usize, TypeNameState>,
        type_bindings: &mut HashMap<String, String>,
        capture: bool,
        in_body: bool,
        body_sites: &mut BodyMacroSites,
        in_expansion: bool,
    ) {
        match node.kind() {
            "preproc_if" | "preproc_ifdef" | "preproc_elif" | "preproc_elifdef"
            | "preproc_else" => {
                if let Some(condition) = node.child_by_field_name("condition") {
                    collect_macro_expansions(condition, bytes, macros, body_sites);
                }
                let take = if matches!(node.kind(), "preproc_if" | "preproc_elif") {
                    let definitions = macros
                        .iter()
                        .map(|(name, definition)| {
                            (
                                name.clone(),
                                if definition.params.is_some() {
                                    name.clone()
                                } else {
                                    definition.body.clone()
                                },
                            )
                        })
                        .collect();
                    preproc_condition(node, bytes, &definitions)
                } else if let Some(name) = node.child_by_field_name("name") {
                    let negated = node.child(0).is_some_and(|directive| {
                        matches!(directive.kind(), "#ifndef" | "#elifndef")
                    });
                    macros.contains_key(text(name, bytes)) != negated
                } else {
                    true
                };
                let alternative = node.child_by_field_name("alternative");
                if take {
                    for child in translation_unit_children(node, bytes) {
                        if Some(child) != node.child_by_field_name("condition")
                            && Some(child) != node.child_by_field_name("name")
                            && Some(child) != alternative
                        {
                            collect(
                                child,
                                bytes,
                                file,
                                units,
                                visiting,
                                once,
                                macros,
                                items,
                                states,
                                header_declarations,
                                typedefs,
                                typedef_states,
                                type_bindings,
                                capture,
                                in_body,
                                body_sites,
                                in_expansion,
                            );
                        }
                    }
                    if capture && !in_body {
                        if let Some(alternative) = alternative {
                            retain_inactive_declaration_context(
                                alternative,
                                bytes,
                                macros,
                                typedefs,
                                states,
                                typedef_states,
                            );
                        }
                    }
                } else {
                    if capture && !in_body {
                        for child in translation_unit_children(node, bytes) {
                            if Some(child) != alternative {
                                retain_inactive_declaration_context(
                                    child,
                                    bytes,
                                    macros,
                                    typedefs,
                                    states,
                                    typedef_states,
                                );
                            }
                        }
                    }
                    if let Some(alternative) = alternative {
                        collect(
                            alternative,
                            bytes,
                            file,
                            units,
                            visiting,
                            once,
                            macros,
                            items,
                            states,
                            header_declarations,
                            typedefs,
                            typedef_states,
                            type_bindings,
                            capture,
                            in_body,
                            body_sites,
                            in_expansion,
                        );
                    }
                }
            }
            _ => {
                let outer_expansion = !in_expansion
                    && node
                        .child_by_field_name("function")
                        .is_some_and(|function| macros.contains_key(text(function, bytes)));
                if !in_expansion {
                    if !in_body && node.kind() == "function_definition" {
                        for child in named_children(node) {
                            if Some(child) != node.child_by_field_name("body") {
                                collect_macro_expansions(child, bytes, macros, body_sites);
                            }
                        }
                    } else if (!in_body && !node.kind().starts_with("preproc_"))
                        || outer_expansion
                        || (in_body && node.named_child_count() == 0)
                    {
                        collect_macro_expansions(node, bytes, macros, body_sites);
                    }
                }
                if capture && !in_body {
                    if matches!(
                        node.kind(),
                        "function_definition"
                            | "declaration"
                            | "type_definition"
                            | "struct_specifier"
                            | "union_specifier"
                            | "enum_specifier"
                    ) {
                        states.insert(node.id(), macros.clone());
                        typedef_states.insert(node.id(), typedefs.clone());
                    }
                    items.push(node);
                }
                if !in_body {
                    update_header_type_bindings(node, bytes, macros, type_bindings);
                }
                if !capture && !in_body {
                    header_declarations.extend(supplied_header_bindings(
                        node,
                        bytes,
                        macros,
                        type_bindings,
                    ));
                }
                if !in_body && node.kind() == "type_definition" {
                    for name in valid_type_definition_names(node, bytes, macros) {
                        Arc::make_mut(typedefs).insert(name);
                    }
                }
                if let Some(include) = included_source_name(node, bytes, file) {
                    if !once.contains(&include) && visiting.insert(include.clone()) {
                        if let Some(unit) = units.iter().find(|unit| {
                            normalize_source_path(std::path::Path::new(&unit.file)) == include
                        }) {
                            let mut header_items = Vec::new();
                            let mut header_states = HashMap::new();
                            for child in translation_unit_children(
                                unit.tree.root_node(),
                                unit.src.as_bytes(),
                            ) {
                                collect(
                                    child,
                                    unit.src.as_bytes(),
                                    &unit.file,
                                    units,
                                    visiting,
                                    once,
                                    macros,
                                    &mut header_items,
                                    &mut header_states,
                                    header_declarations,
                                    typedefs,
                                    typedef_states,
                                    type_bindings,
                                    false,
                                    in_body,
                                    body_sites,
                                    in_expansion,
                                );
                            }
                        }
                        visiting.remove(&include);
                    }
                } else if node.kind() == "preproc_call"
                    && node
                        .child_by_field_name("directive")
                        .is_some_and(|d| text(d, bytes).trim() == "#pragma")
                    && node
                        .child_by_field_name("argument")
                        .is_some_and(|a| text(a, bytes).trim() == "once")
                {
                    once.insert(normalize_source_path(std::path::Path::new(file)));
                } else if let Some((name, definition)) = source_macro_definition(node, bytes, file)
                {
                    Arc::make_mut(macros).insert(name, definition);
                } else if is_undef_directive(node, bytes) {
                    if let Some(name) = node.child_by_field_name("argument") {
                        Arc::make_mut(macros).remove(text(name, bytes).trim());
                    }
                } else if node.kind() == "function_definition" {
                    if let Some(body) = node.child_by_field_name("body") {
                        if capture && body_sites.owns(bytes) {
                            body_sites.bodies.push((body.start_byte(), body.end_byte()));
                            body_sites.record(bytes, body.start_byte(), macros);
                        }
                        collect(
                            body,
                            bytes,
                            file,
                            units,
                            visiting,
                            once,
                            macros,
                            items,
                            states,
                            header_declarations,
                            typedefs,
                            typedef_states,
                            type_bindings,
                            capture,
                            true,
                            body_sites,
                            in_expansion,
                        );
                    }
                } else if in_body {
                    for child in translation_unit_children(node, bytes) {
                        collect(
                            child,
                            bytes,
                            file,
                            units,
                            visiting,
                            once,
                            macros,
                            items,
                            states,
                            header_declarations,
                            typedefs,
                            typedef_states,
                            type_bindings,
                            capture,
                            true,
                            body_sites,
                            in_expansion || outer_expansion,
                        );
                    }
                }
                if in_body {
                    body_sites.record(bytes, node.end_byte(), macros);
                }
            }
        }
    }
    let mut macros = predefined_c_macros(file);
    let mut body_macro_sites = BodyMacroSites::new(root, bytes);
    let mut items = Vec::new();
    let mut states = HashMap::new();
    let mut header_declarations = Vec::new();
    let mut typedefs = TypeNameState::default();
    let mut typedef_states = HashMap::new();
    let mut type_bindings = HashMap::new();
    let mut once = HashSet::new();
    let mut visiting = HashSet::from([normalize_source_path(std::path::Path::new(file))]);
    for child in translation_unit_children(root, bytes) {
        collect(
            child,
            bytes,
            file,
            units,
            &mut visiting,
            &mut once,
            &mut macros,
            &mut items,
            &mut states,
            &mut header_declarations,
            &mut typedefs,
            &mut typedef_states,
            &mut type_bindings,
            true,
            false,
            &mut body_macro_sites,
            false,
        );
    }
    // CDT sorts file-local offsets alone; stable ties retain include traversal order.
    body_macro_sites
        .expansions
        .sort_by_key(|(offset, _)| *offset);
    BodyMacroContext {
        items,
        macros,
        macro_states: states,
        body_macro_sites,
        header_declarations,
        typedef_states,
    }
}

fn normalize_source_path(path: &std::path::Path) -> String {
    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if normalized.file_name().is_some_and(|name| name != "..") {
                    normalized.pop();
                } else if !normalized.has_root() {
                    normalized.push("..");
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized.to_string_lossy().into_owned()
}

fn included_source_name(node: Node, b: &[u8], importing_file: &str) -> Option<String> {
    if node.kind() != "preproc_include" {
        return None;
    }
    let path = text(node.child_by_field_name("path")?, b);
    // A quoted include has a defined relative search location. No synthetic
    // basename/include-path search or filesystem reads are introduced here.
    let path = path.strip_prefix('"')?.strip_suffix('"')?;
    let parent = std::path::Path::new(importing_file)
        .parent()
        .unwrap_or_else(|| std::path::Path::new(""));
    Some(normalize_source_path(&parent.join(path)))
}

fn source_macro_definition(node: Node, bytes: &[u8], file: &str) -> Option<(String, MacroDef)> {
    if !matches!(node.kind(), "preproc_def" | "preproc_function_def") {
        return None;
    }
    let line = preproc_logical_line(node, bytes);
    let payload = preproc_payload(&line);
    let end = payload
        .bytes()
        .position(|c| !(c.is_ascii_alphanumeric() || c == b'_'))
        .unwrap_or(payload.len());
    if end == 0 {
        return None;
    }
    let name = payload[..end].to_string();
    let tail = &payload[end..];
    let (params, body) = if let Some(tail) = tail.strip_prefix('(') {
        let close = tail.find(')')?;
        let formals = &tail[..close];
        let params = if formals.trim().is_empty() {
            Vec::new()
        } else {
            formals.split(',').map(|p| p.trim().to_string()).collect()
        };
        (Some(params), tail[close + 1..].trim().to_string())
    } else {
        (None, tail.trim().to_string())
    };
    Some((
        name,
        MacroDef {
            params,
            body,
            directive: preproc_raw_directive(node, bytes).to_string(),
            file: file.to_string(),
        },
    ))
}

/// The directive token permits whitespace between `#` and `undef`.
/// Keep recognition shared by the include walk and local source-effect tables.
fn is_undef_directive(node: Node, bytes: &[u8]) -> bool {
    node.kind() == "preproc_call"
        && node
            .child_by_field_name("directive")
            .is_some_and(|directive| {
                text(directive, bytes)
                    .trim()
                    .strip_prefix('#')
                    .is_some_and(|name| name.trim() == "undef")
            })
}

/// Typedef alias declarations register the resolved underlying expression type;
/// this is separate from the alias spelling retained on cast TYPE_REF nodes.
fn typedef_underlying_type(node: Node, alias: Node, bytes: &[u8]) -> String {
    format!(
        "{}{}",
        declaration_type(node, bytes, TypeRole::Expression),
        decl_suffix(alias, bytes)
    )
}

fn resolved_typedef_underlying_type(
    node: Node,
    alias: Node,
    bytes: &[u8],
    macros: &MacroState,
) -> String {
    let raw = text(node, bytes);
    if let Some(expanded) = expand_declaration_tokens(raw, macros, &mut HashSet::new(), &mut 65_536)
    {
        if expanded != raw {
            let mut parser = Parser::new();
            parser
                .set_language(&tree_sitter_c::LANGUAGE.into())
                .unwrap();
            if let Some(tree) = parser.parse(&expanded, None) {
                if let Some(declaration) = named_children(tree.root_node())
                    .into_iter()
                    .find(|node| node.kind() == "type_definition" && !node.has_error())
                {
                    let name = type_binding_name(alias, bytes);
                    if let Some(binding) = typedef_declarators(declaration)
                        .into_iter()
                        .find(|binding| type_binding_name(*binding, expanded.as_bytes()) == name)
                    {
                        return typedef_underlying_type(declaration, binding, expanded.as_bytes());
                    }
                }
            }
        }
    }
    typedef_underlying_type(node, alias, bytes)
}

fn typedef_declarators(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.children_by_field_name("declarator", &mut cursor)
        .filter(|&declarator| find_function_declarator(declarator).is_none())
        .collect()
}

/// Parentheses around a function name do not make it a pointer object.
/// A macro prefix such as `API int (f)(int)` is parsed as two nested function
/// declarators: `int(f)` followed by the real parameter list.
fn parenthesized_function_parts(decl: Node) -> Option<(Node, Option<Node>)> {
    let fd = find_function_declarator(decl)?;
    let name = fd.child_by_field_name("declarator")?;
    if name.kind() == "parenthesized_declarator" {
        let mut inner = name;
        while inner.kind() == "parenthesized_declarator" {
            inner = inner.named_child(0)?;
        }
        return (inner.kind() == "identifier").then_some((inner, None));
    }
    if name.kind() == "function_declarator" {
        let ret = name.child_by_field_name("declarator")?;
        let parameters = name.child_by_field_name("parameters")?;
        let parameter = parameters.named_child(0)?;
        if ret.kind() == "identifier"
            && parameters.named_child_count() == 1
            && parameter.kind() == "parameter_declaration"
            && parameter.child_by_field_name("declarator").is_none()
        {
            let real_name = parameter.child_by_field_name("type")?;
            return Some((real_name, Some(ret)));
        }
    }
    None
}

fn is_function_declaration(decl: Node) -> bool {
    // A pointer inside the parentheses still declares an object.
    parenthesized_function_parts(decl).is_some()
        || find_function_declarator(decl)
            .and_then(|fd| fd.child_by_field_name("declarator"))
            .is_some_and(|name| name.kind() == "identifier")
}

fn declared_object_type(declaration: Node, decl: Node, b: &[u8]) -> String {
    let base = declaration_type(declaration, b, TypeRole::Declaration);
    if let Some(fd) = find_function_declarator(decl) {
        if let Some(pointer) = fd.child_by_field_name("declarator") {
            if pointer.kind() == "parenthesized_declarator" {
                let name = innermost_id(pointer, b);
                let shape: String = text(pointer, b)
                    .replacen(&name, "", 1)
                    .chars()
                    .filter(|ch| !ch.is_whitespace())
                    .collect();
                let params = fd
                    .child_by_field_name("parameters")
                    .map(|params| {
                        named_children(params)
                            .into_iter()
                            .filter_map(|param| {
                                if param.kind() == "variadic_parameter" {
                                    return Some("...".to_string());
                                }
                                if param.kind() != "parameter_declaration" {
                                    return None;
                                }
                                let base = declaration_type(param, b, TypeRole::Expression);
                                Some(format!(
                                    "{base}{}",
                                    param
                                        .child_by_field_name("declarator")
                                        .map(|d| decl_suffix(d, b))
                                        .unwrap_or_default()
                                ))
                            })
                            .collect::<Vec<_>>()
                            .join(",")
                            .replace(' ', "")
                    })
                    .unwrap_or_default();
                let base = declaration_type(declaration, b, TypeRole::Expression);
                return format!("{base}{shape}({params})");
            }
        }
    }
    format!("{base}{}", object_decl_suffix(decl, b, false))
}

fn prototype_declarations<'a>(root: Node<'a>, b: &[u8]) -> Vec<Node<'a>> {
    if !prototype_headers(root, b).is_empty() {
        return vec![root];
    }
    named_children(root)
        .into_iter()
        .flat_map(|child| prototype_declarations(child, b))
        .collect()
}

/// (name, return type, params) for a function_definition.
pub(crate) fn function_name(f: Node, b: &[u8]) -> Option<String> {
    fn_header(f, b).map(|(name, _, _)| name)
}

struct ResolvedFunctionHeader {
    header: FunctionHeader,
    call_type: String,
    expanded_code: Option<String>,
}

enum HeaderReturnBinding {
    Known(String),
    Alias(String, String),
}

impl HeaderReturnBinding {
    fn resolve(self, bindings: &HashMap<String, String>) -> String {
        match self {
            Self::Known(ty) => ty,
            Self::Alias(name, suffix) => match bindings.get(&name) {
                Some(base) if base != "ANY" => format!("{base}{suffix}"),
                _ => "ANY".into(),
            },
        }
    }
}

fn header_return_binding(f: Node, decl: Node, bytes: &[u8]) -> HeaderReturnBinding {
    let base = parenthesized_function_parts(decl)
        .and_then(|(_, ret)| ret)
        .or_else(|| macro_declaration_return(f))
        .or_else(|| f.child_by_field_name("type"));
    let call_type = function_return_type(f, decl, bytes, TypeRole::Expression);
    if base.is_some_and(|base| {
        primitive_type(text(base, bytes), TypeRole::Expression).is_some()
            || matches!(
                base.kind(),
                "struct_specifier" | "union_specifier" | "enum_specifier"
            )
    }) {
        return HeaderReturnBinding::Known(call_type);
    }
    let name = base
        .map(|base| normalize_type(text(base, bytes)))
        .unwrap_or_else(|| "ANY".into());
    let suffix = call_type.strip_prefix(&name).unwrap_or("").to_string();
    HeaderReturnBinding::Alias(name, suffix)
}

// The raw parser may recover `API T f(...)` as a function named T with an
// ERROR containing f. Included-header lookup must use the complete expanded
// declaration, never that recovery name or its fallback return spelling.
fn supplied_header_bindings(
    node: Node,
    bytes: &[u8],
    macros: &HashMap<String, MacroDef>,
    bindings: &HashMap<String, String>,
) -> Vec<HeaderDeclaration> {
    if node.kind() != "declaration" {
        return Vec::new();
    }
    let Some(source) =
        expand_declaration_tokens(text(node, bytes), macros, &mut HashSet::new(), &mut 65_536)
    else {
        return Vec::new();
    };
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .unwrap();
    let Some(tree) = parser.parse(&source, None) else {
        return Vec::new();
    };
    let root = tree.root_node();
    let declarations: Vec<_> = named_children(root)
        .into_iter()
        .filter(|node| node.kind() != "comment")
        .collect();
    if root.has_error() || declarations.len() != 1 || declarations[0].kind() != "declaration" {
        return Vec::new();
    }
    let declaration = declarations[0];
    prototype_header_entries(declaration, source.as_bytes())
        .into_iter()
        // This nested-function recovery represents an unexpanded type prefix,
        // not a valid C function declarator after preprocessing.
        .filter(|(decl, _)| {
            !parenthesized_function_parts(*decl).is_some_and(|(_, ret)| ret.is_some())
        })
        .map(|(decl, header)| HeaderDeclaration {
            call_type: header_return_binding(declaration, decl, source.as_bytes())
                .resolve(bindings),
            header,
        })
        .collect()
}

// Callable binding types follow typedef targets, whereas METHOD/PARAMETER
// declaration spellings retain their aliases. A syntactically recognized
// typedef may still have an unresolved underlying binding, which yields ANY.
fn update_header_type_bindings(
    node: Node,
    bytes: &[u8],
    macros: &HashMap<String, MacroDef>,
    bindings: &mut HashMap<String, String>,
) {
    if node.kind() != "type_definition" {
        return;
    }
    let Some(source) =
        expand_declaration_tokens(text(node, bytes), macros, &mut HashSet::new(), &mut 65_536)
    else {
        return;
    };
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .unwrap();
    let Some(tree) = parser.parse(&source, None) else {
        return;
    };
    let Some(declaration) = named_children(tree.root_node())
        .into_iter()
        .find(|node| node.kind() == "type_definition" && !node.has_error())
    else {
        return;
    };
    let Some(base) = declaration.child_by_field_name("type") else {
        return;
    };
    // CDT rejects a primitive modifier applied to a typedef/unknown name.
    // Tree-sitter accepts this shape, but it must not replace an earlier
    // valid binding (for example `typedef int T; typedef unsigned UNKNOWN T`).
    if base.kind() == "sized_type_specifier"
        && named_children(base)
            .iter()
            .any(|child| child.kind() == "type_identifier")
    {
        return;
    }
    let raw = text(base, source.as_bytes());
    let base_type = if primitive_type(raw, TypeRole::Expression).is_some()
        || matches!(
            base.kind(),
            "struct_specifier" | "union_specifier" | "enum_specifier"
        ) {
        declaration_type(declaration, source.as_bytes(), TypeRole::Expression)
    } else {
        bindings.get(raw).cloned().unwrap_or_else(|| "ANY".into())
    };
    let mut cursor = declaration.walk();
    for declarator in declaration.children_by_field_name("declarator", &mut cursor) {
        let mut leaf = declarator;
        while let Some(inner) = leaf.child_by_field_name("declarator") {
            leaf = inner;
        }
        let name = if matches!(leaf.kind(), "identifier" | "type_identifier") {
            text(leaf, source.as_bytes()).to_string()
        } else {
            String::new()
        };
        if name.is_empty() {
            continue;
        }
        let ty = if base_type == "ANY" || find_function_declarator(declarator).is_some() {
            "ANY".to_string()
        } else {
            format!(
                "{base_type}{}",
                object_decl_suffix(declarator, source.as_bytes(), false)
            )
        };
        bindings.insert(name, ty);
    }
}

/// Expand declaration tokens without expression rendering. In particular a
/// type-valued replacement must not first be interpreted as a C call/cast.
fn expand_declaration_tokens(
    source: &str,
    macros: &HashMap<String, MacroDef>,
    disabled: &mut HashSet<String>,
    budget: &mut usize,
) -> Option<String> {
    let bytes = source.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if *budget == 0 {
            return None;
        }
        let start = i;
        if let Some(end) = macro_opaque_end(bytes, i) {
            i = end;
        } else if bytes[i].is_ascii_digit()
            || (bytes[i] == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'.'
                    || (matches!(bytes[i], b'+' | b'-')
                        && matches!(bytes[i - 1], b'e' | b'E' | b'p' | b'P'))
                {
                    i += 1;
                } else {
                    let next = macro_identifier_unit(bytes, i, false);
                    if next == 0 {
                        break;
                    }
                    i += next;
                }
            }
        } else if macro_identifier_unit(bytes, i, true) != 0 {
            i += macro_identifier_unit(bytes, i, true);
            while i < bytes.len() {
                let next = macro_identifier_unit(bytes, i, false);
                if next == 0 {
                    break;
                }
                i += next;
            }
            let name = &source[start..i];
            if disabled.len() < 64 && !disabled.contains(name) {
                if let Some(definition) = macros.get(name) {
                    let replacement = if let Some(params) = &definition.params {
                        let mut open = i;
                        while open < bytes.len() {
                            if bytes[open].is_ascii_whitespace() {
                                open += 1;
                            } else if bytes[open..].starts_with(b"/*")
                                || bytes[open..].starts_with(b"//")
                            {
                                open = macro_opaque_end(bytes, open)?;
                            } else {
                                break;
                            }
                        }
                        if bytes.get(open) != Some(&b'(') {
                            None
                        } else if let Some((args, end)) = macro_arguments(source, open) {
                            i = end + 1;
                            Some(substitute(&definition.body, params, &args))
                        } else {
                            None
                        }
                    } else {
                        Some(definition.body.clone())
                    };
                    if let Some(replacement) = replacement {
                        *budget -= 1;
                        disabled.insert(name.to_string());
                        let replacement =
                            expand_declaration_tokens(&replacement, macros, disabled, budget);
                        disabled.remove(name);
                        out.push_str(&replacement?);
                        continue;
                    }
                }
            }
        } else {
            i += source[i..].chars().next()?.len_utf8();
        }
        let token = &source[start..i];
        *budget = budget.checked_sub(token.len())?;
        out.push_str(token);
    }
    Some(out)
}

fn expanded_declaration_specifier(node: Node, bytes: &[u8]) -> String {
    let Some(base) = node.child_by_field_name("type") else {
        return String::new();
    };
    let raw = text(base, bytes);
    let base_code = if raw == "unsigned long" {
        "long unsigned"
    } else {
        raw
    };
    let prefix = named_children(node)
        .into_iter()
        .filter(|child| {
            child.end_byte() <= base.start_byte()
                && matches!(child.kind(), "storage_class_specifier" | "type_qualifier")
        })
        .map(|child| text(child, bytes))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{prefix} {base_code}")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn expanded_declaration_declarator(node: Node, bytes: &[u8]) -> Option<String> {
    let inner = || {
        node.child_by_field_name("declarator")
            .map(|child| expanded_declaration_declarator(child, bytes))
            .unwrap_or_else(|| Some(String::new()))
    };
    match node.kind() {
        "identifier" => Some(String::new()),
        "pointer_declarator" | "abstract_pointer_declarator" => {
            let qualifiers = named_children(node)
                .into_iter()
                .filter(|child| child.kind() == "type_qualifier")
                .map(|child| text(child, bytes))
                .collect::<Vec<_>>()
                .join(" ");
            let nested = inner()?;
            Some(if qualifiers.is_empty() {
                format!("*{nested}")
            } else {
                format!("* {qualifiers}{nested}")
            })
        }
        "array_declarator" | "abstract_array_declarator" => Some(format!("{}[]", inner()?)),
        "function_declarator" => {
            let nested = inner()?;
            let parameters = node.child_by_field_name("parameters")?;
            let parameters = named_children(parameters)
                .into_iter()
                .filter(|param| {
                    matches!(param.kind(), "parameter_declaration" | "variadic_parameter")
                })
                .map(|param| {
                    if param.kind() == "variadic_parameter" {
                        return Some("...".to_string());
                    }
                    let code = expanded_declaration_specifier(param, bytes);
                    let suffix = param
                        .child_by_field_name("declarator")
                        .map(|decl| expanded_declaration_declarator(decl, bytes))
                        .unwrap_or_else(|| Some(String::new()))?;
                    Some(if suffix.is_empty() {
                        code
                    } else {
                        format!("{code} {suffix}")
                    })
                })
                .collect::<Option<Vec<_>>>()?
                .join(", ");
            Some(format!("{nested}({parameters})"))
        }
        _ => None,
    }
}

fn expanded_prototype_code(node: Node, bytes: &[u8]) -> Option<String> {
    let specifier = expanded_declaration_specifier(node, bytes);
    let mut cursor = node.walk();
    let declarations = node
        .children_by_field_name("declarator", &mut cursor)
        .map(|decl| {
            expanded_declaration_declarator(decl, bytes)
                .map(|declarator| format!("{specifier} {declarator}"))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(esc(&format!("{specifier} {};", declarations.join(" "))))
}

// CDT source ranges start after an empty leading specifier macro. A nonempty
// leading macro instead gives declarations a synthesized specifier/declarator
// spelling; a macro after an ordinary leading qualifier retains raw CODE.
fn declaration_code_start(source: &str, macros: &HashMap<String, MacroDef>) -> usize {
    let bytes = source.as_bytes();
    let mut start = 0;
    loop {
        let mut end = start;
        while end < bytes.len() {
            let size = macro_identifier_unit(bytes, end, end == start);
            if size == 0 {
                break;
            }
            end += size;
        }
        if end == start || !macros.contains_key(&source[start..end]) {
            return start;
        }
        if !expand_declaration_tokens(
            &source[start..end],
            macros,
            &mut HashSet::new(),
            &mut 65_536,
        )
        .is_some_and(|expanded| expanded.trim().is_empty())
        {
            return start;
        }
        start = end;
        while start < bytes.len() && bytes[start].is_ascii_whitespace() {
            start += 1;
        }
    }
}

pub(crate) struct EmptyMacroMethodSpan {
    pub code: String,
    pub start: usize,
}

/// Used lazily by location recovery only when a METHOD's CODE differs from its
/// parsed source. Verify omitted prefixes in the same include-aware, immutable
/// macro environment as lowering; ordinary suffix text is not an anchor.
pub(crate) fn empty_macro_method_spans(
    sources: &[(String, String)],
) -> HashMap<(String, usize), EmptyMacroMethodSpan> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .unwrap();
    let units: Vec<_> = sources
        .iter()
        .map(|(file, src)| SourceUnit {
            file: file.clone(),
            src: src.clone(),
            tree: parser.parse(src, None).unwrap(),
        })
        .collect();
    let mut spans = HashMap::new();
    for unit in &units {
        let bytes = unit.src.as_bytes();
        let context = body_macro_context(unit.tree.root_node(), bytes, &unit.file, &units);
        for node in translation_unit_items(unit.tree.root_node(), bytes) {
            if node.kind() != "function_definition" {
                continue;
            }
            let Some(macros) = context.macro_states.get(&node.id()) else {
                continue;
            };
            let source = text(node, bytes);
            let offset = declaration_code_start(source, macros);
            if offset > 0 {
                spans.insert(
                    (unit.file.clone(), node.start_byte()),
                    EmptyMacroMethodSpan {
                        code: source[offset..].replace("\\n", "\n").trim().to_string(),
                        start: node.start_byte() + offset,
                    },
                );
            }
        }
    }
    spans
}

fn resolved_function_header(
    f: Node,
    decl: Node,
    bytes: &[u8],
    macros: &HashMap<String, MacroDef>,
) -> Option<ResolvedFunctionHeader> {
    let original = fn_header_declarator(f, decl, bytes)?;
    let fallback = || ResolvedFunctionHeader {
        header: fn_header_declarator(f, decl, bytes).expect("original header"),
        call_type: function_return_type(f, decl, bytes, TypeRole::Expression),
        expanded_code: None,
    };
    let end = f
        .child_by_field_name("body")
        .map_or(f.end_byte(), |body| body.start_byte());
    let raw = std::str::from_utf8(&bytes[f.start_byte()..end]).ok()?;
    let Some(expanded) = expand_declaration_tokens(raw, macros, &mut HashSet::new(), &mut 65_536)
    else {
        return Some(fallback());
    };
    if expanded == raw {
        return Some(fallback());
    }
    let source = if f.kind() == "function_definition" {
        format!("{expanded}{{}}")
    } else {
        expanded
    };
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(&source, None)?;
    let node = named_children(tree.root_node())
        .into_iter()
        .find(|node| node.kind() == f.kind());
    let Some(node) = node.filter(|node| !node.has_error()) else {
        return Some(fallback());
    };
    let candidates = if node.kind() == "function_definition" {
        node.child_by_field_name("declarator")
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        let mut cursor = node.walk();
        node.children_by_field_name("declarator", &mut cursor)
            .collect()
    };
    let Some((expanded_decl, mut header)) = candidates
        .into_iter()
        .filter_map(|decl| {
            fn_header_declarator(node, decl, source.as_bytes()).map(|header| (decl, header))
        })
        .find(|(_, header)| header.0 == original.0)
    else {
        return Some(fallback());
    };
    if header.2.len() != original.2.len() {
        return Some(fallback());
    }
    for (parameter, original) in header.2.iter_mut().zip(original.2) {
        parameter.code = original.code;
    }
    let call_type =
        function_return_type(node, expanded_decl, source.as_bytes(), TypeRole::Expression);
    let code = text(f, bytes);
    let code_start = declaration_code_start(code, macros);
    let leading = &code[code_start..];
    let mut token_end = 0;
    while token_end < leading.len() {
        let size = macro_identifier_unit(leading.as_bytes(), token_end, token_end == 0);
        if size == 0 {
            break;
        }
        token_end += size;
    }
    let leading_macro = macros.contains_key(&leading[..token_end]);
    let expanded_code = if node.kind() == "declaration" && leading_macro {
        expanded_prototype_code(node, source.as_bytes())
    } else if code_start > 0 {
        Some(esc(leading))
    } else {
        None
    };
    Some(ResolvedFunctionHeader {
        header,
        call_type,
        expanded_code,
    })
}

fn fn_header(f: Node, b: &[u8]) -> Option<FunctionHeader> {
    fn_header_declarator(f, f.child_by_field_name("declarator")?, b)
}

fn function_return_type(f: Node, decl: Node, b: &[u8], role: TypeRole) -> String {
    let base_node = parenthesized_function_parts(decl)
        .and_then(|(_, ret)| ret)
        .or_else(|| macro_declaration_return(f))
        .or_else(|| f.child_by_field_name("type"));
    let base = specifier_type_for_role(f, base_node, b, role);
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
    format!("{base}{}", "*".repeat(stars))
}

fn fn_header_declarator(f: Node, decl: Node, b: &[u8]) -> Option<FunctionHeader> {
    let role = if f.kind() == "function_definition" {
        TypeRole::DefinitionReturn
    } else {
        TypeRole::Declaration
    };
    let ret = function_return_type(f, decl, b, role);
    let parenthesized = parenthesized_function_parts(decl);
    let fd = find_function_declarator(decl)?;
    let name = if let Some((name, _)) = parenthesized {
        text(name, b).to_string()
    } else {
        fd.child_by_field_name("declarator")
            .map(|d| innermost_id(d, b))?
    };
    let mut params = Vec::new();
    if let Some(pl) = fd.child_by_field_name("parameters") {
        for p in named_children(pl) {
            if p.kind() == "parameter_declaration" {
                let base = declaration_type(p, b, TypeRole::Declaration);
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
                    variadic: false,
                    call_type: decl
                        .filter(|decl| find_function_declarator(*decl).is_some())
                        .map(|decl| function_return_type(p, decl, b, TypeRole::Expression)),
                });
            } else if p.kind() == "variadic_parameter" {
                // CDT uses the previous parameter's type for the synthetic
                // variadic node, while its signature component remains `...`.
                let index = params.len() + 1;
                let ty = params
                    .last()
                    .map(|param| param.ty.clone())
                    .unwrap_or_else(|| "ANY".into());
                params.push(Param {
                    name: format!("<param>{index}"),
                    code: format!("<param>{index}..."),
                    ty,
                    variadic: true,
                    call_type: None,
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

/// Parentheses and function-pointer dereference retain the callable binding.
/// Field/index/arithmetic receivers still require independent type resolution.
fn callable_identifier<'tree>(n: Node<'tree>, b: &[u8]) -> Option<Node<'tree>> {
    let n = unwrap_paren(n);
    if n.kind() == "identifier" {
        Some(n)
    } else if n.kind() == "pointer_expression"
        && n.child_by_field_name("operator")
            .is_some_and(|op| text(op, b) == "*")
    {
        callable_identifier(n.child_by_field_name("argument")?, b)
    } else {
        None
    }
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

/// Literal-node types from the pinned CDT frontend. Integer types follow the
/// suffix, even when an unsuffixed value exceeds the range of C's `int`.
/// Declaration normalization and non-literal macro wrappers have separate rules.
fn numeric_literal_type(literal: &str) -> &'static str {
    let hexadecimal = literal.starts_with("0x") || literal.starts_with("0X");
    let floating = if hexadecimal {
        // Hexadecimal e/E/f/F characters are digits, not floating markers.
        literal.contains(['p', 'P'])
    } else {
        literal.contains(['.', 'e', 'E'])
    };
    if floating {
        return match literal.as_bytes().last() {
            Some(b'f' | b'F') => "float",
            Some(b'l' | b'L') => "longdouble",
            _ => "double",
        };
    }
    let digits = literal.trim_end_matches(['u', 'U', 'l', 'L']);
    match literal[digits.len()..].to_ascii_lowercase().as_str() {
        "u" => "unsigned int",
        "l" => "longint",
        "ll" => "longlongint",
        "ul" | "lu" => "unsigned longint",
        "ull" | "llu" => "unsigned longlongint",
        _ => "int",
    }
}

/// Type suffix from declarator nesting: `*` per pointer level, `[]` per array
/// level (Joern renders `int *p` as `int*` and `int vals[]` as `int[]`).
fn decl_suffix(n: Node, b: &[u8]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = n;
    loop {
        match cur.kind() {
            "pointer_declarator" | "abstract_pointer_declarator" => parts.push("*".into()),
            "array_declarator" | "abstract_array_declarator" => {
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

/// Array members use TypeNameProvider's nodeSignature dimension spelling:
/// an entire macro invocation expands, while a surrounding source expression
/// retains its spelling. cleanType then removes whitespace, including within
/// comments. The macro snapshot belongs to the aggregate's source position.
fn member_decl_suffix(n: Node, b: &[u8], macros: &HashMap<String, MacroDef>) -> String {
    let mut parts = Vec::new();
    let mut cur = n;
    loop {
        match cur.kind() {
            "pointer_declarator" | "abstract_pointer_declarator" => parts.push("*".into()),
            "array_declarator" | "abstract_array_declarator" => {
                let size = cur
                    .child_by_field_name("size")
                    .map(|size| {
                        let whole_macro = match size.kind() {
                            "identifier" => macros
                                .get(text(size, b))
                                .is_some_and(|m| m.params.is_none()),
                            "call_expression" => size
                                .child_by_field_name("function")
                                .and_then(|function| macros.get(text(function, b)))
                                .is_some_and(|m| m.params.is_some()),
                            _ => false,
                        };
                        let spelling = if whole_macro {
                            expand_body_expression(
                                text(size, b),
                                macros,
                                &mut HashSet::new(),
                                &mut 65_536,
                            )
                        } else {
                            text(size, b).to_string()
                        };
                        spelling
                            .chars()
                            .filter(|c| !c.is_whitespace())
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                parts.push(format!("[{size}]"));
            }
            _ => break,
        }
        match cur.child_by_field_name("declarator") {
            Some(child) => cur = child,
            None => break,
        }
    }
    parts.reverse();
    parts.concat()
}

/// Object declarators use CDT's binding spelling; parameter/member renderers
/// retain their distinct suffix ordering. A nested declarator contributes its
/// pointer shape but not inner array dimensions (`*(*p[3])[2]` -> `*(*)[2]`).
fn object_decl_suffix(n: Node, b: &[u8], nested_name: bool) -> String {
    let nested = || {
        n.child_by_field_name("declarator")
            .map(|d| object_decl_suffix(d, b, nested_name))
            .unwrap_or_default()
    };
    match n.kind() {
        "pointer_declarator" | "abstract_pointer_declarator" => format!("*{}", nested()),
        "array_declarator" | "abstract_array_declarator" if !nested_name => {
            let size = n
                .child_by_field_name("size")
                .map(|sz| text(sz, b))
                .unwrap_or_default();
            format!("{}[{size}]", nested())
        }
        "array_declarator" | "abstract_array_declarator" => nested(),
        "parenthesized_declarator" | "abstract_parenthesized_declarator" => {
            let inner = n
                .named_child(0)
                .map(|d| object_decl_suffix(d, b, true))
                .unwrap_or_default();
            if inner.is_empty() {
                inner
            } else {
                format!("({inner})")
            }
        }
        _ => String::new(),
    }
}

/// CDT's nested declarator name has empty CODE on synthesized assignment LHSs.
fn nested_declarator_name(mut n: Node) -> bool {
    loop {
        if n.kind() == "parenthesized_declarator" {
            return true;
        }
        let Some(next) = n.child_by_field_name("declarator") else {
            return false;
        };
        n = next;
    }
}

/// Array dimensions in source order; an absent expression retains unsized [].
fn array_dimensions<'t>(n: Node<'t>) -> Vec<Option<Node<'t>>> {
    let mut out = Vec::new();
    let mut cur = n;
    loop {
        if cur.kind() == "array_declarator" {
            out.push(cur.child_by_field_name("size"));
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

/// CDT uses distinct primitive spellings for declaration specifiers, definition
/// returns and resolved expression results. Keep these roles separate: for
/// `unsigned long`, they are `longunsigned`, `unsigned long` and
/// `unsigned longint`, respectively (all pinned by primitive-roles fixtures).
#[derive(Clone, Copy)]
enum TypeRole {
    Declaration,
    DefinitionReturn,
    Expression,
}

/// C2 AstForExpressionsCreator passes only sizeof's declaration specifier to
/// astForIdentifier. Pointer, array and function declarators do not participate;
/// named/tagged specifiers use a short NAME while primitive CODE keeps qualifiers.
fn sizeof_type_identifier(descriptor: Node, bytes: &[u8]) -> Phantom {
    let base = descriptor.child_by_field_name("type").unwrap_or(descriptor);
    let specifier_end = descriptor
        .child_by_field_name("declarator")
        .map_or(descriptor.end_byte(), |declarator| declarator.start_byte());
    let code = esc(
        std::str::from_utf8(&bytes[descriptor.start_byte()..specifier_end])
            .unwrap_or("")
            .trim(),
    );
    let name = if matches!(
        base.kind(),
        "struct_specifier" | "union_specifier" | "enum_specifier"
    ) {
        base.child_by_field_name("name")
            .map(|name| text(name, bytes).to_string())
            .unwrap_or_else(|| code.clone())
    } else if base.kind() == "type_identifier" {
        text(base, bytes).to_string()
    } else {
        code.clone()
    };
    Phantom {
        name,
        code,
        ty: declaration_type(descriptor, bytes, TypeRole::Declaration),
    }
}

fn declaration_type(declaration: Node, b: &[u8], role: TypeRole) -> String {
    specifier_type_for_role(
        declaration,
        declaration.child_by_field_name("type"),
        b,
        role,
    )
}

fn specifier_type_for_role(
    declaration: Node,
    type_node: Option<Node>,
    b: &[u8],
    role: TypeRole,
) -> String {
    let raw = type_node.map(|node| text(node, b)).unwrap_or("ANY");
    let Some(primitive) = primitive_type(raw, role) else {
        // Typedef names, tagged types and other specifiers retain their existing
        // handling. In particular this does not change cast type rendering.
        return normalize_type(raw);
    };
    // Qualifiers are siblings of the type node, not part of its source range.
    // CDT keeps base `volatile`, drops `const`, and ignores qualifiers nested
    // in pointer declarators for these selected declaration/return properties.
    let volatile = named_children(declaration)
        .iter()
        .any(|node| node.kind() == "type_qualifier" && text(*node, b) == "volatile");
    if volatile {
        format!("volatile {primitive}")
    } else {
        primitive
    }
}

fn primitive_type(raw: &str, role: TypeRole) -> Option<String> {
    let words: Vec<_> = raw.split_whitespace().collect();
    if words.is_empty()
        || words.iter().any(|word| {
            !matches!(
                *word,
                "void"
                    | "char"
                    | "short"
                    | "int"
                    | "long"
                    | "signed"
                    | "unsigned"
                    | "float"
                    | "double"
                    | "_Bool"
            )
        })
    {
        return None;
    }
    let count = |word: &str| words.iter().filter(|&&w| w == word).count();
    if words
        .iter()
        .any(|word| count(word) > if *word == "long" { 2 } else { 1 })
        || (count("signed") > 0 && count("unsigned") > 0)
        || (count("short") > 0 && count("long") > 0)
    {
        return None;
    }
    let signed = count("signed") > 0;
    let unsigned = count("unsigned") > 0;
    let explicit_int = count("int") > 0;
    let width = if count("short") > 0 {
        "short"
    } else {
        match count("long") {
            0 => "",
            1 => "long",
            _ => "longlong",
        }
    };
    for atom in ["void", "char", "float", "double", "_Bool"] {
        if count(atom) == 0 {
            continue;
        }
        let allowed = match atom {
            "char" => {
                width.is_empty()
                    && !explicit_int
                    && words.len() == 1 + usize::from(signed || unsigned)
            }
            "double" => {
                !signed
                    && !unsigned
                    && !explicit_int
                    && matches!(width, "" | "long")
                    && words.len() == 1 + usize::from(width == "long")
            }
            _ => words.len() == 1,
        };
        if !allowed {
            return None;
        }
        return Some(match atom {
            "char" if signed => "signedchar".into(),
            "char" if unsigned => "unsigned char".into(),
            "double" if width == "long" => "longdouble".into(),
            "_Bool" if matches!(role, TypeRole::DefinitionReturn) => "bool".into(),
            _ => atom.into(),
        });
    }
    let ty = match role {
        TypeRole::Expression => format!("{}{width}int", if unsigned { "unsigned " } else { "" }),
        TypeRole::DefinitionReturn => format!(
            "{}{width}{}",
            if unsigned {
                if width.is_empty() && !explicit_int {
                    "unsigned"
                } else {
                    "unsigned "
                }
            } else if signed {
                "signed"
            } else {
                ""
            },
            if explicit_int { "int" } else { "" },
        ),
        TypeRole::Declaration if !width.is_empty() => format!(
            "{width}{}",
            if unsigned {
                if explicit_int {
                    " unsigned int"
                } else {
                    "unsigned"
                }
            } else if signed {
                if explicit_int {
                    "signedint"
                } else {
                    "signed"
                }
            } else {
                ""
            },
        ),
        TypeRole::Declaration => format!(
            "{}{}",
            if unsigned {
                if explicit_int {
                    "unsigned "
                } else {
                    "unsigned"
                }
            } else if signed {
                "signed"
            } else {
                ""
            },
            if explicit_int { "int" } else { "" },
        ),
    };
    Some(ty)
}

/// CDT's reconstructed initializer CODE for a declaration produced entirely
/// by a macro: omit the generated name and dimensions, retain type qualifiers
/// and spell pointer/array operators as separate tokens.
fn expanded_declaration_code(declaration: Node, mut declarator: Node, b: &[u8]) -> String {
    let spec_end = declaration
        .child_by_field_name("type")
        .map(|node| node.end_byte())
        .unwrap_or(declaration.start_byte());
    let mut parts: Vec<_> = std::str::from_utf8(&b[declaration.start_byte()..spec_end])
        .unwrap_or("")
        .split_whitespace()
        .filter(|word| !matches!(*word, "struct" | "union" | "enum"))
        .map(str::to_string)
        .collect();
    loop {
        match declarator.kind() {
            "pointer_declarator" => {
                parts.push("*".into());
                for child in named_children(declarator) {
                    if child.kind() == "type_qualifier" {
                        parts.push(text(child, b).to_string());
                    }
                }
            }
            "array_declarator" => parts.push("[]".into()),
            _ => {}
        }
        let Some(child) = declarator.child_by_field_name("declarator") else {
            break;
        };
        declarator = child;
    }
    esc(&parts.join(" "))
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

// A typedef aggregate owns one body; its tag (or first anonymous alias)
// names that body independently of the alias declarations which follow it.
fn typedef_aggregate(node: Node) -> Option<Node> {
    if node.kind() != "type_definition" {
        return None;
    }
    node.child_by_field_name("type").filter(|n| {
        matches!(n.kind(), "struct_specifier" | "union_specifier")
            && n.child_by_field_name("body").is_some()
    })
}

fn typedef_alias_name(node: Node, bytes: &[u8]) -> String {
    if matches!(node.kind(), "identifier" | "type_identifier") {
        text(node, bytes).to_string()
    } else {
        node.child_by_field_name("declarator")
            .map(|child| typedef_alias_name(child, bytes))
            .unwrap_or_default()
    }
}

fn aggregate_name(node: Node, bytes: &[u8]) -> String {
    node.child_by_field_name("name")
        .map(|name| text(name, bytes).to_string())
        .or_else(|| {
            node.parent()
                .filter(|parent| typedef_aggregate(*parent) == Some(node))
                .and_then(|parent| typedef_declarators(parent).first().copied())
                .map(|alias| typedef_alias_name(alias, bytes))
        })
        .unwrap_or_default()
}

// A named body and its same-spelled typedef alias are distinct TYPE_DECLs.
// Joern's duplicate suffix belongs to the alias, while TYPE references retain
// the original aggregate name.
fn aggregate_alias_full_name(aggregate: Node, alias: Node, bytes: &[u8]) -> String {
    let name = typedef_alias_name(alias, bytes);
    if aggregate
        .child_by_field_name("name")
        .is_some_and(|tag| text(tag, bytes) == name)
    {
        format!("{name}<duplicate>0")
    } else {
        name
    }
}

fn aggregate_code(node: Node, bytes: &[u8]) -> String {
    let start = node
        .parent()
        .filter(|parent| typedef_aggregate(*parent) == Some(node))
        .map_or(node.start_byte(), |parent| parent.start_byte());
    esc(std::str::from_utf8(&bytes[start..node.end_byte()]).unwrap_or(""))
}

fn needs_clinit(n: Node, _b: &[u8]) -> bool {
    let Some(body) = n.child_by_field_name("body") else {
        return false;
    };
    named_children(body).iter().any(|f| {
        (f.kind() == "field_declaration"
            && named_children(*f).iter().any(|d| {
                d.kind() == "array_declarator" && array_dimensions(*d).iter().any(Option::is_some)
            }))
            || (f.kind() == "enumerator" && f.child_by_field_name("value").is_some())
    })
}

/// CDT's MacroArgumentExtractor joins lexical tokens before MacroHandler
/// removes ASCII spaces from its lookup key. Newlines, tabs and comments
/// between tokens therefore never participate in matching. Only this lookup
/// key is normalized; copied nodes retain their original literal spelling.
fn macro_argument_match_code(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(end) = macro_opaque_end(bytes, i) {
            if !bytes[i..].starts_with(b"/*") && !bytes[i..].starts_with(b"//") {
                out.extend(source[i..end].chars().filter(|&c| c != ' '));
            }
            i = end;
        } else {
            let c = source[i..].chars().next().unwrap();
            if !c.is_ascii_whitespace() {
                out.push(c);
            }
            i += c.len_utf8();
        }
    }
    out
}

/// Whole-word substitution of macro parameters with argument source text.
fn substitute(body: &str, params: &[String], args: &[String]) -> String {
    let mut out = String::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes[i], b'\'' | b'"') {
            let quote = bytes[i];
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == quote {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            out.push_str(&body[start..i]);
        } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &body[start..i];
            match params.iter().position(|p| p == word) {
                Some(k) if k < args.len() => out.push_str(&args[k]),
                _ => out.push_str(word),
            }
        } else {
            let c = body[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

fn expanded_cast_descriptor(desc: Node, bytes: &[u8]) -> String {
    fn declarator_code(node: Node, bytes: &[u8]) -> String {
        let inner = node.child_by_field_name("declarator");
        match node.kind() {
            "abstract_pointer_declarator" => {
                let end = inner.map_or(node.end_byte(), |n| n.start_byte());
                let prefix = std::str::from_utf8(&bytes[node.start_byte()..end])
                    .unwrap_or("")
                    .trim();
                let qualifiers = prefix.strip_prefix('*').unwrap_or(prefix).trim();
                let nested = inner.map(|n| declarator_code(n, bytes)).unwrap_or_default();
                ["*", qualifiers, nested.as_str()]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            "abstract_array_declarator" => {
                let nested = inner.map(|n| declarator_code(n, bytes)).unwrap_or_default();
                if nested.starts_with('(') {
                    format!("[] {nested}")
                } else {
                    format!("[]{nested}")
                }
            }
            "abstract_parenthesized_declarator" => {
                let nested = inner.or_else(|| named_children(node).into_iter().next());
                nested.map_or_else(
                    || text(node, bytes).to_string(),
                    |n| format!("({})", declarator_code(n, bytes)),
                )
            }
            _ => text(node, bytes).to_string(),
        }
    }
    let Some(base) = desc.child_by_field_name("type") else {
        return text(desc, bytes).to_string();
    };
    let raw = text(base, bytes);
    let rendered = ["struct ", "union ", "enum "]
        .into_iter()
        .find_map(|prefix| raw.strip_prefix(prefix))
        .unwrap_or(raw);
    let rendered = if rendered == "unsigned long" {
        "long unsigned"
    } else {
        rendered
    };
    let suffix = desc.child_by_field_name("declarator").map_or_else(
        || {
            std::str::from_utf8(&bytes[base.end_byte()..desc.end_byte()])
                .unwrap_or("")
                .trim()
                .to_string()
        },
        |node| declarator_code(node, bytes),
    );
    [
        std::str::from_utf8(&bytes[desc.start_byte()..base.start_byte()])
            .unwrap_or("")
            .trim(),
        rendered,
        suffix.as_str(),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}

fn macro_opaque_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes[start..].starts_with(b"/*") {
        return Some(
            bytes[start + 2..]
                .windows(2)
                .position(|w| w == b"*/")
                .map_or(bytes.len(), |end| start + end + 4),
        );
    }
    if bytes[start..].starts_with(b"//") {
        let mut i = start + 2;
        while i < bytes.len() {
            if bytes[i..].starts_with(b"\\\r\n") {
                i += 3;
            } else if bytes[i..].starts_with(b"\\\n") {
                i += 2;
            } else if bytes[i] == b'\n' {
                return Some(i);
            } else {
                i += 1;
            }
        }
        return Some(i);
    }
    let quote = bytes[start];
    if !matches!(quote, b'\'' | b'"') {
        return None;
    }
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i = (i + 2).min(bytes.len());
        } else if bytes[i] == quote {
            return Some(i + 1);
        } else {
            i += 1;
        }
    }
    Some(i)
}

/// Scan one macro argument list as preprocessing tokens, before C expression
/// parsing. Operator-only and type arguments occupy slots even when they have
/// no named syntax node. Only parentheses nest arguments; quoted tokens and
/// comments are opaque to commas and parentheses.
fn macro_arguments(source: &str, open: usize) -> Option<(Vec<String>, usize)> {
    let bytes = source.as_bytes();
    if bytes.get(open) != Some(&b'(') {
        return None;
    }
    // Translation phase 2 precedes comment and argument recognition. Keep
    // offsets into the original invocation so a splice forming `/*` or `//`
    // cannot expose a comma or close parenthesis inside the logical comment.
    let mut spliced = String::new();
    let mut removed = Vec::new();
    let mut copied = open;
    let mut i = open;
    while i < bytes.len() {
        let count = if bytes[i..].starts_with(b"\\\r\n") {
            3
        } else if bytes[i..].starts_with(b"\\\n") {
            2
        } else {
            i += 1;
            continue;
        };
        spliced.push_str(&source[copied..i]);
        removed.push((spliced.len(), count));
        i += count;
        copied = i;
    }
    if !removed.is_empty() {
        spliced.push_str(&source[copied..]);
        let (args, end) = macro_arguments(&spliced, 0)?;
        let removed_before_end: usize = removed
            .iter()
            .filter(|(at, _)| *at <= end)
            .map(|(_, count)| count)
            .sum();
        return Some((args, open + end + removed_before_end));
    }
    let mut args = Vec::new();
    let mut argument = open + 1;
    let mut end = argument;
    let mut nesting = 1;
    while end < bytes.len() {
        if let Some(next) = macro_opaque_end(bytes, end) {
            end = next;
            continue;
        }
        match bytes[end] {
            b'(' => nesting += 1,
            b')' => {
                nesting -= 1;
                if nesting == 0 {
                    break;
                }
            }
            b',' if nesting == 1 => {
                args.push(macro_argument_text(&source[argument..end]));
                argument = end + 1;
            }
            _ => {}
        }
        end += 1;
    }
    if nesting != 0 {
        return None;
    }
    if argument != end || !args.is_empty() {
        args.push(macro_argument_text(&source[argument..end]));
    }
    Some((args, end))
}

/// Comments become whitespace before substitution. Splices disappear even in
/// quoted tokens, while escaped quotes and literal comma/parenthesis bytes stay.
fn macro_argument_text(source: &str) -> String {
    let spliced = source.replace("\\\r\n", "").replace("\\\n", "");
    let bytes = spliced.as_bytes();
    let mut out = String::new();
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if let Some(end) = macro_opaque_end(bytes, i) {
            if bytes[i..].starts_with(b"/*") || bytes[i..].starts_with(b"//") {
                out.push_str(&spliced[copied..i]);
                out.push(' ');
                copied = end;
            }
            i = end;
        } else {
            i += 1;
        }
    }
    out.push_str(&spliced[copied..]);
    out.trim().to_string()
}

// Consume the complete token even when its spelling is outside the ASCII
// macro-name registry. Otherwise an ASCII suffix of an extended identifier
// could be mistaken for a separate macro invocation.
fn macro_identifier_unit(bytes: &[u8], start: usize, first: bool) -> usize {
    let b = bytes[start];
    if b.is_ascii_alphabetic()
        || matches!(b, b'_' | b'$')
        || !b.is_ascii()
        || (!first && b.is_ascii_digit())
    {
        return 1;
    }
    if b == b'\\' {
        let digits = match bytes.get(start + 1) {
            Some(b'u') => 4,
            Some(b'U') => 8,
            _ => return 0,
        };
        if bytes
            .get(start + 2..start + 2 + digits)
            .is_some_and(|value| value.iter().all(u8::is_ascii_hexdigit))
        {
            return 2 + digits;
        }
    }
    0
}

/// Expand known function macros using preprocessing-token parentheses. Strings
/// and comments are opaque; ordinary calls and type spellings remain untouched.
fn expand_function_macro_tokens(
    source: &str,
    macros: &HashMap<String, MacroDef>,
    disabled: &mut HashSet<String>,
    budget: &mut usize,
) -> String {
    let bytes = source.as_bytes();
    let mut result = String::new();
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() && *budget > 0 {
        if let Some(end) = macro_opaque_end(bytes, i) {
            i = end;
            continue;
        }
        if bytes[i].is_ascii_digit()
            || (bytes[i] == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            // A preprocessing number includes identifier suffixes and signs
            // after e/E/p/P, even when the resulting C literal is invalid.
            // Do not turn its suffix into a function-macro invocation.
            i += 1;
            while i < bytes.len() {
                if matches!(bytes[i], b'+' | b'-')
                    && matches!(bytes[i - 1], b'e' | b'E' | b'p' | b'P')
                    || bytes[i] == b'.'
                {
                    i += 1;
                } else {
                    let next = macro_identifier_unit(bytes, i, false);
                    if next == 0 {
                        break;
                    }
                    i += next;
                }
            }
            continue;
        }
        let first = macro_identifier_unit(bytes, i, true);
        if first == 0 {
            i += 1;
            continue;
        }
        let start = i;
        i += first;
        while i < bytes.len() {
            let next = macro_identifier_unit(bytes, i, false);
            if next == 0 {
                break;
            }
            i += next;
        }
        let name = &source[start..i];
        let Some(definition) = macros.get(name).filter(|m| m.params.is_some()) else {
            continue;
        };
        if disabled.contains(name) || disabled.len() >= 256 {
            continue;
        }
        let mut open = i;
        loop {
            while open < bytes.len() && bytes[open].is_ascii_whitespace() {
                open += 1;
            }
            if open < bytes.len()
                && (bytes[open..].starts_with(b"/*") || bytes[open..].starts_with(b"//"))
            {
                open = macro_opaque_end(bytes, open).unwrap();
            } else {
                break;
            }
        }
        if bytes.get(open) != Some(&b'(') {
            continue;
        }
        let Some((args, end)) = macro_arguments(source, open) else {
            continue;
        };
        *budget -= 1;
        disabled.insert(name.to_string());
        let replaced = substitute(
            &definition.body,
            definition.params.as_deref().unwrap(),
            &args,
        );
        let expanded = expand_body_expression(&replaced, macros, disabled, budget);
        disabled.remove(name);
        result.push_str(&source[copied..start]);
        result.push_str(&expanded);
        copied = end + 1;
        i = copied;
    }
    result.push_str(&source[copied..]);
    result
}

#[cfg(test)]
mod macro_token_tests {
    use super::*;

    #[test]
    fn declaration_expansion_preserves_opaque_preprocessing_tokens() {
        let macros = HashMap::from([(
            "M".to_string(),
            MacroDef {
                params: None,
                body: "int".to_string(),
                file: "test.c".to_string(),
                directive: String::new(),
            },
        )]);
        let source = r#"M éM \u00e9M 1M 0xM 1e+M "M" 'M' /* M */"#;
        assert_eq!(
            expand_declaration_tokens(source, &macros, &mut HashSet::new(), &mut 65_536),
            Some(r#"int éM \u00e9M 1M 0xM 1e+M "M" 'M' /* M */"#.to_string())
        );
    }

    #[test]
    fn declaration_expansion_bounds_recursive_and_oversized_replacements() {
        let macros = HashMap::from([
            (
                "A".to_string(),
                MacroDef {
                    params: None,
                    body: "B".to_string(),
                    file: "test.c".to_string(),
                    directive: String::new(),
                },
            ),
            (
                "B".to_string(),
                MacroDef {
                    params: None,
                    body: "A".to_string(),
                    file: "test.c".to_string(),
                    directive: String::new(),
                },
            ),
            (
                "BIG".to_string(),
                MacroDef {
                    params: None,
                    body: "x".repeat(65_536),
                    file: "test.c".to_string(),
                    directive: String::new(),
                },
            ),
        ]);
        assert_eq!(
            expand_declaration_tokens("A f(A x)", &macros, &mut HashSet::new(), &mut 65_536),
            Some("A f(A x)".to_string())
        );
        assert!(
            expand_declaration_tokens("BIG", &macros, &mut HashSet::new(), &mut 65_536).is_none()
        );
    }

    #[test]
    fn argument_slots_follow_preprocessing_tokens() {
        let source = r#"(+, (a,b), union U *, "comma,) /* // \\\"", ')',, value,) tail"#;
        let (args, end) = macro_arguments(source, 0).unwrap();
        assert_eq!(
            args,
            [
                "+",
                "(a,b)",
                "union U *",
                r#""comma,) /* // \\\"""#,
                "')'",
                "",
                "value",
                ""
            ]
        );
        assert_eq!(&source[end..], ") tail");
        assert!(macro_arguments("(a, (b)", 0).is_none());
    }

    #[test]
    fn argument_splices_precede_comment_recognition() {
        for newline in ["\n", "\r\n"] {
            let source = format!(
                "prefix(/\\{newline}* ignored , ) */ +, // ignored \\{newline}, )\n a, b) tail"
            );
            let (args, end) = macro_arguments(&source, 6).unwrap();
            assert_eq!(args, ["+", "a", "b"]);
            assert_eq!(&source[end..], ") tail");
        }
    }

    #[test]
    fn function_macro_names_inside_preprocessing_numbers_remain_opaque() {
        let macros = HashMap::from([(
            "M".to_string(),
            MacroDef {
                params: Some(vec!["x".to_string()]),
                body: "7".to_string(),
                directive: "#define M(x) 7".to_string(),
                file: "main.c".to_string(),
            },
        )]);
        for source in ["1M(2)", "0xM(2)", "1e+M(2)", "0x1p-M(2)", ".1M(2)"] {
            assert_eq!(
                expand_function_macro_tokens(source, &macros, &mut HashSet::new(), &mut 65_536),
                source,
            );
        }
        assert_eq!(
            expand_function_macro_tokens("1 + M(2)", &macros, &mut HashSet::new(), &mut 65_536),
            "1 + 7",
        );
    }
}

/// Render CDT's expanded expression spelling. A shared work budget and
/// disabled-macro set stop replacement cycles before the AST emitter runs.
fn expand_body_expression(
    source: &str,
    macros: &HashMap<String, MacroDef>,
    disabled: &mut HashSet<String>,
    budget: &mut usize,
) -> String {
    fn render(
        node: Node,
        bytes: &[u8],
        macros: &HashMap<String, MacroDef>,
        disabled: &mut HashSet<String>,
        budget: &mut usize,
    ) -> String {
        let raw = text(node, bytes);
        if *budget == 0 {
            return raw.to_string();
        }
        *budget -= 1;
        // Object-like macros are preprocessing tokens even after . or ->.
        let invoked = if matches!(
            node.kind(),
            "identifier" | "type_identifier" | "field_identifier"
        ) {
            macros
                .get(raw)
                .filter(|m| m.params.is_none())
                .map(|m| (raw, m, Vec::new()))
        } else {
            None
        };
        if let Some((name, definition, args)) = invoked {
            if disabled.len() < 256 && disabled.insert(name.to_string()) {
                let replaced = substitute(
                    &definition.body,
                    definition.params.as_deref().unwrap_or_default(),
                    &args,
                );
                let result = expand_body_expression(&replaced, macros, disabled, budget);
                disabled.remove(name);
                return result;
            }
        }
        let mut child = |field: &str| {
            node.child_by_field_name(field)
                .map(|n| render(n, bytes, macros, disabled, budget))
                .unwrap_or_default()
        };
        match node.kind() {
            "binary_expression" | "assignment_expression" => {
                let left = child("left");
                let op = node
                    .child_by_field_name("operator")
                    .map(|n| text(n, bytes))
                    .unwrap_or("");
                let right = child("right");
                format!("{left} {op} {right}")
            }
            "unary_expression" => {
                let op = node
                    .child_by_field_name("operator")
                    .map(|n| text(n, bytes))
                    .unwrap_or("");
                let argument = child("argument");
                format!("{op}{argument}")
            }
            "comma_expression" => {
                let left = child("left");
                let right = child("right");
                format!("{left}, {right}")
            }
            "conditional_expression" => {
                let a = child("condition");
                let b = child("consequence");
                let c = child("alternative");
                format!("{a} ? {b} : {c}")
            }
            "call_expression" => {
                let function = child("function");
                let args = node
                    .child_by_field_name("arguments")
                    .map(named_children)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|n| render(n, bytes, macros, disabled, budget))
                    .collect::<Vec<_>>()
                    .join(", ");
                let call = format!("{function}({args})");
                // An object macro can supply the function macro name. Rescan
                // that newly formed invocation with the same disabled set.
                if node
                    .child_by_field_name("function")
                    .is_some_and(|original| text(original, bytes) != function)
                    && disabled.len() < 256
                    && !disabled.contains(&function)
                    && macros.get(&function).is_some_and(|m| m.params.is_some())
                {
                    expand_body_expression(&call, macros, disabled, budget)
                } else {
                    call
                }
            }
            "sizeof_expression" => {
                if node.child_by_field_name("type").is_some() {
                    format!("sizeof ({})", child("type"))
                } else if let Some(mut value) = node.child_by_field_name("value") {
                    // CDT renders the operator token separately from its
                    // operand, and removes trivia just inside parentheses.
                    // Ordinary source CODE bypasses this expansion renderer.
                    let mut parentheses = 0;
                    while value.kind() == "parenthesized_expression" {
                        let mut children = named_children(value)
                            .into_iter()
                            .filter(|n| n.kind() != "comment");
                        let Some(inner) = children.next() else {
                            break;
                        };
                        // Error recovery can retain multiple named children.
                        // Keep that complete spelling instead of dropping a
                        // sibling while removing only surrounding trivia.
                        if children.next().is_some() {
                            break;
                        }
                        parentheses += 1;
                        value = inner;
                    }
                    let operand = render(value, bytes, macros, disabled, budget);
                    format!(
                        "sizeof {}{}{}",
                        "(".repeat(parentheses),
                        operand,
                        ")".repeat(parentheses)
                    )
                } else {
                    raw.to_string()
                }
            }
            "offsetof_expression" => {
                let ty = child("type");
                let member = child("member");
                format!("offsetof({ty}, {member})")
            }
            _ => {
                let mut result = raw.to_string();
                for n in named_children(node).into_iter().rev() {
                    let replacement = render(n, bytes, macros, disabled, budget);
                    result.replace_range(
                        n.start_byte() - node.start_byte()..n.end_byte() - node.start_byte(),
                        &replacement,
                    );
                }
                result
            }
        }
    }
    // A function macro accepts preprocessing tokens, including types that are
    // not valid C call arguments. Expand those invocations before C parsing can
    // recover them as casts or discard their arguments as ERROR nodes.
    let source = expand_function_macro_tokens(source, macros, disabled, budget);
    let wrapped = format!("void __m() {{ {source}; }}");
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(&wrapped, None).unwrap();
    if let Some(expr) = expansion_expr_node(tree.root_node()) {
        render(expr, wrapped.as_bytes(), macros, disabled, budget)
    } else {
        source.to_string()
    }
}

/// Collapse adjacent string tokens only in generated macro expansion text.
/// Keep escape spellings intact, discard inter-token comments, and retain the
/// nonempty encoding prefix (CDT still assigns every string type `char*`).
fn join_expansion_strings(root: Node, source: &[u8]) -> Option<String> {
    let mut replacements = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node.kind() != "concatenated_string" {
            pending.extend(named_children(node));
            continue;
        }
        let mut prefix = "";
        let mut contents = String::new();
        let mut valid = true;
        for token in named_children(node) {
            if token.kind() == "comment" {
                continue;
            }
            if token.kind() != "string_literal" {
                valid = false;
                break;
            }
            let spelling = text(token, source);
            let Some(quote) = spelling.find('"') else {
                valid = false;
                break;
            };
            let Some(content) = spelling[quote + 1..].strip_suffix('"') else {
                valid = false;
                break;
            };
            if prefix.is_empty() {
                prefix = &spelling[..quote];
            }
            contents.push_str(content);
        }
        if valid {
            replacements.push((node.byte_range(), format!("{prefix}\"{contents}\"")));
        }
    }
    if replacements.is_empty() {
        return None;
    }
    replacements.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut result = String::from_utf8_lossy(source).into_owned();
    for (range, replacement) in replacements {
        result.replace_range(range, &replacement);
    }
    Some(result)
}

/// Preserve statement roots as well as expression roots in an expansion.
fn expansion_expr_node(root: Node) -> Option<Node> {
    let f = named_children(root)
        .into_iter()
        .find(|n| n.kind() == "function_definition")?;
    let body = f.child_by_field_name("body")?;
    let stmt = named_children(body).into_iter().next()?;
    if stmt.kind() == "expression_statement" {
        named_children(stmt).into_iter().next()
    } else {
        Some(stmt)
    }
}

/// Type CDT assigns to a macro expansion root (drives MFN/SIGNATURE/TYPE).
fn expansion_type(
    expansion: &str,
    symbols: &HashMap<String, String>,
    globals: &HashMap<String, String>,
) -> String {
    let src = format!("void __m() {{ {expansion}; }}");
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .unwrap();
    let Some(tree) = parser.parse(&src, None) else {
        return "ANY".into();
    };
    let Some(e) = expansion_expr_node(tree.root_node()) else {
        return "ANY".into();
    };
    let b = src.as_bytes();
    // CDT types the wrapper node itself. Parenthesized expressions use the
    // generic node-type path (ANY), even when their final child is a literal.
    match e.kind() {
        // tree-sitter includes a leading sign in some number nodes. CDT
        // instead wraps the literal in a unary expression, whose type is ANY.
        "number_literal" if text(e, b).starts_with(['-', '+']) => "ANY".into(),
        "number_literal" => numeric_literal_type(text(e, b)).into(),
        "char_literal" => "char".into(),
        "string_literal" => "char*".into(),
        // A parenthesized replacement is an expression wrapper in CDT; its
        // newly supported concatenation child does not type that wrapper.
        "concatenated_string" if expansion_expr_node(tree.root_node()) == Some(e) => "char*".into(),
        "concatenated_string" => "ANY".into(),
        "identifier" => symbols
            .get(text(e, b))
            .or_else(|| globals.get(text(e, b)))
            .cloned()
            .unwrap_or("ANY".into()),
        _ => "ANY".into(),
    }
}

fn collect_decl_names(
    n: Node,
    b: &[u8],
    macros: &MacroState,
    macro_sites: Option<&BodyMacroSites>,
    out: &mut Vec<String>,
) {
    let macros = macro_sites
        .and_then(|sites| sites.at(n, b))
        .unwrap_or(macros);
    if matches!(
        n.kind(),
        "preproc_if" | "preproc_ifdef" | "preproc_elif" | "preproc_elifdef" | "preproc_else"
    ) {
        for child in kept_preproc_children(n, b, macros) {
            collect_decl_names(child, b, macros, macro_sites, out);
        }
        return;
    }
    if n.kind() == "declaration" {
        for d in named_children(n) {
            let name = innermost_id(d, b);
            if !name.is_empty() {
                out.push(name);
            }
        }
    }
    for c in named_children(n) {
        collect_decl_names(c, b, macros, macro_sites, out);
    }
}

// --- tree helpers ---
/// A comment can end tree-sitter's recovered macro node before the actual
/// directive ends. Tokens in the remainder still belong to that directive;
/// they must not be visited again as file-scope declarations or definitions.
fn translation_unit_children<'tree>(root: Node<'tree>, bytes: &[u8]) -> Vec<Node<'tree>> {
    let mut directive_end = 0;
    named_children(root)
        .into_iter()
        .filter(|node| {
            if node.start_byte() < directive_end {
                return false;
            }
            if matches!(node.kind(), "preproc_def" | "preproc_function_def") {
                directive_end = node.start_byte() + preproc_raw_directive(*node, bytes).len();
            }
            true
        })
        .collect()
}

/// Joern retains declarations from every translation-unit preprocessor branch,
/// including inactive `#if 0` and `#else` branches. Flatten only those wrappers;
/// function bodies and macro-definition selection have their own semantics.
pub(crate) fn translation_unit_items<'tree>(root: Node<'tree>, b: &[u8]) -> Vec<Node<'tree>> {
    fn collect<'tree>(node: Node<'tree>, b: &[u8], items: &mut Vec<Node<'tree>>) {
        if matches!(
            node.kind(),
            "preproc_if" | "preproc_ifdef" | "preproc_else" | "preproc_elif" | "preproc_elifdef"
        ) {
            let condition = node.child_by_field_name("condition");
            let name = node.child_by_field_name("name");
            for child in translation_unit_children(node, b) {
                if Some(child) != condition && Some(child) != name {
                    collect(child, b, items);
                }
            }
        } else {
            items.push(node);
        }
    }

    let mut items = Vec::new();
    for child in translation_unit_children(root, b) {
        collect(child, b, &mut items);
    }
    items
}

/// The active configuration controls macro definitions and function bodies,
/// while `translation_unit_items` independently retains all declarations.
/// Process definitions in source order so later defines do not activate an
/// earlier branch and definitions in an inactive branch cannot leak out.

fn preproc_raw_directive<'bytes>(node: Node, bytes: &'bytes [u8]) -> &'bytes str {
    let source = std::str::from_utf8(&bytes[node.start_byte()..]).unwrap_or("");
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut comment = false;
    let mut line_comment = false;
    let mut quote = None;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"\\\n") {
            i += 2;
            continue;
        }
        if bytes[i..].starts_with(b"\\\r\n") {
            i += 3;
            continue;
        }
        if line_comment {
            if matches!(bytes[i], b'\n' | b'\r') {
                break;
            }
            i += 1;
            continue;
        }
        if comment {
            if bytes[i..].starts_with(b"*/") {
                comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if let Some(delimiter) = quote {
            if bytes[i] == b'\\' {
                i = (i + 2).min(bytes.len());
                continue;
            }
            if bytes[i] == delimiter {
                quote = None;
            }
        } else if bytes[i..].starts_with(b"//") {
            line_comment = true;
            i += 2;
            continue;
        } else if bytes[i..].starts_with(b"/*") {
            comment = true;
            i += 2;
            continue;
        } else if matches!(bytes[i], b'\'' | b'"') {
            quote = Some(bytes[i]);
        } else if matches!(bytes[i], b'\n' | b'\r') {
            break;
        }
        i += 1;
    }
    source[..i].trim_end()
}

fn preproc_logical_line(node: Node, b: &[u8]) -> String {
    let source = std::str::from_utf8(&b[node.start_byte()..]).unwrap_or("");
    let bytes = source.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    let mut block_comment = false;
    let mut quote = None;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"\\\n") {
            i += 2;
            continue;
        }
        if bytes[i..].starts_with(b"\\\r\n") {
            i += 3;
            continue;
        }
        if block_comment {
            if bytes[i..].starts_with(b"*/") {
                block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if let Some(delimiter) = quote {
            if bytes[i] == b'\\' && i + 1 < bytes.len() {
                let end = i + 1 + source[i + 1..].chars().next().unwrap().len_utf8();
                out.push_str(&source[i..end]);
                i = end;
                continue;
            }
            if bytes[i] == delimiter {
                quote = None;
            }
        } else if bytes[i..].starts_with(b"/*") {
            block_comment = true;
            out.push(' ');
            i += 2;
            continue;
        } else if bytes[i..].starts_with(b"//") || matches!(bytes[i], b'\n' | b'\r') {
            break;
        } else if matches!(bytes[i], b'\'' | b'"') {
            quote = Some(bytes[i]);
        }
        let end = i + source[i..].chars().next().unwrap().len_utf8();
        out.push_str(&source[i..end]);
        i = end;
    }
    out
}

fn preproc_payload(line: &str) -> &str {
    let line = line
        .trim_start()
        .strip_prefix('#')
        .unwrap_or(line)
        .trim_start();
    line.trim_start_matches(|c: char| c.is_ascii_alphabetic())
        .trim_start()
}

/// Statements and name discovery share the same selected body branch. The
/// method's immutable macro snapshot already includes preceding source/header
/// definitions; body-local directives are a separate, unsupported state update.
fn kept_preproc_children<'tree>(
    node: Node<'tree>,
    bytes: &[u8],
    macros: &MacroState,
) -> Vec<Node<'tree>> {
    let take = if matches!(node.kind(), "preproc_if" | "preproc_elif") {
        let definitions = macros
            .iter()
            .map(|(name, definition)| {
                (
                    name.clone(),
                    if definition.params.is_some() {
                        name.clone()
                    } else {
                        definition.body.clone()
                    },
                )
            })
            .collect();
        preproc_condition(node, bytes, &definitions)
    } else if let Some(name) = node.child_by_field_name("name") {
        let negated = node
            .child(0)
            .is_some_and(|directive| matches!(directive.kind(), "#ifndef" | "#elifndef"));
        macros.contains_key(text(name, bytes)) != negated
    } else {
        true
    };
    let alternative = node.child_by_field_name("alternative");
    if take {
        let condition = node.child_by_field_name("condition");
        let name = node.child_by_field_name("name");
        named_children(node)
            .into_iter()
            .filter(|child| {
                Some(*child) != condition && Some(*child) != name && Some(*child) != alternative
            })
            .collect()
    } else {
        alternative.into_iter().collect()
    }
}

/// Expand object macro replacement tokens before parsing the condition. In
/// particular, replacing VALUE with `1 + 2` must not implicitly parenthesize
/// it in `VALUE * 3`. The normal C expression grammar also accepts ternaries,
/// which tree-sitter's preprocessor-condition grammar does not represent.
fn preproc_condition(node: Node, b: &[u8], definitions: &HashMap<String, String>) -> bool {
    let line = preproc_logical_line(node, b);
    let condition = preproc_payload(&line);
    let mut budget = 65_536usize;
    let expanded = expand_preproc_objects(condition, definitions, &mut HashSet::new(), &mut budget);
    let source = format!("int __condition(void) {{ return {expanded}; }}");
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(&source, None).unwrap();
    tree.root_node()
        .named_child(0)
        .and_then(|function| function.child_by_field_name("body"))
        .and_then(|body| body.named_child(0))
        .and_then(|returned| returned.named_child(0))
        .is_some_and(|expression| preproc_value(expression, source.as_bytes(), 0) != 0)
}

/// Preserve literal tokens and `defined` operands while expanding object
/// macros. A shared byte budget and cycle guard bound recursive expansion.
fn expand_preproc_objects(
    source: &str,
    definitions: &HashMap<String, String>,
    expanding: &mut HashSet<String>,
    budget: &mut usize,
) -> String {
    let bytes = source.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() && *budget > 0 {
        let start = i;
        if matches!(bytes[i], b'\'' | b'"') {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == quote {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
        } else if bytes[i].is_ascii_digit() {
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'.' | b'\''))
            {
                i += 1;
            }
        } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let name = &source[start..i];
            if name == "defined" {
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                let parentheses = bytes.get(i) == Some(&b'(');
                if parentheses {
                    i += 1;
                }
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                let operand = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let defined = definitions.contains_key(&source[operand..i]);
                if parentheses {
                    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if bytes.get(i) == Some(&b')') {
                        i += 1;
                    }
                }
                out.push(if defined { '1' } else { '0' });
                *budget -= 1;
                continue;
            }
            if expanding.len() < 64 && !expanding.contains(name) {
                if let Some(replacement) = definitions.get(name) {
                    *budget -= 1;
                    expanding.insert(name.to_string());
                    out.push_str(&expand_preproc_objects(
                        replacement,
                        definitions,
                        expanding,
                        budget,
                    ));
                    expanding.remove(name);
                    continue;
                }
            }
        } else if bytes[i..].starts_with(b"//") {
            break;
        } else if bytes[i..].starts_with(b"/*") {
            i += 2;
            while i < bytes.len() && !bytes[i..].starts_with(b"*/") {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            out.push(' ');
            *budget -= 1;
            continue;
        } else {
            i += source[i..].chars().next().unwrap().len_utf8();
        }
        let token = &source[start..i];
        if token.len() > *budget {
            break;
        }
        out.push_str(token);
        *budget -= token.len();
    }
    out
}

fn preproc_character(literal: &str) -> i64 {
    let Some((_, contents)) = literal.split_once('\'') else {
        return 0;
    };
    let contents = contents.strip_suffix('\'').unwrap_or(contents);
    if let Some(escaped) = contents.strip_prefix('\\') {
        if let Some(hex) = escaped.strip_prefix('x') {
            return i64::from_str_radix(hex, 16).unwrap_or(0);
        }
        if escaped.starts_with(|c: char| matches!(c, '0'..='7')) {
            return i64::from_str_radix(escaped, 8).unwrap_or(0);
        }
        return match escaped {
            "n" => 10,
            "r" => 13,
            "t" => 9,
            "v" => 11,
            "a" => 7,
            "b" => 8,
            "f" => 12,
            _ => escaped.bytes().next().unwrap_or(0) as i64,
        };
    }
    contents
        .bytes()
        .fold(0i64, |value, byte| value.wrapping_shl(8) | byte as i64)
}

fn preproc_value(node: Node, b: &[u8], depth: usize) -> i64 {
    if depth >= 256 {
        return 0;
    }
    let value = |child| preproc_value(child, b, depth + 1);
    match node.kind() {
        "number_literal" => {
            let literal = text(node, b).trim_end_matches(['u', 'U', 'l', 'L']);
            let (digits, radix) = if let Some(digits) = literal
                .strip_prefix("0x")
                .or_else(|| literal.strip_prefix("0X"))
            {
                (digits, 16)
            } else if let Some(digits) = literal
                .strip_prefix("0b")
                .or_else(|| literal.strip_prefix("0B"))
            {
                (digits, 2)
            } else if literal.len() > 1 && literal.starts_with('0') {
                (&literal[1..], 8)
            } else {
                (literal, 10)
            };
            u64::from_str_radix(digits, radix).unwrap_or(0) as i64
        }
        "parenthesized_expression" => node.named_child(0).map(value).unwrap_or(0),
        "unary_expression" => {
            let argument = node.child_by_field_name("argument").map(value).unwrap_or(0);
            match node.child_by_field_name("operator").map(|op| text(op, b)) {
                Some("!") => (argument == 0) as i64,
                Some("~") => !argument,
                Some("-") => argument.wrapping_neg(),
                Some("+") => argument,
                _ => 0,
            }
        }
        "binary_expression" => {
            let left = node.child_by_field_name("left").map(value).unwrap_or(0);
            let right = node.child_by_field_name("right").map(value).unwrap_or(0);
            match node.child_by_field_name("operator").map(|op| text(op, b)) {
                Some("||") => (left != 0 || right != 0) as i64,
                Some("&&") => (left != 0 && right != 0) as i64,
                Some("|") => left | right,
                Some("&") => left & right,
                Some("^") => left ^ right,
                Some("==") => (left == right) as i64,
                Some("!=") => (left != right) as i64,
                Some("<") => (left < right) as i64,
                Some("<=") => (left <= right) as i64,
                Some(">") => (left > right) as i64,
                Some(">=") => (left >= right) as i64,
                Some("<<") => left.checked_shl(right as u32).unwrap_or(0),
                Some(">>") => left.checked_shr(right as u32).unwrap_or(0),
                Some("+") => left.wrapping_add(right),
                Some("-") => left.wrapping_sub(right),
                Some("*") => left.wrapping_mul(right),
                Some("/") => left.checked_div(right).unwrap_or(0),
                Some("%") => left.checked_rem(right).unwrap_or(0),
                _ => 0,
            }
        }
        "conditional_expression" => {
            let condition = node
                .child_by_field_name("condition")
                .map(value)
                .unwrap_or(0);
            let branch = if condition != 0 {
                "consequence"
            } else {
                "alternative"
            };
            node.child_by_field_name(branch).map(value).unwrap_or(0)
        }
        "char_literal" => preproc_character(text(node, b)),
        _ => 0,
    }
}

fn named_children(n: Node) -> Vec<Node> {
    let mut cur = n.walk();
    n.named_children(&mut cur).collect()
}
fn text<'a>(n: Node, b: &'a [u8]) -> &'a str {
    n.utf8_text(b).unwrap_or("")
}
/// The pinned frontend emits STATIC only for a leading storage-class token.
/// `inline static` and a definition inheriting an earlier static declaration
/// receive no modifier. Prototype CODE may already contain escaped newlines.
fn has_leading_static(code: &str) -> bool {
    code.trim_start()
        .strip_prefix("static")
        .is_some_and(|mut rest| {
            // A physical line continuation can separate this token from the
            // next one. Prototype CODE has escaped the newline already.
            while let Some(tail) = rest
                .strip_prefix("\\\r\n")
                .or_else(|| rest.strip_prefix("\\\n"))
                .or_else(|| rest.strip_prefix("\\\r\\n"))
                .or_else(|| rest.strip_prefix("\\\\n"))
            {
                rest = tail;
            }
            // `$` is an accepted identifier extension. A universal-character
            // escape also continues an identifier; `\\n` here can instead be
            // the prototype CODE transport's escaped newline.
            rest.starts_with("\\n")
                || rest.chars().next().is_some_and(|next| {
                    !next.is_alphanumeric() && !matches!(next, '_' | '$' | '\\')
                })
        })
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
    has_code: bool,
    full: String,
    has_arg: bool,
    arg_index: i64,
    order: i64,
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

/// FULL_NAME may contain spaces in a filename or a macro return signature.
/// Stop at a following property emitted by `Ctx::line`, rather than a word.
fn extract_full_name(rest: &str) -> String {
    // CODE precedes FULL_NAME and can itself contain property-like text.
    let Some((_, tail)) = rest.rsplit_once(" FULL_NAME=") else {
        return String::new();
    };
    let end = [
        " METHOD_FULL_NAME=",
        " SIGNATURE=",
        " ORDER=",
        " ARGUMENT_INDEX=",
        " DISPATCH_TYPE=",
    ]
    .iter()
    .filter_map(|key| tail.find(key))
    .min()
    .unwrap_or(tail.len());
    tail[..end].to_string()
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
            has_code: rest.contains(" CODE="),
            full: extract_full_name(rest),
            has_arg: rest.contains(" ARGUMENT_INDEX="),
            arg_index,
            order: grab(" ORDER=").parse().unwrap_or(0),
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
    expansion_control_kinds: &'a HashMap<String, String>,
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

    // A control construct must compose child edge lists in the pinned
    // CfgCreator order. Detaching a child preserves that order independently
    // of when the mutable AST builder visits it.
    fn detached_build(
        &mut self,
        id: Option<usize>,
    ) -> (Option<String>, Vec<String>, Vec<(String, String)>) {
        let start = self.edges.len();
        let (entry, fringe) = id.map(|id| self.build(id)).unwrap_or_default();
        (entry, fringe, self.edges.split_off(start))
    }

    fn detached_seq(
        &mut self,
        ids: &[usize],
    ) -> (Option<String>, Vec<String>, Vec<(String, String)>) {
        let start = self.edges.len();
        let (entry, fringe) = self.seq(ids);
        (entry, fringe, self.edges.split_off(start))
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
                    let (e1, o1, left_edges) = self.detached_build(Some(kids[0]));
                    let (e2, o2, right_edges) = self.detached_build(Some(kids[1]));
                    if let Some(e2) = &e2 {
                        self.connect(&o1, e2);
                    }
                    self.edges.extend(left_edges);
                    self.edges.extend(right_edges);
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
                _ => {
                    let (entry, outs) = self.seq(&kids);
                    self.connect(&outs, &me);
                    (entry.or(Some(me.clone())), vec![me])
                }
            },
            "BLOCK" => {
                // Expression blocks and standalone compound statements are
                // CFG nodes after their children, including empty blocks.
                // Method/control bodies remain transparent (stub bodies also
                // carry a spurious ARGUMENT_INDEX, so that is not a predicate).
                let is_cfg_block = n
                    .parent
                    .is_some_and(|p| matches!(self.arena[p].label.as_str(), "CALL" | "BLOCK"));
                let (entry, outs) = self.seq(&kids);
                if is_cfg_block {
                    self.connect(&outs, &me);
                    (entry.or(Some(me.clone())), vec![me])
                } else {
                    (entry, outs)
                }
            }
            "RETURN" => {
                let (entry, outs) = self.seq(&kids);
                let mret = self.mret.clone();
                self.edges.push((me.clone(), mret));
                self.connect(&outs, &me);
                (entry.or(Some(me)), vec![])
            }
            "CONTROL_STRUCTURE" => self.build_control(id, me, &kids),
            _ => (None, vec![]), // LOCAL, MODIFIER, METHOD, TYPE_DECL, params, ...
        }
    }

    fn build_control(
        &mut self,
        id: usize,
        me: String,
        kids: &[usize],
    ) -> (Option<String>, Vec<String>) {
        let kind = self
            .expansion_control_kinds
            .get(&me)
            .cloned()
            .unwrap_or_else(|| {
                self.arena[id]
                    .code1
                    .split(['(', ';', ' '])
                    .next()
                    .unwrap_or("")
                    .to_string()
            });
        let block_child = |b: &Self| kids.iter().copied().find(|&c| b.arena[c].label == "BLOCK");
        match kind.as_str() {
            "if" => {
                let (ce, co, condition_edges) = self.detached_build(kids.first().copied());
                let then = block_child(self);
                let els = kids.iter().copied().find(|&c| {
                    self.arena[c].label == "CONTROL_STRUCTURE" && self.arena[c].code1 == "else"
                });
                let else_body = els.and_then(|e| {
                    self.arena[e]
                        .children
                        .iter()
                        .copied()
                        .find(|&c| self.arena[c].label == "BLOCK")
                });
                let (te, to, true_edges) = self.detached_build(then);
                let (ee, eo, false_edges) = self.detached_build(else_body);
                if let Some(target) = &te {
                    self.connect(&co, target);
                }
                if let Some(target) = &ee {
                    self.connect(&co, target);
                }
                self.edges.extend(condition_edges);
                self.edges.extend(true_edges);
                self.edges.extend(false_edges);
                let outs = if te.is_none() && ee.is_none() {
                    co
                } else {
                    let mut fringe = if te.is_some() { to } else { co.clone() };
                    fringe.extend(if ee.is_some() { eo } else { co });
                    fringe
                };
                (ce, outs)
            }
            "while" => {
                self.breaks.push(Vec::new());
                self.continues.push(Vec::new());
                let (ce, co, condition_edges) = self.detached_build(kids.first().copied());
                let body = kids
                    .iter()
                    .copied()
                    .find(|&child| self.arena[child].order == 2);
                let (be, bo, body_edges) = self.detached_build(body);
                let brs = self.breaks.pop().unwrap();
                let conts = self.continues.pop().unwrap();
                if let Some(target) = &be {
                    self.connect(&co, target);
                }
                if let Some(target) = &ce {
                    self.connect(&bo, target);
                    self.connect(&conts, target);
                }
                self.edges.extend(condition_edges);
                self.edges.extend(body_edges);
                let mut outs = co;
                outs.extend(brs);
                (ce, outs)
            }
            "do" => {
                self.breaks.push(Vec::new());
                self.continues.push(Vec::new());
                let body = (kids.len() > 1).then(|| kids[0]);
                let (be, bo, body_edges) = self.detached_build(body);
                let (ce, co, condition_edges) = self.detached_build(kids.last().copied());
                let brs = self.breaks.pop().unwrap();
                let conts = self.continues.pop().unwrap();
                if let Some(target) = &ce {
                    self.connect(&conts, target);
                    self.connect(&bo, target);
                }
                if let Some(target) = be.as_ref().or(ce.as_ref()) {
                    self.connect(&co, target);
                }
                self.edges.extend(body_edges);
                self.edges.extend(condition_edges);
                let mut outs = co;
                outs.extend(brs);
                (be.or(ce), outs)
            }
            "for" => {
                self.breaks.push(Vec::new());
                self.continues.push(Vec::new());
                // Missing clauses leave gaps in ORDER. Initializer declarations
                // may contribute LOCALs plus assignments before those slots.
                let leading_locals = kids
                    .iter()
                    .take_while(|&&child| self.arena[child].label == "LOCAL")
                    .count();
                let initializer_order = leading_locals as i64 + 1;
                let init_children: Vec<_> = kids
                    .iter()
                    .copied()
                    .filter(|&child| self.arena[child].order <= initializer_order)
                    .collect();
                let condition_order = initializer_order + 1;
                let cond = kids
                    .iter()
                    .copied()
                    .find(|&child| self.arena[child].order == condition_order);
                let update = kids
                    .iter()
                    .copied()
                    .find(|&child| self.arena[child].order == condition_order + 1);
                let body = kids
                    .iter()
                    .copied()
                    .find(|&child| self.arena[child].order == condition_order + 2);
                let (ie, io, init_edges) = self.detached_seq(&init_children);
                let (ce, co, condition_edges) = self.detached_build(cond);
                let (ue, uo, update_edges) = self.detached_build(update);
                let (be, bo, body_edges) = self.detached_build(body);
                let inner_entry = be.as_ref().or(ue.as_ref());
                let loop_entry = ce.as_ref().or(inner_entry).cloned();
                let inner_fringe = if ue.is_some() { &uo } else { &bo };
                let brs = self.breaks.pop().unwrap();
                let conts = self.continues.pop().unwrap();
                if let Some(target) = &loop_entry {
                    self.connect(&io, target);
                    self.connect(inner_fringe, target);
                }
                if let Some(target) = inner_entry.or(ce.as_ref()) {
                    self.connect(&co, target);
                }
                if let Some(target) = ue.as_ref().or(loop_entry.as_ref()) {
                    self.connect(&conts, target);
                }
                self.edges.extend(init_edges);
                self.edges.extend(condition_edges);
                self.edges.extend(body_edges);
                self.edges.extend(update_edges);
                if let Some(target) = &ue {
                    self.connect(&bo, target);
                }
                let mut outs = co;
                outs.extend(brs);
                (ie.or(loop_entry), outs)
            }
            "switch" => {
                self.breaks.push(Vec::new());
                let (ce, co, condition_edges) = self.detached_build(kids.first().copied());
                let bkids = block_child(self)
                    .map(|b| self.arena[b].children.clone())
                    .unwrap_or_default();
                let (_, bo, body_edges) = self.detached_seq(&bkids);
                let mut has_default = false;
                for &c in &bkids {
                    if self.arena[c].label == "JUMP_TARGET" {
                        self.connect(&co, &self.addr(c));
                        has_default |= self.arena[c].name == "default";
                    }
                }
                self.edges.extend(condition_edges);
                self.edges.extend(body_edges);
                let mut outs = if has_default { Vec::new() } else { co };
                outs.extend(self.breaks.pop().unwrap());
                outs.extend(bo);
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
fn cfg_edges_for_block(
    block: &str,
    text: &str,
    expansion_control_kinds: &HashMap<String, String>,
) -> Vec<(String, String)> {
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
        expansion_control_kinds,
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
fn cfg_index_edges(
    block: &str,
    text: &str,
    n: usize,
    expansion_control_kinds: &HashMap<String, String>,
) -> Vec<(usize, usize)> {
    let prefix = format!("{block}#");
    let to_idx = |a: &str| -> Option<usize> {
        if a.starts_with("M:") {
            Some(0)
        } else {
            a.strip_prefix(&prefix)
                .and_then(|s| s.parse::<usize>().ok())
        }
    };
    cfg_edges_for_block(block, text, expansion_control_kinds)
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

/// Reaching-definition membership is dense within a method. Packed words
/// avoid storing one hash-table entry per definition in every saved IN/OUT.
/// Trailing zero words are removed so equality remains set equality.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
struct DefinitionSet(Vec<u64>);

impl DefinitionSet {
    fn insert_all(&mut self, values: &[usize]) {
        for &value in values {
            let word = value / 64;
            if word >= self.0.len() {
                self.0.resize(word + 1, 0);
            }
            self.0[word] |= 1u64 << (value % 64);
        }
    }

    fn union_with(&mut self, other: &Self) {
        if other.0.len() > self.0.len() {
            self.0.resize(other.0.len(), 0);
        }
        for (word, &other) in self.0.iter_mut().zip(&other.0) {
            *word |= other;
        }
    }

    fn remove_all(&mut self, values: &[usize]) {
        for &value in values {
            if let Some(word) = self.0.get_mut(value / 64) {
                *word &= !(1u64 << (value % 64));
            }
        }
        while self.0.last() == Some(&0) {
            self.0.pop();
        }
    }

    fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().enumerate().flat_map(|(index, &word)| {
            let mut remaining = word;
            std::iter::from_fn(move || {
                if remaining == 0 {
                    None
                } else {
                    let bit = remaining.trailing_zeros() as usize;
                    remaining &= remaining - 1;
                    Some(index * 64 + bit)
                }
            })
        })
    }
}

/// REACHING_DEF flows for one method block: (variable, srcIdxAddr, dstIdxAddr)
/// where addresses are `block#idx` or `M:full` for the method node.
fn reaching_def_flows(
    block: &str,
    text: &str,
    expansion_control_kinds: &HashMap<String, String>,
) -> Vec<(String, String, String)> {
    let arena = parse_dump_block(text);
    let n = arena.len();
    if n == 0 || arena[0].label != "METHOD" {
        return Vec::new();
    }
    let cfg = cfg_index_edges(block, text, n, expansion_control_kinds);
    let mut successors = vec![Vec::new(); n];
    for &(source, target) in &cfg {
        successors[source].push(target);
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
            .filter(|&k| {
                arena[k].has_arg
                    && arena[k].label != "FIELD_IDENTIFIER"
                    && !(arena[c].inlined && arena[k].label == "BLOCK")
            })
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
    // Definition identity is separate from the edge label: identifier and
    // parameter definitions use NAME, while call definitions use CODE. A C
    // function-pointer initializer has an empty-code LHS, so using node_var
    // here would conflate different pointers and an unnamed void parameter.
    // Keep calls in their own namespace, as ReachingDefTransferFunction's
    // killsForGens looks them up in allCalls rather than allIdentifiers.
    let definition_key = |i: usize| match arena[i].label.as_str() {
        "METHOD_PARAMETER_IN" | "IDENTIFIER" => ("symbol", arena[i].name.as_str()),
        _ => ("call", arena[i].fullcode.as_str()),
    };
    let mut def_var: HashMap<usize, (&str, &str)> = HashMap::new();
    let mut gen: HashMap<usize, Vec<usize>> = HashMap::new(); // node -> defs generated
                                                              // parameters
    let params: Vec<usize> = arena[0]
        .children
        .iter()
        .copied()
        .filter(|&k| arena[k].label == "METHOD_PARAMETER_IN")
        .collect();
    for &p in &params {
        def_var.insert(p, definition_key(p));
        gen.insert(p, vec![p]);
    }
    // GEN/KILL include method-contained calls even when their CFG nodes are
    // unreachable. Their initial OUT is GEN, so a raw predecessor can supply
    // definitions without ever receiving IN or call-site DDG processing.
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
        def_var.insert(c, definition_key(c));
        for a in args_of(c) {
            if is_gen_arg(a) {
                def_var.insert(a, definition_key(a));
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
            // Method.local traverses method-contained BLOCKs and their direct
            // LOCAL children. A FOR initializer's LOCAL is attached to the
            // CONTROL_STRUCTURE and is absent from this optimization's list.
            if own.contains(&i)
                && arena[i].label == "LOCAL"
                && arena[i]
                    .parent
                    .is_some_and(|parent| arena[parent].label == "BLOCK")
            {
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
        let vars: HashSet<(&str, &str)> = gen[&c].iter().map(|d| def_var[d]).collect();
        let g: HashSet<usize> = gen[&c].iter().copied().collect();
        let k: Vec<usize> = def_var
            .iter()
            .filter(|(d, v)| !g.contains(d) && vars.contains(*v))
            .map(|(d, _)| *d)
            .collect();
        kill.insert(c, k);
    }

    // --- ReachingDefFlowGraph and DataFlowSolver (Joern v4.0.555) ---
    let cfg_nodes: HashSet<usize> = cfg.iter().flat_map(|&(s, d)| [s, d]).collect();
    let exit = arena[0]
        .children
        .iter()
        .copied()
        .find(|&i| arena[i].label == "METHOD_RETURN");
    let output_params: Vec<usize> = arena[0]
        .children
        .iter()
        .copied()
        .filter(|&i| arena[i].label == "METHOD_PARAMETER_OUT")
        .collect();
    let first_body = successors[0].first().copied();
    let mut preds = vec![Vec::new(); n];
    for &(source, target) in &cfg {
        preds[target].push(source);
    }
    // ReachingDefFlowGraph selects the first incoming CFG edge. This is the
    // CfgCreator composition order, not AST order or a reachability filter.
    let last_actual = exit.and_then(|e| preds[e].first().copied());

    // Method.reversePostOrder traverses raw cfgNext, which excludes the
    // METHOD_RETURN. Parameters and the exit are added explicitly afterward.
    // Use an iterative DFS with suspended successor positions, as Joern's
    // NodeOrdering does, rather than an AST-order fixed-point sweep.
    let mut postorder = Vec::new();
    let mut visited = vec![false; n];
    let mut stack = vec![(0usize, 0usize)];
    visited[0] = true;
    while let Some((node, next_index)) = stack.last_mut() {
        if *next_index < successors[*node].len() {
            let next = successors[*node][*next_index];
            *next_index += 1;
            if Some(next) != exit && !visited[next] {
                visited[next] = true;
                stack.push((next, 0));
            }
        } else {
            postorder.push(*node);
            stack.pop();
        }
    }
    let mut worklist = vec![0];
    worklist.extend(params.iter().copied());
    worklist.extend(postorder.into_iter().rev().filter(|&i| i != 0));
    worklist.extend(output_params.iter().copied());
    worklist.extend(exit);

    // initPred and initSucc are intentionally asymmetric. A node whose only
    // CFG successor is exit schedules the first output parameter instead;
    // a loop condition with both body and exit successors still schedules
    // exit directly. Output-parameter IN can therefore retain an earlier
    // round even when the loop condition's OUT later changes.
    let mut flow_successors = successors.clone();
    for &i in &worklist {
        if i == 0 {
            flow_successors[i] = params
                .first()
                .copied()
                .map_or_else(|| successors[i].clone(), |first| vec![first]);
        } else if let Some(position) = params.iter().position(|&p| p == i) {
            preds[i] = vec![position.checked_sub(1).map_or(0, |p| params[p])];
            flow_successors[i] = params
                .get(position + 1)
                .copied()
                .map_or_else(|| successors[0].clone(), |next| vec![next]);
        } else if let Some(position) = output_params.iter().position(|&p| p == i) {
            preds[i] = position
                .checked_sub(1)
                .map(|p| output_params[p])
                .or(last_actual)
                .into_iter()
                .collect();
            flow_successors[i] = output_params
                .get(position + 1)
                .copied()
                .or(exit)
                .into_iter()
                .collect();
        } else {
            if Some(i) == first_body {
                // This case precedes exit handling in initPred, including an
                // empty method whose first CFG node is METHOD_RETURN.
                preds[i] = vec![params.last().copied().unwrap_or(0)];
            } else if Some(i) == exit {
                preds[i] = output_params
                    .last()
                    .copied()
                    .or(last_actual)
                    .into_iter()
                    .collect();
            }
            if arena[i].label == "RETURN"
                || (successors[i].len() == 1 && successors[i].first().copied() == exit)
            {
                flow_successors[i] = output_params
                    .first()
                    .copied()
                    .or(exit)
                    .into_iter()
                    .collect();
            }
        }
    }

    let mut out = vec![DefinitionSet::default(); n];
    for (&node, generated) in &gen {
        out[node].insert_all(generated);
    }
    let mut incoming: Vec<Option<DefinitionSet>> = vec![None; n];
    while !worklist.is_empty() {
        let mut next_worklist = Vec::new();
        let mut queued = vec![false; n];
        for i in worklist {
            let mut in_set = DefinitionSet::default();
            for &previous in &preds[i] {
                in_set.union_with(&out[previous]);
            }
            let mut next_out = in_set.clone();
            if let Some(killed) = kill.get(&i) {
                next_out.remove_all(killed);
            }
            if let Some(generated) = gen.get(&i) {
                next_out.insert_all(generated);
            }
            incoming[i] = Some(in_set);
            if next_out != out[i] {
                out[i] = next_out;
                for &next in &flow_successors[i] {
                    if !queued[next] {
                        queued[next] = true;
                        next_worklist.push(next);
                    }
                }
            }
        }
        worklist = next_worklist;
    }
    // DDG routines read the saved IN solution, not unions recomputed from
    // final predecessor OUT. Unscheduled nodes have no entry in this map.
    let empty_in = DefinitionSet::default();
    let in_of = |i: usize| incoming[i].as_ref().unwrap_or(&empty_in);

    // isUsing(use, inElem) — faithful port of UsageAnalyzer.isUsing =
    // sameVariable || isContainer || isPart || isAlias. Joern compares
    // `nodeToString(use)` (NAME for identifier/param, CODE for expression)
    // against a target string via `Option.contains`, i.e. EXACT equality.
    let is_using = |use_i: usize, def_i: usize| -> bool { rd_is_using(&arena, use_i, def_i) };

    let mut flows: Vec<(String, String, String)> = Vec::new();
    // Gate every candidate edge addEdge(from=s, to=d) through
    // isValidEdge(child=d, parent=s), exactly as DdgGenerator.addEdge does.
    let push = |var: String, s: usize, d: usize, flows: &mut Vec<(String, String, String)>| {
        if own.contains(&s)
            && own.contains(&d)
            && arena[s].label != "UNKNOWN"
            && arena[d].label != "UNKNOWN"
            && rd_valid_edge(&arena, d, s)
        {
            flows.push((var, addr(s), addr(d)));
        }
    };

    // isDdgNode: everything EXCEPT Method, ControlStructure, FieldIdentifier,
    // JumpTarget, MethodReturn. A BLOCK is a ddg node only when it is itself a
    // CFG node: an expression block used as a call argument (the comma
    // operator `(b++, b+1)`) or a standalone compound statement. Method and
    // control-body blocks are not CFG nodes and never get def-use edges.
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
                let ds: Vec<usize> = ins.iter().filter(|&d| is_using(u, d)).collect();
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
    // (the call is a reaching def). Calls with arguments have a nonempty
    // UsageAnalyzer use map even when those arguments use no incoming defs.
    // Zero-argument calls get an entry edge; an INLINED expansion BLOCK has
    // no ARGUMENT edge and therefore does not count as an argument here.
    // isValidEdge in push drops write-only targets.
    // `i` is a node id, not just an arena index: it keys `own`, `assign_lhs`,
    // `is_ddg` and `used_incoming` as well.
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        if i == 0
            || incoming[i].is_none()
            || !own.contains(&i)
            || !is_ddg(i)
            || assign_lhs.contains(&i)
        {
            continue;
        }
        if arena[i].label == "CALL" && !args_of(i).is_empty() {
            continue;
        }
        if used_incoming(i).iter().all(|(_, ds)| ds.is_empty()) {
            push(String::new(), 0, i, &mut flows);
        }
    }

    // 2. call sites: only nodes with a computed IN solution.
    for &c in calls.iter().filter(|&&c| incoming[c].is_some()) {
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
        if arena[i].label != "RETURN" || !own.contains(&i) || incoming[i].is_none() {
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
        // External declarations bind mirrored parameters positionally; two
        // unnamed parameters are distinct even though both names are empty.
        let declaration_only = arena[0].fullcode.trim_end().ends_with(';')
            && arena[0]
                .children
                .iter()
                .any(|&child| arena[child].label == "BLOCK" && arena[child].fullcode.is_empty());
        let parameter_index = arena[0]
            .children
            .iter()
            .copied()
            .filter(|&child| arena[child].label == "METHOD_PARAMETER_OUT")
            .position(|child| child == i);
        let pin = if declaration_only {
            parameter_index.and_then(|index| params.get(index))
        } else {
            params.iter().find(|&&p| arena[p].name == arena[i].name)
        };
        if let Some(&pin) = pin {
            push(arena[pin].name.clone(), pin, i, &mut flows);
        }
        if declaration_only {
            continue;
        }
        // addEdgesToMethodParameterOut: usedIncomingDefs(paramOut) — the defs
        // live at method exit (param-out chain) that the paramOut isUsing. The
        // paramOut's own "use" is itself, so the match is isUsing(paramOut, d):
        // e.g. `*l` (indirection of l) is used by the paramOut of l, and `q.x`
        // (a write through q) by the paramOut of q.
        let mut ds: Vec<usize> = in_of(i).iter().filter(|&d| is_using(i, d)).collect();
        ds.sort();
        for d in ds {
            push(node_var(&arena[d]), d, i, &mut flows);
        }
    }

    // 5. exit node: every def in in(exit) -> exit
    if let Some(e) = exit {
        let mut v: Vec<usize> = in_of(e).iter().collect();
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
            // Merely taking a function's address does not capture the
            // caller's locals. The referenced method must be lexically nested
            // in this AST (as definitions are in the file-global wrapper).
            .filter(|full| {
                arena
                    .iter()
                    .skip(1)
                    .any(|node| node.label == "METHOD" && node.full == *full)
            })
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
        // propertiesMap omits an unset CODE, but Joern's Expression.code
        // getter returns PropertyDefaults.Code ("<empty>"). An explicitly
        // empty CODE remains empty, as for function-pointer initializers.
        _ => Some(if arena[i].has_code {
            arena[i].fullcode.clone()
        } else {
            "<empty>".to_string()
        }),
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
// DefaultSemantics.operatorFlows uses an explicit PassThroughMapping for
// these operators. Unlike absent semantics, it forbids cross-argument flow.
fn rd_passthrough_operator(name: &str) -> bool {
    matches!(
        name,
        "<operator>.modulo"
            | "<operator>.arrayInitializer"
            | "<operator>.tupleLiteral"
            | "<operator>.dictLiteral"
            | "<operator>.setLiteral"
            | "<operator>.listLiteral"
    )
}
fn rd_sem(arena: &[DNode], c: usize) -> Option<Vec<(i64, i64)>> {
    if arena[c].label != "CALL" {
        return None;
    }
    if rd_passthrough_operator(&arena[c].name) {
        // PTF supports unbounded arity and excludes the receiver at index 0.
        return Some(
            rd_args(arena, c)
                .into_iter()
                .map(|argument| arena[argument].arg_index)
                .filter(|&index| index != 0)
                .flat_map(|index| [(index, index), (index, -1)])
                .collect(),
        );
    }
    operator_semantics(&arena[c].name)
}
fn rd_is_call_retval(arena: &[DNode], node: usize) -> bool {
    if arena[node].label != "CALL" || rd_passthrough_operator(&arena[node].name) {
        // PassThroughMapping explicitly permits return flow even at arity 0.
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
mod preproc_tests {
    use super::*;

    #[test]
    fn macro_expansion_bounds_empty_doubling_chains_and_cycles() {
        let mut definitions = HashMap::from([("EMPTY0".to_string(), String::new())]);
        for index in 1..40 {
            definitions.insert(
                format!("EMPTY{index}"),
                format!("EMPTY{} EMPTY{}", index - 1, index - 1),
            );
        }
        let mut budget = 128;
        let expansion =
            expand_preproc_objects("EMPTY39", &definitions, &mut HashSet::new(), &mut budget);
        assert_eq!(budget, 0);
        assert!(expansion.len() <= 128);

        let cycle = HashMap::from([
            ("A".to_string(), "B".to_string()),
            ("B".to_string(), "A".to_string()),
        ]);
        let mut budget = 128;
        assert_eq!(
            expand_preproc_objects("A", &cycle, &mut HashSet::new(), &mut budget),
            "A"
        );
        assert!(budget > 0);
    }
}

#[cfg(test)]
mod dump_property_tests {
    use super::parse_dump_block;

    #[test]
    fn full_names_end_at_properties_or_end_of_line() {
        for (line, expected) in [
            (
                "METHOD NAME=VALUE CODE=#define VALUE 2U FULL_NAME=a b.h:VALUE:unsigned int(0) SIGNATURE=unsigned int(0) ORDER=1",
                "a b.h:VALUE:unsigned int(0)",
            ),
            (
                "TYPE_DECL NAME=<global> FULL_NAME=a b.h:<global> ORDER=1",
                "a b.h:<global>",
            ),
            (
                "METHOD NAME=VALUE FULL_NAME=a X=b.h:VALUE:unsigned longint(1)",
                "a X=b.h:VALUE:unsigned longint(1)",
            ),
            ("METHOD NAME=ordinary FULL_NAME=ordinary ORDER=1", "ordinary"),
            (
                "METHOD CODE=int invoke(int x){ /* FULL_NAME=invoke */ return x; } FULL_NAME=invoke SIGNATURE=int(int) ORDER=1",
                "invoke",
            ),
            ("METHOD FULL_NAME= ORDER=1", ""),
            ("CALL METHOD_FULL_NAME=other", ""),
            ("BLOCK ORDER=1", ""),
        ] {
            assert_eq!(parse_dump_block(line)[0].full, expected, "{line}");
        }
        for property in [
            "METHOD_FULL_NAME=next",
            "SIGNATURE=unsigned int(1)",
            "ORDER=1",
            "ARGUMENT_INDEX=2",
            "DISPATCH_TYPE=STATIC_DISPATCH",
        ] {
            let line = format!("METHOD FULL_NAME=é space x=value {property}");
            assert_eq!(parse_dump_block(&line)[0].full, "é space x=value");
        }
    }
}

#[cfg(test)]
mod rd_semantics_tests {
    use super::*;

    #[test]
    fn pass_through_validation_uses_actual_arguments_without_sibling_flow() {
        for operator in [
            "<operator>.modulo",
            "<operator>.arrayInitializer",
            "<operator>.tupleLiteral",
            "<operator>.dictLiteral",
            "<operator>.setLiteral",
            "<operator>.listLiteral",
        ] {
            let arena = parse_dump_block(&format!(
                "CALL NAME={operator} CODE=initializer ORDER=1\n\
                 \x20\x20LITERAL CODE=receiver ARGUMENT_INDEX=0 ORDER=0\n\
                 \x20\x20LITERAL CODE=first ARGUMENT_INDEX=1 ORDER=1\n\
                 \x20\x20LITERAL CODE=third ARGUMENT_INDEX=3 ORDER=3\n\
                 \x20\x20LITERAL CODE=many ARGUMENT_INDEX=40 ORDER=40\n"
            ));
            assert!(!rd_is_used(&arena, 1), "{operator}: receiver is not input");
            assert!(
                !rd_is_defined(&arena, 1),
                "{operator}: receiver is not output"
            );
            for argument in 2..arena.len() {
                assert!(rd_is_used(&arena, argument), "{operator}: {argument}");
                assert!(rd_is_defined(&arena, argument), "{operator}: {argument}");
                assert!(rd_valid_edge(&arena, argument, argument));
                assert!(rd_valid_edge(&arena, 0, argument));
                for sibling in 2..arena.len() {
                    if sibling != argument {
                        assert!(!rd_valid_edge(&arena, sibling, argument));
                    }
                }
            }
            let empty = parse_dump_block(&format!("CALL NAME={operator} CODE={{}} ORDER=1"));
            assert!(!rd_is_call_retval(&empty, 0), "{operator}: empty call");
        }
    }

    #[test]
    fn absent_expression_code_does_not_alias_explicit_empty_names() {
        let arena = parse_dump_block(
            "METHOD NAME=f FULL_NAME=f\n\
             \x20\x20METHOD_PARAMETER_IN NAME= CODE=void ORDER=1\n\
             \x20\x20BLOCK ORDER=2\n\
             \x20\x20IDENTIFIER NAME= CODE= ARGUMENT_INDEX=1 ORDER=3\n\
             \x20\x20BLOCK CODE= ORDER=4\n",
        );
        assert_eq!(rd_node_str(&arena, 2).as_deref(), Some("<empty>"));
        assert!(!rd_is_using(&arena, 2, 1));
        assert_eq!(rd_node_str(&arena, 3).as_deref(), Some(""));
        assert!(rd_is_using(&arena, 3, 1));
        assert_eq!(rd_node_str(&arena, 4).as_deref(), Some(""));
        assert!(rd_is_using(&arena, 4, 1));
        assert_eq!(rd_node_str(&arena, 0), None);
    }
}

#[cfg(test)]
mod definition_set_tests {
    use super::DefinitionSet;
    use std::collections::BTreeSet;

    #[test]
    fn membership_and_equality_match_sets_through_union_kill_and_regeneration() {
        let mut packed = DefinitionSet::default();
        let mut reference = BTreeSet::new();
        for round in 0..64 {
            let generated: Vec<_> = (0..67).map(|i| (i * 127 + round * 19) % 4097).collect();
            let killed: Vec<_> = (0..83).map(|i| (i * 63 + round * 131) % 4097).collect();
            let mut predecessor = DefinitionSet::default();
            predecessor.insert_all(&generated);
            packed.union_with(&predecessor);
            reference.extend(&generated);
            packed.remove_all(&killed);
            reference.retain(|value| !killed.contains(value));
            let actual: Vec<_> = packed.iter().collect();
            assert_eq!(actual, reference.iter().copied().collect::<Vec<_>>());
            let mut rebuilt = DefinitionSet::default();
            rebuilt.insert_all(&actual);
            assert_eq!(packed, rebuilt);
        }
        let all: Vec<_> = reference.into_iter().collect();
        packed.remove_all(&all);
        assert_eq!(packed, DefinitionSet::default());
        packed.insert_all(&[0, 63, 64, 65, 4096]);
        packed.remove_all(&[4096, 65, 64]);
        let mut compact = DefinitionSet::default();
        compact.insert_all(&[63, 0, 63]);
        assert_eq!(packed, compact);
    }
}
