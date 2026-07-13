use anyhow::{Context, Result};
use rustpython_parser::ast;
use rustpython_parser::text_size::TextRange;
use rustpython_parser::Parse;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PyAstDocument {
    pub backend: String,
    pub version: String,
    pub path: String,
    pub source_length: usize,
    pub root: PyAstNode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceRange {
    pub start_offset: usize,
    pub end_offset: usize,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PyAstNode {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<SourceRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub children: BTreeMap<String, Vec<PyAstNode>>,
}

impl PyAstNode {
    fn new(
        kind: impl Into<String>,
        range: Option<TextRange>,
        source: &str,
        lines: &LineIndex,
    ) -> Self {
        let range = range.map(|range| source_range(range, source, lines));
        let text = range
            .as_ref()
            .and_then(|range| source.get(range.start_offset..range.end_offset))
            .map(ToOwned::to_owned);
        Self {
            kind: kind.into(),
            range,
            text,
            properties: BTreeMap::new(),
            children: BTreeMap::new(),
        }
    }

    fn prop(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.properties.insert(name.into(), value.into());
        self
    }

    fn child(mut self, name: impl Into<String>, value: Option<PyAstNode>) -> Self {
        if let Some(value) = value {
            self.children.insert(name.into(), vec![value]);
        }
        self
    }

    fn children(mut self, name: impl Into<String>, values: Vec<PyAstNode>) -> Self {
        if !values.is_empty() {
            self.children.insert(name.into(), values);
        }
        self
    }
}

pub fn parse_file(path: &Path) -> Result<PyAstDocument> {
    let source = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_source(path, &source)
}

pub fn parse_source(path: &Path, source: &str) -> Result<PyAstDocument> {
    let path_text = path.to_string_lossy().into_owned();
    let suite = ast::Suite::parse(source, &path_text)
        .with_context(|| format!("parsing {}", path.display()))?;
    let lines = LineIndex::new(source);
    let root = module_node(suite, source, &lines);
    Ok(PyAstDocument {
        backend: "oxidized-pyastgen".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        path: path_text,
        source_length: source.len(),
        root,
    })
}

pub fn write_json(path: &Path, value: &PyAstDocument) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

fn module_node(body: ast::Suite, source: &str, lines: &LineIndex) -> PyAstNode {
    let module_range = TextRange::new(0.into(), (source.len() as u32).into());
    PyAstNode::new("Module", Some(module_range), source, lines).children(
        "body",
        body.iter()
            .map(|stmt| stmt_node(stmt, source, lines))
            .collect(),
    )
}

fn stmt_node(stmt: &ast::Stmt, source: &str, lines: &LineIndex) -> PyAstNode {
    match stmt {
        ast::Stmt::FunctionDef(node) => function_node(
            FunctionParts {
                kind: "FunctionDef",
                range: node.range,
                name: &node.name,
                args: &node.args,
                body: &node.body,
                decorator_list: &node.decorator_list,
                returns: node.returns.as_deref(),
                type_comment: &node.type_comment,
                type_params: &node.type_params,
            },
            source,
            lines,
        ),
        ast::Stmt::AsyncFunctionDef(node) => function_node(
            FunctionParts {
                kind: "AsyncFunctionDef",
                range: node.range,
                name: &node.name,
                args: &node.args,
                body: &node.body,
                decorator_list: &node.decorator_list,
                returns: node.returns.as_deref(),
                type_comment: &node.type_comment,
                type_params: &node.type_params,
            },
            source,
            lines,
        ),
        ast::Stmt::ClassDef(node) => PyAstNode::new("ClassDef", Some(node.range), source, lines)
            .prop("name", node.name.as_str())
            .children("bases", expr_nodes(&node.bases, source, lines))
            .children("keywords", keyword_nodes(&node.keywords, source, lines))
            .children("body", stmt_nodes(&node.body, source, lines))
            .children(
                "decorator_list",
                expr_nodes(&node.decorator_list, source, lines),
            )
            .children(
                "type_params",
                type_param_nodes(&node.type_params, source, lines),
            ),
        ast::Stmt::Return(node) => PyAstNode::new("Return", Some(node.range), source, lines).child(
            "value",
            node.value
                .as_deref()
                .map(|value| expr_node(value, source, lines)),
        ),
        ast::Stmt::Delete(node) => PyAstNode::new("Delete", Some(node.range), source, lines)
            .children("targets", expr_nodes(&node.targets, source, lines)),
        ast::Stmt::Assign(node) => PyAstNode::new("Assign", Some(node.range), source, lines)
            .prop_opt("type_comment", node.type_comment.as_deref())
            .children("targets", expr_nodes(&node.targets, source, lines))
            .child("value", Some(expr_node(&node.value, source, lines))),
        ast::Stmt::TypeAlias(node) => PyAstNode::new("TypeAlias", Some(node.range), source, lines)
            .child("name", Some(expr_node(&node.name, source, lines)))
            .children(
                "type_params",
                type_param_nodes(&node.type_params, source, lines),
            )
            .child("value", Some(expr_node(&node.value, source, lines))),
        ast::Stmt::AugAssign(node) => PyAstNode::new("AugAssign", Some(node.range), source, lines)
            .prop("op", debug_name(node.op))
            .child("target", Some(expr_node(&node.target, source, lines)))
            .child("value", Some(expr_node(&node.value, source, lines))),
        ast::Stmt::AnnAssign(node) => PyAstNode::new("AnnAssign", Some(node.range), source, lines)
            .prop("simple", node.simple)
            .child("target", Some(expr_node(&node.target, source, lines)))
            .child(
                "annotation",
                Some(expr_node(&node.annotation, source, lines)),
            )
            .child(
                "value",
                node.value
                    .as_deref()
                    .map(|value| expr_node(value, source, lines)),
            ),
        ast::Stmt::For(node) => for_node(
            ForParts {
                kind: "For",
                range: node.range,
                target: &node.target,
                iter: &node.iter,
                body: &node.body,
                orelse: &node.orelse,
                type_comment: &node.type_comment,
            },
            source,
            lines,
        ),
        ast::Stmt::AsyncFor(node) => for_node(
            ForParts {
                kind: "AsyncFor",
                range: node.range,
                target: &node.target,
                iter: &node.iter,
                body: &node.body,
                orelse: &node.orelse,
                type_comment: &node.type_comment,
            },
            source,
            lines,
        ),
        ast::Stmt::While(node) => PyAstNode::new("While", Some(node.range), source, lines)
            .child("test", Some(expr_node(&node.test, source, lines)))
            .children("body", stmt_nodes(&node.body, source, lines))
            .children("orelse", stmt_nodes(&node.orelse, source, lines)),
        ast::Stmt::If(node) => PyAstNode::new("If", Some(node.range), source, lines)
            .child("test", Some(expr_node(&node.test, source, lines)))
            .children("body", stmt_nodes(&node.body, source, lines))
            .children("orelse", stmt_nodes(&node.orelse, source, lines)),
        ast::Stmt::With(node) => PyAstNode::new("With", Some(node.range), source, lines)
            .prop_opt("type_comment", node.type_comment.as_deref())
            .children("items", with_item_nodes(&node.items, source, lines))
            .children("body", stmt_nodes(&node.body, source, lines)),
        ast::Stmt::AsyncWith(node) => PyAstNode::new("AsyncWith", Some(node.range), source, lines)
            .prop_opt("type_comment", node.type_comment.as_deref())
            .children("items", with_item_nodes(&node.items, source, lines))
            .children("body", stmt_nodes(&node.body, source, lines)),
        ast::Stmt::Match(node) => PyAstNode::new("Match", Some(node.range), source, lines)
            .child("subject", Some(expr_node(&node.subject, source, lines)))
            .children("cases", match_case_nodes(&node.cases, source, lines)),
        ast::Stmt::Raise(node) => PyAstNode::new("Raise", Some(node.range), source, lines)
            .child(
                "exc",
                node.exc
                    .as_deref()
                    .map(|value| expr_node(value, source, lines)),
            )
            .child(
                "cause",
                node.cause
                    .as_deref()
                    .map(|value| expr_node(value, source, lines)),
            ),
        ast::Stmt::Try(node) => try_node(
            TryParts {
                kind: "Try",
                range: node.range,
                body: &node.body,
                handlers: &node.handlers,
                orelse: &node.orelse,
                finalbody: &node.finalbody,
            },
            source,
            lines,
        ),
        ast::Stmt::TryStar(node) => try_node(
            TryParts {
                kind: "TryStar",
                range: node.range,
                body: &node.body,
                handlers: &node.handlers,
                orelse: &node.orelse,
                finalbody: &node.finalbody,
            },
            source,
            lines,
        ),
        ast::Stmt::Assert(node) => PyAstNode::new("Assert", Some(node.range), source, lines)
            .child("test", Some(expr_node(&node.test, source, lines)))
            .child(
                "msg",
                node.msg
                    .as_deref()
                    .map(|value| expr_node(value, source, lines)),
            ),
        ast::Stmt::Import(node) => PyAstNode::new("Import", Some(node.range), source, lines)
            .children("names", alias_nodes(&node.names, source, lines)),
        ast::Stmt::ImportFrom(node) => {
            PyAstNode::new("ImportFrom", Some(node.range), source, lines)
                .prop_opt("module", node.module.as_ref().map(|value| value.as_str()))
                .prop_opt_u32("level", node.level.as_ref().map(|value| value.to_u32()))
                .children("names", alias_nodes(&node.names, source, lines))
        }
        ast::Stmt::Global(node) => PyAstNode::new("Global", Some(node.range), source, lines)
            .prop("names", identifiers(&node.names)),
        ast::Stmt::Nonlocal(node) => PyAstNode::new("Nonlocal", Some(node.range), source, lines)
            .prop("names", identifiers(&node.names)),
        ast::Stmt::Expr(node) => PyAstNode::new("Expr", Some(node.range), source, lines)
            .child("value", Some(expr_node(&node.value, source, lines))),
        ast::Stmt::Pass(node) => PyAstNode::new("Pass", Some(node.range), source, lines),
        ast::Stmt::Break(node) => PyAstNode::new("Break", Some(node.range), source, lines),
        ast::Stmt::Continue(node) => PyAstNode::new("Continue", Some(node.range), source, lines),
    }
}

struct FunctionParts<'a> {
    kind: &'a str,
    range: TextRange,
    name: &'a ast::Identifier,
    args: &'a ast::Arguments,
    body: &'a [ast::Stmt],
    decorator_list: &'a [ast::Expr],
    returns: Option<&'a ast::Expr>,
    type_comment: &'a Option<String>,
    type_params: &'a [ast::TypeParam],
}

fn function_node(parts: FunctionParts<'_>, source: &str, lines: &LineIndex) -> PyAstNode {
    PyAstNode::new(parts.kind, Some(parts.range), source, lines)
        .prop("name", parts.name.as_str())
        .prop_opt("type_comment", parts.type_comment.as_deref())
        .child("args", Some(arguments_node(parts.args, source, lines)))
        .children("body", stmt_nodes(parts.body, source, lines))
        .children(
            "decorator_list",
            expr_nodes(parts.decorator_list, source, lines),
        )
        .child(
            "returns",
            parts.returns.map(|value| expr_node(value, source, lines)),
        )
        .children(
            "type_params",
            type_param_nodes(parts.type_params, source, lines),
        )
}

struct ForParts<'a> {
    kind: &'a str,
    range: TextRange,
    target: &'a ast::Expr,
    iter: &'a ast::Expr,
    body: &'a [ast::Stmt],
    orelse: &'a [ast::Stmt],
    type_comment: &'a Option<String>,
}

