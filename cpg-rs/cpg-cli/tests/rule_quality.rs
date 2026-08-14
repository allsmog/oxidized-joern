//! Labeled quality gate for the default C security pack. The manifest is the
//! release contract: every default rule has a positive, near-miss negative,
//! fix negative, and cross-file case, and both aggregate and per-rule
//! precision/recall must meet the committed thresholds.

use cpg_cli::{build_project, rules, scan};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    schema_version: u32,
    language: String,
    thresholds: Thresholds,
    rules: Vec<String>,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Thresholds {
    aggregate_precision: f64,
    aggregate_recall: f64,
    per_rule_precision: f64,
    per_rule_recall: f64,
}

#[derive(Debug, Deserialize)]
struct Case {
    id: String,
    category: String,
    path: String,
    expected: BTreeMap<String, usize>,
}

#[derive(Default, Debug)]
struct Counts {
    tp: usize,
    fp: usize,
    fn_: usize,
}

impl Counts {
    fn precision(&self) -> f64 {
        if self.tp + self.fp == 0 {
            1.0
        } else {
            self.tp as f64 / (self.tp + self.fp) as f64
        }
    }

    fn recall(&self) -> f64 {
        if self.tp + self.fn_ == 0 {
            1.0
        } else {
            self.tp as f64 / (self.tp + self.fn_) as f64
        }
    }

    fn add(&mut self, expected: usize, actual: usize) {
        self.tp += expected.min(actual);
        self.fp += actual.saturating_sub(expected);
        self.fn_ += expected.saturating_sub(actual);
    }
}

fn catalog_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../acceptance/rules")
}

#[test]
fn default_c_rules_meet_labeled_quality_contract() {
    let root = catalog_root();
    let catalog_text = std::fs::read_to_string(root.join("catalog.json")).unwrap();
    let catalog: Catalog = serde_json::from_str(&catalog_text).unwrap();
    assert_eq!(catalog.schema_version, 1);
    assert_eq!(catalog.language, "c");

    let pack = rules::builtin_pack("c").expect("C has a default rule pack");
    let pack_ids: BTreeSet<_> = pack.rules.iter().map(|r| r.id.clone()).collect();
    let catalog_ids: BTreeSet<_> = catalog.rules.iter().cloned().collect();
    assert_eq!(
        pack_ids, catalog_ids,
        "catalog must cover every default rule"
    );

    let categories: BTreeSet<_> = catalog
        .cases
        .iter()
        .map(|case| case.category.as_str())
        .collect();
    assert_eq!(
        categories,
        BTreeSet::from([
            "fix-negative",
            "multi-file",
            "near-miss-negative",
            "positive"
        ]),
        "all required evidence categories must remain present"
    );

    let mut aggregate = Counts::default();
    let mut per_rule: BTreeMap<String, Counts> = catalog
        .rules
        .iter()
        .cloned()
        .map(|id| (id, Counts::default()))
        .collect();

    for case in &catalog.cases {
        let project = build_project(root.join(&case.path).to_str().unwrap(), "c")
            .unwrap_or_else(|error| panic!("{}: build failed: {error}", case.id));
        let findings = scan::run_pack(&project, &pack);
        for result in findings {
            let expected = case.expected.get(&result.rule.id).copied().unwrap_or(0);
            let actual = result.findings.len();
            aggregate.add(expected, actual);
            per_rule
                .get_mut(&result.rule.id)
                .expect("pack and catalog ids agree")
                .add(expected, actual);
            assert_eq!(
                actual, expected,
                "case {} ({}) rule {}: expected {expected}, got {actual}: {:#?}",
                case.id, case.category, result.rule.id, result.findings
            );
        }
    }

    assert!(
        aggregate.precision() >= catalog.thresholds.aggregate_precision,
        "aggregate precision {} below {}: {aggregate:?}",
        aggregate.precision(),
        catalog.thresholds.aggregate_precision
    );
    assert!(
        aggregate.recall() >= catalog.thresholds.aggregate_recall,
        "aggregate recall {} below {}: {aggregate:?}",
        aggregate.recall(),
        catalog.thresholds.aggregate_recall
    );
    for (id, counts) in per_rule {
        assert!(
            counts.precision() >= catalog.thresholds.per_rule_precision,
            "{id} precision {} below {}: {counts:?}",
            counts.precision(),
            catalog.thresholds.per_rule_precision
        );
        assert!(
            counts.recall() >= catalog.thresholds.per_rule_recall,
            "{id} recall {} below {}: {counts:?}",
            counts.recall(),
            catalog.thresholds.per_rule_recall
        );
    }
}
