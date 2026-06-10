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

    // Pass 1: index free functions by name -> return type (for call typing).
    let mut functions: HashMap<String, String> = HashMap::new();
    for f in named_children(tree.root_node()) {
        if f.kind() == "function_definition" {
            if let Some((name, ret, _)) = fn_header(f, bytes) {
                functions.insert(name, ret);
            }
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
        let mut ctx = Ctx { functions: &functions, symbols: HashMap::new(), out: &mut out };
        ctx.emit_method(f, bytes);
        out.push('\n');
    }
    print!("{out}");
}

/// Per-function emission context.
struct Ctx<'a> {
    functions: &'a HashMap<String, String>,
    symbols: HashMap<String, String>, // local/param name -> type
    out: &'a mut String,
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
            self.emit_block(body, b, block_order, 1);
        }
        self.line(1, "METHOD_RETURN", P {
            code: Some("RET".into()),
            tfn: Some(ret),
            order: Some((params.len() + 2) as i64),
            ..Default::default()
        });
    }

    /// Emit a BLOCK node and its statements, with a fresh child ORDER sequence.
    fn emit_block(&mut self, body: Node, b: &[u8], order: i64, depth: usize) {
        self.line(depth, "BLOCK", P {
            code: Some(esc(text(body, b))),
            tfn: Some("void".into()),
            order: Some(order),
            ..Default::default()
        });
        let mut so = 1i64;
        for s in named_children(body) {
            self.emit_stmt(s, b, &mut so, depth + 1);
        }
    }

    /// A block-level statement. `order` is the running 1-based child position.
    fn emit_stmt(&mut self, n: Node, b: &[u8], order: &mut i64, depth: usize) {
        match n.kind() {
            "declaration" => self.emit_declaration(n, b, order, depth),
            "if_statement" => self.emit_if(n, b, order, depth),
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

    /// A C declaration `T x = init;` → a LOCAL plus, if initialised, an
    /// `<operator>.assignment` CALL — exactly as c2cpg lowers it.
    fn emit_declaration(&mut self, n: Node, b: &[u8], order: &mut i64, depth: usize) {
        let ty = n.child_by_field_name("type").map(|t| text(t, b).to_string()).unwrap_or("ANY".into());
        for d in named_children(n) {
            match d.kind() {
                "init_declarator" => {
                    let name_node = d.child_by_field_name("declarator");
                    let name = name_node.map(|x| innermost_id(x, b)).unwrap_or_default();
                    self.symbols.insert(name.clone(), ty.clone());
                    let lo = *order;
                    *order += 1;
                    self.line(depth, "LOCAL", P {
                        name: Some(name.clone()),
                        code: Some(esc(&format!("{ty} {name}"))),
                        tfn: Some(ty.clone()),
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
                        dispatch: Some("STATIC_DISPATCH".into()),
                        ..Default::default()
                    });
                    // lhs identifier (arg 1), rhs value (arg 2).
                    self.line(depth + 1, "IDENTIFIER", P {
                        name: Some(name.clone()),
                        code: Some(name.clone()),
                        tfn: Some(ty.clone()),
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
                    self.symbols.insert(name.clone(), ty.clone());
                    let lo = *order;
                    *order += 1;
                    self.line(depth, "LOCAL", P {
                        name: Some(name.clone()),
                        code: Some(esc(&format!("{ty} {name}"))),
                        tfn: Some(ty.clone()),
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
                let ty = self.symbols.get(&name).cloned().unwrap_or("ANY".into());
                self.line(depth, "IDENTIFIER", P {
                    name: Some(name.clone()),
                    code: Some(name),
                    tfn: Some(ty),
                    order: Some(order),
                    arg,
                    ..Default::default()
                });
            }
            "number_literal" => {
                self.line(depth, "LITERAL", P {
                    code: Some(text(n, b).to_string()),
                    tfn: Some("int".into()),
                    order: Some(order),
                    arg,
                    ..Default::default()
                });
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
                let ty = p.child_by_field_name("type").map(|t| text(t, b).to_string()).unwrap_or("ANY".into());
                let name = p
                    .child_by_field_name("declarator")
                    .map(|d| innermost_id(d, b))
                    .unwrap_or_default();
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

/// Assignment operator → Joern operator name (`=` → assignment, `+=` → …).
fn assignment_name(op: &str) -> String {
    let n = match op {
        "=" => "assignment",
        "+=" => "assignmentPlus",
        "-=" => "assignmentMinus",
        "*=" => "assignmentMultiplication",
        "/=" => "assignmentDivision",
        "%=" => "assignmentModulo",
        "&=" => "assignmentAnd",
        "|=" => "assignmentOr",
        "^=" => "assignmentXor",
        "<<=" => "assignmentShiftLeft",
        ">>=" => "assignmentArithmeticShiftRight",
        _ => "assignment",
    };
    format!("<operator>.{n}")
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
