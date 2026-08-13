use std::process::Command;

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
