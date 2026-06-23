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
    assert_eq!(document["backend"], "oxidized-cxxastgen");
    assert_eq!(document["language"], "c");
    assert_eq!(document["sourceLines"], 3);
    assert_eq!(
        document["options"]["defines"],
        serde_json::json!(["VALUE=7"])
    );
    assert_eq!(document["options"]["skipFunctionBodies"], true);
    assert_eq!(document["options"]["importHeaderDeclarations"], false);
    assert!(document["options"]["includePaths"][0]
        .as_str()
        .unwrap()
        .ends_with("/include"));
    assert!(!out.join("ignored.txt.json").exists());
}

#[test]
fn reports_unmapped_node_kinds_on_stderr_only() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    // A GCC statement-expression has no dedicated mapping and falls through to the
    // recorded fallback, so the run should surface a stderr summary.
    fs::write(
        input.join("main.c"),
        "int main() { int x = ({ 1; }); return x; }\n",
    )
    .unwrap();

    let output = Command::cargo_bin("cxxastgen")
        .unwrap()
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("cxxastgen:") && stderr.contains("unmapped node(s):"),
        "expected unmapped summary on stderr, got: {stderr:?}"
    );

    // The summary must never reach stdout or the emitted JSON document.
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        !stdout.contains("unmapped node(s)"),
        "stdout was: {stdout:?}"
    );
    let document = fs::read_to_string(out.join("main.c.json")).unwrap();
    assert!(
        !document.contains("unmapped node(s)"),
        "JSON document leaked the summary"
    );
    // The JSON is still valid.
    let _: Value = serde_json::from_str(&document).unwrap();
}

#[test]
fn fully_mapped_source_emits_no_unmapped_summary() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).unwrap();
    fs::write(
        input.join("main.c"),
        "int add(int a, int b) { int total = a + b; return total; }\n",
    )
    .unwrap();

    let output = Command::cargo_bin("cxxastgen")
        .unwrap()
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !stderr.contains("unmapped node(s)"),
        "did not expect an unmapped summary, got: {stderr:?}"
    );
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

#[test]
fn uses_compile_database_sources_and_options() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("project");
    let src = input.join("src");
    let include = input.join("include");
    let system_include = input.join("system-include");
    let cli_include = input.join("cli-include");
    let out = temp.path().join("out");
    let compile_database = input.join("compile_commands.json");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&include).unwrap();
    fs::create_dir_all(&system_include).unwrap();
    fs::create_dir_all(&cli_include).unwrap();
    fs::write(src.join("main.c"), "int main() { return DB_DEFINE; }\n").unwrap();
    fs::write(src.join("not_in_database.c"), "int stray() { return 0; }\n").unwrap();
    fs::write(
        &compile_database,
        serde_json::to_string_pretty(&serde_json::json!([
            {
                "directory": input,
                "file": "src/main.c",
                "arguments": [
                    "cc",
                    "-I",
                    "include",
                    "-isystem",
                    "system-include",
                    "-DDB_DEFINE=1",
                    "/DMSVC_DEFINE",
                    "-c",
                    "src/main.c"
                ]
            }
        ]))
        .unwrap(),
    )
    .unwrap();

    let mut command = Command::cargo_bin("cxxastgen").unwrap();
    command
        .arg("-include")
        .arg(&cli_include)
        .arg("-define")
        .arg("CLI_DEFINE=1")
        .arg("-compilation-database")
        .arg(&compile_database)
        .arg("-out")
        .arg(&out)
        .arg(&input)
        .assert()
        .success();

    assert!(out.join("src/main.c.json").exists());
    assert!(!out.join("src/not_in_database.c.json").exists());

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("src/main.c.json")).unwrap()).unwrap();
    let include_paths = document["options"]["includePaths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(include_paths
        .iter()
        .any(|path| path.ends_with("/cli-include")));
    assert!(include_paths
        .iter()
        .any(|path| path.ends_with("/project/include")));
    assert!(include_paths
        .iter()
        .any(|path| path.ends_with("/project/system-include")));
    assert_eq!(
        document["options"]["defines"],
        serde_json::json!(["CLI_DEFINE=1", "DB_DEFINE=1", "MSVC_DEFINE"])
    );
    assert!(document["options"]["compilationDatabase"]
        .as_str()
        .unwrap()
        .ends_with("/compile_commands.json"));
    assert_eq!(document["options"]["importHeaderDeclarations"], true);
}

#[test]
fn parses_compile_database_command_lines() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("project");
    let include = input.join("quoted include");
    let out = temp.path().join("out");
    let compile_database = input.join("compile_commands.json");
    let source = input.join("main.c");
    fs::create_dir_all(&include).unwrap();
    fs::write(&source, "int main() { return QUOTED_DEFINE; }\n").unwrap();
    fs::write(
        &compile_database,
        serde_json::to_string_pretty(&serde_json::json!([
            {
                "directory": input,
                "file": "main.c",
                "command": "cc -I 'quoted include' -DQUOTED_DEFINE=1 -c main.c"
            }
        ]))
        .unwrap(),
    )
    .unwrap();

    let mut command = Command::cargo_bin("cxxastgen").unwrap();
    command
        .arg("-compilation-database")
        .arg(&compile_database)
        .arg("-out")
        .arg(&out)
        .arg(&source)
        .assert()
        .success();

    let document: Value =
        serde_json::from_str(&fs::read_to_string(out.join("main.c.json")).unwrap()).unwrap();
    let include_paths = document["options"]["includePaths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(include_paths
        .iter()
        .any(|path| path.ends_with("/project/quoted include")));
    assert_eq!(
        document["options"]["defines"],
        serde_json::json!(["QUOTED_DEFINE=1"])
    );
}
