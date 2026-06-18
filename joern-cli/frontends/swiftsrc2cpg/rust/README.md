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

The current implementation emits SwiftSyntax-shaped JSON for an initial parity
slice: empty files, top-level `let`/`var` declarations, simple tuple variable
declarations, simple type annotations, integer and string literal initializers,
identifier references, simple function declarations with bodies, simple
function calls with labeled arguments, return statements, reassignment
expressions, binary arithmetic, comparison, equality, and boolean operators,
simple `if`/`else` control flow, simple `while` loops, simple identifier
`for-in` loops, unlabeled `break`/`continue`, boolean literals, simple
`class`/`struct` member blocks, and dot member-access expressions. Unsupported
syntax fails per-file and is reported on stdout so the existing skipped-file
handling can continue.

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
