//! Differential JSON harness comparing this Rust `SwiftAstGen` against a
//! reference SwiftSyntax-based `swift-astgen` implementation.
//!
//! Gated and self-skipping: when `SWIFTASTGEN_REFERENCE` is unset the single
//! test prints a skip notice and returns Ok, so `cargo test` stays green
//! without the reference binary. When set to a reference executable path, the
//! test runs both tools over the fixture corpus, normalizes volatile fields
//! (absolute paths), and asserts structural JSON equality, reporting the first
//! differing file and JSON path on mismatch.
//!
//! Modeled on the gosrc2cpg `differential_json.rs` harness.

use assert_cmd::Command;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tempfile::tempdir;

#[test]
fn rust_json_matches_reference_when_configured() {
    let Some(reference) = configured_reference_binary() else {
        eprintln!("skipping differential JSON test; set SWIFTASTGEN_REFERENCE to a reference swift-astgen binary");
        return;
    };
    assert!(
        reference.is_file(),
        "SWIFTASTGEN_REFERENCE is not a file: {}",
        reference.display()
    );

    let corpus_root = fixture_root();
    let corpus_dirs = configured_corpus_dirs(&corpus_root);
    assert!(
        !corpus_dirs.is_empty(),
        "no fixture corpus directories under {}; create per-feature subdirectories of .swift files",
        corpus_root.display()
    );

    let mut failures = Vec::new();
    for corpus_dir in corpus_dirs {
        let corpus_dir = corpus_dir
            .canonicalize()
            .unwrap_or_else(|_| corpus_dir.to_path_buf());
        let tmp = tempdir().expect("creating temp dir");
        let reference_out = tmp.path().join("reference");
        let rust_out = tmp.path().join("rust");

        if let Err(err) = run_reference(&reference, &corpus_dir, &reference_out) {
            failures.push(format!(
                "{}: reference failed\n{err}",
                corpus_name(&corpus_dir)
            ));
            continue;
        }
        if let Err(err) = run_rust(&corpus_dir, &rust_out) {
            failures.push(format!("{}: rust failed\n{err}", corpus_name(&corpus_dir)));
            continue;
        }

        let reference_json = match read_json_tree(&reference_out, &corpus_dir) {
            Ok(value) => value,
            Err(err) => {
                failures.push(format!(
                    "{}: failed to read reference output\n{err}",
                    corpus_name(&corpus_dir)
                ));
                continue;
            }
        };
        let rust_json = match read_json_tree(&rust_out, &corpus_dir) {
            Ok(value) => value,
            Err(err) => {
                failures.push(format!(
                    "{}: failed to read rust output\n{err}",
                    corpus_name(&corpus_dir)
                ));
                continue;
            }
        };

        if let Some(diff) = format_json_diff(&corpus_name(&corpus_dir), &reference_json, &rust_json)
        {
            failures.push(diff);
        }
    }

    assert!(
        failures.is_empty(),
        "differential JSON mismatches:\n\n{}",
        failures.join("\n\n")
    );
}

fn configured_reference_binary() -> Option<PathBuf> {
    env::var_os("SWIFTASTGEN_REFERENCE").map(PathBuf::from)
}

fn fixture_root() -> PathBuf {
    swiftastgen_cli_dir().join("../../fixtures/swift-corpus")
}

fn swiftastgen_cli_dir() -> PathBuf {
    let compiled = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if compiled.join("src/main.rs").is_file() {
        return compiled;
    }

    let cwd = env::current_dir().expect("reading current dir");
    for candidate in [cwd.join("crates/swiftastgen-cli"), cwd] {
        if candidate.join("src/main.rs").is_file() {
            return candidate;
        }
    }
    panic!("could not locate swiftastgen-cli crate directory")
}

fn configured_corpus_dirs(fixture_root: &Path) -> Vec<PathBuf> {
    let mut dirs = if fixture_root.is_dir() {
        immediate_child_dirs(fixture_root)
    } else {
        Vec::new()
    };
    if let Some(real_corpus) = env::var_os("SWIFTASTGEN_REAL_CORPUS") {
        let mut real_dirs = env::split_paths(&real_corpus)
            .inspect(|path| {
                assert!(
                    path.is_dir(),
                    "SWIFTASTGEN_REAL_CORPUS entry is not a directory: {}",
                    path.display()
                );
            })
            .collect::<Vec<_>>();
        real_dirs.sort();
        dirs.extend(real_dirs);
    }
    dirs
}