fn for_node(parts: ForParts<'_>, source: &str, lines: &LineIndex) -> PyAstNode {
    PyAstNode::new(parts.kind, Some(parts.range), source, lines)
        .prop_opt("type_comment", parts.type_comment.as_deref())
        .child("target", Some(expr_node(parts.target, source, lines)))
        .child("iter", Some(expr_node(parts.iter, source, lines)))
        .children("body", stmt_nodes(parts.body, source, lines))
        .children("orelse", stmt_nodes(parts.orelse, source, lines))
}

struct TryParts<'a> {
    kind: &'a str,
    range: TextRange,
    body: &'a [ast::Stmt],
    handlers: &'a [ast::ExceptHandler],
    orelse: &'a [ast::Stmt],
    finalbody: &'a [ast::Stmt],
}

fn try_node(parts: TryParts<'_>, source: &str, lines: &LineIndex) -> PyAstNode {
    PyAstNode::new(parts.kind, Some(parts.range), source, lines)
        .children("body", stmt_nodes(parts.body, source, lines))
        .children(
            "handlers",
            except_handler_nodes(parts.handlers, source, lines),
        )
        .children("orelse", stmt_nodes(parts.orelse, source, lines))
        .children("finalbody", stmt_nodes(parts.finalbody, source, lines))
}

