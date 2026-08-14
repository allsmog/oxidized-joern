use assert_cmd::Command;
use std::env;
use std::fs;
use std::path::PathBuf;

/// PHP fixture corpus exercising the common constructs the lowering passes must
/// map: classes/interfaces, traits with adaptations, namespaces, `match`, and
/// heredocs (see `fixtures/php-corpus/<feature>/`).
///
/// Every construct in the corpus must lower without leaving an unmapped
/// tree-sitter node, so the `phpastgen: N unmapped node(s): …` stderr summary
/// acts as a coverage gate. Adding a construct that the core cannot yet map will
/// fail this test with the offending kinds, prompting explicit Rust handling
/// or a fixture change. The same corpus backs the differential parity harness in
/// `differential_json.rs`.
#[test]
fn corpus_lowers_with_zero_unmapped_nodes() {
    let root = fixture_root();
    assert!(
        root.is_dir(),
        "missing PHP fixture corpus at {}",
        root.display()
    );

    // Run the CLI over the whole corpus directory in one shot so a single run
    // drains every fixture's unmapped-node tally.
    let assert = Command::cargo_bin("phpastgen")
        .expect("locating phpastgen binary")
        .arg(&root)
        .assert()
        .success();

    let output = assert.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);

    if let Some(summary) = unmapped_summary_line(&stderr) {
        panic!(
            "PHP corpus produced unmapped tree-sitter nodes; the corpus is a coverage \
             gate, so either map these kinds or update the fixtures.\n{summary}\nfull stderr:\n{stderr}"
        );
    }

    // Sanity check: the CLI must have emitted a JSON dump for the corpus.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("==> JSON dump:"),
        "expected a JSON dump in stdout, got:\n{stdout}"
    );
}

/// Locates `fixtures/php-corpus` relative to the crate, matching the layout that
/// `differential_json.rs` consumes.
fn fixture_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if manifest.join("src/main.rs").is_file() {
        return manifest.join("../../fixtures/php-corpus");
    }

    let cwd = env::current_dir().expect("reading current dir");
    for candidate in [cwd.join("crates/phpastgen-cli"), cwd] {
        if candidate.join("src/main.rs").is_file() {
            return candidate.join("../../fixtures/php-corpus");
        }
    }
    manifest.join("../../fixtures/php-corpus")
}

/// Returns the `phpastgen: … unmapped node(s): …` summary line if present.
fn unmapped_summary_line(stderr: &str) -> Option<&str> {
    stderr
        .lines()
        .find(|line| line.starts_with("phpastgen:") && line.contains("unmapped node(s)"))
}

/// Guards the corpus invariant that a meaningful set of feature directories
/// exists, so the coverage gate cannot silently degrade to an empty corpus.
#[test]
fn corpus_has_expected_feature_dirs() {
    let root = fixture_root();
    for feature in ["classes", "traits", "namespaces", "match_expr", "heredoc"] {
        let dir = root.join(feature);
        assert!(
            dir.is_dir(),
            "expected PHP corpus feature dir {}",
            dir.display()
        );
        let has_php = fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("reading {}: {err}", dir.display()))
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("php"))
            });
        assert!(has_php, "no .php files under {}", dir.display());
    }
}
