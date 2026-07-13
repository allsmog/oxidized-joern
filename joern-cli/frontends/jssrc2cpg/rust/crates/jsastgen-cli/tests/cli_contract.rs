use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use tempfile::TempDir;

#[test]
fn reports_version() {
    Command::cargo_bin("astgen")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout("0.1.0\n");
}

#[test]
fn writes_babel_shaped_json_for_javascript_sources() {
    let input = TempDir::new().unwrap();
    let output = TempDir::new().unwrap();
    fs::write(
        input.path().join("app.js"),
        "const answer = 40 + 2;\nfunction id(x) { return x; }\nid(answer);\n",
    )
    .unwrap();

    Command::cargo_bin("astgen")
        .unwrap()
        .args(["-t", "ts", "-o"])
        .arg(output.path())
        .arg(input.path())
        .assert()
        .success();

    let json_path = output.path().join("app.js.json");
    let document: Value = serde_json::from_slice(&fs::read(json_path).unwrap()).unwrap();
    assert_eq!(document["relativeName"], "app.js");
    assert_eq!(document["ast"]["type"], "File");
    assert_eq!(
        document["ast"]["program"]["body"][0]["type"],
        "VariableDeclaration"
    );
    assert_eq!(
        document["ast"]["program"]["body"][1]["type"],
        "FunctionDeclaration"
    );
    assert_eq!(
        document["ast"]["program"]["body"][2]["type"],
        "ExpressionStatement"
    );
}

#[test]
fn writes_babel_shaped_json_for_typescript_sources() {
    let input = TempDir::new().unwrap();
    let output = TempDir::new().unwrap();
    fs::write(
        input.path().join("app.ts"),
        "for (foo().x of arr) { bar(); }\n",
    )
    .unwrap();

    Command::cargo_bin("astgen")
        .unwrap()
        .args(["-t", "ts", "-o"])
        .arg(output.path())
        .arg(input.path())
        .assert()
        .success();

    let json_path = output.path().join("app.ts.json");
    let document: Value = serde_json::from_slice(&fs::read(json_path).unwrap()).unwrap();
    assert_eq!(document["relativeName"], "app.ts");
    assert_eq!(
        document["ast"]["program"]["body"][0]["type"],
        "ForOfStatement"
    );
}

#[test]
fn writes_babel_shaped_json_for_vue_sources() {
    let input = TempDir::new().unwrap();
    let output = TempDir::new().unwrap();
    fs::write(
        input.path().join("App.vue"),
        "<template><h1>{{ msg }}</h1></template>\n<script lang=\"ts\">export default class App {}</script>\n",
    )
    .unwrap();
    fs::write(input.path().join("app.ts"), "const skipped = true;\n").unwrap();

    Command::cargo_bin("astgen")
        .unwrap()
        .args(["-t", "vue", "-o"])
        .arg(output.path())
        .arg(input.path())
        .assert()
        .success();

    let vue_json_path = output.path().join("App.vue.json");
    let document: Value = serde_json::from_slice(&fs::read(vue_json_path).unwrap()).unwrap();
    assert_eq!(document["relativeName"], "App.vue");
    assert_eq!(
        document["ast"]["program"]["body"][0]["type"],
        "ExpressionStatement"
    );
    assert_eq!(
        document["ast"]["program"]["body"][1]["type"],
        "ExportDefaultDeclaration"
    );
    assert!(!output.path().join("app.ts.json").exists());
}

#[test]
fn honors_exclude_files_regexes_and_hidden_directories() {
    let input = TempDir::new().unwrap();
    let output = TempDir::new().unwrap();
    fs::create_dir_all(input.path().join("folder")).unwrap();
    fs::create_dir_all(input.path().join("regexed")).unwrap();
    fs::create_dir_all(input.path().join(".sub")).unwrap();
    fs::write(input.path().join("keep.js"), "const keep = true;\n").unwrap();
    fs::write(input.path().join("index.js"), "const excluded = true;\n").unwrap();
    fs::write(
        input.path().join("folder").join("nested.js"),
        "const nested = true;\n",
    )
    .unwrap();
    fs::write(
        input.path().join("regexed").join("skip.js"),
        "const regexed = true;\n",
    )
    .unwrap();
    fs::write(
        input.path().join(".sub").join("hidden.js"),
        "const hidden = true;\n",
    )
    .unwrap();

    Command::cargo_bin("astgen")
        .unwrap()
        .args(["-t", "ts", "-o"])
        .arg(output.path())
        .args(["--exclude-file", "index.js"])
        .args(["--exclude-file", "folder"])
        .args(["--exclude-regex", r".*\Q/\E?regexed\Q/\E.*"])
        .arg(input.path())
        .assert()
        .success();

    assert!(output.path().join("keep.js.json").exists());
    assert!(!output.path().join("index.js.json").exists());
    assert!(!output.path().join("folder").join("nested.js.json").exists());
    assert!(!output.path().join("regexed").join("skip.js.json").exists());
    assert!(!output.path().join(".sub").join("hidden.js.json").exists());
}
