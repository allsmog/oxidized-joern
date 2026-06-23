//! Differential JSON harness for the `rust_ast_gen` CLI.
//!
//! This test is gated and self-skipping. It only runs when `RUSTASTGEN_REFERENCE`
//! points at a reference `rust_ast_gen` binary (the rust-analyzer-HIR-based
//! implementation this crate is replacing). When that env var is unset the test
//! prints a skip notice and returns, so the default `cargo test` run is green
//! without any external artifact.
//!
//! When configured it runs both the reference binary and this crate's CLI over
//! the fixture corpus, reads every emitted JSON document, normalizes
//! environment-specific fields (absolute paths in `fullFilePath`, etc.), and
//! asserts the two trees are equal -- reporting the first divergence in a
//! readable form.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn rust_json_matches_reference_when_configured() {
    let Some(reference) = configured_reference_binary() else {
        eprintln!(
            "skipping differential JSON test; set RUSTASTGEN_REFERENCE to a reference rust_ast_gen binary"
        );
        return;
    };
    assert!(
        reference.is_file(),
        "RUSTASTGEN_REFERENCE is not a file: {}",
        reference.display()
    );

    let corpus_root = fixture_root();
    let corpus_dirs = immediate_child_dirs(&corpus_root);
    assert!(
        !corpus_dirs.is_empty(),
        "no fixture corpus directories under {}",
        corpus_root.display()
    );

    let mut failures = Vec::new();
    for corpus_dir in corpus_dirs {
        let corpus_dir = corpus_dir
            .canonicalize()
            .unwrap_or_else(|_| corpus_dir.to_path_buf());
        let tmp = tempfile::tempdir().expect("creating temp dir");
        let reference_out = tmp.path().join("reference");
        let rust_out = tmp.path().join("rust");

        if let Err(err) = run_cli(&reference, &corpus_dir, &reference_out) {
            failures.push(format!(
                "{}: reference failed\n{err}",
                corpus_name(&corpus_dir)
            ));
            continue;
        }
        if let Err(err) = run_cli(rust_cli(), &corpus_dir, &rust_out) {
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
    env::var_os("RUSTASTGEN_REFERENCE").map(PathBuf::from)
}

fn rust_cli() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_rust_ast_gen"))
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rust-corpus")
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

fn run_cli(binary: &Path, input: &Path, out: &Path) -> Result<(), String> {
    let output = Command::new(binary)
        .arg("-i")
        .arg(input)
        .arg("-o")
        .arg(out)
        .output()
        .map_err(|err| format!("spawning {}: {err}", binary.display()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} failed\nstdout:\n{}\nstderr:\n{}",
            binary.display(),
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

/// Strip environment-specific data so two runs from different working trees
/// compare equal. The envelope carries an absolute `fullFilePath`; both that
/// field and any other string mentioning the input root are collapsed to a
/// stable placeholder. `relativeFilePath` is already root-relative.
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
            for (key, value) in values.iter_mut() {
                if key == "fullFilePath" {
                    *value = Value::String("$INPUT".into());
                } else {
                    normalize_value(value, input_root);
                }
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
fn normalizes_full_file_path_and_input_root() {
    let mut value = serde_json::json!({
        "relativeFilePath": "src/lib.rs",
        "fullFilePath": "/work/corpus/basics/src/lib.rs",
        "children": [{
            "nodeKind": "SOURCE_FILE",
            "text": "/work/corpus/basics/src/lib.rs"
        }]
    });
    normalize_value(&mut value, Path::new("/work/corpus/basics"));
    assert_eq!(value["fullFilePath"], "$INPUT");
    assert_eq!(value["relativeFilePath"], "src/lib.rs");
    assert_eq!(value["children"][0]["text"], "$INPUT/src/lib.rs");
}

#[test]
fn first_value_diff_reports_first_divergence() {
    let reference = serde_json::json!({
        "nodeKind": "FN",
        "children": [{"nodeKind": "NAME", "text": "a"}]
    });
    let rust = serde_json::json!({
        "nodeKind": "FN",
        "children": [{"nodeKind": "NAME", "text": "b"}]
    });
    let diff = first_value_diff("$", &reference, &rust).expect("expected a diff");
    assert!(diff.contains("children[0].text"), "diff was: {diff}");
    assert_eq!(first_value_diff("$", &reference, &reference), None);
}
