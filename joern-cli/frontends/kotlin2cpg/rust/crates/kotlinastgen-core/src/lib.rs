use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser, Point, Tree};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KotlinAstDocument {
    pub full_name: String,
    pub relative_name: String,
    pub ast: KotlinAstNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KotlinAstNode {
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
    pub children: Vec<KotlinAstNode>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourcePoint {
    pub line: usize,
    pub column: usize,
}

pub fn parse_file(root: &Path, path: &Path) -> Result<KotlinAstDocument> {
    let content =
        fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_source(root, path, &content)
}

pub fn parse_source(root: &Path, path: &Path, source: &str) -> Result<KotlinAstDocument> {
    let parser_source = source_for_tree_sitter(source);
    let tree = parse_tree(&parser_source)?;
    if tree.root_node().has_error() {
        bail!("parser reported syntax errors in {}", path.display());
    }

    Ok(KotlinAstDocument {
        full_name: full_name(path),
        relative_name: relative_name(root, path),
        ast: node_json(tree.root_node(), None, source),
    })
}

pub fn write_json(path: &Path, document: &KotlinAstDocument) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(document)?;
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

pub fn collect_kind_counts(document: &KotlinAstDocument) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    collect_kind_counts_from_node(&document.ast, &mut counts);
    counts
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

fn collect_kind_counts_from_node(node: &KotlinAstNode, counts: &mut BTreeMap<String, usize>) {
    *counts.entry(node.kind.clone()).or_insert(0) += 1;
    for child in &node.children {
        collect_kind_counts_from_node(child, counts);
    }
}

fn parse_tree(source: &str) -> Result<Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_kotlin::language())
        .context("initializing Kotlin parser")?;
    parser
        .parse(source, None)
        .context("parser returned no tree")
}

fn node_json(node: Node<'_>, field_name: Option<String>, source: &str) -> KotlinAstNode {
    let mut children = Vec::new();
    for index in 0..node.child_count() {
        if let Some(child) = node.child(index) {
            let child_field_name = node.field_name_for_child(index as u32).map(str::to_string);
            children.push(node_json(child, child_field_name, source));
        }
    }

    let code = source_excerpt(source, node.start_byte(), node.end_byte()).to_string();
    KotlinAstNode {
        kind: normalized_node_kind(node.kind(), &code).to_string(),
        field_name,
        named: node.is_named(),
        missing: node.is_missing(),
        extra: node.is_extra(),
        has_error: node.has_error(),
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start: point_json(node.start_position()),
        end: point_json(node.end_position()),
        code,
        children,
    }
}

fn source_for_tree_sitter(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut normalized = bytes.to_vec();
    let mut index = 0;
    while index + 2 < bytes.len() {
        if bytes[index] == b'0'
            && matches!(bytes[index + 1], b'b' | b'B')
            && is_binary_digit(bytes[index + 2])
            && !index
                .checked_sub(1)
                .and_then(|previous| bytes.get(previous))
                .is_some_and(|byte| is_identifier_part(*byte))
        {
            normalized[index + 1] = if bytes[index + 1] == b'B' { b'X' } else { b'x' };
        }
        index += 1;
    }
    String::from_utf8(normalized).unwrap_or_else(|_| source.to_string())
}

fn normalized_node_kind<'a>(kind: &'a str, code: &str) -> &'a str {
    if kind == "hex_literal" && is_binary_literal_code(code) {
        "bin_literal"
    } else {
        kind
    }
}

fn is_binary_literal_code(code: &str) -> bool {
    code.as_bytes()
        .get(0..2)
        .is_some_and(|prefix| matches!(prefix, b"0b" | b"0B"))
}

fn is_binary_digit(byte: u8) -> bool {
    matches!(byte, b'0' | b'1')
}

fn is_identifier_part(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_class_with_spans_and_code() {
        let source = "package demo\nclass Foo {\n  fun value(): Int = 1\n}\n";
        let document = parse_source(Path::new("."), Path::new("Foo.kt"), source).unwrap();
        assert_eq!(document.relative_name, "Foo.kt");
        assert_eq!(document.ast.kind, "source_file");
        assert_eq!(document.ast.start.line, 1);
        assert_eq!(document.ast.start.column, 1);
        assert_eq!(document.ast.end.line, 5);
        assert_eq!(document.ast.code, source);

        let counts = collect_kind_counts(&document);
        assert!(counts.contains_key("package_header"));
        assert!(counts.contains_key("class_declaration"));
        assert!(counts.contains_key("function_declaration"));
    }

    #[test]
    fn rejects_syntax_errors() {
        let err = parse_source(Path::new("."), Path::new("Broken.kt"), "class {").unwrap_err();
        assert!(err.to_string().contains("syntax errors"));
    }

    #[test]
    fn parses_binary_literals_preserving_original_code() {
        let source =
            "package demo\nfun literals() {\n  val bits = 0b010101\n  val longBits = 0B101L\n}\n";
        let document = parse_source(Path::new("."), Path::new("Literals.kt"), source).unwrap();

        let mut binary_literal_codes = Vec::new();
        collect_codes_for_kind(&document.ast, "bin_literal", &mut binary_literal_codes);
        assert_eq!(binary_literal_codes, vec!["0b010101", "0B101"]);

        let mut long_literal_codes = Vec::new();
        collect_codes_for_kind(&document.ast, "long_literal", &mut long_literal_codes);
        assert_eq!(long_literal_codes, vec!["0B101L"]);
    }

    fn collect_codes_for_kind<'a>(node: &'a KotlinAstNode, kind: &str, codes: &mut Vec<&'a str>) {
        if node.kind == kind {
            codes.push(&node.code);
        }
        for child in &node.children {
            collect_codes_for_kind(child, kind, codes);
        }
    }
}
