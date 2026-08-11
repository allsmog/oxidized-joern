//! A Python frontend built on tree-sitter.
//!
//! The second frontend behind the shared contract — its existence is the
//! consolidation argument made concrete. Note the size: this file is a thin
//! mapping from Python's grammar onto the same builder primitives the C
//! frontend uses. Everything semantic (CFG, symbol/call resolution, dataflow
//! summaries, incrementality) is shared and required zero changes to support a
//! dynamically-typed, indentation-structured language.

use cpg_core::{Cpg, NodeId};
use cpg_frontend::{BuildResult, Frontend, Language, LanguageTraits};
use tree_sitter::{Node, Parser};

pub struct Python;

impl Language for Python {
    fn name(&self) -> &'static str {
        "Python"
    }
    fn namespace_delimiter(&self) -> &'static str {
        "."
    }
    fn traits(&self) -> LanguageTraits {
        LanguageTraits::HAS_CLASSES
            | LanguageTraits::HAS_DEFAULT_ARGS
            | LanguageTraits::ALLOWS_FORWARD_REFS
            | LanguageTraits::STRUCTURAL_TYPING
    }
    fn file_extensions(&self) -> &'static [&'static str] {
        &["py"]
    }
}

pub struct PythonFrontend {
    lang: Python,
    parser: Parser,
}

impl Default for PythonFrontend {
    fn default() -> Self {
        Self::new()
    }
}

impl PythonFrontend {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .expect("load Python grammar");
        PythonFrontend {
            lang: Python,
            parser,
        }
    }
}

impl Frontend for PythonFrontend {
    fn language(&self) -> &dyn Language {
        &self.lang
    }

    fn build_file(&mut self, cpg: &mut Cpg, path: &str, source: &str) -> BuildResult {
        let tree = self.parser.parse(source, None).expect("parse");
        let root = tree.root_node();
        let file = cpg.file_id(path);
        let mut b = cpg_core::CpgBuilder::new(cpg, file);
        let file_node = b.file_node(path);

        let mut methods = 0usize;
        let src = source.as_bytes();
        for child in named_children(root) {
            match child.kind() {
                "function_definition" => {
                    if build_function(&mut b, file_node, child, src).is_some() {
                        methods += 1;
                    }
                }
                // Module-level statements: build them under the file node so
                // top-level calls (imports aside) are visible to queries.
                _ => build_stmt(&mut b, file_node, child, src),
            }
        }
        BuildResult {
            file,
            methods_built: methods,
        }
    }
}

fn named_children(node: Node) -> Vec<Node> {
    let mut cur = node.walk();
    node.named_children(&mut cur).collect()
}

fn text<'a>(node: Node, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or("")
}

fn line(node: Node) -> Option<u32> {
    Some(node.start_position().row as u32 + 1)
}

fn build_function(
    b: &mut cpg_core::CpgBuilder,
    file_node: NodeId,
    node: Node,
    src: &[u8],
) -> Option<NodeId> {
    let name = node.child_by_field_name("name").map(|n| text(n, src))?;
    let method = b.method(name, name, &format!("{name}()"), line(node));
    b.contains(file_node, method);

    if let Some(params) = node.child_by_field_name("parameters") {
        let mut idx = 1i32;
        for p in named_children(params) {
            let (pname, ptype) = match p.kind() {
                "identifier" => (text(p, src), "ANY"),
                "typed_parameter" => {
                    let n = named_children(p)
                        .first()
                        .map(|c| text(*c, src))
                        .unwrap_or("");
                    let t = p
                        .child_by_field_name("type")
                        .map(|t| text(t, src))
                        .unwrap_or("ANY");
                    (n, t)
                }
                "default_parameter" | "typed_default_parameter" => {
                    let n = p
                        .child_by_field_name("name")
                        .map(|n| text(n, src))
                        .unwrap_or("");
                    (n, "ANY")
                }
                // *args / **kwargs and friends.
                "list_splat_pattern" | "dictionary_splat_pattern" => {
                    (innermost_identifier(p, src), "ANY")
                }
                _ => continue,
            };
            if pname.is_empty() {
                continue;
            }
            let param = b.parameter(pname, ptype, idx);
            b.ast_child(method, param);
            idx += 1;
        }
    }
    let ret = b.method_return("ANY");
    b.ast_child(method, ret);

    let block = b.block();
    b.ast_child(method, block);
    if let Some(body) = node.child_by_field_name("body") {
        for stmt in named_children(body) {
            build_stmt(b, block, stmt, src);
        }
    }
    Some(method)
}

