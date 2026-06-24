//! Parity / coverage gate for the oxidized `pyastgen` crate.
//!
//! Unlike the counter-based frontends, `pyastgen-core` maps the `rustpython-parser`
//! AST with an *exhaustive* match: there is no silent fallback and no "unmapped"
//! node kind. The coverage gate runs the real CLI over the on-disk fixture corpus
//! (`fixtures/py-corpus/<feature>/`, the same corpus the differential parity
//! harness consumes), then asserts that every fixture parses cleanly, that the
//! emitted JSON trees contain no error/unknown marker, and that — across the
//! corpus — every broad Python-3 construct the mapper claims to support is
//! actually represented.

use assert_cmd::Command;
use serde_json::Value;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

/// Markers that, if they ever appeared as a node `kind`, would mean the mapper
/// fell through to an error/unknown placeholder. The exhaustive match means none
/// of these should ever be emitted; the gate fails loudly if one is.
const ERROR_KIND_MARKERS: &[&str] = &[
    "Unknown",
    "Unmapped",
    "Unsupported",
    "Error",
    "Invalid",
    "NotHandled",
    "Placeholder",
];

#[test]
fn corpus_parses_without_error_kinds_and_covers_constructs() {
    let corpus = fixture_root();
    assert!(
        corpus.is_dir(),
        "missing Python fixture corpus at {}",
        corpus.display()
    );

    let out = tempdir().unwrap();
    Command::cargo_bin("pyastgen")
        .unwrap()
        .arg("-out")
        .arg(out.path())
        .arg(&corpus)
        .assert()
        .success();

    // Collect every node kind that appears anywhere in any emitted document, and
    // sanity-check each document's envelope while doing so.
    let mut kinds = BTreeSet::new();
    let mut documents = 0usize;
    for json_file in json_files(out.path()) {
        let document: Value = serde_json::from_slice(&fs::read(&json_file).unwrap())
            .unwrap_or_else(|err| panic!("{} is not valid JSON: {err}", json_file.display()));

        assert_eq!(
            document["backend"],
            "oxidized-pyastgen",
            "unexpected backend in {}",
            json_file.display()
        );
        assert_eq!(
            document["root"]["kind"],
            "Module",
            "unexpected root kind in {}",
            json_file.display()
        );
        collect_kinds(&document["root"], &mut kinds);
        documents += 1;
    }

    assert!(
        documents >= 6,
        "expected the corpus to emit a document per feature fixture, got {documents}"
    );
    assert!(
        !kinds.is_empty(),
        "emitted trees contained no node kinds at all"
    );

    // The exhaustive mapper must never emit an error/unknown marker.
    let offending = kinds
        .iter()
        .filter(|kind| {
            ERROR_KIND_MARKERS
                .iter()
                .any(|marker| kind.contains(marker))
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    assert!(
        offending.is_empty(),
        "emitted JSON contained error/unknown node kinds: {offending:?}\n\
         pyastgen-core maps the parser AST exhaustively, so this means a fallback was introduced."
    );

    // Meaningful coverage gate: each broad construct must actually be represented
    // somewhere in the corpus. These kinds collectively cover functions+decorators,
    // classes, async/await, comprehensions, f-strings, walrus, match/case,
    // try/except/finally, with, PEP 695 type params, lambda, generators,
    // star/double-star args and type hints.
    const REQUIRED_KINDS: &[&str] = &[
        // functions, decorators, classes, type hints
        "FunctionDef",
        "AsyncFunctionDef",
        "ClassDef",
        "Arguments",
        "Arg",
        "ArgWithDefault",
        "AnnAssign",
        // async / await
        "Await",
        "AsyncFor",
        "AsyncWith",
        "With",
        "WithItem",
        // generators
        "Yield",
        "YieldFrom",
        // comprehensions
        "ListComp",
        "SetComp",
        "DictComp",
        "GeneratorExp",
        "Comprehension",
        // f-strings
        "JoinedStr",
        "FormattedValue",
        // walrus
        "NamedExpr",
        // structural pattern matching
        "Match",
        "MatchCase",
        "MatchSequence",
        "MatchStar",
        "MatchMapping",
        "MatchClass",
        "MatchOr",
        "MatchValue",
        "MatchAs",
        // try / except / finally + raise-from
        "Try",
        "ExceptHandler",
        "Raise",
        // lambda
        "Lambda",
        // star / double-star call args
        "Starred",
        "Keyword",
        // PEP 695 type params
        "TypeVar",
        // imports + type aliases on the typing surface
        "Import",
        "ImportFrom",
        "Alias",
    ];

    let missing = REQUIRED_KINDS
        .iter()
        .filter(|kind| !kinds.contains(**kind))
        .copied()
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "Python corpus did not produce expected node kinds: {missing:?}\n\
         emitted kinds were: {kinds:?}"
    );
}

/// Locates `fixtures/py-corpus` relative to the crate, matching the layout that
/// `differential_json.rs` consumes.
fn fixture_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if manifest.join("src/main.rs").is_file() {
        return manifest.join("../../fixtures/py-corpus");
    }

    let cwd = env::current_dir().expect("reading current dir");
    for candidate in [cwd.join("crates/pyastgen-cli"), cwd] {
        if candidate.join("src/main.rs").is_file() {
            return candidate.join("../../fixtures/py-corpus");
        }
    }
    manifest.join("../../fixtures/py-corpus")
}

/// Recursively collects `*.json` output files under `root`.
fn json_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// Recursively walk `{kind, children}` nodes, recording every `kind`.
fn collect_kinds(node: &Value, out: &mut BTreeSet<String>) {
    match node {
        Value::Object(obj) => {
            if let Some(kind) = obj.get("kind").and_then(Value::as_str) {
                out.insert(kind.to_string());
            }
            for value in obj.values() {
                collect_kinds(value, out);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_kinds(value, out);
            }
        }
        _ => {}
    }
}
