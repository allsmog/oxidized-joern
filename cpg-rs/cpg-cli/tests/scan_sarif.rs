//! Integration tests for the Gap 5 scan/rule/SARIF layer: build a tiny
//! vulnerable C project in a tempdir, run the same code paths the `cpg scan`
//! subcommand and the serve loop use, and check the SARIF/JSON output.

use cpg_cli::{build_project, handle, rules::RulePack, scan};
use serde_json::{json, Value};
use std::path::PathBuf;

/// A tempdir that removes itself, so failed asserts don't leak fixtures.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "cpg-scan-test-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const VULN_C: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
    char *cmd = getenv("USER_CMD");
    system(cmd);
    return 0;
}

void echo(void) {
    char in[256];
    char out[64];
    char *line = gets(in);
    strcpy(out, line);
}
"#;

fn example_pack() -> RulePack {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/rules/default.json");
    RulePack::from_file(path).expect("example rule pack must parse")
}

#[test]
fn scan_emits_valid_sarif_with_codeflows() {
    let tmp = TempDir::new("sarif");
    let src = tmp.0.join("vuln.c");
    std::fs::write(&src, VULN_C).unwrap();

    let project = build_project(tmp.0.to_str().unwrap(), "c");
    let pack = example_pack();
    let log = scan::scan_to_sarif(&project, &pack, tmp.0.to_str().unwrap());
    let text = log.to_json_pretty();

    // The emitted SARIF must parse back as JSON.
    let parsed: Value = serde_json::from_str(&text).expect("SARIF output is valid JSON");
    assert_eq!(parsed["version"], "2.1.0");
    assert!(parsed["$schema"].as_str().unwrap().contains("sarif-schema-2.1.0"));

    // The rule pack is mapped onto tool.driver.rules with metadata.
    let rules = parsed["runs"][0]["tool"]["driver"]["rules"].as_array().unwrap();
    let cpg001 = rules
        .iter()
        .find(|r| r["id"] == "CPG-001")
        .expect("CPG-001 present in driver.rules");
    assert_eq!(cpg001["properties"]["cwe"], "CWE-78");
    assert_eq!(cpg001["properties"]["severity"], "high");
    assert_eq!(cpg001["defaultConfiguration"]["level"], "error");
    assert_eq!(cpg001["name"], "env-to-system");

    // getenv -> system must be reported under CPG-001 …
    let results = parsed["runs"][0]["results"].as_array().unwrap();
    let hit = results
        .iter()
        .find(|r| r["ruleId"] == "CPG-001")
        .expect("expected a CPG-001 finding for getenv->system");
    assert_eq!(hit["level"], "error");

    // … with a codeFlow whose threadFlow replays the witness (>= 2 steps),
    // each step carrying code text and a physical location in vuln.c.
    let steps = hit["codeFlows"][0]["threadFlows"][0]["locations"]
        .as_array()
        .unwrap();
    assert!(steps.len() >= 2, "witness path must have >= 2 steps, got {}", steps.len());
    for step in steps {
        assert!(step["location"]["message"]["text"].as_str().unwrap().len() > 0);
        let uri = step["location"]["physicalLocation"]["artifactLocation"]["uri"]
            .as_str()
            .unwrap();
        assert!(uri.ends_with("vuln.c"), "step uri should point at the fixture, got {uri}");
        assert!(
            step["location"]["physicalLocation"]["region"]["startLine"].as_u64().unwrap() >= 1
        );
    }
    let codes: Vec<&str> = steps
        .iter()
        .map(|s| s["location"]["message"]["text"].as_str().unwrap())
        .collect();
    assert!(codes.first().unwrap().contains("getenv"));
    assert!(codes.last().unwrap().contains("system"));

    // gets -> strcpy must be reported under CPG-002 (second real rule).
    assert!(
        results.iter().any(|r| r["ruleId"] == "CPG-002"),
        "expected a CPG-002 finding for gets->strcpy"
    );

    // The top-level result location points at the sink line.
    assert!(
        hit["locations"][0]["physicalLocation"]["region"]["startLine"].as_u64().unwrap() >= 1
    );
}

#[test]
fn server_scan_command_groups_findings_by_rule_id() {
    let tmp = TempDir::new("serve");
    std::fs::write(tmp.0.join("vuln.c"), VULN_C).unwrap();
    let mut project = build_project(tmp.0.to_str().unwrap(), "c");

    // Inline rules, including forward-compat keys the loader must ignore.
    let resp = handle(
        &mut project,
        &json!({"cmd": "scan", "rules": [
            {"id": "CPG-001", "name": "env-to-system", "cwe": "CWE-78",
             "severity": "high", "sources": ["getenv"], "sinks": ["system"],
             "sanitizers": ["shell_escape"], "someFutureKey": [1, 2, 3]},
            {"id": "CPG-XXX", "sources": ["nonexistent_fn"], "sinks": ["also_absent"]}
        ]}),
    );

    let grouped = resp["findings"].as_object().expect("findings grouped by rule id");
    let hits = grouped["CPG-001"].as_array().unwrap();
    assert!(!hits.is_empty(), "CPG-001 should fire on the fixture");
    assert_eq!(hits[0]["sink"], "system");
    assert_eq!(hits[0]["origin"], "getenv");
    assert!(hits[0]["path"].as_array().unwrap().len() >= 2);
    // A rule that matches nothing still appears, with an empty group.
    assert_eq!(grouped["CPG-XXX"].as_array().unwrap().len(), 0);

    // Malformed rules are rejected with an error, not a panic.
    let bad = handle(&mut project, &json!({"cmd": "scan", "rules": [{"name": "no-id"}]}));
    assert!(bad["error"].as_str().unwrap().contains("bad rules"));
    let missing = handle(&mut project, &json!({"cmd": "scan"}));
    assert!(missing["error"].as_str().is_some());
}

/// A rule whose `sanitizers` name a function on the only source→sink path must
/// suppress the finding — the payoff of threading rule sanitizers into the
/// taint query (`find_taint_with_sanitizers`). Same fixture, with vs. without
/// the sanitizer named, to prove it is the sanitizer (not an absent flow) that
/// removes the result.
const SANITIZED_C: &str = r#"
#include <stdlib.h>

char *clean(char *s) { return s; }

int main(void) {
    char *c = getenv("USER_CMD");
    char *safe = clean(c);
    system(safe);
    return 0;
}
"#;

#[test]
fn rule_sanitizer_suppresses_finding() {
    let tmp = TempDir::new("sanitize");
    std::fs::write(tmp.0.join("vuln.c"), SANITIZED_C).unwrap();
    let project = build_project(tmp.0.to_str().unwrap(), "c");

    let base = r#"{"rules":[{"id":"CPG-1","sources":["getenv"],"sinks":["system"]}]}"#;
    let with_san = r#"{"rules":[{"id":"CPG-1","sources":["getenv"],"sinks":["system"],"sanitizers":["clean"]}]}"#;

    let base_pack = RulePack::from_json(base).unwrap();
    let no_san = scan::run_pack(&project, &base_pack);
    assert!(
        !no_san[0].findings.is_empty(),
        "without a sanitizer, getenv->clean->system is a flow"
    );

    let san_pack = RulePack::from_json(with_san).unwrap();
    let sanitized = scan::run_pack(&project, &san_pack);
    assert!(
        sanitized[0].findings.is_empty(),
        "naming `clean` as a sanitizer must suppress the only path"
    );
}