fn expr_node(expr: &ast::Expr, source: &str, lines: &LineIndex) -> PyAstNode {
    match expr {
        ast::Expr::BoolOp(node) => PyAstNode::new("BoolOp", Some(node.range), source, lines)
            .prop("op", debug_name(node.op))
            .children("values", expr_nodes(&node.values, source, lines)),
        ast::Expr::NamedExpr(node) => PyAstNode::new("NamedExpr", Some(node.range), source, lines)
            .child("target", Some(expr_node(&node.target, source, lines)))
            .child("value", Some(expr_node(&node.value, source, lines))),
        ast::Expr::BinOp(node) => PyAstNode::new("BinOp", Some(node.range), source, lines)
            .prop("op", debug_name(node.op))
            .child("left", Some(expr_node(&node.left, source, lines)))
            .child("right", Some(expr_node(&node.right, source, lines))),
        ast::Expr::UnaryOp(node) => PyAstNode::new("UnaryOp", Some(node.range), source, lines)
            .prop("op", debug_name(node.op))
            .child("operand", Some(expr_node(&node.operand, source, lines))),
        ast::Expr::Lambda(node) => PyAstNode::new("Lambda", Some(node.range), source, lines)
            .child("args", Some(arguments_node(&node.args, source, lines)))
            .child("body", Some(expr_node(&node.body, source, lines))),
        ast::Expr::IfExp(node) => PyAstNode::new("IfExp", Some(node.range), source, lines)
            .child("test", Some(expr_node(&node.test, source, lines)))
            .child("body", Some(expr_node(&node.body, source, lines)))
            .child("orelse", Some(expr_node(&node.orelse, source, lines))),
        ast::Expr::Dict(node) => PyAstNode::new("Dict", Some(node.range), source, lines)
            .children(
                "keys",
                optional_expr_nodes(&node.keys, "DictUnpack", source, lines),
            )
            .children("values", expr_nodes(&node.values, source, lines)),
        ast::Expr::Set(node) => PyAstNode::new("Set", Some(node.range), source, lines)
            .children("elts", expr_nodes(&node.elts, source, lines)),
        ast::Expr::ListComp(node) => comprehension_expr_node(
            "ListComp",
            node.range,
            Some(("elt", &node.elt)),
            None,
            &node.generators,
            source,
            lines,
        ),
        ast::Expr::SetComp(node) => comprehension_expr_node(
            "SetComp",
            node.range,
            Some(("elt", &node.elt)),
            None,
            &node.generators,
            source,
            lines,
        ),
        ast::Expr::DictComp(node) => comprehension_expr_node(
            "DictComp",
            node.range,
            Some(("key", &node.key)),
            Some(("value", &node.value)),
            &node.generators,
            source,
            lines,
        ),
        ast::Expr::GeneratorExp(node) => comprehension_expr_node(
            "GeneratorExp",
            node.range,
            Some(("elt", &node.elt)),
            None,
            &node.generators,
            source,
            lines,
        ),
        ast::Expr::Await(node) => PyAstNode::new("Await", Some(node.range), source, lines)
            .child("value", Some(expr_node(&node.value, source, lines))),
        ast::Expr::Yield(node) => PyAstNode::new("Yield", Some(node.range), source, lines).child(
            "value",
            node.value
                .as_deref()
                .map(|value| expr_node(value, source, lines)),
        ),
        ast::Expr::YieldFrom(node) => PyAstNode::new("YieldFrom", Some(node.range), source, lines)
            .child("value", Some(expr_node(&node.value, source, lines))),
        ast::Expr::Compare(node) => PyAstNode::new("Compare", Some(node.range), source, lines)
            .prop("ops", node.ops.iter().map(debug_name).collect::<Vec<_>>())
            .child("left", Some(expr_node(&node.left, source, lines)))
            .children("comparators", expr_nodes(&node.comparators, source, lines)),
        ast::Expr::Call(node) => PyAstNode::new("Call", Some(node.range), source, lines)
            .child("func", Some(expr_node(&node.func, source, lines)))
            .children("args", expr_nodes(&node.args, source, lines))
            .children("keywords", keyword_nodes(&node.keywords, source, lines)),
        ast::Expr::FormattedValue(node) => {
            PyAstNode::new("FormattedValue", Some(node.range), source, lines)
                .prop("conversion", debug_name(node.conversion))
                .child("value", Some(expr_node(&node.value, source, lines)))
                .child(
                    "format_spec",
                    node.format_spec
                        .as_deref()
                        .map(|value| expr_node(value, source, lines)),
                )
        }
        ast::Expr::JoinedStr(node) => PyAstNode::new("JoinedStr", Some(node.range), source, lines)
            .children("values", expr_nodes(&node.values, source, lines)),
        ast::Expr::Constant(node) => PyAstNode::new("Constant", Some(node.range), source, lines)
            .prop("value_kind", constant_kind(&node.value))
            .prop("value", constant_value(&node.value))
            .prop_opt("literal_kind", node.kind.as_deref()),
        ast::Expr::Attribute(node) => PyAstNode::new("Attribute", Some(node.range), source, lines)
            .prop("attr", node.attr.as_str())
            .prop("ctx", debug_name(node.ctx))
            .child("value", Some(expr_node(&node.value, source, lines))),
        ast::Expr::Subscript(node) => PyAstNode::new("Subscript", Some(node.range), source, lines)
            .prop("ctx", debug_name(node.ctx))
            .child("value", Some(expr_node(&node.value, source, lines)))
            .child("slice", Some(expr_node(&node.slice, source, lines))),
        ast::Expr::Starred(node) => PyAstNode::new("Starred", Some(node.range), source, lines)
            .prop("ctx", debug_name(node.ctx))
            .child("value", Some(expr_node(&node.value, source, lines))),
        ast::Expr::Name(node) => PyAstNode::new("Name", Some(node.range), source, lines)
            .prop("id", node.id.as_str())
            .prop("ctx", debug_name(node.ctx)),
        ast::Expr::List(node) => PyAstNode::new("List", Some(node.range), source, lines)
            .prop("ctx", debug_name(node.ctx))
            .children("elts", expr_nodes(&node.elts, source, lines)),
        ast::Expr::Tuple(node) => PyAstNode::new("Tuple", Some(node.range), source, lines)
            .prop("ctx", debug_name(node.ctx))
            .children("elts", expr_nodes(&node.elts, source, lines)),
        ast::Expr::Slice(node) => PyAstNode::new("Slice", Some(node.range), source, lines)
            .child(
                "lower",
                node.lower
                    .as_deref()
                    .map(|value| expr_node(value, source, lines)),
            )
            .child(
                "upper",
                node.upper
                    .as_deref()
                    .map(|value| expr_node(value, source, lines)),
            )
            .child(
                "step",
                node.step
                    .as_deref()
                    .map(|value| expr_node(value, source, lines)),
            ),
    }
}

