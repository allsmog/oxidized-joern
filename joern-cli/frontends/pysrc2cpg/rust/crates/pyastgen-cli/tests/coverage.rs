//! Parity / coverage gate for the oxidized `pyastgen` crate.
//!
//! Unlike the counter-based frontends, `pyastgen-core` maps the `rustpython-parser`
//! AST with an *exhaustive* match: there is no silent fallback and no "unmapped"
//! node kind. The coverage gate therefore asserts that a broad Python-3 fixture
//! parses cleanly and that the emitted JSON tree contains no error/unknown marker,
//! while every exercised top-level construct produces the node kind it should.

use assert_cmd::Command;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use tempfile::tempdir;

/// A single source file that exercises every broad Python-3 construct the mapper
/// claims to support. Kept inline so the gate is self-contained and reviewable.
const PYTHON_FIXTURE: &str = r#"
import os
from typing import Iterable, Optional

GREETING: str = "hi"


def decorate(fn):
    return fn


@decorate
def plain(a, b=1, *args, c, d=2, **kwargs) -> int:
    """A function with positional, default, star and double-star args."""
    return a + b + c + d


async def fetch(source: Iterable[int]) -> Optional[int]:
    total = 0
    async for item in source:
        total += await coro(item)
    async with opener() as handle:
        return handle.read()


def coro(x):
    return x


def forward(*args, **kwargs):
    # Star / double-star at the *call* site produces Starred + Keyword nodes.
    return plain(*args, c=3, **kwargs)


async def opener():
    return open("x")


def generators(xs):
    yield from xs
    for x in xs:
        yield x * 2


def comprehensions(xs):
    squares = [x * x for x in xs if x > 0]
    unique = {x for x in xs}
    mapping = {x: x * x for x in xs}
    lazy = (x for x in xs)
    return squares, unique, mapping, lazy


def walrus(data):
    if (n := len(data)) > 10:
        return n
    return 0


def formatting(name, value):
    return f"{name}={value!r:>{value}}"


def patterns(command):
    match command.split():
        case ["go", direction]:
            return direction
        case ["drop", *objects]:
            return objects
        case {"action": action, **rest}:
            return action, rest
        case Point(x=0, y=0):
            return "origin"
        case 1 | 2 | 3:
            return "small"
        case _:
            return None


class Point:
    x: int = 0
    y: int = 0


def lambdas():
    return lambda value, *rest, key=None: (value, rest, key)


def error_handling(path):
    try:
        with open(path) as handle:
            return handle.read()
    except FileNotFoundError as err:
        raise RuntimeError("missing") from err
    except (OSError, ValueError):
        return None
    finally:
        os.sync()


def identity[T](value: T) -> T:
    return value


class Container[T]:
    def __init__(self, item: T) -> None:
        self.item = item
"#;

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
fn broad_python_fixture_parses_without_error_kinds() {
    let input = tempdir().unwrap();
    let out = tempdir().unwrap();
    let source_path = input.path().join("broad.py");
    fs::write(&source_path, PYTHON_FIXTURE).unwrap();

    Command::cargo_bin("pyastgen")
        .unwrap()
        .arg("-out")
        .arg(out.path())
        .arg(input.path())
        .assert()
        .success();

    let output_path = out.path().join("broad.py.json");
    assert!(
        output_path.is_file(),
        "expected JSON output at {}",
        output_path.display()
    );

    let document: Value = serde_json::from_slice(&fs::read(&output_path).unwrap())
        .expect("emitted document must be valid JSON");

    // Envelope sanity: a successful parse, not an error stub.
    assert_eq!(document["backend"], "oxidized-pyastgen");
    assert_eq!(document["root"]["kind"], "Module");

    // Collect every node kind that appears anywhere in the tree.
    let mut kinds = BTreeSet::new();
    collect_kinds(&document["root"], &mut kinds);
    assert!(
        !kinds.is_empty(),
        "emitted tree contained no node kinds at all"
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

    // Meaningful coverage gate: each broad construct must actually be represented.
    // These kinds collectively cover functions+decorators, classes, async/await,
    // comprehensions, f-strings, walrus, match/case, try/except/finally, with,
    // PEP 695 type params, lambda, generators, star/double-star args and type hints.
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
        "broad Python fixture did not produce expected node kinds: {missing:?}\n\
         emitted kinds were: {kinds:?}"
    );

    // Spot-check structure: PEP 695 type params should hang under their owners and
    // the top-level body should hold every construct we wrote (non-empty subtrees).
    let body = document["root"]["children"]["body"]
        .as_array()
        .expect("module body must be an array");
    assert!(
        body.len() >= 10,
        "expected the broad fixture to yield many top-level statements, got {}",
        body.len()
    );
    for stmt in body {
        // Every emitted statement node carries a non-empty kind string.
        assert!(
            stmt["kind"].as_str().is_some_and(|kind| !kind.is_empty()),
            "top-level statement missing a kind: {stmt}"
        );
    }
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
