//! Backward program slicing over the ReachingDef layer.
//!
//! Given criterion nodes (a sink call's arguments, or all nodes at a
//! method:line), walk REACHING_DEF edges backwards to collect every
//! definition that can influence the criterion. Two interprocedural hops are
//! supported, both bounded by `depth`:
//!
//! - a `MethodParameterIn` continues at every caller's argument of the same
//!   index (via resolved Call edges), and
//! - a `Call` definition continues inside its resolved callee at the defs
//!   reaching the callee's `MethodReturn` (the value the call produced).
//!
//! The result is the classic "what feeds this sink" slice, as source
//! locations — the unit a human (or a security agent) actually reads.

use cpg_core::{Cpg, EdgeKind, NodeId, NodeKind, Query};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};

pub struct SliceEntry {
    pub node: NodeId,
    pub kind: NodeKind,
    pub file: String,
    pub line: Option<u32>,
    pub code: String,
    pub method: String,
    pub hops: usize,
}

/// The enclosing Method of a node, by climbing AST in-edges.
pub fn enclosing_method(cpg: &Cpg, n: NodeId) -> Option<NodeId> {
    let mut cur = n;
    for _ in 0..1024 {
        if cpg.kind_of(cur) == NodeKind::Method {
            return Some(cur);
        }
        cur = cpg.in_kind(cur, EdgeKind::Ast).next()?;
    }
    None
}

fn entry(cpg: &Cpg, n: NodeId, hops: usize) -> SliceEntry {
    let code = cpg
        .code_of(n)
        .filter(|c| !c.is_empty())
        .or_else(|| cpg.name_of(n))
        .unwrap_or("")
        .to_string();
    let method = enclosing_method(cpg, n)
        .and_then(|m| cpg.name_of(m))
        .unwrap_or("")
        .to_string();
    SliceEntry {
        node: n,
        kind: cpg.kind_of(n),
        file: cpg.path_of(cpg.file_of(n)).unwrap_or("").to_string(),
        line: cpg.line_of(n),
        code,
        method,
        hops,
    }
}

/// Backward slice from `criteria`. `depth` bounds interprocedural hops;
/// `max_nodes` bounds the slice size (a truncated slice sets the flag in
/// [`slice_json`] so silent truncation can't read as full coverage).
pub fn backward_slice(
    cpg: &Cpg,
    criteria: &[NodeId],
    depth: usize,
    max_nodes: usize,
) -> (Vec<SliceEntry>, bool) {
    // callee method -> call sites, from resolved Call edges (built once).
    // Every target: RPC stitching fans a call out to several handlers, and a
    // handler reached only by the second edge must still find its callers.
    let mut callers: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for c in cpg.calls() {
        for m in cpg.call_targets(c) {
            callers.entry(m).or_default().push(c);
        }
    }

    let mut visited: HashSet<NodeId> = HashSet::new();
    let mut out: Vec<SliceEntry> = Vec::new();
    let mut truncated = false;
    let mut q: VecDeque<(NodeId, usize)> = VecDeque::new();
    for &n in criteria {
        if visited.insert(n) {
            q.push_back((n, 0));
        }
    }
    while let Some((n, hops)) = q.pop_front() {
        if out.len() >= max_nodes {
            truncated = true;
            break;
        }
        out.push(entry(cpg, n, hops));

        // Intraprocedural: definitions reaching this node.
        for d in cpg.in_kind(n, EdgeKind::ReachingDef) {
            if cpg.is_live(d) && visited.insert(d) {
                q.push_back((d, hops));
            }
        }
        // Parameter -> caller arguments of the same index.
        if cpg.kind_of(n) == NodeKind::MethodParameterIn && hops < depth {
            if let Some(m) = enclosing_method(cpg, n) {
                let idx = cpg.argument_index_of(n);
                for &call in callers.get(&m).map(|v| v.as_slice()).unwrap_or(&[]) {
                    for a in cpg.out_kind(call, EdgeKind::Argument) {
                        if cpg.argument_index_of(a) == idx && visited.insert(a) {
                            q.push_back((a, hops + 1));
                        }
                    }
                }
            }
        }
        // Call definition -> the defs reaching each callee's return value.
        if cpg.kind_of(n) == NodeKind::Call && hops < depth {
            for m in cpg.call_targets(n) {
                if let Some(mr) = cpg
                    .out_kind(m, EdgeKind::Ast)
                    .find(|&c| cpg.kind_of(c) == NodeKind::MethodReturn)
                {
                    for d in cpg.in_kind(mr, EdgeKind::ReachingDef) {
                        if cpg.is_live(d) && visited.insert(d) {
                            q.push_back((d, hops + 1));
                        }
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    (out, truncated)
}

/// Criterion selection: `--call NAME` picks every call site of NAME (plus its
/// arguments, so the slice starts from the data flowing *into* the sink);
/// filters narrow by file substring / exact line / enclosing method name.
pub fn criterion_calls(
    cpg: &Cpg,
    call_name: &str,
    file_substr: Option<&str>,
    line: Option<u32>,
    method_name: Option<&str>,
) -> Vec<NodeId> {
    let mut crit = Vec::new();
    for c in cpg.calls_named(call_name) {
        if let Some(f) = file_substr {
            if !cpg.path_of(cpg.file_of(c)).unwrap_or("").contains(f) {
                continue;
            }
        }
        if let Some(l) = line {
            if cpg.line_of(c) != Some(l) {
                continue;
            }
        }
        if let Some(mn) = method_name {
            let em = enclosing_method(cpg, c).and_then(|m| cpg.name_of(m));
            if em != Some(mn) {
                continue;
            }
        }
        crit.push(c);
        crit.extend(cpg.out_kind(c, EdgeKind::Argument));
    }
    crit
}

/// Criterion selection: every expression node of `method_name` at `line`.
pub fn criterion_location(cpg: &Cpg, method_name: &str, line: u32) -> Vec<NodeId> {
    let mut crit = Vec::new();
    for m in cpg.methods() {
        if cpg.name_of(m) != Some(method_name) {
            continue;
        }
        let mut stack: Vec<NodeId> = cpg.out_kind(m, EdgeKind::Ast).collect();
        while let Some(n) = stack.pop() {
            if cpg.kind_of(n) == NodeKind::Method {
                continue; // nested method: separate unit
            }
            if cpg.line_of(n) == Some(line)
                && matches!(
                    cpg.kind_of(n),
                    NodeKind::Call | NodeKind::Identifier | NodeKind::Return
                )
            {
                crit.push(n);
            }
            stack.extend(cpg.out_kind(n, EdgeKind::Ast));
        }
    }
    crit
}

pub fn slice_json(
    cpg: &Cpg,
    criteria: &[NodeId],
    entries: &[SliceEntry],
    truncated: bool,
) -> Value {
    let crit: Vec<Value> = criteria
        .iter()
        .map(|&n| {
            json!({
                "code": cpg.code_of(n).or_else(|| cpg.name_of(n)),
                "file": cpg.path_of(cpg.file_of(n)),
                "line": cpg.line_of(n),
            })
        })
        .collect();
    let nodes: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "kind": format!("{:?}", e.kind),
                "file": e.file,
                "line": e.line,
                "code": e.code,
                "method": e.method,
                "hops": e.hops,
            })
        })
        .collect();
    json!({
        "criteria": crit,
        "nodes": nodes,
        "count": entries.len(),
        "truncated": truncated,
    })
}