fn comprehension_expr_node(
    kind: &str,
    range: TextRange,
    first_expr: Option<(&str, &ast::Expr)>,
    second_expr: Option<(&str, &ast::Expr)>,
    generators: &[ast::Comprehension],
    source: &str,
    lines: &LineIndex,
) -> PyAstNode {
    let mut node = PyAstNode::new(kind, Some(range), source, lines);
    if let Some((name, value)) = first_expr {
        node = node.child(name, Some(expr_node(value, source, lines)));
    }
    if let Some((name, value)) = second_expr {
        node = node.child(name, Some(expr_node(value, source, lines)));
    }
    node.children("generators", comprehension_nodes(generators, source, lines))
}

fn arguments_node(args: &ast::Arguments, source: &str, lines: &LineIndex) -> PyAstNode {
    PyAstNode::new("Arguments", Some(args.range), source, lines)
        .children(
            "posonlyargs",
            arg_with_default_nodes(&args.posonlyargs, source, lines),
        )
        .children("args", arg_with_default_nodes(&args.args, source, lines))
        .child(
            "vararg",
            args.vararg
                .as_deref()
                .map(|value| arg_node(value, source, lines)),
        )
        .children(
            "kwonlyargs",
            arg_with_default_nodes(&args.kwonlyargs, source, lines),
        )
        .child(
            "kwarg",
            args.kwarg
                .as_deref()
                .map(|value| arg_node(value, source, lines)),
        )
}

