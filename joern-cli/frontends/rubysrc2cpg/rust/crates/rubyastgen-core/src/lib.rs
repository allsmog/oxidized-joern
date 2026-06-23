use anyhow::{Context, Result};
use lib_ruby_parser::source::DecodedInput;
use lib_ruby_parser::{Loc, LocExt, Node, Parser, ParserOptions};
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

thread_local! {
    /// Tally of `lib-ruby-parser` `Node` variants that fell through to the
    /// `__unknown` fallback, keyed by the parser-gem node name (`str_type`).
    /// Accumulates across every file processed in a single CLI run.
    static UNKNOWN_NODES: RefCell<BTreeMap<&'static str, usize>> =
        const { RefCell::new(BTreeMap::new()) };
}

fn record_unknown_node(node: &Node) {
    let name = node.str_type();
    UNKNOWN_NODES.with(|counts| {
        *counts.borrow_mut().entry(name).or_insert(0) += 1;
    });
}

/// Drains the unmapped-node tally and returns a one-line human summary, or
/// `None` if every node parsed in this run was mapped. Callers (the CLI) should
/// print this to stderr exactly once at the end of a run; it must never reach
/// stdout or the emitted JSON.
pub fn take_unknown_node_summary() -> Option<String> {
    UNKNOWN_NODES.with(|counts| {
        let counts = std::mem::take(&mut *counts.borrow_mut());
        if counts.is_empty() {
            return None;
        }
        let total: usize = counts.values().sum();
        let details = counts
            .iter()
            .map(|(name, count)| format!("{name}(x{count})"))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!("rubyastgen: {total} unmapped node(s): {details}"))
    })
}

pub fn generate_file(path: &Path, input_root: &Path) -> Result<Value> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let full_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let rel_path = relative_path(path, input_root);
    if is_erb_file(path) {
        let source = String::from_utf8_lossy(&bytes);
        let lowered = lower_erb_template(&source);
        let mut json = generate_source_inner(lowered.as_bytes(), &full_path, &rel_path, true)?;
        clear_locations(&mut json);
        Ok(json)
    } else {
        generate_source(&bytes, &full_path, &rel_path)
    }
}

pub fn generate_source(bytes: &[u8], full_path: &Path, rel_path: &Path) -> Result<Value> {
    generate_source_inner(bytes, full_path, rel_path, false)
}

fn generate_source_inner(
    bytes: &[u8],
    full_path: &Path,
    rel_path: &Path,
    fallback_on_diagnostics: bool,
) -> Result<Value> {
    let parser = Parser::new(
        bytes.to_vec(),
        ParserOptions {
            buffer_name: full_path.to_string_lossy().to_string(),
            record_tokens: false,
            ..ParserOptions::default()
        },
    );
    let result = parser.do_parse();
    if fallback_on_diagnostics && !result.diagnostics.is_empty() {
        return generate_source_inner("\"#{nil}\"".as_bytes(), full_path, rel_path, false);
    }
    let input = result.input;
    let root_loc = root_loc(result.ast.as_deref(), &input);
    let body = result
        .ast
        .as_deref()
        .map(|node| match node {
            Node::Begin(begin) => begin
                .statements
                .iter()
                .map(|stmt| lower_node(stmt, &input))
                .collect(),
            _ => vec![lower_node(node, &input)],
        })
        .unwrap_or_default();

    let mut root = object("begin", &root_loc, &input, [("body", Value::Array(body))]);
    root.as_object_mut().expect("root object").insert(
        "file_path".to_string(),
        Value::String(full_path.to_string_lossy().to_string()),
    );
    root.as_object_mut().expect("root object").insert(
        "rel_file_path".to_string(),
        Value::String(rel_path.to_string_lossy().to_string()),
    );
    Ok(root)
}

fn is_erb_file(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("erb"))
}

#[derive(Debug)]
struct ErbOutputBlock {
    lambda_name: String,
    params: Vec<String>,
}

fn lower_erb_template(source: &str) -> String {
    let mut lowerer = ErbLowerer::default();
    lowerer.lower(source)
}

#[derive(Default)]
struct ErbLowerer {
    lowered: String,
    output_blocks: Vec<ErbOutputBlock>,
    lambda_counter: usize,
}

impl ErbLowerer {
    fn lower(&mut self, source: &str) -> String {
        self.emit_line("self.joernBuffer = \"\"");

        let mut rest = source;
        while let Some(open) = rest.find("<%") {
            let (static_text, after_open) = rest.split_at(open);
            self.emit_static_text(static_text);

            let after_open = &after_open[2..];
            let Some(close) = after_open.find("%>") else {
                self.emit_static_text(after_open);
                return std::mem::take(&mut self.lowered);
            };

            let (tag, after_tag) = after_open.split_at(close);
            self.emit_tag(tag.trim());
            rest = &after_tag[2..];
        }
        self.emit_static_text(rest);
        std::mem::take(&mut self.lowered)
    }

    fn emit_tag(&mut self, tag: &str) {
        if tag.is_empty() || tag.starts_with('#') {
            return;
        }
        if let Some(expr) = tag.strip_prefix("==") {
            self.emit_output_expression(expr.trim(), "joernTemplateOutRaw");
        } else if let Some(expr) = tag.strip_prefix('=') {
            self.emit_output_expression(expr.trim(), "joernTemplateOutEscape");
        } else {
            self.emit_code(tag);
        }
    }

    fn emit_code(&mut self, code: &str) {
        if code == "end" && !self.output_blocks.is_empty() {
            self.close_output_block();
        } else {
            self.emit_line(code);
        }
    }

    fn emit_output_expression(&mut self, expr: &str, helper: &str) {
        if let Some((body, condition)) = split_conditional_output(expr) {
            self.emit_line(&format!("if {condition}"));
            self.emit_output_expression(body, helper);
            self.emit_line("end");
            return;
        }

        if let Some((call_expr, params)) = split_output_block(expr) {
            self.emit_append_expr(&normalize_rails_call(call_expr));
            let lambda_name = format!("rails_lambda_{}", self.lambda_counter);
            self.lambda_counter += 1;
            self.emit_line(&format!(
                "{lambda_name} = lambda do |{}|",
                params.join(", ")
            ));
            self.emit_line("joernInnerBuffer = \"\"");
            self.output_blocks.push(ErbOutputBlock {
                lambda_name,
                params,
            });
            return;
        }

        self.emit_append_expr(&format!("{helper}({expr})"));
    }

    fn close_output_block(&mut self) {
        let Some(block) = self.output_blocks.pop() else {
            return;
        };
        self.emit_line("joernInnerBuffer");
        self.emit_line("end");
        self.emit_append_expr(&format!(
            "{}.call({})",
            block.lambda_name,
            block.params.join(", ")
        ));
    }

