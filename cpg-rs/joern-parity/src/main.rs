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
    let path = std::env::args().nth(1).expect("usage: joern-parity <file.c>");
    let src = std::fs::read_to_string(&path).expect("read file");
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_c::LANGUAGE.into()).unwrap();
    let tree = parser.parse(&src, None).unwrap();
    let bytes = src.as_bytes();

    // Pass 1: index free functions (name -> return type) and file-level
    // object declarations (globals: name -> type), skipping prototypes.
    let mut functions: HashMap<String, String> = HashMap::new();
    let mut globals: HashMap<String, String> = HashMap::new();
    for f in named_children(tree.root_node()) {
        match f.kind() {
            "function_definition" => {
                if let Some((name, ret, _)) = fn_header(f, bytes) {
                    functions.insert(name, ret);
                }
            }
            "declaration" => {
                let base = normalize_type(
                    &f.child_by_field_name("type").map(|t| text(t, bytes).to_string()).unwrap_or("ANY".into()),
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
                            let name = innermost_id(decl, bytes);
                            if !name.is_empty() {
                                globals.insert(name, format!("{base}{}", decl_suffix(decl)));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Pass 2: emit each function's canonical AST, sorted by name (as the oracle).
    let mut funcs: Vec<Node> = named_children(tree.root_node())
        .into_iter()
        .filter(|f| f.kind() == "function_definition")
        .collect();
    funcs.sort_by_key(|f| fn_header(*f, bytes).map(|h| h.0).unwrap_or_default());

    let mut out = String::new();
    for f in funcs {
        let mut ctx = Ctx { functions: &functions, globals: &globals, symbols: HashMap::new(), phantoms: Vec::new(), out: &mut out };
        ctx.emit_method(f, bytes);
        out.push('\n');
    }
    print!("{out}");
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
    out: &'a mut String,
}

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
    fn line(&mut self, depth: usize, label: &str, p: P) {
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

    fn emit_method(&mut self, f: Node, b: &[u8]) {
        let (name, ret, params) = fn_header(f, b).expect("function header");
        let sig = format!(
            "{ret}({})",
            params.iter().map(|p| p.ty.clone()).collect::<Vec<_>>().join(",")
        );
        self.line(0, "METHOD", P {
            name: Some(name.clone()),
            code: Some(esc(text(f, b))),
            full: Some(name.clone()),
            sig: Some(sig),
            order: Some(1),
            ..Default::default()
        });

        // Parameters: a METHOD_PARAMETER_IN and a mirrored _OUT, sharing ORDER.
        for (i, p) in params.iter().enumerate() {
            self.symbols.insert(p.name.clone(), p.ty.clone());
            let order = (i + 1) as i64;
            for label in ["METHOD_PARAMETER_IN", "METHOD_PARAMETER_OUT"] {
                self.line(1, label, P {
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
            self.emit_block(body, b, block_order, 1);
        }
        self.line(1, "METHOD_RETURN", P {
            code: Some("RET".into()),
            tfn: Some(ret),
            order: Some((params.len() + 2) as i64),
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
                self.line(depth, "CONTROL_STRUCTURE", P {
                    code: Some(esc(text(n, b))),
                    order: Some(o),
                    ..Default::default()
                });
                if let Some(cond) = n.child_by_field_name("condition") {
                    self.emit_expr(unwrap_paren(cond), b, depth + 1, 1, None);
                }
                if let Some(body) = n.child_by_field_name("body") {
                    self.emit_block(body, b, 2, depth + 1);
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
                if let Some(body) = n.child_by_field_name("body") {
                    if body.kind() == "compound_statement" {
                        self.emit_block(body, b, 1, depth + 1);
                    }
                }
                if let Some(cond) = n.child_by_field_name("condition") {
                    self.emit_expr(unwrap_paren(cond), b, depth + 1, 2, None);
                }
            }
            "while_statement" => {
                let o = *order;
                *order += 1;
                let cond = n.child_by_field_name("condition");
                // c2cpg quirk: a while's CODE is just `while <cond>`, not the body.
                let code = cond.map(|c| format!("while {}", text(c, b))).unwrap_or("while".into());
                self.line(depth, "CONTROL_STRUCTURE", P {
                    code: Some(esc(&code)),
                    order: Some(o),
                    ..Default::default()
                });
                if let Some(c) = cond {
                    self.emit_expr(unwrap_paren(c), b, depth + 1, 1, None);
                }
                if let Some(body) = n.child_by_field_name("body") {
                    if body.kind() == "compound_statement" {
                        self.emit_block(body, b, 2, depth + 1);
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
        self.line(depth, "CONTROL_STRUCTURE", P {
            code: Some(esc(text(n, b))),
            order: Some(o),
            ..Default::default()
        });
        if let Some(cond) = n.child_by_field_name("condition") {
            self.emit_expr(unwrap_paren(cond), b, depth + 1, 1, None);
        }
        if let Some(cons) = n.child_by_field_name("consequence") {
            if cons.kind() == "compound_statement" {
                self.emit_block(cons, b, 2, depth + 1);
            }
        }
        if let Some(alt) = n.child_by_field_name("alternative") {
            self.line(depth + 1, "CONTROL_STRUCTURE", P {
                code: Some("else".into()),
                order: Some(3),
                ..Default::default()
            });
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
        self.line(depth, "CONTROL_STRUCTURE", P {
            code: Some(esc(&format!("for ({};{};{})", part(init), part(cond), part(update)))),
            order: Some(o),
            ..Default::default()
        });
        let mut co = 1i64;
        if let Some(i) = init {
            if i.kind() == "declaration" {
                self.emit_declaration(i, b, &mut co, depth + 1, Some(1));
            } else {
                self.emit_expr(i, b, depth + 1, co, Some(1));
                co += 1;
            }
        }
        if let Some(c) = cond {
            self.emit_expr(c, b, depth + 1, co, None);
            co += 1;
        }
        if let Some(u) = update {
            self.emit_expr(u, b, depth + 1, co, None);
            co += 1;
        }
        if let Some(body) = n.child_by_field_name("body") {
            if body.kind() == "compound_statement" {
                self.emit_block(body, b, co, depth + 1);
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
                    let full_ty = format!("{ty}{}", name_node.map(decl_suffix).unwrap_or_default());
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
                    let full_ty = format!("{ty}{}", decl_suffix(d));
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
                let ty = format!("{base}{}", decl.map(decl_suffix).unwrap_or_default());
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
fn decl_suffix(n: Node) -> String {
    let mut s = String::new();
    let mut cur = n;
    loop {
        match cur.kind() {
            "pointer_declarator" => s.push('*'),
            "array_declarator" => s.push_str("[]"),
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
