//! `cpg-analysis` — the pass framework and dataflow.
//!
//! Passes declare read/write [`Layer`](cpg_core::Layer)s so ordering and
//! incremental re-runs are derived rather than hand-wired. Dataflow is
//! summary-first and the summary cache is precisely invalidatable.

pub mod callgraph;
pub mod cfg;
pub mod pass;
pub mod provenance;
pub mod query;
pub mod relations;
pub mod scan;
pub mod summaries;
pub mod symbols;
pub mod taint;
pub mod value_flow;

pub use callgraph::CallGraphPass;
pub use cfg::CfgPass;
pub use pass::{Pass, PassContext, PassManager};
pub use provenance::{Fact, FactId, FactKind, ProvenanceGraph};
pub use query::{LogicalPlan, NodeSelector, Predicate, QueryCompiler, QueryError, QueryExecutor};
pub use relations::{Relation, RelationStore, Tuple};
pub use scan::{ScanDelta, ScanSubscription};
pub use summaries::{Flow, FunctionSummary, Point, SummaryStore};
pub use symbols::SymbolResolutionPass;
pub use taint::{find_flows, Finding, TaintSpec};
pub use value_flow::{SparseValueFlow, ValueFlowEdge, ValueFlowKind};

/// Build the standard pass pipeline. Order is derived from layer dependencies,
/// so the sequence here is irrelevant — the manager sorts it.
pub fn standard_pipeline() -> PassManager {
    let mut pm = PassManager::new();
    pm.add(Box::new(CallGraphPass))
        .add(Box::new(CfgPass))
        .add(Box::new(SymbolResolutionPass));
    pm
}
