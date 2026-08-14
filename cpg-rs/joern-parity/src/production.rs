//! Canonical adapter for the shipped C graph.
//!
//! This deliberately constructs a `cpg_incremental::Project` with
//! `cpg_lang_c::CFrontend` and `cpg_analysis::standard_pipeline`, exactly as
//! the released CLI does. During convergence the historical standalone dump
//! remains the required oracle path; `--migration-report` makes every
//! difference visible without normalising semantic gaps away.

use cpg_core::{Cpg, EdgeKind, NodeId, NodeKind};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

#[derive(Clone, Copy)]
pub enum Mode {
    Production,
    MigrationReport,
}

pub fn dump_paths(paths: &[String]) -> String {
    let sources: Vec<(String, String)> = paths
        .iter()
        .map(|path| {
            let source = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("read production parity input {path}: {e}"));
            let name = Path::new(path)
                .file_name()
                .unwrap_or_else(|| panic!("input has no filename: {path}"))
                .to_string_lossy()
                .into_owned();
            (name, source)
        })
        .collect();
    dump_sources(&sources)
}

pub fn dump_sources(sources: &[(String, String)]) -> String {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    let refs: Vec<(&str, &str)> = sources
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    project.build(&refs);
    canonical_dump(&project.cpg)
}

/// Render a stable Joern-oracle-shaped view of a production graph.
///
/// The format intentionally exposes missing node kinds and properties: absent
/// production facts stay absent and therefore remain visible in exact diffs.
pub fn canonical_dump(cpg: &Cpg) -> String {
    let mut methods: Vec<NodeId> = cpg
        .nodes()
        .filter(|&node| cpg.kind_of(node) == NodeKind::Method)
        .collect();
    methods.sort_by_key(|&node| {
        (
            cpg.full_name_of(node).unwrap_or(""),
            cpg.path_of(cpg.file_of(node)).unwrap_or(""),
            node,
        )
    });

    let mut out = String::new();
    let mut addresses: HashMap<NodeId, String> = HashMap::new();
    let mut emitted = HashSet::new();
    for method in methods {
        let block = cpg
            .full_name_of(method)
            .or_else(|| cpg.name_of(method))
            .unwrap_or("<anonymous>");
        let mut line = 0usize;
        render_ast(
            cpg,
            method,
            block,
            0,
            &mut line,
            &mut addresses,
            &mut emitted,
            &mut out,
        );
        out.push('\n');
    }

    // Nodes outside method ASTs are reported separately, as in oracle_all.
    let mut nodes: Vec<NodeId> = cpg.nodes().filter(|n| !emitted.contains(n)).collect();
    nodes.sort_by_key(|&node| stable_node_key(cpg, node));
    for node in nodes {
        addresses
            .entry(node)
            .or_insert_with(|| external_address(cpg, node));
        out.push_str("NODES|");
        out.push_str(&render_node(cpg, node, 0));
        out.push('\n');
    }

    let mut edge_lines = BTreeSet::new();
    let mut flow_lines = BTreeSet::new();
    for src in cpg.nodes() {
        for edge in cpg.out(src) {
            let Some(src_addr) = addresses.get(&src) else {
                continue;
            };
            let Some(dst_addr) = addresses.get(&edge.other) else {
                continue;
            };
            match edge.kind {
                EdgeKind::Ast | EdgeKind::Ddg | EdgeKind::Receiver => {}
                EdgeKind::ReachingDef => {
                    let variable = cpg
                        .name_of(src)
                        .or_else(|| cpg.code_of(src))
                        .unwrap_or("ANY");
                    flow_lines.insert(format!(
                        "REACHING_DEF[{}] {} -> {}",
                        escape(variable),
                        src_addr,
                        dst_addr
                    ));
                }
                kind => {
                    edge_lines.insert(format!("{} {} -> {}", edge_name(kind), src_addr, dst_addr));
                }
            }
        }
    }
    for line in edge_lines {
        out.push_str("EDGES|");
        out.push_str(&line);
        out.push('\n');
    }
    for line in flow_lines {
        out.push_str("FLOWS|");
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn render_ast(
    cpg: &Cpg,
    node: NodeId,
    block: &str,
    depth: usize,
    line: &mut usize,
    addresses: &mut HashMap<NodeId, String>,
    emitted: &mut HashSet<NodeId>,
    out: &mut String,
) {
    if !emitted.insert(node) {
        return;
    }
    addresses.insert(node, format!("{block}#{}", *line));
    *line += 1;
    out.push_str(&render_node(cpg, node, depth));
    out.push('\n');

    let mut children: Vec<NodeId> = cpg.out_kind(node, EdgeKind::Ast).collect();
    children.sort_by_key(|&child| (cpg.order_of(child), cpg.argument_index_of(child), child));
    for child in children {
        render_ast(cpg, child, block, depth + 1, line, addresses, emitted, out);
    }
}

fn render_node(cpg: &Cpg, node: NodeId, depth: usize) -> String {
    let mut out = format!("{}{}", "  ".repeat(depth), node_name(cpg.kind_of(node)));
    push_text(&mut out, "NAME", cpg.name_of(node));
    push_text(&mut out, "CODE", cpg.code_of(node));
    push_text(&mut out, "TYPE_FULL_NAME", cpg.type_full_name_of(node));
    push_text(&mut out, "FULL_NAME", cpg.full_name_of(node));
    push_text(&mut out, "SIGNATURE", cpg.signature_of(node));
    if cpg.order_of(node) != 0 || cpg.kind_of(node) == NodeKind::Method {
        out.push_str(&format!(" ORDER={}", cpg.order_of(node)));
    }
    if cpg.argument_index_of(node) >= 0 {
        out.push_str(&format!(" ARGUMENT_INDEX={}", cpg.argument_index_of(node)));
    }
    out
}

fn push_text(out: &mut String, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(&escape(value));
    }
}

fn escape(value: &str) -> String {
    value.replace('\n', "\\n").trim().to_string()
}

fn node_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::File => "FILE",
        NodeKind::Namespace => "NAMESPACE_BLOCK",
        NodeKind::TypeDecl => "TYPE_DECL",
        NodeKind::Member => "MEMBER",
        NodeKind::Method => "METHOD",
        NodeKind::MethodParameterIn => "METHOD_PARAMETER_IN",
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
        NodeKind::Unknown => "UNKNOWN",
    }
}

