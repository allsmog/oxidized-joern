mod hir;

use anyhow::{Context, Result};
use hir::HirResolver;
use ra_ap_syntax::{
    AstNode, Edition, NodeOrToken, SourceFile, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken,
    TextRange,
};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

pub fn parse_file(input_root: &Path, file: &Path) -> Result<Value> {
    parse_file_with_sysroot(input_root, file, false)
}

pub fn parse_file_with_sysroot(input_root: &Path, file: &Path, sysroot: bool) -> Result<Value> {
    let content =
        fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))?;
    parse_source_with_sysroot(input_root, file, &content, sysroot)
}

pub fn parse_source(input_root: &Path, file: &Path, content: &str) -> Result<Value> {
    parse_source_with_sysroot(input_root, file, content, false)
}

pub fn parse_source_with_sysroot(
    input_root: &Path,
    file: &Path,
    content: &str,
    sysroot: bool,
) -> Result<Value> {
    let parse = SourceFile::parse(content, Edition::CURRENT);
    let tree = parse.tree();
    let root = tree.syntax();
    let line_index = LineIndex::new(content);
    let relative_file_path = relative_file_path(input_root, file);
    let full_file_path = full_file_path(file);
    let crate_name = crate_name_for(input_root, file);
    let module_path = module_path_for(Path::new(&relative_file_path));
    let semantic = SemanticModel::new(root, crate_name.as_deref(), sysroot);
    // Real HIR-backed resolution is gated behind the sysroot flag (the same flag
    // that enables std resolution in the heuristic). It only *fills* fields the
    // heuristic left empty, so existing output is preserved; if it cannot load
    // (no on-disk file, no sysroot, internal panic) it is simply absent.
    let hir = sysroot
        .then(|| HirResolver::try_build(file, crate_name.as_deref()))
        .flatten();

    let mut doc = Map::new();
    doc.insert("relativeFilePath".into(), Value::String(relative_file_path));
    doc.insert("fullFilePath".into(), Value::String(full_file_path));
    doc.insert("content".into(), Value::String(content.into()));
    if let Some(crate_name) = crate_name {
        doc.insert("crateName".into(), Value::String(crate_name));
    }
    if let Some(module_path) = module_path {
        doc.insert("modulePath".into(), Value::String(module_path));
    }
    doc.insert("loc".into(), json!(line_count(content)));
    doc.insert(
        "children".into(),
        Value::Array(vec![node_to_json(
            root,
            &line_index,
            &semantic,
            hir.as_ref(),
        )]),
    );
    Ok(Value::Object(doc))
}

pub fn write_json(target: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let data = serde_json::to_vec_pretty(value)?;
    fs::write(target, data).with_context(|| format!("failed to write {}", target.display()))
}

fn node_to_json(
    node: &SyntaxNode,
    line_index: &LineIndex,
    semantic: &SemanticModel,
    hir: Option<&HirResolver>,
) -> Value {
    let mut obj = base_object(kind_name(node.kind()), node.text_range(), line_index);
    let children: Vec<_> = node
        .children_with_tokens()
        .filter_map(|child| element_to_json(child, line_index, semantic, hir))
        .collect();

    if !children.is_empty() {
        obj.insert("children".into(), Value::Array(children));
    }
    enrich_node(node, &mut obj, semantic);
    enrich_node_with_hir(node, &mut obj, hir);
    if node.kind() == SyntaxKind::MACRO_CALL
        && let Some(expansion) = hir.and_then(|hir| hir.macro_expansion(node.text_range()))
    {
        obj.insert("macroExpansion".into(), expansion.clone());
    }
    Value::Object(obj)
}

fn token_to_json(token: &SyntaxToken, line_index: &LineIndex) -> Option<Value> {
    if should_skip_kind(token.kind()) {
        return None;
    }
    let mut obj = base_object(kind_name(token.kind()), token.text_range(), line_index);
    obj.insert("children".into(), Value::Array(Vec::new()));
    Some(Value::Object(obj))
}

fn element_to_json(
    element: SyntaxElement,
    line_index: &LineIndex,
    semantic: &SemanticModel,
    hir: Option<&HirResolver>,
) -> Option<Value> {
    match element {
        NodeOrToken::Node(node) => {
            if should_skip_kind(node.kind()) {
                None
            } else {
                Some(node_to_json(&node, line_index, semantic, hir))
            }
        }
        NodeOrToken::Token(token) => token_to_json(&token, line_index),
    }
}

fn base_object(
    node_kind: String,
    range: ra_ap_syntax::TextRange,
    line_index: &LineIndex,
) -> Map<String, Value> {
    let start = offset_to_usize(range.start());
    let end = offset_to_usize(range.end());
    let (start_line, start_column) = line_index.line_col(start);
    let mut obj = Map::new();
    obj.insert("nodeKind".into(), Value::String(node_kind));
    obj.insert(
        "range".into(),
        json!({
            "startOffset": start,
            "endOffset": end,
            "startLine": start_line,
            "startColumn": start_column
        }),
    );
    obj
}

fn enrich_node(node: &SyntaxNode, obj: &mut Map<String, Value>, semantic: &SemanticModel) {
    match node.kind() {
        SyntaxKind::IDENT_PAT => {
            if let Some(type_full_name) = type_for_ident_pat(node, semantic) {
                obj.insert("typeFullName".into(), Value::String(type_full_name));
            }
        }
        SyntaxKind::SELF_PARAM => {
            if let Some(type_full_name) = self_param_type(node, semantic) {
                obj.insert("typeFullName".into(), Value::String(type_full_name));
            }
        }
        SyntaxKind::NAME_REF => {
            if let Some(type_full_name) = type_for_name_ref(node, semantic) {
                obj.insert("typeFullName".into(), Value::String(type_full_name));
            }
        }
        SyntaxKind::LITERAL => {
            if !is_range_pat_literal(node)
                && !is_array_type_const_arg_literal(node)
                && let Some(type_full_name) = comparison_operand_context_type(node, semantic)
                    .or_else(|| literal_context_type(node, semantic))
                    .or_else(|| literal_type(node))
            {
                obj.insert("typeFullName".into(), Value::String(type_full_name));
            }
        }
        SyntaxKind::BIN_EXPR
        | SyntaxKind::BLOCK_EXPR
        | SyntaxKind::CALL_EXPR
        | SyntaxKind::CAST_EXPR
        | SyntaxKind::CLOSURE_EXPR
        | SyntaxKind::BREAK_EXPR
        | SyntaxKind::CONTINUE_EXPR
        | SyntaxKind::FIELD_EXPR
        | SyntaxKind::FOR_EXPR
        | SyntaxKind::IF_EXPR
        | SyntaxKind::INDEX_EXPR
        | SyntaxKind::LET_EXPR
        | SyntaxKind::MACRO_EXPR
        | SyntaxKind::MATCH_EXPR
        | SyntaxKind::METHOD_CALL_EXPR
        | SyntaxKind::PAREN_EXPR
        | SyntaxKind::PATH_EXPR
        | SyntaxKind::PREFIX_EXPR
        | SyntaxKind::RANGE_EXPR
        | SyntaxKind::REF_EXPR
        | SyntaxKind::RECORD_EXPR
        | SyntaxKind::RETURN_EXPR
        | SyntaxKind::TRY_EXPR => {
            if let Some(type_full_name) = expr_type(node, semantic) {
                obj.insert("typeFullName".into(), Value::String(type_full_name));
            }
            if matches!(
                node.kind(),
                SyntaxKind::CALL_EXPR | SyntaxKind::METHOD_CALL_EXPR
            ) && let Some(method_full_name) = callable_method_full_name(node, semantic)
            {
                obj.insert("methodFullName".into(), Value::String(method_full_name));
            }
        }
        SyntaxKind::TUPLE_EXPR => {
            if let Some(type_full_name) = tuple_type(node, semantic) {
                obj.insert("typeFullName".into(), Value::String(type_full_name));
            }
        }
        SyntaxKind::ARRAY_EXPR => {
            if let Some(type_full_name) = array_type(node, semantic) {
                obj.insert("typeFullName".into(), Value::String(type_full_name));
            }
        }
        _ => {}
    }
}

/// Fill `typeFullName`/`methodFullName` from real HIR resolution, but only where
/// the heuristic ([`enrich_node`]) produced nothing. The heuristic stays the
/// source of truth for the cases it already covers; HIR extends coverage to
/// generics, trait methods, and standard-library APIs the heuristic cannot
/// resolve. Only the node kinds the heuristic annotates are considered, so the
/// set of nodes that may carry these fields is unchanged.
fn enrich_node_with_hir(
    node: &SyntaxNode,
    obj: &mut Map<String, Value>,
    hir: Option<&HirResolver>,
) {
    let Some(hir) = hir else {
        return;
    };
    let range = node.text_range();
    if enriches_type(node.kind())
        && !suppresses_hir_type_fill(node)
        && !obj.contains_key("typeFullName")
        && let Some(type_full_name) = hir.type_full_name(range)
    {
        let type_full_name =
            source_hir_adjusted_type(node, normalize_sysroot_type_text(type_full_name));
        obj.insert("typeFullName".into(), Value::String(type_full_name));
    }
    if matches!(
        node.kind(),
        SyntaxKind::CALL_EXPR | SyntaxKind::METHOD_CALL_EXPR
    ) && !obj.contains_key("methodFullName")
        && let Some(method_full_name) = hir.method_full_name(range)
    {
        let method_full_name = source_hir_adjusted_method_full_name(node, method_full_name);
        obj.insert("methodFullName".into(), Value::String(method_full_name));
    }
}

fn source_hir_adjusted_method_full_name(node: &SyntaxNode, method_full_name: &str) -> String {
    let method_name = direct_child(node, SyntaxKind::NAME_REF).map(|name| name.text().to_string());
    match (node.kind(), method_name.as_deref(), method_full_name) {
        (SyntaxKind::METHOD_CALL_EXPR, Some("lines"), "core::str::lines") => "str::lines".into(),
        (SyntaxKind::METHOD_CALL_EXPR, Some("trim"), "core::str::trim") => "str::trim".into(),
        (SyntaxKind::METHOD_CALL_EXPR, Some("contains"), "core::str::contains") => {
            "str::contains<P>".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("is_empty"), "core::str::is_empty") => {
            "str::is_empty".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("starts_with"), "core::str::starts_with") => {
            "str::starts_with<P>".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("strip_prefix"), "core::str::strip_prefix") => {
            "str::strip_prefix<P>".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("strip_suffix"), "core::str::strip_suffix") => {
            "str::strip_suffix<P>".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("split_once"), "core::str::split_once") => {
            "str::split_once<P>".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("split_whitespace"), "core::str::split_whitespace") => {
            "str::split_whitespace".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("parse"), "core::str::parse") => {
            "str::parse<F>".into()
        }
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("is_alphanumeric"),
            "core::char::methods::is_alphanumeric",
        ) => "char::is_alphanumeric".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("to_ascii_lowercase"),
            "core::char::methods::to_ascii_lowercase",
        ) => "char::to_ascii_lowercase".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("to_ascii_uppercase"),
            "core::char::methods::to_ascii_uppercase",
        ) => "char::to_ascii_uppercase".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("is_ascii_lowercase"),
            "core::char::methods::is_ascii_lowercase",
        ) => "char::is_ascii_lowercase".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("is_ascii_uppercase"),
            "core::char::methods::is_ascii_uppercase",
        ) => "char::is_ascii_uppercase".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("next"),
            "core::iter::adapters::peekable::Peekable::next",
        ) => "<core::iter::adapters::peekable::Peekable<I> as core::iter::traits::iterator::Iterator>::next".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("peek"),
            "core::iter::adapters::peekable::Peekable::peek",
        ) => "core::iter::adapters::peekable::Peekable<I>::peek".into(),
        (SyntaxKind::METHOD_CALL_EXPR, Some("ok"), "core::result::Result::ok") => {
            "core::result::Result<T, E>::ok".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("unwrap"), "core::result::Result::unwrap") => {
            "core::result::Result<T, E>::unwrap".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("unwrap"), "core::option::Option::unwrap") => {
            "core::option::Option<T>::unwrap".into()
        }
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("unwrap_or_else"),
            "core::result::Result::unwrap_or_else",
        ) => "core::result::Result<T, E>::unwrap_or_else<F>".into(),
        (SyntaxKind::METHOD_CALL_EXPR, Some("to_string"), "alloc::string::to_string") => {
            "<T as alloc::string::ToString>::to_string".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("clone"), "alloc::string::String::clone") => {
            "<alloc::string::String as core::clone::Clone>::clone".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("iter"), "core::slice::iter") => "[T]::iter".into(),
        (SyntaxKind::METHOD_CALL_EXPR, Some("len"), "core::slice::len") => "[T]::len".into(),
        (SyntaxKind::METHOD_CALL_EXPR, Some("is_empty"), "core::slice::is_empty") => {
            "[T]::is_empty".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("reverse"), "core::slice::reverse") => {
            "[T]::reverse".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("sort"), "alloc::slice::sort") => {
            "[T]::sort".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("sort_by"), "alloc::slice::sort_by") => {
            "[T]::sort_by".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("sort_by_key"), "alloc::slice::sort_by_key") => {
            "[T]::sort_by_key".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("last_mut"), "core::slice::last_mut") => {
            "[T]::last_mut".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("to_vec"), "alloc::slice::to_vec") => {
            "[T]::to_vec".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("join"), "alloc::slice::join") => {
            "[T]::join<Separator>".into()
        }
        (SyntaxKind::CALL_EXPR, _, "alloc::vec::Vec::new") => {
            "alloc::vec::Vec<T, alloc::alloc::Global>::new".into()
        }
        (SyntaxKind::CALL_EXPR, _, "alloc::vec::Vec::with_capacity") => {
            "alloc::vec::Vec<T, alloc::alloc::Global>::with_capacity".into()
        }
        (SyntaxKind::CALL_EXPR, _, "std::collections::hash::set::HashSet::new") => {
            "std::collections::hash::set::HashSet<T, std::hash::random::RandomState, alloc::alloc::Global>::new".into()
        }
        (SyntaxKind::CALL_EXPR, _, "alloc::collections::vec_deque::VecDeque::new") => {
            "alloc::collections::vec_deque::VecDeque<T, alloc::alloc::Global>::new".into()
        }
        (SyntaxKind::CALL_EXPR, _, "alloc::collections::binary_heap::BinaryHeap::new") => {
            "alloc::collections::binary_heap::BinaryHeap<T, alloc::alloc::Global>::new".into()
        }
        (SyntaxKind::CALL_EXPR, _, "alloc::boxed::Box::new") => {
            "alloc::boxed::Box<T, alloc::alloc::Global>::new".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("push"), "alloc::vec::Vec::push") => {
            "alloc::vec::Vec<T, A>::push".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("pop"), "alloc::vec::Vec::pop") => {
            "alloc::vec::Vec<T, A>::pop".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("len"), "alloc::vec::Vec::len") => {
            "alloc::vec::Vec<T, A>::len".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("is_empty"), "alloc::vec::Vec::is_empty") => {
            "alloc::vec::Vec<T, A>::is_empty".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("into_iter"), "alloc::vec::Vec::into_iter") => {
            "<alloc::vec::Vec<T, A> as core::iter::traits::collect::IntoIterator>::into_iter"
                .into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("pop"), "alloc::collections::binary_heap::BinaryHeap::pop") => {
            "alloc::collections::binary_heap::BinaryHeap<T, A>::pop".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("push"), "alloc::collections::binary_heap::BinaryHeap::push") => {
            "alloc::collections::binary_heap::BinaryHeap<T, A>::push".into()
        }
        (SyntaxKind::METHOD_CALL_EXPR, Some("len"), "alloc::collections::binary_heap::BinaryHeap::len") => {
            "alloc::collections::binary_heap::BinaryHeap<T, A>::len".into()
        }
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("entry"),
            "std::collections::hash::map::HashMap::entry",
        ) => "std::collections::hash::map::HashMap<K, V, S, A>::entry".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("get"),
            "std::collections::hash::map::HashMap::get",
        ) => "std::collections::hash::map::HashMap<K, V, S, A>::get<Q>".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("values"),
            "std::collections::hash::map::HashMap::values",
        ) => "std::collections::hash::map::HashMap<K, V, S, A>::values".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("insert"),
            "std::collections::hash::map::HashMap::insert",
        ) => "std::collections::hash::map::HashMap<K, V, S, A>::insert".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("insert"),
            "std::collections::hash::set::HashSet::insert",
        ) => "std::collections::hash::set::HashSet<T, S, A>::insert".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("iter"),
            "alloc::collections::vec_deque::VecDeque::iter",
        ) => "alloc::collections::vec_deque::VecDeque<T, A>::iter".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("len"),
            "alloc::collections::vec_deque::VecDeque::len",
        ) => "alloc::collections::vec_deque::VecDeque<T, A>::len".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("push_back"),
            "alloc::collections::vec_deque::VecDeque::push_back",
        ) => "alloc::collections::vec_deque::VecDeque<T, A>::push_back".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("push_front"),
            "alloc::collections::vec_deque::VecDeque::push_front",
        ) => "alloc::collections::vec_deque::VecDeque<T, A>::push_front".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("pop_front"),
            "alloc::collections::vec_deque::VecDeque::pop_front",
        ) => "alloc::collections::vec_deque::VecDeque<T, A>::pop_front".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("front"),
            "alloc::collections::vec_deque::VecDeque::front",
        ) => "alloc::collections::vec_deque::VecDeque<T, A>::front".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("back"),
            "alloc::collections::vec_deque::VecDeque::back",
        ) => "alloc::collections::vec_deque::VecDeque<T, A>::back".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("pop_back"),
            "alloc::collections::vec_deque::VecDeque::pop_back",
        ) => "alloc::collections::vec_deque::VecDeque<T, A>::pop_back".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("remove"),
            "alloc::collections::vec_deque::VecDeque::remove",
        ) => "alloc::collections::vec_deque::VecDeque<T, A>::remove".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("or_default"),
            "std::collections::hash::map::Entry::or_default",
        ) => {
            "std::collections::hash::map::Entry<'a, K, V, alloc::alloc::Global>::or_default".into()
        }
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("or_insert"),
            "std::collections::hash::map::Entry::or_insert",
        ) => "std::collections::hash::map::Entry<'a, K, V, A>::or_insert".into(),
        (SyntaxKind::METHOD_CALL_EXPR, Some("extend"), "alloc::vec::Vec::extend") => {
            "<alloc::vec::Vec<T, A> as core::iter::traits::collect::Extend<T>>::extend<I>".into()
        }
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("extend_from_slice"),
            "alloc::vec::Vec::extend_from_slice",
        ) => "alloc::vec::Vec<T, A>::extend_from_slice".into(),
        (SyntaxKind::METHOD_CALL_EXPR, Some("insert"), "alloc::vec::Vec::insert") => {
            "alloc::vec::Vec<T, A>::insert".into()
        }
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("into_iter"),
            "std::collections::hash::map::HashMap::into_iter",
        ) => "<std::collections::hash::map::HashMap<K, V, S, A> as core::iter::traits::collect::IntoIterator>::into_iter".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("and_then"),
            "core::option::Option<T>::and_then" | "core::option::Option::and_then",
        ) => "core::option::Option<T>::and_then<U, F>".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("map"),
            "core::option::Option::map",
        ) => "core::option::Option<T>::map<U, F>".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("position"),
            "core::iter::traits::iterator::Iterator::position",
        ) => "core::iter::traits::iterator::Iterator::position<P>".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("filter"),
            "core::iter::traits::iterator::Iterator::filter",
        ) => "core::iter::traits::iterator::Iterator::filter<P>".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("zip"),
            "core::iter::traits::iterator::Iterator::zip",
        ) => "core::iter::traits::iterator::Iterator::zip<U>".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("eq"),
            "core::iter::traits::iterator::Iterator::eq",
        ) => "core::iter::traits::iterator::Iterator::eq<I>".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("take_while"),
            "core::iter::traits::iterator::Iterator::take_while",
        ) => "core::iter::traits::iterator::Iterator::take_while<P>".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("map"),
            "core::iter::traits::iterator::Iterator::map",
        ) => "core::iter::traits::iterator::Iterator::map<B, F>".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("fold"),
            "core::iter::traits::iterator::Iterator::fold",
        ) => "core::iter::traits::iterator::Iterator::fold<B, F>".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("max_by_key"),
            "core::iter::traits::iterator::Iterator::max_by_key",
        ) => "core::iter::traits::iterator::Iterator::max_by_key<B, F>".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("collect"),
            "core::iter::traits::iterator::Iterator::collect",
        ) => "core::iter::traits::iterator::Iterator::collect<B>".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("cloned"),
            "core::iter::traits::iterator::Iterator::cloned",
        ) => "core::iter::traits::iterator::Iterator::cloned<T>".into(),
        (
            SyntaxKind::METHOD_CALL_EXPR,
            Some("sum"),
            "core::iter::traits::iterator::Iterator::sum",
        ) => "core::iter::traits::iterator::Iterator::sum<S>".into(),
        _ => method_full_name.into(),
    }
}

fn suppresses_hir_type_fill(node: &SyntaxNode) -> bool {
    (node.kind() == SyntaxKind::IDENT_PAT
        && ident_name(node).is_some_and(|name| {
            name.chars()
                .next()
                .is_some_and(|first| first.is_uppercase())
        }))
        || is_range_pat_literal(node)
        || (node.kind() == SyntaxKind::NAME_REF
            && (is_type_bound_name_ref(node)
                || is_type_anchor_trait_name_ref(node)
                || is_non_final_call_path_name_ref(node)))
}

fn is_range_pat_literal(node: &SyntaxNode) -> bool {
    node.kind() == SyntaxKind::LITERAL
        && node
            .ancestors()
            .any(|ancestor| ancestor.kind() == SyntaxKind::RANGE_PAT)
}

fn is_non_final_call_path_name_ref(node: &SyntaxNode) -> bool {
    let Some(call) = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::CALL_EXPR)
    else {
        return false;
    };
    let Some(callee) = call.children().find(|child| is_expr_kind(child.kind())) else {
        return false;
    };
    if !range_contains(callee.text_range(), node.text_range()) {
        return false;
    }
    let name_refs = callee
        .descendants()
        .filter(|child| child.kind() == SyntaxKind::NAME_REF)
        .collect::<Vec<_>>();
    match name_refs.as_slice() {
        [qualifier, _] => qualifier.text_range() == node.text_range(),
        [.., trait_name, _] => trait_name.text_range() == node.text_range(),
        _ => false,
    }
}

fn source_hir_adjusted_type(node: &SyntaxNode, typ: String) -> String {
    let typ = if typ.contains("impl Fn") {
        let typ = typ
            .replace("alloc::string::String", "String")
            .replace("core::option::Option", "Option")
            .replace(
                "alloc::vec::Vec<String, alloc::alloc::Global>",
                "Vec<String>",
            );
        if node.kind() == SyntaxKind::CLOSURE_EXPR {
            typ
        } else {
            typ.replace(
                "core::slice::iter::Iter<'a, String>",
                "core::slice::iter::Iter<'a, alloc::string::String>",
            )
            .replace(
                "core::slice::iter::Iter<'a, Vec<String>>",
                "core::slice::iter::Iter<'a, alloc::vec::Vec<alloc::string::String, alloc::alloc::Global>>",
            )
        }
    } else {
        typ
    };
    if is_assignment_lhs(node) && !is_index_operand(node) {
        return borrow_mut_type(&typ);
    }
    if node.kind() == SyntaxKind::REF_EXPR
        && typ == "&alloc::string::String"
        && is_push_str_argument_ref_expr(node)
    {
        return "&str".into();
    }
    if typ == "&char" && is_hir_char_comparison_operand(node) {
        return "&&char".into();
    }
    if let Some(adjusted) = source_hir_generic_double_deref_peer_type(node, &typ) {
        return adjusted;
    }
    if let Some(adjusted) = source_hir_generic_comparison_deref_type(node, &typ) {
        return adjusted;
    }
    if let Some(adjusted) = source_hir_receiver_expr_adjusted_type(node, &typ) {
        return adjusted;
    }
    let Some(path_expr) = receiver_path_expr_for_node(node) else {
        return typ;
    };
    let Some(call) = path_expr
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::METHOD_CALL_EXPR)
    else {
        return typ;
    };
    if first_expr_child(&call).is_none_or(|first| first.text_range() != path_expr.text_range()) {
        return typ;
    }
    match direct_child(&call, SyntaxKind::NAME_REF)
        .map(|name| name.text().to_string())
        .as_deref()
    {
        Some("iter") if vec_receiver_element_type(&typ).is_some() => {
            format!("&[{}]", vec_receiver_element_type(&typ).unwrap())
        }
        Some("join") if vec_receiver_element_type(&typ).is_some() => {
            format!("&[{}]", vec_receiver_element_type(&typ).unwrap())
        }
        Some("contains") if is_string_like(&typ) => "&str".into(),
        Some("is_empty") if is_string_like(&typ) => "&str".into(),
        _ => typ,
    }
}

fn source_hir_generic_comparison_deref_type(node: &SyntaxNode, typ: &str) -> Option<String> {
    if node.kind() != SyntaxKind::PREFIX_EXPR
        || !has_direct_token(node, SyntaxKind::STAR)
        || !is_simple_generic_type_param(typ)
    {
        return None;
    }
    let (_, comparison_operand) = enclosing_comparison_operand(node)?;
    if comparison_operand.text_range() != node.text_range() {
        return None;
    }
    let inner = first_expr_child(node)?;
    (inner.kind() == SyntaxKind::PREFIX_EXPR && has_direct_token(&inner, SyntaxKind::STAR))
        .then(|| format!("&{typ}"))
}

fn source_hir_generic_double_deref_peer_type(node: &SyntaxNode, typ: &str) -> Option<String> {
    let base = receiver_base_type(typ);
    if !is_simple_generic_type_param(base) {
        return None;
    }
    let (bin_expr, operand) = enclosing_comparison_operand(node)?;
    if operand.kind() != SyntaxKind::PATH_EXPR
        || !range_contains(operand.text_range(), node.text_range())
    {
        return None;
    }
    bin_expr
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .filter(|child| child.text_range() != operand.text_range())
        .any(|child| is_double_deref_prefix_expr(&child))
        .then(|| format!("&{base}"))
}

