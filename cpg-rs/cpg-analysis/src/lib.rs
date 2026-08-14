//! `cpg-analysis` — the pass framework and dataflow.
//!
//! Passes declare read/write [`Layer`](cpg_core::Layer)s so ordering and
//! incremental re-runs are derived rather than hand-wired. Dataflow is
//! summary-first and the summary cache is precisely invalidatable.

pub mod authz;
pub mod callgraph;
pub mod cfg;
pub mod entries;
pub mod middleware;
pub mod pass;
pub mod provenance;
pub mod query;
pub mod reaching_def;
pub mod relations;
pub mod scan;
pub mod structural;
pub mod summaries;
pub mod symbols;
pub mod taint;
pub mod value_flow;

pub use authz::{annotate_authz, is_authz_name};
pub use callgraph::CallGraphPass;
pub use cfg::{cfg_edges_for_method, CfgPass};
pub use entries::{mine_registration_entries, mine_routes, RouteEntry};
pub use middleware::{
    authz_census, authz_census_with_config, AuthzCensus, AuthzCensusConfig, MiddlewareGate,
};
pub use pass::{Pass, PassContext, PassManager};
pub use provenance::{Fact, FactId, FactKind, ProvenanceGraph};
pub use query::{
    node_kind_label, Direction, LogicalPlan, NodeSelector, Predicate, Property, QueryCompiler,
    QueryError, QueryExecutor, QueryResult, Traversal,
};
pub use reaching_def::{reaching_def_flows, ReachingDefFlow, ReachingDefPass};
pub use relations::{Relation, RelationStore, Tuple};
pub use scan::{ScanDelta, ScanSubscription};
pub use summaries::{Flow, FunctionSummary, Point, Sanitizer, SummaryOrigin, SummaryStore};
pub use symbols::SymbolResolutionPass;
pub use taint::{annotate_confined, find_flows, Finding, Provenance, Step, TaintSpec, Trace};
pub use value_flow::{SparseValueFlow, ValueFlowEdge, ValueFlowKind};

/// Build the standard pass pipeline. Order is derived from layer dependencies,
/// so the sequence here is irrelevant — the manager sorts it (ReachingDefPass
/// reads the Cfg layer, so it always runs after CfgPass).
pub fn standard_pipeline() -> PassManager {
    let mut pm = PassManager::new();
    pm.add(Box::new(CallGraphPass))
        .add(Box::new(CfgPass))
        .add(Box::new(ReachingDefPass))
        .add(Box::new(SymbolResolutionPass));
    pm
}
