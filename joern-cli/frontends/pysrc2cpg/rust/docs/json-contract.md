# pyastgen JSON Contract

This document freezes the compatibility contract for the oxidized `pyastgen`
binary used (optionally) by `pysrc2cpg`.

> Context: `pysrc2cpg`'s production parser is the in-tree JavaCC grammar
> (`pythonGrammar.jj` → `io.joern.pythonparser.PythonParser`), which parses
> Python source **directly into a JVM AST with no JSON envelope and no external
> process**. The Rust `pyastgen` crate is an alternative `Oxidized` backend
> (selected via `Py2CpgOnFileSystem`'s `PythonParserBackend`) that runs as an
> external binary and emits JSON consumed by `parser/PyAstJsonParser.scala`.
> Because the only reference is the in-tree parser, there is **no cross-binary
> differential**; the coverage gate guards the emitter.

## CLI Contract

When the `Oxidized` backend is active, `PyAstGenRunner` invokes:

```bash
pyastgen -out <output-dir> <input>
pyastgen -version
```

## JSON Envelope

The emitted document (`PyAstDocument`) wraps a single AST root:

```json
{
  "backend": "oxidized-pyastgen",
  "version": "<crate version>",
  "path": "<source path>",
  "source_length": <int>,
  "root": {
    "kind": "<node kind>",
    "range": { ... },                 // optional source range
    "text": "<optional>",
    "properties": { ... },            // scalar/string properties
    "children": { "<field>": [ <PyAstNode>, ... ] }
  }
}
```

`PyAstNode` recurses through `children`, which is a map from child-field name to
an ordered list of child nodes. The underlying parser is `rustpython-parser`
with `all-nodes-with-ranges`.

The Scala consumer is `parser/PyAstJsonParser.scala` (used only when the
`Oxidized` backend is selected).

## Coverage Signal

There is **no unmapped counter**: the emitter is expected to map every construct
exhaustively. The coverage gate (`tests/coverage.rs`) enforces this directly —
it fails if any node `kind` is one of the error/unknown markers (`Unknown`,
`Unmapped`, `Unsupported`, `Error`, `Invalid`, `NotHandled`, `Placeholder`) and
asserts that the emitted JSON contains the expected Python construct kinds
(functions, classes, comprehensions, `match`, etc.). A regression surfaces as an
error-marker node or a missing expected kind.

## Known / Intentional Divergences

The defining divergence is structural: the production path does **not** use this
JSON contract at all — it uses the in-tree JavaCC parser and an in-memory AST,
so this envelope only applies to the optional `Oxidized` backend. There are no
other documented divergences. The coverage gate (zero error markers + required
construct kinds) is the parity signal in lieu of a cross-binary differential.