fn arg_with_default_node(arg: &ast::ArgWithDefault, source: &str, lines: &LineIndex) -> PyAstNode {
    PyAstNode::new("ArgWithDefault", Some(arg.range), source, lines)
        .child("def", Some(arg_node(&arg.def, source, lines)))
        .child(
            "default",
            arg.default
                .as_deref()
                .map(|value| expr_node(value, source, lines)),
        )
}

fn arg_node(arg: &ast::Arg, source: &str, lines: &LineIndex) -> PyAstNode {
    PyAstNode::new("Arg", Some(arg.range), source, lines)
        .prop("arg", arg.arg.as_str())
        .prop_opt("type_comment", arg.type_comment.as_deref())
        .child(
            "annotation",
            arg.annotation
                .as_deref()
                .map(|value| expr_node(value, source, lines)),
        )
}

fn keyword_node(keyword: &ast::Keyword, source: &str, lines: &LineIndex) -> PyAstNode {
    PyAstNode::new("Keyword", Some(keyword.range), source, lines)
        .prop_opt("arg", keyword.arg.as_ref().map(|value| value.as_str()))
        .child("value", Some(expr_node(&keyword.value, source, lines)))
}

fn alias_node(alias: &ast::Alias, source: &str, lines: &LineIndex) -> PyAstNode {
    PyAstNode::new("Alias", Some(alias.range), source, lines)
        .prop("name", alias.name.as_str())
        .prop_opt("asname", alias.asname.as_ref().map(|value| value.as_str()))
}

