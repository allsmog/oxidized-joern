# rust_ast_gen JSON Contract

This document freezes the compatibility contract between the oxidized
`rust_ast_gen` binary and the Scala `rust2cpg` pipeline. The Rust
implementation is expected to preserve this contract before any Scala-side CPG
construction changes are considered.

## CLI Contract

The binary is invoked by `RustAstGenRunner.runAstGenNative` as:

```bash
rust_ast_gen -i <input> -o <output-dir>
rust_ast_gen --version
```

For each accepted Rust source file the emitter writes one JSON document.

## JSON Envelope

The root document carries file metadata and a single source-file child node:

```json
{
  "relativeFilePath": "<path relative to input root>",
  "fullFilePath":     "<absolute path>",
  "content":          "<source>",
  "crateName":        "<optional string>",
  "modulePath":       "<optional string>",
  "loc":              <int>,
  "children":         [ <SOURCE_FILE node> ]
}
```

Each node carries `nodeKind` (the `ra_ap_syntax` `SyntaxKind` rendered via
`format!("{kind:?}")`, e.g. `SOURCE_FILE`, `STRUCT`, `FN`), a `range`
(`startOffset`, `endOffset`, `startLine`, `startColumn`), an optional
`children` array, and optional enrichment fields `text` (tokens),
`typeFullName`, `methodFullName`, and `macroExpansion`.

The Scala consumer is implemented by `parser/RustJsonParser.scala`, which reads
`relativeFilePath` (the `filename`), `fullFilePath`, `content`, optional
`crateName`/`modulePath`, `loc`, and the single AST root at `children.head`
(handed to the `RustNodeSyntax` builder).

## Coverage Signal

Unlike the other oxidized frontends, `rust_ast_gen` is a generic
`ra_ap_syntax` tree-walker: it emits **every** syntax-node kind via
`format!("{kind:?}")`, so there is **no `Unknown`/unmapped fallback to count**.
Coverage (`tests/coverage.rs`) is instead asserted positively: the CLI exits
successfully and produces JSON, a representative set of `nodeKind`s is present,
and the semantic enrichment ran (at least one node carries `typeFullName` and at
least one carries `methodFullName`).

## Known / Intentional Divergences

There are currently no documented Scala-compatible divergences specific to this
frontend beyond the path normalization the differential harness applies.
Because there is no unmapped counter, the positive coverage assertions above are
the parity signal: a regression shows up as a missing expected `nodeKind` or
missing enrichment rather than as a tallied unmapped node. Changes to the
emitted node shape should track what `rust2cpg`'s AST creation consumes, or be
deliberately recorded here.
