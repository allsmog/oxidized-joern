//! Symbol-resolution pass (reads Ast, writes SymbolRef).
//!
//! Links each identifier in a method to its declaration (parameter or local)
//! by name, emitting `Ref` edges. Trait-aware: a fuller implementation would
//! consult `LanguageTraits::ALLOWS_FORWARD_REFS` / scoping rules, which is the
//! point of the trait contract — the resolution policy is parameterised rather
//! than re-written per language.

use crate::pass::{ast_descendants, Pass, PassContext};
use cpg_core::{Cpg, EdgeKind, FileId, NodeId, NodeKind};
use std::collections::HashMap;

pub struct SymbolResolutionPass;

impl Pass for SymbolResolutionPass {
    fn name(&self) -> &'static str {
        "SymbolResolutionPass"
    }
    fn reads(&self) -> &'static [cpg_core::Layer] {
        &[cpg_core::Layer::Ast]
    }
    fn writes(&self) -> &'static [cpg_core::Layer] {
        &[cpg_core::Layer::SymbolRef]
    }
    fn output_edge(&self) -> Option<EdgeKind> {
        Some(EdgeKind::Ref)
    }
    fn run_file(&self, cpg: &mut Cpg, file: FileId, _ctx: &PassContext) {
        let methods: Vec<NodeId> = cpg
            .nodes_in_file(file)
            .iter()
            .copied()
            .filter(|&n| cpg.is_live(n) && cpg.kind_of(n) == NodeKind::Method)
            .collect();

        for m in methods {
            let descendants = ast_descendants(cpg, m);
            // Declarations visible in this method, by name.
            let mut decls: HashMap<String, NodeId> = HashMap::new();
            for &d in &descendants {
                if matches!(
                    cpg.kind_of(d),
                    NodeKind::MethodParameterIn | NodeKind::Local
                ) {
                    if let Some(name) = cpg.name_of(d) {
                        decls.insert(name.to_string(), d);
                    }
                }
            }
            for &id in &descendants {
                if cpg.kind_of(id) == NodeKind::Identifier {
                    if let Some(name) = cpg.name_of(id) {
                        if let Some(&decl) = decls.get(name) {
                            cpg.add_edge(id, decl, EdgeKind::Ref);
                        }
                    }
                }
            }
        }
    }
}