fn with_item_node(item: &ast::WithItem, source: &str, lines: &LineIndex) -> PyAstNode {
    PyAstNode::new("WithItem", Some(item.range), source, lines)
        .child(
            "context_expr",
            Some(expr_node(&item.context_expr, source, lines)),
        )
        .child(
            "optional_vars",
            item.optional_vars
                .as_deref()
                .map(|value| expr_node(value, source, lines)),
        )
}

fn match_case_node(case: &ast::MatchCase, source: &str, lines: &LineIndex) -> PyAstNode {
    PyAstNode::new("MatchCase", Some(case.range), source, lines)
        .child("pattern", Some(pattern_node(&case.pattern, source, lines)))
        .child(
            "guard",
            case.guard
                .as_deref()
                .map(|value| expr_node(value, source, lines)),
        )
        .children("body", stmt_nodes(&case.body, source, lines))
}

fn pattern_node(pattern: &ast::Pattern, source: &str, lines: &LineIndex) -> PyAstNode {
    match pattern {
        ast::Pattern::MatchValue(node) => {
            PyAstNode::new("MatchValue", Some(node.range), source, lines)
                .child("value", Some(expr_node(&node.value, source, lines)))
        }
        ast::Pattern::MatchSingleton(node) => {
            PyAstNode::new("MatchSingleton", Some(node.range), source, lines)
                .prop("value_kind", constant_kind(&node.value))
                .prop("value", constant_value(&node.value))
        }
        ast::Pattern::MatchSequence(node) => {
            PyAstNode::new("MatchSequence", Some(node.range), source, lines)
                .children("patterns", pattern_nodes(&node.patterns, source, lines))
        }
        ast::Pattern::MatchMapping(node) => {
            PyAstNode::new("MatchMapping", Some(node.range), source, lines)
                .prop_opt("rest", node.rest.as_ref().map(|value| value.as_str()))
                .children("keys", expr_nodes(&node.keys, source, lines))
                .children("patterns", pattern_nodes(&node.patterns, source, lines))
        }
        ast::Pattern::MatchClass(node) => {
            PyAstNode::new("MatchClass", Some(node.range), source, lines)
                .prop("kwd_attrs", identifiers(&node.kwd_attrs))
                .child("cls", Some(expr_node(&node.cls, source, lines)))
                .children("patterns", pattern_nodes(&node.patterns, source, lines))
                .children(
                    "kwd_patterns",
                    pattern_nodes(&node.kwd_patterns, source, lines),
                )
        }
        ast::Pattern::MatchStar(node) => {
            PyAstNode::new("MatchStar", Some(node.range), source, lines)
                .prop_opt("name", node.name.as_ref().map(|value| value.as_str()))
        }
        ast::Pattern::MatchAs(node) => PyAstNode::new("MatchAs", Some(node.range), source, lines)
            .prop_opt("name", node.name.as_ref().map(|value| value.as_str()))
            .child(
                "pattern",
                node.pattern
                    .as_deref()
                    .map(|value| pattern_node(value, source, lines)),
            ),
        ast::Pattern::MatchOr(node) => PyAstNode::new("MatchOr", Some(node.range), source, lines)
            .children("patterns", pattern_nodes(&node.patterns, source, lines)),
    }
}

fn comprehension_node(
    comprehension: &ast::Comprehension,
    source: &str,
    lines: &LineIndex,
) -> PyAstNode {
    PyAstNode::new("Comprehension", Some(comprehension.range), source, lines)
        .prop("is_async", comprehension.is_async)
        .child(
            "target",
            Some(expr_node(&comprehension.target, source, lines)),
        )
        .child("iter", Some(expr_node(&comprehension.iter, source, lines)))
        .children("ifs", expr_nodes(&comprehension.ifs, source, lines))
}

