//! Merge per-language CPGs into one graph and stitch gRPC boundaries.
//!
//! cpg-rs builds one CPG per frontend, so a Go service and the Scala
//! api-server live in separate graphs. `cpg merge` absorbs them into a
//! single Cpg, re-resolves the call graph globally (a call in module A can
//! now reach a method that came from module B's graph), and — given the
//! .proto files — links RPC client stubs to server handlers across the
//! language gap:
//!
//!   Scala   rds.queryChartData(req)      (ScalaPB stub, lowerCamel)
//!     ==>   Go  func (s) QueryChartData  (server handler, PascalCase)
//!
//! The link is a plain Call edge, so `cpg slice`'s parameter->caller-argument
//! hop walks straight from a Go handler's request parameter to the Scala
//! call site's arguments — a cross-service, cross-language slice.

use cpg_analysis::{CallGraphPass, Pass, PassContext};
use cpg_core::{Cpg, EdgeKind, NodeId, NodeKind, Query};
use std::collections::HashMap;

/// All `rpc Name(...)` method names declared under `dir` (recursive .proto
/// scan). Proto syntax is regular enough that a line scan is exact for this.
pub fn rpc_names(dir: &std::path::Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rpc_names(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("proto") {
            let Ok(src) = std::fs::read_to_string(&path) else {
                continue;
            };
            for line in src.lines() {
                let t = line.trim_start();
                if let Some(rest) = t.strip_prefix("rpc ").or_else(|| t.strip_prefix("rpc\t")) {
                    let name: String = rest
                        .trim_start()
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        out.push(name);
                    }
                }
            }
        }
    }
}

pub fn lc_first(s: &str) -> String {
    let mut cs = s.chars();
    match cs.next() {
        Some(c) => c.to_lowercase().collect::<String>() + cs.as_str(),
        None => String::new(),
    }
}

/// Re-run call resolution over the merged graph, so name resolution sees
/// methods from every donor CPG (each donor only resolved against itself).
pub fn relink_calls(cpg: &mut Cpg) {
    let files = cpg.files();
    let ctx = PassContext::empty();
    // Clear per-file Call edges first (the pass manager normally does this).
    for &f in &files {
        let nodes: Vec<NodeId> = cpg.nodes_in_file(f).to_vec();
        for n in nodes {
            if cpg.is_live(n) {
                cpg.remove_out_edges_of_kind(n, EdgeKind::Call);
            }
        }
    }
    CallGraphPass.run_batch(cpg, &files, &ctx);
}

/// Link RPC client stub calls to server handler methods. Returns the number
/// of edges added. For each declared rpc `M`, calls named `lcFirst(M)`
/// (ScalaPB stubs) or unresolved calls named `M` gain a Call edge to every
/// method named `M` that has at least one parameter (the handler). Fans out
/// to at most `MAX_IMPLS` targets — an over-generic name is skipped rather
/// than silently wiring half the graph together.
pub fn link_rpcs(cpg: &mut Cpg, rpcs: &[String]) -> (usize, Vec<String>) {
    const MAX_IMPLS: usize = 8;
    let mut methods_by_name: HashMap<String, Vec<NodeId>> = HashMap::new();
    for m in cpg.methods() {
        if let Some(n) = cpg.name_of(m) {
            methods_by_name.entry(n.to_string()).or_default().push(m);
        }
    }
    let mut added = 0;
    let mut skipped: Vec<String> = Vec::new();
    for rpc in rpcs {
        let impls: Vec<NodeId> = methods_by_name
            .get(rpc.as_str())
            .map(|v| {
                v.iter()
                    .copied()
                    .filter(|&m| !cpg.parameters_of(m).is_empty())
                    .collect()
            })
            .unwrap_or_default();
        if impls.is_empty() {
            continue;
        }
        if impls.len() > MAX_IMPLS {
            skipped.push(format!("{rpc} ({} impls)", impls.len()));
            continue;
        }
        let stub = lc_first(rpc);
        let mut clients: Vec<NodeId> = if stub != *rpc {
            cpg.calls_named(&stub)
        } else {
            Vec::new()
        };
        // Same-case client calls that stayed unresolved (e.g. only the stub
        // interface, not the handler, was in that donor's graph).
        clients.extend(
            cpg.calls_named(rpc)
                .into_iter()
                .filter(|&c| cpg.call_target(c).is_none()),
        );
        for call in clients {
            for &m in &impls {
                // The handler is not the call's own enclosing method's file
                // twin — link every impl; slice fan-out stays bounded by
                // MAX_IMPLS.
                if cpg.kind_of(call) == NodeKind::Call {
                    cpg.add_edge(call, m, EdgeKind::Call);
                    added += 1;
                }
            }
        }
    }
    (added, skipped)
}
