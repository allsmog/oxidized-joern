//! `cpg-analysis` — the pass framework and dataflow.
//!
//! Passes declare read/write [`Layer`](cpg_core::Layer)s so ordering and
//! incremental re-runs are derived rather than hand-wired. Dataflow is
//! summary-first and the summary cache is precisely invalidatable.

pub mod callgraph;
pub mod cfg;
pub mod pass;
pub mod summaries;
pub mod symbols;
pub mod taint;

pub use callgraph::CallGraphPass;
pub use cfg::CfgPass;
pub use pass::{Pass, PassContext, PassManager};
pub use summaries::{Flow, FunctionSummary, Point, Sanitizer, SummaryOrigin, SummaryStore};
pub use symbols::SymbolResolutionPass;
pub use taint::{find_flows, Finding, Provenance, Step, TaintSpec, Trace};

/// Build the standard pass pipeline. Order is derived from layer dependencies,
/// so the sequence here is irrelevant — the manager sorts it.
pub fn standard_pipeline() -> PassManager {
    let mut pm = PassManager::new();
    pm.add(Box::new(CallGraphPass))
        .add(Box::new(CfgPass))
        .add(Box::new(SymbolResolutionPass));
    pm
}
