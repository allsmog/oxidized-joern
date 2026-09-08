//! Import the exact C lowering into the shared production graph.
//!
//! The exact lowerer emits a deterministic neutral text form. This adapter is
//! intentionally strict: every node and edge label must map to the shared
//! schema, and every edge address must resolve. That makes schema omissions a
//! build failure instead of silently dropping semantics.

use cpg_core::{Cpg, EdgeKind, Layer, NodeId, NodeKind};
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Debug)]
struct RawNode {
    kind: NodeKind,
    props: HashMap<String, String>,
    address: Option<String>,
    parent: Option<usize>,
    depth: usize,
}

#[derive(Debug)]
struct RawEdge {
    kind: EdgeKind,
    source: String,
    target: String,
}

pub fn graph_from_canonical_dump(dump: &str, sources: &[(String, String)]) -> Cpg {
    let mut raw_nodes = Vec::new();
    let mut raw_edges = Vec::new();
    let mut raw_ast_edges = Vec::new();
    let mut address_to_raw = HashMap::new();
    let mut ast_stack: Vec<(usize, usize)> = Vec::new();
    let mut methods_by_full: HashMap<String, usize> = HashMap::new();
    let mut type_decls_by_full: HashMap<String, usize> = HashMap::new();
    let mut reuse: Option<(usize, Vec<usize>, usize)> = None;
    let mut block = String::new();
    let mut block_line = 0usize;

    for original in dump.lines() {
        if original.is_empty() {
            ast_stack.clear();
            block.clear();
            block_line = 0;
            continue;
        }
        if let Some(line) = original.strip_prefix("NODES|") {
            let (kind, props) = parse_node(line);
            let address = external_address(kind, &props);
            if kind == NodeKind::TypeDecl {
                if let Some(&existing) = props
                    .get("FULL_NAME")
                    .and_then(|full| type_decls_by_full.get(full))
                {
                    for alias in address_aliases(kind, &props, address.as_deref()) {
                        address_to_raw.entry(alias).or_insert(existing);
                    }
                    continue;
                }
            }
            let raw = raw_nodes.len();
            raw_nodes.push(RawNode {
                kind,
                props,
                address: address.clone(),
                parent: None,
                depth: 0,
            });
            for alias in address_aliases(kind, &raw_nodes[raw].props, address.as_deref()) {
                address_to_raw.entry(alias).or_insert(raw);
            }
            if kind == NodeKind::TypeDecl {
                if let Some(full) = raw_nodes[raw].props.get("FULL_NAME") {
                    type_decls_by_full.insert(full.clone(), raw);
                }
            }
            continue;
        }
        if let Some(line) = original.strip_prefix("EDGES|") {
            raw_edges.push(parse_edge(line));
            continue;
        }
        if let Some(line) = original.strip_prefix("FLOWS|") {
            raw_edges.push(parse_flow(line));
            continue;
        }

        let depth = (original.len() - original.trim_start().len()) / 2;
        let line = original.trim_start();
        let (kind, props) = parse_node(line);
        if depth == 0 {
            block = props
                .get("FULL_NAME")
                .cloned()
                .unwrap_or_else(|| panic!("top-level exact node has no FULL_NAME: {line}"));
            block_line = 0;
            ast_stack.clear();
        }
        if reuse
            .as_ref()
            .is_some_and(|(base_depth, _, _)| depth <= *base_depth)
        {
            reuse = None;
        }
        while ast_stack.last().is_some_and(|(d, _)| *d >= depth) {
            ast_stack.pop();
        }
        let parent = ast_stack.last().map(|(_, raw)| *raw);
        let address = format!("{block}#{block_line}");
        block_line += 1;

        if let Some((_, template, next)) = reuse.as_mut() {
            let raw = *template
                .get(*next)
                .unwrap_or_else(|| panic!("duplicate method view is longer than its template"));
            assert_eq!(
                raw_nodes[raw].kind, kind,
                "duplicate method node kind drift"
            );
            assert_eq!(
                raw_nodes[raw].props, props,
                "duplicate method node properties drift"
            );
            address_to_raw.insert(address, raw);
            ast_stack.push((depth, raw));
            *next += 1;
            continue;
        }

        if kind == NodeKind::Method {
            if let Some(full) = props.get("FULL_NAME") {
                if let Some(&existing) = methods_by_full.get(full) {
                    if let Some(parent) = parent {
                        raw_ast_edges.push((parent, existing));
                    }
                    let template = raw_subtree(existing, &raw_nodes);
                    address_to_raw.insert(address, existing);
                    ast_stack.push((depth, existing));
                    reuse = Some((depth, template, 1));
                    continue;
                }
            }
        }
        let raw = raw_nodes.len();
        raw_nodes.push(RawNode {
            kind,
            props,
            address: Some(address.clone()),
            parent,
            depth,
        });
        if let Some(parent) = parent {
            raw_ast_edges.push((parent, raw));
        }
        if kind == NodeKind::Method {
            if let Some(full) = raw_nodes[raw].props.get("FULL_NAME") {
                methods_by_full.insert(full.clone(), raw);
                address_to_raw.entry(format!("M:{full}")).or_insert(raw);
            }
        }
        if kind == NodeKind::TypeDecl {
            if let Some(full) = raw_nodes[raw].props.get("FULL_NAME") {
                type_decls_by_full.insert(full.clone(), raw);
                address_to_raw.entry(format!("TD:{full}")).or_insert(raw);
            }
        }
        address_to_raw.insert(address, raw);
        ast_stack.push((depth, raw));
    }

    // SOURCE_FILE edges determine the incrementality partition of method ASTs.
    let source_files: HashMap<&str, &str> = raw_edges
        .iter()
        .filter(|edge| edge.kind == EdgeKind::SourceFile)
        .filter_map(|edge| {
            edge.target
                .strip_prefix("F:")
                .map(|file| (edge.source.as_str(), file))
        })
        .collect();

    let mut cpg = Cpg::new();
    cpg.file_id("<unknown>");
    cpg.file_id("<includes>");
    for (path, _) in sources {
        cpg.file_id(path);
    }

    let mut raw_to_node = Vec::with_capacity(raw_nodes.len());
    for (raw_index, raw) in raw_nodes.iter().enumerate() {
        let file = node_file(raw_index, &raw_nodes, &source_files).unwrap_or("<unknown>");
        let file_id = cpg.file_id(file);
        let node = cpg.add_node(raw.kind, file_id);
        apply_properties(&mut cpg, node, raw);
        raw_to_node.push(node);
    }

    for (parent, child) in raw_ast_edges {
        cpg.add_edge(raw_to_node[parent], raw_to_node[child], EdgeKind::Ast);
    }

    for edge in raw_edges {
        let source = resolve_address(&address_to_raw, &edge.source);
        let target = resolve_address(&address_to_raw, &edge.target);
        cpg.add_edge(raw_to_node[source], raw_to_node[target], edge.kind);
    }

    for layer in [Layer::SymbolRef, Layer::CallGraph, Layer::Cfg, Layer::Ddg] {
        cpg.mark_layer_authoritative(layer);
    }

    assign_source_lines(&mut cpg, sources);
    cpg
}