    fn emit_static_text(&mut self, text: &str) {
        let normalized = normalize_static_text(text);
        if !normalized.is_empty() {
            self.emit_append_expr(&ruby_string_literal(&normalized));
        }
    }

    fn emit_append_expr(&mut self, expr: &str) {
        let buffer = self.current_buffer();
        self.emit_line(&format!("self.joernBufferAppend({buffer}, {expr})"));
    }

    fn current_buffer(&self) -> &'static str {
        if self.output_blocks.is_empty() {
            "self.joernBuffer"
        } else {
            "joernInnerBuffer"
        }
    }

    fn emit_line(&mut self, line: &str) {
        self.lowered.push_str(line);
        self.lowered.push('\n');
    }
}

fn split_conditional_output(expr: &str) -> Option<(&str, &str)> {
    let (body, condition) = expr.rsplit_once(" if ")?;
    if body.trim().is_empty() || condition.trim().is_empty() {
        None
    } else {
        Some((body.trim(), condition.trim()))
    }
}

fn split_output_block(expr: &str) -> Option<(&str, Vec<String>)> {
    if let Some((call, params)) = expr.rsplit_once(" do |") {
        let params = params.strip_suffix('|')?;
        let params = params
            .split(',')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        Some((call.trim(), params))
    } else {
        expr.strip_suffix(" do")
            .map(|call| (call.trim(), Vec::<String>::new()))
    }
}

fn normalize_rails_call(expr: &str) -> String {
    let expr = expr.trim();
    if expr.contains('(') || expr.contains('.') || expr.contains("::") {
        return expr.to_string();
    }
    let Some((name, args)) = expr.split_once(char::is_whitespace) else {
        return expr.to_string();
    };
    if name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '!' || c == '?')
    {
        format!("{}({})", name, args.trim())
    } else {
        expr.to_string()
    }
}

fn normalize_static_text(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn ruby_string_literal(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization")
}

fn clear_locations(value: &mut Value) {
    match value {
        Value::Object(obj) => {
            if let Some(Value::Object(meta)) = obj.get_mut("meta_data") {
                meta.insert("start_line".to_string(), json!(-1));
                meta.insert("start_column".to_string(), json!(-1));
                meta.insert("end_line".to_string(), json!(-1));
                meta.insert("end_column".to_string(), json!(-1));
            }
            for child in obj.values_mut() {
                clear_locations(child);
            }
        }
        Value::Array(values) => values.iter_mut().for_each(clear_locations),
        _ => {}
    }
}

fn relative_path(path: &Path, input_root: &Path) -> PathBuf {
    let canonical_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let canonical_root = fs::canonicalize(input_root).unwrap_or_else(|_| input_root.to_path_buf());
    if canonical_root.is_file() {
        canonical_path
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| canonical_path.clone())
    } else {
        canonical_path
            .strip_prefix(&canonical_root)
            .map(PathBuf::from)
            .unwrap_or(canonical_path)
    }
}

fn root_loc(ast: Option<&Node>, input: &DecodedInput) -> Loc {
    ast.map(|node| *node.expression()).unwrap_or(Loc {
        begin: 0,
        end: input.as_shared_bytes().len(),
    })
}

