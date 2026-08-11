//! Graph export (JoernExport parity): dump the CPG — whole, or split into
//! per-method subgraphs — as dot, graphml, or json.
//!
//! Representations select which edge kinds are emitted:
//!   ast   — AST edges only
//!   cfg   — CFG edges only
//!   ddg   — DDG + REACHING_DEF edges
//!   cpg14 — the classic "code property graph" paper representation
//!           (AST + CFG + DDG + REF/ARGUMENT structure)
//!   all   — every edge kind, whole graph in a single file
//!
//! For every repr except `all`, the export is split by method exactly as
//! JoernExport does: each method's subgraph (the method node plus its AST
//! descendants) goes to `<method-file-path>/<method-name>.<ext>` under the
//! output directory, and only edges with both endpoints inside the subgraph
//! are written. `all` writes a single `export.<ext>` for the whole graph.
//!
//! Formats graphson and neo4jcsv from the Scala tool are deliberately not
//! ported: dot covers visualisation, graphml covers gephi-style tooling, and
//! json is the machine-readable form the Scala tool lacked.

use cpg_core::{Cpg, EdgeKind, NodeId, Query};
use std::collections::HashSet;
use std::io::Write;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Repr {
    Ast,
    Cfg,
    Ddg,
    Cpg14,
    All,
}

impl Repr {
    pub fn parse(s: &str) -> Option<Repr> {
        match s.to_ascii_lowercase().as_str() {
            "ast" => Some(Repr::Ast),
            "cfg" => Some(Repr::Cfg),
            "ddg" => Some(Repr::Ddg),
            "cpg14" | "cpg" => Some(Repr::Cpg14),
            "all" => Some(Repr::All),
            _ => None,
        }
    }