fn build_stmt(b: &mut cpg_core::CpgBuilder, parent: NodeId, node: Node, src: &[u8]) {
    match node.kind() {
        "return_statement" => {
            let ret = b.ret(text(node, src), line(node));
            b.ast_child(parent, ret);
            for c in named_children(node) {
                if let Some(e) = build_expr(b, c, src) {
                    b.ast_child(ret, e);
                }
            }
        }
        "if_statement" | "while_statement" | "for_statement" | "try_statement"
        | "with_statement" | "match_statement" => {
            let cs = b.control_structure(node.kind(), line(node));
            b.ast_child(parent, cs);
            for c in named_children(node) {
                build_stmt(b, cs, c, src);
            }
        }
        "block" | "elif_clause" | "else_clause" | "except_clause" | "finally_clause"
        | "case_clause" => {
            for c in named_children(node) {
                build_stmt(b, parent, c, src);
            }
        }
        "expression_statement" => {
            for c in named_children(node) {
                if let Some(e) = build_expr(b, c, src) {
                    b.ast_child(parent, e);
                }
            }
        }
        "function_definition" => {
            // Nested defs: attach to the enclosing scope's parent chain.
            build_function(b, parent, node, src);
        }
        "import_statement" | "import_from_statement" | "comment" => {}
        _ => {
            if let Some(e) = build_expr(b, node, src) {
                b.ast_child(parent, e);
            }
        }
    }
}

fn build_expr(b: &mut cpg_core::CpgBuilder, node: Node, src: &[u8]) -> Option<NodeId> {
    match node.kind() {
        "call" => {
            let name = node
                .child_by_field_name("function")
                .map(|c| text(c, src))
                .unwrap_or("<anon>");
            let call = b.call(name, text(node, src), line(node));
            if let Some(args) = node.child_by_field_name("arguments") {
                let mut idx = 1i32;
                for a in named_children(args) {
                    if let Some(arg) = build_expr(b, a, src) {
                        b.add_argument(call, arg, idx);
                        idx += 1;
                    }
                }
            }
            Some(call)
        }
        "assignment" | "augmented_assignment" => {
            let call = b.call("=", text(node, src), line(node));
            if let Some(lhs) = node
                .child_by_field_name("left")
                .and_then(|n| build_expr(b, n, src))
            {
                b.add_argument(call, lhs, 1);
            }
            if let Some(rhs) = node
                .child_by_field_name("right")
                .and_then(|n| build_expr(b, n, src))
            {
                b.add_argument(call, rhs, 2);
            }
            Some(call)
        }
        "binary_operator" | "boolean_operator" | "comparison_operator" => {
            let op = node
                .child_by_field_name("operator")
                .map(|n| text(n, src))
                .unwrap_or("<op>");
            let call = b.call(op, text(node, src), line(node));
            let mut idx = 1i32;
            for c in named_children(node) {
                if let Some(e) = build_expr(b, c, src) {
                    b.add_argument(call, e, idx);
                    idx += 1;
                }
            }
            Some(call)
        }
        "parenthesized_expression" => named_children(node)
            .into_iter()
            .find_map(|c| build_expr(b, c, src)),
        "identifier" => Some(b.identifier(text(node, src), line(node))),
        "integer" | "float" | "string" | "true" | "false" | "none" => {
            Some(b.literal(text(node, src), line(node)))
        }
        _ => named_children(node)
            .into_iter()
            .find_map(|c| build_expr(b, c, src)),
    }
}

fn innermost_identifier<'a>(node: Node, src: &'a [u8]) -> &'a str {
    if node.kind() == "identifier" {
        return text(node, src);
    }
    for c in named_children(node) {
        let r = innermost_identifier(c, src);
        if !r.is_empty() {
            return r;
        }
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpg_core::Query;

    #[test]
    fn parses_function_with_params_and_call() {
        let mut cpg = Cpg::new();
        let mut fe = PythonFrontend::new();
        let code = r#"
def add(a, b):
    return a + b

def main():
    r = add(1, 2)
    print("hello")
"#;
        fe.build_file(&mut cpg, "t.py", code);
        let add = cpg.method_named("add");
        assert_eq!(add.len(), 1);
        assert_eq!(cpg.parameters_of(add[0]).len(), 2);
        assert_eq!(cpg.calls_named("add").len(), 1);
        let print = cpg.calls_named("print");
        assert_eq!(print.len(), 1);
        assert_eq!(cpg.arguments_of(print[0]).len(), 1);
    }

    #[test]
    fn tolerates_broken_code() {
        let mut cpg = Cpg::new();
        let mut fe = PythonFrontend::new();
        let r = fe.build_file(&mut cpg, "b.py", "def f(x):\n    return g(x.\n");
        assert_eq!(r.methods_built, 1);
    }
}
