//! Call-graph pass (reads Ast, writes CallGraph).
//!
//! Resolves call sites to method declarations by name and emits `Call` edges.
//! This is the one inherently cross-file pass: a call in `a.c` may target a
//! method defined in `b.c`. When `a.c` changes we re-resolve its calls against
//! the project-maintained name index from the `PassContext` (falling back to a
//! one-shot scan when no context index is provided); the incremental driver
//! additionally re-runs this pass for callers when a *callee's* file changes
//! (see `cpg-incremental`).

use crate::pass::{ast_descendants, method_name_index, Pass, PassContext};
use cpg_core::{Cpg, EdgeKind, FileId, NodeId, NodeKind};

pub struct CallGraphPass;

impl Pass for CallGraphPass {
    fn name(&self) -> &'static str {
        "CallGraphPass"
    }
    fn reads(&self) -> &'static [cpg_core::Layer] {
        &[cpg_core::Layer::Ast]
    }
    fn writes(&self) -> &'static [cpg_core::Layer] {
        &[cpg_core::Layer::CallGraph]
    }
    fn output_edge(&self) -> Option<EdgeKind> {
        Some(EdgeKind::Call)
    }
    fn run_file(&self, cpg: &mut Cpg, file: FileId, ctx: &PassContext) {
        match ctx.methods_by_name {
            Some(index) => self.resolve_file(cpg, file, index),
            None => {
                let index = method_name_index(cpg);
                self.resolve_file(cpg, file, &index);
            }
        }
    }

    /// Without a context index, build the global method-name index once and
    /// resolve every file against it — O(methods + calls), not O(files × methods).
    fn run_batch(&self, cpg: &mut Cpg, files: &[FileId], ctx: &PassContext) {
        match ctx.methods_by_name {
            Some(index) => {
                for &f in files {
                    self.resolve_file(cpg, f, index);
                }
            }
            None => {
                let index = method_name_index(cpg);
                for &f in files {
                    self.resolve_file(cpg, f, &index);
                }
            }
        }
    }
}

impl CallGraphPass {
    fn resolve_file(
        &self,
        cpg: &mut Cpg,
        file: FileId,
        index: &std::collections::HashMap<String, Vec<NodeId>>,
    ) {
        let methods: Vec<NodeId> = cpg
            .nodes_in_file(file)
            .iter()
            .copied()
            .filter(|&n| cpg.is_live(n) && cpg.kind_of(n) == NodeKind::Method)
            .collect();

        for m in methods {
            for n in ast_descendants(cpg, m) {
                if cpg.kind_of(n) == NodeKind::Call {
                    if let Some(name) = cpg.name_of(n) {
                        if let Some(targets) = index.get(name) {
                            // Without overloading we take the unique definition.
                            if let Some(&t) = targets.first() {
                                cpg.add_edge(n, t, EdgeKind::Call);
                            }
                        }
                    }
                }
            }
        }
    }
}
