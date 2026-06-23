use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

/// Exercises the breadth of common and recently-mapped JS/TS constructs and
/// asserts the CLI maps every tree-sitter node, i.e. it prints no
/// `jsastgen: N unmapped node(s): ...` summary to stderr.
///
/// The fixture is a `.tsx` file so it can cover JSX in addition to advanced
/// TypeScript type machinery. It is parsed with `-t ts`, matching how the Scala
/// frontend invokes astgen (`AstGenRunner` passes `-t ts` for every `.js/.ts/.tsx`
/// input; there is no separate `tsx` type).
///
/// Deliberately excluded constructs (they currently fall through to `Noop` by
/// design and would trip the zero-unmapped assertion):
///   * `debugger` statements and hash-bang (`#!`) lines (see
///     `jsastgen_core::take_unmapped_summary` doc comment).
///   * The `undefined` keyword in *type* position inside a union (e.g.
///     `number | undefined`). `undefined` as a value/standalone type is mapped,
///     but the union-member path routes the keyword through `Noop`. `null` is
///     used instead so the union/optional surface is still covered.
const COVERAGE_TSX: &str = r#"// Advanced TypeScript type machinery.
type Keys = keyof { a: number; b: string };
type Indexed = { a: number; b: string }["a"];
type Conditional<T> = T extends string ? number : boolean;
type Mapped<T> = { [K in keyof T]: T[K] };
type Tuple = [first: number, ...rest: string[]];

interface Container<T extends object = {}> {
  value: T;
  read(): T;
}

enum Color {
  Red,
  Green = "green",
}

// Generics, classes, async, abstract members.
abstract class Repository<T> implements Container<object> {
  value: object = {};
  private items: T[] = [];

  constructor(public readonly name: string) {}

  read(): object {
    return this.value;
  }

  async load(id?: number): Promise<T> {
    const found = await Promise.resolve(this.items[id ?? 0]);
    return found;
  }

  *stream(): Generator<T> {
    for (const item of this.items) {
      yield item;
    }
  }

  *delegate(other: Repository<T>): Generator<T> {
    yield* other.stream();
  }
}

// Arrow functions, destructuring, spread, default + rest params.
const transform = <U,>({ value, ...rest }: Container<U>, ...extra: U[]): U[] => {
  const merged = [value, ...extra];
  return merged;
};

// Template literals + tagged templates.
function tag(strings: TemplateStringsArray, ...values: number[]): string {
  return strings.join("") + values.length;
}
const label = `count=${1 + 2}`;
const tagged = tag`sum=${1}${2}`;

// Optional chaining including an optional call `a?.b?.()`.
function probe(obj?: { nested?: { run?: () => number } }): number | null {
  return obj?.nested?.run?.() ?? null;
}

// new.target and import.meta.
function Widget(this: unknown): void {
  if (new.target) {
    void new.target;
  }
}
const metaUrl = import.meta.url;

// JSX (the reason this fixture is .tsx).
const view = (
  <div className="root" data-count={42}>
    <span>{label}</span>
    {[1, 2].map((n) => (
      <i key={n}>{n}</i>
    ))}
  </div>
);

export { Repository, Color, transform, probe, Widget, view, metaUrl, tagged };
export type { Keys, Indexed, Conditional, Mapped, Tuple, Container };
"#;

#[test]
fn covers_common_and_newly_mapped_constructs_without_unmapped_nodes() {
    let input = TempDir::new().unwrap();
    let output = TempDir::new().unwrap();
    let fixture = input.path().join("coverage.tsx");
    fs::write(&fixture, COVERAGE_TSX).unwrap();

    let assert = Command::cargo_bin("astgen")
        .unwrap()
        .args(["-t", "ts", "-o"])
        .arg(output.path())
        .arg(input.path())
        .assert()
        .success();

    // The run must produce JSON for the fixture.
    let json_path = output.path().join("coverage.tsx.json");
    assert!(
        json_path.is_file(),
        "expected emitted AST at {}",
        json_path.display()
    );

    // The headline assertion: nothing fell through to the unmapped counter.
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let unmapped: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("unmapped node(s)"))
        .collect();
    assert!(
        unmapped.is_empty(),
        "CLI reported unmapped nodes for the coverage fixture:\n{}",
        unmapped.join("\n")
    );
}
