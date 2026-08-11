//! Incremental scan subscription support.
//!
//! This is the library-side version of "scan as subscription": keep a standing
//! taint spec, materialize the current finding set, and report added/removed
//! findings after the caller applies an edit and refreshes the subscription.
//! The caller owns graph updates; this module owns stable finding diffing.

use crate::taint::{find_flows, Finding, TaintSpec};
use crate::SummaryStore;
use cpg_core::Cpg;
use std::collections::HashMap;

/// Delta between two materialized finding snapshots.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanDelta {
    pub added: Vec<Finding>,
    pub removed: Vec<Finding>,
    pub total: usize,
}

/// A standing scan over the current graph and summary store.
///
/// The subscription is intentionally transport-agnostic: CLI, daemon, LSP, and CI
/// integrations can all keep one of these and call `refresh` after an edit. That
/// mirrors the target architecture's materialized-finding view without forcing a
/// TCP/HTTP daemon into the core crates yet.
#[derive(Clone, Debug)]
pub struct ScanSubscription {
    spec: TaintSpec,
    last: HashMap<String, Finding>,
}

impl ScanSubscription {
    /// Create a subscription and prime its materialized view from the current CPG.
    pub fn new(cpg: &Cpg, summaries: &SummaryStore, spec: TaintSpec) -> Self {
        let last = snapshot(cpg, summaries, &spec);
        ScanSubscription { spec, last }
    }

    /// Convenience constructor for the common source/sink/sanitizer use case.
    pub fn from_names(
        cpg: &Cpg,
        summaries: &SummaryStore,
        sources: &[&str],
        sinks: &[&str],
        sanitizers: &[&str],
    ) -> Self {
        Self::new(
            cpg,
            summaries,
            TaintSpec::with_sanitizers(sources, sinks, sanitizers),
        )
    }

    /// Current materialized findings.
    pub fn current(&self) -> impl Iterator<Item = &Finding> {
        self.last.values()
    }

    /// Re-run the subscribed scan and return only the finding delta.
    pub fn refresh(&mut self, cpg: &Cpg, summaries: &SummaryStore) -> ScanDelta {
        let next = snapshot(cpg, summaries, &self.spec);
        let added = next
            .iter()
            .filter(|(key, _)| !self.last.contains_key(*key))
            .map(|(_, finding)| finding.clone())
            .collect();
        let removed = self
            .last
            .iter()
            .filter(|(key, _)| !next.contains_key(*key))
            .map(|(_, finding)| finding.clone())
            .collect();
        self.last = next;
        ScanDelta {
            added,
            removed,
            total: self.last.len(),
        }
    }
}

fn snapshot(cpg: &Cpg, summaries: &SummaryStore, spec: &TaintSpec) -> HashMap<String, Finding> {
    find_flows(cpg, summaries, spec)
        .into_iter()
        .map(|finding| (finding_key(&finding), finding))
        .collect()
}

fn finding_key(f: &Finding) -> String {
    let path = f
        .path
        .iter()
        .map(|step| format!("{}@{:?}", step.code, step.line))
        .collect::<Vec<_>>()
        .join("->");
    format!(
        "{}|{}|{:?}|{}|{}",
        f.method, f.sink, f.sink_line, f.origin, path
    )
}
