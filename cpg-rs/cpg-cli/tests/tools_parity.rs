//! End-to-end coverage for the JoernExport / JoernFlow / JoernVectors
//! parity surface: export a built project per method and whole-graph,
//! run an ad-hoc glob flow query the way `cpg flow` does, and emit the
//! vectors document.

use cpg_cli::export::{export, Format, Repr};
use cpg_cli::{glob_match, make_project, rules::RulePack};
use cpg_core::Query;

const SRC: &str = r#"
#include <stdlib.h>
void doit(char *c) { system(c); }
int main() {
    char *cmd = getenv("CMD");
    doit(cmd);
    return 0;
}
"#;

fn build() -> cpg_incremental::Project {
    let (mut project, _) = make_project("c");
    project.build(&[("app.c", SRC)]);
    project
}

#[test]
fn export_splits_by_method_and_writes_dot() {
    let project = build();
    let dir = std::env::temp_dir().join(format!("cpgx-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let stats = export(&project.cpg, Repr::Cpg14, Format::Dot, &dir).expect("export");
    assert!(
        stats.files >= 2,
        "one file per method (main, doit): {}",
        stats.files
    );
    assert!(stats.nodes > 0 && stats.edges > 0);
    let main_dot = dir.join("app.c/main.dot");
    let text = std::fs::read_to_string(&main_dot).expect("main.dot written");
    assert!(text.starts_with("digraph"), "dot header: {text}");
    assert!(text.contains("getenv"), "main's subgraph carries its calls");
    assert!(
        !text.contains("system("),
        "doit's body must not leak into main's subgraph"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_all_is_one_valid_json_graph() {
    let project = build();
    let dir = std::env::temp_dir().join(format!("cpgx-export-all-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let stats = export(&project.cpg, Repr::All, Format::Json, &dir).expect("export");
    assert_eq!(stats.files, 1);
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("export.json")).unwrap())
            .expect("valid json");
    let nodes = doc["nodes"].as_array().unwrap();
    let edges = doc["edges"].as_array().unwrap();
    assert_eq!(nodes.len(), project.cpg.live_count());
    assert!(!edges.is_empty());
    // Every edge endpoint refers to an exported node id.
    let ids: std::collections::HashSet<u64> =
        nodes.iter().map(|n| n["id"].as_u64().unwrap()).collect();
    for e in edges {
        assert!(ids.contains(&e["src"].as_u64().unwrap()));
        assert!(ids.contains(&e["dst"].as_u64().unwrap()));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The `cpg flow` recipe: expand globs over call names, run the ad-hoc
/// single-rule pack. getenv->system through the helper call must surface.
#[test]
fn glob_flow_query_finds_interprocedural_flow() {
    let project = build();
    let cpg = &project.cpg;
    let sources: std::collections::BTreeSet<String> = cpg
        .calls()
        .into_iter()
        .filter_map(|c| cpg.name_of(c))
        .filter(|n| glob_match("getenv", n))
        .map(str::to_string)
        .collect();
    let sinks: std::collections::BTreeSet<String> = cpg
        .calls()
        .into_iter()
        .filter_map(|c| cpg.name_of(c))
        .filter(|n| glob_match("sys*", n))
        .map(str::to_string)
        .collect();
    assert_eq!(sources.len(), 1);
    assert_eq!(sinks.len(), 1, "glob must catch system: {sinks:?}");
    let rule_json = serde_json::json!({"rules": [{
        "id": "FLOW",
        "sources": sources,
        "sinks": sinks,
    }]});
    let pack = RulePack::from_json(&rule_json.to_string()).expect("pack");
    let per_rule = cpg_cli::scan::run_pack_entry(&project, &pack, &[], &[], &[]);
    let findings: Vec<&cpg_analysis::Finding> =
        per_rule.iter().flat_map(|rf| rf.findings.iter()).collect();
    assert!(
        findings
            .iter()
            .any(|f| f.method == "main" && f.sink == "system"),
        "getenv->doit->system must be found: {findings:?}"
    );
}

#[test]
fn vectors_document_covers_every_node() {
    let project = build();
    let mut buf = Vec::new();
    cpg_cli::vectors::write_vectors(&project.cpg, false, &mut buf).expect("vectors");
    let doc: serde_json::Value = serde_json::from_slice(&buf).expect("valid json");
    assert_eq!(
        doc["objects"].as_array().unwrap().len(),
        project.cpg.live_count()
    );
    assert_eq!(
        doc["objects"].as_array().unwrap().len(),
        doc["vectors"].as_array().unwrap().len()
    );
    assert!(
        doc.get("dimToFeature").is_none(),
        "--features off by default"
    );
}
