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

#[test]
fn supplemental_cli_reads_include_fields_and_explicit_absence_from_version_three() {
    let mut cpg = Cpg::new();
    let file = cpg.file_id("unavailable caller.c");
    let import = cpg.add_node(NodeKind::Import, file);
    let dependency = cpg.add_node(NodeKind::Dependency, file);
    let empty = cpg.intern("");
    cpg.set_imported_as(import, empty);
    let entity = cpg.intern("missing 雪.h");
    cpg.set_imported_entity(import, entity);
    cpg.set_dependency_group_id(dependency, entity);
    let version = cpg.intern("include");
    cpg.set_version(dependency, version);
    cpg.set_column_number(import, i32::MAX);
    cpg.set_order_property(import, i32::MIN);
    cpg.clear_order_property(dependency);
    cpg.add_edge(import, dependency, EdgeKind::Imports);
    let directory = tempfile::tempdir().unwrap();
    let graph = directory.path().join("saved.cpg");
    cpg.save(graph.to_str().unwrap()).unwrap();
    let original = std::fs::read(&graph).unwrap();
    assert_eq!(&original[..6], b"CPG2\x03\x00");
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
    let import = rows
        .iter()
        .find(|row| row["record"] == "NODE" && row["label"] == "IMPORT")
        .unwrap();
    let dependency = rows
        .iter()
        .find(|row| row["record"] == "NODE" && row["label"] == "DEPENDENCY")
        .unwrap();
    assert_eq!(
        import["properties"]["IMPORTED_ENTITY"]["value"],
        "missing 雪.h"
    );
    assert_eq!(import["properties"]["IMPORTED_AS"]["value"], "");
    assert_eq!(
        import["properties"]["COLUMN_NUMBER"]["value"],
        i32::MAX.to_string()
    );
    assert_eq!(import["properties"]["ORDER"]["value"], i32::MIN.to_string());
    assert_eq!(
        dependency["properties"]["DEPENDENCY_GROUP_ID"]["value"],
        "missing 雪.h"
    );
    assert_eq!(dependency["properties"]["VERSION"]["value"], "include");
    assert!(dependency["properties"].get("ORDER").is_none());
    assert!(dependency["properties"].get("COLUMN_NUMBER").is_none());
    assert_eq!(dependency["storage"]["orderPropertyPresence"], "absent");
    let edge = rows.iter().find(|row| row["record"] == "EDGE").unwrap();
    assert_eq!(edge["label"], "IMPORTS");
    assert_eq!(edge["source"], import["id"]);
    assert_eq!(edge["destination"], dependency["id"]);
    assert_eq!(std::fs::read(&graph).unwrap(), original);
}