fn raw_subtree(root: usize, nodes: &[RawNode]) -> Vec<usize> {
    let depth = nodes[root].depth;
    let end = (root + 1..nodes.len())
        .find(|&index| nodes[index].depth <= depth)
        .unwrap_or(nodes.len());
    (root..end).collect()
}

/// Render the exact compatibility view from the shared graph. This is a graph
/// traversal, not a replay of the frontend text: nodes, properties, ordering,
/// and edges are all read back from `Cpg` after construction/passes.
pub fn canonical_dump(cpg: &Cpg) -> String {
    let mut roots: Vec<NodeId> = cpg
        .nodes()
        .filter(|&node| cpg.kind_of(node) == NodeKind::Method)
        .collect();
    roots.sort_by_key(|&node| (cpg.full_name_of(node).unwrap_or(""), node));

    let mut out = String::new();
    let mut ast_nodes = HashSet::new();
    let mut addresses = HashMap::new();
    for root in roots {
        let block = cpg.full_name_of(root).unwrap_or("<anonymous>");
        let mut index = 0usize;
        let mut block_seen = HashSet::new();
        render_ast(
            cpg,
            root,
            block,
            0,
            false,
            &mut index,
            &mut block_seen,
            &mut ast_nodes,
            &mut addresses,
            &mut out,
        );
        out.push('\n');
    }

    let mut scaffolding: Vec<NodeId> = cpg
        .nodes()
        .filter(|node| !ast_nodes.contains(node) || cpg.kind_of(*node) == NodeKind::TypeDecl)
        .collect();
    scaffolding.sort_by_key(|&node| scaffolding_key(cpg, node));
    for node in scaffolding {
        if let Some(address) = graph_external_address(cpg, node) {
            addresses.entry(node).or_insert(address);
        }
        out.push_str("NODES|");
        out.push_str(&render_scaffolding_node(cpg, node));
        out.push('\n');
    }

    let mut edges = BTreeSet::new();
    let mut flows = BTreeSet::new();
    let mut seen_flow_pairs = HashSet::new();
    for source in cpg.nodes() {
        for edge in cpg.out(source) {
            if matches!(
                edge.kind,
                EdgeKind::Ast | EdgeKind::Ddg | EdgeKind::Receiver
            ) {
                continue;
            }
            let source_address = addresses
                .get(&source)
                .unwrap_or_else(|| panic!("missing canonical address for {source:?}"));
            let target_address = addresses
                .get(&edge.other)
                .unwrap_or_else(|| panic!("missing canonical address for {:?}", edge.other));
            if edge.kind == EdgeKind::ReachingDef {
                flows.insert(format!(
                    "REACHING_DEF[{}] {} -> {}",
                    escape(&flow_variable(cpg, source)),
                    source_address,
                    target_address
                ));
                if !seen_flow_pairs.insert((source, edge.other))
                    && cpg.kind_of(source) == NodeKind::Block
                    && cpg.code_of(source).is_none()
                {
                    // Joern emits both the empty expression-block label and
                    // an unlabeled fact for this one duplicate endpoint.
                    flows.insert(format!(
                        "REACHING_DEF[] {} -> {}",
                        source_address, target_address
                    ));
                }
            } else {
                edges.insert(format!(
                    "{} {} -> {}",
                    canonical_edge_name(edge.kind),
                    source_address,
                    target_address
                ));
            }
        }
    }
    for edge in edges {
        out.push_str("EDGES|");
        out.push_str(&edge);
        out.push('\n');
    }
    for flow in flows {
        out.push_str("FLOWS|");
        out.push_str(&flow);
        out.push('\n');
    }
    out
}

fn scaffolding_key(cpg: &Cpg, node: NodeId) -> (u8, String, u32) {
    let category = match cpg.kind_of(node) {
        NodeKind::MetaData => 0,
        NodeKind::File => 1,
        NodeKind::NamespaceBlock => 2,
        NodeKind::Namespace => 3,
        NodeKind::TypeDecl => 4,
        NodeKind::Type => 5,
        kind => panic!("unexpected scaffolding node {kind:?}"),
    };
    let identity = if matches!(cpg.kind_of(node), NodeKind::TypeDecl | NodeKind::Type) {
        cpg.full_name_of(node).unwrap_or("").to_string()
    } else {
        String::new()
    };
    (category, identity, node.0)
}

