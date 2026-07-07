//! Relation catalog for fact-oriented analysis.
//!
//! This is the transition layer from imperative passes to derived facts. A pass
//! or query can insert named tuples, attach support facts, and ask for a small
//! binary transitive closure relation.

use crate::provenance::{FactId, ProvenanceGraph};
use std::collections::{HashMap, HashSet};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Tuple(pub Vec<String>);

impl Tuple {
    pub fn key(&self) -> String {
        self.0.join("|")
    }
}

#[derive(Clone, Debug, Default)]
pub struct Relation {
    tuples: HashMap<Tuple, FactId>,
}

impl Relation {
    pub fn len(&self) -> usize { self.tuples.len() }
    pub fn is_empty(&self) -> bool { self.tuples.is_empty() }
    pub fn contains(&self, tuple: &Tuple) -> bool { self.tuples.contains_key(tuple) }
    pub fn tuples(&self) -> impl Iterator<Item = (&Tuple, &FactId)> { self.tuples.iter() }
}

#[derive(Clone, Debug, Default)]
pub struct RelationStore {
    relations: HashMap<String, Relation>,
    pub provenance: ProvenanceGraph,
}

impl RelationStore {
    pub fn new() -> Self { Self::default() }

    pub fn insert_base(&mut self, relation: impl Into<String>, tuple: Tuple) -> FactId {
        let relation = relation.into();
        if let Some(id) = self.relations.get(&relation).and_then(|r| r.tuples.get(&tuple)).copied() {
            return id;
        }
        let id = self.provenance.insert_base(relation.clone(), tuple.key());
        self.relations.entry(relation).or_default().tuples.insert(tuple, id);
        id
    }

    pub fn insert_derived(&mut self, relation: impl Into<String>, tuple: Tuple, supports: Vec<FactId>, rule: impl Into<String>) -> FactId {
        let relation = relation.into();
        if let Some(id) = self.relations.get(&relation).and_then(|r| r.tuples.get(&tuple)).copied() {
            return id;
        }
        let id = self.provenance.insert_derived(relation.clone(), tuple.key(), supports, rule);
        self.relations.entry(relation).or_default().tuples.insert(tuple, id);
        id
    }

    pub fn relation(&self, name: &str) -> Option<&Relation> {
        self.relations.get(name)
    }

    pub fn invalidate_fact(&mut self, id: FactId) -> HashSet<FactId> {
        let invalid = self.provenance.invalidated_by(id);
        for relation in self.relations.values_mut() {
            relation.tuples.retain(|_, fact| !invalid.contains(fact));
        }
        invalid
    }

    pub fn derive_transitive_closure(&mut self, source: &str, target: &str, rule_name: &str) -> usize {
        let Some(source_rel) = self.relations.get(source).cloned() else { return 0 };
        let mut added = 0;
        let mut edges: Vec<(String, String, FactId)> = source_rel
            .tuples()
            .filter_map(|(tuple, &id)| match tuple.0.as_slice() {
                [a, b] => Some((a.clone(), b.clone(), id)),
                _ => None,
            })
            .collect();

        let mut changed = true;
        while changed {
            changed = false;
            let snapshot = edges.clone();
            for (a, b, aid) in &snapshot {
                for (c, d, bid) in &snapshot {
                    if b == c {
                        let tuple = Tuple(vec![a.clone(), d.clone()]);
                        if self.relations.get(target).map(|r| r.contains(&tuple)).unwrap_or(false) {
                            continue;
                        }
                        let id = self.insert_derived(target.to_string(), tuple, vec![*aid, *bid], rule_name.to_string());
                        edges.push((a.clone(), d.clone(), id));
                        added += 1;
                        changed = true;
                    }
                }
            }
        }
        added
    }
}