fn source_hir_receiver_expr_adjusted_type(node: &SyntaxNode, typ: &str) -> Option<String> {
    let receiver = receiver_expr_for_node(node)?;
    let call = receiver
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::METHOD_CALL_EXPR)?;
    if first_expr_child(&call).is_none_or(|first| first.text_range() != receiver.text_range()) {
        return None;
    }
    match direct_child(&call, SyntaxKind::NAME_REF)
        .map(|name| name.text().to_string())
        .as_deref()
    {
        Some("join") => vec_receiver_element_type(typ).map(|element| format!("&[{element}]")),
        Some(method) if is_vec_slice_shared_method(method) => vec_slice_type(typ),
        Some(method) if is_vec_slice_mutating_method(method) => vec_mut_slice_type(typ),
        Some("is_alphanumeric") if typ == "&char" => Some("char".into()),
        Some(
            "to_ascii_lowercase" | "to_ascii_uppercase" | "is_ascii_lowercase"
            | "is_ascii_uppercase",
        ) if typ == "char" => Some("&char".into()),
        Some(method) if is_str_receiver_method(method) && is_string_like(typ) => {
            Some("&str".into())
        }
        Some("into_iter") => None,
        Some(_) if box_inner_type(typ).is_some() => {
            box_inner_type(typ).map(|inner| boxed_receiver_inner_type(typ, &inner))
        }
        Some(method) if is_mutating_method(method) && is_mutating_collection_receiver(typ) => {
            Some(borrow_mut_type(typ))
        }
        _ => boxed_dyn_trait_name(typ).map(|trait_name| format!("&dyn {trait_name}")),
    }
}

fn receiver_expr_for_node(node: &SyntaxNode) -> Option<SyntaxNode> {
    receiver_path_expr_for_node(node).or_else(|| is_expr_kind(node.kind()).then(|| node.clone()))
}

fn is_push_str_argument_ref_expr(node: &SyntaxNode) -> bool {
    let Some(arg_list) = node
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::ARG_LIST)
    else {
        return false;
    };
    let Some(call) = arg_list
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::METHOD_CALL_EXPR)
    else {
        return false;
    };
    direct_child(&call, SyntaxKind::NAME_REF).is_some_and(|method| method.text() == "push_str")
}

fn receiver_path_expr_for_node(node: &SyntaxNode) -> Option<SyntaxNode> {
    match node.kind() {
        SyntaxKind::PATH_EXPR => Some(node.clone()),
        SyntaxKind::NAME_REF => node
            .ancestors()
            .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_EXPR),
        _ => None,
    }
}

/// Node kinds for which the heuristic emits a `typeFullName`; HIR only fills
/// these to keep the emitted-field shape identical to the heuristic-only output.
fn enriches_type(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IDENT_PAT
            | SyntaxKind::NAME_REF
            | SyntaxKind::LITERAL
            | SyntaxKind::BIN_EXPR
            | SyntaxKind::BLOCK_EXPR
            | SyntaxKind::CALL_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::CLOSURE_EXPR
            | SyntaxKind::BREAK_EXPR
            | SyntaxKind::CONTINUE_EXPR
            | SyntaxKind::FIELD_EXPR
            | SyntaxKind::FOR_EXPR
            | SyntaxKind::IF_EXPR
            | SyntaxKind::INDEX_EXPR
            | SyntaxKind::LET_EXPR
            | SyntaxKind::MACRO_EXPR
            | SyntaxKind::MATCH_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::PATH_EXPR
            | SyntaxKind::PREFIX_EXPR
            | SyntaxKind::REF_EXPR
            | SyntaxKind::RECORD_EXPR
            | SyntaxKind::RETURN_EXPR
            | SyntaxKind::WHILE_EXPR
            | SyntaxKind::LOOP_EXPR
            | SyntaxKind::SELF_PARAM
            | SyntaxKind::TUPLE_EXPR
            | SyntaxKind::ARRAY_EXPR
    )
}

fn type_for_ident_pat(ident_pat: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if let Some(for_expr) = ident_pat
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::FOR_EXPR)
        && let Some(typ) = for_pattern_ident_type(ident_pat, &for_expr, semantic)
    {
        return Some(typ);
    }
    if let Some(let_expr) = ident_pat
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::LET_EXPR)
        && let Some(typ) = let_expr_pattern_ident_type(ident_pat, &let_expr, semantic)
    {
        return Some(typ);
    }
    if let Some(match_arm) = ident_pat
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::MATCH_ARM)
        && let Some(typ) = match_arm_pattern_ident_type(ident_pat, &match_arm, semantic)
    {
        return Some(typ);
    }
    let parent = ident_pat.parent()?;
    match parent.kind() {
        SyntaxKind::LET_STMT => {
            let declared =
                type_node(&parent, semantic).map(|typ| let_declared_type(&parent, typ, semantic));
            let initializer = initializer_expr(&parent).and_then(|expr| expr_type(&expr, semantic));
            let later = || later_assignment_type(ident_pat, &parent, semantic);
            declared.or_else(|| match initializer {
                Some(typ) if typ == "i32" => later().or(Some(typ)),
                Some(typ) => Some(typ),
                None => later(),
            })
        }
        SyntaxKind::PARAM => type_node(&parent, semantic)
            .map(|typ| declared_type_defaults(typ, semantic))
            .or_else(|| initializer_expr(&parent).and_then(|expr| expr_type(&expr, semantic)))
            .or_else(|| later_assignment_type(ident_pat, &parent, semantic)),
        _ => None,
    }
}

fn for_pattern_ident_type(
    ident_pat: &SyntaxNode,
    for_expr: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    let pattern = pattern_child_containing(for_expr, ident_pat)?;
    let iterable = for_expr
        .children()
        .find(|child| is_expr_kind(child.kind()))?;
    let item_type = iterable_item_type(&iterable, semantic)?;
    let item_type = if item_type == "i32" {
        for_index_usage_type(ident_pat, for_expr).unwrap_or(item_type)
    } else {
        item_type
    };
    pattern_ident_type_from_value(ident_pat, &pattern, &item_type)
}

fn for_index_usage_type(ident_pat: &SyntaxNode, for_expr: &SyntaxNode) -> Option<String> {
    let name = ident_name(ident_pat)?;
    let pattern_end = ident_pat.text_range().end();
    let used_as_index = for_expr
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::PATH_EXPR)
        .filter(|node| node.text_range().start() >= pattern_end)
        .filter(|node| path_expr_name(node).as_deref() == Some(name.as_str()))
        .any(|node| is_index_operand(&node));
    used_as_index.then(|| "usize".into())
}

fn let_expr_pattern_ident_type(
    ident_pat: &SyntaxNode,
    let_expr: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    let pattern = pattern_child_containing(let_expr, ident_pat)?;
    let value = let_expr
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .last()?;
    let value_type = expr_type(&value, semantic)?;
    if let Some(typ) =
        enum_variant_pattern_ident_type_from_value(ident_pat, &pattern, &value_type, semantic)
    {
        return Some(typ);
    }
    pattern_ident_type_from_value(ident_pat, &pattern, &value_type)
}

fn match_arm_pattern_ident_type(
    ident_pat: &SyntaxNode,
    match_arm: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    let pattern = pattern_child_containing(match_arm, ident_pat)?;
    if pattern.text_range() == ident_pat.text_range()
        && pattern.kind() == SyntaxKind::IDENT_PAT
        && ident_name(ident_pat).is_some_and(|name| {
            name.chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_uppercase())
        })
    {
        return None;
    }
    let match_expr = match_arm
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::MATCH_EXPR)?;
    let value = match_expr
        .children()
        .find(|child| is_expr_kind(child.kind()))?;
    let value_type = expr_type(&value, semantic)?;
    if let Some(typ) =
        enum_variant_pattern_ident_type_from_value(ident_pat, &pattern, &value_type, semantic)
    {
        return Some(typ);
    }
    pattern_ident_type_from_value(ident_pat, &pattern, &value_type)
}

fn enum_variant_pattern_ident_type_from_value(
    ident_pat: &SyntaxNode,
    pattern: &SyntaxNode,
    value_type: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    if pattern.kind() != SyntaxKind::TUPLE_STRUCT_PAT {
        return None;
    }
    let path = direct_child(pattern, SyntaxKind::PATH)?;
    let names = path
        .descendants()
        .filter(|child| child.kind() == SyntaxKind::NAME_REF)
        .map(|name| name.text().to_string())
        .collect::<Vec<_>>();
    let [type_name, variant] = names.as_slice() else {
        return None;
    };
    let qualified = semantic.qualify_type_name(type_name);
    let info = semantic
        .structs
        .get(&qualified)
        .or_else(|| semantic.structs.get(type_name))?;
    let fields = info.variant_tuple_fields.get(variant)?;
    let (child, field_type) = enum_variant_pattern_field_type(ident_pat, pattern, fields)?;
    let field_type = if value_type.trim_start().starts_with("&mut ") {
        format!("&mut {field_type}")
    } else if value_type.trim_start().starts_with('&') {
        format!("&{field_type}")
    } else {
        field_type
    };
    pattern_ident_type_from_value(ident_pat, &child, &field_type)
}

fn enum_variant_pattern_field_type(
    ident_pat: &SyntaxNode,
    pattern: &SyntaxNode,
    fields: &[String],
) -> Option<(SyntaxNode, String)> {
    let patterns = pattern
        .children()
        .filter(|child| is_pattern_kind(child.kind()))
        .collect::<Vec<_>>();
    let (idx, child) = patterns
        .iter()
        .enumerate()
        .find(|(_, child)| range_contains(child.text_range(), ident_pat.text_range()))?;
    Some((child.clone(), fields.get(idx)?.clone()))
}

fn pattern_child_containing(parent: &SyntaxNode, ident_pat: &SyntaxNode) -> Option<SyntaxNode> {
    parent.children().find(|child| {
        is_pattern_kind(child.kind()) && range_contains(child.text_range(), ident_pat.text_range())
    })
}

fn pattern_ident_type_from_value(
    ident_pat: &SyntaxNode,
    pattern: &SyntaxNode,
    value_type: &str,
) -> Option<String> {
    if pattern.text_range() == ident_pat.text_range() && pattern.kind() == SyntaxKind::IDENT_PAT {
        return Some(value_type.into());
    }
    match pattern.kind() {
        SyntaxKind::REF_PAT => {
            let inner = pattern.children().find(|child| {
                is_pattern_kind(child.kind())
                    && range_contains(child.text_range(), ident_pat.text_range())
            })?;
            let inner_type = unborrow_type(value_type);
            pattern_ident_type_from_value(ident_pat, &inner, &inner_type)
        }
        SyntaxKind::TUPLE_PAT => {
            let patterns = pattern
                .children()
                .filter(|child| is_pattern_kind(child.kind()))
                .collect::<Vec<_>>();
            let (idx, child) = patterns
                .iter()
                .enumerate()
                .find(|(_, child)| range_contains(child.text_range(), ident_pat.text_range()))?;
            let field_type = tuple_field_type(value_type, &idx.to_string())?;
            pattern_ident_type_from_value(ident_pat, child, &field_type)
        }
        SyntaxKind::TUPLE_STRUCT_PAT => {
            let path = direct_child(pattern, SyntaxKind::PATH)?;
            let name = path
                .descendants()
                .find(|child| child.kind() == SyntaxKind::NAME_REF)?
                .text()
                .to_string();
            let inner_type = if name == "Some" {
                option_pattern_inner_type(value_type)?
            } else {
                value_type.into()
            };
            let child = pattern.children().find(|child| {
                is_pattern_kind(child.kind())
                    && range_contains(child.text_range(), ident_pat.text_range())
            })?;
            pattern_ident_type_from_value(ident_pat, &child, &inner_type)
        }
        _ => pattern
            .children()
            .find(|child| {
                is_pattern_kind(child.kind())
                    && range_contains(child.text_range(), ident_pat.text_range())
            })
            .and_then(|child| pattern_ident_type_from_value(ident_pat, &child, value_type)),
    }
}

fn iterable_item_type(iterable: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if iterable.kind() == SyntaxKind::METHOD_CALL_EXPR {
        let method_name = direct_child(iterable, SyntaxKind::NAME_REF)?
            .text()
            .to_string();
        let receiver = first_expr_child(iterable)?;
        if method_name == "enumerate" {
            let inner = iterable_item_type(&receiver, semantic)?;
            return Some(format!("(usize, {inner})"));
        }
        if method_name == "iter" {
            let receiver_type = expr_type(&receiver, semantic)?;
            let element = slice_or_array_element_type(&receiver_type)?;
            return Some(format!("&{element}"));
        }
        if method_name == "chars" {
            return Some("char".into());
        }
    }
    let iterable_type = expr_type(iterable, semantic)?;
    if let Some(item) = range_item_type(&iterable_type) {
        return Some(item);
    }
    let element = slice_or_array_element_type(&iterable_type)?;
    Some(format!("&{element}"))
}

fn range_item_type(typ: &str) -> Option<String> {
    let base = receiver_base_type(typ);
    base.strip_prefix("core::ops::range::RangeInclusive<")
        .or_else(|| base.strip_prefix("core::ops::range::Range<"))?
        .strip_suffix('>')
        .map(str::to_string)
}

fn slice_or_array_element_type(typ: &str) -> Option<String> {
    let base = receiver_base_type(typ);
    array_element_type(base)
}

fn option_inner_type(typ: &str) -> Option<String> {
    let base = receiver_base_type(typ);
    base.strip_prefix("core::option::Option<")
        .or_else(|| base.strip_prefix("Option<"))?
        .strip_suffix('>')
        .and_then(first_top_level_arg)
}

fn option_pattern_inner_type(typ: &str) -> Option<String> {
    let inner = option_inner_type(typ)?;
    let trimmed = typ.trim_start();
    if trimmed.starts_with("&mut ") {
        Some(format!("&mut {inner}"))
    } else if trimmed.starts_with('&') {
        Some(format!("&{inner}"))
    } else {
        Some(inner)
    }
}

fn is_pattern_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IDENT_PAT
            | SyntaxKind::REF_PAT
            | SyntaxKind::TUPLE_PAT
            | SyntaxKind::TUPLE_STRUCT_PAT
            | SyntaxKind::RECORD_PAT
            | SyntaxKind::SLICE_PAT
            | SyntaxKind::WILDCARD_PAT
            | SyntaxKind::LITERAL_PAT
            | SyntaxKind::OR_PAT
            | SyntaxKind::PATH_PAT
    )
}

fn let_declared_type(let_stmt: &SyntaxNode, typ: String, semantic: &SemanticModel) -> String {
    let typ = declared_type_defaults(typ, semantic);
    if initializer_expr(let_stmt).is_some_and(|expr| is_hashmap_new_call(&expr)) {
        expand_root_hashmap_defaults(&typ)
    } else {
        typ
    }
}

fn declared_type_defaults(typ: String, semantic: &SemanticModel) -> String {
    if semantic.sysroot {
        expand_root_hashmap_defaults(&expand_root_vecdeque_defaults(
            &expand_root_binary_heap_defaults(&expand_root_vec_defaults(
                &expand_root_box_defaults(&typ),
            )),
        ))
    } else {
        typ
    }
}

fn is_hashmap_new_call(expr: &SyntaxNode) -> bool {
    expr.kind() == SyntaxKind::CALL_EXPR
        && path_matches(call_path_names(expr).as_deref(), &["HashMap", "new"])
}

fn self_param_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let self_type = enclosing_self_type(node, semantic)?;
    if has_direct_token(node, SyntaxKind::AMP) {
        let mutability = if has_direct_token(node, SyntaxKind::MUT_KW) {
            "mut "
        } else {
            ""
        };
        Some(format!("&{mutability}{self_type}"))
    } else {
        Some(self_type)
    }
}

fn enclosing_method_self_param_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let function = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::FN)?;
    direct_child(&function, SyntaxKind::PARAM_LIST)?
        .children()
        .find(|child| child.kind() == SyntaxKind::SELF_PARAM)
        .and_then(|self_param| self_param_type(&self_param, semantic))
}

fn type_for_name_ref(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if node.text().to_string() == "self" {
        if node
            .ancestors()
            .any(|ancestor| ancestor.kind() == SyntaxKind::SELF_PARAM)
        {
            return enclosing_self_type(node, semantic);
        }
        let self_type = enclosing_method_self_param_type(node, semantic)
            .or_else(|| enclosing_self_type(node, semantic));
        if let Some(self_type) = self_type {
            if let Some(path_expr) = node
                .ancestors()
                .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_EXPR)
            {
                return Some(adjusted_expr_type(&path_expr, self_type, semantic));
            }
            return Some(self_type);
        }
    }
    if let Some(path_expr) = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_EXPR)
        && let Some(typ) = assignment_lhs_path_context_type(&path_expr, semantic)
    {
        return Some(typ);
    }
    if let Some(path_expr) = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_EXPR)
        && let Some(typ) = generic_double_deref_peer_context_type(&path_expr, semantic)
    {
        return Some(typ);
    }
    if let Some(typ) = method_receiver_name_ref_adjusted_type(node, semantic) {
        return Some(typ);
    }
    if let Some(signature) = enum_variant_name_ref_signature(node, semantic) {
        return Some(signature);
    }
    if let Some(enum_type) = enum_variant_name_ref_value_type(node, semantic) {
        return Some(enum_type);
    }
    if is_impl_trait_name_ref(node) {
        return None;
    }
    if let Some(assoc_arg_type) = associated_type_arg_value_name_ref_type(node, semantic) {
        return Some(assoc_arg_type);
    }
    if is_type_bound_name_ref(node) || is_type_anchor_trait_name_ref(node) {
        return None;
    }
    if let Some(sysroot_path_type) = sysroot_path_segment_name_ref_type(node, semantic) {
        return Some(sysroot_path_type);
    }
    if is_trait_ufcs_non_final_name_ref(node, semantic) {
        return None;
    }
    if let Some(ufcs_type) = trait_ufcs_name_ref_type(node, semantic) {
        return Some(ufcs_type);
    }
    if let Some(assoc_type) = associated_function_name_ref_type(node, semantic) {
        return Some(assoc_type);
    }
    if let Some(collect_type) = collect_vec_turbofish_type(node, semantic) {
        return Some(collect_type);
    }
    if is_type_name_ref(node) {
        return node
            .ancestors()
            .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_TYPE)
            .and_then(|path_type| type_text(&path_type, semantic))
            .or_else(|| Some(semantic.qualify_type_name(&node.text().to_string())));
    }
    if let Some(record_type) = record_expr_name_ref_type(node, semantic) {
        return Some(record_type);
    }
    if let Some(pattern_type) = pattern_path_name_ref_type(node, semantic) {
        return Some(pattern_type);
    }
    if let Some(use_type) = use_tree_name_ref_type(node, semantic) {
        return Some(use_type);
    }
    if let Some(arg_type) = method_argument_context_type(node, semantic) {
        return Some(arg_type);
    }
    path_expr_name_ref_type(node, semantic)
}

fn is_trait_ufcs_non_final_name_ref(node: &SyntaxNode, semantic: &SemanticModel) -> bool {
    if is_type_anchor_self_type_name_ref(node) {
        return false;
    }
    let Some(call) = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::CALL_EXPR)
    else {
        return false;
    };
    if trait_ufcs_parts_for_call(&call, semantic).is_none() {
        return false;
    }
    let Some(callee) = call.children().find(|child| is_expr_kind(child.kind())) else {
        return false;
    };
    if !range_contains(callee.text_range(), node.text_range()) {
        return false;
    }
    let name_refs = callee
        .descendants()
        .filter(|child| child.kind() == SyntaxKind::NAME_REF)
        .collect::<Vec<_>>();
    match name_refs.as_slice() {
        [qualifier, _] => qualifier.text_range() == node.text_range(),
        [.., trait_name, _] => trait_name.text_range() == node.text_range(),
        _ => false,
    }
}

fn sysroot_path_segment_name_ref_type(
    node: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    if !semantic.sysroot {
        return None;
    }
    if node.ancestors().any(|ancestor| {
        matches!(
            ancestor.kind(),
            SyntaxKind::PATH_PAT | SyntaxKind::TUPLE_STRUCT_PAT
        )
    }) && let Some(qualified) = sysroot_qualified_type_name(&node.text().to_string())
    {
        return Some(qualified.into());
    }
    if node.text() != "Box" {
        return None;
    }
    if node
        .ancestors()
        .any(|ancestor| ancestor.kind() == SyntaxKind::MACRO_CALL)
    {
        return Some("alloc::boxed::Box".into());
    }
    let path_expr = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_EXPR)?;
    let names = path_name_refs(&path_expr)?;
    names
        .windows(2)
        .any(|window| matches!(window, [module, typ] if module == "boxed" && typ == "Box"))
        .then_some("alloc::boxed::Box".into())
}

fn is_type_anchor_self_type_name_ref(node: &SyntaxNode) -> bool {
    let Some(type_anchor) = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::TYPE_ANCHOR)
    else {
        return false;
    };
    if !type_anchor
        .children()
        .any(|child| child.kind() == SyntaxKind::AS_KW)
    {
        return false;
    }
    type_anchor
        .children()
        .filter(|child| child.kind() == SyntaxKind::PATH_TYPE)
        .next()
        .is_some_and(|self_type| range_contains(self_type.text_range(), node.text_range()))
}

fn collect_vec_turbofish_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if !semantic.sysroot || node.text() != "Vec" {
        return None;
    }
    let path_type = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_TYPE)?;
    if path_type.text().to_string() != "Vec<_>" {
        return None;
    }
    let collect_call = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::METHOD_CALL_EXPR)?;
    if direct_child(&collect_call, SyntaxKind::NAME_REF).is_none_or(|name| name.text() != "collect")
    {
        return None;
    }
    if collect_call
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::METHOD_CALL_EXPR)
        .and_then(|parent| direct_child(&parent, SyntaxKind::NAME_REF))
        .is_none_or(|name| name.text() != "join")
    {
        return None;
    }
    Some("alloc::vec::Vec<alloc::string::String>".into())
}

fn is_type_name_ref(node: &SyntaxNode) -> bool {
    node.ancestors()
        .skip(1)
        .any(|ancestor| matches!(ancestor.kind(), SyntaxKind::PATH_TYPE))
}

fn is_type_bound_name_ref(node: &SyntaxNode) -> bool {
    node.ancestors()
        .skip(1)
        .any(|ancestor| ancestor.kind() == SyntaxKind::TYPE_BOUND)
}

fn associated_type_arg_value_name_ref_type(
    node: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    let assoc_arg = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::ASSOC_TYPE_ARG)?;
    let value_type = assoc_arg
        .children()
        .filter(|child| is_type_kind(child.kind()))
        .last()?;
    if !range_contains(value_type.text_range(), node.text_range()) {
        return None;
    }
    type_text(&value_type, semantic)
}

fn is_type_anchor_trait_name_ref(node: &SyntaxNode) -> bool {
    let Some(type_anchor) = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::TYPE_ANCHOR)
    else {
        return false;
    };
    if !type_anchor
        .children()
        .any(|child| child.kind() == SyntaxKind::AS_KW)
    {
        return false;
    }
    type_anchor
        .children()
        .filter(|child| child.kind() == SyntaxKind::PATH_TYPE)
        .last()
        .is_some_and(|trait_type| range_contains(trait_type.text_range(), node.text_range()))
}

fn is_hir_char_comparison_operand(node: &SyntaxNode) -> bool {
    let expr = if node.kind() == SyntaxKind::NAME_REF {
        node.ancestors()
            .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_EXPR)
    } else if node.kind() == SyntaxKind::PATH_EXPR {
        Some(node.clone())
    } else {
        None
    };
    expr.and_then(|expr| enclosing_comparison_operand(&expr))
        .is_some()
}

fn is_impl_trait_name_ref(node: &SyntaxNode) -> bool {
    let Some(path_type) = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_TYPE)
    else {
        return false;
    };
    let Some(impl_node) = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::IMPL)
    else {
        return false;
    };
    if !has_direct_token(&impl_node, SyntaxKind::FOR_KW) {
        return false;
    }
    if path_type.parent().as_ref() != Some(&impl_node) {
        return false;
    }
    let Some(self_type) = impl_node
        .children()
        .filter(|child| is_type_kind(child.kind()))
        .last()
    else {
        return false;
    };
    !range_contains(self_type.text_range(), path_type.text_range())
        && range_contains(path_type.text_range(), node.text_range())
}

fn type_node(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    node.children()
        .find(|child| is_type_kind(child.kind()))
        .and_then(|child| type_text(&child, semantic))
}

fn initializer_expr(node: &SyntaxNode) -> Option<SyntaxNode> {
    let mut seen_eq = false;
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::EQ => seen_eq = true,
            NodeOrToken::Node(child_node) if seen_eq && is_expr_kind(child_node.kind()) => {
                return Some(child_node);
            }
            _ => {}
        }
    }
    None
}

fn later_assignment_type(
    ident_pat: &SyntaxNode,
    let_stmt: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    let name = ident_name(ident_pat)?;
    let stmt_list = let_stmt
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::STMT_LIST)?;
    let let_end = let_stmt.text_range().end();

    stmt_list
        .children()
        .filter(|child| child.text_range().start() >= let_end)
        .find_map(|stmt| {
            assignment_type_for_name(&stmt, &name, semantic)
                .or_else(|| usage_context_type_for_name(&stmt, &name, semantic))
        })
}

fn assignment_type_for_name(
    stmt: &SyntaxNode,
    name: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    let bin_expr = if stmt.kind() == SyntaxKind::BIN_EXPR {
        stmt.clone()
    } else {
        stmt.descendants()
            .find(|child| child.kind() == SyntaxKind::BIN_EXPR)?
    };
    if !bin_expr
        .children_with_tokens()
        .any(|child| matches!(child, NodeOrToken::Token(token) if token.kind() == SyntaxKind::EQ))
    {
        return None;
    }
    let exprs: Vec<_> = bin_expr
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .collect();
    match exprs.as_slice() {
        [lhs, rhs, ..] if path_expr_name(lhs).as_deref() == Some(name) => expr_type(rhs, semantic),
        _ => None,
    }
}

fn usage_context_type_for_name(
    stmt: &SyntaxNode,
    name: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    stmt.descendants()
        .filter(|node| node.kind() == SyntaxKind::PATH_EXPR)
        .filter(|node| path_expr_name(node).as_deref() == Some(name))
        .find_map(|path| {
            if is_index_operand(&path) {
                return Some("usize".into());
            }
            comparison_peer_type_for_usage(&path, semantic)
        })
}

fn comparison_peer_type_for_usage(
    path_expr: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    for ancestor in path_expr.ancestors() {
        if ancestor.kind() != SyntaxKind::BIN_EXPR || !is_comparison_expr(&ancestor) {
            continue;
        }
        let operands = ancestor
            .children()
            .filter(|child| is_expr_kind(child.kind()))
            .collect::<Vec<_>>();
        let current = operands
            .iter()
            .find(|operand| range_contains(operand.text_range(), path_expr.text_range()))?;
        let peer_type = operands
            .iter()
            .filter(|operand| operand.text_range() != current.text_range())
            .find_map(|operand| expr_type(operand, semantic))?;
        let peer_type = unborrow_type(&peer_type);
        if peer_type != "i32" {
            return Some(peer_type);
        }
    }
    None
}

