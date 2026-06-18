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
