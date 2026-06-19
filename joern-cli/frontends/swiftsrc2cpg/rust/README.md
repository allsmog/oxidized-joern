# Rust SwiftAstGen

This workspace contains the oxidized implementation track for the `SwiftAstGen`
binary used by `swiftsrc2cpg`.

The CLI is compatible with the existing frontend runner:

```bash
SwiftAstGen -o <out-dir> [--exclude-regex <regex>] [input]
SwiftAstGen --version
```

When `input` is omitted, the current working directory is parsed. This matches
the Scala runner, which invokes `SwiftAstGen` from the source directory.
Directory walks skip the same default top-level folders as the Scala runner
(`.*`, `__*`, `test`, `tests`, `spec`, and `specs`), and `--exclude-regex`
accepts Java-style quoted fragments such as `\Q/\E`.

The current implementation emits SwiftSyntax-shaped JSON for an initial parity
slice: empty files, top-level `let`/`var` declarations, simple tuple variable
declarations, simple type annotations, integer and string literal initializers,
identifier references, simple function declarations with bodies, simple
function external parameter labels, simple function calls with labeled
arguments, return statements, reassignment expressions, binary arithmetic,
comparison, equality, and boolean operators, range expressions, ordinary prefix
operator expressions, array, dictionary, and tuple literal expressions, simple
import declarations, simple closure literals and trailing closures, simple
subscript/index expressions, simple `if`/`else` control flow, simple `while`
loops, simple `guard` statements, simple `defer` statements, simple identifier
`for-in` loops, unlabeled `break`/`continue`, boolean literals, simple
`class`/`struct` member blocks,
class/struct inheritance
clauses, simple enum declarations and enum case declarations, simple protocol
declarations with protocol functions, protocol properties, and associated
types, recovered initialized protocol members emitted for upstream test
compatibility, simple initializer and deinitializer declarations, simple
subscript declarations with direct bodies and `get`/`set` accessors, computed
property bodies and protocol property accessors, simple typealias declarations
with identifier, tuple, and function-type initializers, simple switch
statements, wildcard patterns, simple extension declarations with inheritance
clauses, dot member-access expressions including `self` and `super` bases,
simple implicit member expressions, simple actor declarations, simple operator
declarations, parser-only precedence group declarations, simple declaration
attributes, and simple declaration modifiers. Unsupported syntax fails per-file
and is reported on stdout so the existing skipped-file handling can continue.

Run the Rust checks with:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Install the locally built binary into the frontend's `bin/astgen` directory with:

```bash
sbt 'swiftsrc2cpg/swiftAstGenBuildRust'
```