fn ident_name(ident_pat: &SyntaxNode) -> Option<String> {
    ident_pat
        .descendants_with_tokens()
        .filter_map(|element| match element {
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::IDENT => {
                Some(token.text().to_string())
            }
            _ => None,
        })
        .next()
}

fn path_expr_name(path_expr: &SyntaxNode) -> Option<String> {
    if path_expr.kind() != SyntaxKind::PATH_EXPR {
        return None;
    }
    path_expr
        .descendants()
        .find(|child| child.kind() == SyntaxKind::NAME_REF)
        .map(|node| node.text().to_string())
}

fn literal_context_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if let Some(typ) = range_operand_context_type(node, semantic) {
        return Some(typ);
    }
    if let Some(typ) = method_argument_context_type(node, semantic)
        && typ != "ANY"
    {
        return Some(typ);
    }
    if let Some(typ) = assignment_rhs_context_type(node, semantic) {
        return Some(typ);
    }
    if let Some(typ) = record_field_context_type(node, semantic) {
        return Some(typ);
    }
    if is_index_operand(node) {
        return Some("usize".into());
    }
    if is_array_repeat_count(node) {
        return Some("usize".into());
    }
    if let Some(typ) = returned_some_argument_context_type(node, semantic) {
        return Some(typ);
    }
    if let Some(typ) = return_expr_context_type(node, semantic) {
        return Some(typ);
    }
    if let Some(typ) = tuple_initializer_element_context_type(node, semantic) {
        return Some(typ);
    }
    if let Some(typ) = arithmetic_operand_context_type(node, semantic) {
        return Some(typ);
    }
    if is_string_with_capacity_arg(node, semantic) {
        return Some("usize".into());
    }
    for ancestor in node.ancestors().skip(1) {
        if !matches!(ancestor.kind(), SyntaxKind::LET_STMT | SyntaxKind::CONST) {
            continue;
        }
        let Some(initializer) = initializer_expr(&ancestor) else {
            continue;
        };
        if range_contains(initializer.text_range(), node.text_range()) {
            return tuple_initializer_element_context_type(node, semantic)
                .or_else(|| type_node(&ancestor, semantic))
                .or_else(|| {
                    direct_child(&ancestor, SyntaxKind::IDENT_PAT)
                        .and_then(|pat| later_assignment_type(&pat, &ancestor, semantic))
                });
        }
    }
    None
}

fn is_string_with_capacity_arg(node: &SyntaxNode, semantic: &SemanticModel) -> bool {
    if !semantic.sysroot {
        return false;
    }
    let Some(call) = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::CALL_EXPR)
    else {
        return false;
    };
    if !path_matches(
        call_path_names(&call).as_deref(),
        &["String", "with_capacity"],
    ) {
        return false;
    }
    direct_child(&call, SyntaxKind::ARG_LIST).is_some_and(|args| {
        args.children()
            .filter(|child| is_expr_kind(child.kind()))
            .any(|arg| range_contains(arg.text_range(), node.text_range()))
    })
}

fn assignment_rhs_context_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let bin_expr = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::BIN_EXPR)?;
    if !has_assignment_operator(&bin_expr) {
        return None;
    }
    let exprs = bin_expr
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .collect::<Vec<_>>();
    let [lhs, rhs, ..] = exprs.as_slice() else {
        return None;
    };
    if !range_contains(rhs.text_range(), node.text_range()) {
        return None;
    }
    expr_type(lhs, semantic)
        .map(|typ| unborrow_type(&typ))
        .or_else(|| path_expr_name(lhs).and_then(|name| index_usage_type_for_name(lhs, &name)))
}

fn index_usage_type_for_name(node: &SyntaxNode, name: &str) -> Option<String> {
    let stmt_list = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::STMT_LIST)?;
    stmt_list
        .descendants()
        .filter(|descendant| descendant.kind() == SyntaxKind::INDEX_EXPR)
        .any(|index_expr| {
            index_expr
                .children()
                .filter(|child| is_expr_kind(child.kind()))
                .nth(1)
                .and_then(|operand| path_expr_name(&operand))
                .is_some_and(|operand_name| operand_name == name)
        })
        .then(|| "usize".into())
}

fn record_field_context_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let field = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::RECORD_EXPR_FIELD)?;
    let expr = field.children().find(|child| is_expr_kind(child.kind()))?;
    if !range_contains(expr.text_range(), node.text_range()) {
        return None;
    }
    let field_name = direct_child(&field, SyntaxKind::NAME_REF)?
        .text()
        .to_string();
    let record_expr = field
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::RECORD_EXPR)?;
    let record_type = record_expr_type(&record_expr, semantic)?;
    semantic
        .structs
        .get(&record_type)?
        .fields
        .get(&field_name)
        .cloned()
}

fn arithmetic_operand_context_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let (bin_expr, operand) = enclosing_bin_operand(node)?;
    if !has_any_direct_token(
        &bin_expr,
        &[
            SyntaxKind::PLUS,
            SyntaxKind::MINUS,
            SyntaxKind::STAR,
            SyntaxKind::SLASH,
            SyntaxKind::PERCENT,
            SyntaxKind::SHL,
            SyntaxKind::SHR,
        ],
    ) || has_assignment_operator(&bin_expr)
        || is_comparison_expr(&bin_expr)
    {
        return None;
    }
    if !range_contains(operand.text_range(), node.text_range()) {
        return None;
    }
    comparison_peer_raw_type(&bin_expr, &operand, semantic)
        .map(|typ| unborrow_type(&typ))
        .filter(|typ| typ != "i32")
}

fn return_expr_context_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let return_expr = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::RETURN_EXPR)?;
    let value = first_expr_child(&return_expr)?;
    if !range_contains(value.text_range(), node.text_range()) {
        return None;
    }
    if has_return_context_boundary(node, &value) {
        return None;
    }
    enclosing_function_return_type(&return_expr, semantic)
}

fn tail_expr_return_context_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let function = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::FN)?;
    let block = direct_child(&function, SyntaxKind::BLOCK_EXPR)?;
    let stmt_list = direct_child(&block, SyntaxKind::STMT_LIST)?;
    let tail = stmt_list
        .children()
        .filter(|child| child.kind() == SyntaxKind::EXPR_STMT || is_expr_kind(child.kind()))
        .last()?;
    let tail_expr = if tail.kind() == SyntaxKind::EXPR_STMT {
        tail.children().find(|child| is_expr_kind(child.kind()))?
    } else {
        tail
    };
    (tail_expr.text_range() == node.text_range())
        .then(|| enclosing_function_return_type(node, semantic))
        .flatten()
}

fn block_tail_expr_context_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let stmt_list = node
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::STMT_LIST)?;
    let block = stmt_list
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::BLOCK_EXPR)?;
    let tail = stmt_list
        .children()
        .filter(|child| child.kind() == SyntaxKind::EXPR_STMT || is_expr_kind(child.kind()))
        .last()?;
    let tail_expr = if tail.kind() == SyntaxKind::EXPR_STMT {
        tail.children().find(|child| is_expr_kind(child.kind()))?
    } else {
        tail
    };
    if tail_expr.text_range() != node.text_range() {
        return None;
    }
    match_arm_context_type(&block, semantic)
        .or_else(|| tail_expr_return_context_type(&block, semantic))
}

fn has_return_context_boundary(node: &SyntaxNode, value: &SyntaxNode) -> bool {
    if node.text_range() == value.text_range() {
        return false;
    }
    for ancestor in node.ancestors().skip(1) {
        if !range_contains(value.text_range(), ancestor.text_range()) {
            break;
        }
        if matches!(
            ancestor.kind(),
            SyntaxKind::CALL_EXPR
                | SyntaxKind::METHOD_CALL_EXPR
                | SyntaxKind::INDEX_EXPR
                | SyntaxKind::FIELD_EXPR
                | SyntaxKind::MACRO_EXPR
        ) {
            return true;
        }
        if ancestor.text_range() == value.text_range() {
            break;
        }
    }
    false
}

fn range_contains(outer: ra_ap_syntax::TextRange, inner: ra_ap_syntax::TextRange) -> bool {
    outer.start() <= inner.start() && inner.end() <= outer.end()
}

fn comparison_operand_context_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if !matches!(
        node.kind(),
        SyntaxKind::LITERAL
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::PATH_EXPR
            | SyntaxKind::INDEX_EXPR
            | SyntaxKind::BIN_EXPR
            | SyntaxKind::CALL_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
    ) {
        return None;
    }
    let (bin_expr, operand) = enclosing_comparison_operand(node)?;
    if !is_comparison_expr(&bin_expr) {
        return None;
    }
    if operand.text_range() != node.text_range()
        && !matches!(
            operand.kind(),
            SyntaxKind::PAREN_EXPR | SyntaxKind::CAST_EXPR
        )
    {
        return None;
    }
    let current_type = comparison_operand_raw_type(&operand, semantic);
    let peer_type = comparison_peer_raw_type(&bin_expr, &operand, semantic);
    let base_type = if current_type.as_deref() == Some("i32") {
        peer_type.or(current_type)
    } else {
        current_type.or(peer_type)
    }?;
    if base_type == "char" {
        Some("&&char".into())
    } else if base_type == "&char" {
        Some("&&char".into())
    } else {
        Some(borrow_shared_type(&base_type))
    }
}

fn enclosing_comparison_operand(node: &SyntaxNode) -> Option<(SyntaxNode, SyntaxNode)> {
    enclosing_bin_operand(node).filter(|(bin_expr, _)| is_comparison_expr(bin_expr))
}

fn enclosing_bin_operand(node: &SyntaxNode) -> Option<(SyntaxNode, SyntaxNode)> {
    for ancestor in node.ancestors() {
        let Some(parent) = ancestor.parent() else {
            continue;
        };
        if parent.kind() == SyntaxKind::BIN_EXPR && is_expr_kind(ancestor.kind()) {
            return Some((parent, ancestor));
        }
        if ancestor.kind() == SyntaxKind::BIN_EXPR {
            return None;
        }
    }
    None
}

fn is_comparison_expr(node: &SyntaxNode) -> bool {
    has_any_direct_token(
        node,
        &[
            SyntaxKind::EQ2,
            SyntaxKind::NEQ,
            SyntaxKind::L_ANGLE,
            SyntaxKind::R_ANGLE,
            SyntaxKind::LTEQ,
            SyntaxKind::GTEQ,
        ],
    )
}

fn comparison_peer_raw_type(
    bin_expr: &SyntaxNode,
    operand: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    bin_expr
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .filter(|child| child.text_range() != operand.text_range())
        .find_map(|child| comparison_operand_raw_type(&child, semantic))
}

fn comparison_operand_raw_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    match node.kind() {
        SyntaxKind::PAREN_EXPR => {
            first_expr_child(node).and_then(|expr| comparison_operand_raw_type(&expr, semantic))
        }
        SyntaxKind::CAST_EXPR => type_node(node, semantic),
        SyntaxKind::LITERAL => literal_type(node),
        SyntaxKind::PATH_EXPR => path_expr_type(node, semantic),
        SyntaxKind::INDEX_EXPR => index_expr_type(node, semantic),
        SyntaxKind::BIN_EXPR => bin_expr_type(node, semantic),
        SyntaxKind::CALL_EXPR => call_expr_type(node, semantic),
        SyntaxKind::METHOD_CALL_EXPR => method_call_expr_type(node, semantic),
        _ => expr_type(node, semantic),
    }
}

fn borrow_shared_type(typ: &str) -> String {
    if typ.trim_start().starts_with('&') {
        typ.into()
    } else {
        format!("&{typ}")
    }
}

fn unborrow_type(typ: &str) -> String {
    typ.trim_start()
        .strip_prefix("&mut ")
        .or_else(|| typ.trim_start().strip_prefix('&'))
        .unwrap_or(typ)
        .to_string()
}

fn return_expr_type(node: &SyntaxNode) -> Option<String> {
    if node
        .ancestors()
        .any(|ancestor| ancestor.kind() == SyntaxKind::MATCH_ARM)
    {
        Some("()".into())
    } else {
        Some("!".into())
    }
}

fn expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    match node.kind() {
        SyntaxKind::LITERAL => comparison_operand_context_type(node, semantic)
            .or_else(|| literal_context_type(node, semantic))
            .or_else(|| literal_type(node)),
        SyntaxKind::TUPLE_EXPR => tuple_type(node, semantic),
        SyntaxKind::ARRAY_EXPR => array_type(node, semantic),
        SyntaxKind::PAREN_EXPR => comparison_operand_context_type(node, semantic)
            .or_else(|| first_expr_child(node).and_then(|expr| expr_type(&expr, semantic))),
        SyntaxKind::BREAK_EXPR | SyntaxKind::CONTINUE_EXPR => Some("!".into()),
        SyntaxKind::RETURN_EXPR => return_expr_type(node),
        SyntaxKind::REF_EXPR => ref_expr_type(node, semantic),
        SyntaxKind::PREFIX_EXPR => prefix_expr_type(node, semantic),
        SyntaxKind::TRY_EXPR => try_expr_type(node, semantic),
        SyntaxKind::CAST_EXPR => {
            comparison_operand_context_type(node, semantic).or_else(|| type_node(node, semantic))
        }
        SyntaxKind::BLOCK_EXPR => block_expr_type(node, semantic),
        SyntaxKind::FOR_EXPR => Some("()".into()),
        SyntaxKind::WHILE_EXPR | SyntaxKind::LOOP_EXPR => Some("()".into()),
        SyntaxKind::IF_EXPR => {
            block_tail_expr_context_type(node, semantic).or_else(|| if_expr_type(node, semantic))
        }
        SyntaxKind::LET_EXPR => Some("bool".into()),
        SyntaxKind::PATH_EXPR => assignment_lhs_path_context_type(node, semantic)
            .or_else(|| generic_double_deref_peer_context_type(node, semantic))
            .or_else(|| comparison_operand_context_type(node, semantic))
            .or_else(|| path_expr_type(node, semantic)),
        SyntaxKind::RANGE_EXPR => range_expr_type(node, semantic),
        SyntaxKind::RECORD_EXPR => record_expr_type(node, semantic),
        SyntaxKind::FIELD_EXPR => field_expr_type(node, semantic).map(|typ| {
            let typ = index_base_adjusted_type(node, typ);
            method_receiver_adjusted_type(node, typ, semantic)
        }),
        SyntaxKind::INDEX_EXPR => method_argument_context_type(node, semantic)
            .or_else(|| comparison_operand_context_type(node, semantic))
            .or_else(|| index_expr_type(node, semantic)),
        SyntaxKind::BIN_EXPR => comparison_operand_context_type(node, semantic)
            .or_else(|| bin_expr_type(node, semantic)),
        SyntaxKind::MATCH_EXPR => tail_expr_return_context_type(node, semantic)
            .or_else(|| match_expr_type(node, semantic)),
        SyntaxKind::CALL_EXPR => comparison_operand_context_type(node, semantic)
            .or_else(|| call_expr_type(node, semantic)),
        SyntaxKind::METHOD_CALL_EXPR => {
            comparison_operand_context_type(node, semantic).or_else(|| {
                method_call_expr_type(node, semantic)
                    .map(|typ| method_receiver_adjusted_type(node, typ, semantic))
            })
        }
        SyntaxKind::MACRO_EXPR => macro_expr_type(node, semantic),
        _ => None,
    }
}

fn path_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if let Some(signature) = enum_variant_constructor_signature(node, semantic) {
        return Some(signature);
    }
    if let Some(typ) = assignment_lhs_path_context_type(node, semantic) {
        return Some(typ);
    }
    if let Some(typ) = returned_some_argument_context_type(node, semantic) {
        return Some(typ);
    }
    if let Some(typ) = method_argument_context_type(node, semantic) {
        return Some(typ);
    }
    path_expr_name(node).and_then(|name| {
        if name == "self" {
            enclosing_method_self_param_type(node, semantic)
                .or_else(|| enclosing_impl_self_type(node, semantic))
                .map(|typ| adjusted_expr_type(node, typ, semantic))
        } else if let Some(enum_type) = enum_variant_value_type(node, semantic) {
            Some(enum_type)
        } else if let Some(function_type) = associated_function_path_type(node, semantic) {
            Some(function_type)
        } else if let Some(function_type) = tuple_struct_constructor_signature(node, semantic) {
            Some(function_type)
        } else if let Some(function_type) = sysroot_constructor_path_type(node, semantic) {
            Some(function_type)
        } else if let Some(function_type) = function_path_type(node, &name, semantic) {
            Some(function_type)
        } else {
            semantic
                .resolve_var(node, &name)
                .map(|typ| adjusted_expr_type(node, typ, semantic))
                .or_else(|| for_pattern_binding_type_for_path(node, semantic))
                .or_else(|| index_usage_type_for_name(node, &name))
        }
    })
}

fn assignment_lhs_path_context_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if !is_assignment_lhs(node) {
        return None;
    }
    if is_index_operand(node) {
        return None;
    }
    let name = path_expr_name(node)?;
    semantic
        .resolve_var(node, &name)
        .or_else(|| index_usage_type_for_name(node, &name))
        .map(|typ| borrow_mut_type(&typ))
}

fn first_expr_child(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.children().find(|child| is_expr_kind(child.kind()))
}

fn block_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let stmt_list = node
        .descendants()
        .find(|child| child.kind() == SyntaxKind::STMT_LIST)?;
    let last = stmt_list
        .children()
        .filter(|child| child.kind() == SyntaxKind::EXPR_STMT || is_expr_kind(child.kind()))
        .last();
    match last {
        Some(expr_stmt)
            if expr_stmt.kind() == SyntaxKind::EXPR_STMT
                && has_direct_token(&expr_stmt, SyntaxKind::SEMICOLON) =>
        {
            if node
                .parent()
                .is_some_and(|parent| parent.kind() == SyntaxKind::LET_ELSE)
                && expr_stmt
                    .children()
                    .find(|child| is_expr_kind(child.kind()))
                    .and_then(|expr| expr_type(&expr, semantic))
                    .as_deref()
                    == Some("!")
            {
                return Some("!".into());
            }
            Some("()".into())
        }
        Some(expr) if is_expr_kind(expr.kind()) => expr_type(&expr, semantic),
        _ => Some("()".into()),
    }
}

fn if_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    node.children()
        .filter(|child| child.kind() == SyntaxKind::BLOCK_EXPR)
        .filter_map(|block| expr_type(&block, semantic))
        .next()
        .or_else(|| Some("()".into()))
}

fn range_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if has_direct_token(node, SyntaxKind::DOT2)
        && !node.children().any(|child| is_expr_kind(child.kind()))
    {
        return Some("core::ops::range::RangeFull".into());
    }
    let element_type = range_element_type(node, semantic)?;
    let operand_count = node
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .count();
    let text = node.text().to_string();
    let range_type = if operand_count == 1 && text.trim_start().starts_with("..=") {
        "RangeToInclusive"
    } else if operand_count == 1 && text.trim_start().starts_with("..") {
        "RangeTo"
    } else if operand_count == 1 && has_direct_token(node, SyntaxKind::DOT2) {
        "RangeFrom"
    } else if has_direct_token(node, SyntaxKind::DOT2EQ) {
        "RangeInclusive"
    } else {
        "Range"
    };
    Some(format!("core::ops::range::{range_type}<{element_type}>"))
}

fn range_operand_context_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let range = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::RANGE_EXPR)?;
    let operand = range
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .find(|operand| range_contains(operand.text_range(), node.text_range()))?;
    range_element_type(&range, semantic)
        .filter(|_| range_contains(operand.text_range(), node.text_range()))
}

fn range_element_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if is_range_index_operand(node) {
        return Some("usize".into());
    }
    if let Some(typ) = for_range_index_item_type(node) {
        return Some(typ);
    }
    let mut first = None;
    for operand in node.children().filter(|child| is_expr_kind(child.kind())) {
        let typ = range_operand_raw_type(&operand, semantic).map(|typ| unborrow_type(&typ));
        if first.is_none() {
            first = typ.clone();
        }
        if let Some(typ) = typ
            && typ != "i32"
        {
            return Some(typ);
        }
    }
    first
}

fn for_range_index_item_type(range_expr: &SyntaxNode) -> Option<String> {
    let for_expr = range_expr
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::FOR_EXPR)?;
    let range_start = range_expr.text_range().start();
    for_expr
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::IDENT_PAT)
        .filter(|node| node.text_range().end() <= range_start)
        .find_map(|pat| for_index_usage_type(&pat, &for_expr))
}

fn is_range_index_operand(node: &SyntaxNode) -> bool {
    let Some(index_expr) = node
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::INDEX_EXPR)
    else {
        return false;
    };
    first_expr_child(&index_expr).is_none_or(|base| base.text_range() != node.text_range())
}

fn range_operand_raw_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    match node.kind() {
        SyntaxKind::PAREN_EXPR => {
            first_expr_child(node).and_then(|expr| range_operand_raw_type(&expr, semantic))
        }
        SyntaxKind::CAST_EXPR => type_node(node, semantic),
        SyntaxKind::LITERAL => literal_type(node),
        SyntaxKind::PATH_EXPR => path_expr_type(node, semantic),
        SyntaxKind::BIN_EXPR => bin_expr_type(node, semantic),
        _ => expr_type(node, semantic),
    }
}

fn literal_type(node: &SyntaxNode) -> Option<String> {
    node.children_with_tokens().find_map(|child| match child {
        NodeOrToken::Token(token) => match token.kind() {
            SyntaxKind::INT_NUMBER => Some(infer_int_type(token.text())),
            SyntaxKind::FLOAT_NUMBER => Some(infer_float_type(token.text())),
            SyntaxKind::STRING => Some("&str".into()),
            SyntaxKind::BYTE_STRING => Some(format!("&[u8; {}]", byte_string_len(token.text()))),
            SyntaxKind::C_STRING => Some("&CStr".into()),
            SyntaxKind::CHAR => Some("char".into()),
            SyntaxKind::BYTE => Some("u8".into()),
            SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW => Some("bool".into()),
            _ => None,
        },
        NodeOrToken::Node(_) => None,
    })
}

fn infer_int_type(text: &str) -> String {
    const SUFFIXES: &[&str] = &[
        "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize",
    ];
    match SUFFIXES.iter().find(|suffix| text.ends_with(**suffix)) {
        Some(suffix) => (*suffix).into(),
        None => "i32".into(),
    }
}

fn infer_float_type(text: &str) -> String {
    if text.ends_with("f32") {
        "f32".into()
    } else {
        "f64".into()
    }
}

fn byte_string_len(text: &str) -> usize {
    text.strip_prefix("b\"")
        .and_then(|value| value.strip_suffix('"'))
        .map(str::len)
        .unwrap_or(0)
}

fn tuple_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let exprs: Vec<_> = node
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .collect();
    if exprs.is_empty() {
        Some("()".into())
    } else if let Some(types) = tuple_initializer_context_types(node, semantic) {
        let types = types
            .into_iter()
            .enumerate()
            .map(|(idx, typ)| {
                if typ == "ANY" {
                    exprs
                        .get(idx)
                        .and_then(|expr| {
                            expr_type(expr, semantic)
                                .filter(|typ| typ != "ANY")
                                .or_else(|| tuple_element_raw_type(expr, semantic))
                        })
                        .unwrap_or(typ)
                } else {
                    typ
                }
            })
            .collect::<Vec<_>>();
        Some(format!("({})", types.join(", ")))
    } else {
        let types: Vec<_> = exprs
            .iter()
            .map(|expr| expr_type(expr, semantic).unwrap_or_else(|| "ANY".into()))
            .collect();
        Some(format!("({})", types.join(", ")))
    }
}

fn tuple_element_raw_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    match node.kind() {
        SyntaxKind::PATH_EXPR => path_expr_name(node).and_then(|name| {
            semantic
                .resolve_var(node, &name)
                .map(|typ| adjusted_expr_type(node, typ, semantic))
                .or_else(|| for_pattern_binding_type_for_path(node, semantic))
                .or_else(|| index_usage_type_for_name(node, &name))
        }),
        SyntaxKind::LITERAL => literal_type(node),
        SyntaxKind::BIN_EXPR => bin_expr_type(node, semantic),
        _ => None,
    }
}

fn for_pattern_binding_type_for_path(
    path_expr: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    let name = path_expr_name(path_expr)?;
    path_expr
        .ancestors()
        .filter(|ancestor| ancestor.kind() == SyntaxKind::FOR_EXPR)
        .find_map(|for_expr| {
            for_expr
                .descendants()
                .filter(|node| node.kind() == SyntaxKind::IDENT_PAT)
                .filter(|pat| pat.text_range().end() <= path_expr.text_range().start())
                .filter(|pat| ident_name(pat).as_deref() == Some(name.as_str()))
                .find_map(|pat| for_pattern_ident_type(&pat, &for_expr, semantic))
        })
}

fn tuple_initializer_element_context_type(
    node: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    let tuple_expr = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::TUPLE_EXPR)?;
    let types = tuple_initializer_context_types(&tuple_expr, semantic)?;
    let exprs = tuple_expr
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .collect::<Vec<_>>();
    let index = exprs
        .iter()
        .position(|expr| range_contains(expr.text_range(), node.text_range()))?;
    types.get(index).cloned()
}

fn tuple_initializer_context_types(
    tuple_expr: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<Vec<String>> {
    let let_stmt = tuple_expr
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::LET_STMT)?;
    let initializer = initializer_expr(&let_stmt)?;
    if initializer.text_range() != tuple_expr.text_range() {
        return None;
    }
    let tuple_pat = direct_child(&let_stmt, SyntaxKind::TUPLE_PAT)?;
    let types = tuple_pat
        .children()
        .filter(|child| is_pattern_kind(child.kind()))
        .map(|pat| tuple_pattern_context_type(&pat, &let_stmt, semantic))
        .collect::<Option<Vec<_>>>()?;
    (!types.is_empty()).then_some(types)
}

fn tuple_pattern_context_type(
    pattern: &SyntaxNode,
    let_stmt: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    match pattern.kind() {
        SyntaxKind::IDENT_PAT => later_assignment_type(pattern, let_stmt, semantic),
        _ => None,
    }
}

