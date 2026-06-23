//! Differential JSON comparison against a reference `pyastgen` implementation.
//!
//! This test is gated: it only runs when `PYASTGEN_REFERENCE` points at a
//! reference binary. When unset it self-skips so the default `cargo test` run
//! stays green. When set, it runs both the reference and the Rust CLI over a
//! fixture directory, normalizes volatile fields (absolute paths, version), and
//! asserts JSON equality with a readable first-diff. Modeled on the gosrc2cpg
//! `differential_json.rs` harness.

use assert_cmd::Command;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tempfile::tempdir;

/// Inline corpus written to a temp dir for the differential run. Mirrors the
/// breadth of the coverage gate so the comparison is meaningful.
const DIFFERENTIAL_FIXTURES: &[(&str, &str)] = &[
    (
        "functions.py",
        "@decorator\ndef f(a, b=1, *args, c, **kwargs) -> int:\n    return a + b\n\n\nasync def g(x):\n    return await h(x)\n",
    ),
    (
        "classes.py",
        "class C[T]:\n    field: int = 0\n\n    def method(self, value: T) -> T:\n        return value\n",
    ),
    (
        "patterns.py",
        "def m(cmd):\n    match cmd:\n        case [a, *rest]:\n            return a, rest\n        case {\"k\": v}:\n            return v\n        case _:\n            return None\n",
    ),
    (
        "expressions.py",
        "def e(xs):\n    total = [x for x in xs if x > 0]\n    if (n := len(total)) > 0:\n        return f\"{n} items\"\n    return None\n",
    ),
];

#[test]
fn rust_json_matches_reference_when_configured() {
    let Some(reference) = configured_reference_binary() else {
        eprintln!("skipping differential JSON test; set PYASTGEN_REFERENCE to a reference binary");
        return;
    };
    assert!(
        reference.is_file(),
        "PYASTGEN_REFERENCE is not a file: {}",
        reference.display()
    );

    let corpus = tempdir().expect("creating corpus dir");
    for (name, source) in DIFFERENTIAL_FIXTURES {
        fs::write(corpus.path().join(name), source).expect("writing fixture");
    }
    let corpus_dir = corpus
        .path()
        .canonicalize()
        .unwrap_or_else(|_| corpus.path().to_path_buf());

    let tmp = tempdir().expect("creating temp dir");
    let reference_out = tmp.path().join("reference");
    let rust_out = tmp.path().join("rust");

    run_reference(&reference, &corpus_dir, &reference_out).expect("reference run failed");
    run_rust(&corpus_dir, &rust_out).expect("rust run failed");

    let reference_json = read_json_tree(&reference_out, &corpus_dir).expect("reading reference");
    let rust_json = read_json_tree(&rust_out, &corpus_dir).expect("reading rust");

    if let Some(diff) = format_json_diff(&reference_json, &rust_json) {
        panic!("differential JSON mismatch:\n\n{diff}");
    }
}

fn configured_reference_binary() -> Option<PathBuf> {
    env::var_os("PYASTGEN_REFERENCE").map(PathBuf::from)
}

fn run_reference(reference: &Path, input: &Path, out: &Path) -> Result<(), String> {
    let output = StdCommand::new(reference)
        .arg("-out")
        .arg(out)
        .arg(input)
        .output()
        .map_err(|err| err.to_string())?;
    check_output(output, "reference")
}

fn run_rust(input: &Path, out: &Path) -> Result<(), String> {
    let output = Command::cargo_bin("pyastgen")
        .map_err(|err| err.to_string())?
        .arg("-out")
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

/// Strip volatile fields so two correct implementations compare equal:
/// - `path` carries the absolute input path,
/// - `version` carries the crate version (differs between builds).
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
                if key == "version" {
                    *value = Value::String("$VERSION".into());
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
        .replace('\\', "/")
}

fn format_json_diff(
    reference_json: &BTreeMap<String, Value>,
    rust_json: &BTreeMap<String, Value>,
) -> Option<String> {
    let reference_files = reference_json.keys().cloned().collect::<Vec<_>>();
    let rust_files = rust_json.keys().cloned().collect::<Vec<_>>();
    if reference_files != rust_files {
        return Some(format!(
            "JSON output files differ\nreference files: {reference_files:?}\nrust files: {rust_files:?}"
        ));
    }

    for key in reference_files {
        match (reference_json.get(&key), rust_json.get(&key)) {
            (Some(reference_value), Some(rust_value)) if reference_value != rust_value => {
                if let Some(value_diff) = first_value_diff("$", reference_value, rust_value) {
                    return Some(format!("first differing file: {key}\n{value_diff}"));
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