#[allow(clippy::too_many_arguments)]
fn render_ast(
    cpg: &Cpg,
    node: NodeId,
    block: &str,
    depth: usize,
    foreign_method: bool,
    index: &mut usize,
    block_seen: &mut HashSet<NodeId>,
    ast_nodes: &mut HashSet<NodeId>,
    addresses: &mut HashMap<NodeId, String>,
    out: &mut String,
) {
    if !block_seen.insert(node) {
        return;
    }
    ast_nodes.insert(node);
    let address = format!("{block}#{}", *index);
    if depth == 0 || cpg.kind_of(node) == NodeKind::Method {
        addresses.entry(node).or_insert(address);
    } else if !foreign_method {
        addresses.insert(node, address);
    }
    *index += 1;
    out.push_str(&"  ".repeat(depth));
    out.push_str(canonical_node_name(cpg.kind_of(node)));
    append_property(&mut *out, "NAME", cpg.name_of(node));
    append_property(&mut *out, "CODE", cpg.code_of(node).map(escape).as_deref());
    append_property(&mut *out, "TYPE_FULL_NAME", cpg.type_full_name_of(node));
    if matches!(cpg.kind_of(node), NodeKind::Call | NodeKind::MethodRef) {
        append_property(&mut *out, "METHOD_FULL_NAME", cpg.full_name_of(node));
    } else {
        append_property(&mut *out, "FULL_NAME", cpg.full_name_of(node));
    }
    append_property(&mut *out, "SIGNATURE", cpg.signature_of(node));
    out.push_str(&format!(" ORDER={}", cpg.order_of(node)));
    if cpg.argument_index_of(node) >= 0 {
        out.push_str(&format!(" ARGUMENT_INDEX={}", cpg.argument_index_of(node)));
    }
    if cpg.kind_of(node) == NodeKind::Call {
        out.push_str(" DISPATCH_TYPE=");
        out.push_str(dispatch_type(cpg, node));
    }
    out.push('\n');
    let child_is_foreign = foreign_method || (depth > 0 && cpg.kind_of(node) == NodeKind::Method);
    for child in cpg.out_kind(node, EdgeKind::Ast) {
        render_ast(
            cpg,
            child,
            block,
            depth + 1,
            child_is_foreign,
            index,
            block_seen,
            ast_nodes,
            addresses,
            out,
        );
    }
}

fn render_scaffolding_node(cpg: &Cpg, node: NodeId) -> String {
    let kind = cpg.kind_of(node);
    let mut out = canonical_node_name(kind).to_string();
    match kind {
        NodeKind::MetaData => append_property(&mut out, "LANGUAGE", cpg.name_of(node)),
        NodeKind::File => {
            append_property(&mut out, "NAME", cpg.name_of(node));
            out.push_str(&format!(" ORDER={}", cpg.order_of(node)));
        }
        NodeKind::NamespaceBlock => {
            append_property(&mut out, "NAME", cpg.name_of(node));
            append_property(&mut out, "FULL_NAME", cpg.full_name_of(node));
            append_property(&mut out, "FILENAME", cpg.path_of(cpg.file_of(node)));
            out.push_str(&format!(" ORDER={}", cpg.order_of(node)));
        }
        NodeKind::Namespace => append_property(&mut out, "NAME", cpg.name_of(node)),
        NodeKind::Type => {
            append_property(&mut out, "NAME", cpg.name_of(node));
            append_property(&mut out, "FULL_NAME", cpg.full_name_of(node));
            append_property(&mut out, "TYPE_DECL_FULL_NAME", cpg.signature_of(node));
        }
        NodeKind::TypeDecl => render_type_decl(cpg, node, &mut out),
        _ => panic!("AST node escaped into scaffolding: {kind:?}"),
    }
    out
}

fn render_type_decl(cpg: &Cpg, node: NodeId, out: &mut String) {
    let name = cpg.name_of(node).unwrap_or("");
    let full = cpg.full_name_of(node).unwrap_or("");
    let code = cpg.code_of(node).unwrap_or("");
    let file = cpg.path_of(cpg.file_of(node)).unwrap_or("<unknown>");
    append_property(out, "NAME", Some(name));
    append_property(out, "FULL_NAME", Some(full));
    append_property(out, "CODE", Some(&escape(code)));
    let external = file == "<includes>";
    if external {
        out.push_str(" IS_EXTERNAL=true");
        out.push_str(" AST_PARENT_TYPE=NAMESPACE_BLOCK");
        out.push_str(" AST_PARENT_FULL_NAME=<includes>:<global>");
    } else if name == "<global>" {
        out.push_str(" AST_PARENT_TYPE=NAMESPACE_BLOCK");
        out.push_str(&format!(" AST_PARENT_FULL_NAME={full}"));
    } else if code == name || code.strip_prefix("<unresolvedNamespace>.") == Some(name) {
        out.push_str(" AST_PARENT_TYPE=TYPE_DECL");
        out.push_str(&format!(" AST_PARENT_FULL_NAME={file}:<global>"));
    } else {
        out.push_str(" AST_PARENT_TYPE= AST_PARENT_FULL_NAME=");
    }
    out.push_str(&format!(" FILENAME={file}"));
    if cpg.order_of(node) != 0 {
        out.push_str(&format!(" ORDER={}", cpg.order_of(node)));
    }
}

fn append_property(out: &mut String, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(value);
    }
}

fn escape(value: &str) -> String {
    value.replace('\n', "\\n").trim().to_string()
}

fn dispatch_type(cpg: &Cpg, node: NodeId) -> &'static str {
    if cpg.name_of(node) == Some("<operator>.pointerCall") {
        "DYNAMIC_DISPATCH"
    } else if cpg
        .full_name_of(node)
        .is_some_and(|full| full.matches(':').count() >= 2)
    {
        "INLINED"
    } else {
        "STATIC_DISPATCH"
    }
}

fn flow_variable(cpg: &Cpg, node: NodeId) -> String {
    if matches!(
        cpg.kind_of(node),
        NodeKind::MethodParameterIn | NodeKind::MethodParameterOut
    ) {
        cpg.name_of(node).unwrap_or("").to_string()
    } else {
        match cpg.kind_of(node) {
            NodeKind::Method => String::new(),
            NodeKind::Return => "<RET>".to_string(),
            NodeKind::Block if cpg.code_of(node).is_none() => "<empty>".to_string(),
            _ => cpg.code_of(node).unwrap_or("").to_string(),
        }
    }
}

fn graph_external_address(cpg: &Cpg, node: NodeId) -> Option<String> {
    let identity = cpg
        .full_name_of(node)
        .or_else(|| cpg.name_of(node))
        .unwrap_or("");
    match cpg.kind_of(node) {
        NodeKind::File => Some(format!("F:{identity}")),
        NodeKind::Namespace => Some(format!("NS:{identity}")),
        NodeKind::NamespaceBlock => Some(format!("NB:{identity}")),
        NodeKind::Type => Some(format!("T:{identity}")),
        NodeKind::TypeDecl => {
            let code = cpg.code_of(node).unwrap_or("");
            if cpg.path_of(cpg.file_of(node)) != Some("<includes>")
                && cpg.name_of(node) != Some("<global>")
                && code != cpg.name_of(node).unwrap_or("")
                && code.strip_prefix("<unresolvedNamespace>.") != cpg.name_of(node)
            {
                Some(format!("TD:{identity}"))
            } else {
                Some(format!("D:{identity}"))
            }
        }
        NodeKind::MetaData => None,
        _ => None,
    }
}