fn array_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if let Some(declared) = literal_context_type(node, semantic) {
        return Some(declared);
    }
    let exprs: Vec<_> = node
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .collect();
    let element_type = exprs
        .first()
        .and_then(|expr| expr_type(expr, semantic))
        .unwrap_or_else(|| "ANY".into());
    if has_direct_token(node, SyntaxKind::SEMICOLON) {
        let count = exprs
            .get(1)
            .map(|expr| expr.text().to_string())
            .unwrap_or_else(|| "0".into());
        Some(format!("[{}; {}]", element_type, count))
    } else {
        Some(format!("[{}; {}]", element_type, exprs.len()))
    }
}

#[derive(Default)]
struct SemanticModel {
    crate_name: Option<String>,
    sysroot: bool,
    /// Resolved binding types keyed by the binding node's range so that two
    /// bindings with the same name (shadowing) never collide.
    variables: HashMap<TextRange, String>,
    structs: HashMap<String, StructInfo>,
    traits: HashMap<String, TraitInfo>,
    functions: HashMap<String, String>,
    /// Inherent/trait methods keyed by the Self type's full name.
    impls: HashMap<String, ImplInfo>,
}

#[derive(Default)]
struct StructInfo {
    full_name: String,
    fields: HashMap<String, String>,
    tuple_fields: Vec<String>,
    variant_tuple_fields: HashMap<String, Vec<String>>,
}

#[derive(Default)]
struct TraitInfo {
    full_name: String,
    methods: HashMap<String, Option<String>>,
}

/// Inherent/trait methods for a single Self type, keyed in the model by the
/// Self type's full name. Method name -> declared return type (`None` when the
/// method has no explicit `-> T`).
#[derive(Default)]
struct ImplInfo {
    methods: HashMap<String, Option<String>>,
    mut_methods: HashSet<String>,
}

impl SemanticModel {
    fn new(root: &SyntaxNode, crate_name: Option<&str>, sysroot: bool) -> Self {
        let mut model = Self {
            crate_name: crate_name.map(ToOwned::to_owned),
            sysroot,
            ..Self::default()
        };
        model.collect_structs(root);
        model.collect_traits(root);
        model.collect_functions(root);
        model.collect_impls(root);
        model.collect_variables(root);
        model
    }

    fn collect_structs(&mut self, root: &SyntaxNode) {
        for node in root
            .descendants()
            .filter(|node| matches!(node.kind(), SyntaxKind::STRUCT | SyntaxKind::ENUM))
        {
            let Some(name) = name_child_text(&node) else {
                continue;
            };
            let full_name = self.qualify_value_name(&name);
            let mut info = StructInfo {
                full_name: full_name.clone(),
                ..StructInfo::default()
            };
            self.structs.insert(name.clone(), info.clone());
            self.structs.insert(full_name.clone(), info.clone());
            if node.kind() == SyntaxKind::STRUCT
                && let Some(record_fields) = direct_child(&node, SyntaxKind::RECORD_FIELD_LIST)
            {
                for field in record_fields
                    .children()
                    .filter(|child| child.kind() == SyntaxKind::RECORD_FIELD)
                {
                    if let (Some(field_name), Some(field_type)) =
                        (name_child_text(&field), type_node(&field, self))
                    {
                        info.fields
                            .insert(field_name, declared_type_defaults(field_type, self));
                    }
                }
            }
            if node.kind() == SyntaxKind::STRUCT
                && let Some(tuple_fields) = direct_child(&node, SyntaxKind::TUPLE_FIELD_LIST)
            {
                for field in tuple_fields
                    .children()
                    .filter(|child| child.kind() == SyntaxKind::TUPLE_FIELD)
                {
                    if let Some(field_type) = type_node(&field, self) {
                        info.tuple_fields.push(field_type);
                    }
                }
            }
            if node.kind() == SyntaxKind::ENUM
                && let Some(variant_list) = direct_child(&node, SyntaxKind::VARIANT_LIST)
            {
                for variant in variant_list
                    .children()
                    .filter(|child| child.kind() == SyntaxKind::VARIANT)
                {
                    let Some(variant_name) = name_child_text(&variant) else {
                        continue;
                    };
                    let Some(tuple_fields) = direct_child(&variant, SyntaxKind::TUPLE_FIELD_LIST)
                    else {
                        continue;
                    };
                    let fields = tuple_fields
                        .children()
                        .filter(|child| child.kind() == SyntaxKind::TUPLE_FIELD)
                        .filter_map(|field| type_node(&field, self))
                        .map(|typ| declared_type_defaults(typ, self))
                        .collect::<Vec<_>>();
                    info.variant_tuple_fields.insert(variant_name, fields);
                }
            }
            self.structs.insert(name.clone(), info.clone());
            self.structs.insert(full_name, info);
        }
    }

    fn collect_traits(&mut self, root: &SyntaxNode) {
        for node in root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::TRAIT)
        {
            let Some(name) = name_child_text(&node) else {
                continue;
            };
            let full_name = self.qualify_value_name(&name);
            let mut info = TraitInfo {
                full_name: full_name.clone(),
                ..TraitInfo::default()
            };
            if let Some(items) = direct_child(&node, SyntaxKind::ASSOC_ITEM_LIST) {
                for func in items
                    .children()
                    .filter(|child| child.kind() == SyntaxKind::FN)
                {
                    if let Some(method_name) = name_child_text(&func) {
                        let return_type = direct_child(&func, SyntaxKind::RET_TYPE)
                            .and_then(|ret| type_node(&ret, self));
                        info.methods.insert(method_name, return_type);
                    }
                }
            }
            self.traits.insert(name.clone(), info.clone());
            self.traits.insert(full_name, info);
        }
    }

    fn collect_functions(&mut self, root: &SyntaxNode) {
        for node in root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::FN)
        {
            if let Some(name) = name_child_text(&node) {
                self.functions
                    .insert(name.clone(), self.qualify_value_name(&name));
            }
        }
    }

    fn collect_variables(&mut self, root: &SyntaxNode) {
        for node in root.descendants() {
            match node.kind() {
                SyntaxKind::PARAM => {
                    if let (Some(pat), Some(typ)) = (
                        direct_child(&node, SyntaxKind::IDENT_PAT),
                        type_node(&node, self),
                    ) {
                        self.variables
                            .insert(pat.text_range(), declared_type_defaults(typ, self));
                    }
                }
                SyntaxKind::LET_STMT => {
                    if let Some(pat) = direct_child(&node, SyntaxKind::IDENT_PAT) {
                        let declared =
                            type_node(&node, self).map(|typ| let_declared_type(&node, typ, self));
                        let initializer =
                            initializer_expr(&node).and_then(|expr| expr_type(&expr, self));
                        let later = || later_assignment_type(&pat, &node, self);
                        let typ = declared.or_else(|| match initializer {
                            Some(typ) if typ == "i32" => later().or(Some(typ)),
                            Some(typ) => Some(typ),
                            None => later(),
                        });
                        if let Some(typ) = typ {
                            self.variables.insert(pat.text_range(), typ);
                        }
                    } else if let Some(tuple_pat) = direct_child(&node, SyntaxKind::TUPLE_PAT) {
                        for pat in tuple_pat
                            .descendants()
                            .filter(|child| child.kind() == SyntaxKind::IDENT_PAT)
                        {
                            if let Some(typ) = later_assignment_type(&pat, &node, self) {
                                self.variables.insert(pat.text_range(), typ);
                            }
                        }
                    }
                }
                SyntaxKind::CONST => {
                    if let (Some(name), Some(typ)) = (
                        direct_child(&node, SyntaxKind::NAME),
                        type_node(&node, self),
                    ) {
                        self.variables.insert(name.text_range(), typ);
                    }
                }
                SyntaxKind::FOR_EXPR | SyntaxKind::LET_EXPR => {
                    for pat in node
                        .descendants()
                        .filter(|child| child.kind() == SyntaxKind::IDENT_PAT)
                    {
                        let typ = match node.kind() {
                            SyntaxKind::FOR_EXPR => for_pattern_ident_type(&pat, &node, self),
                            SyntaxKind::LET_EXPR => let_expr_pattern_ident_type(&pat, &node, self),
                            _ => None,
                        };
                        if let Some(typ) = typ {
                            self.variables.insert(pat.text_range(), typ);
                        }
                    }
                }
                SyntaxKind::MATCH_ARM => {
                    for pat in node
                        .descendants()
                        .filter(|child| child.kind() == SyntaxKind::IDENT_PAT)
                    {
                        if let Some(typ) = match_arm_pattern_ident_type(&pat, &node, self) {
                            self.variables.insert(pat.text_range(), typ);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn collect_impls(&mut self, root: &SyntaxNode) {
        for node in root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::IMPL)
        {
            // For `impl Foo` and `impl Trait for Foo` alike the Self type is the
            // last type child (it follows the optional `for`).
            let Some(self_type) = node
                .children()
                .filter(|child| is_type_kind(child.kind()))
                .last()
                .and_then(|child| type_text(&child, self))
            else {
                continue;
            };
            let Some(items) = direct_child(&node, SyntaxKind::ASSOC_ITEM_LIST) else {
                continue;
            };
            let mut methods = HashMap::new();
            let mut mut_methods = HashSet::new();
            for func in items
                .children()
                .filter(|child| child.kind() == SyntaxKind::FN)
            {
                if let Some(name) = name_child_text(&func) {
                    let return_type = direct_child(&func, SyntaxKind::RET_TYPE)
                        .and_then(|ret| type_node(&ret, self));
                    if direct_child(&func, SyntaxKind::PARAM_LIST)
                        .and_then(|params| {
                            params
                                .children()
                                .find(|child| child.kind() == SyntaxKind::SELF_PARAM)
                        })
                        .is_some_and(|self_param| has_direct_token(&self_param, SyntaxKind::MUT_KW))
                    {
                        mut_methods.insert(name.clone());
                    }
                    methods.insert(name, return_type);
                }
            }
            let entry = self.impls.entry(self_type).or_default();
            entry.methods.extend(methods);
            entry.mut_methods.extend(mut_methods);
        }
    }

    /// Resolve a variable reference to the type of its nearest enclosing
    /// binding, respecting lexical scope and shadowing. Walks the use site's
    /// ancestors from innermost outward; within each scope the latest binding
    /// that precedes (or, for params, encloses) the use wins.
    fn resolve_var(&self, use_site: &SyntaxNode, name: &str) -> Option<String> {
        let use_start = use_site.text_range().start();
        for scope in use_site.ancestors() {
            let mut best: Option<(TextRange, &String)> = None;
            for binding in
                scope_bindings(&scope, name).chain(enclosing_scope_pattern_bindings(&scope, name))
            {
                // A `let`/`const` binding only comes into scope after its whole
                // declaration statement (so `let x = x + 1;` reads the outer
                // `x`); params, declared in the PARAM_LIST, precede the body.
                let is_param = binding
                    .parent()
                    .is_some_and(|parent| parent.kind() == SyntaxKind::PARAM);
                let is_control_pattern = is_control_pattern_binding(&binding);
                let visible_from = if is_param {
                    binding.text_range().start()
                } else if is_control_pattern {
                    binding.text_range().end()
                } else {
                    binding
                        .parent()
                        .map(|stmt| stmt.text_range().end())
                        .unwrap_or_else(|| binding.text_range().end())
                };
                if visible_from > use_start {
                    continue;
                }
                if let Some(typ) = self.variables.get(&binding.text_range()) {
                    match best {
                        Some((range, _)) if range.start() >= binding.text_range().start() => {}
                        _ => best = Some((binding.text_range(), typ)),
                    }
                }
            }
            if let Some((_, typ)) = best {
                return Some(typ.clone());
            }
        }
        None
    }

    fn qualify_type_name(&self, name: &str) -> String {
        if is_builtin_type_name(name) || name.contains("::") {
            name.into()
        } else if self.sysroot
            && let Some(qualified) = sysroot_qualified_type_name(name)
        {
            qualified.into()
        } else if self.structs.contains_key(name) {
            self.qualify_value_name(name)
        } else {
            name.into()
        }
    }

    fn qualify_value_name(&self, name: &str) -> String {
        self.crate_name
            .as_ref()
            .map(|crate_name| format!("{crate_name}::{name}"))
            .unwrap_or_else(|| name.into())
    }
}

fn is_control_pattern_binding(binding: &SyntaxNode) -> bool {
    for ancestor in binding.ancestors().skip(1) {
        match ancestor.kind() {
            SyntaxKind::FOR_EXPR | SyntaxKind::LET_EXPR => return true,
            SyntaxKind::LET_STMT
            | SyntaxKind::PARAM
            | SyntaxKind::CONST
            | SyntaxKind::FN
            | SyntaxKind::BLOCK_EXPR => return false,
            _ => {}
        }
    }
    false
}

impl Clone for StructInfo {
    fn clone(&self) -> Self {
        Self {
            full_name: self.full_name.clone(),
            fields: self.fields.clone(),
            tuple_fields: self.tuple_fields.clone(),
            variant_tuple_fields: self.variant_tuple_fields.clone(),
        }
    }
}

impl Clone for TraitInfo {
    fn clone(&self) -> Self {
        Self {
            full_name: self.full_name.clone(),
            methods: self.methods.clone(),
        }
    }
}

fn type_text(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    match node.kind() {
        SyntaxKind::PATH_TYPE => path_type_text(node, semantic),
        SyntaxKind::REF_TYPE => {
            let inner = node
                .children()
                .find(|child| is_type_kind(child.kind()))
                .and_then(|child| type_text(&child, semantic))?;
            let mutability = if has_direct_token(node, SyntaxKind::MUT_KW) {
                "mut "
            } else {
                ""
            };
            Some(normalize_composite_type_text(
                format!("&{mutability}{inner}"),
                semantic,
            ))
        }
        SyntaxKind::PTR_TYPE => {
            let inner = node
                .children()
                .find(|child| is_type_kind(child.kind()))
                .and_then(|child| type_text(&child, semantic))?;
            let qualifier = if has_direct_token(node, SyntaxKind::CONST_KW) {
                "const "
            } else {
                "mut "
            };
            Some(normalize_composite_type_text(
                format!("*{qualifier}{inner}"),
                semantic,
            ))
        }
        SyntaxKind::SLICE_TYPE => {
            let inner = node
                .children()
                .find(|child| is_type_kind(child.kind()))
                .and_then(|child| type_text(&child, semantic))?;
            Some(normalize_composite_type_text(
                format!("[{inner}]"),
                semantic,
            ))
        }
        SyntaxKind::ARRAY_TYPE => {
            let inner = node
                .children()
                .find(|child| is_type_kind(child.kind()))
                .and_then(|child| type_text(&child, semantic))?;
            let count = node
                .children()
                .find(|child| child.kind() == SyntaxKind::CONST_ARG)
                .map(|child| child.text().to_string())
                .unwrap_or_default();
            Some(normalize_composite_type_text(
                format!("[{inner}; {count}]"),
                semantic,
            ))
        }
        SyntaxKind::TUPLE_TYPE => {
            let types: Vec<_> = node
                .children()
                .filter(|child| is_type_kind(child.kind()))
                .filter_map(|child| type_text(&child, semantic))
                .collect();
            Some(normalize_composite_type_text(
                format!("({})", types.join(", ")),
                semantic,
            ))
        }
        SyntaxKind::PAREN_TYPE => node
            .children()
            .find(|child| is_type_kind(child.kind()))
            .and_then(|child| type_text(&child, semantic)),
        SyntaxKind::NEVER_TYPE => Some("!".into()),
        _ => Some(node.text().to_string()),
    }
}

fn normalize_composite_type_text(text: String, semantic: &SemanticModel) -> String {
    if semantic.sysroot {
        normalize_sysroot_type_text(&text)
    } else {
        text
    }
}

fn path_expr_name_ref_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let path_expr = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_EXPR)?;
    if path_expr
        .descendants()
        .filter(|child| child.kind() == SyntaxKind::NAME_REF)
        .last()
        .as_ref()
        .is_some_and(|last| last == node)
    {
        if let Some(typ) = generic_double_deref_peer_context_type(&path_expr, semantic) {
            return Some(typ);
        }
        if let Some(typ) = returned_some_argument_context_type(&path_expr, semantic) {
            return Some(typ);
        }
        if let Some(typ) = comparison_operand_context_type(&path_expr, semantic) {
            return Some(typ);
        }
        if let Some(signature) = enum_variant_constructor_signature(&path_expr, semantic) {
            return Some(signature);
        }
        path_expr_name(&path_expr)
            .and_then(|name| {
                tuple_struct_constructor_signature(&path_expr, semantic)
                    .or_else(|| sysroot_constructor_path_type(&path_expr, semantic))
                    .or_else(|| function_path_type(&path_expr, &name, semantic))
                    .or_else(|| {
                        semantic
                            .resolve_var(&path_expr, &name)
                            .map(|typ| adjusted_expr_type(&path_expr, typ, semantic))
                    })
            })
            .filter(|typ| semantic.sysroot || !is_unresolved_generic_container(typ))
    } else {
        None
    }
}

fn adjusted_expr_type(node: &SyntaxNode, typ: String, semantic: &SemanticModel) -> String {
    let typ = lvalue_adjusted_type(node, typ);
    let typ = index_base_adjusted_type(node, typ);
    let typ = field_base_adjusted_type(node, typ, semantic);
    method_receiver_adjusted_type(node, typ, semantic)
}

fn field_base_adjusted_type(node: &SyntaxNode, typ: String, semantic: &SemanticModel) -> String {
    let Some(field_expr) = node
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::FIELD_EXPR)
    else {
        return typ;
    };
    if first_expr_child(&field_expr).is_none_or(|base| base.text_range() != node.text_range()) {
        return typ;
    }
    let base = receiver_base_type(&typ);
    if semantic.structs.contains_key(base) || base.starts_with('(') {
        base.to_string()
    } else {
        typ
    }
}

fn index_base_adjusted_type(node: &SyntaxNode, typ: String) -> String {
    let Some(index_expr) = node
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::INDEX_EXPR)
    else {
        return typ;
    };
    if first_expr_child(&index_expr).is_none_or(|base| base.text_range() != node.text_range()) {
        return typ;
    }
    if index_expr_is_assignment_lhs(&index_expr) {
        return borrow_mut_type(&typ);
    }
    if let Some(inner) = typ.strip_prefix("&mut ") {
        return format!("&{inner}");
    }
    if is_indexable_container_type(&typ) && !typ.trim_start().starts_with('&') {
        return format!("&{typ}");
    }
    typ
}

fn index_expr_is_assignment_lhs(index_expr: &SyntaxNode) -> bool {
    let mut root = index_expr.clone();
    while let Some(parent_index) = root
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::INDEX_EXPR)
    {
        root = parent_index;
    }
    let Some(parent) = root
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::BIN_EXPR)
    else {
        return false;
    };
    if !has_assignment_operator(&parent) {
        return false;
    }
    parent
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .next()
        .is_some_and(|lhs| range_contains(lhs.text_range(), root.text_range()))
}

fn method_receiver_adjusted_type(
    receiver: &SyntaxNode,
    typ: String,
    semantic: &SemanticModel,
) -> String {
    if !semantic.sysroot {
        return typ;
    }
    let Some(call) = receiver
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::METHOD_CALL_EXPR)
    else {
        return typ;
    };
    if first_expr_child(&call).is_none_or(|first| first.text_range() != receiver.text_range()) {
        return typ;
    }
    let Some(method_name) =
        direct_child(&call, SyntaxKind::NAME_REF).map(|name| name.text().to_string())
    else {
        return typ;
    };
    if is_vec_slice_shared_method(&method_name)
        && let Some(slice_type) = vec_slice_type(&typ)
    {
        return slice_type;
    }
    if typ.trim_start().starts_with("&mut ")
        && user_method_requires_mut_receiver(&typ, &method_name, semantic)
    {
        return typ;
    }
    if let Some(inner) = typ.strip_prefix("&mut ")
        && !is_mutating_method(&method_name)
    {
        return format!("&{inner}");
    }
    if method_name == "iter"
        && let Some(element_type) = vec_receiver_element_type(&typ)
    {
        return format!("&[{element_type}]");
    }
    if method_name == "into_iter" {
        return typ;
    }
    if let Some(inner) = box_inner_type(&typ)
        && semantic.structs.contains_key(&inner)
    {
        return boxed_receiver_inner_type(&typ, &inner);
    }
    if let Some(inner) = box_inner_type(&typ)
        && semantic
            .impls
            .get(&inner)
            .is_some_and(|info| info.methods.contains_key(&method_name))
    {
        return boxed_receiver_inner_type(&typ, &inner);
    }
    if is_str_receiver_method(&method_name) && is_string_like(&typ) {
        return "&str".into();
    }
    if is_vec_slice_mutating_method(&method_name)
        && let Some(slice_type) = vec_mut_slice_type(&typ)
    {
        return slice_type;
    }
    if is_mutating_collection_receiver(&typ) && !is_mutating_method(&method_name) {
        return borrow_shared_type(&typ);
    }
    if let Some(trait_name) = boxed_dyn_trait_name(&typ)
        && trait_method_return_type(&trait_name, &method_name, semantic).is_some()
    {
        return format!("&dyn {trait_name}");
    }
    match method_name.as_str() {
        "push" | "push_str" if is_owned_string(&typ) => borrow_mut_type(&typ),
        "next" | "peek" | "position" => borrow_mut_type(&typ),
        "is_alphanumeric" if typ == "&char" => "char".into(),
        "to_ascii_lowercase" | "to_ascii_uppercase" | "is_ascii_lowercase"
        | "is_ascii_uppercase"
            if typ == "char" =>
        {
            borrow_shared_type(&typ)
        }
        method if is_mutating_method(method) && is_mutating_collection_receiver(&typ) => {
            borrow_mut_type(&typ)
        }
        "entry" | "insert" if is_hashmap_type(&typ) => borrow_mut_type(&typ),
        "clone" if !typ.trim_start().starts_with('&') => format!("&{typ}"),
        "trim" if is_owned_string(&typ) => "&str".into(),
        "is_empty" if is_string_like(&typ) => "&str".into(),
        _ => typ,
    }
}

fn method_receiver_name_ref_adjusted_type(
    node: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    let path_expr = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_EXPR)?;
    if path_expr
        .descendants()
        .filter(|child| child.kind() == SyntaxKind::NAME_REF)
        .last()
        .as_ref()
        .is_none_or(|last| last.text_range() != node.text_range())
    {
        return None;
    }
    let call = path_expr
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::METHOD_CALL_EXPR)?;
    if first_expr_child(&call)
        .is_none_or(|receiver| receiver.text_range() != path_expr.text_range())
    {
        return None;
    }
    let name = path_expr_name(&path_expr)?;
    semantic
        .resolve_var(&path_expr, &name)
        .map(|typ| method_receiver_adjusted_type(&path_expr, typ, semantic))
}

fn boxed_receiver_inner_type(receiver_type: &str, inner: &str) -> String {
    let receiver_type = receiver_type.trim_start();
    if receiver_type.starts_with("&mut ") {
        format!("&mut {inner}")
    } else if receiver_type.starts_with('&') {
        format!("&{inner}")
    } else {
        inner.into()
    }
}

fn user_method_requires_mut_receiver(
    receiver_type: &str,
    method_name: &str,
    semantic: &SemanticModel,
) -> bool {
    let self_type = receiver_base_type(receiver_type);
    semantic
        .impls
        .get(self_type)
        .is_some_and(|info| info.mut_methods.contains(method_name))
}

fn is_mutating_collection_receiver(typ: &str) -> bool {
    let base = receiver_base_type(typ);
    base.starts_with("alloc::vec::Vec<")
        || base.starts_with("std::collections::hash::map::HashMap<")
        || base.starts_with("std::collections::hash::set::HashSet<")
        || base.starts_with("alloc::collections::vec_deque::VecDeque<")
        || base.starts_with("alloc::collections::binary_heap::BinaryHeap<")
}

fn is_mutating_method(method_name: &str) -> bool {
    matches!(
        method_name,
        "push"
            | "push_str"
            | "push_back"
            | "push_front"
            | "extend_from_slice"
            | "pop"
            | "pop_front"
            | "pop_back"
            | "remove"
            | "insert"
            | "entry"
            | "or_default"
            | "append"
            | "extend"
            | "retain"
            | "clear"
            | "truncate"
            | "resize"
            | "reverse"
            | "sort"
            | "sort_by"
            | "sort_by_key"
            | "fill"
            | "last_mut"
            | "get_mut"
            | "next"
            | "peek"
            | "position"
    )
}

fn borrow_mut_type(typ: &str) -> String {
    if typ.trim_start().starts_with('&') {
        typ.into()
    } else {
        format!("&mut {typ}")
    }
}

fn prefix_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let operand = first_expr_child(node)?;
    if has_direct_token(node, SyntaxKind::BANG) {
        Some("bool".into())
    } else if has_direct_token(node, SyntaxKind::STAR) {
        expr_type(&operand, semantic).and_then(|typ| {
            if is_assignment_lhs(node) && typ.trim_start().starts_with('&') {
                return Some(typ);
            }
            if let Some(generic_ref) = generic_comparison_deref_type(node, &typ) {
                return Some(generic_ref);
            }
            typ.strip_prefix("*const ")
                .or_else(|| typ.strip_prefix("*mut "))
                .map(str::to_string)
        })
    } else {
        expr_type(&operand, semantic)
    }
}

fn generic_comparison_deref_type(node: &SyntaxNode, operand_type: &str) -> Option<String> {
    let (_, comparison_operand) = enclosing_comparison_operand(node)?;
    if comparison_operand.kind() != SyntaxKind::PREFIX_EXPR
        || !range_contains(comparison_operand.text_range(), node.text_range())
    {
        return None;
    }
    let typ = operand_type.trim_start();
    if let Some(inner) = typ.strip_prefix("&&") {
        let inner = inner.trim_start();
        if is_simple_generic_type_param(inner) {
            return Some(format!("&{inner}"));
        }
    }
    if comparison_operand.text_range() == node.text_range()
        && let Some(inner) = typ.strip_prefix('&')
    {
        let inner = inner
            .trim_start()
            .strip_prefix("mut ")
            .unwrap_or(inner.trim_start())
            .trim_start();
        if is_simple_generic_type_param(inner) {
            return Some(typ.into());
        }
    }
    None
}

fn generic_double_deref_peer_context_type(
    node: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    let (bin_expr, operand) = enclosing_comparison_operand(node)?;
    if operand.text_range() != node.text_range() {
        return None;
    }
    let name = path_expr_name(node)?;
    let typ = semantic.resolve_var(node, &name)?;
    let base = receiver_base_type(&typ);
    if !is_simple_generic_type_param(base) {
        return None;
    }
    bin_expr
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .filter(|child| child.text_range() != operand.text_range())
        .any(|child| is_double_deref_prefix_expr(&child))
        .then(|| format!("&{base}"))
}

