//! Intra-procedural control-flow pass (reads Ast, writes Cfg).
//!
//! A deliberately simple linearisation: it chains a method's control-flow
//! relevant nodes in source order. The point here is to exercise the *layer
//! and incrementality machinery* end-to-end, not to ship a precise CFG — a
//! production version would model branches/loops via the structured grammar.

use crate::pass::{ast_descendants, Pass};
use cpg_core::{Cpg, EdgeKind, FileId, NodeId, NodeKind};

pub struct CfgPass;

impl Pass for CfgPass {
    fn name(&self) -> &'static str {
        "CfgPass"
    }
    fn reads(&self) -> &'static [cpg_core::Layer] {
        &[cpg_core::Layer::Ast]
    }
    fn writes(&self) -> &'static [cpg_core::Layer] {
        &[cpg_core::Layer::Cfg]
    }
    fn output_edge(&self) -> Option<EdgeKind> {
        Some(EdgeKind::Cfg)
    }
    fn run_file(&self, cpg: &mut Cpg, file: FileId) {
        let methods: Vec<NodeId> = cpg
            .nodes_in_file(file)
            .iter()
            .copied()
            .filter(|&n| cpg.is_live(n) && cpg.kind_of(n) == NodeKind::Method)
            .collect();

        for m in methods {
            let mut flow: Vec<NodeId> = ast_descendants(cpg, m)
                .into_iter()
                .filter(|&n| {
                    matches!(
                        cpg.kind_of(n),
                        NodeKind::Call | NodeKind::Return | NodeKind::ControlStructure
                    )
                })
                .collect();
            flow.sort_by_key(|&n| (cpg.line_of(n).unwrap_or(0), n.0));
            let mut prev = m;
            for n in flow {
                cpg.add_edge(prev, n, EdgeKind::Cfg);
                prev = n;
            }
        }
    }
}