fn canonical_node_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::File => "FILE",
        NodeKind::Namespace => "NAMESPACE",
        NodeKind::NamespaceBlock => "NAMESPACE_BLOCK",
        NodeKind::Type => "TYPE",
        NodeKind::TypeDecl => "TYPE_DECL",
        NodeKind::MetaData => "META_DATA",
        NodeKind::Member => "MEMBER",
        NodeKind::Method => "METHOD",
        NodeKind::MethodParameterIn => "METHOD_PARAMETER_IN",
        NodeKind::MethodParameterOut => "METHOD_PARAMETER_OUT",
        NodeKind::MethodReturn => "METHOD_RETURN",
        NodeKind::Block => "BLOCK",
        NodeKind::Call => "CALL",
        NodeKind::Identifier => "IDENTIFIER",
        NodeKind::Literal => "LITERAL",
        NodeKind::Local => "LOCAL",
        NodeKind::FieldIdentifier => "FIELD_IDENTIFIER",
        NodeKind::ControlStructure => "CONTROL_STRUCTURE",
        NodeKind::Return => "RETURN",
        NodeKind::MethodRef => "METHOD_REF",
        NodeKind::TypeRef => "TYPE_REF",
        NodeKind::JumpTarget => "JUMP_TARGET",
        NodeKind::Modifier => "MODIFIER",
        NodeKind::Unknown => "UNKNOWN",
    }
}

fn canonical_edge_name(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Argument => "ARGUMENT",
        EdgeKind::Call => "CALL",
        EdgeKind::Cfg => "CFG",
        EdgeKind::Condition => "CONDITION",
        EdgeKind::Contains => "CONTAINS",
        EdgeKind::DoBody => "DO_BODY",
        EdgeKind::EvalType => "EVAL_TYPE",
        EdgeKind::FalseBody => "FALSE_BODY",
        EdgeKind::ForBody => "FOR_BODY",
        EdgeKind::ForInit => "FOR_INIT",
        EdgeKind::ForUpdate => "FOR_UPDATE",
        EdgeKind::ParameterLink => "PARAMETER_LINK",
        EdgeKind::Ref => "REF",
        EdgeKind::SourceFile => "SOURCE_FILE",
        EdgeKind::TrueBody => "TRUE_BODY",
        EdgeKind::Ast | EdgeKind::Ddg | EdgeKind::Receiver | EdgeKind::ReachingDef => {
            unreachable!("filtered before canonical edge rendering")
        }
    }
}

fn parse_node(line: &str) -> (NodeKind, HashMap<String, String>) {
    let label_end = line.find(' ').unwrap_or(line.len());
    let label = &line[..label_end];
    let kind = match label {
        "FILE" => NodeKind::File,
        "NAMESPACE" => NodeKind::Namespace,
        "NAMESPACE_BLOCK" => NodeKind::NamespaceBlock,
        "TYPE" => NodeKind::Type,
        "TYPE_DECL" => NodeKind::TypeDecl,
        "META_DATA" => NodeKind::MetaData,
        "MEMBER" => NodeKind::Member,
        "METHOD" => NodeKind::Method,
        "METHOD_PARAMETER_IN" => NodeKind::MethodParameterIn,
        "METHOD_PARAMETER_OUT" => NodeKind::MethodParameterOut,
        "METHOD_RETURN" => NodeKind::MethodReturn,
        "BLOCK" => NodeKind::Block,
        "CALL" => NodeKind::Call,
        "IDENTIFIER" => NodeKind::Identifier,
        "LITERAL" => NodeKind::Literal,
        "LOCAL" => NodeKind::Local,
        "FIELD_IDENTIFIER" => NodeKind::FieldIdentifier,
        "CONTROL_STRUCTURE" => NodeKind::ControlStructure,
        "RETURN" => NodeKind::Return,
        "METHOD_REF" => NodeKind::MethodRef,
        "TYPE_REF" => NodeKind::TypeRef,
        "JUMP_TARGET" => NodeKind::JumpTarget,
        "MODIFIER" => NodeKind::Modifier,
        "UNKNOWN" => NodeKind::Unknown,
        _ => panic!("unsupported exact C node label {label:?}"),
    };
    let props = parse_properties(&line[label_end..]);
    (kind, props)
}