fn except_handler_node(handler: &ast::ExceptHandler, source: &str, lines: &LineIndex) -> PyAstNode {
    match handler {
        ast::ExceptHandler::ExceptHandler(node) => {
            PyAstNode::new("ExceptHandler", Some(node.range), source, lines)
                .prop_opt("name", node.name.as_ref().map(|value| value.as_str()))
                .child(
                    "type",
                    node.type_
                        .as_deref()
                        .map(|value| expr_node(value, source, lines)),
                )
                .children("body", stmt_nodes(&node.body, source, lines))
        }
    }
}

fn type_param_node(type_param: &ast::TypeParam, source: &str, lines: &LineIndex) -> PyAstNode {
    match type_param {
        ast::TypeParam::TypeVar(node) => PyAstNode::new("TypeVar", Some(node.range), source, lines)
            .prop("name", node.name.as_str())
            .child(
                "bound",
                node.bound
                    .as_deref()
                    .map(|value| expr_node(value, source, lines)),
            ),
        ast::TypeParam::ParamSpec(node) => {
            PyAstNode::new("ParamSpec", Some(node.range), source, lines)
                .prop("name", node.name.as_str())
        }
        ast::TypeParam::TypeVarTuple(node) => {
            PyAstNode::new("TypeVarTuple", Some(node.range), source, lines)
                .prop("name", node.name.as_str())
        }
    }
}

fn stmt_nodes(nodes: &[ast::Stmt], source: &str, lines: &LineIndex) -> Vec<PyAstNode> {
    nodes
        .iter()
        .map(|node| stmt_node(node, source, lines))
        .collect()
}

fn expr_nodes(nodes: &[ast::Expr], source: &str, lines: &LineIndex) -> Vec<PyAstNode> {
    nodes
        .iter()
        .map(|node| expr_node(node, source, lines))
        .collect()
}

fn optional_expr_nodes(
    nodes: &[Option<ast::Expr>],
    none_kind: &str,
    source: &str,
    lines: &LineIndex,
) -> Vec<PyAstNode> {
    nodes
        .iter()
        .map(|node| {
            node.as_ref()
                .map(|node| expr_node(node, source, lines))
                .unwrap_or_else(|| PyAstNode::new(none_kind, None, source, lines))
        })
        .collect()
}

fn keyword_nodes(nodes: &[ast::Keyword], source: &str, lines: &LineIndex) -> Vec<PyAstNode> {
    nodes
        .iter()
        .map(|node| keyword_node(node, source, lines))
        .collect()
}

fn alias_nodes(nodes: &[ast::Alias], source: &str, lines: &LineIndex) -> Vec<PyAstNode> {
    nodes
        .iter()
        .map(|node| alias_node(node, source, lines))
        .collect()
}

fn with_item_nodes(nodes: &[ast::WithItem], source: &str, lines: &LineIndex) -> Vec<PyAstNode> {
    nodes
        .iter()
        .map(|node| with_item_node(node, source, lines))
        .collect()
}

fn match_case_nodes(nodes: &[ast::MatchCase], source: &str, lines: &LineIndex) -> Vec<PyAstNode> {
    nodes
        .iter()
        .map(|node| match_case_node(node, source, lines))
        .collect()
}

fn pattern_nodes(nodes: &[ast::Pattern], source: &str, lines: &LineIndex) -> Vec<PyAstNode> {
    nodes
        .iter()
        .map(|node| pattern_node(node, source, lines))
        .collect()
}

fn comprehension_nodes(
    nodes: &[ast::Comprehension],
    source: &str,
    lines: &LineIndex,
) -> Vec<PyAstNode> {
    nodes
        .iter()
        .map(|node| comprehension_node(node, source, lines))
        .collect()
}

fn except_handler_nodes(
    nodes: &[ast::ExceptHandler],
    source: &str,
    lines: &LineIndex,
) -> Vec<PyAstNode> {
    nodes
        .iter()
        .map(|node| except_handler_node(node, source, lines))
        .collect()
}

fn type_param_nodes(nodes: &[ast::TypeParam], source: &str, lines: &LineIndex) -> Vec<PyAstNode> {
    nodes
        .iter()
        .map(|node| type_param_node(node, source, lines))
        .collect()
}

fn arg_with_default_nodes(
    nodes: &[ast::ArgWithDefault],
    source: &str,
    lines: &LineIndex,
) -> Vec<PyAstNode> {
    nodes
        .iter()
        .map(|node| arg_with_default_node(node, source, lines))
        .collect()
}

fn identifiers(values: &[ast::Identifier]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.as_str().to_owned())
        .collect()
}

