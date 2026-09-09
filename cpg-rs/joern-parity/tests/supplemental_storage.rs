use cpg_core::{Cpg, EdgeKind, NodeKind};
use serde_json::Value;
use std::process::Command;

#[test]
fn supplemental_cli_reads_the_actual_saved_graph_without_rebuilding() {
    let mut cpg = Cpg::new();
    let file = cpg.file_id("source file deliberately unavailable.c");
    let binding = cpg.add_node(NodeKind::Binding, file);
    let method = cpg.add_node(NodeKind::Method, file);
    let sym = cpg.intern("literal\nMETHOD_FULL_NAME=preserved\r\u{2028}雪");
    cpg.set_name(binding, sym);
    cpg.set_method_full_name(binding, sym);
    for _ in 0..2 {
        cpg.add_edge(binding, method, EdgeKind::Ref);
    }
    let directory = tempfile::tempdir().unwrap();
    let graph = directory.path().join("saved.cpg");
    cpg.save(graph.to_str().unwrap()).unwrap();
    let original = std::fs::read(&graph).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_joern-parity"))
        .arg("--supplemental-cpg")
        .arg(&graph)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: Vec<Value> = output
        .stdout
        .split(|&b| b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(rows[0]["nodeCount"], "2");
    assert_eq!(rows[0]["edgeCount"], "2");
    let observed = rows
        .iter()
        .find(|row| row["record"] == "NODE" && row["label"] == "BINDING")
        .unwrap();
    assert_eq!(
        observed["properties"]["METHOD_FULL_NAME"]["value"],
        cpg.method_full_name_of(binding).unwrap()
    );
    assert!(observed["properties"].get("FULL_NAME").is_none());
    assert_eq!(
        rows.iter()
            .filter(|row| row["record"] == "EDGE" && row["label"] == "REF")
            .count(),
        2
    );
    assert_eq!(std::fs::read(&graph).unwrap(), original);
    assert_eq!(rows.last().unwrap()["record"], "END");
}