fn parse_properties(rest: &str) -> HashMap<String, String> {
    let bytes = rest.as_bytes();
    let mut starts = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b' ' {
            let key_start = i + 1;
            let mut j = key_start;
            while j < bytes.len() && (bytes[j].is_ascii_uppercase() || bytes[j] == b'_') {
                j += 1;
            }
            if j > key_start && j < bytes.len() && bytes[j] == b'=' {
                starts.push((i, key_start, j));
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    let mut props = HashMap::new();
    for (index, &(_, key_start, equals)) in starts.iter().enumerate() {
        let value_start = equals + 1;
        let value_end = starts
            .get(index + 1)
            .map(|(space, _, _)| *space)
            .unwrap_or(rest.len());
        props.insert(
            rest[key_start..equals].to_string(),
            rest[value_start..value_end].trim_end().to_string(),
        );
    }
    props
}

fn parse_edge(line: &str) -> RawEdge {
    let (kind, rest) = line
        .split_once(' ')
        .unwrap_or_else(|| panic!("malformed exact edge: {line}"));
    let (source, target) = rest
        .split_once(" -> ")
        .unwrap_or_else(|| panic!("malformed exact edge endpoints: {line}"));
    RawEdge {
        kind: edge_kind(kind),
        source: source.to_string(),
        target: target.to_string(),
    }
}

fn parse_flow(line: &str) -> RawEdge {
    let split = line
        .rfind("] ")
        .unwrap_or_else(|| panic!("malformed exact flow: {line}"));
    let rest = &line[split + 2..];
    let (source, target) = rest
        .split_once(" -> ")
        .unwrap_or_else(|| panic!("malformed exact flow endpoints: {line}"));
    RawEdge {
        kind: EdgeKind::ReachingDef,
        source: source.to_string(),
        target: target.to_string(),
    }
}

fn edge_kind(label: &str) -> EdgeKind {
    match label {
        "ARGUMENT" => EdgeKind::Argument,
        "CALL" => EdgeKind::Call,
        "CFG" => EdgeKind::Cfg,
        "CONDITION" => EdgeKind::Condition,
        "CONTAINS" => EdgeKind::Contains,
        "DO_BODY" => EdgeKind::DoBody,
        "EVAL_TYPE" => EdgeKind::EvalType,
        "FALSE_BODY" => EdgeKind::FalseBody,
        "FOR_BODY" => EdgeKind::ForBody,
        "FOR_INIT" => EdgeKind::ForInit,
        "FOR_UPDATE" => EdgeKind::ForUpdate,
        "PARAMETER_LINK" => EdgeKind::ParameterLink,
        "REF" => EdgeKind::Ref,
        "SOURCE_FILE" => EdgeKind::SourceFile,
        "TRUE_BODY" => EdgeKind::TrueBody,
        _ => panic!("unsupported exact C edge label {label:?}"),
    }
}

fn external_address(kind: NodeKind, props: &HashMap<String, String>) -> Option<String> {
    let get = |key| props.get(key).map(String::as_str).unwrap_or("");
    match kind {
        NodeKind::File => Some(format!("F:{}", get("NAME"))),
        NodeKind::Namespace => Some(format!("NS:{}", get("NAME"))),
        NodeKind::NamespaceBlock => Some(format!("NB:{}", get("FULL_NAME"))),
        NodeKind::Type => Some(format!("T:{}", get("FULL_NAME"))),
        NodeKind::TypeDecl => Some(format!("D:{}", get("FULL_NAME"))),
        NodeKind::MetaData => None,
        _ => None,
    }
}

fn address_aliases(
    kind: NodeKind,
    props: &HashMap<String, String>,
    address: Option<&str>,
) -> Vec<String> {
    let mut aliases = address.into_iter().map(str::to_string).collect::<Vec<_>>();
    if kind == NodeKind::TypeDecl {
        if let Some(full) = props.get("FULL_NAME") {
            aliases.push(format!("TD:{full}"));
        }
    }
    aliases
}

fn resolve_address(addresses: &HashMap<String, usize>, address: &str) -> usize {
    *addresses
        .get(address)
        .unwrap_or_else(|| panic!("exact C edge references unknown address {address:?}"))
}

fn node_file<'a>(
    raw: usize,
    nodes: &'a [RawNode],
    source_files: &HashMap<&str, &'a str>,
) -> Option<&'a str> {
    let node = &nodes[raw];
    if node.kind == NodeKind::File {
        return node.props.get("NAME").map(String::as_str);
    }
    if let Some(filename) = node.props.get("FILENAME") {
        return Some(filename);
    }
    if let Some(address) = node.address.as_deref() {
        if let Some(file) = source_files.get(address) {
            return Some(file);
        }
    }
    let mut parent = node.parent;
    while let Some(index) = parent {
        if let Some(address) = nodes[index].address.as_deref() {
            if let Some(file) = source_files.get(address) {
                return Some(file);
            }
        }
        parent = nodes[index].parent;
    }
    None
}

fn apply_properties(cpg: &mut Cpg, node: NodeId, raw: &RawNode) {
    let intern = |cpg: &mut Cpg, value: &str| cpg.intern(value);
    if let Some(value) = raw.props.get("NAME") {
        let sym = intern(cpg, value);
        cpg.set_name(node, sym);
    }
    if let Some(value) = raw.props.get("CODE") {
        let sym = intern(cpg, &value.replace("\\n", "\n"));
        cpg.set_code(node, sym);
    }
    if let Some(value) = raw.props.get("TYPE_FULL_NAME") {
        let sym = intern(cpg, value);
        cpg.set_type_full_name(node, sym);
    }
    let full_key = if matches!(raw.kind, NodeKind::Call | NodeKind::MethodRef) {
        "METHOD_FULL_NAME"
    } else {
        "FULL_NAME"
    };
    if let Some(value) = raw.props.get(full_key) {
        let sym = intern(cpg, value);
        cpg.set_full_name(node, sym);
    }
    if let Some(value) = raw.props.get("SIGNATURE") {
        let sym = intern(cpg, value);
        cpg.set_signature(node, sym);
    } else if raw.kind == NodeKind::Type {
        if let Some(value) = raw.props.get("TYPE_DECL_FULL_NAME") {
            let sym = intern(cpg, value);
            cpg.set_signature(node, sym);
        }
    }
    if raw.kind == NodeKind::MetaData {
        if let Some(value) = raw.props.get("LANGUAGE") {
            let sym = intern(cpg, value);
            cpg.set_name(node, sym);
        }
    }
    if let Some(value) = raw.props.get("ORDER") {
        cpg.set_order(
            node,
            value
                .parse()
                .unwrap_or_else(|_| panic!("invalid exact ORDER {value:?}")),
        );
    }
    if let Some(value) = raw.props.get("ARGUMENT_INDEX") {
        cpg.set_argument_index(
            node,
            value
                .parse()
                .unwrap_or_else(|_| panic!("invalid exact ARGUMENT_INDEX {value:?}")),
        );
    }
}

