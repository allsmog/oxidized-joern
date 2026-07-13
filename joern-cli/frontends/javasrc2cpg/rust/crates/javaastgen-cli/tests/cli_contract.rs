use serde_json::Value;
use std::fs;
use std::process::Command;

#[test]
fn version_flag_prints_crate_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_javaastgen"))
        .arg("--version")
        .output()
        .expect("running javaastgen --version");

    assert!(
        output.status.success(),
        "javaastgen --version failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("v{}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn writes_one_json_document_per_java_file() {
    let temp = tempfile::tempdir().expect("temp dir");
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(input.join("demo")).expect("creating input dirs");
    fs::write(
        input.join("demo").join("Sample.java"),
        "package demo;\nclass Sample { int value() { return 1; } }\n",
    )
    .expect("writing sample");

    let output = Command::new(env!("CARGO_BIN_EXE_javaastgen"))
        .args(["-out", out.to_str().unwrap()])
        .arg(&input)
        .output()
        .expect("running javaastgen");

    assert!(
        output.status.success(),
        "javaastgen failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let json_path = out.join("demo").join("Sample.java.json");
    assert!(json_path.exists(), "missing {}", json_path.display());
    let document: Value =
        serde_json::from_slice(&fs::read(json_path).expect("reading json")).expect("parsing json");

    assert_eq!(document["relativeName"], "demo/Sample.java");
    assert_eq!(document["ast"]["kind"], "program");
    assert_eq!(
        document["ast"]["children"][0]["kind"],
        "package_declaration"
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Converted AST"));
}

#[test]
fn exclude_regex_skips_matching_files() {
    let temp = tempfile::tempdir().expect("temp dir");
    let input = temp.path().join("src");
    let out = temp.path().join("out");
    fs::create_dir_all(&input).expect("creating input dir");
    fs::write(input.join("Keep.java"), "class Keep {}\n").expect("writing keep");
    fs::write(input.join("Skip.java"), "class Skip {}\n").expect("writing skip");

    let output = Command::new(env!("CARGO_BIN_EXE_javaastgen"))
        .args(["-out", out.to_str().unwrap(), "-exclude", "Skip\\.java"])
        .arg(&input)
        .output()
        .expect("running javaastgen");

    assert!(
        output.status.success(),
        "javaastgen failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out.join("Keep.java.json").exists());
    assert!(!out.join("Skip.java.json").exists());
}
