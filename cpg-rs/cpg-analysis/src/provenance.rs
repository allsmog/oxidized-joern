//! Auditable provenance for derived facts.
//!
//! Incremental security analysis needs to know why a derived tuple exists. This
//! module gives every fact a stable id plus support edges to the facts that
//! produced it. Retraction walks the support graph to identify facts that must be
//! invalidated when a base fact disappears.

use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct FactId(pub u64);

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum FactKind {
    Base,
    Derived,
    Query,
    Summary,
    ModelVerdict,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Fact {
    pub relation: String,
    pub key: String,
    pub kind: FactKind,
}

#[derive(Clone, Debug)]
pub struct ProvenanceEntry {
    pub id: FactId,
    pub fact: Fact,
    pub supports: Vec<FactId>,
    pub rule: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ProvenanceGraph {
    next: u64,
    entries: HashMap<FactId, ProvenanceEntry>,
    ids_by_fact: HashMap<Fact, FactId>,
    users_by_support: HashMap<FactId, HashSet<FactId>>,
}

impl ProvenanceGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_base(&mut self, relation: impl Into<String>, key: impl Into<String>) -> FactId {
        self.insert(
            Fact { relation: relation.into(), key: key.into(), kind: FactKind::Base },
            Vec::new(),
            None,
        )
    }

    pub fn insert_derived(
        &mut self,
        relation: impl Into<String>,
        key: impl Into<String>,
        supports: Vec<FactId>,
        rule: impl Into<String>,
    ) -> FactId {
        self.insert(
            Fact { relation: relation.into(), key: key.into(), kind: FactKind::Derived },
            supports,
            Some(rule.into()),
        )
    }

    pub fn insert(&mut self, fact: Fact, supports: Vec<FactId>, rule: Option<String>) -> FactId {
        if let Some(&id) = self.ids_by_fact.get(&fact) {
            return id;
        }
        let id = FactId(self.next);
        self.next += 1;
        for &support in &supports {
            self.users_by_support.entry(support).or_default().insert(id);
        }
        self.ids_by_fact.insert(fact.clone(), id);
        self.entries.insert(id, ProvenanceEntry { id, fact, supports, rule });
        id
    }

    pub fn entry(&self, id: FactId) -> Option<&ProvenanceEntry> {
        self.entries.get(&id)
    }

    pub fn support_chain(&self, id: FactId) -> Vec<FactId> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut q = VecDeque::from([id]);
        while let Some(cur) = q.pop_front() {
            if !seen.insert(cur) {
                continue;
            }
            out.push(cur);
            if let Some(entry) = self.entries.get(&cur) {
                for &s in &entry.supports {
                    q.push_back(s);
                }
            }
        }
        out
    }

    /// Return the facts that depend on `retracted`, including transitive users.
    pub fn invalidated_by(&self, retracted: FactId) -> HashSet<FactId> {
        let mut out = HashSet::new();
        let mut q = VecDeque::from([retracted]);
        while let Some(cur) = q.pop_front() {
            if !out.insert(cur) {
                continue;
            }
            if let Some(users) = self.users_by_support.get(&cur) {
                for &u in users {
                    q.push_back(u);
                }
            }
        }
        out
    }
}
