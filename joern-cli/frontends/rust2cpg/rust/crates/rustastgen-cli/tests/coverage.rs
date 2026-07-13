//! Coverage gate for the `rust_ast_gen` CLI.
//!
//! `rustastgen` is a generic `ra_ap_syntax` tree-walker: it emits every syntax
//! node kind via `format!("{kind:?}")`, so there is no "unmapped"/Unknown
//! fallback to count. The gate therefore asserts three things over a broad
//! inline fixture:
//!
//! 1. the CLI exits successfully and produces a JSON file;
//! 2. a representative set of `nodeKind`s appears in the tree; and
//! 3. semantic enrichment actually ran -- at least one node carries a
//!    `typeFullName` and at least one carries a `methodFullName`.

use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::process::Command;
use tempfile::tempdir;

/// Broad Rust fixture exercising the constructs the rust2cpg frontend cares
/// about: a struct + impl with methods, an enum, a trait, a generic fn, `let`
/// bindings (including shadowing), `match`, closures, `if/else`,
/// `for`/`while`/`loop`, method chains on `String`/`Vec`, `vec![]`, tuples, and
/// `?` error handling.
const FIXTURE: &str = r#"use std::collections::HashMap;

#[derive(Debug)]
struct Point {
    x: i32,
    y: i32,
}

impl Point {
    fn new(x: i32, y: i32) -> Self {
        Point { x, y }
    }

    fn magnitude(&self) -> i32 {
        self.x * self.x + self.y * self.y
    }
}

enum Shape {
    Circle(i32),
    Rect { w: i32, h: i32 },
    Empty,
}

trait Area {
    fn area(&self) -> i32;
}

impl Area for Shape {
    fn area(&self) -> i32 {
        match self {
            Shape::Circle(r) => 3 * r * r,
            Shape::Rect { w, h } => w * h,
            Shape::Empty => 0,
        }
    }
}

fn largest<T: PartialOrd + Copy>(items: &[T]) -> T {
    let mut best = items[0];
    for &item in items {
        if item > best {
            best = item;
        }
    }
    best
}

fn parse_one(text: &str) -> Result<i32, std::num::ParseIntError> {
    let value = text.trim().parse::<i32>()?;
    Ok(value)
}

fn run() -> Result<(), std::num::ParseIntError> {
    let p = Point::new(3, 4);
    let m = p.magnitude();

    // Shadowing: `m` is rebound from `i32` to `String`.
    let m = m.to_string();
    let mut greeting = String::new();
    greeting.push_str("len=");
    greeting.push_str(m.as_str());
    let _trimmed = greeting.trim().to_string();

    let mut numbers = vec![1, 2, 3];
    numbers.push(4);
    let _count = numbers.len();

    let pair = (p, greeting);
    let scores: HashMap<&str, i32> = HashMap::new();
    let _empty = scores.len();

    let double = |n: i32| n * 2;
    let _doubled = double(21);

    let shapes = [Shape::Circle(2), Shape::Empty];
    let _max_area = largest(&[shapes[0].area(), shapes[1].area()]);

    if pair.1.is_empty() {
        let _branch = 0;
    } else {
        let _branch = 1;
    }

    let mut i = 0;
    while i < 3 {
        i += 1;
    }

    let mut total = 0;
    loop {
        total += 1;
        if total > 5 {
            break;
        }
    }

    let _parsed = parse_one("42")?;
    Ok(())
}

fn main() {
    let _ = run();
}
"#;

/// `nodeKind`s every run of the fixture must contain. These are the
/// `SyntaxKind` Debug names emitted by the tree-walker.
const EXPECTED_NODE_KINDS: &[&str] = &[
    "SOURCE_FILE",
    "STRUCT",
    "IMPL",
    "FN",
    "ENUM",
    "TRAIT",
    "MATCH_EXPR",
    "MATCH_ARM",
    "LET_STMT",
    "METHOD_CALL_EXPR",
    "CALL_EXPR",
    "CLOSURE_EXPR",
    "IF_EXPR",
    "FOR_EXPR",
    "WHILE_EXPR",
    "LOOP_EXPR",
    "TUPLE_EXPR",
    "MACRO_EXPR",
    "TRY_EXPR",
    "GENERIC_PARAM_LIST",
];

#[test]
fn cli_emits_broad_coverage_with_semantic_enrichment() {
    let tmp = tempdir().expect("creating temp dir");
    let input = tmp.path().join("fixture.rs");
    let out = tmp.path().join("out");
    fs::write(&input, FIXTURE).expect("writing fixture");

    // Default invocation keeps the sysroot enrichment enabled.
    let output = Command::new(env!("CARGO_BIN_EXE_rust_ast_gen"))
        .arg("-i")
        .arg(&input)
        .arg("-o")
        .arg(&out)
        .output()
        .expect("running rust_ast_gen");

    assert!(
        output.status.success(),
        "rust_ast_gen exited unsuccessfully\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let json_path = out.join("fixture.rs.json");
    assert!(
        json_path.is_file(),
        "expected JSON output at {} (stdout:\n{})",
        json_path.display(),
        String::from_utf8_lossy(&output.stdout),
    );

    let document: Value =
        serde_json::from_slice(&fs::read(&json_path).expect("reading JSON output"))
            .expect("decoding JSON output");

    // The envelope wraps the parsed tree under `children`.
    let source_file = &document["children"][0];
    assert_eq!(
        source_file["nodeKind"], "SOURCE_FILE",
        "top-level child should be the SOURCE_FILE node"
    );

    // (a) Expected node kinds are present.
    let mut node_kinds = BTreeSet::new();
    collect_node_kinds(&document, &mut node_kinds);
    let missing = EXPECTED_NODE_KINDS
        .iter()
        .filter(|kind| !node_kinds.contains(**kind))
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "fixture did not emit expected node kinds: {missing:?}\nemitted: {node_kinds:?}"
    );

    // (b) Semantic enrichment ran: at least one node carries each annotation.
    let type_full_names = collect_string_field(&document, "typeFullName");
    let method_full_names = collect_string_field(&document, "methodFullName");
    assert!(
        !type_full_names.is_empty(),
        "no node carried a typeFullName; semantic enrichment did not run"
    );
    assert!(
        !method_full_names.is_empty(),
        "no node carried a methodFullName; semantic enrichment did not run"
    );

    // The fixture mixes user-defined and sysroot-backed callables, so both
    // crate-qualified and standard-library method names should surface.
    assert!(
        method_full_names
            .iter()
            .any(|name| name.contains("Point::") || name.contains("::magnitude")),
        "expected a user-defined methodFullName (e.g. Point::magnitude); got {method_full_names:?}"
    );
    assert!(
        method_full_names
            .iter()
            .any(|name| name.starts_with("alloc::") || name.starts_with("str::")),
        "expected a sysroot methodFullName (e.g. alloc::string::String::push_str); got {method_full_names:?}"
    );
}

fn collect_node_kinds(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(obj) => {
            if let Some(kind) = obj.get("nodeKind").and_then(Value::as_str) {
                out.insert(kind.to_string());
            }
            for child in obj.values() {
                collect_node_kinds(child, out);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_node_kinds(child, out);
            }
        }
        _ => {}
    }
}

fn collect_string_field(value: &Value, field: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    collect_string_field_into(value, field, &mut out);
    out
}

fn collect_string_field_into(value: &Value, field: &str, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(obj) => {
            if let Some(found) = obj.get(field).and_then(Value::as_str) {
                out.insert(found.to_string());
            }
            for child in obj.values() {
                collect_string_field_into(child, field, out);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_string_field_into(child, field, out);
            }
        }
        _ => {}
    }
}