/// Recover locations without coupling unrelated methods to graph insertion
/// order. Method CODE preserves the parsed function text; anchor it first, then
/// match complete tokens inside that source range. Children may overlap their
/// parent's expression, while ordinary siblings consume source monotonically.
fn assign_source_lines(cpg: &mut Cpg, sources: &[(String, String)]) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_c::LANGUAGE.into())
        .unwrap();
    for (path, source) in sources {
        let file = cpg.file_id(path);
        let mut tokens = source_tokens(source);
        if tokens.is_empty() {
            continue;
        }
        let tree = parser.parse(source, None).unwrap();
        // Array allocations and explicit initializers can describe the same
        // declarator, and all allocations precede all initializers in the AST.
        // Mark parsed declaration spans so each view can find its actual source
        // without matching an identically spelled expression inside a dimension.
        let mut pending = vec![tree.root_node()];
        while let Some(node) = pending.pop() {
            let mut cursor = node.walk();
            if node.kind() == "declaration" {
                for declarator in node.children_by_field_name("declarator", &mut cursor) {
                    let start =
                        tokens.partition_point(|token| token.start < declarator.start_byte());
                    let end = tokens.partition_point(|token| token.start < declarator.end_byte());
                    if start < end {
                        tokens[start].declarator_len = Some(end - start);
                    }
                }
            }
            pending.extend(node.named_children(&mut cursor));
        }
        let function_nodes: Vec<_> = crate::exact::translation_unit_items(tree.root_node())
            .into_iter()
            .filter(|node| node.kind() == "function_definition")
            .collect();
        let mut occurrences: HashMap<String, usize> = HashMap::new();
        let functions: HashMap<_, _> = function_nodes
            .iter()
            .filter_map(|node| {
                let name = crate::exact::function_name(*node, source.as_bytes())?;
                let occurrence = occurrences.entry(name.clone()).or_default();
                let identity = (name, *occurrence);
                *occurrence += 1;
                // The canonical transport decodes both escaped newlines and
                // source newlines; apply the same decoding to the anchor.
                let code = source[node.byte_range()].replace("\\n", "\n");
                let start = tokens.partition_point(|token| token.start < node.start_byte());
                let end = tokens.partition_point(|token| token.start < node.end_byte());
                Some((identity, (code.trim().to_owned(), start..end)))
            })
            .collect();
        let methods: Vec<_> = cpg
            .nodes_in_file(file)
            .iter()
            .copied()
            .filter(|&node| cpg.kind_of(node) == NodeKind::Method)
            .collect();
        for &method in &methods {
            let Some(name) = cpg.name_of(method) else {
                continue;
            };
            // Same-name preprocessor alternatives can have identical CODE.
            // Their canonical duplicate suffix preserves declaration order.
            let occurrence = cpg
                .full_name_of(method)
                .and_then(|full| full.rsplit_once("<duplicate>"))
                .and_then(|(_, suffix)| suffix.parse::<usize>().ok())
                .map_or(0, |index| index + 1);
            let Some((code, range)) = functions.get(&(name.to_owned(), occurrence)) else {
                continue;
            };
            if cpg.code_of(method).map(str::trim) != Some(code.as_str()) {
                continue;
            }
            locate_ast(
                cpg,
                method,
                &tokens,
                range.clone(),
                tokens[range.start].line,
            );
        }
        // Global declarations have their own source stream. Excluding function
        // definitions prevents a repeated initializer from matching a local one.
        let global_tokens: Vec<_> = tokens
            .into_iter()
            .filter(|token| {
                let index = function_nodes.partition_point(|node| node.end_byte() <= token.start);
                !function_nodes
                    .get(index)
                    .is_some_and(|node| node.start_byte() <= token.start)
            })
            .collect();
        if !global_tokens.is_empty() {
            for &method in &methods {
                if cpg.name_of(method) == Some("<global>") {
                    locate_ast(cpg, method, &global_tokens, 0..global_tokens.len(), 1);
                }
            }
        }
        let method_lines: HashMap<_, _> = methods
            .iter()
            .filter_map(|&method| {
                Some((cpg.full_name_of(method)?.to_owned(), cpg.line_of(method)?))
            })
            .collect();
        let refs: Vec<_> = cpg
            .nodes_in_file(file)
            .iter()
            .copied()
            .filter(|&node| cpg.kind_of(node) == NodeKind::MethodRef)
            .collect();
        for node in refs {
            if let Some(&line) = cpg
                .type_full_name_of(node)
                .and_then(|full| method_lines.get(full))
            {
                cpg.set_line(node, line);
            }
        }
    }
}

struct SourceToken<'a> {
    text: std::borrow::Cow<'a, str>,
    start: usize,
    line: u32,
    declarator_len: Option<usize>,
}

/// Lex only enough to recover source spans: comments are ignored, quoted
/// literals are indivisible, and identifiers never match inside other names.
/// Compound C operators remain indivisible; spacing between tokens is ignored.
fn source_tokens(source: &str) -> Vec<SourceToken<'_>> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    let mut line = 1;
    while i < bytes.len() {
        let start = i;
        let token_line = line;
        let mut comment = false;
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c => {
                line += u32::from(bytes[i] == b'\n');
                i += 1;
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                comment = true;
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                comment = true;
                i += 2;
                while i < bytes.len() && !bytes[i..].starts_with(b"*/") {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            quote @ (b'\'' | b'"') => {
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
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | 0x80..=0xff => {
                i += 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] >= 0x80)
                {
                    i += 1;
                }
            }
            _ => {
                const OPERATORS: &[&[u8]] = &[
                    b"<<=", b">>=", b"...", b"->", b"++", b"--", b"<<", b">>", b"<=", b">=", b"==",
                    b"!=", b"&&", b"||", b"*=", b"/=", b"%=", b"+=", b"-=", b"&=", b"^=", b"|=",
                    b"##",
                ];
                i += OPERATORS
                    .iter()
                    .find(|operator| bytes[i..].starts_with(operator))
                    .map_or(1, |operator| operator.len());
            }
        }
        line += bytes[start..i]
            .iter()
            .filter(|&&byte| byte == b'\n')
            .count() as u32;
        if !comment {
            let text = &source[start..i];
            tokens.push(SourceToken {
                text: if text.contains("\\n") {
                    text.replace("\\n", "\n").into()
                } else {
                    text.into()
                },
                start,
                line: token_line,
                declarator_len: None,
            });
        }
    }
    tokens
}