fn is_double_deref_prefix_expr(node: &SyntaxNode) -> bool {
    if node.kind() != SyntaxKind::PREFIX_EXPR || !has_direct_token(node, SyntaxKind::STAR) {
        return false;
    }
    first_expr_child(node).is_some_and(|inner| {
        inner.kind() == SyntaxKind::PREFIX_EXPR && has_direct_token(&inner, SyntaxKind::STAR)
    })
}

fn is_simple_generic_type_param(typ: &str) -> bool {
    let mut chars = typ.chars();
    chars.next().is_some_and(|first| first.is_ascii_uppercase())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn try_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let operand = first_expr_child(node)?;
    expr_type(&operand, semantic)
        .map(|typ| try_success_type(&typ))
        .or_else(|| method_argument_context_type(node, semantic))
}

fn try_success_type(typ: &str) -> String {
    let base = receiver_base_type(typ);
    if let Some(inner) = base
        .strip_prefix("core::option::Option<")
        .or_else(|| base.strip_prefix("Option<"))
        .and_then(|inner| inner.strip_suffix('>'))
        .and_then(first_top_level_arg)
    {
        return inner;
    }
    if let Some(ok) = base
        .strip_prefix("core::result::Result<")
        .or_else(|| base.strip_prefix("Result<"))
        .and_then(|inner| inner.strip_suffix('>'))
        .and_then(first_top_level_arg)
    {
        return ok;
    }
    base.to_string()
}

fn method_argument_context_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let arg_list = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::ARG_LIST)?;
    let call = arg_list
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::METHOD_CALL_EXPR)?;
    let args = arg_list
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .collect::<Vec<_>>();
    let (arg_index, arg) = args
        .iter()
        .enumerate()
        .find(|(_, arg)| range_contains(arg.text_range(), node.text_range()))?;
    let method_name = direct_child(&call, SyntaxKind::NAME_REF)?
        .text()
        .to_string();
    let receiver_type = first_expr_child(&call).and_then(|receiver| expr_type(&receiver, semantic));
    let expected = match (method_name.as_str(), arg_index, receiver_type.as_deref()) {
        ("push", 0, Some(typ)) => {
            vec_receiver_element_type(typ).or_else(|| binary_heap_receiver_element_type(typ))
        }
        ("push_back", 0, Some(typ)) => vecdeque_receiver_element_type(typ),
        ("or_insert", 0, Some(typ)) => hashmap_entry_value_type(typ),
        _ => None,
    }?;
    if arg.text_range() == node.text_range() {
        return Some(expected);
    }
    if arg.kind() == SyntaxKind::TUPLE_EXPR {
        let (idx, _) = arg
            .children()
            .filter(|child| is_expr_kind(child.kind()))
            .enumerate()
            .find(|(_, child)| range_contains(child.text_range(), node.text_range()))?;
        return tuple_field_type(&expected, &idx.to_string());
    }
    None
}

fn ref_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let operand = first_expr_child(node)?;
    let operand_type = expr_type(&operand, semantic)?;
    if let Some(expected) = function_argument_context_type(node, semantic) {
        Some(expected)
    } else if ref_expr_coerces_to_str(node, &operand_type) {
        Some("&str".into())
    } else if has_direct_token(node, SyntaxKind::MUT_KW) {
        Some(format!("&mut {operand_type}"))
    } else {
        Some(format!("&{operand_type}"))
    }
}

fn function_argument_context_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let arg_list = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::ARG_LIST)?;
    let call = arg_list
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::CALL_EXPR)?;
    let args = arg_list
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .collect::<Vec<_>>();
    let (arg_index, arg) = args
        .iter()
        .enumerate()
        .find(|(_, arg)| range_contains(arg.text_range(), node.text_range()))?;
    if arg.text_range() != node.text_range() {
        return None;
    }
    if let Some(typ) = enum_variant_argument_type(&call, arg_index, semantic) {
        return Some(typ);
    }
    let name = call_name(&call)?;
    let decl = function_decl(&call, &name)?;
    direct_child(&decl, SyntaxKind::PARAM_LIST)?
        .children()
        .filter(|child| child.kind() == SyntaxKind::PARAM)
        .nth(arg_index)
        .and_then(|param| type_node(&param, semantic))
        .map(|typ| declared_type_defaults(typ, semantic))
}

fn enum_variant_argument_type(
    call: &SyntaxNode,
    arg_index: usize,
    semantic: &SemanticModel,
) -> Option<String> {
    let callee = first_expr_child(call)?;
    let names = path_name_refs(&callee)?;
    let [type_name, variant] = names.as_slice() else {
        return None;
    };
    let qualified = semantic.qualify_type_name(type_name);
    let info = semantic
        .structs
        .get(&qualified)
        .or_else(|| semantic.structs.get(type_name))?;
    info.variant_tuple_fields
        .get(variant)?
        .get(arg_index)
        .cloned()
}

fn ref_expr_coerces_to_str(node: &SyntaxNode, operand_type: &str) -> bool {
    if !is_owned_string(operand_type) {
        return false;
    }
    let Some(arg_list) = node
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::ARG_LIST)
    else {
        return false;
    };
    let Some(call) = arg_list
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::METHOD_CALL_EXPR)
    else {
        return false;
    };
    direct_child(&call, SyntaxKind::NAME_REF).is_some_and(|method| method.text() == "push_str")
}

fn record_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    direct_child(node, SyntaxKind::PATH)
        .and_then(|path| {
            path.descendants()
                .find(|child| child.kind() == SyntaxKind::NAME_REF)
        })
        .map(|name| semantic.qualify_type_name(&name.text().to_string()))
}

fn record_expr_name_ref_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let record_expr = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::RECORD_EXPR)?;
    let path = direct_child(&record_expr, SyntaxKind::PATH)?;
    if !range_contains(path.text_range(), node.text_range()) {
        return None;
    }
    record_expr_type(&record_expr, semantic)
}

fn pattern_path_name_ref_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let path = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::PATH)?;
    if !path.ancestors().any(|ancestor| {
        matches!(
            ancestor.kind(),
            SyntaxKind::PATH_PAT | SyntaxKind::RECORD_PAT | SyntaxKind::TUPLE_STRUCT_PAT
        )
    }) {
        return None;
    }
    let first_name_ref = path
        .descendants()
        .find(|child| child.kind() == SyntaxKind::NAME_REF)?;
    if first_name_ref.text_range() != node.text_range() {
        return None;
    }
    semantic
        .structs
        .get(&node.text().to_string())
        .map(|info| info.full_name.clone())
}

fn use_tree_name_ref_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if !semantic.sysroot {
        return None;
    }
    let use_tree = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::USE_TREE)?;
    let name_refs = use_tree
        .descendants()
        .filter(|child| child.kind() == SyntaxKind::NAME_REF)
        .collect::<Vec<_>>();
    if name_refs
        .last()
        .is_none_or(|last| last.text_range() != node.text_range())
    {
        return None;
    }
    let names = name_refs
        .iter()
        .map(|name| name.text().to_string())
        .collect::<Vec<_>>();
    names
        .last()
        .and_then(|name| sysroot_qualified_type_name(name).map(str::to_string))
}

fn field_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let base = first_expr_child(node)?;
    let base_type = expr_type(&base, semantic)?;
    let field_name = direct_child(node, SyntaxKind::NAME_REF)?.text().to_string();
    let (typ, is_tuple_field) = if let Some(tuple_type) = tuple_field_type(&base_type, &field_name)
    {
        (tuple_type, true)
    } else {
        let info = semantic.structs.get(&base_type)?;
        (
            info.fields.get(&field_name).cloned().or_else(|| {
                field_name
                    .parse::<usize>()
                    .ok()
                    .and_then(|idx| info.tuple_fields.get(idx).cloned())
            })?,
            false,
        )
    };
    let typ = if is_tuple_field {
        typ
    } else {
        lvalue_adjusted_type(node, typ)
    };
    if is_binary_comparison_operand(node) {
        Some(borrow_shared_type(&typ))
    } else {
        Some(typ)
    }
}

fn lvalue_adjusted_type(node: &SyntaxNode, typ: String) -> String {
    if is_assignment_lhs(node) {
        format!("&mut {typ}")
    } else {
        typ
    }
}

fn is_assignment_lhs(node: &SyntaxNode) -> bool {
    if is_index_operand(node) {
        return false;
    }
    for ancestor in node.ancestors() {
        let Some(parent) = ancestor.parent() else {
            continue;
        };
        if parent.kind() == SyntaxKind::BIN_EXPR {
            if !has_compound_assignment_operator(&parent) {
                return false;
            }
            return parent
                .children()
                .filter(|child| is_expr_kind(child.kind()))
                .next()
                .is_some_and(|lhs| range_contains(lhs.text_range(), node.text_range()));
        }
        if ancestor.kind() == SyntaxKind::BIN_EXPR {
            return false;
        }
    }
    false
}

fn is_binary_comparison_operand(node: &SyntaxNode) -> bool {
    let Some(parent) = node
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::BIN_EXPR)
    else {
        return false;
    };
    if !has_any_direct_token(
        &parent,
        &[
            SyntaxKind::EQ2,
            SyntaxKind::NEQ,
            SyntaxKind::L_ANGLE,
            SyntaxKind::R_ANGLE,
            SyntaxKind::LTEQ,
            SyntaxKind::GTEQ,
        ],
    ) {
        return false;
    }
    parent
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .any(|operand| operand.text_range() == node.text_range())
}

fn has_assignment_operator(node: &SyntaxNode) -> bool {
    node.children_with_tokens().any(|child| {
        matches!(
            child,
            NodeOrToken::Token(token)
                if matches!(
                    token.kind(),
                    SyntaxKind::EQ
                        | SyntaxKind::PLUSEQ
                        | SyntaxKind::MINUSEQ
                        | SyntaxKind::STAREQ
                        | SyntaxKind::SLASHEQ
                        | SyntaxKind::PERCENTEQ
                        | SyntaxKind::SHLEQ
                        | SyntaxKind::SHREQ
                        | SyntaxKind::AMPEQ
                        | SyntaxKind::PIPEEQ
                        | SyntaxKind::CARETEQ
                )
        )
    })
}

fn has_compound_assignment_operator(node: &SyntaxNode) -> bool {
    node.children_with_tokens().any(|child| {
        matches!(
            child,
            NodeOrToken::Token(token)
                if matches!(
                    token.kind(),
                    SyntaxKind::PLUSEQ
                        | SyntaxKind::MINUSEQ
                        | SyntaxKind::STAREQ
                        | SyntaxKind::SLASHEQ
                        | SyntaxKind::PERCENTEQ
                        | SyntaxKind::SHLEQ
                        | SyntaxKind::SHREQ
                        | SyntaxKind::AMPEQ
                        | SyntaxKind::PIPEEQ
                        | SyntaxKind::CARETEQ
                )
        )
    })
}

fn index_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if let Some(generic_type) = generic_index_expr_type(node, semantic) {
        return Some(generic_type);
    }
    let base = node.children().find(|child| is_expr_kind(child.kind()))?;
    expr_type(&base, semantic).and_then(|typ| indexed_element_type_for(node, &typ))
}

fn bin_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if has_assignment_operator(node) {
        return Some("()".into());
    }
    if has_any_direct_token(
        node,
        &[
            SyntaxKind::EQ2,
            SyntaxKind::NEQ,
            SyntaxKind::L_ANGLE,
            SyntaxKind::R_ANGLE,
            SyntaxKind::LTEQ,
            SyntaxKind::GTEQ,
            SyntaxKind::AMP2,
            SyntaxKind::PIPE2,
        ],
    ) {
        return Some("bool".into());
    }
    node.children()
        .filter(|child| is_expr_kind(child.kind()))
        .find_map(|child| expr_type(&child, semantic))
}

fn match_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    direct_child(node, SyntaxKind::MATCH_ARM_LIST)?
        .children()
        .filter(|child| child.kind() == SyntaxKind::MATCH_ARM)
        .filter_map(|arm| {
            arm.children()
                .filter(|child| is_expr_kind(child.kind()))
                .last()
                .and_then(|expr| expr_type(&expr, semantic))
        })
        .next()
}

fn call_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let names = call_path_names(node);
    if let Some(ret) = trait_ufcs_call_type(node, semantic) {
        return Some(coerced_never_type(node, ret, semantic));
    }
    // Return type of a user-defined associated function, e.g. `Point::new()`.
    if let Some([type_name, method]) = names.as_deref()
        && let Some(ret) = user_assoc_fn_return_type(type_name, method, semantic)
    {
        return Some(coerced_never_type(node, ret, semantic));
    }
    if let Some([type_name, method]) = names.as_deref()
        && let Some(ret) = contextual_new_return_type(node, type_name, semantic)
        && method == "new"
    {
        return Some(coerced_never_type(node, ret, semantic));
    }
    if path_matches(names.as_deref(), &["Box", "new"])
        && let Some(arg_type) = call_argument_types(node, semantic).into_iter().next()
    {
        return Some(format!(
            "alloc::boxed::Box<{arg_type}, alloc::alloc::Global>"
        ));
    }
    if let Some([type_name, method]) = names.as_deref()
        && let Some(ret) = sysroot_assoc_fn_return_type(type_name, method, semantic)
    {
        return Some(coerced_never_type(node, ret.into(), semantic));
    }
    if let Some(ret) = tuple_struct_constructor_call_type(node, semantic) {
        return Some(coerced_never_type(node, ret, semantic));
    }
    if let Some(ret) = sysroot_constructor_call_type(node, semantic) {
        return Some(ret);
    }
    call_name(node).and_then(|name| {
        semantic
            .functions
            .get(&name)
            .and_then(|_| function_return_type(node, &name, semantic))
            .map(|typ| coerced_never_type(node, typ, semantic))
    })
}

fn sysroot_constructor_call_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if !semantic.sysroot {
        return None;
    }
    let names = call_path_names(node)?;
    match names.as_slice() {
        [name] if name == "Some" => {
            let arg_type = return_some_context_type(node, semantic)
                .or_else(|| call_argument_types(node, semantic).into_iter().next())?;
            Some(format!("core::option::Option<{arg_type}>"))
        }
        [name] if name == "Reverse" => {
            if let Some(expected) = binary_heap_push_argument_type(node, semantic)
                .or_else(|| method_argument_context_type(node, semantic))
                && is_reverse_type(&expected)
            {
                return Some(expected);
            }
            let arg_type = call_argument_types(node, semantic).into_iter().next()?;
            Some(format!("core::cmp::Reverse<{arg_type}>"))
        }
        _ => None,
    }
}

fn sysroot_constructor_path_type(
    path_expr: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    if !semantic.sysroot {
        return None;
    }
    let call = path_expr
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::CALL_EXPR)?;
    let names = path_name_refs(path_expr)?;
    match names.as_slice() {
        [name] if name == "Some" => {
            let arg_type = return_some_context_type(&call, semantic)
                .or_else(|| call_argument_types(&call, semantic).into_iter().next())?;
            Some(format!(
                "fn({arg_type}) -> core::option::Option<{arg_type}>"
            ))
        }
        [name] if name == "Reverse" => {
            if let Some(expected) = binary_heap_push_argument_type(&call, semantic)
                .or_else(|| method_argument_context_type(&call, semantic))
                && let Some(arg_type) = reverse_inner_type(&expected)
            {
                return Some(format!("fn({arg_type}) -> {expected}"));
            }
            let arg_type = call_argument_types(&call, semantic).into_iter().next()?;
            Some(format!("fn({arg_type}) -> core::cmp::Reverse<{arg_type}>"))
        }
        _ => None,
    }
}

fn return_some_context_type(call: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let return_expr = call
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::RETURN_EXPR)?;
    let value = first_expr_child(&return_expr)?;
    if value.text_range() != call.text_range() {
        return None;
    }
    enclosing_function_return_type(call, semantic).and_then(|typ| option_inner_type(&typ))
}

fn is_reverse_type(typ: &str) -> bool {
    typ.trim_start().starts_with("core::cmp::Reverse<")
}

fn reverse_inner_type(typ: &str) -> Option<String> {
    typ.trim_start()
        .strip_prefix("core::cmp::Reverse<")?
        .strip_suffix('>')
        .map(str::to_string)
}

fn binary_heap_push_argument_type(arg: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let arg_list = arg
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::ARG_LIST)?;
    let call = arg_list
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::METHOD_CALL_EXPR)?;
    if direct_child(&call, SyntaxKind::NAME_REF)?.text() != "push" {
        return None;
    }
    let first_arg = arg_list
        .children()
        .find(|child| is_expr_kind(child.kind()))?;
    if first_arg.text_range() != arg.text_range() {
        return None;
    }
    let receiver = first_expr_child(&call)?;
    let receiver_type = expr_type(&receiver, semantic).or_else(|| {
        path_expr_name(&receiver)
            .and_then(|name| semantic.resolve_var(&receiver, &name))
            .map(|typ| adjusted_expr_type(&receiver, typ, semantic))
    })?;
    binary_heap_receiver_element_type(&receiver_type)
}

fn returned_some_argument_context_type(
    node: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    let arg_list = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::ARG_LIST)?;
    if !arg_list
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .any(|arg| range_contains(arg.text_range(), node.text_range()))
    {
        return None;
    }
    let call = arg_list
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::CALL_EXPR)?;
    if !path_matches(call_path_names(&call).as_deref(), &["Some"]) {
        return None;
    }
    return_some_context_type(&call, semantic)
}

fn call_argument_types(node: &SyntaxNode, semantic: &SemanticModel) -> Vec<String> {
    direct_child(node, SyntaxKind::ARG_LIST)
        .map(|arg_list| {
            arg_list
                .children()
                .filter(|child| is_expr_kind(child.kind()))
                .filter_map(|arg| expr_type(&arg, semantic))
                .collect()
        })
        .unwrap_or_default()
}

fn tuple_struct_constructor_call_type(
    node: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    let callee = first_expr_child(node)?;
    let info = tuple_struct_info_for_path(&callee, semantic)?;
    Some(info.full_name.clone())
}

fn tuple_struct_constructor_signature(
    path_expr: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    path_expr
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::CALL_EXPR)?;
    let info = tuple_struct_info_for_path(path_expr, semantic)?;
    Some(format!(
        "fn({}) -> {}",
        info.tuple_fields.join(", "),
        info.full_name
    ))
}

fn enum_variant_constructor_signature(
    path_expr: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    let call = path_expr
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::CALL_EXPR)?;
    let names = path_name_refs(path_expr)?;
    let [type_name, variant] = names.as_slice() else {
        return None;
    };
    let qualified = semantic.qualify_type_name(type_name);
    let Some(info) = semantic
        .structs
        .get(&qualified)
        .or_else(|| semantic.structs.get(type_name))
    else {
        return None;
    };
    if let Some(fields) = info.variant_tuple_fields.get(variant) {
        return Some(format!("fn({}) -> {qualified}", fields.join(", ")));
    }
    let args = call_argument_types(&call, semantic);
    Some(format!("fn({}) -> {qualified}", args.join(", ")))
}

fn enum_variant_value_type(path_expr: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let names = path_name_refs(path_expr)?;
    let [type_name, _variant] = names.as_slice() else {
        return None;
    };
    let qualified = semantic.qualify_type_name(type_name);
    (semantic.structs.contains_key(&qualified) || semantic.structs.contains_key(type_name))
        .then_some(qualified)
}

fn enum_variant_name_ref_signature(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    node.ancestors()
        .filter(|ancestor| ancestor.kind() == SyntaxKind::PATH_EXPR)
        .find_map(|path_expr| {
            let last = path_expr
                .descendants()
                .filter(|child| child.kind() == SyntaxKind::NAME_REF)
                .last()?;
            (last.text_range() == node.text_range())
                .then(|| enum_variant_constructor_signature(&path_expr, semantic))
                .flatten()
        })
}

fn enum_variant_name_ref_value_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    node.ancestors()
        .filter(|ancestor| ancestor.kind() == SyntaxKind::PATH_EXPR)
        .find_map(|path_expr| {
            if !range_contains(path_expr.text_range(), node.text_range()) {
                return None;
            }
            enum_variant_value_type(&path_expr, semantic)
        })
}

fn tuple_struct_info_for_path<'a>(
    path_expr: &SyntaxNode,
    semantic: &'a SemanticModel,
) -> Option<&'a StructInfo> {
    if path_expr.kind() != SyntaxKind::PATH_EXPR {
        return None;
    }
    let names = path_name_refs(path_expr)?;
    let name = names.last()?;
    let qualified = if names.len() > 1 {
        names.join("::")
    } else {
        semantic.qualify_type_name(name)
    };
    semantic
        .structs
        .get(&qualified)
        .or_else(|| semantic.structs.get(name))
        .filter(|info| !info.tuple_fields.is_empty())
}

fn coerced_never_type(node: &SyntaxNode, typ: String, semantic: &SemanticModel) -> String {
    if typ != "!" {
        return typ;
    }
    match_arm_context_type(node, semantic).unwrap_or(typ)
}

fn match_arm_context_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let arm = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::MATCH_ARM)?;
    let body = arm
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .last()?;
    if body.text_range() != node.text_range() {
        return None;
    }
    let match_expr = arm
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::MATCH_EXPR)?;
    direct_child(&match_expr, SyntaxKind::MATCH_ARM_LIST)?
        .children()
        .filter(|child| child.kind() == SyntaxKind::MATCH_ARM)
        .filter_map(|other_arm| {
            other_arm
                .children()
                .filter(|child| is_expr_kind(child.kind()))
                .last()
                .filter(|expr| expr.text_range() != node.text_range())
                .and_then(|expr| expr_type(&expr, semantic))
                .filter(|typ| typ != "!")
        })
        .next()
        .or_else(|| enclosing_function_return_type(&match_expr, semantic))
}

fn enclosing_function_return_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    node.ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::FN)
        .and_then(|function| direct_child(&function, SyntaxKind::RET_TYPE))
        .and_then(|ret| function_ret_type_text(&ret, semantic))
}

fn function_ret_type_text(ret: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    type_node(ret, semantic).map(|typ| declared_type_defaults(typ, semantic))
}

fn associated_function_path_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let names = path_name_refs(node)?;
    let [type_name, method] = names.as_slice() else {
        return trait_ufcs_function_signature(node, semantic);
    };
    trait_ufcs_function_signature(node, semantic)
        .or_else(|| contextual_new_signature(node, type_name, method, semantic))
        .or_else(|| contextual_box_new_signature(node, type_name, method, semantic))
        .or_else(|| associated_function_signature(type_name, method, semantic))
}

fn associated_function_name_ref_type(
    node: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    let path_expr = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_EXPR)?;
    if path_expr
        .parent()
        .is_none_or(|parent| parent.kind() != SyntaxKind::CALL_EXPR)
    {
        return None;
    }
    let names = path_name_refs(&path_expr)?;
    let [type_name, method] = names.as_slice() else {
        return None;
    };
    let name_refs = path_expr
        .descendants()
        .filter(|child| child.kind() == SyntaxKind::NAME_REF)
        .collect::<Vec<_>>();
    let index = name_refs
        .iter()
        .position(|name_ref| name_ref.text_range() == node.text_range())?;
    let signature = contextual_new_signature(&path_expr, type_name, method, semantic)
        .or_else(|| contextual_box_new_signature(&path_expr, type_name, method, semantic))
        .or_else(|| associated_function_signature(type_name, method, semantic));
    match index {
        0 => Some(semantic.qualify_type_name(type_name)),
        1 => signature,
        _ => None,
    }
}

fn trait_ufcs_name_ref_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let path_expr = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::PATH_EXPR)?;
    if path_expr
        .parent()
        .is_none_or(|parent| parent.kind() != SyntaxKind::CALL_EXPR)
    {
        return None;
    }
    let name_refs = path_expr
        .descendants()
        .filter(|child| child.kind() == SyntaxKind::NAME_REF)
        .collect::<Vec<_>>();
    if name_refs
        .last()
        .is_none_or(|last| last.text_range() != node.text_range())
    {
        return None;
    }
    trait_ufcs_function_signature(&path_expr, semantic)
}

fn trait_ufcs_call_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let method = trait_ufcs_parts_for_call(node, semantic)?;
    trait_method_return_type(&method.trait_name, &method.method_name, semantic)
}

fn trait_ufcs_function_signature(
    path_expr: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    let call = path_expr
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::CALL_EXPR)?;
    let method = trait_ufcs_parts_for_call(&call, semantic)?;
    let params = call_argument_types(&call, semantic);
    let ret = trait_method_return_type(&method.trait_name, &method.method_name, semantic)
        .unwrap_or_else(|| "()".into());
    Some(format!("fn({}) -> {ret}", params.join(", ")))
}

fn trait_ufcs_method_full_name(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let method = trait_ufcs_parts_for_call(node, semantic)?;
    let info = trait_info(&method.trait_name, semantic)?;
    info.methods
        .contains_key(&method.method_name)
        .then(|| format!("{}::{}", info.full_name, method.method_name))
}

struct TraitMethodPath {
    trait_name: String,
    method_name: String,
}

fn trait_ufcs_parts_for_call(
    call: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<TraitMethodPath> {
    let names = call_path_names(call)?;
    let method_name = names.last()?.clone();
    let trait_name = match names.as_slice() {
        [trait_name, _] if trait_info(trait_name, semantic).is_some() => trait_name.clone(),
        [.., trait_name, _] if trait_info(trait_name, semantic).is_some() => trait_name.clone(),
        _ => return None,
    };
    Some(TraitMethodPath {
        trait_name,
        method_name,
    })
}

fn trait_info<'a>(trait_name: &str, semantic: &'a SemanticModel) -> Option<&'a TraitInfo> {
    semantic.traits.get(trait_name).or_else(|| {
        semantic
            .traits
            .get(&semantic.qualify_value_name(trait_name))
    })
}

fn trait_method_return_type(
    trait_name: &str,
    method_name: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    let ret = trait_info(trait_name, semantic)?
        .methods
        .get(method_name)?
        .clone()?;
    Some(if ret == "Self" {
        trait_name.to_string()
    } else {
        ret
    })
}

fn contextual_new_signature(
    path_expr: &SyntaxNode,
    type_name: &str,
    method: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    if !semantic.sysroot || method != "new" {
        return None;
    }
    let ret = contextual_new_return_type(path_expr, type_name, semantic)?;
    Some(format!("fn() -> {ret}"))
}

fn contextual_box_new_signature(
    path_expr: &SyntaxNode,
    type_name: &str,
    method: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    if !semantic.sysroot || type_name != "Box" || method != "new" {
        return None;
    }
    let call = path_expr
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::CALL_EXPR)?;
    if let Some(arg_type) = call_argument_types(&call, semantic).into_iter().next() {
        return Some(format!(
            "fn({arg_type}) -> alloc::boxed::Box<{arg_type}, alloc::alloc::Global>"
        ));
    }
    let ret = function_argument_context_type(&call, semantic)?;
    let arg_type = box_inner_type(&ret)?;
    Some(format!("fn({arg_type}) -> {ret}"))
}

