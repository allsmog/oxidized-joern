//! Coverage gate for the ABAP statement classifier.
//!
//! Runs the real `abapgen` CLI over the fixture corpus, then asserts the run
//! succeeds and prints *no* `abapastgen: N unclassified statement(s)` summary on
//! stderr. The summary is emitted by `crates/abapastgen-cli/src/main.rs`
//! whenever `abapastgen_core::unclassified_count()` is non-zero, so an empty
//! stderr means there were no unexpected classifier fallthroughs.
//!
//! The classifier's `Unknown` counter lives in a process-global atomic, but each
//! CLI invocation is a fresh process, so the stderr gate is unaffected by test
//! ordering or parallelism.

use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

/// Control-flow, data, and security-relevant statements the classifier handles. Every one
/// of these must appear in the fixture's emitted `statements[].type` set.
const EXPECTED_TYPES: &[&str] = &[
    "If",
    "ElseIf",
    "Else",
    "EndIf",
    "Case",
    "When",
    "WhenOthers",
    "EndCase",
    "While",
    "EndWhile",
    "Do",
    "EndDo",
    "Loop",
    "EndLoop",
    "Try",
    "Catch",
    "Cleanup",
    "EndTry",
    "Check",
    "Exit",
    "Continue",
    "Return",
    "Raise",
    "Data",
    "Move",
    "Comment",
    "OpenDataset",
    "ReadDataset",
    "DeleteDataset",
    "Transfer",
    "AuthorityCheck",
    "GenerateSubroutine",
    "EditorCall",
    "Unknown",
];

#[test]
fn fixture_is_fully_classified_with_no_stderr_summary() {
    let out = tempdir().expect("creating temp out dir");
    let output = Command::new(env!("CARGO_BIN_EXE_abapgen"))
        .arg(fixture_dir())
        .arg(out.path())
        .output()
        .expect("running abapgen CLI");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "abapgen exited unsuccessfully\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // The gate: a fully classified fixture produces no unclassified summary.
    assert!(
        !stderr.contains("unclassified statement"),
        "classifier reported unclassified statements; keep the coverage gate at zero \
         by classifying the statement or excluding it from the fixture\nstderr:\n{stderr}"
    );
}

#[test]
fn emitted_statement_types_cover_control_flow() {
    let out = tempdir().expect("creating temp out dir");
    let status = Command::new(env!("CARGO_BIN_EXE_abapgen"))
        .arg(fixture_dir())
        .arg(out.path())
        .status()
        .expect("running abapgen CLI");
    assert!(status.success(), "abapgen exited unsuccessfully");

    let emitted = emitted_statement_types(out.path());
    let expected = EXPECTED_TYPES
        .iter()
        .map(|t| (*t).to_string())
        .collect::<BTreeSet<_>>();
    let missing = expected.difference(&emitted).cloned().collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "fixture does not exercise expected control-flow statement types: {missing:?}\n\
         emitted: {emitted:?}"
    );
}

fn emitted_statement_types(out: &Path) -> BTreeSet<String> {
    let mut json_files = Vec::new();
    collect_json_files(out, &mut json_files);
    assert!(
        !json_files.is_empty(),
        "fixture run produced no JSON files under {}",
        out.display()
    );

    let mut types = BTreeSet::new();
    for file in json_files {
        let bytes =
            fs::read(&file).unwrap_or_else(|err| panic!("reading {}: {err}", file.display()));
        let value: Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|err| panic!("decoding {}: {err}", file.display()));
        if let Some(statements) = value.get("statements").and_then(Value::as_array) {
            for statement in statements {
                if let Some(kind) = statement.get("type").and_then(Value::as_str) {
                    types.insert(kind.to_string());
                }
            }
        }
    }
    types
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/abap-corpus/control-flow")
}

fn collect_json_files(root: &Path, out: &mut Vec<PathBuf>) {
    if !root.exists() {
        return;
    }
    for entry in
        fs::read_dir(root).unwrap_or_else(|err| panic!("reading {}: {err}", root.display()))
    {
        let path = entry.expect("reading directory entry").path();
        if path.is_dir() {
            collect_json_files(&path, out);
        } else if path.extension().and_then(|x| x.to_str()) == Some("json") {
            out.push(path);
        }
    }
    out.sort();
}
