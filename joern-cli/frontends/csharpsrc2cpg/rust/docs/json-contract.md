# dotnetastgen JSON Contract

This document freezes the compatibility contract between the oxidized
`dotnetastgen` binary and the Scala `csharpsrc2cpg` pipeline. The Rust
implementation is expected to preserve this contract before any Scala-side CPG
construction changes are considered.

## CLI Contract

The binary is invoked by `DotNetAstGenRunner.runAstGenNative` as:

```bash
dotnetastgen -i <input> -o <output-dir>
dotnetastgen --version
```

For each accepted C# source file the emitter writes one JSON document into the
output directory.

## JSON Envelope

The root document represents a compilation unit. Top-level shape:

```json
{
  "FileName": "<path>",
  "AstRoot": {
    "MetaData": {
      "Kind": "ast.CompilationUnit",
      "Code": "<source>",
      "LineStart": <int>, "ColumnStart": <int>,
      "LineEnd": <int>,   "ColumnEnd": <int>
    },
    "Usings":  [ <using directives> ],
    "Members": [ <type declarations and statements> ]
  }
}
```

Every AST node carries a `MetaData` object with `Kind` (an `ast.<NodeType>`
string), `Code`, and `Line/Column` start/end positions, plus node-type-specific
fields.

The Scala consumer is implemented by:

- `parser/DotNetJsonParser.scala` (`DotNetJsonParser.readFile`), which reads
  each JSON document.
- `parser/DotNetJsonAst.scala`, whose `ParserKeys` object enumerates the
  field names read (`FileName`, `AstRoot`, `MetaData`, `Kind`, `Code`,
  `LineStart`/`ColumnStart`/`LineEnd`/`ColumnEnd`, `Usings`, `Members`,
  `Body`, `Expression`, `Identifier`, `Parameters`, `Type`, etc.), and the
  sealed `DotNetParserNode` hierarchy keyed on `Kind`.

## Coverage Signal

tree-sitter node kinds that fall through to an `Unknown` mapping are accumulated
in a thread-local `UNMAPPED_KINDS` map. The CLI renders a single loud summary
line to **stderr** at the end of a run, e.g.:

```text
dotnetastgen: <count> unmapped node(s): <kind>(x<n>), ...
```

It is never written to stdout/JSON. The coverage test (`tests/coverage.rs`)
gates on zero unmapped nodes; this is the primary parity signal between
differential runs.

## Known / Intentional Divergences

There are currently no documented Scala-compatible divergences specific to this
frontend beyond ordinary path normalization performed by the differential
harness. The loud unmapped counter above is the coverage signal: any new
construct that the emitter cannot map surfaces as an unmapped tally and fails
the coverage gate, rather than silently producing `Unknown` nodes. New node
kinds should not be introduced until the Scala `DotNetJsonAst`/AST-creation code
is extended and covered by CPG tests, or the behavior is deliberately recorded
here.
