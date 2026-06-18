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