fn locate_ast(
    cpg: &mut Cpg,
    node: NodeId,
    tokens: &[SourceToken<'_>],
    range: std::ops::Range<usize>,
    inherited_line: u32,
) -> usize {
    let kind = cpg.kind_of(node);
    let code = cpg.code_of(node).unwrap_or("");
    let keyword_only = kind == NodeKind::ControlStructure && code == "else";
    let header_only = kind == NodeKind::ControlStructure
        && (code.starts_with("while ") || code.starts_with("for ("));
    let needle = source_tokens(code);
    let matched = if needle.is_empty() || kind == NodeKind::MethodReturn {
        None
    } else {
        tokens[range.clone()]
            .windows(needle.len())
            .position(|window| {
                window
                    .iter()
                    .zip(&needle)
                    .all(|(left, right)| left.text == right.text)
            })
            .map(|offset| range.start + offset..range.start + offset + needle.len())
    };
    let line = matched
        .as_ref()
        .map_or(inherited_line, |span| tokens[span.start].line);
    let synthetic_truth_test = matched.is_none()
        && cpg.name_of(node) == Some("<operator>.notEquals")
        && cpg.in_kind(node, EdgeKind::Condition).next().is_some();
    // Synthesized or transformed nodes use the closest located AST ancestor;
    // they cannot redirect later source searches or escape the owning method.
    cpg.set_line(node, line);
    // `else` has only keyword CODE, but its children occupy the following
    // statement. A CODE-less synthetic block likewise borrows its parent range.
    let child_range = if keyword_only {
        matched
            .as_ref()
            .map_or(range.clone(), |span| span.end..range.end)
    } else if header_only {
        matched
            .as_ref()
            .map_or(range.clone(), |span| span.start..range.end)
    } else {
        matched.clone().unwrap_or_else(|| range.clone())
    };
    let mut next = child_range.start;
    // Each declaration view advances independently: an alloc for a later
    // declarator must not consume an earlier scalar/array initializer. The
    // ordinary statement cursor still advances past both views.
    let mut declaration_next = [child_range.start; 2];
    let mut children: Vec<_> = cpg.out_kind(node, EdgeKind::Ast).collect();
    children.sort_by_key(|&child| (cpg.order_of(child), child));
    for child in children {
        if cpg.kind_of(child) == NodeKind::Method {
            continue;
        }
        // A normalized identifier condition introduces a zero/NULL literal
        // absent from the source. Searching for it can consume a later loop
        // update or body and move every following statement's location.
        if synthetic_truth_test
            && cpg.kind_of(child) == NodeKind::Literal
            && cpg.argument_index_of(child) == 2
        {
            cpg.set_line(child, line);
            continue;
        }
        let declaration_span = if cpg.kind_of(child) == NodeKind::Call
            && cpg.name_of(child) == Some("<operator>.assignment")
            && cpg.type_full_name_of(child) == Some("void")
        {
            let role = usize::from(
                cpg.out_kind(child, EdgeKind::Ast)
                    .any(|argument| cpg.name_of(argument) == Some("<operator>.alloc")),
            );
            let needle = source_tokens(cpg.code_of(child).unwrap_or(""));
            let span = (!needle.is_empty())
                .then(|| {
                    tokens[declaration_next[role]..child_range.end]
                        .windows(needle.len())
                        .position(|window| {
                            window[0].declarator_len == Some(needle.len())
                                && window.iter().zip(&needle).all(|(a, b)| a.text == b.text)
                        })
                        .map(|offset| {
                            declaration_next[role] + offset
                                ..declaration_next[role] + offset + needle.len()
                        })
                })
                .flatten();
            if let Some(span) = &span {
                declaration_next[role] = span.end;
            }
            span
        } else {
            None
        };
        let is_declaration_view = declaration_span.is_some();
        let end = locate_ast(
            cpg,
            child,
            tokens,
            declaration_span.unwrap_or(next..child_range.end),
            line,
        );
        // LOCAL and mirrored parameters describe overlapping source with the
        // assignment/parameter nodes that follow, and do not consume it.
        if !matches!(
            cpg.kind_of(child),
            NodeKind::Local
                | NodeKind::MethodParameterIn
                | NodeKind::MethodParameterOut
                | NodeKind::JumpTarget
        ) {
            next = next.max(end);
            if !is_declaration_view {
                declaration_next
                    .iter_mut()
                    .for_each(|position| *position = (*position).max(next));
            }
        }
    }
    matched.map_or(next, |span| span.end.max(next))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn import_source(source: &str) -> Cpg {
        let sources = vec![("locations.c".into(), source.into())];
        graph_from_canonical_dump(&crate::exact::canonical_dump_sources(&sources), &sources)
    }

    fn method_nodes(cpg: &Cpg, name: &str) -> Vec<NodeId> {
        let method = cpg
            .nodes()
            .find(|&n| {
                cpg.kind_of(n) == NodeKind::Method
                    && cpg.name_of(n) == Some(name)
                    && cpg
                        .out_kind(n, EdgeKind::SourceFile)
                        .next()
                        .is_some_and(|file| cpg.name_of(file) == Some("locations.c"))
            })
            .unwrap();
        let mut nodes = vec![method];
        let mut index = 0;
        while index < nodes.len() {
            nodes.extend(
                cpg.out_kind(nodes[index], EdgeKind::Ast)
                    .filter(|&node| cpg.kind_of(node) != NodeKind::Method),
            );
            index += 1;
        }
        nodes
    }

    #[test]
    fn source_lines_keep_lua_sweep2old_in_its_own_method() {
        // Reduced from Lua 5.4.7 lgc.c: the final assignment jumped back to
        // sweeplist after a braceless branch added nodes to the graph.
        let source = r#"#define maskcolors (bitmask(BLACKBIT) | WHITEBITS)
#define set2gray(x) resetbits(x->marked, maskcolors)
static void sweeplist(GCObject **p, GCObject *curr) {
  if (curr) { }
  else {
    p = &curr->next;
  }
}
static void separatetobefnz(GCObject **p, GCObject *curr, int all) {
  if (!all)
    p = &curr->next;
}
static void sweep2old (lua_State *L, GCObject **p) {
  GCObject *curr;
  global_State *g = G(L);
  while ((curr = *p) != NULL) {
    if (iswhite(curr)) {
      lua_assert(isdead(g, curr));
      *p = curr->next;
      freeobj(L, curr);
    }
    else {
      setage(curr, G_OLD);
      if (curr->tt == LUA_VTHREAD) {
        lua_State *th = gco2th(curr);
        linkgclist(th, g->grayagain);
      }
      else if (curr->tt == LUA_VUPVAL && upisopen(gco2upv(curr)))
        set2gray(curr);
      else
        nw2black(curr);
      p = &curr->next;  /* sweep2old target */
    }
  }
}
"#;
        let cpg = import_source(source);
        let assignment = method_nodes(&cpg, "sweep2old")
            .into_iter()
            .find(|&n| {
                cpg.kind_of(n) == NodeKind::Call && cpg.code_of(n) == Some("p = &curr->next")
            })
            .unwrap();
        let expected = source
            .lines()
            .position(|line| line.contains("sweep2old target"))
            .unwrap() as u32
            + 1;
        assert_eq!(cpg.line_of(assignment), Some(expected));
        for name in ["sweeplist", "separatetobefnz", "sweep2old"] {
            let start = source
                .lines()
                .position(|line| line.starts_with(&format!("static void {name}")))
                .unwrap() as u32
                + 1;
            for node in method_nodes(&cpg, name) {
                assert!(
                    cpg.line_of(node).is_none_or(|line| line >= start),
                    "{name}: {:?} {:?} {:?}",
                    cpg.kind_of(node),
                    cpg.code_of(node),
                    cpg.line_of(node)
                );
            }
        }
    }

    #[test]
    fn source_lines_match_tokens_and_repeated_statements_monotonically() {
        let source = r#"int target(int x) {
  /* x = x + 1; */
  const char *text = "x = x + 1;";
  int prefix_x = 0;
  x = x + 1;
  x = x + 1;
  return x;
}
int alpha(int x) { return x; }
"#;
        let cpg = import_source(source);
        let mut assignments: Vec<_> = method_nodes(&cpg, "target")
            .into_iter()
            .filter(|&n| cpg.kind_of(n) == NodeKind::Call && cpg.code_of(n) == Some("x = x + 1"))
            .map(|n| cpg.line_of(n))
            .collect();
        assignments.sort();
        assert_eq!(assignments, vec![Some(5), Some(6)]);
        for node in method_nodes(&cpg, "target") {
            if cpg.kind_of(node) == NodeKind::Identifier && cpg.code_of(node) == Some("x") {
                assert!(
                    matches!(cpg.line_of(node), Some(5..=7)),
                    "identifier at {:?}",
                    cpg.line_of(node)
                );
            }
        }
    }

    #[test]
    fn source_lines_preserve_overlapping_parameters_declarations_and_children() {
        let cpg = import_source("int target(int x) {\n  int value = x;\n  return value;\n}\n");
        for node in method_nodes(&cpg, "target") {
            if matches!(
                cpg.kind_of(node),
                NodeKind::MethodParameterIn | NodeKind::MethodParameterOut
            ) {
                assert_eq!(cpg.line_of(node), Some(1));
            }
            if cpg.kind_of(node) == NodeKind::Call && cpg.code_of(node) == Some("value = x") {
                assert_eq!(cpg.line_of(node), Some(2));
                for child in cpg.out_kind(node, EdgeKind::Ast) {
                    assert_eq!(cpg.line_of(child), Some(2));
                }
            }
        }
    }

    #[test]
    fn source_lines_unmatched_nodes_inherit_without_consuming_siblings() {
        let source = "int target(int x) {\n  if (x > 0)\n    unknown(x);\n  return x;\n}\n";
        let cpg = import_source(source);
        let nodes = method_nodes(&cpg, "target");
        let synthetic_block = nodes
            .iter()
            .copied()
            .find(|&node| cpg.kind_of(node) == NodeKind::Block && cpg.code_of(node).is_none())
            .unwrap();
        assert_eq!(cpg.line_of(synthetic_block), Some(2));
        let returned = nodes
            .iter()
            .copied()
            .find(|&node| cpg.kind_of(node) == NodeKind::Return)
            .unwrap();
        assert_eq!(cpg.line_of(returned), Some(4));
        let call = nodes
            .iter()
            .copied()
            .find(|&node| {
                cpg.kind_of(node) == NodeKind::Call && cpg.code_of(node) == Some("unknown(x)")
            })
            .unwrap();
        assert_eq!(cpg.line_of(call), Some(3));
    }

    #[test]
    fn source_lines_keep_global_declarations_outside_function_bodies() {
        let source = "int before = 1;\nint target(void) {\n  int after = 2;\n  return after;\n}\nint after = 2;\n";
        let cpg = import_source(source);
        let assignments: Vec<_> = method_nodes(&cpg, "<global>")
            .into_iter()
            .filter(|&node| {
                cpg.kind_of(node) == NodeKind::Call && cpg.code_of(node) == Some("after = 2")
            })
            .collect();
        assert_eq!(assignments.len(), 1);
        assert_eq!(cpg.line_of(assignments[0]), Some(6));
        let method_ref = cpg
            .nodes()
            .find(|&node| {
                cpg.kind_of(node) == NodeKind::MethodRef && cpg.code_of(node) == Some("target")
            })
            .unwrap();
        assert_eq!(cpg.line_of(method_ref), Some(2));
    }

    #[test]
    fn source_lines_anchor_conditional_and_identical_duplicate_definitions() {
        let source = "#if 0\nint repeated(int x) { return x; }\n#else\nint repeated(int x) { return x; }\n#endif\n#if 1\nint target(void) {\n  int after = 2;\n  return after;\n}\n#endif\nint after = 2;\n";
        let cpg = import_source(source);
        for (full, line) in [("repeated", 2), ("repeated<duplicate>0", 4), ("target", 7)] {
            let method = cpg
                .nodes()
                .find(|&node| {
                    cpg.kind_of(node) == NodeKind::Method && cpg.full_name_of(node) == Some(full)
                })
                .unwrap();
            assert_eq!(cpg.line_of(method), Some(line), "{full}");
        }
        let assignments: Vec<_> = method_nodes(&cpg, "<global>")
            .into_iter()
            .filter(|&node| {
                cpg.kind_of(node) == NodeKind::Call && cpg.code_of(node) == Some("after = 2")
            })
            .collect();
        assert_eq!(assignments.len(), 1);
        assert_eq!(cpg.line_of(assignments[0]), Some(12));
    }

    #[test]
    fn imports_every_exact_schema_kind_and_edge_without_loss() {
        let sources = vec![(
            "a.c".to_string(),
            "int main(void) { int x = 1; return x; }".to_string(),
        )];
        let dump = crate::exact::canonical_dump_sources(&sources);
        let cpg = graph_from_canonical_dump(&dump, &sources);
        assert!(cpg.nodes().any(|n| cpg.kind_of(n) == NodeKind::Method));
        assert!(cpg
            .nodes()
            .any(|n| cpg.kind_of(n) == NodeKind::MethodParameterOut));
        assert!(cpg
            .nodes()
            .any(|n| cpg.out_kind(n, EdgeKind::SourceFile).next().is_some()));
        assert!(cpg
            .nodes()
            .any(|n| cpg.out_kind(n, EdgeKind::ReachingDef).next().is_some()));
        let without_flows = |text: &str| {
            text.lines()
                .filter(|line| !line.starts_with("FLOWS|"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(without_flows(&canonical_dump(&cpg)), without_flows(&dump));
    }
}