fn constant_kind(value: &ast::Constant) -> &'static str {
    match value {
        ast::Constant::None => "None",
        ast::Constant::Bool(_) => "Bool",
        ast::Constant::Str(_) => "Str",
        ast::Constant::Bytes(_) => "Bytes",
        ast::Constant::Int(_) => "Int",
        ast::Constant::Tuple(_) => "Tuple",
        ast::Constant::Float(_) => "Float",
        ast::Constant::Complex { .. } => "Complex",
        ast::Constant::Ellipsis => "Ellipsis",
    }
}

fn constant_value(value: &ast::Constant) -> Value {
    match value {
        ast::Constant::None => Value::Null,
        ast::Constant::Bool(value) => json!(value),
        ast::Constant::Str(value) => json!(value),
        ast::Constant::Bytes(value) => json!(value),
        ast::Constant::Int(value) => json!(value.to_string()),
        ast::Constant::Tuple(values) => {
            json!(values.iter().map(constant_value).collect::<Vec<_>>())
        }
        ast::Constant::Float(value) => json!(value),
        ast::Constant::Complex { real, imag } => json!({ "real": real, "imag": imag }),
        ast::Constant::Ellipsis => json!("..."),
    }
}

fn debug_name(value: impl std::fmt::Debug) -> String {
    format!("{value:?}")
}

trait OptionalProperties {
    fn prop_opt(self, name: impl Into<String>, value: Option<&str>) -> Self;
    fn prop_opt_u32(self, name: impl Into<String>, value: Option<u32>) -> Self;
}

impl OptionalProperties for PyAstNode {
    fn prop_opt(mut self, name: impl Into<String>, value: Option<&str>) -> Self {
        if let Some(value) = value {
            self.properties.insert(name.into(), json!(value));
        }
        self
    }

    fn prop_opt_u32(mut self, name: impl Into<String>, value: Option<u32>) -> Self {
        if let Some(value) = value {
            self.properties.insert(name.into(), json!(value));
        }
        self
    }
}

fn source_range(range: TextRange, source: &str, lines: &LineIndex) -> SourceRange {
    let start_offset = usize::from(range.start()).min(source.len());
    let end_offset = usize::from(range.end()).min(source.len());
    let (start_line, start_column) = lines.line_column(start_offset);
    let (end_line, end_column) = lines.line_column(end_offset);
    SourceRange {
        start_offset,
        end_offset,
        start_line,
        start_column,
        end_line,
        end_column,
    }
}

struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(source: &str) -> Self {
        let mut starts = vec![0];
        for (index, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                starts.push(index + 1);
            }
        }
        Self { starts }
    }

    fn line_column(&self, offset: usize) -> (usize, usize) {
        let line_index = self
            .starts
            .partition_point(|start| *start <= offset)
            .saturating_sub(1);
        let line_start = self.starts.get(line_index).copied().unwrap_or(0);
        let column = offset.saturating_sub(line_start) + 1;
        (line_index + 1, column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parses_functions_classes_and_control_flow() {
        let source = r#"
class Service:
    def run(self, xs):
        for x in xs:
            if x > 0:
                print(x)
"#;

        let document = parse_source(Path::new("service.py"), source).unwrap();
        assert_eq!(document.backend, "oxidized-pyastgen");
        assert_eq!(document.root.kind, "Module");

        let class_node = &document.root.children["body"][0];
        assert_eq!(class_node.kind, "ClassDef");
        assert_eq!(class_node.properties["name"], "Service");

        let function_node = &class_node.children["body"][0];
        assert_eq!(function_node.kind, "FunctionDef");
        assert_eq!(function_node.properties["name"], "run");

        let for_node = &function_node.children["body"][0];
        assert_eq!(for_node.kind, "For");
        assert_eq!(for_node.children["target"][0].properties["id"], "x");
        assert_eq!(for_node.children["iter"][0].properties["id"], "xs");

        let if_node = &for_node.children["body"][0];
        assert_eq!(if_node.kind, "If");
        assert_eq!(if_node.children["test"][0].kind, "Compare");
    }

    #[test]
    fn preserves_offsets_and_text() {
        let source = "name = 'Ada'\nprint(name)\n";
        let document = parse_source(Path::new("main.py"), source).unwrap();
        let assign = &document.root.children["body"][0];
        assert_eq!(assign.kind, "Assign");
        assert_eq!(assign.text.as_deref(), Some("name = 'Ada'"));
        assert_eq!(assign.range.as_ref().unwrap().start_line, 1);
        assert_eq!(assign.range.as_ref().unwrap().start_column, 1);
        assert_eq!(assign.range.as_ref().unwrap().end_line, 1);
    }
}
