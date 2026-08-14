use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser, Point, Tree};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JavaAstDocument {
    pub full_name: String,
    pub relative_name: String,
    pub ast: JavaAstNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JavaAstNode {
    pub kind: String,
    pub field_name: Option<String>,
    pub named: bool,
    pub missing: bool,
    pub extra: bool,
    pub has_error: bool,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start: SourcePoint,
    pub end: SourcePoint,
    pub code: String,
    pub children: Vec<JavaAstNode>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourcePoint {
    pub line: usize,
    pub column: usize,
}

pub fn parse_file(root: &Path, path: &Path) -> Result<JavaAstDocument> {
    let content =
        fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_source(root, path, &content)
}

pub fn parse_source(root: &Path, path: &Path, source: &str) -> Result<JavaAstDocument> {
    let (tree, node_source) = parse_tree_for_source(source)?;
    if tree.root_node().has_error() {
        bail!("parser reported syntax errors in {}", path.display());
    }

    let root_node = node_json(tree.root_node(), None, node_source);
    Ok(JavaAstDocument {
        full_name: full_name(path),
        relative_name: relative_name(root, path),
        ast: root_node,
    })
}

pub fn write_json(path: &Path, document: &JavaAstDocument) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(document)?;
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

pub fn collect_kind_counts(document: &JavaAstDocument) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    collect_kind_counts_from_node(&document.ast, &mut counts);
    counts
}

fn collect_kind_counts_from_node(node: &JavaAstNode, counts: &mut BTreeMap<String, usize>) {
    *counts.entry(node.kind.clone()).or_insert(0) += 1;
    for child in &node.children {
        collect_kind_counts_from_node(child, counts);
    }
}

fn parse_tree(source: &str) -> Result<Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .context("initializing Java parser")?;
    parser
        .parse(source, None)
        .context("parser returned no tree")
}

fn parse_tree_for_source(source: &str) -> Result<(Tree, &str)> {
    let tree = parse_tree(source)?;
    if !tree.root_node().has_error() {
        return Ok((tree, source));
    }

    if let Some(sanitized) = sanitize_module_imports(source) {
        let sanitized_tree = parse_tree(&sanitized)?;
        if !sanitized_tree.root_node().has_error() {
            // The sanitizer preserves byte length, so tree-sitter positions still
            // slice the original source correctly when JSON node code is emitted.
            return Ok((sanitized_tree, source));
        }
    }

    Ok((tree, source))
}

fn sanitize_module_imports(source: &str) -> Option<String> {
    let mut changed = false;
    let mut sanitized = String::with_capacity(source.len());

    for segment in source.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map(|line| (line, "\n"))
            .unwrap_or((segment, ""));
        if let Some(clean_line) = sanitize_module_import_line(line) {
            changed = true;
            sanitized.push_str(&clean_line);
        } else {
            sanitized.push_str(line);
        }
        sanitized.push_str(newline);
    }

    changed.then_some(sanitized)
}

fn sanitize_module_import_line(line: &str) -> Option<String> {
    let leading = line.len() - line.trim_start().len();
    let after_import = keyword_followed_by_whitespace(line, leading, "import")?;
    let module_start = skip_ascii_whitespace(line, after_import);
    let after_module = keyword_followed_by_whitespace(line, module_start, "module")?;
    let name_start = skip_ascii_whitespace(line, after_module);
    if name_start == after_module {
        return None;
    }

    let replacement_len = name_start - module_start;
    let mut replacement = String::from("/*m*/");
    replacement.push_str(&" ".repeat(replacement_len.saturating_sub(replacement.len())));

    let mut sanitized = String::with_capacity(line.len());
    sanitized.push_str(&line[..module_start]);
    sanitized.push_str(&replacement);
    sanitized.push_str(&line[name_start..]);
    Some(sanitized)
}

fn keyword_followed_by_whitespace(line: &str, start: usize, keyword: &str) -> Option<usize> {
    let end = start + keyword.len();
    if line.get(start..end)? != keyword {
        return None;
    }
    line.as_bytes()
        .get(end)
        .filter(|byte| byte.is_ascii_whitespace())
        .map(|_| end)
}

fn skip_ascii_whitespace(line: &str, mut index: usize) -> usize {
    while line
        .as_bytes()
        .get(index)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        index += 1;
    }
    index
}

fn node_json(node: Node<'_>, field_name: Option<String>, source: &str) -> JavaAstNode {
    let mut children = Vec::new();
    let mut cursor = node.walk();
    for (index, child) in (0u32..).zip(node.children(&mut cursor)) {
        let child_field_name = node.field_name_for_child(index).map(str::to_string);
        children.push(node_json(child, child_field_name, source));
    }

    JavaAstNode {
        kind: node.kind().to_string(),
        field_name,
        named: node.is_named(),
        missing: node.is_missing(),
        extra: node.is_extra(),
        has_error: node.has_error(),
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start: point_json(node.start_position()),
        end: point_json(node.end_position()),
        code: source_excerpt(source, node.start_byte(), node.end_byte()).to_string(),
        children,
    }
}

fn point_json(point: Point) -> SourcePoint {
    SourcePoint {
        line: point.row + 1,
        column: point.column + 1,
    }
}

fn source_excerpt(source: &str, start: usize, end: usize) -> &str {
    source.get(start..end).unwrap_or("")
}

fn full_name(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn relative_name(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or_else(|_| {
            path.file_name()
                .map(Path::new)
                .unwrap_or_else(|| Path::new(""))
        })
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn output_path(input: &Path, out: &Path, file: &Path) -> PathBuf {
    let relative = if input.is_dir() {
        file.strip_prefix(input).unwrap_or(file)
    } else {
        file.file_name().map(Path::new).unwrap_or(file)
    };
    let mut target = out.join(relative);
    let file_name = target
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| format!("{value}.json"))
        .unwrap_or_else(|| "out.json".into());
    target.set_file_name(file_name);
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_class_with_spans_and_code() {
        let source = "package demo;\nclass Foo { int x = 1; }\n";
        let document = parse_source(Path::new("."), Path::new("Foo.java"), source).unwrap();
        assert_eq!(document.relative_name, "Foo.java");
        assert_eq!(document.ast.kind, "program");
        assert_eq!(document.ast.start.line, 1);
        assert_eq!(document.ast.start.column, 1);
        assert_eq!(document.ast.end.line, 3);
        assert_eq!(document.ast.code, source);

        let counts = collect_kind_counts(&document);
        assert!(counts.contains_key("package_declaration"));
        assert!(counts.contains_key("class_declaration"));
        assert!(counts.contains_key("field_declaration"));
    }

    #[test]
    fn rejects_syntax_errors() {
        let err = parse_source(Path::new("."), Path::new("Broken.java"), "class {").unwrap_err();
        assert!(err.to_string().contains("syntax errors"));
    }

    #[test]
    fn parses_module_imports_and_preserves_original_code() {
        let source = "import module java.base;\nclass Foo {}\n";
        let document = parse_source(Path::new("."), Path::new("Foo.java"), source).unwrap();

        let import =
            find_descendant(&document.ast, "import_declaration").expect("import declaration");
        assert_eq!(import.code, "import module java.base;");
        assert!(import
            .children
            .iter()
            .any(|child| child.kind == "scoped_identifier" && child.code == "java.base"));
    }

    fn find_descendant<'a>(node: &'a JavaAstNode, kind: &str) -> Option<&'a JavaAstNode> {
        node.children.iter().find_map(|child| {
            if child.kind == kind {
                Some(child)
            } else {
                find_descendant(child, kind)
            }
        })
    }
}