fn lower_node(node: &Node, input: &DecodedInput) -> Value {
    match node {
        Node::Alias(n) => object(
            "alias",
            &n.expression_l,
            input,
            [
                ("name", lower_node(&n.from, input)),
                ("alias", lower_node(&n.to, input)),
            ],
        ),
        Node::And(n) => binary_named("and", &n.lhs, &n.rhs, &n.expression_l, input),
        Node::AndAsgn(n) => object(
            "and_asgn",
            &n.expression_l,
            input,
            [
                ("lhs", lower_node(&n.recv, input)),
                ("rhs", lower_node(&n.value, input)),
            ],
        ),
        Node::Arg(n) => object("arg", &n.expression_l, input, [("value", json!(n.name))]),
        Node::Args(n)
            if n.args.len() == 1 && matches!(n.args.first(), Some(Node::ForwardArg(_))) =>
        {
            object("forward_args", &n.expression_l, input, empty())
        }
        Node::Args(n) => object(
            "args",
            &n.expression_l,
            input,
            [(
                "children",
                Value::Array(n.args.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::Array(n) => object(
            "array",
            &n.expression_l,
            input,
            [(
                "children",
                Value::Array(n.elements.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::ArrayPattern(n) => object(
            "array_pattern",
            &n.expression_l,
            input,
            [(
                "children",
                Value::Array(n.elements.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::ArrayPatternWithTail(n) => object(
            "array_pattern_with_tail",
            &n.expression_l,
            input,
            [(
                "children",
                Value::Array(n.elements.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::BackRef(n) => {
            object_with_column_delta("back_ref", &n.expression_l, input, -1, empty())
        }
        Node::Begin(n) => object(
            "begin",
            &n.expression_l,
            input,
            [(
                "body",
                Value::Array(n.statements.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::Block(n) => object(
            "block",
            &n.expression_l,
            input,
            [
                ("call_name", lower_node(&n.call, input)),
                (
                    "arguments",
                    lower_args_opt(n.args.as_deref(), &n.expression_l, input),
                ),
                (
                    "body",
                    lower_body_opt(n.body.as_deref(), &n.expression_l, input),
                ),
            ],
        ),
        Node::Blockarg(n) => object(
            "blockarg",
            &n.expression_l,
            input,
            [(
                "value",
                n.name.as_ref().map_or(Value::Null, |name| json!(name)),
            )],
        ),
        Node::BlockPass(n) => object(
            "block_pass",
            &n.expression_l,
            input,
            with_opts(
                vec![],
                [optional_field(
                    "value",
                    n.value.as_deref().map(|x| lower_node(x, input)),
                )],
            ),
        ),
        Node::Break(n) => object("break", &n.expression_l, input, empty()),
        Node::Case(n) => object(
            "case",
            &n.expression_l,
            input,
            with_opts(
                vec![(
                    "when_clauses",
                    Value::Array(n.when_bodies.iter().map(|x| lower_node(x, input)).collect()),
                )],
                [
                    optional_field(
                        "case_expression",
                        n.expr.as_deref().map(|x| lower_node(x, input)),
                    ),
                    optional_field(
                        "else_clause",
                        n.else_body.as_deref().map(|x| lower_node(x, input)),
                    ),
                ],
            ),
        ),
        Node::CaseMatch(n) => object(
            "case_match",
            &n.expression_l,
            input,
            with_opts(
                vec![
                    ("statement", lower_node(&n.expr, input)),
                    (
                        "bodies",
                        Value::Array(n.in_bodies.iter().map(|x| lower_node(x, input)).collect()),
                    ),
                ],
                [optional_field(
                    "else_clause",
                    n.else_body
                        .as_deref()
                        .map(|x| lower_body_opt(Some(x), &n.expression_l, input)),
                )],
            ),
        ),
        Node::Casgn(n) => {
            let lhs = const_name(n.scope.as_deref(), &n.name, input);
            object(
                "casgn",
                &n.expression_l,
                input,
                with_opts(
                    vec![("lhs", Value::String(lhs))],
                    [optional_field(
                        "rhs",
                        n.value.as_deref().map(|x| lower_node(x, input)),
                    )],
                ),
            )
        }
        Node::Cbase(n) => object("cbase", &n.expression_l, input, empty()),
        Node::Class(n) => object(
            "class",
            &n.expression_l,
            input,
            with_opts(
                vec![("name", lower_node(&n.name, input))],
                [
                    optional_field(
                        "superclass",
                        n.superclass.as_deref().map(|x| lower_node(x, input)),
                    ),
                    optional_field("body", n.body.as_deref().map(|x| lower_node(x, input))),
                ],
            ),
        ),
        Node::Const(n) => object(
            "const",
            &n.expression_l,
            input,
            with_opts(
                vec![("name", Value::String(n.name.clone()))],
                [optional_field(
                    "base",
                    n.scope.as_deref().map(|x| lower_node(x, input)),
                )],
            ),
        ),
        Node::CSend(n) => lower_send(
            "csend",
            Some(n.recv.as_ref()),
            &n.method_name,
            &n.args,
            &n.expression_l,
            input,
        ),
        Node::Cvar(n) => object("cvar", &n.expression_l, input, [("value", json!(n.name))]),
        Node::Cvasgn(n) => assignment(
            "cvasgn",
            &n.name,
            n.value.as_deref(),
            &n.expression_l,
            input,
        ),
        Node::Def(n) => object(
            "def",
            &n.expression_l,
            input,
            with_opts(
                vec![
                    ("name", Value::String(n.name.clone())),
                    (
                        "arguments",
                        lower_args_opt(n.args.as_deref(), &n.name_l, input),
                    ),
                ],
                [optional_field(
                    "body",
                    n.body.as_deref().map(|x| lower_node(x, input)),
                )],
            ),
        ),
        Node::Defined(n) => object(
            "defined?",
            &n.expression_l,
            input,
            [("arguments", Value::Array(vec![lower_node(&n.value, input)]))],
        ),
        Node::Defs(n) => object(
            "defs",
            &n.expression_l,
            input,
            with_opts(
                vec![
                    ("base", lower_node(&n.definee, input)),
                    ("name", Value::String(n.name.clone())),
                    (
                        "arguments",
                        lower_args_opt(n.args.as_deref(), &n.name_l, input),
                    ),
                ],
                [optional_field(
                    "body",
                    n.body.as_deref().map(|x| lower_node(x, input)),
                )],
            ),
        ),
        Node::Dstr(n) => object(
            "dstr",
            &n.expression_l,
            input,
            [(
                "children",
                Value::Array(n.parts.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::Dsym(n) => object(
            "dsym",
            &n.expression_l,
            input,
            [(
                "children",
                Value::Array(n.parts.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::EFlipFlop(n) => range(
            "eflipflop",
            n.left.as_deref(),
            n.right.as_deref(),
            &n.expression_l,
            input,
        ),
        Node::Ensure(n) => object(
            "ensure",
            &n.expression_l,
            input,
            [
                (
                    "statement",
                    lower_body_opt(n.body.as_deref(), &n.expression_l, input),
                ),
                (
                    "body",
                    lower_body_opt(n.ensure.as_deref(), &n.expression_l, input),
                ),
            ],
        ),
        Node::Erange(n) => range(
            "erange",
            n.left.as_deref(),
            n.right.as_deref(),
            &n.expression_l,
            input,
        ),
        Node::False(n) => object("false", &n.expression_l, input, empty()),
        Node::FindPattern(n) => object(
            "find_pattern",
            &n.expression_l,
            input,
            [(
                "children",
                Value::Array(n.elements.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::Float(n) => object("float", &n.expression_l, input, [("value", json!(n.value))]),
        Node::For(n) => object(
            "for",
            &n.expression_l,
            input,
            [
                ("variable", lower_node(&n.iterator, input)),
                ("collection", lower_node(&n.iteratee, input)),
                (
                    "body",
                    lower_body_opt(n.body.as_deref(), &n.expression_l, input),
                ),
            ],
        ),
        Node::ForwardArg(n) => object("forward_args", &n.expression_l, input, empty()),
        Node::ForwardedArgs(n) => object("forwarded_args", &n.expression_l, input, empty()),
        Node::Gvar(n) => object("gvar", &n.expression_l, input, [("value", json!(n.name))]),
        Node::Gvasgn(n) => assignment(
            "gvasgn",
            &n.name,
            n.value.as_deref(),
            &n.expression_l,
            input,
        ),
        Node::Hash(n) => object(
            "hash",
            &n.expression_l,
            input,
            [(
                "children",
                Value::Array(n.pairs.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::HashPattern(n) => object(
            "hash_pattern",
            &n.expression_l,
            input,
            [(
                "children",
                Value::Array(n.elements.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::Heredoc(n) => lower_heredoc(&n.parts, &n.heredoc_body_l, input),
        Node::If(n) => lower_if(
            "if",
            &n.cond,
            n.if_true.as_deref(),
            n.if_false.as_deref(),
            &n.expression_l,
            input,
        ),
        Node::IfGuard(n) => object(
            "if_guard",
            &n.expression_l,
            input,
            [("condition", lower_node(&n.cond, input))],
        ),
        Node::IFlipFlop(n) => range(
            "iflipflop",
            n.left.as_deref(),
            n.right.as_deref(),
            &n.expression_l,
            input,
        ),
        Node::IfMod(n) => lower_if(
            "if",
            &n.cond,
            n.if_true.as_deref(),
            n.if_false.as_deref(),
            &n.expression_l,
            input,
        ),
        Node::IfTernary(n) => lower_if(
            "if",
            &n.cond,
            Some(&n.if_true),
            Some(&n.if_false),
            &n.expression_l,
            input,
        ),
        Node::Index(n) => lower_send(
            "send",
            Some(&n.recv),
            "[]",
            &n.indexes,
            &n.expression_l,
            input,
        ),
        Node::IndexAsgn(n) if n.value.is_none() => lower_send(
            "send",
            Some(&n.recv),
            "[]",
            &n.indexes,
            &n.expression_l,
            input,
        ),
        Node::IndexAsgn(n) => object(
            "send",
            &n.expression_l,
            input,
            [
                ("name", Value::String("[]=".to_string())),
                ("receiver", lower_node(&n.recv, input)),
                (
                    "arguments",
                    Value::Array(
                        n.indexes
                            .iter()
                            .map(|x| lower_node(x, input))
                            .chain(n.value.iter().map(|x| lower_node(x, input)))
                            .collect(),
                    ),
                ),
            ],
        ),
        Node::InPattern(n) => object(
            "in_pattern",
            &n.expression_l,
            input,
            with_opts(
                vec![
                    ("pattern", lower_node(&n.pattern, input)),
                    (
                        "body",
                        lower_body_opt(n.body.as_deref(), &n.expression_l, input),
                    ),
                ],
                [optional_field(
                    "guard",
                    n.guard.as_deref().map(|x| lower_node(x, input)),
                )],
            ),
        ),
        Node::Int(n) => object("int", &n.expression_l, input, [("value", json!(n.value))]),
        Node::Irange(n) => range(
            "irange",
            n.left.as_deref(),
            n.right.as_deref(),
            &n.expression_l,
            input,
        ),
        Node::Ivar(n) => object("ivar", &n.expression_l, input, [("value", json!(n.name))]),
        Node::Ivasgn(n) => assignment(
            "ivasgn",
            &n.name,
            n.value.as_deref(),
            &n.expression_l,
            input,
        ),
        Node::Kwarg(n) => object("kwarg", &n.expression_l, input, [("key", json!(n.name))]),
        Node::Kwargs(n) => object(
            "hash",
            &n.expression_l,
            input,
            [(
                "children",
                Value::Array(n.pairs.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::KwBegin(n) => object(
            "kwbegin",
            &n.expression_l,
            input,
            [(
                "body",
                Value::Array(n.statements.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::Kwnilarg(n) => object("kwnilarg", &n.expression_l, input, empty()),
        Node::Kwoptarg(n) => object(
            "kwoptarg",
            &n.expression_l,
            input,
            [
                ("key", Value::String(n.name.clone())),
                ("value", lower_node(&n.default, input)),
            ],
        ),
        Node::Kwrestarg(n) => object(
            "kwrestarg",
            &n.expression_l,
            input,
            [(
                "value",
                n.name.as_ref().map_or(Value::Null, |name| json!(name)),
            )],
        ),
        Node::Kwsplat(n) => object(
            "kwsplat",
            &n.expression_l,
            input,
            [("value", lower_node(&n.value, input))],
        ),
        Node::Lambda(n) => lower_send("send", None, "lambda", &[], &n.expression_l, input),
        Node::Lvar(n) => object("lvar", &n.expression_l, input, [("value", json!(n.name))]),
        Node::Lvasgn(n) => assignment(
            "lvasgn",
            &n.name,
            n.value.as_deref(),
            &n.expression_l,
            input,
        ),
        Node::Masgn(n) => object(
            "masgn",
            &n.expression_l,
            input,
            [
                ("lhs", lower_node(&n.lhs, input)),
                ("rhs", lower_node(&n.rhs, input)),
            ],
        ),
        Node::MatchAlt(n) => binary_named("match_alt", &n.lhs, &n.rhs, &n.expression_l, input),
        Node::MatchAs(n) => object(
            "match_as",
            &n.expression_l,
            input,
            [
                ("value", lower_node(&n.value, input)),
                ("as", lower_node(&n.as_, input)),
            ],
        ),
        Node::MatchNilPattern(n) => object("match_nil_pattern", &n.expression_l, input, empty()),
        Node::MatchPattern(n) => object(
            "match_pattern",
            &n.expression_l,
            input,
            [
                ("value", lower_node(&n.value, input)),
                ("pattern", lower_node(&n.pattern, input)),
            ],
        ),
        Node::MatchPatternP(n) => object(
            "match_pattern_p",
            &n.expression_l,
            input,
            [
                ("value", lower_node(&n.value, input)),
                ("pattern", lower_node(&n.pattern, input)),
            ],
        ),
        Node::MatchRest(n) => object(
            "match_rest",
            &n.expression_l,
            input,
            with_opts(
                vec![],
                [optional_field(
                    "value",
                    n.name.as_deref().map(|x| lower_node(x, input)),
                )],
            ),
        ),
        Node::MatchVar(n) => object("match_var", &n.expression_l, input, empty()),
        Node::MatchWithLvasgn(n) => object(
            "match_with_lvasgn",
            &n.expression_l,
            input,
            [
                ("lhs", lower_node(&n.re, input)),
                ("rhs", lower_node(&n.value, input)),
            ],
        ),
        Node::Mlhs(n) => object(
            "mlhs",
            &n.expression_l,
            input,
            [(
                "children",
                Value::Array(n.items.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::Module(n) => object(
            "module",
            &n.expression_l,
            input,
            with_opts(
                vec![("name", lower_node(&n.name, input))],
                [optional_field(
                    "body",
                    n.body.as_deref().map(|x| lower_node(x, input)),
                )],
            ),
        ),
        Node::Next(n) => object("next", &n.expression_l, input, empty()),
        Node::Nil(n) => object("nil", &n.expression_l, input, empty()),
        Node::NthRef(n) => object(
            "nth_ref",
            &n.expression_l,
            input,
            [("value", json!(n.name.parse::<i64>().unwrap_or_default()))],
        ),
        Node::Numblock(n) => object(
            "numblock",
            &n.expression_l,
            input,
            [
                ("call_name", lower_node(&n.call, input)),
                ("arguments", empty_args(&n.expression_l, input)),
                (
                    "body",
                    lower_body_opt(Some(n.body.as_ref()), &n.expression_l, input),
                ),
            ],
        ),
        Node::OpAsgn(n) => object(
            "op_asgn",
            &n.expression_l,
            input,
            [
                ("lhs", lower_node(&n.recv, input)),
                (
                    "op",
                    Value::String(n.operator.trim_end_matches('=').to_string()),
                ),
                ("rhs", lower_node(&n.value, input)),
            ],
        ),
        Node::Optarg(n) => object(
            "optarg",
            &n.expression_l,
            input,
            [
                ("key", Value::String(n.name.clone())),
                ("value", lower_node(&n.default, input)),
            ],
        ),
        Node::Or(n) => binary_named("or", &n.lhs, &n.rhs, &n.expression_l, input),
        Node::OrAsgn(n) => object(
            "or_asgn",
            &n.expression_l,
            input,
            [
                ("lhs", lower_node(&n.recv, input)),
                ("rhs", lower_node(&n.value, input)),
            ],
        ),
        Node::Pair(n) => object(
            "pair",
            &n.expression_l,
            input,
            [
                ("key", lower_node(&n.key, input)),
                ("value", lower_node(&n.value, input)),
            ],
        ),
        Node::Postexe(n) => object(
            "postexe",
            &n.expression_l,
            input,
            [(
                "body",
                lower_body_opt(n.body.as_deref(), &n.expression_l, input),
            )],
        ),
        Node::Preexe(n) => object(
            "preexe",
            &n.expression_l,
            input,
            [(
                "body",
                lower_body_opt(n.body.as_deref(), &n.expression_l, input),
            )],
        ),
        Node::Procarg0(n) => {
            if n.args.len() == 1 {
                lower_node(&n.args[0], input)
            } else {
                object(
                    "array",
                    &n.expression_l,
                    input,
                    [(
                        "children",
                        Value::Array(n.args.iter().map(|x| lower_node(x, input)).collect()),
                    )],
                )
            }
        }
        Node::Rational(n) => object(
            "rational",
            &n.expression_l,
            input,
            [("value", json!(n.value))],
        ),
        Node::Redo(n) => object("redo", &n.expression_l, input, empty()),
        Node::Regexp(n) => {
            let dynamic = n.parts.iter().any(|part| !matches!(part, Node::Str(_)));
            object(
                "regexp",
                &n.expression_l,
                input,
                with_opts(
                    vec![],
                    [optional_field(
                        "value",
                        dynamic.then(|| {
                            object(
                                "begin",
                                &n.expression_l,
                                input,
                                [(
                                    "body",
                                    Value::Array(
                                        n.parts.iter().map(|x| lower_node(x, input)).collect(),
                                    ),
                                )],
                            )
                        }),
                    )],
                ),
            )
        }
        Node::RegOpt(n) => object("regopt", &n.expression_l, input, empty()),
        Node::Rescue(n) => object(
            "rescue",
            &n.expression_l,
            input,
            with_opts(
                vec![
                    (
                        "statement",
                        lower_body_opt(n.body.as_deref(), &n.expression_l, input),
                    ),
                    (
                        "bodies",
                        Value::Array(
                            n.rescue_bodies
                                .iter()
                                .map(|x| lower_node(x, input))
                                .collect(),
                        ),
                    ),
                ],
                [optional_field(
                    "else_clause",
                    n.else_.as_deref().map(|x| lower_node(x, input)),
                )],
            ),
        ),
        Node::RescueBody(n) => object(
            "resbody",
            &n.expression_l,
            input,
            with_opts(
                vec![],
                [
                    optional_field(
                        "exec_list",
                        n.exc_list.as_deref().map(|x| lower_node(x, input)),
                    ),
                    optional_field(
                        "exec_var",
                        n.exc_var.as_deref().map(|x| lower_node(x, input)),
                    ),
                    optional_field("body", n.body.as_deref().map(|x| lower_node(x, input))),
                ],
            ),
        ),
        Node::Restarg(n) => object(
            "restarg",
            &n.expression_l,
            input,
            [(
                "value",
                n.name.as_ref().map_or(Value::Null, |name| json!(name)),
            )],
        ),
        Node::Retry(n) => object("retry", &n.expression_l, input, empty()),
        Node::Return(n) => object(
            "return",
            &n.expression_l,
            input,
            [(
                "values",
                Value::Array(n.args.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::SClass(n) => object(
            "sclass",
            &n.expression_l,
            input,
            with_opts(
                vec![("name", lower_node(&n.expr, input))],
                [optional_field(
                    "def",
                    n.body.as_deref().map(|x| lower_node(x, input)),
                )],
            ),
        ),
        Node::Self_(n) => object("self", &n.expression_l, input, empty()),
        Node::Send(n) => lower_send(
            "send",
            n.recv.as_deref(),
            &n.method_name,
            &n.args,
            &n.expression_l,
            input,
        ),
        Node::Shadowarg(n) => object(
            "shadowarg",
            &n.expression_l,
            input,
            [("value", json!(n.name))],
        ),
        Node::Splat(n) => object(
            "splat",
            &n.expression_l,
            input,
            with_opts(
                vec![],
                [optional_field(
                    "value",
                    n.value.as_deref().map(|x| lower_node(x, input)),
                )],
            ),
        ),
        Node::Str(n) => {
            let source = n.expression_l.source(input).unwrap_or_default();
            if source.starts_with("%q<") && source.contains('\n') {
                multiline_percent_q_string(&n.expression_l, &source, input)
            } else {
                object(
                    "str",
                    &n.expression_l,
                    input,
                    [("value", Value::String(n.value.to_string_lossy()))],
                )
            }
        }
        Node::Super(n) => object(
            "super",
            &n.expression_l,
            input,
            [(
                "arguments",
                Value::Array(n.args.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::Sym(n) => object(
            "sym",
            &n.expression_l,
            input,
            [("value", json!(n.name.to_string_lossy()))],
        ),
        Node::True(n) => object("true", &n.expression_l, input, empty()),
        Node::Undef(n) => object(
            "undef",
            &n.expression_l,
            input,
            [(
                "children",
                Value::Array(n.names.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::UnlessGuard(n) => object(
            "unless_guard",
            &n.expression_l,
            input,
            [("condition", lower_node(&n.cond, input))],
        ),
        Node::Until(n) => object(
            "until",
            &n.expression_l,
            input,
            [
                ("condition", lower_node(&n.cond, input)),
                (
                    "body",
                    lower_body_opt(n.body.as_deref(), &n.expression_l, input),
                ),
            ],
        ),
        Node::UntilPost(n) => object(
            "until_post",
            &n.expression_l,
            input,
            [
                ("condition", lower_node(&n.cond, input)),
                (
                    "body",
                    lower_body_opt(Some(n.body.as_ref()), &n.expression_l, input),
                ),
            ],
        ),
        Node::When(n) => object(
            "when",
            &n.expression_l,
            input,
            [
                (
                    "conditions",
                    Value::Array(n.patterns.iter().map(|x| lower_node(x, input)).collect()),
                ),
                (
                    "then_branch",
                    lower_body_opt(n.body.as_deref(), &n.expression_l, input),
                ),
            ],
        ),
        Node::While(n) => object(
            "while",
            &n.expression_l,
            input,
            [
                ("condition", lower_node(&n.cond, input)),
                (
                    "body",
                    lower_body_opt(n.body.as_deref(), &n.expression_l, input),
                ),
            ],
        ),
        Node::WhilePost(n) => object(
            "while_post",
            &n.expression_l,
            input,
            [
                ("condition", lower_node(&n.cond, input)),
                (
                    "body",
                    lower_body_opt(Some(n.body.as_ref()), &n.expression_l, input),
                ),
            ],
        ),
        Node::XHeredoc(n) => object(
            "xstr",
            &n.expression_l,
            input,
            [(
                "arguments",
                Value::Array(n.parts.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::Xstr(n) => object(
            "xstr",
            &n.expression_l,
            input,
            [(
                "arguments",
                Value::Array(n.parts.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::Yield(n) => object(
            "yield",
            &n.expression_l,
            input,
            [(
                "arguments",
                Value::Array(n.args.iter().map(|x| lower_node(x, input)).collect()),
            )],
        ),
        Node::ZSuper(n) => object(
            "zsuper",
            &n.expression_l,
            input,
            [("arguments", Value::Array(vec![]))],
        ),
        _ => {
            record_unknown_node(node);
            object("__unknown", node.expression(), input, empty())
        }
    }
}

fn lower_send(
    node_type: &str,
    recv: Option<&Node>,
    method_name: &str,
    args: &[Node],
    loc: &Loc,
    input: &DecodedInput,
) -> Value {
    if recv.is_none() && method_name == "retry!" {
        if let Some((base_loc, full_loc)) = retry_receiver_locs(loc, input) {
            return object(
                node_type,
                &full_loc,
                input,
                [
                    ("name", Value::String(method_name.to_string())),
                    (
                        "arguments",
                        Value::Array(args.iter().map(|x| lower_node(x, input)).collect()),
                    ),
                    (
                        "receiver",
                        object(
                            "send",
                            &base_loc,
                            input,
                            [
                                ("name", Value::String("retry".to_string())),
                                ("arguments", Value::Array(vec![])),
                            ],
                        ),
                    ),
                ],
            );
        }
    }

    object(
        node_type,
        loc,
        input,
        with_opts(
            vec![
                ("name", Value::String(method_name.to_string())),
                (
                    "arguments",
                    Value::Array(args.iter().map(|x| lower_node(x, input)).collect()),
                ),
            ],
            [optional_field(
                "receiver",
                recv.map(|x| lower_node(x, input)),
            )],
        ),
    )
}

fn retry_receiver_locs(loc: &Loc, input: &DecodedInput) -> Option<(Loc, Loc)> {
    let bytes = input.as_shared_bytes();
    let prefix = bytes.get(..loc.begin)?;
    let (base_begin, base_end) = if prefix.ends_with(b"retry.") {
        (loc.begin.checked_sub("retry.".len())?, loc.begin - 1)
    } else if prefix.ends_with(b"retry::") {
        (loc.begin.checked_sub("retry::".len())?, loc.begin - 2)
    } else {
        return None;
    };
    let base_loc = Loc {
        begin: base_begin,
        end: base_end,
    };
    let full_loc = Loc {
        begin: base_begin,
        end: loc.end,
    };
    Some((base_loc, full_loc))
}

fn lower_if(
    node_type: &str,
    cond: &Node,
    if_true: Option<&Node>,
    if_false: Option<&Node>,
    loc: &Loc,
    input: &DecodedInput,
) -> Value {
    object(
        node_type,
        loc,
        input,
        with_opts(
            vec![("condition", lower_node(cond, input))],
            [
                optional_field("then_branch", if_true.map(|x| lower_node(x, input))),
                optional_field("else_branch", if_false.map(|x| lower_node(x, input))),
            ],
        ),
    )
}

fn binary_named(node_type: &str, lhs: &Node, rhs: &Node, loc: &Loc, input: &DecodedInput) -> Value {
    object(
        node_type,
        loc,
        input,
        [
            ("lhs", lower_node(lhs, input)),
            ("rhs", lower_node(rhs, input)),
        ],
    )
}

fn assignment(
    node_type: &str,
    name: &str,
    value: Option<&Node>,
    loc: &Loc,
    input: &DecodedInput,
) -> Value {
    object(
        node_type,
        loc,
        input,
        with_opts(
            vec![("lhs", Value::String(name.to_string()))],
            [optional_field("rhs", value.map(|x| lower_node(x, input)))],
        ),
    )
}

fn range(
    node_type: &str,
    start: Option<&Node>,
    end: Option<&Node>,
    loc: &Loc,
    input: &DecodedInput,
) -> Value {
    object(
        node_type,
        loc,
        input,
        with_opts(
            vec![],
            [
                optional_field("start", start.map(|x| lower_node(x, input))),
                optional_field("end", end.map(|x| lower_node(x, input))),
            ],
        ),
    )
}

fn lower_args_opt(args: Option<&Node>, loc: &Loc, input: &DecodedInput) -> Value {
    args.map(|node| lower_node(node, input))
        .unwrap_or_else(|| empty_args(loc, input))
}

fn empty_args(loc: &Loc, input: &DecodedInput) -> Value {
    object("args", loc, input, [("children", Value::Array(vec![]))])
}

fn lower_body_opt(body: Option<&Node>, loc: &Loc, input: &DecodedInput) -> Value {
    body.map(|node| lower_node(node, input))
        .unwrap_or_else(|| object("begin", loc, input, [("body", Value::Array(vec![]))]))
}

fn lower_heredoc(parts: &[Node], loc: &Loc, input: &DecodedInput) -> Value {
    let body_loc = parts_loc(parts, loc);
    if parts.iter().all(|part| matches!(part, Node::Str(_))) {
        object(
            "str",
            &body_loc,
            input,
            [("value", Value::String(static_parts_value(parts)))],
        )
    } else {
        object(
            "dstr",
            &body_loc,
            input,
            [(
                "children",
                Value::Array(parts.iter().map(|x| lower_node(x, input)).collect()),
            )],
        )
    }
}

fn parts_loc(parts: &[Node], fallback: &Loc) -> Loc {
    match (parts.first(), parts.last()) {
        (Some(first), Some(last)) => Loc {
            begin: first.expression().begin,
            end: last.expression().end,
        },
        _ => *fallback,
    }
}

fn static_parts_value(parts: &[Node]) -> String {
    parts
        .iter()
        .filter_map(|part| match part {
            Node::Str(str_node) => Some(str_node.value.to_string_lossy()),
            _ => None,
        })
        .collect()
}

fn multiline_percent_q_string(loc: &Loc, source: &str, input: &DecodedInput) -> Value {
    let lines = source.split('\n').collect::<Vec<_>>();
    let mut offset = loc.begin;
    let mut body = Vec::new();
    for line in lines.iter().take(lines.len().saturating_sub(1)) {
        let line_loc = Loc {
            begin: offset,
            end: offset + line.len(),
        };
        body.push(object(
            "str",
            &line_loc,
            input,
            [("value", Value::String((*line).to_string()))],
        ));
        offset += line.len() + 1;
    }
    object("begin", loc, input, [("body", Value::Array(body))])
}

fn optional_field(name: &'static str, value: Option<Value>) -> Option<(&'static str, Value)> {
    value.map(|value| (name, value))
}

fn with_opts(
    mut fields: Vec<(&'static str, Value)>,
    opts: impl IntoIterator<Item = Option<(&'static str, Value)>>,
) -> Vec<(&'static str, Value)> {
    fields.extend(opts.into_iter().flatten());
    fields
}

fn const_name(scope: Option<&Node>, name: &str, input: &DecodedInput) -> String {
    match scope {
        Some(Node::Const(n)) => format!(
            "{}::{}",
            const_name(n.scope.as_deref(), &n.name, input),
            name
        ),
        Some(Node::Cbase(_)) => format!("::{name}"),
        Some(other) => {
            let base = other
                .expression()
                .source(input)
                .unwrap_or_else(|| other.str_type().to_string());
            format!("{base}::{name}")
        }
        None => name.to_string(),
    }
}

fn object<'a>(
    node_type: &str,
    loc: &Loc,
    input: &DecodedInput,
    fields: impl IntoIterator<Item = (&'a str, Value)>,
) -> Value {
    let mut obj = Map::new();
    obj.insert("type".to_string(), Value::String(node_type.to_string()));
    obj.insert("meta_data".to_string(), meta(loc, input));
    for (key, value) in fields {
        obj.insert(key.to_string(), value);
    }
    Value::Object(obj)
}

fn object_with_column_delta<'a>(
    node_type: &str,
    loc: &Loc,
    input: &DecodedInput,
    column_delta: isize,
    fields: impl IntoIterator<Item = (&'a str, Value)>,
) -> Value {
    let mut obj = Map::new();
    obj.insert("type".to_string(), Value::String(node_type.to_string()));
    obj.insert(
        "meta_data".to_string(),
        meta_with_column_delta(loc, input, column_delta),
    );
    for (key, value) in fields {
        obj.insert(key.to_string(), value);
    }
    Value::Object(obj)
}

fn meta(loc: &Loc, input: &DecodedInput) -> Value {
    meta_with_column_delta(loc, input, 0)
}

fn meta_with_column_delta(loc: &Loc, input: &DecodedInput, column_delta: isize) -> Value {
    let (start_line, start_column) = input
        .line_col_for_pos(loc.begin)
        .unwrap_or((usize::MAX, usize::MAX));
    let (end_line, end_column) = input
        .line_col_for_pos(loc.end)
        .unwrap_or((usize::MAX, usize::MAX));
    json!({
        "code": loc.source(input).unwrap_or_default(),
        "start_line": line_to_json(start_line),
        "start_column": column_to_json_with_delta(start_column, column_delta),
        "end_line": line_to_json(end_line),
        "end_column": column_to_json_with_delta(end_column, column_delta),
        "offset_start": loc.begin,
        "offset_end": loc.end,
    })
}

fn line_to_json(line: usize) -> isize {
    if line == usize::MAX {
        -1
    } else {
        (line + 1) as isize
    }
}

fn column_to_json_with_delta(column: usize, delta: isize) -> isize {
    if column == usize::MAX {
        -1
    } else {
        ((column + 1) as isize + delta).max(1)
    }
}

fn empty<'a>() -> impl IntoIterator<Item = (&'a str, Value)> {
    std::iter::empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Value {
        generate_source(
            source.as_bytes(),
            Path::new("/tmp/test.rb"),
            Path::new("test.rb"),
        )
        .expect("json")
    }

    #[test]
    fn lowers_assignment_and_call() {
        let json = parse("x = foo(1)\n");
        assert_eq!(json["type"], "begin");
        assert_eq!(json["body"][0]["type"], "lvasgn");
        assert_eq!(json["body"][0]["lhs"], "x");
        assert_eq!(json["body"][0]["rhs"]["type"], "send");
        assert_eq!(json["body"][0]["rhs"]["name"], "foo");
    }

    #[test]
    fn lowers_class_and_method() {
        let json = parse("class A < B\n  def m(x = 1)\n    x\n  end\nend\n");
        let class_node = &json["body"][0];
        assert_eq!(class_node["type"], "class");
        assert_eq!(class_node["name"]["name"], "A");
        assert_eq!(class_node["superclass"]["name"], "B");
        assert_eq!(class_node["body"]["type"], "def");
    }

    #[test]
    fn lowers_blocks_and_hashes() {
        let json = parse("items.each { |x| puts({a: x}) }\n");
        let block = &json["body"][0];
        assert_eq!(block["type"], "block");
        assert_eq!(block["arguments"]["children"][0]["value"], "x");
        assert_eq!(block["body"]["type"], "send");
    }

    #[test]
    fn recovers_retry_keyword_receiver() {
        let json = parse("retry::retry!()\n");
        let send = &json["body"][0];
        assert_eq!(send["type"], "send");
        assert_eq!(send["meta_data"]["code"], "retry::retry!()");
        assert_eq!(send["receiver"]["type"], "send");
        assert_eq!(send["receiver"]["name"], "retry");
        assert_eq!(send["receiver"]["meta_data"]["code"], "retry");
    }

    #[test]
    fn lowers_forwarded_method_args() {
        let json = parse("def foo(...)\n  bar('foo', ...)\nend\n");
        let method = &json["body"][0];
        assert_eq!(method["arguments"]["type"], "forward_args");
        assert_eq!(method["arguments"]["meta_data"]["code"], "(...)");
        assert_eq!(method["body"]["arguments"][1]["type"], "forwarded_args");
    }

    #[test]
    fn lowers_keyword_call_arguments_as_unbraced_hashes() {
        let json = parse("foo(\"hello\", bar: \"baz\")\n");
        let call = &json["body"][0];
        assert_eq!(call["arguments"][1]["type"], "hash");
        assert_eq!(call["arguments"][1]["children"][0]["type"], "pair");
        assert_eq!(call["arguments"][1]["children"][0]["key"]["type"], "sym");
        assert_eq!(call["arguments"][1]["children"][0]["value"]["value"], "baz");
    }

    #[test]
    fn lowers_back_references() {
        let json = parse("foo { urls << $& }\n");
        let block_body = &json["body"][0]["body"];
        assert_eq!(block_body["arguments"][0]["type"], "back_ref");
        assert_eq!(block_body["arguments"][0]["meta_data"]["code"], "$&");
        assert_eq!(block_body["arguments"][0]["meta_data"]["start_column"], 14);
    }

    #[test]
    fn lowers_numeric_back_references() {
        let json = parse("puts $1\n");
        let arg = &json["body"][0]["arguments"][0];
        assert_eq!(arg["type"], "nth_ref");
        assert_eq!(arg["value"], 1);
    }

    #[test]
    fn lowers_index_access_as_bracket_send() {
        let json = parse("params[:type]\nSet[]\n");
        let index = &json["body"][0];
        assert_eq!(index["type"], "send");
        assert_eq!(index["name"], "[]");
        assert_eq!(index["receiver"]["name"], "params");
        assert_eq!(index["arguments"][0]["value"], "type");

        let empty_brackets = &json["body"][1];
        assert_eq!(empty_brackets["type"], "send");
        assert_eq!(empty_brackets["name"], "[]");
        assert_eq!(empty_brackets["receiver"]["name"], "Set");
        assert_eq!(empty_brackets["arguments"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn lowers_index_assignment_as_bracket_assignment_send() {
        let json = parse("hash[:id] = value\n");
        let assignment = &json["body"][0];
        assert_eq!(assignment["type"], "send");
        assert_eq!(assignment["name"], "[]=");
        assert_eq!(assignment["receiver"]["name"], "hash");
        assert_eq!(assignment["arguments"][0]["value"], "id");
        assert_eq!(assignment["arguments"][1]["name"], "value");
    }

    #[test]
    fn lowers_index_assignment_target_as_index_access() {
        let json = parse("hash[:id] ||= value\n");
        let assignment = &json["body"][0];
        assert_eq!(assignment["type"], "or_asgn");
        assert_eq!(assignment["lhs"]["type"], "send");
        assert_eq!(assignment["lhs"]["name"], "[]");
    }

    #[test]
    fn lowers_case_match_array_patterns() {
        let json = parse("case [1, 2]\nin [x, y]\n  puts x\nend\n");
        let case_match = &json["body"][0];
        assert_eq!(case_match["type"], "case_match");
        assert_eq!(case_match["bodies"][0]["type"], "in_pattern");
        assert_eq!(case_match["bodies"][0]["pattern"]["type"], "array_pattern");
        assert_eq!(
            case_match["bodies"][0]["pattern"]["children"][0]["type"],
            "match_var"
        );
    }

    #[test]
    fn lowers_static_heredoc_as_string_literal() {
        let json = parse("value = <<-SQL\n  SELECT * FROM table;\nSQL\n");
        let assignment = &json["body"][0];
        assert_eq!(assignment["type"], "lvasgn");
        assert_eq!(assignment["rhs"]["type"], "str");
        assert_eq!(assignment["rhs"]["value"], "  SELECT * FROM table;\n");
        assert_eq!(
            assignment["rhs"]["meta_data"]["code"],
            "  SELECT * FROM table;\n"
        );
    }

    #[test]
    fn lowers_heredoc_argument_as_string_literal() {
        let json = parse("bar(<<-TEXT)\n  body\nTEXT\n");
        let argument = &json["body"][0]["arguments"][0];
        assert_eq!(argument["type"], "str");
        assert_eq!(argument["value"], "  body\n");
    }

    #[test]
    fn lowers_interpolated_heredoc_as_dynamic_string() {
        let json = parse("name = 'world'\n<<-TEXT\nhello #{name}\nTEXT\n");
        let heredoc = &json["body"][1];
        assert_eq!(heredoc["type"], "dstr");
        assert_eq!(heredoc["children"][0]["type"], "str");
        assert_eq!(heredoc["children"][1]["type"], "begin");
    }

    #[test]
    fn lowers_erb_templates_to_ruby_helper_calls() {
        let dir = tempfile::tempdir().expect("tempdir");
        let erb = dir.path().join("test.erb");
        fs::write(
            &erb,
            "hello <%= user.name %>\n<% if enabled %>\nworld\n<% end %>\n",
        )
        .expect("erb");

        let json = generate_file(&erb, dir.path()).expect("json");
        assert_eq!(json["body"][1]["name"], "joernBufferAppend");
        assert_eq!(
            json["body"][2]["arguments"][1]["name"],
            "joernTemplateOutEscape"
        );
        assert_eq!(json["body"][3]["type"], "if");
        assert_eq!(json["body"][3]["meta_data"]["start_line"], -1);
    }

    #[test]
    fn invalid_erb_falls_back_to_dynamic_string() {
        let dir = tempfile::tempdir().expect("tempdir");
        let erb = dir.path().join("broken.erb");
        fs::write(&erb, "<% if enabled %>\nmissing end\n").expect("erb");

        let json = generate_file(&erb, dir.path()).expect("json");
        assert_eq!(json["body"][0]["type"], "dstr");
    }

    /// Collects every `type` string in the tree so newly mapped variants can be
    /// asserted on regardless of where they nest.
    fn collect_types(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(obj) => {
                if let Some(Value::String(t)) = obj.get("type") {
                    out.push(t.clone());
                }
                for child in obj.values() {
                    collect_types(child, out);
                }
            }
            Value::Array(items) => items.iter().for_each(|x| collect_types(x, out)),
            _ => {}
        }
    }

    fn assert_emits(source: &str, expected_type: &str) {
        let json = parse(source);
        let mut types = Vec::new();
        collect_types(&json, &mut types);
        assert!(
            types.iter().any(|t| t == expected_type),
            "expected type `{expected_type}` for source `{source}`, got {types:?}"
        );
        assert!(
            !types.iter().any(|t| t == "__unknown"),
            "source `{source}` produced an __unknown node: {types:?}"
        );
    }

    #[test]
    fn lowers_match_patterns() {
        // drain any prior tally so this test owns the counter
        let _ = take_unknown_node_summary();
        assert_emits("case x\nin [1, *rest]\n  rest\nend\n", "match_rest");
        assert_emits("case x\nin {a:, **rest}\n  a\nend\n", "hash_pattern");
        assert_emits("case x\nin [*, 1, *]\n  1\nend\n", "find_pattern");
        assert_emits("case x\nin 1 | 2\n  x\nend\n", "match_alt");
        assert_emits("case x\nin Integer => n\n  n\nend\n", "match_as");
        assert_emits("case x\nin **nil\n  x\nend\n", "match_nil_pattern");
        assert_emits("x => Integer\n", "match_pattern");
        assert_emits("x in Integer\n", "match_pattern_p");
        // every snippet above mapped cleanly, so nothing was tallied
        assert_eq!(take_unknown_node_summary(), None);
    }

    #[test]
    fn lowers_flip_flops() {
        assert_emits("if (i == 1)..(i == 5)\n  i\nend\n", "iflipflop");
        assert_emits("if (i == 1)...(i == 5)\n  i\nend\n", "eflipflop");
    }

    #[test]
    fn lowers_pattern_guards() {
        assert_emits("case x\nin Integer if x > 0\n  x\nend\n", "if_guard");
        assert_emits(
            "case x\nin Integer unless x.zero?\n  x\nend\n",
            "unless_guard",
        );
    }

    #[test]
    fn lowers_begin_end_blocks() {
        assert_emits("BEGIN { puts 1 }\n", "preexe");
        assert_emits("END { puts 1 }\n", "postexe");
    }

    #[test]
    fn lowers_keyword_and_literal_nodes() {
        assert_emits("loop do\n  redo\nend\n", "redo");
        assert_emits("begin\nrescue\n  retry\nend\n", "retry");
        assert_emits("undef foo, bar\n", "undef");
        assert_emits("x = 3r\n", "rational");
    }

    #[test]
    fn lowers_shadow_arguments() {
        assert_emits("foo { |x; shadow| shadow }\n", "shadowarg");
    }

    #[test]
    fn unmapped_nodes_are_tallied_off_stream() {
        let _ = take_unknown_node_summary();
        // `__ENCODING__` lowers to the still-unmapped `Encoding` node, whose
        // parser-gem name is `__ENCODING__`.
        let json = parse("__ENCODING__\n");
        assert_eq!(json["body"][0]["type"], "__unknown");
        let summary = take_unknown_node_summary().expect("summary");
        assert!(summary.contains("__ENCODING__(x1)"), "got: {summary}");
        assert!(summary.starts_with("rubyastgen: 1 unmapped node(s):"));
        // tally drains, so a clean run afterwards reports nothing
        assert_eq!(take_unknown_node_summary(), None);
    }
}
