//! Dominator and post-dominator trees over the native CFG.

use crate::pass::{Pass, PassContext};
use cpg_core::{Cpg, EdgeKind, FileId, NodeId, NodeKind};
use std::collections::{BTreeSet, HashMap, HashSet};

pub struct DominancePass;
pub struct PostDominancePass;

impl Pass for DominancePass {
    fn name(&self) -> &'static str {
        "DominancePass"
    }
    fn reads(&self) -> &'static [cpg_core::Layer] {
        &[cpg_core::Layer::Cfg]
    }
    fn writes(&self) -> &'static [cpg_core::Layer] {
        &[]
    }
    fn output_edge(&self) -> Option<EdgeKind> {
        Some(EdgeKind::Dominate)
    }
    fn run_file(&self, cpg: &mut Cpg, file: FileId, _ctx: &PassContext) {
        materialize(cpg, file, false, EdgeKind::Dominate);
    }
}

impl Pass for PostDominancePass {
    fn name(&self) -> &'static str {
        "PostDominancePass"
    }
    fn reads(&self) -> &'static [cpg_core::Layer] {
        &[cpg_core::Layer::Cfg]
    }
    fn writes(&self) -> &'static [cpg_core::Layer] {
        &[]
    }
    fn output_edge(&self) -> Option<EdgeKind> {
        Some(EdgeKind::PostDominate)
    }
    fn run_file(&self, cpg: &mut Cpg, file: FileId, _ctx: &PassContext) {
        materialize(cpg, file, true, EdgeKind::PostDominate);
    }
}

fn materialize(cpg: &mut Cpg, file: FileId, reverse: bool, edge: EdgeKind) {
    let methods: Vec<NodeId> = cpg
        .nodes_in_file(file)
        .iter()
        .copied()
        .filter(|&node| cpg.is_live(node) && cpg.kind_of(node) == NodeKind::Method)
        .collect();
    for method in methods {
        for (parent, child) in immediate_dominance_edges(cpg, method, reverse) {
            cpg.add_edge(parent, child, edge);
        }
    }
}

/// Immediate dominator edges. With `reverse = true`, computes the immediate
/// post-dominator tree by running the same fixed point over the reversed CFG.
pub fn immediate_dominance_edges(
    cpg: &Cpg,
    method: NodeId,
    reverse: bool,
) -> Vec<(NodeId, NodeId)> {
    let nodes = reachable_cfg(cpg, method);
    if nodes.is_empty() {
        return Vec::new();
    }
    let universe: BTreeSet<NodeId> = nodes.iter().copied().collect();
    let roots: BTreeSet<NodeId> = if reverse {
        nodes
            .iter()
            .copied()
            .filter(|&node| {
                cpg.out_kind(node, EdgeKind::Cfg)
                    .all(|successor| !universe.contains(&successor))
            })
            .collect()
    } else {
        BTreeSet::from([method])
    };
    if roots.is_empty() {
        return Vec::new();
    }

    let mut dominators: HashMap<NodeId, BTreeSet<NodeId>> = nodes
        .iter()
        .copied()
        .map(|node| {
            let initial = if roots.contains(&node) {
                BTreeSet::from([node])
            } else {
                universe.clone()
            };
            (node, initial)
        })
        .collect();

    loop {
        let mut changed = false;
        for &node in &nodes {
            if roots.contains(&node) {
                continue;
            }
            let adjacent: Vec<NodeId> = if reverse {
                cpg.out_kind(node, EdgeKind::Cfg)
                    .filter(|candidate| universe.contains(candidate))
                    .collect()
            } else {
                cpg.in_kind(node, EdgeKind::Cfg)
                    .filter(|candidate| universe.contains(candidate))
                    .collect()
            };
            let mut next = if let Some((&first, rest)) = adjacent.split_first() {
                let mut intersection = dominators[&first].clone();
                for predecessor in rest {
                    intersection = intersection
                        .intersection(&dominators[predecessor])
                        .copied()
                        .collect();
                }
                intersection
            } else {
                BTreeSet::new()
            };
            next.insert(node);
            if next != dominators[&node] {
                dominators.insert(node, next);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut edges = Vec::new();
    for &node in &nodes {
        if roots.contains(&node) {
            continue;
        }
        let strict: Vec<NodeId> = dominators[&node]
            .iter()
            .copied()
            .filter(|candidate| *candidate != node)
            .collect();
        let immediate = strict.iter().copied().find(|candidate| {
            !strict
                .iter()
                .copied()
                .any(|other| other != *candidate && dominators[&other].contains(candidate))
        });
        if let Some(parent) = immediate {
            edges.push((parent, node));
        }
    }
    edges.sort_unstable();
    edges
}

fn reachable_cfg(cpg: &Cpg, method: NodeId) -> Vec<NodeId> {
    let mut seen = HashSet::new();
    let mut stack = vec![method];
    while let Some(node) = stack.pop() {
        if !seen.insert(node) {
            continue;
        }
        let mut successors: Vec<NodeId> = cpg.out_kind(node, EdgeKind::Cfg).collect();
        successors.reverse();
        stack.extend(successors);
    }
    let mut nodes: Vec<NodeId> = seen.into_iter().collect();
    nodes.sort_unstable();
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::{QueryCompiler, QueryExecutor, QueryResult};
    use cpg_core::CpgBuilder;

    #[test]
    fn computes_branch_dominator_and_post_dominator_trees() {
        let mut cpg = Cpg::new();
        let file = cpg.file_id("branch.c");
        let mut builder = CpgBuilder::new(&mut cpg, file);
        let method = builder.method("main", "main", "int()", Some(1));
        let condition = builder.call(">", "argc > 1", Some(2));
        let left = builder.call("left", "left()", Some(3));
        let right = builder.call("right", "right()", Some(5));
        let join = builder.call("join", "join()", Some(7));
        let ret = builder.method_return("int");
        builder.cpg.add_edge(method, condition, EdgeKind::Cfg);
        builder.cpg.add_edge(condition, left, EdgeKind::Cfg);
        builder.cpg.add_edge(condition, right, EdgeKind::Cfg);
        builder.cpg.add_edge(left, join, EdgeKind::Cfg);
        builder.cpg.add_edge(right, join, EdgeKind::Cfg);
        builder.cpg.add_edge(join, ret, EdgeKind::Cfg);

        let dominators = immediate_dominance_edges(builder.cpg, method, false);
        assert_eq!(
            dominators,
            vec![
                (method, condition),
                (condition, left),
                (condition, right),
                (condition, join),
                (join, ret),
            ]
        );
        let post_dominators = immediate_dominance_edges(builder.cpg, method, true);
        assert_eq!(
            post_dominators,
            vec![
                (condition, method),
                (join, condition),
                (join, left),
                (join, right),
                (ret, join),
            ]
        );

        for (parent, child) in dominators {
            builder.cpg.add_edge(parent, child, EdgeKind::Dominate);
        }
        for (parent, child) in post_dominators {
            builder.cpg.add_edge(parent, child, EdgeKind::PostDominate);
        }
        let execute = |query: &str| {
            let plan = QueryCompiler::compile(query).expect("compile query");
            QueryExecutor::new(builder.cpg)
                .execute(&plan)
                .expect("execute query")
        };
        assert_eq!(
            execute(r#"cpg.call.code("argc > 1").controls.code"#),
            QueryResult::Strings(vec!["left()".to_string(), "right()".to_string()])
        );
        assert_eq!(
            execute(r#"cpg.call("right").controlledBy.code"#),
            QueryResult::Strings(vec!["argc > 1".to_string()])
        );
    }
}
