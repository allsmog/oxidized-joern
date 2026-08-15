//! Generated labeled quality gate for every promoted non-C default pack.

use cpg_cli::{build_project_from_sources, rules, scan};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    schema_version: u32,
    thresholds: Thresholds,
    languages: Vec<LanguageCases>,
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
struct LanguageCases {
    id: String,
    extension: String,
    rules: Vec<Probe>,
}

#[derive(Debug, Deserialize)]
struct Probe {
    id: String,
    source: String,
    sink: String,
}

#[derive(Default, Debug)]
struct Counts {
    tp: usize,
    fp: usize,
    fn_: usize,
}

impl Counts {
    fn add(&mut self, expected: usize, actual: usize) {
        self.tp += expected.min(actual);
        self.fp += actual.saturating_sub(expected);
        self.fn_ += expected.saturating_sub(actual);
    }

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
}

fn call(spec: &str, value: &str) -> String {
    let (name, index) = spec
        .rsplit_once('@')
        .and_then(|(name, index)| index.parse::<usize>().ok().map(|index| (name, index)))
        .unwrap_or((spec, 0));
    let mut args = vec!["0"; index + 1];
    args[index] = value;
    format!("{name}({})", args.join(", "))
}

fn unit_source(language: &str, body: &[String]) -> String {
    match language {
        "go" => format!(
            "package corpus\nfunc quality_case() {{\n{}\n}}\n",
            body.iter()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
        "java" => format!(
            "class QualityCase {{ void qualityCase() {{\n{}\n}} }}\n",
            body.iter()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
        "python" => format!(
            "def quality_case():\n{}\n",
            body.iter()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
        "ruby" => format!(
            "def quality_case\n{}\nend\n",
            body.iter()
                .map(|line| format!("  {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
        "rust" => format!(
            "fn quality_case() {{\n{}\n}}\n",
            body.iter()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
        "cpp" => format!(
            "void quality_case() {{\n{}\n}}\n",
            body.iter()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
        _ => format!(
            "function quality_case() {{\n{}\n}}\n",
            body.iter()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    }
}

fn terminated(language: &str, expression: String) -> String {
    if matches!(language, "python" | "ruby") {
        expression
    } else {
        format!("{expression};")
    }
}

fn multi_sources(language: &str, probes: &[Probe]) -> String {
    let definitions: Vec<String> = probes
        .iter()
        .enumerate()
        .map(|(index, probe)| match language {
            "go" => format!(
                "func externalInput{index}() any {{ return {}() }}",
                probe.source
            ),
            "java" => format!(
                "static Object externalInput{index}() {{ return {}(); }}",
                probe.source
            ),
            "python" => format!(
                "def external_input_{index}():\n    return {}()",
                probe.source
            ),
            "ruby" => format!("def external_input_{index}\n  {}()\nend", probe.source),
            "rust" => format!(
                "fn external_input_{index}() -> usize {{ {}() }}",
                probe.source
            ),
            "cpp" => format!(
                "void *externalInput{index}() {{ return {}(); }}",
                probe.source
            ),
            _ => format!(
                "function externalInput{index}() {{ return {}(); }}",
                probe.source
            ),
        })
        .collect();
    if language == "go" {
        format!("package corpus\n{}\n", definitions.join("\n"))
    } else if language == "java" {
        format!("class Sources {{ {} }}\n", definitions.join("\n"))
    } else {
        format!("{}\n", definitions.join("\n"))
    }
}

fn multi_sinks(language: &str, probes: &[Probe]) -> String {
    let body: Vec<String> = probes
        .iter()
        .enumerate()
        .map(|(index, probe)| {
            let source = match language {
                "python" | "ruby" | "rust" => format!("external_input_{index}()"),
                "java" => format!("Sources.externalInput{index}()"),
                _ => format!("externalInput{index}()"),
            };
            terminated(language, call(&probe.sink, &source))
        })
        .collect();
    unit_source(language, &body)
}

fn run_case(
    language: &str,
    sources: Vec<(String, String)>,
    expected: usize,
    pack: &rules::RulePack,
    totals: &mut BTreeMap<String, Counts>,
) {
    let project = build_project_from_sources(language, &sources, None)
        .unwrap_or_else(|error| panic!("{language}: build failed: {error}"));
    for result in scan::run_pack(&project, pack) {
        let actual = result.findings.len();
        totals
            .get_mut(&result.rule.id)
            .unwrap()
            .add(expected, actual);
        assert_eq!(
            actual, expected,
            "{language}/{}: expected {expected}, got {actual}: {:#?}",
            result.rule.id, result.findings
        );
    }
}

#[test]
fn promoted_language_rules_meet_labeled_quality_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../acceptance/rules/languages.json");
    let catalog: Catalog = serde_json::from_str(&std::fs::read_to_string(root).unwrap()).unwrap();
    assert_eq!(catalog.schema_version, 1);

    let mut aggregate = Counts::default();
    for language in catalog.languages {
        let pack = rules::builtin_pack(&language.id)
            .unwrap_or_else(|| panic!("{} has no built-in pack", language.id));
        let pack_ids: BTreeSet<_> = pack.rules.iter().map(|rule| rule.id.as_str()).collect();
        let probe_ids: BTreeSet<_> = language.rules.iter().map(|rule| rule.id.as_str()).collect();
        assert_eq!(pack_ids, probe_ids, "{} catalog coverage", language.id);
        let mut totals: BTreeMap<String, Counts> = language
            .rules
            .iter()
            .map(|probe| (probe.id.clone(), Counts::default()))
            .collect();

        let positive: Vec<String> = language
            .rules
            .iter()
            .map(|probe| {
                terminated(
                    &language.id,
                    call(&probe.sink, &format!("{}()", probe.source)),
                )
            })
            .collect();
        run_case(
            &language.id,
            vec![(
                format!("positive.{}", language.extension),
                unit_source(&language.id, &positive),
            )],
            1,
            &pack,
            &mut totals,
        );

        let negative: Vec<String> = language
            .rules
            .iter()
            .flat_map(|probe| {
                [
                    terminated(&language.id, format!("{}()", probe.source)),
                    terminated(&language.id, call(&probe.sink, "0")),
                ]
            })
            .collect();
        for category in ["near-miss", "fixed"] {
            run_case(
                &language.id,
                vec![(
                    format!("{category}.{}", language.extension),
                    unit_source(&language.id, &negative),
                )],
                0,
                &pack,
                &mut totals,
            );
        }

        run_case(
            &language.id,
            vec![
                (
                    format!("multi-source.{}", language.extension),
                    multi_sources(&language.id, &language.rules),
                ),
                (
                    format!("multi-sink.{}", language.extension),
                    multi_sinks(&language.id, &language.rules),
                ),
            ],
            1,
            &pack,
            &mut totals,
        );

        for (id, counts) in totals {
            assert!(
                counts.precision() >= catalog.thresholds.per_rule_precision,
                "{id} precision {}: {counts:?}",
                counts.precision()
            );
            assert!(
                counts.recall() >= catalog.thresholds.per_rule_recall,
                "{id} recall {}: {counts:?}",
                counts.recall()
            );
            aggregate.tp += counts.tp;
            aggregate.fp += counts.fp;
            aggregate.fn_ += counts.fn_;
        }
    }
    assert!(aggregate.precision() >= catalog.thresholds.aggregate_precision);
    assert!(aggregate.recall() >= catalog.thresholds.aggregate_recall);
}
