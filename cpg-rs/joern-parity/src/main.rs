//! A pure-Rust C frontend whose output is driven to byte-for-byte parity with
//! Joern's `c2cpg`, verified by differential testing against a real Joern
//! install (see joern-parity/README.md). It reproduces Joern/x2cpg conventions:
//! operators lowered to `<operator>.*` CALL nodes, a declaration split into a
//! LOCAL plus an `<operator>.assignment` CALL, a synthetic METHOD_RETURN and
//! mirrored METHOD_PARAMETER_OUT nodes, ORDER/ARGUMENT_INDEX sequencing, and
//! simple type resolution — all emitted in Joern's canonical AST-dump format so
//! the output diffs cleanly against the oracle.

use std::collections::HashMap;
use tree_sitter::{Node, Parser};

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: joern-parity <file.c>...");
        std::process::exit(2);
    }
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_c::LANGUAGE.into()).unwrap();

    struct Unit {
        file: String,
        src: String,
        tree: tree_sitter::Tree,
    }
    let units: Vec<Unit> = paths
        .iter()
        .map(|p| {
            let src = std::fs::read_to_string(p).expect("read file");
            let tree = parser.parse(&src, None).unwrap();
            let file = std::path::Path::new(p)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            Unit { file, src, tree }
        })
        .collect();

    // Project-wide registries: defined functions (calls to these never become
    // stubs; each also gets a method TYPE_DECL) and struct definitions.
    let mut defined: Vec<String> = Vec::new();
    let mut fn_decls: Vec<(String, String)> = Vec::new(); // (name, file)
    let mut struct_decls: Vec<(String, String, String)> = Vec::new(); // (tag, code, file)
    for u in &units {
        let b = u.src.as_bytes();
        for f in named_children(u.tree.root_node()) {
            match f.kind() {
                "function_definition" => {
                    if let Some((name, _, _)) = fn_header(f, b) {
                        defined.push(name.clone());
                        fn_decls.push((name, u.file.clone()));
                    }
                }
                "struct_specifier" if f.child_by_field_name("body").is_some() => {
                    let tag = f.child_by_field_name("name").map(|x| text(x, b).to_string()).unwrap_or_default();
                    struct_decls.push((tag, esc(text(f, b)), u.file.clone()));
                }
                _ => {}
            }
        }
    }

    // Each dump is one method subtree keyed by FULL_NAME; Joern's oracle sorts
    // all methods (user, <global> wrappers, <operator> stubs) by fullName.
    let mut dumps: Vec<(String, String)> = Vec::new();
    let mut stub_uses: HashMap<String, usize> = HashMap::new();
    let mut used_types: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut edges: Vec<(String, String, String)> = Vec::new();
    let mut placements: HashMap<String, Vec<(String, usize)>> = HashMap::new();

    for u in &units {
        let b = u.src.as_bytes();
        let root = u.tree.root_node();

        // Per-file tables (c2cpg resolves within the translation unit).
        let mut functions: HashMap<String, String> = HashMap::new();
        let mut globals: HashMap<String, String> = HashMap::new();
        for f in named_children(root) {
            match f.kind() {
                "function_definition" => {
                    if let Some((name, ret, _)) = fn_header(f, b) {
                        functions.insert(name, ret);
                    }
                }
                "declaration" => {
                    let base = normalize_type(
                        &f.child_by_field_name("type").map(|t| text(t, b).to_string()).unwrap_or("ANY".into()),
                    );
                    for d in named_children(f) {
                        let decl = if d.kind() == "init_declarator" {
                            d.child_by_field_name("declarator")
                        } else if matches!(d.kind(), "identifier" | "pointer_declarator" | "array_declarator") {
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
            globals: &globals,
            symbols: HashMap::new(),
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
        for f in named_children(root) {
            match f.kind() {
                "function_definition" => {
                    if let Some((name, _, _)) = fn_header(f, b) {
                        ctx.begin_block(&name);
                        ctx.emit_method(f, b, 0);
                        ctx.edge("SOURCE_FILE", format!("M:{name}"), format!("F:{}", u.file));
                        dumps.push((name, std::mem::take(&mut ctx.out)));
                    }
                }
                "struct_specifier" if struct_has_sized_array(f) => {
                    let tag = f.child_by_field_name("name").map(|x| text(x, b).to_string()).unwrap_or_default();
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
    let empty_globals: HashMap<String, String> = HashMap::new();
    let mut stub_uses2: HashMap<String, usize> = HashMap::new();
    let mut sctx = Ctx {
        functions: &empty_fns,
        globals: &empty_globals,
        symbols: HashMap::new(),
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
    sctx.begin_block("<includes>:<global>");
    sctx.emit_includes_global();
    sctx.edge("SOURCE_FILE", "M:<includes>:<global>".into(), "F:<includes>".into());
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
    out.push_str("NODES|NAMESPACE_BLOCK NAME=<global> FULL_NAME=<global> FILENAME=<unknown> ORDER=1\n");
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
        tds.push((format!("{f}:<global>"), format!(
            "NODES|TYPE_DECL NAME=<global> FULL_NAME={f}:<global> CODE=<global> AST_PARENT_TYPE=NAMESPACE_BLOCK AST_PARENT_FULL_NAME={f}:<global> FILENAME={f} ORDER=1\n"
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
        out.push_str(&format!("NODES|TYPE NAME={t} FULL_NAME={t} TYPE_DECL_FULL_NAME={t}\n"));
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
    edges.push(("REF".into(), "NB:<includes>:<global>".into(), "NS:<global>".into()));
    edges.push(("SOURCE_FILE".into(), "NB:<global>".into(), "F:<unknown>".into()));
    edges.push(("SOURCE_FILE".into(), "NB:<includes>:<global>".into(), "F:<includes>".into()));
    for f in &files {
        edges.push(("REF".into(), format!("NB:{f}:<global>"), "NS:<global>".into()));
        edges.push(("SOURCE_FILE".into(), format!("NB:{f}:<global>"), format!("F:{f}")));
    }
    // The per-file <global> TYPE_DECL CONTAINS the file-global METHOD and the
    // method TYPE_DECLs of that file; each FILE contains its <global>
    // TYPE_DECL, and <includes> contains its method + external TYPE_DECLs.
    for f in &files {
        edges.push(("CONTAINS".into(), format!("D:{f}:<global>"), format!("M:{f}:<global>")));
        edges.push(("CONTAINS".into(), format!("F:{f}"), format!("D:{f}:<global>")));
    }
    for (name, file) in &fn_decls {
        edges.push(("CONTAINS".into(), format!("D:{file}:<global>"), format!("D:{name}")));
    }
    edges.push(("CONTAINS".into(), "F:<includes>".into(), "M:<includes>:<global>".into()));
    for t in &used_types {
        if !struct_tags.contains(&t) && !defined.contains(t) {
            edges.push(("CONTAINS".into(), "F:<includes>".into(), format!("D:{t}")));
        }
    }
    // SOURCE_FILE for the TYPE_DECL population.
    for (tag, _, file) in &struct_decls {
        edges.push(("SOURCE_FILE".into(), format!("TD:{tag}"), format!("F:{file}")));
    }
    for (name, file) in &fn_decls {
        edges.push(("SOURCE_FILE".into(), format!("D:{name}"), format!("F:{file}")));
    }
    for f in &files {
        edges.push(("SOURCE_FILE".into(), format!("D:{f}:<global>"), format!("F:{f}")));
    }
    for t in &used_types {
        if !struct_tags.contains(&t) && !defined.contains(t) {
            edges.push(("SOURCE_FILE".into(), format!("D:{t}"), "F:<includes>".into()));
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
    print!("{out}");
}

fn count_members(n: Node) -> i64 {
    let Some(body) = n.child_by_field_name("body") else { return 0 };
    named_children(body)
        .iter()
        .filter(|f| f.kind() == "field_declaration")
        .map(|f| {
            named_children(*f)
                .iter()
                .filter(|d| matches!(d.kind(), "field_identifier" | "pointer_declarator" | "array_declarator"))
                .count() as i64
        })
        .sum()
}

/// Per-function emission context.
struct Ctx<'a> {
    functions: &'a HashMap<String, String>,
    globals: &'a HashMap<String, String>,
    symbols: HashMap<String, String>, // local/param name -> type
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
    parent_stack: Vec<(usize, String, String)>, // (depth, label, addr) for ARGUMENT
    sym_line: HashMap<String, usize>, // LOCAL/param name -> defining line idx
    param_in_line: HashMap<String, usize>,
    edges: &'a mut Vec<(String, String, String)>,
    placements: &'a mut HashMap<String, Vec<(String, usize)>>,
}

/// AST labels that receive a CONTAINS edge from their enclosing method or
/// type-decl context (Joern's ContainsEdgePass destination list — note that
/// LOCAL, parameters, METHOD_RETURN, MODIFIER and MEMBER are absent).
const CONTAINS_DST: [&str; 13] = [
    "BLOCK", "IDENTIFIER", "FIELD_IDENTIFIER", "RETURN", "METHOD", "TYPE_DECL",
    "CALL", "LITERAL", "METHOD_REF", "TYPE_REF", "CONTROL_STRUCTURE",
    "JUMP_TARGET", "UNKNOWN",
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
        let suppressed = self.suppress_below.map_or(false, |nd| depth > nd);
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
                let key = if label == "METHOD" { format!("M:{f}") } else { format!("TD:{f}") };
                self.placements.entry(key).or_default().push((self.block.clone(), idx));
            }
            if CONTAINS_DST.contains(&label) {
                if let Some((_, src)) = self.ctx_stack.last() {
                    let src = src.clone();
                    self.edges.push(("CONTAINS".into(), src, my_addr.clone()));
                }
            }
            if let Some((_, plabel, paddr)) = self.parent_stack.last() {
                if plabel == "CALL" || plabel == "RETURN" {
                    let paddr = paddr.clone();
                    self.edges.push(("ARGUMENT".into(), paddr, my_addr.clone()));
                }
            }
            if let Some(t) = &p.tfn {
                self.edges.push(("EVAL_TYPE".into(), my_addr.clone(), format!("T:{t}")));
            }
            if label == "CALL" {
                if let Some(mfn) = &p.mfn {
                    self.edges.push(("CALL".into(), my_addr.clone(), format!("M:{mfn}")));
                }
            }
            if label == "METHOD_REF" {
                if let Some(mfn) = &p.mfn {
                    self.edges.push(("REF".into(), my_addr.clone(), format!("M:{mfn}")));
                }
            }
            if label == "IDENTIFIER" {
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
                    self.edges.push(("PARAMETER_LINK".into(), src, my_addr.clone()));
                }
            }
        }
        if label == "METHOD" || label == "TYPE_DECL" {
            self.ctx_stack.push((depth, my_addr.clone()));
        }
        self.parent_stack.push((depth, label.to_string(), my_addr));
        self.line_no += 1;
        let mut s = format!("{}{label}", "  ".repeat(depth));
        let mut kv = |k: &str, v: &str| s.push_str(&format!(" {k}={v}"));
        if let Some(v) = &p.name { kv("NAME", v); }
        if let Some(v) = &p.code { kv("CODE", v); }
        if let Some(v) = &p.tfn { kv("TYPE_FULL_NAME", v); }
        if let Some(v) = &p.full { kv("FULL_NAME", v); }
        if let Some(v) = &p.mfn { kv("METHOD_FULL_NAME", v); }
        if let Some(v) = &p.sig { kv("SIGNATURE", v); }
        if let Some(v) = p.order { kv("ORDER", &v.to_string()); }
        if let Some(v) = p.arg { kv("ARGUMENT_INDEX", &v.to_string()); }
        if let Some(v) = &p.dispatch { kv("DISPATCH_TYPE", v); }
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
        let nested = d > 0;
        let sig = format!(
            "{ret}({})",
            params.iter().map(|p| p.ty.clone()).collect::<Vec<_>>().join(",")
        );
        self.line(d, "METHOD", P {
            name: Some(name.clone()),
            code: Some(esc(text(f, b))),
            full: Some(name.clone()),
            sig: Some(sig),
            order: Some(1),
            ..Default::default()
        });
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
                self.line(d + 1, label, P {
                    name: Some(p.name.clone()),
                    code: Some(esc(&p.code)),
                    tfn: Some(p.ty.clone()),
                    order: Some(order),
                    ..Default::default()
                });
            }
        }

        // Body block, then the synthetic method return.
        let block_order = (params.len() + 1) as i64;
        if let Some(body) = f.child_by_field_name("body") {
            self.collect_phantoms(body, b);
            self.emit_block(body, b, block_order, d + 1);
        }
        self.line(d + 1, "METHOD_RETURN", P {
            code: Some("RET".into()),
            tfn: Some(ret),
            order: Some((params.len() + 2) as i64),
            ..Default::default()
        });
    }

    /// A `<operator>.*` stub method. Layout mirrors Joern's stable sort by
    /// ORDER over insertion order [IN p1..pn, BLOCK(1), RET(2), OUT p1..pn].
    fn emit_stub(&mut self, name: &str, arity: usize) {
        self.line(0, "METHOD", P {
            name: Some(name.into()),
            full: Some(name.into()),
            order: Some(0),
            ..Default::default()
        });
        let pin = |k: usize| P {
            name: Some(format!("p{k}")),
            code: Some(format!("p{k}")),
            tfn: Some("ANY".into()),
            order: Some(k as i64),
            ..Default::default()
        };
        self.line(1, "METHOD_PARAMETER_IN", pin(1));
        self.line(1, "BLOCK", P {
            tfn: Some("ANY".into()),
            order: Some(1),
            arg: Some(1),
            ..Default::default()
        });
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

    fn emit_includes_global(&mut self) {
        self.line(0, "METHOD", P {
            name: Some("<global>".into()),
            code: Some("<global>".into()),
            full: Some("<includes>:<global>".into()),
            order: Some(1),
            ..Default::default()
        });
        self.line(1, "BLOCK", P {
            tfn: Some("ANY".into()),
            order: Some(1),
            ..Default::default()
        });
        self.line(1, "METHOD_RETURN", P {
            code: Some("RET".into()),
            tfn: Some("ANY".into()),
            order: Some(2),
            ..Default::default()
        });
    }

    /// The per-file `<global>` wrapper: TYPE_DECLs and nested METHOD dumps in
    /// source order (each ORDER=1), then a BLOCK holding one slot per
    /// top-level construct in source order — TYPE_REF for a struct def, LOCAL
    /// per global object declarator, METHOD_REF per function definition;
    /// prototypes consume no slot — then METHOD_RETURN.
    fn emit_file_global(&mut self, root: Node, b: &[u8], file: &str) {
        self.line(0, "METHOD", P {
            name: Some("<global>".into()),
            code: Some("<global>".into()),
            full: Some(format!("{file}:<global>")),
            order: Some(1),
            ..Default::default()
        });
        for n in named_children(root) {
            match n.kind() {
                "struct_specifier" if n.child_by_field_name("body").is_some() => {
                    self.emit_type_decl(n, b, 1);
                }
                "function_definition" => self.emit_method(n, b, 1),
                _ => {}
            }
        }
        self.line(1, "BLOCK", P {
            tfn: Some("ANY".into()),
            order: Some(1),
            ..Default::default()
        });
        let mut slot = 1i64;
        for n in named_children(root) {
            match n.kind() {
                "struct_specifier" if n.child_by_field_name("body").is_some() => {
                    let name = n.child_by_field_name("name").map(|x| text(x, b).to_string()).unwrap_or_default();
                    self.line(2, "TYPE_REF", P {
                        code: Some(esc(text(n, b))),
                        tfn: Some(name),
                        order: Some(slot),
                        ..Default::default()
                    });
                    slot += 1;
                }
                "declaration" => {
                    // Same lowering as in a method body: LOCAL per declarator,
                    // plus an assignment CALL when initialised (`int g = 5;`).
                    // Prototypes contribute nothing and consume no slot.
                    if named_children(n).iter().any(|d| find_function_declarator(*d).is_some()) {
                        continue;
                    }
                    self.emit_declaration(n, b, &mut slot, 2, None);
                }
                "function_definition" => {
                    if let Some((name, _, _)) = fn_header(n, b) {
                        self.line(2, "METHOD_REF", P {
                            code: Some(name.clone()),
                            tfn: Some(name.clone()),
                            mfn: Some(name),
                            order: Some(slot),
                            ..Default::default()
                        });
                        slot += 1;
                    }
                }
                _ => {}
            }
        }
        self.line(1, "METHOD_RETURN", P {
            code: Some("RET".into()),
            tfn: Some("ANY".into()),
            order: Some(2),
            ..Default::default()
        });
    }

    /// `struct T { ... }` → TYPE_DECL with one MEMBER per field (CODE is the
    /// member's declarator text: `x`, `*ptr`, `arr[4]`). If any member is a
    /// sized array, a `<clinit>` method follows the members to host the
    /// `<operator>.arrayInitializer` calls.
    fn emit_type_decl(&mut self, n: Node, b: &[u8], depth: usize) {
        let name = n.child_by_field_name("name").map(|x| text(x, b).to_string()).unwrap_or_default();
        self.line(depth, "TYPE_DECL", P {
            name: Some(name.clone()),
            code: Some(esc(text(n, b))),
            full: Some(name.clone()),
            order: Some(1),
            ..Default::default()
        });
        let Some(body) = n.child_by_field_name("body") else { return };
        let mut order = 1i64;
        for f in named_children(body) {
            if f.kind() != "field_declaration" {
                continue;
            }
            let ty = normalize_type(
                &f.child_by_field_name("type").map(|t| text(t, b).to_string()).unwrap_or("ANY".into()),
            );
            for d in named_children(f) {
                if matches!(d.kind(), "field_identifier" | "pointer_declarator" | "array_declarator") {
                    let mname = innermost_id(d, b);
                    let key = format!("MB:{name}.{mname}");
                    let placement = (self.block.clone(), self.line_no);
                    self.placements.entry(key).or_default().push(placement);
                    self.line(depth + 1, "MEMBER", P {
                        name: Some(mname),
                        code: Some(text(d, b).to_string()),
                        tfn: Some(format!("{ty}{}", decl_suffix(d, b))),
                        order: Some(order),
                        ..Default::default()
                    });
                    order += 1;
                }
            }
        }
        if struct_has_sized_array(n) {
            self.emit_clinit(n, b, depth + 1, order);
        }
    }

    /// The synthetic `<clinit>` static initialiser Joern adds to a struct with
    /// sized-array members: a property-less BLOCK holding one
    /// `<operator>.arrayInitializer` call per sized array, two bare MODIFIERs,
    /// and a METHOD_RETURN typed as the struct.
    fn emit_clinit(&mut self, n: Node, b: &[u8], depth: usize, order: i64) {
        let tag = n.child_by_field_name("name").map(|x| text(x, b).to_string()).unwrap_or_default();
        self.line(depth, "METHOD", P {
            name: Some("<clinit>".into()),
            code: Some("<clinit>".into()),
            full: Some(format!("{tag}.<clinit>:{tag}()")),
            order: Some(order),
            ..Default::default()
        });
        if depth > 0 {
            self.suppress_below = Some(depth);
        }
        self.line(depth + 1, "BLOCK", P { order: Some(1), ..Default::default() });
        let mut co = 1i64;
        if let Some(body) = n.child_by_field_name("body") {
            for f in named_children(body) {
                if f.kind() != "field_declaration" {
                    continue;
                }
                for d in named_children(f) {
                    if d.kind() == "array_declarator" {
                        if let Some(sz) = d.child_by_field_name("size") {
                            self.note_call("<operator>.arrayInitializer", 1);
                            self.line(depth + 2, "CALL", P {
                                name: Some("<operator>.arrayInitializer".into()),
                                code: Some(esc(text(d, b))),
                                tfn: Some("ANY".into()),
                                mfn: Some("<operator>.arrayInitializer".into()),
                                order: Some(co),
                                dispatch: Some("STATIC_DISPATCH".into()),
                                ..Default::default()
                            });
                            self.emit_expr(sz, b, depth + 3, 1, Some(1));
                            co += 1;
                        }
                    }
                }
            }
        }
        self.line(depth + 1, "MODIFIER", P { order: Some(2), ..Default::default() });
        self.line(depth + 1, "MODIFIER", P { order: Some(3), ..Default::default() });
        self.line(depth + 1, "METHOD_RETURN", P {
            code: Some("RET".into()),
            tfn: Some(tag),
            order: Some(4),
            ..Default::default()
        });
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
                        }
                    }
                }
                "sizeof_expression" => {
                    if let Some(t) = n.child_by_field_name("type") {
                        let ts = text(t, b).to_string();
                        if !seen.contains(&ts) {
                            seen.push(ts.clone());
                            self.phantoms.push(Phantom { name: ts.clone(), code: ts.clone(), ty: ts });
                        }
                    }
                }
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
        self.line(depth, "BLOCK", P {
            code: Some(esc(text(body, b))),
            tfn: Some("void".into()),
            order: Some(order),
            ..Default::default()
        });
        for ph in std::mem::take(&mut self.phantoms) {
            self.line(depth + 1, "LOCAL", P {
                name: Some(ph.name),
                code: Some(ph.code),
                tfn: Some(ph.ty),
                order: Some(0),
                ..Default::default()
            });
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
            "break_statement" | "continue_statement" => {
                let o = *order;
                *order += 1;
                self.line(depth, "CONTROL_STRUCTURE", P {
                    code: Some(esc(text(n, b))),
                    order: Some(o),
                    ..Default::default()
                });
            }
            "switch_statement" => {
                let o = *order;
                *order += 1;
                let cs = self.line_no;
                self.line(depth, "CONTROL_STRUCTURE", P {
                    code: Some(esc(text(n, b))),
                    order: Some(o),
                    ..Default::default()
                });
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
                self.line(depth, "JUMP_TARGET", P {
                    name: Some(name.into()),
                    code: Some(esc(&code)),
                    order: Some(o),
                    ..Default::default()
                });
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
                self.line(depth, "CONTROL_STRUCTURE", P {
                    code: Some(esc(text(n, b))),
                    order: Some(o),
                    ..Default::default()
                });
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
                let code = cond.map(|c| format!("while {}", text(c, b))).unwrap_or("while".into());
                self.line(depth, "CONTROL_STRUCTURE", P {
                    code: Some(esc(&code)),
                    order: Some(o),
                    ..Default::default()
                });
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
                self.line(depth, "RETURN", P {
                    code: Some(esc(text(n, b))),
                    order: Some(o),
                    ..Default::default()
                });
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
        self.line(depth, "CONTROL_STRUCTURE", P {
            code: Some(esc(text(n, b))),
            order: Some(o),
            ..Default::default()
        });
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
            self.line(depth + 1, "CONTROL_STRUCTURE", P {
                code: Some("else".into()),
                order: Some(3),
                ..Default::default()
            });
            self.edge("FALSE_BODY", self.at(cs), self.at(ei));
            if let Some(body) = named_children(alt).into_iter().find(|c| c.kind() == "compound_statement") {
                self.emit_block(body, b, 1, depth + 2);
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
            x.map(|c| text(c, b).trim_end_matches(';').trim().to_string()).unwrap_or_default()
        };
        let o = *order;
        *order += 1;
        let cs = self.line_no;
        self.line(depth, "CONTROL_STRUCTURE", P {
            code: Some(esc(&format!("for ({};{};{})", part(init), part(cond), part(update)))),
            order: Some(o),
            ..Default::default()
        });
        let mut co = 1i64;
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
    fn emit_declaration(&mut self, n: Node, b: &[u8], order: &mut i64, depth: usize, assign_arg: Option<i64>) {
        let ty = normalize_type(&n.child_by_field_name("type").map(|t| text(t, b).to_string()).unwrap_or("ANY".into()));
        // LOCAL CODE is rebuilt per declarator: the decl-specifier source text
        // (keeps `const`/`struct`/`unsigned ...` spellings the type drops)
        // plus that declarator alone — so `int a, b = 1;` yields `int a`,`int b`.
        let spec_end = n.child_by_field_name("type").map(|t| t.end_byte()).unwrap_or(n.start_byte());
        let decl_code = |d: Node| {
            let spec = std::str::from_utf8(&b[n.start_byte()..spec_end]).unwrap_or("");
            esc(&format!("{spec} {}", text(d, b)))
        };
        for d in named_children(n) {
            match d.kind() {
                "init_declarator" => {
                    let name_node = d.child_by_field_name("declarator");
                    let name = name_node.map(|x| innermost_id(x, b)).unwrap_or_default();
                    // LOCAL CODE keeps the source declarator form (`int *q`),
                    // while TYPE_FULL_NAME normalises pointers to `int*`.
                    let full_ty = format!("{ty}{}", name_node.map(|x| decl_suffix(x, b)).unwrap_or_default());
                    self.symbols.insert(name.clone(), full_ty.clone());
                    let lo = *order;
                    *order += 1;
                    self.line(depth, "LOCAL", P {
                        name: Some(name.clone()),
                        code: name_node.map(decl_code),
                        tfn: Some(full_ty.clone()),
                        order: Some(lo),
                        ..Default::default()
                    });
                    // The assignment call.
                    let ao = *order;
                    *order += 1;
                    self.note_call("<operator>.assignment", 2);
                    self.line(depth, "CALL", P {
                        name: Some("<operator>.assignment".into()),
                        code: Some(esc(text(d, b))),
                        tfn: Some("void".into()),
                        mfn: Some("<operator>.assignment".into()),
                        order: Some(ao),
                        arg: assign_arg,
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    });
                    // lhs identifier (arg 1), rhs value (arg 2).
                    self.line(depth + 1, "IDENTIFIER", P {
                        name: Some(name.clone()),
                        code: Some(name.clone()),
                        tfn: Some(full_ty),
                        order: Some(1),
                        arg: Some(1),
                        ..Default::default()
                    });
                    if let Some(v) = d.child_by_field_name("value") {
                        self.emit_expr(v, b, depth + 1, 2, Some(2));
                    }
                }
                "identifier" | "pointer_declarator" | "array_declarator" => {
                    let name = innermost_id(d, b);
                    let full_ty = format!("{ty}{}", decl_suffix(d, b));
                    self.symbols.insert(name.clone(), full_ty.clone());
                    let lo = *order;
                    *order += 1;
                    self.line(depth, "LOCAL", P {
                        name: Some(name.clone()),
                        code: Some(decl_code(d)),
                        tfn: Some(full_ty),
                        order: Some(lo),
                        ..Default::default()
                    });
                }
                _ => {}
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
                self.line(depth, "CALL", P {
                    name: Some(name.clone()),
                    code: Some(esc(text(n, b))),
                    tfn: Some("ANY".into()),
                    mfn: Some(name),
                    order: Some(order),
                    arg,
                    dispatch: Some("STATIC_DISPATCH".into()),
                    ..Default::default()
                });
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
                let op = n.child_by_field_name("operator").map(|o| text(o, b)).unwrap_or("=");
                let name = assignment_name(op);
                self.note_call(&name, 2);
                self.line(depth, "CALL", P {
                    name: Some(name.clone()),
                    code: Some(esc(text(n, b))),
                    tfn: Some("ANY".into()),
                    mfn: Some(name),
                    order: Some(order),
                    arg,
                    dispatch: Some("STATIC_DISPATCH".into()),
                    ..Default::default()
                });
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
                self.line(depth, "CALL", P {
                    name: Some(name.clone()),
                    code: Some(esc(text(n, b))),
                    tfn: Some("ANY".into()),
                    mfn: Some(name),
                    order: Some(order),
                    arg,
                    dispatch: Some("STATIC_DISPATCH".into()),
                    ..Default::default()
                });
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
                self.line(depth, "CALL", P {
                    name: Some(name.clone()),
                    code: Some(esc(text(n, b))),
                    tfn: Some("ANY".into()),
                    mfn: Some(name),
                    order: Some(order),
                    arg,
                    dispatch: Some("STATIC_DISPATCH".into()),
                    ..Default::default()
                });
                if let Some(a) = arg_node {
                    self.emit_expr(a, b, depth + 1, 1, Some(1));
                }
            }
            "conditional_expression" => {
                self.note_call("<operator>.conditional", 3);
                self.line(depth, "CALL", P {
                    name: Some("<operator>.conditional".into()),
                    code: Some(esc(text(n, b))),
                    tfn: Some("ANY".into()),
                    mfn: Some("<operator>.conditional".into()),
                    order: Some(order),
                    arg,
                    dispatch: Some("STATIC_DISPATCH".into()),
                    ..Default::default()
                });
                for (i, field) in ["condition", "consequence", "alternative"].iter().enumerate() {
                    if let Some(c) = n.child_by_field_name(*field) {
                        let k = (i + 1) as i64;
                        self.emit_expr(c, b, depth + 1, k, Some(k));
                    }
                }
            }
            "field_expression" => {
                // `.` → fieldAccess, `->` → indirectFieldAccess; the member is
                // a FIELD_IDENTIFIER child with CODE only (no NAME).
                let op = n.child_by_field_name("operator").map(|o| text(o, b)).unwrap_or(".");
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
                self.line(depth, "CALL", P {
                    name: Some(name.clone()),
                    code: Some(esc(text(n, b))),
                    tfn: Some("ANY".into()),
                    mfn: Some(name),
                    order: Some(order),
                    arg,
                    dispatch: Some("STATIC_DISPATCH".into()),
                    ..Default::default()
                });
                if let Some(a) = n.child_by_field_name("argument") {
                    self.emit_expr(a, b, depth + 1, 1, Some(1));
                }
                if let Some(f) = n.child_by_field_name("field") {
                    self.line(depth + 1, "FIELD_IDENTIFIER", P {
                        code: Some(text(f, b).to_string()),
                        order: Some(2),
                        arg: Some(2),
                        ..Default::default()
                    });
                }
            }
            "subscript_expression" => {
                self.note_call("<operator>.indirectIndexAccess", 2);
                self.line(depth, "CALL", P {
                    name: Some("<operator>.indirectIndexAccess".into()),
                    code: Some(esc(text(n, b))),
                    tfn: Some("ANY".into()),
                    mfn: Some("<operator>.indirectIndexAccess".into()),
                    order: Some(order),
                    arg,
                    dispatch: Some("STATIC_DISPATCH".into()),
                    ..Default::default()
                });
                if let Some(a) = n.child_by_field_name("argument") {
                    self.emit_expr(a, b, depth + 1, 1, Some(1));
                }
                if let Some(i) = n.child_by_field_name("index") {
                    self.emit_expr(i, b, depth + 1, 2, Some(2));
                }
            }
            "call_expression" => {
                let callee = n.child_by_field_name("function");
                let name = callee.map(|c| text(c, b).to_string()).unwrap_or("<anon>".into());
                let ty = self.functions.get(&name).cloned().unwrap_or("ANY".into());
                let argc = n
                    .child_by_field_name("arguments")
                    .map(|a| named_children(a).len())
                    .unwrap_or(0);
                self.note_call(&name, argc);
                self.line(depth, "CALL", P {
                    name: Some(name.clone()),
                    code: Some(esc(text(n, b))),
                    tfn: Some(ty),
                    mfn: Some(name),
                    order: Some(order),
                    arg,
                    dispatch: Some("STATIC_DISPATCH".into()),
                    ..Default::default()
                });
                if let Some(args) = n.child_by_field_name("arguments") {
                    for (i, a) in named_children(args).into_iter().enumerate() {
                        let k = (i + 1) as i64;
                        self.emit_expr(a, b, depth + 1, k, Some(k));
                    }
                }
            }
            "identifier" => {
                let name = text(n, b).to_string();
                let (code, ty) = if let Some(t) = self.symbols.get(&name) {
                    (name.clone(), t.clone())
                } else if let Some(t) = self.globals.get(&name) {
                    (format!("<global> {name}"), t.clone())
                } else {
                    (name.clone(), "ANY".to_string())
                };
                self.line(depth, "IDENTIFIER", P {
                    name: Some(name),
                    code: Some(code),
                    tfn: Some(ty),
                    order: Some(order),
                    arg,
                    ..Default::default()
                });
            }
            "number_literal" => {
                // tree-sitter folds a leading sign into the literal; Joern
                // (CDT) lowers `-1` to <operator>.minus applied to `1`.
                let t = text(n, b).to_string();
                if let Some(rest) = t.strip_prefix('-').or_else(|| t.strip_prefix('+')) {
                    let name = unary_name(&t[..1]);
                    self.note_call(&name, 1);
                    self.line(depth, "CALL", P {
                        name: Some(name.clone()),
                        code: Some(t.clone()),
                        tfn: Some("ANY".into()),
                        mfn: Some(name),
                        order: Some(order),
                        arg,
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    });
                    self.line(depth + 1, "LITERAL", P {
                        code: Some(rest.to_string()),
                        tfn: Some("int".into()),
                        order: Some(1),
                        arg: Some(1),
                        ..Default::default()
                    });
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
                    self.line(depth, "LITERAL", P {
                        code: Some(t),
                        tfn: Some(tfn.into()),
                        order: Some(order),
                        arg,
                        ..Default::default()
                    });
                }
            }
            "char_literal" => {
                self.line(depth, "LITERAL", P {
                    code: Some(text(n, b).to_string()),
                    tfn: Some("char".into()),
                    order: Some(order),
                    arg,
                    ..Default::default()
                });
            }
            "string_literal" => {
                self.line(depth, "LITERAL", P {
                    code: Some(esc(text(n, b))),
                    tfn: Some("char*".into()),
                    order: Some(order),
                    arg,
                    ..Default::default()
                });
            }
            "cast_expression" => {
                // `(T)e` → <operator>.cast typed T, with a TYPE_REF arg 1.
                let ty = n
                    .child_by_field_name("type")
                    .map(|t| normalize_type(text(t, b)))
                    .unwrap_or("ANY".into());
                self.note_call("<operator>.cast", 2);
                self.line(depth, "CALL", P {
                    name: Some("<operator>.cast".into()),
                    code: Some(esc(text(n, b))),
                    tfn: Some(ty.clone()),
                    mfn: Some("<operator>.cast".into()),
                    order: Some(order),
                    arg,
                    dispatch: Some("STATIC_DISPATCH".into()),
                    ..Default::default()
                });
                self.line(depth + 1, "TYPE_REF", P {
                    code: Some(ty.clone()),
                    tfn: Some(ty),
                    order: Some(1),
                    arg: Some(1),
                    ..Default::default()
                });
                if let Some(v) = n.child_by_field_name("value") {
                    self.emit_expr(v, b, depth + 1, 2, Some(2));
                }
            }
            "sizeof_expression" => {
                self.note_call("<operator>.sizeOf", 1);
                self.line(depth, "CALL", P {
                    name: Some("<operator>.sizeOf".into()),
                    code: Some(esc(text(n, b))),
                    tfn: Some("ANY".into()),
                    mfn: Some("<operator>.sizeOf".into()),
                    order: Some(order),
                    arg,
                    dispatch: Some("STATIC_DISPATCH".into()),
                    ..Default::default()
                });
                if let Some(t) = n.child_by_field_name("type") {
                    // sizeof(T): the type name appears as an IDENTIFIER typed
                    // as itself (and spawns the ORDER=0 phantom LOCAL).
                    let ts = text(t, b).to_string();
                    self.line(depth + 1, "IDENTIFIER", P {
                        name: Some(ts.clone()),
                        code: Some(ts.clone()),
                        tfn: Some(ts),
                        order: Some(1),
                        arg: Some(1),
                        ..Default::default()
                    });
                } else if let Some(v) = n.child_by_field_name("value") {
                    self.emit_expr(unwrap_paren(v), b, depth + 1, 1, Some(1));
                }
            }
            "comma_expression" => {
                // `(a, b)` → a CODE-less BLOCK typed ANY whose children carry
                // ORDER but no ARGUMENT_INDEX.
                self.line(depth, "BLOCK", P {
                    tfn: Some("ANY".into()),
                    order: Some(order),
                    arg,
                    ..Default::default()
                });
                let mut parts = Vec::new();
                flatten_comma(n, &mut parts);
                for (i, part) in parts.into_iter().enumerate() {
                    self.emit_expr(part, b, depth + 1, (i + 1) as i64, None);
                }
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
    let ret = f.child_by_field_name("type").map(|t| text(t, b).to_string()).unwrap_or("ANY".into());
    let decl = f.child_by_field_name("declarator")?;
    let fd = find_function_declarator(decl)?;
    let name = fd.child_by_field_name("declarator").map(|d| innermost_id(d, b))?;
    let mut params = Vec::new();
    if let Some(pl) = fd.child_by_field_name("parameters") {
        for p in named_children(pl) {
            if p.kind() == "parameter_declaration" {
                let base = normalize_type(&p.child_by_field_name("type").map(|t| text(t, b).to_string()).unwrap_or("ANY".into()));
                let decl = p.child_by_field_name("declarator");
                let ty = format!("{base}{}", decl.map(|d| decl_suffix(d, b)).unwrap_or_default());
                let name = decl.map(|d| innermost_id(d, b)).unwrap_or_default();
                if !name.is_empty() {
                    params.push(Param { name, ty, code: text(p, b).to_string() });
                }
            }
        }
    }
    Some((name, ret, params))
}

fn find_function_declarator(n: Node) -> Option<Node> {
    if n.kind() == "function_declarator" {
        return Some(n);
    }
    named_children(n).into_iter().find_map(find_function_declarator)
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
    let mut s = String::new();
    let mut cur = n;
    loop {
        match cur.kind() {
            "pointer_declarator" => s.push('*'),
            "array_declarator" => {
                // CDT keeps the size: `int arr[4]` types as `int[4]`.
                let size = cur.child_by_field_name("size");
                s.push('[');
                if let Some(sz) = size {
                    s.push_str(text(sz, b));
                }
                s.push(']');
            }
            _ => break,
        }
        match cur.child_by_field_name("declarator") {
            Some(c) => cur = c,
            None => break,
        }
    }
    s
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
fn normalize_type(base: &str) -> String {
    let t = base.trim();
    for tag in ["struct ", "union ", "enum "] {
        if let Some(rest) = t.strip_prefix(tag) {
            return rest.trim().into();
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

fn struct_has_sized_array(n: Node) -> bool {
    let Some(body) = n.child_by_field_name("body") else { return false };
    named_children(body).iter().any(|f| {
        f.kind() == "field_declaration"
            && named_children(*f).iter().any(|d| {
                d.kind() == "array_declarator" && d.child_by_field_name("size").is_some()
            })
    })
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
    full: String,
    has_arg: bool,
    children: Vec<usize>,
    parent: Option<usize>,
    idx: usize,
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
                .map(|i| rest[i + key.len()..].split(' ').next().unwrap_or("").to_string())
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
        arena.push(DNode {
            label,
            name: grab(" NAME="),
            code1: grab(" CODE="),
            full: grab(" FULL_NAME="),
            has_arg: rest.contains(" ARGUMENT_INDEX="),
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
    breaks: Vec<Vec<String>>,    // per breakable construct
    continues: Vec<Vec<String>>, // per loop
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
                _ => {
                    let (entry, outs) = self.seq(&kids);
                    self.connect(&outs, &me);
                    (entry.or(Some(me.clone())), vec![me])
                }
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

    fn build_control(&mut self, id: usize, me: String, kids: &[usize]) -> (Option<String>, Vec<String>) {
        let kind = self.arena[id].code1.split(['(', ';', ' ']).next().unwrap_or("").to_string();
        let block_child = |b: &Self| kids.iter().copied().find(|&c| b.arena[c].label == "BLOCK");
        match kind.as_str() {
            "if" => {
                let (ce, co) = self.build(kids[0]);
                let then = block_child(self);
                let els = kids
                    .iter()
                    .copied()
                    .find(|&c| self.arena[c].label == "CONTROL_STRUCTURE" && self.arena[c].code1 == "else");
                let mut outs = Vec::new();
                if let Some(t) = then {
                    let (te, to) = self.build(t);
                    if let Some(te) = &te {
                        self.connect(&co, te);
                    }
                    outs.extend(to);
                }
                if let Some(e) = els {
                    let eb = self.arena[e].children.iter().copied().find(|&c| self.arena[c].label == "BLOCK");
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
                let cond = kids.iter().copied().find(|&c| self.arena[c].label != "BLOCK");
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
                let body = block_child(self);
                // init = the ARGUMENT_INDEX=1 expression; the remaining two
                // expression children in order are condition and update.
                let init = kids.iter().copied().find(|&c| self.arena[c].has_arg && self.arena[c].label != "BLOCK");
                let rest: Vec<usize> = kids
                    .iter()
                    .copied()
                    .filter(|&c| {
                        Some(c) != init
                            && Some(c) != body
                            && !matches!(self.arena[c].label.as_str(), "LOCAL" | "BLOCK")
                    })
                    .collect();
                let (cond, update) = (rest.first().copied(), rest.get(1).copied());
                let (ie, io) = init.map(|i| self.build(i)).unwrap_or((None, vec![]));
                let (ce, co) = cond.map(|c| self.build(c)).unwrap_or((None, vec![]));
                let (ue, uo) = update.map(|u| self.build(u)).unwrap_or((None, vec![]));
                let (be, bo) = body.map(|b| self.build(b)).unwrap_or((None, vec![]));
                if let Some(ce) = &ce {
                    self.connect(&io, ce);
                    self.connect(&uo, ce);
                }
                if let Some(be) = &be {
                    self.connect(&co, be);
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
        .find(|&c| arena[c].label == "METHOD_RETURN" );
    let body = root_kids.iter().copied().find(|&c| arena[c].label == "BLOCK");
    let Some(mret) = mret else { return Vec::new() };
    let mut b = CfgBuilder {
        arena: &arena,
        block,
        mret: format!("{block}#{}", arena[mret].idx),
        edges: Vec::new(),
        breaks: Vec::new(),
        continues: Vec::new(),
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