fn immediate_child_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = fs::read_dir(root)
        .unwrap_or_else(|err| panic!("reading {}: {err}", root.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs
}

fn run_reference(reference: &Path, input: &Path, out: &Path) -> Result<(), String> {
    // The upstream SwiftSyntax-based reference uses `--src/--output`, NOT the
    // `-o <out> <input>` shape the Scala AstGenRunner was adapted to for this
    // Rust binary. (Discovered by running the real reference; the two CLIs are
    // not drop-in identical.)
    let output = StdCommand::new(reference)
        .arg("--src")
        .arg(input)
        .arg("--output")
        .arg(out)
        .output()
        .map_err(|err| err.to_string())?;
    check_output(output, "reference")
}

fn run_rust(input: &Path, out: &Path) -> Result<(), String> {
    let mut command = Command::cargo_bin("SwiftAstGen").map_err(|err| err.to_string())?;
    let output = command
        .arg("-o")
        .arg(out)
        .arg(input)
        .output()
        .map_err(|err| err.to_string())?;
    check_output(output, "rust")
}

fn check_output(output: std::process::Output, label: &str) -> Result<(), String> {
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{label} command failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn read_json_tree(out: &Path, input_root: &Path) -> Result<BTreeMap<String, Value>, String> {
    let mut files = Vec::new();
    collect_json_files(out, &mut files)?;

    let mut values = BTreeMap::new();
    for file in files {
        let relative = file
            .strip_prefix(out)
            .map_err(|err| err.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = fs::read(&file).map_err(|err| format!("reading {}: {err}", file.display()))?;
        let mut value: Value = serde_json::from_slice(&bytes)
            .map_err(|err| format!("decoding {}: {err}", file.display()))?;
        normalize_value(&mut value, input_root);
        values.insert(relative, value);
    }
    Ok(values)
}

fn collect_json_files(root: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|err| format!("reading {}: {err}", root.display()))? {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, out)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("json") {
            out.push(path);
        }
    }
    out.sort();
    Ok(())
}

/// Replaces machine-specific absolute paths (e.g. the emitted `fullFilePath`)
/// with a stable placeholder so the comparison reflects structure, not the
/// temp directory each tool ran in.
///
/// NOTE: for the strict byte-identity gate we deliberately do NOT strip the
/// SwiftSyntax `name` keypath label here — it must match the reference. The
/// [`strip_name_keys`] helper is retained for an optional CPG-relevant-only
/// comparison mode and exercised by its unit test.
fn normalize_value(value: &mut Value, input_root: &Path) {
    match value {
        Value::String(text) => {
            *text = normalize_string(text, input_root);
        }
        Value::Array(values) => {
            for value in values {
                normalize_value(value, input_root);
            }
        }
        Value::Object(values) => {
            for (_key, value) in values.iter_mut() {
                normalize_value(value, input_root);
            }
        }
        _ => {}
    }
}

/// Recursively removes every `"name"` object key from the JSON tree.
///
/// `name` is the SwiftSyntax child-field/keypath label a node occupies in its
/// parent (e.g. `item`, `decl`, `signature`, `body`, `parameters`,
/// `leftOperand`, `operator`, `importKeyword`, `''` on the root). It is a
/// SwiftSyntax serialization artifact: tree-sitter exposes no equivalent
/// keypath, and the Scala `swiftsrc2cpg` CPG builder never consumes it (the
/// full swift CPG suite passes without it). We therefore treat it as a
/// documented, CPG-irrelevant divergence and strip it from BOTH the reference
/// and rust trees before comparison, mirroring how gosrc2cpg documents its
/// legacy-identity divergences rather than chasing cosmetic parity.
fn strip_name_keys(value: &mut Value) {
    match value {
        Value::Object(values) => {
            values.remove("name");
            for (_key, value) in values.iter_mut() {
                strip_name_keys(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                strip_name_keys(value);
            }
        }
        _ => {}
    }
}

fn normalize_string(text: &str, input_root: &Path) -> String {
    let root = input_root.to_string_lossy();
    let normalized_root = root.replace('\\', "/");
    text.replace(root.as_ref(), "$INPUT")
        .replace(&normalized_root, "$INPUT")
        .replace('\\', "/")
}

/// One byte-level divergence between the reference and rust JSON, tagged with a
/// `kind` (index/name/tokenKind/range/structure/keys/value) and the nearest
/// enclosing `nodeType` so deltas can be ranked and prioritised.
struct DiffEntry {
    path: String,
    kind: String,
    node_type: String,
    detail: String,
}

/// Classify a leaf value-diff by the JSON field it sits on.
fn classify_leaf(path: &str) -> &'static str {
    if path.ends_with(".index") {
        "index"
    } else if path.ends_with(".name") {
        "name"
    } else if path.ends_with(".tokenKind") {
        "tokenKind"
    } else if path.contains(".range") {
        "range"
    } else {
        "value"
    }
}

/// Recursively collect ALL divergences (not just the first), tracking the
/// nearest enclosing `nodeType`/`tokenKind` for ranking.
fn collect_diffs(
    path: &str,
    node_type: &str,
    reference: &Value,
    rust: &Value,
    out: &mut Vec<DiffEntry>,
) {
    match (reference, rust) {
        (Value::Object(reference_obj), Value::Object(rust_obj)) => {
            let nt = reference_obj
                .get("nodeType")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    reference_obj
                        .get("tokenKind")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                })
                .map(str::to_string)
                .unwrap_or_else(|| node_type.to_string());
            let reference_keys = reference_obj.keys().cloned().collect::<BTreeSet<_>>();
            let rust_keys = rust_obj.keys().cloned().collect::<BTreeSet<_>>();
            if reference_keys != rust_keys {
                out.push(DiffEntry {
                    path: path.into(),
                    kind: "keys".into(),
                    node_type: nt.clone(),
                    detail: format!("reference {reference_keys:?} vs rust {rust_keys:?}"),
                });
            }
            for key in reference_keys.intersection(&rust_keys) {
                collect_diffs(
                    &format!("{path}.{key}"),
                    &nt,
                    &reference_obj[key],
                    &rust_obj[key],
                    out,
                );
            }
        }
        (Value::Array(reference_values), Value::Array(rust_values)) => {
            if reference_values.len() != rust_values.len() {
                out.push(DiffEntry {
                    path: path.into(),
                    kind: "structure".into(),
                    node_type: node_type.into(),
                    detail: format!(
                        "array length reference {} vs rust {}",
                        reference_values.len(),
                        rust_values.len()
                    ),
                });
            }
            for (index, (reference_value, rust_value)) in
                reference_values.iter().zip(rust_values.iter()).enumerate()
            {
                collect_diffs(
                    &format!("{path}[{index}]"),
                    node_type,
                    reference_value,
                    rust_value,
                    out,
                );
            }
        }
        _ if reference == rust => {}
        _ => out.push(DiffEntry {
            path: path.into(),
            kind: classify_leaf(path).into(),
            node_type: node_type.into(),
            detail: format!(
                "reference {} vs rust {}",
                short_json(reference),
                short_json(rust)
            ),
        }),
    }
}

