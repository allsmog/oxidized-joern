# SwiftAstGen JSON Contract

This document freezes the compatibility contract between the oxidized
`SwiftAstGen` binary and the Scala `swiftsrc2cpg` pipeline. The Rust
implementation is expected to preserve this contract before any Scala-side CPG
construction changes are considered.

## CLI Contract

The binary must support the forms used by the Scala runner:

```bash
SwiftAstGen -o <out-dir> [--exclude-regex <regex>] [input]
SwiftAstGen --version
```

When `input` is omitted, the current working directory is parsed (the Scala
`AstGenRunner` invokes `SwiftAstGen -o <out> [--exclude-regex <regex>]` from the
source directory). Directory walks skip the same default top-level folders as
the Scala runner (`.*`, `__*`, `test`, `tests`, `spec`, `specs`). Unsupported
syntax fails per file and is reported on stdout so the existing skipped-file
handling can continue.

## JSON Envelope

The emitter produces one JSON document per parsed source file, shaped like a
SwiftSyntax tree. The root is a `SourceFileSyntax` object whose top-level fields
are read by `parser/SwiftJsonParser.scala`:

- `relativeFilePath` — path relative to the input root (the Scala `filename`).
- `fullFilePath` — absolute source path.
- `content` — full source file content (attached to the parse result).
- `loc` — integer line-of-code count.
- `nodeType` — the SwiftSyntax node type (e.g. `SourceFileSyntax`,
  `FunctionDeclSyntax`); token nodes additionally carry `tokenKind`.
- `children` — array of child nodes.

The Scala consumer is implemented by `parser/SwiftJsonParser.scala` (reads the
envelope fields above) and the generated `SwiftNodeSyntax` dispatch, which keys
on `nodeType` (falling back to `tokenKind`) and accesses named children by
keypath.

## Coverage Signal

tree-sitter nodes that cannot be mapped to a precise SwiftSyntax type degrade to
`Missing*` placeholders and are tallied process-wide
(`UNSUPPORTED_NODE_TALLY`). The CLI drains the tally and prints a single loud
line to **stderr** at the end of a run, e.g.:

```text
swiftastgen: 5 unsupported node(s) degraded to placeholders: missing_type(x3), missing_expr(x2)
```

When zero nodes degrade, stderr stays empty. The coverage test
(`tests/coverage.rs`) gates on this signal; it is the primary parity indicator
between differential runs.

## Known / Intentional Divergences

The reference differential against the upstream SwiftSyntax `SwiftAstGen`
surfaced the following. These are resolved and treated as compatible:

- **`projectFullPath` (root node only)** — the absolute path of the input
  (`--src`) root directory. This is a real reference field and is now emitted,
  threaded from the CLI input root through `parse_file`.
- **`index` = position only within `*ListSyntax` parents** — the child index is
  meaningful only for list-syntax parents; elsewhere it is not a positional
  ordinal.
- **List-element `name = ""`** — elements of a `*ListSyntax` parent carry an
  empty keypath `name`, matching the reference.
- **`if` / `switch` wrapped in `ExpressionStmtSyntax`** — control-flow `if` and
  `switch` constructs are wrapped in an `ExpressionStmtSyntax` to match the
  reference shape.
- **Compound assignments are `BinaryOperatorExprSyntax`** — `+=`, `-=`, etc. are
  emitted as `BinaryOperatorExprSyntax`, not a dedicated assignment node.
- **`name` (keypath label on essentially every node)** — the SwiftSyntax
  child-field/keypath label a node occupies in its parent (e.g. `item`, `decl`,
  `signature`, `body`, `''` on the root). It is a SwiftSyntax serialization
  artifact with no tree-sitter equivalent; the Scala CPG builder never consumes
  it (the full swift CPG suite passes without it). The differential harness
  strips it from both trees.

With those normalizations in place, the checked-in fixture corpus is expected to
match the reference `SwiftAstGen` JSON. New mismatches are treated as either
Rust compatibility bugs or newly discovered intentional divergences that must be
documented here with a fixture-backed normalizer.
