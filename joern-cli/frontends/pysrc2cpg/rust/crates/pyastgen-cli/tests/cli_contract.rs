use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use tempfile::tempdir;

#[test]
fn prints_version() {
    let mut command = Command::cargo_bin("pyastgen").unwrap();
    command.arg("-version").assert().success();
}

#[test]
fn writes_one_json_document_per_python_input() {
    let input = tempdir().unwrap();
    let out = tempdir().unwrap();
    fs::write(
        input.path().join("service.py"),
        "class Service:\n    def run(self):\n        return 1\n",
    )
    .unwrap();
    fs::write(input.path().join("notes.txt"), "not python").unwrap();

    let mut command = Command::cargo_bin("pyastgen").unwrap();
    command
        .arg("-out")
        .arg(out.path())
        .arg(input.path())
        .assert()
        .success();

    let output_path = out.path().join("service.py.json");
    let value: Value = serde_json::from_slice(&fs::read(output_path).unwrap()).unwrap();
    assert_eq!(value["backend"], "oxidized-pyastgen");
    assert_eq!(value["root"]["kind"], "Module");
    assert_eq!(value["root"]["children"]["body"][0]["kind"], "ClassDef");
    assert_eq!(
        value["root"]["children"]["body"][0]["properties"]["name"],
        "Service"
    );
}

#[test]
fn honors_exclude_regex() {
    let input = tempdir().unwrap();
    let out = tempdir().unwrap();
    fs::create_dir(input.path().join("skip")).unwrap();
    fs::write(input.path().join("keep.py"), "x = 1\n").unwrap();
    fs::write(input.path().join("skip").join("drop.py"), "x = 2\n").unwrap();

    let mut command = Command::cargo_bin("pyastgen").unwrap();
    command
        .arg("-out")
        .arg(out.path())
        .arg("-exclude")
        .arg("skip")
        .arg(input.path())
        .assert()
        .success();

    assert!(out.path().join("keep.py.json").exists());
    assert!(!out.path().join("skip").join("drop.py.json").exists());
}
