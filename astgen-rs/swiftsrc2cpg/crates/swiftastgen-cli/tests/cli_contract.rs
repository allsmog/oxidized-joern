use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

#[test]
fn honors_default_ignored_directories_and_java_quoted_exclude_regex() {
    let input = TempDir::new().unwrap();
    let output = TempDir::new().unwrap();

    for dir in [".sub", "__sub", "tests", "specs", "test", "spec", "folder"] {
        fs::create_dir_all(input.path().join(dir)).unwrap();
    }
    fs::write(input.path().join("a.swift"), "let a = 1\n").unwrap();
    fs::write(input.path().join("index.swift"), "let index = 1\n").unwrap();
    fs::write(input.path().join("folder").join("b.swift"), "let b = 1\n").unwrap();
    fs::write(input.path().join(".sub").join("e.swift"), "let e = 1\n").unwrap();
    fs::write(input.path().join("__sub").join("x.swift"), "let x = 1\n").unwrap();
    fs::write(input.path().join("tests").join("x.swift"), "let x = 1\n").unwrap();
    fs::write(input.path().join("specs").join("x.swift"), "let x = 1\n").unwrap();
    fs::write(input.path().join("test").join("x.swift"), "let x = 1\n").unwrap();
    fs::write(input.path().join("spec").join("x.swift"), "let x = 1\n").unwrap();

    Command::cargo_bin("SwiftAstGen")
        .unwrap()
        .args(["-o"])
        .arg(output.path())
        .args(["--exclude-regex", r".*\Q/\E?folder\Q/\E.*"])
        .arg(input.path())
        .assert()
        .success();

    assert!(output.path().join("a.swift.json").exists());
    assert!(output.path().join("index.swift.json").exists());
    assert!(!output.path().join("folder").join("b.swift.json").exists());
    assert!(!output.path().join(".sub").join("e.swift.json").exists());
    assert!(!output.path().join("__sub").join("x.swift.json").exists());
    assert!(!output.path().join("tests").join("x.swift.json").exists());
    assert!(!output.path().join("specs").join("x.swift.json").exists());
    assert!(!output.path().join("test").join("x.swift.json").exists());
    assert!(!output.path().join("spec").join("x.swift.json").exists());
}
