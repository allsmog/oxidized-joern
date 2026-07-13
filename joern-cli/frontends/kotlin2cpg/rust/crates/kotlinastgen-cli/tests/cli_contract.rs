use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn reports_version() {
    let mut cmd = Command::cargo_bin("kotlinastgen").unwrap();
    cmd.arg("-version")
        .assert()
        .success()
        .stdout(predicate::str::contains("v0.1.0"));
}

#[test]
fn extracts_minimal_kotlin_file() {
    let input = tempdir().unwrap();
    let output = tempdir().unwrap();
    let sample = input.path().join("demo").join("Sample.kt");
    fs::create_dir_all(sample.parent().unwrap()).unwrap();
    fs::write(
        &sample,
        "package demo\nclass Sample {\n  fun value(): Int = 1\n}\n",
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("kotlinastgen").unwrap();
    cmd.arg("-out")
        .arg(output.path())
        .arg(input.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Converted AST for"));

    let json_path = output.path().join("demo").join("Sample.kt.json");
    let json = fs::read_to_string(json_path).unwrap();
    assert!(json.contains("\"relativeName\": \"demo/Sample.kt\""));
    assert!(json.contains("\"kind\": \"class_declaration\""));
}