fn edge_name(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Cfg => "CFG",
        EdgeKind::Call => "CALL",
        EdgeKind::Ref => "REF",
        EdgeKind::Argument => "ARGUMENT",
        EdgeKind::Contains => "CONTAINS",
        EdgeKind::Ast | EdgeKind::Ddg | EdgeKind::Receiver | EdgeKind::ReachingDef => {
            unreachable!("filtered before rendering")
        }
    }
}

fn stable_node_key(cpg: &Cpg, node: NodeId) -> (u8, String, String, u32) {
    (
        cpg.kind_of(node).to_u8(),
        cpg.full_name_of(node)
            .or_else(|| cpg.name_of(node))
            .unwrap_or("")
            .to_string(),
        cpg.path_of(cpg.file_of(node)).unwrap_or("").to_string(),
        node.0,
    )
}

fn external_address(cpg: &Cpg, node: NodeId) -> String {
    let prefix = match cpg.kind_of(node) {
        NodeKind::File => "F",
        NodeKind::Namespace => "NB",
        NodeKind::TypeDecl => "D",
        NodeKind::Method => "M",
        _ => "N",
    };
    let identity = cpg
        .full_name_of(node)
        .or_else(|| cpg.name_of(node))
        .or_else(|| cpg.code_of(node))
        .map(escape)
        .unwrap_or_else(|| node.0.to_string());
    format!("{prefix}:{identity}")
}

pub fn migration_report(standalone: &str, production: &str) -> String {
    let old = sections(standalone);
    let new = sections(production);
    let mut report = String::new();
    let mut total_removed = 0usize;
    let mut total_added = 0usize;
    for name in ["AST", "NODES", "EDGES", "FLOWS"] {
        let a = &old[name];
        let b = &new[name];
        let common = a.intersection(b).count();
        let removed = a.difference(b).count();
        let added = b.difference(a).count();
        total_removed += removed;
        total_added += added;
        report.push_str(&format!(
            "SECTION {name} standalone={} production={} common={common} removed={removed} added={added} exact={}\n",
            a.len(),
            b.len(),
            removed == 0 && added == 0
        ));
    }
    report.push_str(&format!(
        "TOTAL removed={total_removed} added={total_added} exact={}\n",
        total_removed == 0 && total_added == 0
    ));
    report
}

fn sections(text: &str) -> BTreeMap<&'static str, BTreeSet<String>> {
    let mut result: BTreeMap<&'static str, BTreeSet<String>> = [
        ("AST", BTreeSet::new()),
        ("NODES", BTreeSet::new()),
        ("EDGES", BTreeSet::new()),
        ("FLOWS", BTreeSet::new()),
    ]
    .into_iter()
    .collect();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let (section, value) = if let Some(value) = line.strip_prefix("NODES|") {
            ("NODES", value)
        } else if let Some(value) = line.strip_prefix("EDGES|") {
            ("EDGES", value)
        } else if let Some(value) = line.strip_prefix("FLOWS|") {
            ("FLOWS", value)
        } else {
            ("AST", line.strip_prefix("AST|").unwrap_or(line))
        };
        result.get_mut(section).unwrap().insert(value.to_string());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_dump_is_deterministic_and_uses_standard_layers() {
        let sources = vec![
            (
                "b.c".to_string(),
                "int twice(int x) { return x + x; }".to_string(),
            ),
            (
                "a.c".to_string(),
                "int main(void) { return twice(2); }".to_string(),
            ),
        ];
        let first = dump_sources(&sources);
        let second = dump_sources(&sources);
        assert_eq!(first, second);
        assert!(first.contains("METHOD NAME=main"));
        assert!(first.contains("EDGES|CFG "));
        assert!(first.contains("FLOWS|REACHING_DEF["));
    }

    #[test]
    fn migration_report_is_stable_and_sectioned() {
        let report = migration_report(
            "METHOD NAME=a\nNODES|FILE NAME=a.c\n",
            "METHOD NAME=b\nNODES|FILE NAME=a.c\n",
        );
        assert!(report.contains(
            "SECTION AST standalone=1 production=1 common=0 removed=1 added=1 exact=false"
        ));
        assert!(report.contains(
            "SECTION NODES standalone=1 production=1 common=1 removed=0 added=0 exact=true"
        ));
        assert!(report.ends_with("TOTAL removed=1 added=1 exact=false\n"));
    }
}