fn contextual_new_return_type(
    node: &SyntaxNode,
    type_name: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    let call = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::CALL_EXPR)?;
    let let_stmt = call
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::LET_STMT)?;
    let initializer = initializer_expr(&let_stmt)?;
    if initializer.text_range() != call.text_range() {
        return None;
    }
    let typ = type_node(&let_stmt, semantic)?;
    match type_name {
        "HashMap" => Some(expand_root_hashmap_defaults(&typ)),
        "VecDeque" => Some(expand_root_vecdeque_defaults(&typ)),
        _ => None,
    }
}

fn associated_function_signature(
    type_name: &str,
    method: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    user_assoc_fn_return_type(type_name, method, semantic)
        .map(|ret| format!("fn() -> {ret}"))
        .or_else(|| sysroot_assoc_fn_signature(type_name, method, semantic).map(str::to_string))
}

fn sysroot_assoc_fn_return_type(
    type_name: &str,
    method: &str,
    semantic: &SemanticModel,
) -> Option<&'static str> {
    if !semantic.sysroot {
        return None;
    }
    match (type_name, method) {
        ("String", "from" | "new" | "with_capacity") => Some("alloc::string::String"),
        _ => None,
    }
}

fn sysroot_assoc_fn_signature(
    type_name: &str,
    method: &str,
    semantic: &SemanticModel,
) -> Option<&'static str> {
    if !semantic.sysroot {
        return None;
    }
    match (type_name, method) {
        ("String", "from") => Some("fn(&str) -> alloc::string::String"),
        ("String", "new") => Some("fn() -> alloc::string::String"),
        ("String", "with_capacity") => Some("fn(usize) -> alloc::string::String"),
        ("Box", "new") => Some("fn(T) -> alloc::boxed::Box<T, alloc::alloc::Global>"),
        _ => None,
    }
}

fn function_return_type(
    root_call: &SyntaxNode,
    name: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    function_decl(root_call, name)
        .and_then(|node| direct_child(&node, SyntaxKind::RET_TYPE))
        .and_then(|ret| function_ret_type_text(&ret, semantic))
}

fn function_path_type(
    path_expr: &SyntaxNode,
    name: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    if path_expr
        .parent()
        .is_none_or(|parent| parent.kind() != SyntaxKind::CALL_EXPR)
        || !semantic.functions.contains_key(name)
    {
        return None;
    }
    function_signature(path_expr, name, semantic)
}

fn function_signature(
    use_site: &SyntaxNode,
    name: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    let decl = function_decl(use_site, name)?;
    let params = direct_child(&decl, SyntaxKind::PARAM_LIST)
        .map(|param_list| {
            param_list
                .children()
                .filter(|child| child.kind() == SyntaxKind::PARAM)
                .filter_map(|param| {
                    type_node(&param, semantic).map(|typ| declared_type_defaults(typ, semantic))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let params = specialize_type_generic_param_types(params, use_site, &decl, semantic);
    let params = specialize_const_generic_param_types(params, use_site, &decl, semantic);
    let ret = direct_child(&decl, SyntaxKind::RET_TYPE)
        .and_then(|ret| function_ret_type_text(&ret, semantic))
        .unwrap_or_else(|| "()".into());
    Some(format!("fn({}) -> {ret}", params.join(", ")))
}

fn specialize_type_generic_param_types(
    params: Vec<String>,
    use_site: &SyntaxNode,
    decl: &SyntaxNode,
    semantic: &SemanticModel,
) -> Vec<String> {
    let type_names = type_param_names(decl);
    if type_names.is_empty() {
        return params;
    }
    let Some(call) = use_site
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::CALL_EXPR)
    else {
        return params;
    };
    let args = direct_child(&call, SyntaxKind::ARG_LIST)
        .map(|arg_list| {
            arg_list
                .children()
                .filter(|child| is_expr_kind(child.kind()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut replacements = HashMap::new();
    for (param, arg) in params.iter().zip(args.iter()) {
        let Some(arg_type) = expr_type(arg, semantic) else {
            continue;
        };
        for name in &type_names {
            if param == name {
                replacements
                    .entry(name.clone())
                    .or_insert_with(|| arg_type.clone());
            }
        }
    }
    if replacements.is_empty() {
        return params;
    }
    params
        .into_iter()
        .map(|param| substitute_const_params(&param, &replacements))
        .collect()
}

fn type_param_names(decl: &SyntaxNode) -> Vec<String> {
    direct_child(decl, SyntaxKind::GENERIC_PARAM_LIST)
        .map(|generic_params| {
            generic_params
                .children()
                .filter(|child| child.kind() == SyntaxKind::TYPE_PARAM)
                .filter_map(|param| name_child_text(&param))
                .collect()
        })
        .unwrap_or_default()
}

fn specialize_const_generic_param_types(
    params: Vec<String>,
    use_site: &SyntaxNode,
    decl: &SyntaxNode,
    semantic: &SemanticModel,
) -> Vec<String> {
    let const_names = const_param_names(decl);
    if const_names.is_empty() {
        return params;
    }
    let Some(call) = use_site
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::CALL_EXPR)
    else {
        return params;
    };
    let arg_types = call_argument_types(&call, semantic);
    let mut replacements = HashMap::new();
    for (param, arg) in params.iter().zip(arg_types.iter()) {
        infer_array_const_param_replacements(param, arg, &const_names, &mut replacements);
    }
    if replacements.is_empty() {
        return params;
    }
    params
        .into_iter()
        .map(|param| substitute_const_params(&param, &replacements))
        .collect()
}

fn const_param_names(decl: &SyntaxNode) -> Vec<String> {
    direct_child(decl, SyntaxKind::GENERIC_PARAM_LIST)
        .map(|params| {
            params
                .children()
                .filter(|param| param.kind() == SyntaxKind::CONST_PARAM)
                .filter_map(|param| name_child_text(&param))
                .collect()
        })
        .unwrap_or_default()
}

fn generic_param_names(decl: &SyntaxNode) -> Vec<String> {
    direct_child(decl, SyntaxKind::GENERIC_PARAM_LIST)
        .map(|params| {
            params
                .children()
                .filter_map(|param| name_child_text(&param))
                .collect()
        })
        .unwrap_or_default()
}

fn infer_array_const_param_replacements(
    param: &str,
    arg: &str,
    const_names: &[String],
    replacements: &mut HashMap<String, String>,
) {
    let Some((param_element, param_len)) = fixed_array_type_parts(param) else {
        return;
    };
    let Some((arg_element, arg_len)) = fixed_array_type_parts(arg) else {
        return;
    };
    if const_names.iter().any(|name| name == param_len) {
        replacements
            .entry(param_len.to_string())
            .or_insert_with(|| arg_len.to_string());
    }
    infer_array_const_param_replacements(param_element, arg_element, const_names, replacements);
}

fn substitute_const_params(text: &str, replacements: &HashMap<String, String>) -> String {
    replacements
        .iter()
        .fold(text.to_string(), |current, (name, value)| {
            replace_identifier(&current, name, value)
        })
}

fn replace_identifier(text: &str, name: &str, value: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(relative) = text[cursor..].find(name) {
        let start = cursor + relative;
        let end = start + name.len();
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        out.push_str(&text[cursor..start]);
        if is_identifier_boundary(before) && is_identifier_boundary(after) {
            out.push_str(value);
        } else {
            out.push_str(&text[start..end]);
        }
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    out
}

fn is_identifier_boundary(ch: Option<char>) -> bool {
    !matches!(ch, Some(ch) if ch.is_alphanumeric() || ch == '_')
}

fn function_decl(use_site: &SyntaxNode, name: &str) -> Option<SyntaxNode> {
    let root = use_site.ancestors().last()?;
    root.descendants().find(|node| {
        node.kind() == SyntaxKind::FN && name_child_text(node).as_deref() == Some(name)
    })
}

fn callable_method_full_name(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    match node.kind() {
        SyntaxKind::CALL_EXPR => call_method_full_name(node, semantic),
        SyntaxKind::METHOD_CALL_EXPR => method_call_method_full_name(node, semantic),
        _ => None,
    }
}

fn call_method_full_name(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let names = call_path_names(node);
    if let Some(full_name) = trait_ufcs_method_full_name(node, semantic) {
        return Some(full_name);
    }
    // User-defined associated function, e.g. `Point::new(..)`.
    if let Some([type_name, method]) = names.as_deref()
        && let Some(full_name) = user_assoc_fn_full_name(type_name, method, semantic)
    {
        return Some(full_name);
    }
    if let Some(full_name) = tuple_struct_constructor_method_full_name(node, semantic) {
        return Some(full_name);
    }
    if let Some([type_name, variant]) = names.as_deref() {
        let qualified = semantic.qualify_type_name(type_name);
        if semantic.structs.contains_key(&qualified) || semantic.structs.contains_key(type_name) {
            return Some(format!("{qualified}::{variant}"));
        }
    }
    if semantic.sysroot {
        if path_matches(names.as_deref(), &["String", "from"]) {
            return Some("core::convert::From<T>::from".into());
        }
        if path_matches(names.as_deref(), &["String", "new"]) {
            return Some("alloc::string::String::new".into());
        }
        if path_matches(names.as_deref(), &["String", "with_capacity"]) {
            return Some("alloc::string::String::with_capacity".into());
        }
        if path_matches(names.as_deref(), &["Vec", "with_capacity"]) {
            return Some("alloc::vec::Vec<T, alloc::alloc::Global>::with_capacity".into());
        }
        if path_matches(names.as_deref(), &["HashMap", "new"]) {
            return Some(
                "std::collections::hash::map::HashMap<K, V, std::hash::random::RandomState, alloc::alloc::Global>::new"
                    .into(),
            );
        }
        if path_matches(names.as_deref(), &["Option", "Some"]) {
            return Some("core::option::Option::Some".into());
        }
        if path_matches(names.as_deref(), &["Some"]) {
            return Some("core::option::Option<T>::Some".into());
        }
        if path_matches(names.as_deref(), &["Reverse"]) {
            return Some("core::cmp::Reverse<T>".into());
        }
        if path_matches(names.as_deref(), &["Option", "None"]) {
            return Some("core::option::Option::None".into());
        }
        if path_matches(names.as_deref(), &["Result", "Ok"]) {
            return Some("core::result::Result::Ok".into());
        }
        if path_matches(names.as_deref(), &["Result", "Err"]) {
            return Some("core::result::Result::Err".into());
        }
    }
    call_name(node).and_then(|name| function_method_full_name(node, &name, semantic))
}

fn tuple_struct_constructor_method_full_name(
    node: &SyntaxNode,
    semantic: &SemanticModel,
) -> Option<String> {
    let callee = first_expr_child(node)?;
    let info = tuple_struct_info_for_path(&callee, semantic)?;
    Some(info.full_name.clone())
}

fn function_method_full_name(
    use_site: &SyntaxNode,
    name: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    let full_name = semantic.functions.get(name)?;
    let generic_params = function_decl(use_site, name)
        .map(|decl| generic_param_names(&decl))
        .unwrap_or_default();
    if generic_params.is_empty() {
        Some(full_name.clone())
    } else {
        Some(format!("{full_name}<{}>", generic_params.join(", ")))
    }
}

/// `methodFullName` for a call to an associated function of a user-defined
/// type, e.g. `Point::new` -> `crate::Point::new`.
fn user_assoc_fn_full_name(
    type_name: &str,
    method: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    let qualified = semantic.qualify_type_name(type_name);
    let info = semantic.impls.get(&qualified)?;
    info.methods
        .contains_key(method)
        .then(|| format!("{qualified}::{method}"))
}

/// Declared return type of a user-defined associated function, with `Self`
/// resolved to the owning type's full name.
fn user_assoc_fn_return_type(
    type_name: &str,
    method: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    let qualified = semantic.qualify_type_name(type_name);
    let ret = semantic
        .impls
        .get(&qualified)?
        .methods
        .get(method)?
        .clone()?;
    Some(if ret == "Self" { qualified } else { ret })
}

fn call_name(node: &SyntaxNode) -> Option<String> {
    node.children()
        .find(|child| is_expr_kind(child.kind()))
        .and_then(|expr| path_expr_name(&expr))
}

fn call_path_names(node: &SyntaxNode) -> Option<Vec<String>> {
    node.children()
        .find(|child| is_expr_kind(child.kind()))
        .and_then(|expr| path_name_refs(&expr))
}

fn path_name_refs(path_expr: &SyntaxNode) -> Option<Vec<String>> {
    if path_expr.kind() != SyntaxKind::PATH_EXPR {
        return None;
    }
    Some(
        path_expr
            .descendants()
            .filter(|child| child.kind() == SyntaxKind::NAME_REF)
            .map(|node| node.text().to_string())
            .collect(),
    )
}

fn path_matches(actual: Option<&[String]>, expected: &[&str]) -> bool {
    actual.is_some_and(|actual| {
        actual.len() == expected.len()
            && actual
                .iter()
                .zip(expected.iter())
                .all(|(actual, expected)| actual == expected)
    })
}

fn method_call_method_full_name(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let method_name = direct_child(node, SyntaxKind::NAME_REF)?.text().to_string();
    let receiver = first_expr_child(node)?;
    let receiver_type = expr_type(&receiver, semantic);
    // User-defined methods resolve regardless of sysroot.
    if let Some(typ) = receiver_type.as_deref()
        && let Some(full_name) = user_method_full_name(typ, &method_name, semantic)
    {
        return Some(full_name);
    }
    if !semantic.sysroot {
        return None;
    }
    if is_slice_method(&method_name)
        && receiver_type
            .as_deref()
            .is_some_and(is_slice_or_array_receiver_type)
    {
        return Some(format!("[T]::{method_name}"));
    }
    if let Some(typ) = receiver_type.as_deref()
        && let Some(full_name) = std_collection_method_full_name(typ, &method_name)
    {
        return Some(full_name);
    }
    match (method_name.as_str(), receiver_type.as_deref()) {
        ("push", Some(typ)) if typ.starts_with("alloc::vec::Vec<") => {
            Some("alloc::vec::Vec<T, A>::push".into())
        }
        ("len", Some(typ)) if typ.starts_with("alloc::vec::Vec<") => {
            Some("alloc::vec::Vec<T, A>::len".into())
        }
        ("is_empty", Some(typ)) if typ.starts_with("alloc::vec::Vec<") => {
            Some("alloc::vec::Vec<T, A>::is_empty".into())
        }
        ("into_iter", Some(typ)) if typ.starts_with("alloc::vec::Vec<") => Some(
            "<alloc::vec::Vec<T, A> as core::iter::traits::collect::IntoIterator>::into_iter"
                .into(),
        ),
        ("iter", Some(typ)) if typ.starts_with("&[") => Some("[T]::iter".into()),
        ("len", Some(typ)) if typ.starts_with("&[") => Some("[T]::len".into()),
        ("to_vec", Some(typ)) if typ.starts_with("&[") => Some("[T]::to_vec".into()),
        ("trim", Some(typ)) if is_string_like(typ) => Some("str::trim".into()),
        ("chars", Some(typ)) if is_string_like(typ) => Some("str::chars".into()),
        ("lines", Some(typ)) if is_string_like(typ) => Some("str::lines".into()),
        ("contains", Some(typ)) if is_string_like(typ) => Some("str::contains<P>".into()),
        ("split", Some(typ)) if is_string_like(typ) => Some("str::split<P>".into()),
        ("split_once", Some(typ)) if is_string_like(typ) => Some("str::split_once<P>".into()),
        ("split_whitespace", Some(typ)) if is_string_like(typ) => {
            Some("str::split_whitespace".into())
        }
        ("parse", Some(typ)) if is_string_like(typ) => Some("str::parse<F>".into()),
        ("is_alphanumeric", Some(typ)) if receiver_base_type(typ) == "char" => {
            Some("char::is_alphanumeric".into())
        }
        ("to_ascii_lowercase", Some(typ)) if receiver_base_type(typ) == "char" => {
            Some("char::to_ascii_lowercase".into())
        }
        ("to_ascii_uppercase", Some(typ)) if receiver_base_type(typ) == "char" => {
            Some("char::to_ascii_uppercase".into())
        }
        ("ok", Some(typ)) if is_result_type(typ) => Some("core::result::Result<T, E>::ok".into()),
        ("unwrap", Some(typ)) if is_result_type(typ) => {
            Some("core::result::Result<T, E>::unwrap".into())
        }
        ("unwrap", Some(typ)) if is_option_type(typ) => {
            Some("core::option::Option<T>::unwrap".into())
        }
        ("next", Some(typ)) if is_peekable_type(typ) => Some(
            "<core::iter::adapters::peekable::Peekable<I> as core::iter::traits::iterator::Iterator>::next"
                .into(),
        ),
        ("peek", Some(typ)) if is_peekable_type(typ) => {
            Some("core::iter::adapters::peekable::Peekable<I>::peek".into())
        }
        ("is_empty", Some(typ)) if is_string_like(typ) => Some("str::is_empty".into()),
        ("starts_with", Some(typ)) if is_string_like(typ) => Some("str::starts_with<P>".into()),
        ("strip_prefix", Some(typ)) if is_string_like(typ) => Some("str::strip_prefix<P>".into()),
        ("strip_suffix", Some(typ)) if is_string_like(typ) => Some("str::strip_suffix<P>".into()),
        ("len", Some(typ)) if is_string_like(typ) => Some("str::len".into()),
        ("as_bytes", Some(typ)) if is_string_like(typ) => Some("str::as_bytes".into()),
        ("as_str", Some(typ)) if is_owned_string(typ) => {
            Some("alloc::string::String::as_str".into())
        }
        ("push", Some(typ)) if is_owned_string(typ) => Some("alloc::string::String::push".into()),
        ("push_str", Some(typ)) if is_owned_string(typ) => {
            Some("alloc::string::String::push_str".into())
        }
        ("to_string", Some(typ)) if is_string_like(typ) => {
            Some("<T as alloc::string::ToString>::to_string".into())
        }
        ("saturating_sub", Some(typ)) => Some(format!("{}::saturating_sub", unborrow_type(typ))),
        _ => None,
    }
}

fn std_collection_method_full_name(receiver_type: &str, method_name: &str) -> Option<String> {
    let base = receiver_base_type(receiver_type);
    if base.starts_with("std::collections::hash::map::HashMap<") {
        return match method_name {
            "get" => Some("std::collections::hash::map::HashMap<K, V, S, A>::get<Q>".into()),
            "values" => Some("std::collections::hash::map::HashMap<K, V, S, A>::values".into()),
            "entry" => Some("std::collections::hash::map::HashMap<K, V, S, A>::entry".into()),
            "insert" => Some("std::collections::hash::map::HashMap<K, V, S, A>::insert".into()),
            _ => None,
        };
    }
    if base.starts_with("std::collections::hash::set::HashSet<") {
        return match method_name {
            "insert" => Some("std::collections::hash::set::HashSet<T, S, A>::insert".into()),
            _ => None,
        };
    }
    if base.starts_with("alloc::collections::vec_deque::VecDeque<") {
        return match method_name {
            "iter" => Some("alloc::collections::vec_deque::VecDeque<T, A>::iter".into()),
            "len" => Some("alloc::collections::vec_deque::VecDeque<T, A>::len".into()),
            "push_back" => Some("alloc::collections::vec_deque::VecDeque<T, A>::push_back".into()),
            "push_front" => {
                Some("alloc::collections::vec_deque::VecDeque<T, A>::push_front".into())
            }
            "pop_front" => Some("alloc::collections::vec_deque::VecDeque<T, A>::pop_front".into()),
            "pop_back" => Some("alloc::collections::vec_deque::VecDeque<T, A>::pop_back".into()),
            "remove" => Some("alloc::collections::vec_deque::VecDeque<T, A>::remove".into()),
            "front" => Some("alloc::collections::vec_deque::VecDeque<T, A>::front".into()),
            "back" => Some("alloc::collections::vec_deque::VecDeque<T, A>::back".into()),
            _ => None,
        };
    }
    if base.starts_with("alloc::collections::binary_heap::BinaryHeap<") {
        return match method_name {
            "push" => Some("alloc::collections::binary_heap::BinaryHeap<T, A>::push".into()),
            "pop" => Some("alloc::collections::binary_heap::BinaryHeap<T, A>::pop".into()),
            "len" => Some("alloc::collections::binary_heap::BinaryHeap<T, A>::len".into()),
            _ => None,
        };
    }
    if base.starts_with("alloc::vec::Vec<") {
        return match method_name {
            "push" => Some("alloc::vec::Vec<T, A>::push".into()),
            "pop" => Some("alloc::vec::Vec<T, A>::pop".into()),
            "len" => Some("alloc::vec::Vec<T, A>::len".into()),
            "is_empty" => Some("alloc::vec::Vec<T, A>::is_empty".into()),
            _ => None,
        };
    }
    None
}

fn method_call_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let method_name = direct_child(node, SyntaxKind::NAME_REF)?.text().to_string();
    let receiver = first_expr_child(node)?;
    let receiver_type = expr_type(&receiver, semantic);
    // Return type of a user-defined method.
    if let Some(typ) = receiver_type.as_deref()
        && let Some(ret) = user_method_return_type(typ, &method_name, semantic)
    {
        return Some(ret);
    }
    if !semantic.sysroot {
        return None;
    }
    match (method_name.as_str(), receiver_type.as_deref()) {
        ("push" | "push_str" | "push_back" | "push_front" | "extend_from_slice", _) => {
            Some("()".into())
        }
        ("pop", Some(typ)) => {
            vec_receiver_element_type(typ).map(|inner| format!("core::option::Option<{inner}>"))
        }
        ("pop_front", Some(typ)) => vecdeque_receiver_element_type(typ)
            .map(|inner| format!("core::option::Option<{inner}>")),
        ("remove", Some(typ)) => vecdeque_receiver_element_type(typ)
            .map(|inner| format!("core::option::Option<{inner}>")),
        ("iter", Some(typ)) => vecdeque_receiver_element_type(typ)
            .map(|inner| format!("alloc::collections::vec_deque::iter::Iter<'a, {inner}>")),
        ("get", Some(typ)) if is_hashmap_type(typ) => {
            hashmap_value_type(typ).map(|value| format!("core::option::Option<&{value}>"))
        }
        ("into_iter", Some(typ)) if typ.starts_with("alloc::vec::Vec<") => {
            vec_receiver_element_type(typ).map(|element| {
                format!("alloc::vec::into_iter::IntoIter<{element}, alloc::alloc::Global>")
            })
        }
        ("join", Some(typ)) if typ.starts_with("&[") => Some("alloc::string::String".into()),
        ("trim", _) => Some("&str".into()),
        ("chars", Some(typ)) if is_string_like(typ) => Some("core::str::iter::Chars<'a>".into()),
        ("peekable", Some(typ)) => Some(format!("core::iter::adapters::peekable::Peekable<{typ}>")),
        ("next", Some(typ)) if is_peekable_type(typ) => {
            peekable_item_type(typ).map(|item| format!("core::option::Option<{item}>"))
        }
        ("peek", Some(typ)) if is_peekable_type(typ) => {
            peekable_item_type(typ).map(|item| format!("core::option::Option<&{item}>"))
        }
        ("entry", Some(typ)) if is_hashmap_type(typ) => {
            let key = hashmap_key_type(typ)?;
            let value = hashmap_value_type(typ)?;
            Some(format!(
                "std::collections::hash::map::Entry<'a, {key}, {value}, alloc::alloc::Global>"
            ))
        }
        ("or_default" | "or_insert", Some(typ)) => {
            hashmap_entry_value_type(typ).map(|value| format!("&mut {value}"))
        }
        ("lines", Some(typ)) if is_string_like(typ) => Some("core::str::iter::Lines<'a>".into()),
        ("split", Some(typ)) if is_string_like(typ) => {
            Some("core::str::iter::Split<'a, char>".into())
        }
        ("split_once", Some(typ)) if is_string_like(typ) => {
            Some("core::option::Option<(&str, &str)>".into())
        }
        ("split_whitespace", Some(typ)) if is_string_like(typ) => {
            Some("core::str::iter::SplitWhitespace<'a>".into())
        }
        ("is_empty", Some(typ)) if is_string_like(typ) => Some("bool".into()),
        ("contains", Some(typ)) if is_string_like(typ) => Some("bool".into()),
        ("starts_with", Some(typ)) if is_string_like(typ) => Some("bool".into()),
        ("strip_prefix" | "strip_suffix", Some(typ)) if is_string_like(typ) => {
            Some("core::option::Option<&str>".into())
        }
        ("is_alphanumeric", Some(typ)) if receiver_base_type(typ) == "char" => Some("bool".into()),
        ("to_ascii_lowercase" | "to_ascii_uppercase", Some(typ))
            if receiver_base_type(typ) == "char" =>
        {
            Some("char".into())
        }
        ("to_vec", Some(typ)) if typ.starts_with("&[") => slice_or_array_element_type(typ)
            .map(|element| format!("alloc::vec::Vec<{element}, alloc::alloc::Global>")),
        ("reverse" | "sort" | "sort_by" | "sort_by_key", Some(typ))
            if is_slice_or_array_receiver_type(typ) =>
        {
            Some("()".into())
        }
        ("last_mut", Some(typ)) if is_slice_or_array_receiver_type(typ) => {
            slice_or_array_element_type(typ)
                .map(|element| format!("core::option::Option<&mut {element}>"))
        }
        ("as_bytes", _) => Some("&[u8]".into()),
        ("as_str", Some(typ)) if is_owned_string(typ) => Some("&str".into()),
        ("to_string", _) => Some("alloc::string::String".into()),
        ("len", _) => Some("usize".into()),
        ("saturating_sub", Some(typ)) => Some(unborrow_type(typ)),
        _ => None,
    }
}

/// `methodFullName` for a method call whose receiver resolves to a user-defined
/// type recorded in an `impl` block.
fn user_method_full_name(
    receiver_type: &str,
    method_name: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    if let Some(trait_name) = dyn_trait_name(receiver_type)
        && let Some(info) = trait_info(&trait_name, semantic)
        && info.methods.contains_key(method_name)
    {
        return Some(format!("{}::{method_name}", info.full_name));
    }
    let self_type = receiver_base_type(receiver_type);
    let info = semantic.impls.get(self_type)?;
    info.methods
        .contains_key(method_name)
        .then(|| format!("{self_type}::{method_name}"))
}

fn user_method_return_type(
    receiver_type: &str,
    method_name: &str,
    semantic: &SemanticModel,
) -> Option<String> {
    if let Some(trait_name) = dyn_trait_name(receiver_type)
        && let Some(ret) = trait_method_return_type(&trait_name, method_name, semantic)
    {
        return Some(ret);
    }
    let self_type = receiver_base_type(receiver_type);
    let ret = semantic
        .impls
        .get(self_type)?
        .methods
        .get(method_name)?
        .clone()?;
    Some(if ret == "Self" {
        self_type.to_string()
    } else {
        ret
    })
}

fn boxed_dyn_trait_name(typ: &str) -> Option<String> {
    let base = receiver_base_type(typ);
    let inner = base
        .strip_prefix("alloc::boxed::Box<")
        .or_else(|| base.strip_prefix("Box<"))?
        .strip_suffix('>')?;
    dyn_trait_name(inner)
}

fn box_inner_type(typ: &str) -> Option<String> {
    let base = receiver_base_type(typ);
    let inner = base
        .strip_prefix("alloc::boxed::Box<")
        .or_else(|| base.strip_prefix("Box<"))?
        .strip_suffix('>')?;
    first_top_level_arg(inner)
}

fn dyn_trait_name(typ: &str) -> Option<String> {
    let base = receiver_base_type(typ);
    let dyn_type = base.strip_prefix("dyn ")?;
    let trait_name = dyn_type
        .split([',', '+'])
        .next()
        .map(str::trim)
        .filter(|name| !name.is_empty())?;
    Some(trait_name.to_string())
}

/// `&str`, `String`, or `alloc::string::String`, including auto-borrowed
/// owned strings.
fn is_string_like(typ: &str) -> bool {
    typ == "&str" || receiver_base_type(typ) == "str" || is_owned_string(typ)
}

fn is_owned_string(typ: &str) -> bool {
    let typ = receiver_base_type(typ);
    typ == "String" || typ == "alloc::string::String"
}

fn is_str_receiver_method(method_name: &str) -> bool {
    matches!(
        method_name,
        "trim"
            | "chars"
            | "lines"
            | "contains"
            | "split"
            | "split_once"
            | "split_whitespace"
            | "parse"
            | "len"
            | "is_empty"
            | "starts_with"
            | "strip_prefix"
            | "strip_suffix"
            | "as_bytes"
            | "to_string"
    )
}

fn is_hashmap_type(typ: &str) -> bool {
    receiver_base_type(typ).starts_with("std::collections::hash::map::HashMap<")
}

fn hashmap_value_type(typ: &str) -> Option<String> {
    let base = receiver_base_type(typ);
    let inner = base
        .strip_prefix("std::collections::hash::map::HashMap<")?
        .strip_suffix('>')?;
    inner
        .split(',')
        .map(str::trim)
        .nth(1)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn hashmap_key_type(typ: &str) -> Option<String> {
    let base = receiver_base_type(typ);
    let inner = base
        .strip_prefix("std::collections::hash::map::HashMap<")?
        .strip_suffix('>')?;
    nth_top_level_arg(inner, 0)
}

fn hashmap_entry_value_type(typ: &str) -> Option<String> {
    let base = receiver_base_type(typ);
    let inner = base
        .strip_prefix("std::collections::hash::map::Entry<")?
        .strip_suffix('>')?;
    nth_top_level_arg(inner, 2)
}

fn is_result_type(typ: &str) -> bool {
    let base = receiver_base_type(typ);
    base.starts_with("core::result::Result<") || base.starts_with("Result<")
}

fn is_option_type(typ: &str) -> bool {
    let base = receiver_base_type(typ);
    base.starts_with("core::option::Option<") || base.starts_with("Option<")
}

fn is_peekable_type(typ: &str) -> bool {
    receiver_base_type(typ).starts_with("core::iter::adapters::peekable::Peekable<")
}

fn is_slice_or_array_receiver_type(typ: &str) -> bool {
    receiver_base_type(typ).starts_with('[')
}

fn is_slice_method(method_name: &str) -> bool {
    matches!(
        method_name,
        "iter"
            | "len"
            | "is_empty"
            | "to_vec"
            | "reverse"
            | "sort"
            | "sort_by"
            | "sort_by_key"
            | "binary_search"
            | "last_mut"
    )
}

fn is_vec_slice_shared_method(method_name: &str) -> bool {
    matches!(method_name, "binary_search")
}

fn is_vec_slice_mutating_method(method_name: &str) -> bool {
    matches!(
        method_name,
        "reverse" | "sort" | "sort_by" | "sort_by_key" | "last_mut"
    )
}

fn vec_slice_type(typ: &str) -> Option<String> {
    vec_receiver_element_type(typ).map(|element| format!("&[{element}]"))
}

fn vec_mut_slice_type(typ: &str) -> Option<String> {
    vec_receiver_element_type(typ).map(|element| format!("&mut [{element}]"))
}

fn peekable_item_type(typ: &str) -> Option<String> {
    let base = receiver_base_type(typ);
    let inner = base
        .strip_prefix("core::iter::adapters::peekable::Peekable<")?
        .strip_suffix('>')?;
    if inner.starts_with("core::str::iter::Chars<") {
        Some("char".into())
    } else {
        None
    }
}

fn vec_receiver_element_type(typ: &str) -> Option<String> {
    let typ = receiver_base_type(typ);
    let inner = typ
        .strip_prefix("alloc::vec::Vec<")
        .or_else(|| typ.strip_prefix("Vec<"))?
        .strip_suffix('>')?;
    first_top_level_arg(inner)
}

fn vecdeque_receiver_element_type(typ: &str) -> Option<String> {
    let typ = receiver_base_type(typ);
    let inner = typ
        .strip_prefix("alloc::collections::vec_deque::VecDeque<")
        .or_else(|| typ.strip_prefix("VecDeque<"))?
        .strip_suffix('>')?;
    first_top_level_arg(inner)
}

fn binary_heap_receiver_element_type(typ: &str) -> Option<String> {
    let typ = receiver_base_type(typ);
    let inner = typ
        .strip_prefix("alloc::collections::binary_heap::BinaryHeap<")
        .or_else(|| typ.strip_prefix("BinaryHeap<"))?
        .strip_suffix('>')?;
    first_top_level_arg(inner)
}

fn first_top_level_arg(input: &str) -> Option<String> {
    nth_top_level_arg(input, 0)
}

fn nth_top_level_arg(input: &str, target: usize) -> Option<String> {
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut index = 0usize;
    for (idx, ch) in input.char_indices() {
        match ch {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                if index == target {
                    let arg = input[start..idx].trim();
                    return (!arg.is_empty()).then(|| arg.to_string());
                }
                index += 1;
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    let trimmed = input[start..].trim();
    if index != target {
        return None;
    }
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn receiver_base_type(typ: &str) -> &str {
    let mut typ = typ.trim_start();
    while let Some(rest) = typ.strip_prefix('&') {
        typ = rest.trim_start();
        typ = typ.strip_prefix("mut ").unwrap_or(typ).trim_start();
    }
    typ
}

fn macro_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let text = node.text().to_string();
    if semantic.sysroot && text.starts_with("vec![") {
        if let Some(inner_vec) = nested_vec_macro_type(&text) {
            return Some(inner_vec);
        }
        Some(format!(
            "alloc::vec::Vec<{}, alloc::alloc::Global>",
            vec_macro_element_type(&text).unwrap_or("i32")
        ))
    } else if semantic.sysroot && text.starts_with("format!(") {
        Some("alloc::string::String".into())
    } else if text.starts_with("panic!(") {
        Some("!".into())
    } else {
        None
    }
}

fn nested_vec_macro_type(text: &str) -> Option<String> {
    let inner = text.strip_prefix("vec![")?.strip_suffix(']')?.trim();
    if !inner.starts_with("vec![") {
        return None;
    }
    let end = inner.find("];").map(|idx| idx + 1)?;
    let nested = &inner[..end];
    let element = vec_macro_element_type(nested).unwrap_or("i32");
    Some(format!(
        "alloc::vec::Vec<alloc::vec::Vec<{element}, alloc::alloc::Global>, alloc::alloc::Global>"
    ))
}

fn vec_macro_element_type(text: &str) -> Option<&'static str> {
    let inner = text.strip_prefix("vec![")?.strip_suffix(']')?.trim();
    let first = inner
        .split([';', ','])
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    if matches!(first, "true" | "false") {
        Some("bool")
    } else if first.starts_with('"') {
        Some("&str")
    } else if first.starts_with('\'') {
        Some("char")
    } else if first.starts_with("usize::") || first.ends_with("usize") {
        Some("usize")
    } else if first.starts_with("u64::") || first.ends_with("u64") {
        Some("u64")
    } else if first.starts_with("u32::") || first.ends_with("u32") {
        Some("u32")
    } else if first.starts_with("i64::") || first.ends_with("i64") {
        Some("i64")
    } else if first.starts_with("i32::") || first.ends_with("i32") {
        Some("i32")
    } else {
        None
    }
}

fn is_array_repeat_count(node: &SyntaxNode) -> bool {
    let Some(array) = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::ARRAY_EXPR)
    else {
        return false;
    };
    if !has_direct_token(&array, SyntaxKind::SEMICOLON) {
        return false;
    }
    array
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .nth(1)
        .is_some_and(|count| range_contains(count.text_range(), node.text_range()))
}

fn is_index_operand(node: &SyntaxNode) -> bool {
    let Some(index) = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::INDEX_EXPR)
    else {
        return false;
    };
    index
        .children()
        .filter(|child| is_expr_kind(child.kind()))
        .nth(1)
        .is_some_and(|operand| range_contains(operand.text_range(), node.text_range()))
}

fn is_array_type_const_arg_literal(node: &SyntaxNode) -> bool {
    node.ancestors()
        .skip(1)
        .any(|ancestor| ancestor.kind() == SyntaxKind::ARRAY_TYPE)
}

fn direct_child(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    node.children().find(|child| child.kind() == kind)
}

fn name_child_text(node: &SyntaxNode) -> Option<String> {
    direct_child(node, SyntaxKind::NAME).map(|name| name.text().to_string())
}

/// Binding nodes (IDENT_PAT for `let`/params, NAME for `const`) that declare
/// `name` directly within `scope`. A binding belongs to a scope when its
/// declaration statement is an immediate child of that scope, so nested blocks
/// (which appear later as their own ancestor) are not double-counted.
fn scope_bindings<'a>(
    scope: &'a SyntaxNode,
    name: &'a str,
) -> impl Iterator<Item = SyntaxNode> + 'a {
    scope
        .children()
        .filter_map(move |child| match child.kind() {
            SyntaxKind::LET_STMT => direct_child(&child, SyntaxKind::IDENT_PAT)
                .filter(|pat| ident_name(pat).as_deref() == Some(name)),
            SyntaxKind::CONST => {
                direct_child(&child, SyntaxKind::NAME).filter(|n| n.text() == name)
            }
            SyntaxKind::PARAM_LIST => child.children().find_map(|param| {
                direct_child(&param, SyntaxKind::IDENT_PAT)
                    .filter(|pat| ident_name(pat).as_deref() == Some(name))
            }),
            _ => None,
        })
}

fn enclosing_scope_pattern_bindings(scope: &SyntaxNode, name: &str) -> Vec<SyntaxNode> {
    let Some(owner) = scope
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::BLOCK_EXPR)
        .and_then(|block| block.parent())
    else {
        return Vec::new();
    };
    let pattern_owner = match owner.kind() {
        SyntaxKind::FOR_EXPR => Some(owner),
        SyntaxKind::IF_EXPR | SyntaxKind::WHILE_EXPR => direct_child(&owner, SyntaxKind::LET_EXPR),
        _ => None,
    };
    pattern_owner
        .into_iter()
        .flat_map(|node| {
            node.descendants()
                .filter(|child| child.kind() == SyntaxKind::IDENT_PAT)
                .filter(|pat| ident_name(pat).as_deref() == Some(name))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn has_direct_token(node: &SyntaxNode, kind: SyntaxKind) -> bool {
    node.children_with_tokens()
        .any(|child| matches!(child, NodeOrToken::Token(token) if token.kind() == kind))
}

fn has_any_direct_token(node: &SyntaxNode, kinds: &[SyntaxKind]) -> bool {
    node.children_with_tokens().any(|child| {
        matches!(child, NodeOrToken::Token(token) if kinds.iter().any(|kind| *kind == token.kind()))
    })
}

fn tuple_field_type(tuple_type: &str, field_name: &str) -> Option<String> {
    let index = field_name.parse::<usize>().ok()?;
    let tuple_type = receiver_base_type(tuple_type);
    let inner = tuple_type.strip_prefix('(')?.strip_suffix(')')?;
    inner
        .split(',')
        .map(str::trim)
        .nth(index)
        .map(str::to_string)
}

fn array_element_type(array_type: &str) -> Option<String> {
    let inner = array_type.strip_prefix('[')?;
    let inner = inner.strip_suffix(']')?;
    let element = inner
        .rsplit_once(';')
        .map(|(element, _)| element)
        .unwrap_or(inner)
        .trim();
    Some(element.into())
}

fn fixed_array_type_parts(array_type: &str) -> Option<(&str, &str)> {
    let inner = array_type.strip_prefix('[')?.strip_suffix(']')?;
    let (element, len) = inner.rsplit_once(';')?;
    Some((element.trim(), len.trim()))
}

fn indexed_element_type(container_type: &str) -> Option<String> {
    if let Some(referenced) = container_type.strip_prefix('&') {
        let referenced = referenced.strip_prefix("mut ").unwrap_or(referenced).trim();
        let element = vec_element_type(referenced).or_else(|| array_element_type(referenced))?;
        if fixed_array_type_parts(&element).is_some() {
            Some(format!("&{element}"))
        } else {
            Some(element)
        }
    } else {
        vec_element_type(container_type).or_else(|| array_element_type(container_type))
    }
}

fn is_indexable_container_type(typ: &str) -> bool {
    vec_element_type(typ).is_some() || array_element_type(typ).is_some()
}

fn indexed_element_type_for(index_expr: &SyntaxNode, container_type: &str) -> Option<String> {
    if index_expr_has_range_index(index_expr) {
        if is_string_like(container_type) {
            if index_expr_is_ref_operand(index_expr) {
                return Some("str".into());
            }
            return Some("&str".into());
        }
        let element = vec_element_type(container_type)
            .or_else(|| array_element_type(receiver_base_type(container_type)))?;
        if index_expr_is_assignment_lhs(index_expr)
            && container_type.trim_start().starts_with("&mut ")
        {
            return Some(format!("&mut [{element}]"));
        }
        if index_expr_is_ref_operand(index_expr) {
            return Some(format!("[{element}]"));
        }
        return Some(format!("&[{element}]"));
    }
    if index_expr_is_parent_index_base(index_expr) {
        let element = vec_element_type(container_type)
            .or_else(|| array_element_type(receiver_base_type(container_type)))?;
        if index_expr_is_assignment_lhs(index_expr) {
            return Some(format!("&mut {element}"));
        }
        return Some(borrow_shared_type(&element));
    }
    if index_expr_is_method_receiver(index_expr) {
        let element = vec_element_type(container_type)
            .or_else(|| array_element_type(receiver_base_type(container_type)))?;
        return Some(borrow_shared_type(&element));
    }
    if index_expr_is_direct_push_argument(index_expr)
        && container_type.trim_start().starts_with('&')
    {
        let base = receiver_base_type(container_type);
        let element =
            vec_receiver_element_type(container_type).or_else(|| array_element_type(base))?;
        return Some(element);
    }
    if index_expr_is_assignment_lhs(index_expr) && index_expr_is_compound_assignment_lhs(index_expr)
    {
        let element = vec_receiver_element_type(container_type)
            .or_else(|| array_element_type(receiver_base_type(container_type)))?;
        return Some(format!("&mut {element}"));
    }
    if index_expr_is_assignment_lhs(index_expr)
        && let Some(referenced) = container_type.trim_start().strip_prefix("&mut ")
    {
        let element = vec_element_type(referenced).or_else(|| array_element_type(referenced))?;
        return Some(element);
    }
    indexed_element_type(container_type)
}

fn index_expr_has_range_index(index_expr: &SyntaxNode) -> bool {
    let base_range = first_expr_child(index_expr).map(|base| base.text_range());
    index_expr.children().any(|child| {
        child.kind() == SyntaxKind::RANGE_EXPR && Some(child.text_range()) != base_range
    })
}

fn index_expr_is_parent_index_base(index_expr: &SyntaxNode) -> bool {
    let Some(parent_index) = index_expr
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::INDEX_EXPR)
    else {
        return false;
    };
    first_expr_child(&parent_index).is_some_and(|base| base.text_range() == index_expr.text_range())
}

fn index_expr_is_ref_operand(index_expr: &SyntaxNode) -> bool {
    index_expr
        .parent()
        .is_some_and(|parent| parent.kind() == SyntaxKind::REF_EXPR)
}

fn index_expr_is_compound_assignment_lhs(index_expr: &SyntaxNode) -> bool {
    let mut root = index_expr.clone();
    while let Some(parent_index) = root
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::INDEX_EXPR)
    {
        root = parent_index;
    }
    root.parent()
        .filter(|parent| parent.kind() == SyntaxKind::BIN_EXPR)
        .is_some_and(|parent| has_compound_assignment_operator(&parent))
}

fn index_expr_is_method_receiver(index_expr: &SyntaxNode) -> bool {
    let Some(call) = index_expr
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::METHOD_CALL_EXPR)
    else {
        return false;
    };
    first_expr_child(&call).is_some_and(|receiver| receiver.text_range() == index_expr.text_range())
}

fn index_expr_is_direct_push_argument(index_expr: &SyntaxNode) -> bool {
    let Some(arg_list) = index_expr
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::ARG_LIST)
    else {
        return false;
    };
    let Some(call) = arg_list
        .parent()
        .filter(|parent| parent.kind() == SyntaxKind::METHOD_CALL_EXPR)
    else {
        return false;
    };
    matches!(
        direct_child(&call, SyntaxKind::NAME_REF)
            .map(|name| name.text().to_string())
            .as_deref(),
        Some("push" | "push_back")
    )
}

fn path_type_text(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    if node.text().to_string() == "Self"
        && let Some(self_type) = enclosing_impl_self_type(node, semantic)
    {
        return Some(self_type);
    }
    if direct_child(node, SyntaxKind::PATH).is_some_and(|path| {
        path.descendants()
            .any(|child| child.kind() == SyntaxKind::GENERIC_ARG_LIST)
    }) {
        let normalized = normalize_type_text(&node.text().to_string(), semantic);
        if semantic.sysroot && normalized.starts_with("Vec<") {
            return Some(format!("alloc::vec::{}", normalized));
        }
        return Some(normalized);
    }
    node.descendants()
        .find(|child| child.kind() == SyntaxKind::NAME_REF)
        .map(|name| semantic.qualify_type_name(&name.text().to_string()))
}

fn enclosing_impl_self_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let impl_node = node
        .ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::IMPL)?;
    let self_type = impl_node
        .children()
        .filter(|child| is_type_kind(child.kind()))
        .last()?;
    if range_contains(self_type.text_range(), node.text_range()) {
        return None;
    }
    type_text(&self_type, semantic)
}

