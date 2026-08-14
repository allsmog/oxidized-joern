use std::process::Command;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "cpg-cli-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn version_matches_the_cargo_package() {
    let output = Command::new(env!("CARGO_BIN_EXE_cpg"))
        .arg("--version")
        .output()
        .expect("run cpg --version");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("version is UTF-8"),
        format!("cpg {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn help_is_successful_and_lists_public_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_cpg"))
        .arg("--help")
        .output()
        .expect("run cpg --help");

    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("help is UTF-8");
    for command in [
        "build", "scan", "slice", "merge", "apis", "export", "flow", "vectors", "serve", "mcp",
    ] {
        assert!(
            help.contains(&format!("cpg {command}")),
            "missing {command}"
        );
    }
    assert!(output.stderr.is_empty());
}

#[test]
fn build_rejects_invalid_language_without_creating_output() {
    let root = scratch("invalid-language");
    std::fs::write(root.join("main.c"), "int main(void) { return 0; }").unwrap();
    let graph = root.join("should-not-exist.cpg");
    let output = Command::new(env!("CARGO_BIN_EXE_cpg"))
        .args([
            "build",
            &root.to_string_lossy(),
            "-o",
            &graph.to_string_lossy(),
            "--lang",
            "pyhton",
        ])
        .output()
        .expect("run cpg build");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported language 'pyhton'"));
    assert!(!graph.exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn build_rejects_missing_or_empty_root_without_creating_output() {
    let root = scratch("missing-empty");
    let missing = root.join("missing");
    let graph = root.join("should-not-exist.cpg");
    for source_root in [&missing, &root] {
        let output = Command::new(env!("CARGO_BIN_EXE_cpg"))
            .args([
                "build",
                &source_root.to_string_lossy(),
                "-o",
                &graph.to_string_lossy(),
                "--lang",
                "c",
            ])
            .output()
            .expect("run cpg build");
        assert!(
            !output.status.success(),
            "accepted {}",
            source_root.display()
        );
        assert!(!graph.exists());
    }
    let _ = std::fs::remove_dir_all(root);
}
