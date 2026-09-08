//! Method-local return dependencies for the authoritative C graph.
//!
//! Reaching definitions supply branch joins and assignment kills. Named calls
//! are boundaries: their result depends only on the declared return flows of
//! their summaries, rather than the DDG's conservative call-site edges.

use crate::pass::ast_descendants;
use crate::summaries::{is_operator, CallReturn, Flow, FunctionSummary, Point, Sanitizer};
use crate::{SummaryOrigin, SummaryStore};
use cpg_core::{Cpg, EdgeKind, Layer, NodeId, NodeKind, Query};
use std::collections::{HashMap, HashSet, VecDeque};

pub(crate) fn is_authoritative(cpg: &Cpg, method: NodeId) -> bool {
    [Layer::Cfg, Layer::Ddg, Layer::CallGraph]
        .into_iter()
        .all(|layer| cpg.is_layer_authoritative(layer))
        // Absorbing a graph unions its layer flags. In a mixed-language
        // graph those flags alone do not make a generic method canonical.
        // Exact C methods carry SOURCE_FILE linkage; generic frontends do
        // not emit this edge. Require that method-local provenance as well.
        && cpg.out_kind(method, EdgeKind::SourceFile).next().is_some()
}

#[derive(Clone)]
pub(crate) struct SummaryHop {
    pub call: NodeId,
    pub fqn: String,
    pub origin: SummaryOrigin,
    pub parameter: usize,
}

pub(crate) struct ReturnEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub via: Option<Sanitizer>,
    pub hop: Option<SummaryHop>,
}

pub(crate) struct ReturnFlowGraph {
    pub edges: Vec<ReturnEdge>,
    pub call_origins: HashMap<NodeId, Vec<CallReturn>>,
    pub dependencies: HashSet<String>,
    returns: HashSet<NodeId>,
    incoming: HashMap<NodeId, Vec<usize>>,
    outgoing: HashMap<NodeId, Vec<usize>>,
}

impl ReturnFlowGraph {
    pub fn new(cpg: &Cpg, method: NodeId, store: &SummaryStore) -> Self {
        let mut nodes = ast_descendants(cpg, method);
        nodes.sort();
        let owned: HashSet<NodeId> = nodes.iter().copied().collect();
        let named_calls: HashSet<NodeId> = nodes
            .iter()
            .copied()
            .filter(|&node| {
                cpg.kind_of(node) == NodeKind::Call
                    && cpg.name_of(node).is_some_and(|name| !is_operator(name))
            })
            .collect();
        let mut graph = Self {
            edges: Vec::new(),
            call_origins: HashMap::new(),
            dependencies: HashSet::new(),
            returns: nodes
                .iter()
                .copied()
                .filter(|&node| cpg.kind_of(node) == NodeKind::Return)
                .collect(),
            incoming: HashMap::new(),
            outgoing: HashMap::new(),
        };
        for &from in &nodes {
            let mut targets: Vec<_> = cpg.out_kind(from, EdgeKind::ReachingDef).collect();
            targets.sort();
            targets.dedup();
            for to in targets {
                if owned.contains(&to) && !named_calls.contains(&to) {
                    graph.add_edge(from, to, None, None);
                }
            }
        }
        for call in nodes.into_iter().filter(|node| named_calls.contains(node)) {
            let name = cpg.name_of(call).unwrap_or("");
            let args = cpg.arguments_of(call);
            if store.sanitizer_names().contains(name) {
                graph.dependencies.insert(name.to_string());
                for arg in args {
                    graph.add_edge(arg, call, Some(Sanitizer::new(name)), None);
                }
                continue;
            }
            let key = cpg
                .call_target(call)
                .and_then(|target| cpg.full_name_of(target))
                .unwrap_or(name);
            graph.dependencies.insert(key.to_string());
            let mut origins = vec![CallReturn {
                call: name.to_string(),
                via: None,
            }];
            if let Some((summary, origin)) = store.get_with_origin(key) {
                let mut flows: Vec<_> = summary.flows.iter().collect();
                flows.sort();
                for flow in flows {
                    let (Point::Param(parameter), Point::Return) = (flow.from, flow.to) else {
                        continue;
                    };
                    if let Some(&arg) = args.get(parameter) {
                        graph.add_edge(
                            arg,
                            call,
                            flow.via.clone(),
                            Some(SummaryHop {
                                call,
                                fqn: key.to_string(),
                                origin,
                                parameter,
                            }),
                        );
                    }
                }
                origins.extend(summary.call_returns.iter().cloned());
            }
            origins.sort();
            origins.dedup();
            graph.call_origins.insert(call, origins);
        }
        graph
    }

    fn add_edge(
        &mut self,
        from: NodeId,
        to: NodeId,
        via: Option<Sanitizer>,
        hop: Option<SummaryHop>,
    ) {
        let index = self.edges.len();
        self.edges.push(ReturnEdge { from, to, via, hop });
        self.incoming.entry(to).or_default().push(index);
        self.outgoing.entry(from).or_default().push(index);
    }

    pub fn summary(&self, cpg: &Cpg, method: NodeId) -> FunctionSummary {
        let parameters: HashMap<NodeId, usize> = cpg
            .parameters_of(method)
            .into_iter()
            .enumerate()
            .map(|(index, node)| (node, index))
            .collect();
        let mut summary = FunctionSummary {
            fqn: cpg.full_name_of(method).unwrap_or("<anon>").to_string(),
            ..Default::default()
        };
        // Return statements, not METHOD_RETURN: the latter also receives all
        // definitions live at method exit, even when they are not returned.
        let mut queue: VecDeque<_> = self.returns.iter().map(|&node| (node, None)).collect();
        let mut seen = HashSet::new();
        while let Some((node, via)) = queue.pop_front() {
            if !seen.insert((node, via.clone())) {
                continue;
            }
            if let Some(&parameter) = parameters.get(&node) {
                summary.flows.insert(Flow {
                    from: Point::Param(parameter),
                    to: Point::Return,
                    via: via.clone(),
                });
            }
            for origin in self.call_origins.get(&node).into_iter().flatten() {
                summary.call_returns.insert(CallReturn {
                    call: origin.call.clone(),
                    via: origin.via.clone().or_else(|| via.clone()),
                });
            }
            for &index in self.incoming.get(&node).into_iter().flatten() {
                let edge = &self.edges[index];
                queue.push_back((edge.from, edge.via.clone().or_else(|| via.clone())));
            }
        }
        summary
    }

    /// Deterministic shortest dependency path to a return. The caller checks
    /// query-specific sanitizer cuts and recursively validates summary hops.
    pub fn path_to_return(
        &self,
        start: NodeId,
        mut allowed: impl FnMut(usize, &ReturnEdge) -> bool,
    ) -> Option<Vec<usize>> {
        let mut previous: HashMap<NodeId, usize> = HashMap::new();
        let mut seen = HashSet::from([start]);
        let mut queue = VecDeque::from([start]);
        while let Some(node) = queue.pop_front() {
            if self.returns.contains(&node) {
                let mut path = Vec::new();
                let mut current = node;
                while let Some(&index) = previous.get(&current) {
                    path.push(index);
                    current = self.edges[index].from;
                }
                path.reverse();
                return Some(path);
            }
            for &index in self.outgoing.get(&node).into_iter().flatten() {
                let edge = &self.edges[index];
                if !seen.contains(&edge.to) && allowed(index, edge) {
                    seen.insert(edge.to);
                    previous.insert(edge.to, index);
                    queue.push_back(edge.to);
                }
            }
        }
        None
    }
}