fn enclosing_self_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    enclosing_impl_self_type(node, semantic).or_else(|| enclosing_trait_self_type(node))
}

fn enclosing_trait_self_type(node: &SyntaxNode) -> Option<String> {
    node.ancestors()
        .find(|ancestor| ancestor.kind() == SyntaxKind::TRAIT)
        .map(|_| "Self".into())
}

fn normalize_type_text(text: &str, semantic: &SemanticModel) -> String {
    let compact = compact_type_text(text);
    let compact = if semantic.sysroot {
        normalize_sysroot_type_text(&compact)
    } else {
        compact
    };
    semantic
        .structs
        .keys()
        .filter(|name| !name.contains("::"))
        .fold(compact, |acc, name| {
            acc.replace(name, &semantic.qualify_type_name(name))
        })
}

fn compact_type_text(text: &str) -> String {
    restore_dyn_trait_spacing(
        &text
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>(),
    )
}

fn restore_dyn_trait_spacing(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(relative) = input[cursor..].find("dyn") {
        let start = cursor + relative;
        let end = start + 3;
        let before = input[..start].chars().next_back();
        let after = input[end..].chars().next();
        out.push_str(&input[cursor..start]);
        out.push_str("dyn");
        if is_identifier_boundary(before)
            && matches!(after, Some(ch) if ch.is_alphanumeric() || ch == '_')
        {
            out.push(' ');
        }
        cursor = end;
    }
    out.push_str(&input[cursor..]);
    out
}

fn normalize_sysroot_type_text(text: &str) -> String {
    let text = restore_dyn_trait_spacing(text);
    let text = text
        .replace("core::str::iter::Chars<_>", "core::str::iter::Chars<'a>")
        .replace("core::str::iter::Lines<_>", "core::str::iter::Lines<'a>")
        .replace("core::str::iter::Split<_,", "core::str::iter::Split<'a,")
        .replace("core::slice::iter::Iter<_,", "core::slice::iter::Iter<'a,")
        .replace(
            "std::collections::hash::map::Entry<_,",
            "std::collections::hash::map::Entry<'a,",
        );
    let normalized = [
        "String",
        "Option",
        "Result",
        "Vec",
        "Box",
        "HashMap",
        "HashSet",
        "VecDeque",
        "BinaryHeap",
        "BTreeMap",
        "BTreeSet",
        "Reverse",
        "Ordering",
        "Rc",
        "Weak",
        "RefCell",
        "Cell",
        "PhantomData",
        "Cow",
        "Arc",
        "Hash",
        "Hasher",
        "FromStr",
        "TryFrom",
        "TryInto",
        "Borrow",
        "Deref",
        "DerefMut",
        "Index",
        "IndexMut",
        "Add",
        "AddAssign",
        "Mul",
        "Neg",
        "Not",
        "Error",
        "Formatter",
        "RandomState",
        "Global",
    ]
    .iter()
    .fold(text, |acc, name| {
        let Some(qualified) = sysroot_qualified_type_name(name) else {
            return acc;
        };
        replace_bare_type_name(&acc, name, qualified)
    });
    let normalized = expand_nested_box_defaults(&normalized);
    let normalized = expand_nested_hashmap_defaults(&normalized);
    normalize_generic_spacing(&expand_nested_vec_defaults(&normalized))
}

fn expand_nested_hashmap_defaults(input: &str) -> String {
    expand_nested_hashmap_defaults_inner(input, true)
}

fn expand_root_hashmap_defaults(input: &str) -> String {
    const HASHMAP: &str = "std::collections::hash::map::HashMap<";
    let expanded = expand_nested_hashmap_defaults(input);
    if !expanded.starts_with(HASHMAP) {
        return expanded;
    }
    let angle_idx = HASHMAP.len() - 1;
    let Some(end_idx) = find_matching_angle(&expanded, angle_idx) else {
        return expanded;
    };
    if end_idx != expanded.len() - 1 {
        return expanded;
    }
    let inner = &expanded[angle_idx + 1..end_idx];
    if top_level_arg_count(inner) != 2 {
        return expanded;
    }
    format!("{HASHMAP}{inner}, std::hash::random::RandomState, alloc::alloc::Global>")
}

fn expand_root_vec_defaults(input: &str) -> String {
    const VEC: &str = "alloc::vec::Vec<";
    let expanded = expand_nested_vec_defaults(input);
    if !expanded.starts_with(VEC) {
        return expanded;
    }
    let angle_idx = VEC.len() - 1;
    let Some(end_idx) = find_matching_angle(&expanded, angle_idx) else {
        return expanded;
    };
    if end_idx != expanded.len() - 1 {
        return expanded;
    }
    let inner = &expanded[angle_idx + 1..end_idx];
    if top_level_arg_count(inner) != 1 {
        return expanded;
    }
    format!("{VEC}{inner}, alloc::alloc::Global>")
}

fn expand_root_binary_heap_defaults(input: &str) -> String {
    const HEAP: &str = "alloc::collections::binary_heap::BinaryHeap<";
    if !input.starts_with(HEAP) {
        return input.into();
    }
    let angle_idx = HEAP.len() - 1;
    let Some(end_idx) = find_matching_angle(input, angle_idx) else {
        return input.into();
    };
    if end_idx != input.len() - 1 {
        return input.into();
    }
    let inner = &input[angle_idx + 1..end_idx];
    if top_level_arg_count(inner) != 1 {
        return input.into();
    }
    format!("{HEAP}{inner}, alloc::alloc::Global>")
}

fn expand_root_vecdeque_defaults(input: &str) -> String {
    const DEQUE: &str = "alloc::collections::vec_deque::VecDeque<";
    if !input.starts_with(DEQUE) {
        return input.into();
    }
    let angle_idx = DEQUE.len() - 1;
    let Some(end_idx) = find_matching_angle(input, angle_idx) else {
        return input.into();
    };
    if end_idx != input.len() - 1 {
        return input.into();
    }
    let inner = &input[angle_idx + 1..end_idx];
    if top_level_arg_count(inner) != 1 {
        return input.into();
    }
    format!("{DEQUE}{inner}, alloc::alloc::Global>")
}

fn expand_nested_vec_defaults(input: &str) -> String {
    expand_nested_vec_defaults_inner(input, true)
}

fn expand_nested_vec_defaults_inner(input: &str, root: bool) -> String {
    const VEC: &str = "alloc::vec::Vec<";
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(relative_idx) = input[cursor..].find(VEC) {
        let idx = cursor + relative_idx;
        let angle_idx = idx + VEC.len() - 1;
        let Some(end_idx) = find_matching_angle(input, angle_idx) else {
            break;
        };
        out.push_str(&input[cursor..idx]);
        let inner = expand_nested_vec_defaults_inner(&input[angle_idx + 1..end_idx], false);
        let is_whole_root = root && idx == 0 && end_idx == input.len() - 1;
        out.push_str(VEC);
        out.push_str(&inner);
        if !is_whole_root && top_level_arg_count(&inner) == 1 {
            out.push_str(", alloc::alloc::Global");
        }
        out.push('>');
        cursor = end_idx + 1;
    }
    out.push_str(&input[cursor..]);
    out
}

fn expand_root_box_defaults(input: &str) -> String {
    const BOX_TYPE: &str = "alloc::boxed::Box<";
    let expanded = expand_nested_box_defaults(input);
    if !expanded.starts_with(BOX_TYPE) {
        return expanded;
    }
    let angle_idx = BOX_TYPE.len() - 1;
    let Some(end_idx) = find_matching_angle(&expanded, angle_idx) else {
        return expanded;
    };
    if end_idx != expanded.len() - 1 {
        return expanded;
    }
    let inner = &expanded[angle_idx + 1..end_idx];
    if top_level_arg_count(inner) != 1 {
        return expanded;
    }
    format!("{BOX_TYPE}{}>", box_inner_with_defaults(inner))
}

