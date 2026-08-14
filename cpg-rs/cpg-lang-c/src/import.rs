//! Import the exact C lowering into the shared production graph.
//!
//! The exact lowerer emits a deterministic neutral text form. This adapter is
//! intentionally strict: every node and edge label must map to the shared
//! schema, and every edge address must resolve. That makes schema omissions a
//! build failure instead of silently dropping semantics.

use cpg_core::{Cpg, EdgeKind, NodeId, NodeKind};
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Debug)]
struct RawNode {
    kind: NodeKind,
    props: HashMap<String, String>,
    address: Option<String>,
    parent: Option<usize>,
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
    let mut address_to_raw = HashMap::new();
    let mut ast_stack: Vec<(usize, usize)> = Vec::new();
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
            let raw = raw_nodes.len();
            raw_nodes.push(RawNode {
                kind,
                props,
                address: address.clone(),
                parent: None,
            });
            for alias in address_aliases(kind, &raw_nodes[raw].props, address.as_deref()) {
                address_to_raw.entry(alias).or_insert(raw);
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
        while ast_stack.last().is_some_and(|(d, _)| *d >= depth) {
            ast_stack.pop();
        }
        let parent = ast_stack.last().map(|(_, raw)| *raw);
        let address = format!("{block}#{block_line}");
        block_line += 1;
        let raw = raw_nodes.len();
        raw_nodes.push(RawNode {
            kind,
            props,
            address: Some(address.clone()),
            parent,
        });
        if kind == NodeKind::Method {
            if let Some(full) = raw_nodes[raw].props.get("FULL_NAME") {
                address_to_raw.entry(format!("M:{full}")).or_insert(raw);
            }
        }
        if kind == NodeKind::TypeDecl {
            if let Some(full) = raw_nodes[raw].props.get("FULL_NAME") {
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

    for (raw, node) in raw_nodes.iter().zip(raw_to_node.iter().copied()) {
        if let Some(parent) = raw.parent {
            cpg.add_edge(raw_to_node[parent], node, EdgeKind::Ast);
        }
    }

    for edge in raw_edges {
        let source = resolve_address(&address_to_raw, &edge.source);
        let target = resolve_address(&address_to_raw, &edge.target);
        cpg.add_edge(raw_to_node[source], raw_to_node[target], edge.kind);
    }

    assign_source_lines(&mut cpg, sources);
    cpg
}

/// Render the exact compatibility view from the shared graph. This is a graph
/// traversal, not a replay of the frontend text: nodes, properties, ordering,
/// and edges are all read back from `Cpg` after construction/passes.
pub fn canonical_dump(cpg: &Cpg) -> String {
    let mut roots: Vec<NodeId> = cpg
        .nodes()
        .filter(|&node| {
            cpg.kind_of(node) == NodeKind::Method
                && cpg.in_kind(node, EdgeKind::Ast).next().is_none()
        })
        .collect();
    roots.sort_by_key(|&node| (cpg.full_name_of(node).unwrap_or(""), node));

    let mut out = String::new();
    let mut emitted = HashSet::new();
    let mut addresses = HashMap::new();
    for root in roots {
        let block = cpg.full_name_of(root).unwrap_or("<anonymous>");
        let mut index = 0usize;
        render_ast(
            cpg,
            root,
            block,
            0,
            &mut index,
            &mut emitted,
            &mut addresses,
            &mut out,
        );
        out.push('\n');
    }

    for node in cpg.nodes().filter(|node| !emitted.contains(node)) {
        if let Some(address) = graph_external_address(cpg, node) {
            addresses.insert(node, address);
        }
        out.push_str("NODES|");
        out.push_str(&render_scaffolding_node(cpg, node));
        out.push('\n');
    }

    let mut edges = BTreeSet::new();
    let mut flows = BTreeSet::new();
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
                    flow_variable(cpg, source),
                    source_address,
                    target_address
                ));
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

#[allow(clippy::too_many_arguments)]
fn render_ast(
    cpg: &Cpg,
    node: NodeId,
    block: &str,
    depth: usize,
    index: &mut usize,
    emitted: &mut HashSet<NodeId>,
    addresses: &mut HashMap<NodeId, String>,
    out: &mut String,
) {
    if !emitted.insert(node) {
        return;
    }
    addresses.insert(node, format!("{block}#{}", *index));
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
    for child in cpg.out_kind(node, EdgeKind::Ast) {
        render_ast(cpg, child, block, depth + 1, index, emitted, addresses, out);
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
    } else if code == name {
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
    } else if cpg.kind_of(node) == NodeKind::Block && cpg.code_of(node).is_none() {
        "<empty>".to_string()
    } else {
        cpg.code_of(node).unwrap_or("").to_string()
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
    let (_, rest) = line
        .split_once("] ")
        .unwrap_or_else(|| panic!("malformed exact flow: {line}"));
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

/// Recover source locations for the analysis-facing graph. The exact oracle
/// intentionally omits location properties, so match canonical code back to
/// the owning source in stable source order. Ambiguous repeated snippets use
/// the first occurrence at or after the previous matched line.
fn assign_source_lines(cpg: &mut Cpg, sources: &[(String, String)]) {
    for (path, source) in sources {
        let file = cpg.file_id(path);
        let lines: Vec<&str> = source.lines().collect();
        let nodes = cpg.nodes_in_file(file).to_vec();
        let mut cursor = 0usize;
        for node in nodes {
            let needle = cpg
                .code_of(node)
                .or_else(|| cpg.name_of(node))
                .unwrap_or("")
                .trim();
            if needle.is_empty() || needle.starts_with('<') {
                continue;
            }
            let first = needle.lines().next().unwrap_or(needle).trim();
            if let Some(offset) = lines[cursor..]
                .iter()
                .position(|line| line.contains(first))
                .or_else(|| lines.iter().position(|line| line.contains(first)))
            {
                let line = if lines[cursor..].iter().any(|line| line.contains(first)) {
                    cursor + offset
                } else {
                    offset
                };
                cpg.set_line(node, line as u32 + 1);
                cursor = line;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