/// Compare both JSON trees and, on divergence, return a ranked classification
/// summary (by delta kind and by `kind @ nodeType`) plus sample paths — the
/// work-list for driving the swift emitter to byte-identity.
fn format_json_diff(
    corpus_name: &str,
    reference_json: &BTreeMap<String, Value>,
    rust_json: &BTreeMap<String, Value>,
) -> Option<String> {
    let reference_files = reference_json.keys().cloned().collect::<Vec<_>>();
    let rust_files = rust_json.keys().cloned().collect::<Vec<_>>();
    if reference_files != rust_files {
        return Some(format!(
            "{corpus_name}: file set differs\nreference files: {reference_files:?}\nrust files: {rust_files:?}"
        ));
    }

    let mut diffs = Vec::new();
    for key in &reference_files {
        if let (Some(reference_value), Some(rust_value)) =
            (reference_json.get(key), rust_json.get(key))
        {
            collect_diffs(
                &format!("{key}:$"),
                "",
                reference_value,
                rust_value,
                &mut diffs,
            );
        }
    }
    if diffs.is_empty() {
        return None;
    }

    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_kind_node: BTreeMap<(String, String), usize> = BTreeMap::new();
    for d in &diffs {
        *by_kind.entry(d.kind.clone()).or_default() += 1;
        *by_kind_node
            .entry((d.kind.clone(), d.node_type.clone()))
            .or_default() += 1;
    }
    let kind_lines = by_kind
        .iter()
        .map(|(k, c)| format!("  {k}: {c}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut node_pairs = by_kind_node.into_iter().collect::<Vec<_>>();
    node_pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let top_nodes = node_pairs
        .iter()
        .take(15)
        .map(|((kind, nt), c)| {
            format!(
                "  {kind} @ {}: {c}",
                if nt.is_empty() { "<root>" } else { nt }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let samples = diffs
        .iter()
        .take(12)
        .map(|d| format!("  [{}] {}\n      {}", d.kind, d.path, d.detail))
        .collect::<Vec<_>>()
        .join("\n");

    Some(format!(
        "{corpus_name}: {} JSON delta(s) vs reference\nby kind:\n{kind_lines}\ntop (kind @ nodeType):\n{top_nodes}\nsamples:\n{samples}",
        diffs.len()
    ))
}

fn first_value_diff(path: &str, reference: &Value, rust: &Value) -> Option<String> {
    match (reference, rust) {
        (Value::Object(reference_obj), Value::Object(rust_obj)) => {
            let reference_keys = reference_obj.keys().cloned().collect::<BTreeSet<_>>();
            let rust_keys = rust_obj.keys().cloned().collect::<BTreeSet<_>>();
            if reference_keys != rust_keys {
                return Some(format!(
                    "{path}: object keys differ\nreference: {reference_keys:?}\nrust: {rust_keys:?}"
                ));
            }
            for key in reference_keys {
                if let Some(diff) = first_value_diff(
                    &format!("{path}.{key}"),
                    &reference_obj[&key],
                    &rust_obj[&key],
                ) {
                    return Some(diff);
                }
            }
            None
        }
        (Value::Array(reference_values), Value::Array(rust_values)) => {
            if reference_values.len() != rust_values.len() {
                return Some(format!(
                    "{path}: array length differs\nreference: {}\nrust: {}",
                    reference_values.len(),
                    rust_values.len()
                ));
            }
            for (index, (reference_value, rust_value)) in
                reference_values.iter().zip(rust_values.iter()).enumerate()
            {
                if let Some(diff) =
                    first_value_diff(&format!("{path}[{index}]"), reference_value, rust_value)
                {
                    return Some(diff);
                }
            }
            None
        }
        _ if reference == rust => None,
        _ => Some(format!(
            "{path}: value differs\nreference: {}\nrust: {}",
            short_json(reference),
            short_json(rust)
        )),
    }
}

fn short_json(value: &Value) -> String {
    let text = serde_json::to_string(value).unwrap_or_else(|_| "<unprintable>".into());
    if text.len() > 500 {
        format!("{}...", &text[..500])
    } else {
        text
    }
}

fn corpus_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("<unknown>"))
        .to_string_lossy()
        .into_owned()
}

#[test]
fn json_diff_detects_value_and_structure_mismatches() {
    let reference = serde_json::json!({
        "nodeType": "SourceFileSyntax",
        "children": [{"nodeType": "CodeBlockItemListSyntax", "loc": 1}],
    });
    let same = reference.clone();
    assert_eq!(first_value_diff("$", &reference, &same), None);

    let differing_value = serde_json::json!({
        "nodeType": "SourceFileSyntax",
        "children": [{"nodeType": "CodeBlockItemListSyntax", "loc": 2}],
    });
    assert!(first_value_diff("$", &reference, &differing_value).is_some());

    let differing_keys = serde_json::json!({
        "nodeType": "SourceFileSyntax",
    });
    assert!(first_value_diff("$", &reference, &differing_keys).is_some());
}

#[test]
fn strip_name_keys_removes_swiftsyntax_keypath_labels() {
    // Reference emits a `name` keypath label on essentially every node; the
    // rust tree omits it. After stripping, the two trees compare equal.
    let mut reference = serde_json::json!({
        "name": "",
        "nodeType": "SourceFileSyntax",
        "children": [
            {
                "name": "item",
                "nodeType": "CodeBlockItemSyntax",
                "children": [
                    {"name": "decl", "nodeType": "ImportDeclSyntax", "children": []}
                ]
            }
        ]
    });
    let mut rust = serde_json::json!({
        "nodeType": "SourceFileSyntax",
        "children": [
            {
                "nodeType": "CodeBlockItemSyntax",
                "children": [
                    {"nodeType": "ImportDeclSyntax", "children": []}
                ]
            }
        ]
    });

    strip_name_keys(&mut reference);
    strip_name_keys(&mut rust);

    // No `name` key survives anywhere in the reference tree.
    assert!(reference.get("name").is_none());
    assert!(reference["children"][0].get("name").is_none());
    assert!(reference["children"][0]["children"][0]
        .get("name")
        .is_none());
    // Stripping the reference's labels makes it structurally equal to the rust
    // tree, and re-stripping the already-clean rust tree is a no-op.
    assert_eq!(reference, rust);
    assert_eq!(first_value_diff("$", &reference, &rust), None);
}
