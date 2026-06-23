use serde_json::Value;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Differential test against a reference PHP-Parser binary.
///
/// This mirrors the gosrc2cpg `differential_json` harness but adapts to the
/// phpastgen CLI, which emits each file's JSON tree to stdout (prefixed by a
/// `==> JSON dump:` header) rather than to an `-out` directory. The test
/// self-skips unless `PHPASTGEN_REFERENCE` points at a reference binary that
/// accepts a single PHP file argument and prints the same dump format, so it is
/// safe to run in CI without a reference present.
#[test]
fn rust_json_matches_reference_when_configured() {
    let Some(reference) = configured_reference_binary() else {
        eprintln!("skipping differential JSON test; set PHPASTGEN_REFERENCE to a reference binary");
        return;
    };
    assert!(
        reference.is_file(),
        "PHPASTGEN_REFERENCE is not a file: {}",
        reference.display()
    );

    let fixtures = configured_fixtures();
    assert!(
        !fixtures.is_empty(),
        "no PHP fixtures found; set PHPASTGEN_FIXTURES or add files under {}",
        fixture_root().display()
    );

    let mut failures = Vec::new();
    for fixture in fixtures {
        let fixture = fixture.canonicalize().unwrap_or(fixture);

        let reference_json = match run_and_read(Command::new(&reference), &fixture, &fixture) {
            Ok(value) => value,
            Err(err) => {
                failures.push(format!("{}: reference failed\n{err}", display(&fixture)));
                continue;
            }
        };
        let rust_json = match run_and_read(rust_command(), &fixture, &fixture) {
            Ok(value) => value,
            Err(err) => {
                failures.push(format!("{}: rust failed\n{err}", display(&fixture)));
                continue;
            }
        };

        if let Some(diff) = first_value_diff("$", &reference_json, &rust_json) {
            failures.push(format!("{}: JSON differs\n{diff}", display(&fixture)));
        }
    }

    assert!(
        failures.is_empty(),
        "differential JSON mismatches:\n\n{}",
        failures.join("\n\n")
    );
}

fn configured_reference_binary() -> Option<PathBuf> {
    env::var_os("PHPASTGEN_REFERENCE").map(PathBuf::from)
}

/// The freshly built phpastgen binary. Cargo exposes its path to integration
/// tests via `CARGO_BIN_EXE_<bin-name>`.
fn rust_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_phpastgen"))
}

/// Fixture files to compare. Defaults to `*.php` under the fixture root, with an
/// optional `PHPASTGEN_FIXTURES` path-list override (matching Go's corpus env).
fn configured_fixtures() -> Vec<PathBuf> {
    if let Some(paths) = env::var_os("PHPASTGEN_FIXTURES") {
        let mut files = Vec::new();
        for path in env::split_paths(&paths) {
            if path.is_dir() {
                collect_php_files(&path, &mut files);
            } else if path.is_file() {
                files.push(path);
            } else {
                panic!(
                    "PHPASTGEN_FIXTURES entry does not exist: {}",
                    path.display()
                );
            }
        }
        files.sort();
        files.dedup();
        return files;
    }

    let mut files = Vec::new();
    collect_php_files(&fixture_root(), &mut files);
    files
}

fn fixture_root() -> PathBuf {
    phpastgen_cli_dir().join("../../fixtures/php-corpus")
}

fn phpastgen_cli_dir() -> PathBuf {
    let compiled = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if compiled.join("src/main.rs").is_file() {
        return compiled;
    }

    let cwd = env::current_dir().expect("reading current dir");
    for candidate in [cwd.join("crates/phpastgen-cli"), cwd] {
        if candidate.join("src/main.rs").is_file() {
            return candidate;
        }
    }
    panic!("could not locate phpastgen-cli crate directory")
}

fn collect_php_files(root: &Path, out: &mut Vec<PathBuf>) {
    if !root.exists() {
        return;
    }
    let entries =
        fs::read_dir(root).unwrap_or_else(|err| panic!("reading {}: {err}", root.display()));
    for entry in entries {
        let path = entry.expect("reading directory entry").path();
        if path.is_dir() {
            collect_php_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("php") {
            out.push(path);
        }
    }
    out.sort();
}

/// Runs `command` over `fixture`, returning the normalized JSON tree parsed from
/// the CLI's `==> JSON dump:` stdout section.
fn run_and_read(mut command: Command, fixture: &Path, input_root: &Path) -> Result<Value, String> {
    let output = command
        .arg(fixture)
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "command failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut value = parse_json_dump(&stdout)?;
    normalize_value(&mut value, input_root);
    Ok(value)
}

/// Extracts and parses the JSON array following the `==> JSON dump:` marker.
fn parse_json_dump(stdout: &str) -> Result<Value, String> {
    const MARKER: &str = "==> JSON dump:";
    let body = stdout
        .split_once(MARKER)
        .map(|(_, rest)| rest.trim_start())
        .ok_or_else(|| format!("missing `{MARKER}` marker in stdout:\n{stdout}"))?;
    serde_json::from_str(body).map_err(|err| format!("decoding JSON dump: {err}\nbody:\n{body}"))
}

/// Normalizes volatile fields: absolute paths are replaced with `$INPUT` and
/// byte-offset attributes (which differ between parser implementations) are
/// elided so the comparison focuses on structural node shape.
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
                if key == "startFilePos" || key == "endFilePos" {
                    *value = Value::String("$POS".into());
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

fn display(path: &Path) -> String {
    path.file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("<unknown>"))
        .to_string_lossy()
        .into_owned()
}

/// Confirms the byte-offset normalization (the one volatile field that differs
/// between parser implementations) collapses positions while preserving shape.
#[test]
fn normalization_elides_byte_offsets() {
    let mut reference = serde_json::json!({
        "nodeType": "Scalar_LNumber",
        "value": 1,
        "attributes": {"startFilePos": 6, "endFilePos": 6, "startLine": 1, "endLine": 1}
    });
    let mut rust = serde_json::json!({
        "nodeType": "Scalar_LNumber",
        "value": 1,
        "attributes": {"startFilePos": 99, "endFilePos": 120, "startLine": 1, "endLine": 1}
    });
    let root = Path::new("/tmp/example");
    normalize_value(&mut reference, root);
    normalize_value(&mut rust, root);

    assert_eq!(first_value_diff("$", &reference, &rust), None);

    // A genuine structural difference (line numbers) is still reported.
    let mut other = serde_json::json!({
        "nodeType": "Scalar_LNumber",
        "value": 1,
        "attributes": {"startFilePos": 1, "endFilePos": 2, "startLine": 9, "endLine": 9}
    });
    normalize_value(&mut other, root);
    assert!(first_value_diff("$", &reference, &other).is_some());
}
