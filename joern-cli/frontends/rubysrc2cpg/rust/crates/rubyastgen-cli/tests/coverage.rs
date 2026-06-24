//! Coverage gate for the `rubyastgen` CLI.
//!
//! Runs the real binary over the on-disk Ruby fixture corpus
//! (`fixtures/ruby-corpus/<feature>/`, the same corpus the differential parity
//! harness consumes). The corpus exercises the common constructs plus the
//! recently-mapped ones (classes/modules, blocks, pattern matching, flip-flops,
//! `BEGIN`/`END`, `redo`/`retry`, rationals, `undef`, interpolation, heredocs,
//! regex options, …). It is held at *zero* `__unknown` fallbacks: the CLI prints
//! a `rubyastgen: N unmapped node(s): …` summary to stderr whenever a
//! `lib-ruby-parser` node falls through to the `__unknown` catch-all, and this
//! test fails (listing the offending variants) if that summary ever appears.
//!
//! Constructs that map to an existing node kind rather than a dedicated type
//! string (and so do not surface their own `"type"` literal) are still covered
//! because they parse and lower cleanly without an `__unknown` fallback:
//!   * heredocs (`<<~TEXT`) lower to `str`/`dstr` (see `lower_heredoc`);
//!   * regex options (`/…/ix`) are folded into the `regexp` node, so the
//!     `regopt` child is not re-emitted as a standalone node.
//!
//! No construct is intentionally excluded from the gate: every node the corpus
//! produces is mapped, keeping the unmapped tally at zero.

use assert_cmd::Command;
use std::env;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn corpus_emits_no_unmapped_nodes() {
    let corpus = fixture_root();
    assert!(
        corpus.is_dir(),
        "missing Ruby fixture corpus at {}",
        corpus.display()
    );

    let out = tempdir().expect("creating temp dir");

    // Run the CLI over the whole corpus directory in one shot so a single run
    // drains every fixture's unmapped-node tally.
    let assert = Command::cargo_bin("rubyastgen")
        .expect("locating rubyastgen binary")
        .arg("-o")
        .arg(out.path())
        .arg(&corpus)
        .assert()
        .success();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    let unmapped: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("rubyastgen:") && line.contains("unmapped node(s)"))
        .collect();
    assert!(
        unmapped.is_empty(),
        "CLI reported unmapped nodes for the corpus; either map the construct or \
         exclude it from the fixtures:\n{}",
        unmapped.join("\n")
    );

    // Sanity: at least one JSON document was emitted and none contains the
    // `__unknown` fallback node, independent of the stderr summary.
    let mut documents = 0usize;
    let mut stack = vec![out.path().to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("reading {}: {err}", dir.display()))
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                let json = fs::read_to_string(&path)
                    .unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
                assert!(
                    !json.contains("__unknown"),
                    "emitted JSON {} contains an __unknown fallback node",
                    path.display()
                );
                documents += 1;
            }
        }
    }
    assert!(
        documents >= 5,
        "expected the corpus to emit a document per feature fixture, got {documents}"
    );
}

/// Locates `fixtures/ruby-corpus` relative to the crate, matching the layout that
/// `differential_json.rs` consumes.
fn fixture_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if manifest.join("src/main.rs").is_file() {
        return manifest.join("../../fixtures/ruby-corpus");
    }

    let cwd = env::current_dir().expect("reading current dir");
    for candidate in [cwd.join("crates/rubyastgen-cli"), cwd] {
        if candidate.join("src/main.rs").is_file() {
            return candidate.join("../../fixtures/ruby-corpus");
        }
    }
    manifest.join("../../fixtures/ruby-corpus")
}
