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
    let output = StdCommand::new(reference)
        .arg("-o")
        .arg(out)
        .arg(input)
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

fn normalize_string(text: &str, input_root: &Path) -> String {
    let root = input_root.to_string_lossy();
    let normalized_root = root.replace('\\', "/");
    text.replace(root.as_ref(), "$INPUT")
        .replace(&normalized_root, "$INPUT")
        .replace('\\', "/")
}

fn format_json_diff(
    corpus_name: &str,
    reference_json: &BTreeMap<String, Value>,
    rust_json: &BTreeMap<String, Value>,
) -> Option<String> {
    let reference_files = reference_json.keys().cloned().collect::<Vec<_>>();
    let rust_files = rust_json.keys().cloned().collect::<Vec<_>>();
    let mut message = format!("{corpus_name}: JSON output differs");
    if reference_files != rust_files {
        message.push_str(&format!(
            "\nreference files: {reference_files:?}\nrust files: {rust_files:?}"
        ));
        return Some(message);
    }

    for key in reference_files {
        match (reference_json.get(&key), rust_json.get(&key)) {
            (Some(reference_value), Some(rust_value)) if reference_value != rust_value => {
                if let Some(value_diff) = first_value_diff("$", reference_value, rust_value) {
                    message.push_str(&format!("\nfirst differing file: {key}\n{value_diff}"));
                    return Some(message);
                }
            }
            _ => {}
        }
    }
    None
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