    /// Edge kinds this representation includes; `None` means "all kinds".
    fn edge_kinds(self) -> Option<&'static [EdgeKind]> {
        match self {
            Repr::Ast => Some(&[EdgeKind::Ast]),
            Repr::Cfg => Some(&[EdgeKind::Cfg]),
            Repr::Ddg => Some(&[EdgeKind::Ddg, EdgeKind::ReachingDef]),
            Repr::Cpg14 => Some(&[
                EdgeKind::Ast,
                EdgeKind::Cfg,
                EdgeKind::Ddg,
                EdgeKind::ReachingDef,
                EdgeKind::Ref,
                EdgeKind::Argument,
            ]),
            Repr::All => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Dot,
    Graphml,
    Json,
}

impl Format {
    pub fn parse(s: &str) -> Option<Format> {
        match s.to_ascii_lowercase().as_str() {
            "dot" => Some(Format::Dot),
            "graphml" => Some(Format::Graphml),
            "json" => Some(Format::Json),
            _ => None,
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Format::Dot => "dot",
            Format::Graphml => "graphml",
            Format::Json => "json",
        }
    }
}

pub struct ExportStats {
    pub nodes: usize,
    pub edges: usize,
    pub files: usize,
}

/// Export the graph under `out_dir`. Splits by method unless repr is `all`.
pub fn export(
    cpg: &Cpg,
    repr: Repr,
    format: Format,
    out_dir: &std::path::Path,
) -> std::io::Result<ExportStats> {
    std::fs::create_dir_all(out_dir)?;
    let mut stats = ExportStats {
        nodes: 0,
        edges: 0,
        files: 0,
    };
    if repr == Repr::All {
        let nodes: Vec<NodeId> = cpg.nodes().collect();
        let path = out_dir.join(format!("export.{}", format.extension()));
        write_subgraph(cpg, "export", &nodes, repr, format, &path, &mut stats)?;
        return Ok(stats);
    }
    // One file per method, deduplicated the way JoernExport does for
    // same-named methods (suffix instead of silent overwrite).
    let mut used: HashSet<String> = HashSet::new();
    for m in cpg.methods() {
        let name = cpg.name_of(m).unwrap_or("<unnamed>");
        let file = cpg.path_of(cpg.file_of(m)).unwrap_or("_root_");
        let mut rel = format!("{}/{}", sanitize(file), sanitize(name));
        while used.contains(&rel) {
            rel.push('_');
        }
        used.insert(rel.clone());
        let path = out_dir.join(format!("{rel}.{}", format.extension()));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let nodes = method_subgraph(cpg, m);
        write_subgraph(cpg, name, &nodes, repr, format, &path, &mut stats)?;
    }
    Ok(stats)
}

/// The method node plus its AST descendants — the JoernExport subgraph unit.
fn method_subgraph(cpg: &Cpg, method: NodeId) -> Vec<NodeId> {
    let mut seen: HashSet<NodeId> = HashSet::new();
    let mut stack = vec![method];
    let mut out = Vec::new();
    while let Some(n) = stack.pop() {
        if !seen.insert(n) {
            continue;
        }
        out.push(n);
        for child in cpg.out_kind(n, EdgeKind::Ast) {
            stack.push(child);
        }
    }
    out
}

fn edges_within(
    cpg: &Cpg,
    nodes: &[NodeId],
    kinds: Option<&[EdgeKind]>,
) -> Vec<(NodeId, NodeId, EdgeKind)> {
    let set: HashSet<NodeId> = nodes.iter().copied().collect();
    let mut edges = Vec::new();
    for &n in nodes {
        for e in cpg.out(n) {
            if !set.contains(&e.other) {
                continue;
            }
            if kinds.is_none_or(|ks| ks.contains(&e.kind)) {
                edges.push((n, e.other, e.kind));
            }
        }
    }
    edges
}

fn node_label(cpg: &Cpg, n: NodeId) -> String {
    let kind = format!("{:?}", cpg.kind_of(n));
    let detail = cpg.name_of(n).or_else(|| cpg.code_of(n)).unwrap_or("");
    // Long code strings make dot unreadable; clip like Joern's dumps do.
    let clipped: String = detail.chars().take(40).collect();
    if clipped.is_empty() {
        kind
    } else {
        format!("{kind}: {clipped}")
    }
}

fn write_subgraph(
    cpg: &Cpg,
    graph_name: &str,
    nodes: &[NodeId],
    repr: Repr,
    format: Format,
    path: &std::path::Path,
    stats: &mut ExportStats,
) -> std::io::Result<()> {
    let edges = edges_within(cpg, nodes, repr.edge_kinds());
    let f = std::fs::File::create(path)?;
    let mut w = std::io::BufWriter::new(f);
    match format {
        Format::Dot => write_dot(cpg, graph_name, nodes, &edges, &mut w)?,
        Format::Graphml => write_graphml(cpg, nodes, &edges, &mut w)?,
        Format::Json => write_json(cpg, nodes, &edges, &mut w)?,
    }
    stats.nodes += nodes.len();
    stats.edges += edges.len();
    stats.files += 1;
    Ok(())
}

fn dot_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn write_dot(
    cpg: &Cpg,
    name: &str,
    nodes: &[NodeId],
    edges: &[(NodeId, NodeId, EdgeKind)],
    w: &mut impl Write,
) -> std::io::Result<()> {
    writeln!(w, "digraph \"{}\" {{", dot_escape(name))?;
    for &n in nodes {
        writeln!(
            w,
            "  \"{}\" [label = \"{}\"]",
            n.0,
            dot_escape(&node_label(cpg, n))
        )?;
    }
    for (src, dst, kind) in edges {
        writeln!(
            w,
            "  \"{}\" -> \"{}\" [label = \"{:?}\"]",
            src.0, dst.0, kind
        )?;
    }
    writeln!(w, "}}")
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn write_graphml(
    cpg: &Cpg,
    nodes: &[NodeId],
    edges: &[(NodeId, NodeId, EdgeKind)],
    w: &mut impl Write,
) -> std::io::Result<()> {
    writeln!(w, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>")?;
    writeln!(
        w,
        "<graphml xmlns=\"http://graphml.graphdrawing.org/xmlns\">"
    )?;
    writeln!(
        w,
        "  <key id=\"label\" for=\"node\" attr.name=\"label\" attr.type=\"string\"/>"
    )?;
    writeln!(
        w,
        "  <key id=\"kind\" for=\"edge\" attr.name=\"kind\" attr.type=\"string\"/>"
    )?;
    writeln!(w, "  <graph id=\"G\" edgedefault=\"directed\">")?;
    for &n in nodes {
        writeln!(
            w,
            "    <node id=\"n{}\"><data key=\"label\">{}</data></node>",
            n.0,
            xml_escape(&node_label(cpg, n))
        )?;
    }
    for (i, (src, dst, kind)) in edges.iter().enumerate() {
        writeln!(
            w,
            "    <edge id=\"e{i}\" source=\"n{}\" target=\"n{}\"><data key=\"kind\">{:?}</data></edge>",
            src.0, dst.0, kind
        )?;
    }
    writeln!(w, "  </graph>")?;
    writeln!(w, "</graphml>")
}

fn write_json(
    cpg: &Cpg,
    nodes: &[NodeId],
    edges: &[(NodeId, NodeId, EdgeKind)],
    w: &mut impl Write,
) -> std::io::Result<()> {
    let node_objs: Vec<serde_json::Value> = nodes
        .iter()
        .map(|&n| {
            serde_json::json!({
                "id": n.0,
                "kind": format!("{:?}", cpg.kind_of(n)),
                "name": cpg.name_of(n),
                "code": cpg.code_of(n),
                "file": cpg.path_of(cpg.file_of(n)),
                "line": cpg.line_of(n),
            })
        })
        .collect();
    let edge_objs: Vec<serde_json::Value> = edges
        .iter()
        .map(|(src, dst, kind)| {
            serde_json::json!({"src": src.0, "dst": dst.0, "kind": format!("{kind:?}")})
        })
        .collect();
    let doc = serde_json::json!({"nodes": node_objs, "edges": edge_objs});
    write!(
        w,
        "{}",
        serde_json::to_string_pretty(&doc).expect("serialize")
    )
}

/// JoernExport's filename sanitisation: anything outside [a-zA-Z0-9-_./]
/// becomes `_`, and a leading `/` is remapped under `_root_` so exports
/// never escape the output directory.
fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_./".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let no_traversal = cleaned.replace("..", "_");
    if let Some(rest) = no_traversal.strip_prefix('/') {
        format!("_root_/{rest}")
    } else if no_traversal.is_empty() {
        "_root_".to_string()
    } else {
        no_traversal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_remaps_absolute_and_hostile_names() {
        assert_eq!(sanitize("/etc/x.c"), "_root_/etc/x.c");
        assert_eq!(sanitize("a b<c>"), "a_b_c_");
        assert_eq!(sanitize("../../up"), "_/_/up");
        assert_eq!(sanitize(""), "_root_");
    }

    #[test]
    fn repr_and_format_parse_case_insensitively() {
        assert_eq!(Repr::parse("CPG14"), Some(Repr::Cpg14));
        assert_eq!(Repr::parse("All"), Some(Repr::All));
        assert_eq!(Repr::parse("pdg"), None);
        assert_eq!(Format::parse("DOT"), Some(Format::Dot));
        assert_eq!(Format::parse("graphson"), None);
    }
}
