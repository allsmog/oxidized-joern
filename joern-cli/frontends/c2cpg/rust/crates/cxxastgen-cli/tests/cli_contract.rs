use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use tempfile::tempdir;

#[test]
fn prints_version() {
    let mut command = Command::cargo_bin("cxxastgen").unwrap();
    command
        .arg("-version")
        .assert()
        .success()
        .stdout("v0.1.0\n");
}

#[test]
fn writes_one_json_document_per_cxx_input() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let include = temp.path().join("include");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&include).unwrap();
    fs::write(input.join("main.c"), "int main() {\n  return VALUE;\n}\n").unwrap();
    fs::write(input.join("ignored.txt"), "not c").unwrap();

    let mut command = Command::cargo_bin("cxxastgen").unwrap();
    command
        .arg("-include")
        .arg(&include)
        .arg("-define")
        .arg("VALUE=7")
        .arg("-skip-function-bodies")
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .assert()
        .success();

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("main.c.json")).unwrap()).unwrap();
    assert_eq!(document["schemaVersion"], 1);
    assert_eq!(document["backend"], "oxidized-cxxastgen-scaffold");
    assert_eq!(document["language"], "c");
    assert_eq!(document["sourceLines"], 3);
    assert_eq!(
        document["options"]["defines"],
        serde_json::json!(["VALUE=7"])
    );
    assert_eq!(document["options"]["skipFunctionBodies"], true);
    assert!(document["options"]["includePaths"][0]
        .as_str()
        .unwrap()
        .ends_with("/include"));
    assert!(!out.join("ignored.txt.json").exists());
}

#[test]
fn applies_exclude_regex() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::write(input.join("keep.c"), "int keep() { return 1; }\n").unwrap();
    fs::write(input.join("skip.c"), "int skip() { return 0; }\n").unwrap();

    let mut command = Command::cargo_bin("cxxastgen").unwrap();
    command
        .arg("-exclude")
        .arg("skip\\.c$")
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .assert()
        .success();

    assert!(out.join("keep.c.json").exists());
    assert!(!out.join("skip.c.json").exists());
}