fn expand_nested_box_defaults(input: &str) -> String {
    expand_nested_box_defaults_inner(input, true)
}

fn expand_nested_box_defaults_inner(input: &str, root: bool) -> String {
    const BOX_TYPE: &str = "alloc::boxed::Box<";
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(relative_idx) = input[cursor..].find(BOX_TYPE) {
        let idx = cursor + relative_idx;
        let angle_idx = idx + BOX_TYPE.len() - 1;
        let Some(end_idx) = find_matching_angle(input, angle_idx) else {
            break;
        };
        out.push_str(&input[cursor..idx]);
        let inner = expand_nested_box_defaults_inner(&input[angle_idx + 1..end_idx], false);
        let is_whole_root = root && idx == 0 && end_idx == input.len() - 1;
        out.push_str(BOX_TYPE);
        if is_whole_root && top_level_arg_count(&inner) == 1 {
            out.push_str(&box_inner_with_dyn_static(&inner));
        } else if top_level_arg_count(&inner) == 1 {
            out.push_str(&box_inner_with_defaults(&inner));
        } else {
            out.push_str(&inner);
        }
        out.push('>');
        cursor = end_idx + 1;
    }
    out.push_str(&input[cursor..]);
    out
}

fn box_inner_with_defaults(inner: &str) -> String {
    format!("{}, alloc::alloc::Global", box_inner_with_dyn_static(inner))
}

fn box_inner_with_dyn_static(inner: &str) -> String {
    let inner = inner.trim();
    if inner.starts_with("dyn ") && !inner.contains(" + '") {
        format!("{inner} + 'static")
    } else {
        inner.to_string()
    }
}

fn expand_nested_hashmap_defaults_inner(input: &str, root: bool) -> String {
    const HASHMAP: &str = "std::collections::hash::map::HashMap<";
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(relative_idx) = input[cursor..].find(HASHMAP) {
        let idx = cursor + relative_idx;
        let angle_idx = idx + HASHMAP.len() - 1;
        let Some(end_idx) = find_matching_angle(input, angle_idx) else {
            break;
        };
        out.push_str(&input[cursor..idx]);
        let inner = expand_nested_hashmap_defaults_inner(&input[angle_idx + 1..end_idx], false);
        let is_whole_root = root && idx == 0 && end_idx == input.len() - 1;
        out.push_str(HASHMAP);
        out.push_str(&inner);
        if !is_whole_root && top_level_arg_count(&inner) == 2 {
            out.push_str(", std::hash::random::RandomState, alloc::alloc::Global");
        }
        out.push('>');
        cursor = end_idx + 1;
    }
    out.push_str(&input[cursor..]);
    out
}

fn find_matching_angle(input: &str, open_idx: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (idx, ch) in input[open_idx..].char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open_idx + idx);
                }
            }
            _ => {}
        }
    }
    None
}

fn top_level_arg_count(input: &str) -> usize {
    let mut count = usize::from(!input.trim().is_empty());
    let mut depth = 0usize;
    for ch in input.chars() {
        match ch {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => count += 1,
            _ => {}
        }
    }
    count
}

fn normalize_generic_spacing(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        out.push(ch);
        if ch == ',' {
            while matches!(chars.peek(), Some(next) if next.is_whitespace()) {
                chars.next();
            }
            out.push(' ');
        }
    }
    out
}

fn replace_bare_type_name(input: &str, name: &str, qualified: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    for (idx, _) in input.match_indices(name) {
        let end = idx + name.len();
        if is_bare_type_name(input, idx, end) {
            out.push_str(&input[cursor..idx]);
            out.push_str(qualified);
            cursor = end;
        }
    }
    out.push_str(&input[cursor..]);
    out
}

fn is_bare_type_name(input: &str, start: usize, end: usize) -> bool {
    let before = input[..start].chars().next_back();
    let after = input[end..].chars().next();
    !matches!(before, Some(ch) if ch.is_alphanumeric() || ch == '_' || ch == ':')
        && !matches!(after, Some(ch) if ch.is_alphanumeric() || ch == '_' || ch == ':')
}

fn sysroot_qualified_type_name(name: &str) -> Option<&'static str> {
    match name {
        "String" => Some("alloc::string::String"),
        "Option" => Some("core::option::Option"),
        "Result" => Some("core::result::Result"),
        "Vec" => Some("alloc::vec::Vec"),
        "Box" => Some("alloc::boxed::Box"),
        "HashMap" => Some("std::collections::hash::map::HashMap"),
        "HashSet" => Some("std::collections::hash::set::HashSet"),
        "VecDeque" => Some("alloc::collections::vec_deque::VecDeque"),
        "BinaryHeap" => Some("alloc::collections::binary_heap::BinaryHeap"),
        "BTreeMap" => Some("alloc::collections::btree::map::BTreeMap"),
        "BTreeSet" => Some("alloc::collections::btree::set::BTreeSet"),
        "Reverse" => Some("core::cmp::Reverse"),
        "Ordering" => Some("core::cmp::Ordering"),
        "Rc" => Some("alloc::rc::Rc"),
        "Weak" => Some("alloc::rc::Weak"),
        "RefCell" => Some("core::cell::RefCell"),
        "Cell" => Some("core::cell::Cell"),
        "PhantomData" => Some("core::marker::PhantomData"),
        "Cow" => Some("alloc::borrow::Cow"),
        "Arc" => Some("alloc::sync::Arc"),
        "Hash" => Some("core::hash::Hash"),
        "Hasher" => Some("core::hash::Hasher"),
        "FromStr" => Some("core::str::traits::FromStr"),
        "TryFrom" => Some("core::convert::TryFrom"),
        "TryInto" => Some("core::convert::TryInto"),
        "Borrow" => Some("core::borrow::Borrow"),
        "Deref" => Some("core::ops::deref::Deref"),
        "DerefMut" => Some("core::ops::deref::DerefMut"),
        "Index" => Some("core::ops::index::Index"),
        "IndexMut" => Some("core::ops::index::IndexMut"),
        "Add" => Some("core::ops::arith::Add"),
        "AddAssign" => Some("core::ops::arith::AddAssign"),
        "Mul" => Some("core::ops::arith::Mul"),
        "Neg" => Some("core::ops::arith::Neg"),
        "Not" => Some("core::ops::bit::Not"),
        "Error" => Some("core::error::Error"),
        "Formatter" => Some("core::fmt::Formatter"),
        "RandomState" => Some("std::hash::random::RandomState"),
        "Global" => Some("alloc::alloc::Global"),
        _ => None,
    }
}

fn is_unresolved_generic_container(typ: &str) -> bool {
    typ.starts_with("Vec<") || typ == "Vec"
}

fn generic_index_expr_type(node: &SyntaxNode, semantic: &SemanticModel) -> Option<String> {
    let root_name = index_root_name(node)?;
    let mut typ = semantic.resolve_var(node, &root_name)?;
    for _ in 0..index_depth(node) {
        if let Some(inner) = indexed_element_type_for(node, &typ) {
            typ = inner;
        } else {
            return None;
        }
    }
    (!is_unresolved_generic_container(&typ)).then_some(typ)
}

fn index_root_name(node: &SyntaxNode) -> Option<String> {
    let mut current = node.clone();
    loop {
        let base = current
            .children()
            .find(|child| is_expr_kind(child.kind()))?;
        match base.kind() {
            SyntaxKind::PATH_EXPR => return path_expr_name(&base),
            SyntaxKind::INDEX_EXPR => current = base,
            _ => return None,
        }
    }
}

fn index_depth(node: &SyntaxNode) -> usize {
    let mut depth = 0;
    let mut current = node.clone();
    loop {
        if current.kind() != SyntaxKind::INDEX_EXPR {
            return depth;
        }
        depth += 1;
        let Some(base) = current.children().find(|child| is_expr_kind(child.kind())) else {
            return depth;
        };
        current = base;
    }
}

fn vec_element_type(typ: &str) -> Option<String> {
    let inner = typ
        .strip_prefix("alloc::vec::Vec<")
        .or_else(|| typ.strip_prefix("Vec<"))?
        .strip_suffix('>')?;
    first_top_level_arg(inner)
}

fn is_builtin_type_name(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "char"
            | "f32"
            | "f64"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "str"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "()"
            | "!"
    )
}

fn is_expr_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ARRAY_EXPR
            | SyntaxKind::ASM_EXPR
            | SyntaxKind::AWAIT_EXPR
            | SyntaxKind::BIN_EXPR
            | SyntaxKind::BLOCK_EXPR
            | SyntaxKind::BREAK_EXPR
            | SyntaxKind::CALL_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::CLOSURE_EXPR
            | SyntaxKind::CONTINUE_EXPR
            | SyntaxKind::FIELD_EXPR
            | SyntaxKind::FOR_EXPR
            | SyntaxKind::FORMAT_ARGS_EXPR
            | SyntaxKind::IF_EXPR
            | SyntaxKind::INDEX_EXPR
            | SyntaxKind::LET_EXPR
            | SyntaxKind::LITERAL
            | SyntaxKind::LOOP_EXPR
            | SyntaxKind::MACRO_EXPR
            | SyntaxKind::MATCH_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::OFFSET_OF_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::PATH_EXPR
            | SyntaxKind::PREFIX_EXPR
            | SyntaxKind::RANGE_EXPR
            | SyntaxKind::RECORD_EXPR
            | SyntaxKind::REF_EXPR
            | SyntaxKind::RETURN_EXPR
            | SyntaxKind::TRY_EXPR
            | SyntaxKind::TUPLE_EXPR
            | SyntaxKind::UNDERSCORE_EXPR
            | SyntaxKind::WHILE_EXPR
            | SyntaxKind::YIELD_EXPR
    )
}

fn is_type_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ARRAY_TYPE
            | SyntaxKind::DYN_TRAIT_TYPE
            | SyntaxKind::FN_PTR_TYPE
            | SyntaxKind::FOR_TYPE
            | SyntaxKind::IMPL_TRAIT_TYPE
            | SyntaxKind::INFER_TYPE
            | SyntaxKind::MACRO_TYPE
            | SyntaxKind::NEVER_TYPE
            | SyntaxKind::PAREN_TYPE
            | SyntaxKind::PATH_TYPE
            | SyntaxKind::PTR_TYPE
            | SyntaxKind::REF_TYPE
            | SyntaxKind::SLICE_TYPE
            | SyntaxKind::TUPLE_TYPE
    )
}

fn should_skip_kind(kind: SyntaxKind) -> bool {
    kind.is_trivia() || matches!(kind, SyntaxKind::ERROR)
}

fn kind_name(kind: SyntaxKind) -> String {
    format!("{kind:?}")
}

fn offset_to_usize(offset: ra_ap_syntax::TextSize) -> usize {
    let raw: u32 = offset.into();
    raw as usize
}

fn relative_file_path(input_root: &Path, file: &Path) -> String {
    let relative = file
        .strip_prefix(input_root)
        .ok()
        .or_else(|| file.file_name().map(Path::new))
        .unwrap_or(file);
    path_to_slash_string(relative)
}

fn full_file_path(file: &Path) -> String {
    file.canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn path_to_slash_string(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn crate_name_for(input_root: &Path, file: &Path) -> Option<String> {
    let mut current = if file.is_dir() {
        file.to_path_buf()
    } else {
        file.parent().unwrap_or(file).to_path_buf()
    };

    loop {
        if !current.starts_with(input_root) {
            return None;
        }
        let cargo_toml = current.join("Cargo.toml");
        if cargo_toml.is_file() {
            return parse_package_name(&cargo_toml);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn parse_package_name(cargo_toml: &Path) -> Option<String> {
    let contents = fs::read_to_string(cargo_toml).ok()?;
    let mut in_package = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if in_package && trimmed.starts_with("name") {
            let (_, raw) = trimmed.split_once('=')?;
            return Some(raw.trim().trim_matches('"').to_string());
        }
    }
    None
}

fn module_path_for(relative_file: &Path) -> Option<String> {
    let mut parts: Vec<String> = relative_file
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if parts.first().map(String::as_str) == Some("src") {
        parts.remove(0);
    }
    let last = parts.last_mut()?;
    if last == "lib.rs" || last == "main.rs" || last == "mod.rs" {
        parts.pop();
    } else if let Some(stripped) = last.strip_suffix(".rs") {
        *last = stripped.to_string();
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("::"))
    }
}

fn line_count(content: &str) -> usize {
    content.lines().count().max(1)
}

struct LineIndex {
    line_starts: Vec<usize>,
}

impl LineIndex {
    fn new(content: &str) -> Self {
        let mut line_starts = vec![0];
        for (idx, byte) in content.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(idx + 1);
            }
        }
        Self { line_starts }
    }

    fn line_col(&self, offset: usize) -> (usize, usize) {
        let line = match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(next) => next.saturating_sub(1),
        };
        (line, offset.saturating_sub(self.line_starts[line]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn emits_source_file_and_fn_shape() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"
"#,
        )
        .unwrap();
        let file = root.join("src/lib.rs");
        fs::write(&file, "fn main(x: i32) -> i32 { x }\n").unwrap();

        let json = parse_file(root, &file).unwrap();
        assert_eq!(json["relativeFilePath"], "src/lib.rs");
        assert_eq!(json["crateName"], "demo");
        assert_eq!(json["children"][0]["nodeKind"], "SOURCE_FILE");
        assert!(contains_kind(&json["children"][0], "FN"));
        assert!(contains_kind(&json["children"][0], "PARAM_LIST"));
        assert!(contains_kind(&json["children"][0], "RET_TYPE"));
    }

    #[test]
    fn infers_untyped_ident_pat_from_literal_initializer() {
        let file = PathBuf::from("/tmp/src/lib.rs");
        let json = parse_source(
            Path::new("/tmp"),
            &file,
            "fn main() {\n  let s = \"hello\";\n  let n = 1;\n}\n",
        )
        .unwrap();
        let root = &json["children"][0];
        let ident_pats = collect_kind(root, "IDENT_PAT");
        assert_eq!(ident_pats.len(), 2);
        assert_eq!(ident_pats[0]["typeFullName"], "&str");
        assert_eq!(ident_pats[1]["typeFullName"], "i32");
    }

    #[test]
    fn emits_type_full_name_on_path_type_leaf_name_refs() {
        let file = PathBuf::from("/tmp/src/lib.rs");
        let json = parse_source(
            Path::new("/tmp"),
            &file,
            "const MAX_SIZE: usize = 1024;\nfn id(x: i32) -> i32 { x }\n",
        )
        .unwrap();
        let name_refs = collect_kind(&json["children"][0], "NAME_REF");
        let typed_refs: Vec<_> = name_refs
            .into_iter()
            .filter_map(|node| node.get("typeFullName").and_then(|value| value.as_str()))
            .collect();
        assert!(typed_refs.contains(&"usize"));
        assert!(typed_refs.contains(&"i32"));
    }

    #[test]
    fn applies_declared_type_to_initializer_literals() {
        let file = PathBuf::from("/tmp/src/lib.rs");
        let json = parse_source(
            Path::new("/tmp"),
            &file,
            "fn main() { let x: usize = 10; }\n",
        )
        .unwrap();
        let literals = collect_kind(&json["children"][0], "LITERAL");
        assert_eq!(literals[0]["typeFullName"], "usize");
    }

    #[test]
    fn infers_let_type_from_later_simple_assignment() {
        let file = PathBuf::from("/tmp/src/lib.rs");
        let json = parse_source(Path::new("/tmp"), &file, "fn main() { let x; x = 5; }\n").unwrap();
        let ident_pats = collect_kind(&json["children"][0], "IDENT_PAT");
        assert_eq!(ident_pats[0]["typeFullName"], "i32");
    }

    #[test]
    fn inner_let_shadows_outer_for_variable_reference() {
        let file = PathBuf::from("/tmp/src/lib.rs");
        // The outer `x` is `&str`; the inner block rebinds `x` to `i32`. The
        // reference inside the inner block must resolve to the inner binding,
        // while the reference after the block resolves to the outer one.
        let json = parse_source(
            Path::new("/tmp"),
            &file,
            "fn main() {\n  let x = \"hello\";\n  {\n    let x = 1;\n    let y = x;\n  }\n  let z = x;\n}\n",
        )
        .unwrap();
        let root = &json["children"][0];
        let ident_pats = collect_kind(root, "IDENT_PAT");
        // Order: outer x (&str), inner x (i32), y (from inner x), z (from outer x).
        assert_eq!(ident_pats[0]["typeFullName"], "&str");
        assert_eq!(ident_pats[1]["typeFullName"], "i32");
        assert_eq!(ident_pats[2]["typeFullName"], "i32");
        assert_eq!(ident_pats[3]["typeFullName"], "&str");
    }

    #[test]
    fn resolves_string_new_against_sysroot() {
        let file = PathBuf::from("/tmp/src/lib.rs");
        let json = parse_source_with_sysroot(
            Path::new("/tmp"),
            &file,
            "fn main() { let s = String::new(); }\n",
            true,
        )
        .unwrap();
        let root = &json["children"][0];
        let call = &collect_kind(root, "CALL_EXPR")[0];
        assert_eq!(call["typeFullName"], "alloc::string::String");
        assert_eq!(call["methodFullName"], "alloc::string::String::new");
    }

    #[test]
    fn resolves_string_push_str_against_sysroot() {
        let file = PathBuf::from("/tmp/src/lib.rs");
        let json = parse_source_with_sysroot(
            Path::new("/tmp"),
            &file,
            "fn main() { let mut s = String::new(); s.push_str(\"x\"); }\n",
            true,
        )
        .unwrap();
        let root = &json["children"][0];
        let call = &collect_kind(root, "METHOD_CALL_EXPR")[0];
        assert_eq!(call["methodFullName"], "alloc::string::String::push_str");
        assert_eq!(call["typeFullName"], "()");
    }

    #[test]
    fn resolves_user_defined_method_and_return_type() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"
"#,
        )
        .unwrap();
        let file = root.join("src/lib.rs");
        fs::write(
            &file,
            "struct Point { x: i32 }\n\
             impl Point {\n\
             \x20   fn new() -> Self { Point { x: 0 } }\n\
             \x20   fn value(&self) -> i32 { self.x }\n\
             }\n\
             fn main() {\n\
             \x20   let p = Point::new();\n\
             \x20   let v = p.value();\n\
             }\n",
        )
        .unwrap();

        let json = parse_file(root, &file).unwrap();
        let source = &json["children"][0];
        let calls = collect_kind(source, "CALL_EXPR");
        // `Point::new()` -> associated fn returning `Self`.
        let new_call = calls
            .iter()
            .find(|call| call["methodFullName"] == "demo::Point::new")
            .expect("Point::new should resolve");
        assert_eq!(new_call["typeFullName"], "demo::Point");
        // `p.value()` -> instance method returning `i32`.
        let value_call = &collect_kind(source, "METHOD_CALL_EXPR")[0];
        assert_eq!(value_call["methodFullName"], "demo::Point::value");
        assert_eq!(value_call["typeFullName"], "i32");
    }

    /// Write `source` into a fresh on-disk cargo crate named `crate_name` and
    /// run the generator with the sysroot (HIR) path enabled, returning the
    /// SOURCE_FILE node. HIR needs a real file plus a discoverable sysroot, so
    /// these tests are inherently filesystem/toolchain bound.
    fn parse_hir_crate(crate_name: &str, source: &str) -> (tempfile::TempDir, Value) {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"
            ),
        )
        .unwrap();
        let file = root.join("src/lib.rs");
        fs::write(&file, source).unwrap();
        let json = parse_file_with_sysroot(root, &file, true).unwrap();
        let source_file = json["children"][0].clone();
        (dir, source_file)
    }

    fn text_of(value: &Value, source: &str) -> String {
        let Some(range) = value.get("range").and_then(Value::as_object) else {
            return String::new();
        };
        let Some(start) = range
            .get("startOffset")
            .and_then(Value::as_u64)
            .map(|offset| offset as usize)
        else {
            return String::new();
        };
        let Some(end) = range
            .get("endOffset")
            .and_then(Value::as_u64)
            .map(|offset| offset as usize)
        else {
            return String::new();
        };
        source.get(start..end).unwrap_or_default().to_string()
    }

    /// First node of `kind` whose reconstructed source text contains `needle`.
    fn find_kind_containing<'a>(
        value: &'a Value,
        source: &str,
        kind: &str,
        needle: &str,
    ) -> Option<&'a Value> {
        collect_kind(value, kind)
            .into_iter()
            .find(|node| text_of(node, source).contains(needle))
    }

    fn contains_kind(value: &Value, kind: &str) -> bool {
        value["nodeKind"] == kind
            || value["children"]
                .as_array()
                .is_some_and(|children| children.iter().any(|child| contains_kind(child, kind)))
    }

    fn collect_kind<'a>(value: &'a Value, kind: &str) -> Vec<&'a Value> {
        let mut out = Vec::new();
        if value["nodeKind"] == kind {
            out.push(value);
        }
        if let Some(children) = value["children"].as_array() {
            for child in children {
                out.extend(collect_kind(child, kind));
            }
        }
        out
    }

    // --- HIR-backed resolution -------------------------------------------------
    //
    // The tests below prove the rust-analyzer HIR path resolves cases the
    // hand-rolled `SemanticModel` cannot: a generic function's monomorphized
    // return type, a trait method's canonical callable, `HashMap`/`Option`
    // methods (absent from the heuristic's hard-coded table), and the inferred
    // element type of `Vec::new()`. Each requires a discoverable sysroot.

    const HIR_FIXTURE: &str = r#"use std::collections::HashMap;

trait Greeter {
    fn greet(&self) -> i32;
}

struct Robot;

impl Greeter for Robot {
    fn greet(&self) -> i32 { 42 }
}

fn identity<T>(value: T) -> T { value }

fn exercise() {
    let g = identity(7i64);
    let mut v = Vec::new();
    v.push(1u8);
    let mut m: HashMap<i32, i32> = HashMap::new();
    let got = m.get(&1);
    let opt = Some(3i16);
    let inner = opt.unwrap();
    let r = Robot;
    let greeting = r.greet();
}
"#;

    #[test]
    fn hir_resolves_generic_return_type() {
        let (_dir, source) = parse_hir_crate("hirdemo", HIR_FIXTURE);
        // `opt.unwrap()` on `Option<i16>` returns the monomorphized `i16`. The
        // heuristic has no entry for `Option::unwrap`, so it emits nothing here;
        // HIR resolves both the callable and the generic return type, and the
        // inferred type flows to the `inner` binding.
        let unwrap_call =
            find_kind_containing(&source, HIR_FIXTURE, "METHOD_CALL_EXPR", "opt.unwrap()")
                .expect("unwrap call present");
        assert_eq!(
            unwrap_call["methodFullName"],
            "core::option::Option::unwrap"
        );
        assert_eq!(unwrap_call["typeFullName"], "i16");
        let inner = collect_kind(&source, "IDENT_PAT")
            .into_iter()
            .find(|node| text_of(node, HIR_FIXTURE) == "inner")
            .expect("binding `inner` present");
        assert_eq!(inner["typeFullName"], "i16");
        // The user generic fn's call target includes its declared generic
        // parameter list, matching the reference JSON's methodFullName shape.
        let identity_call =
            find_kind_containing(&source, HIR_FIXTURE, "CALL_EXPR", "identity(7i64)")
                .expect("identity call present");
        assert_eq!(identity_call["methodFullName"], "hirdemo::identity<T>");
    }

    #[test]
    fn hir_resolves_trait_method_call() {
        let (_dir, source) = parse_hir_crate("hirdemo", HIR_FIXTURE);
        let call = find_kind_containing(&source, HIR_FIXTURE, "METHOD_CALL_EXPR", "r.greet()")
            .expect("greet call");
        // Resolved through the trait impl to the user type's canonical method,
        // with the detached-file crate stem rewritten to the package name.
        assert_eq!(call["methodFullName"], "hirdemo::Robot::greet");
        assert_eq!(call["typeFullName"], "i32");
    }

    #[test]
    fn hir_resolves_hashmap_and_option_methods() {
        let (_dir, source) = parse_hir_crate("hirdemo", HIR_FIXTURE);
        // `HashMap::get` is not in the heuristic's table at all.
        let get_call = find_kind_containing(&source, HIR_FIXTURE, "METHOD_CALL_EXPR", "m.get(&1)")
            .expect("get call");
        assert_eq!(
            get_call["methodFullName"],
            "std::collections::hash::map::HashMap::get"
        );
        // `Option::unwrap` resolves where the heuristic is silent.
        let unwrap_call =
            find_kind_containing(&source, HIR_FIXTURE, "METHOD_CALL_EXPR", "opt.unwrap()")
                .expect("unwrap call");
        assert_eq!(
            unwrap_call["methodFullName"],
            "core::option::Option::unwrap"
        );
    }

    #[test]
    fn hir_resolves_vec_new_element_type() {
        let (_dir, source) = parse_hir_crate("hirdemo", HIR_FIXTURE);
        // The element type of `Vec::new()` is only known after unifying it with
        // the later `v.push(1u8)`; HIR renders the full monomorphized type,
        // including the allocator parameter.
        let call = find_kind_containing(&source, HIR_FIXTURE, "CALL_EXPR", "Vec::new()")
            .expect("Vec::new call");
        assert_eq!(
            call["typeFullName"],
            "alloc::vec::Vec<u8, alloc::alloc::Global>"
        );
    }

    #[test]
    fn heuristic_alone_cannot_resolve_what_hir_does() {
        // Same fixture without the sysroot/HIR path: the heuristic cannot type a
        // generic return, an unknown std method, or a `Vec::new()` element. This
        // pins the contrast the HIR integration is meant to close.
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"hirdemo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let file = root.join("src/lib.rs");
        fs::write(&file, HIR_FIXTURE).unwrap();

        let json = parse_file_with_sysroot(root, &file, false).unwrap();
        let source = &json["children"][0];

        let get_call = find_kind_containing(source, HIR_FIXTURE, "METHOD_CALL_EXPR", "m.get(&1)")
            .expect("get call");
        assert!(
            get_call.get("methodFullName").is_none(),
            "heuristic must not resolve HashMap::get; got {:?}",
            get_call.get("methodFullName")
        );
        let identity_call =
            find_kind_containing(source, HIR_FIXTURE, "CALL_EXPR", "identity(7i64)")
                .expect("identity call");
        assert_ne!(
            identity_call.get("typeFullName").and_then(Value::as_str),
            Some("i64"),
            "heuristic cannot monomorphize the generic return type"
        );
    }
}
