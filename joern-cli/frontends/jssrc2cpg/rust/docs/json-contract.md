# astgen (JavaScript) JSON Contract

This document freezes the compatibility contract between the oxidized JavaScript
`astgen` binary and the Scala `jssrc2cpg` pipeline. The Rust implementation is
expected to preserve this contract before any Scala-side CPG construction
changes are considered.

## CLI Contract

The binary is invoked by `AstGenRunner` as:

```bash
astgen -t ts  -o <output-dir>   # JavaScript / TypeScript
astgen -t vue -o <output-dir>   # Vue single-file components
astgen --version
```

For each accepted source file the emitter writes one JSON document (a
Babel-style AST) plus an optional `.typemap` sidecar.

## JSON Envelope

Each emitted document wraps a Babel `File` AST:

```json
{
  "fullName":     "<absolute path>",
  "relativeName": "<path relative to input root>",
  "ast": {
    "type": "File",
    "start": <int>, "end": <int>,
    "loc": { "start": {"line": <int>, "column": <int>, "index": <int>},
             "end":   {"line": <int>, "column": <int>, "index": <int>} },
    "program": { "type": "Program", "sourceType": "module",
                 "extra": {"topLevelAwait": false}, "body": [ ... ] },
    "comments": [],
    "errors": []
  }
}
```

Every node carries a `type` field (Babel spec node names such as
`VariableDeclaration`, `ClassDeclaration`, `FunctionExpression`), `start`/`end`
offsets, and a `loc` line/column range.

The Scala consumer is implemented by `parser/BabelJsonParser.scala`. Its
`ParseResult` reads `relativeName` (the `filename`), `fullName`, and `ast` from
the top-level object, loads the `.typemap` sidecar into a `typeMap`, and parses
the JSON via ujson.

## Coverage Signal

tree-sitter node kinds that fall through to a `Noop` mapping are accumulated in
a thread-local `UNMAPPED_KINDS` map. The CLI drains it via
`take_unmapped_summary()` and prints a single loud line to **stderr**, e.g.:

```text
jsastgen: 2 unmapped node(s): unsupported_node(x2)
```

It never reaches stdout/JSON. The coverage test (`tests/coverage.rs`) gates on
zero unmapped nodes; this is the primary parity signal between differential
runs.

## Known / Intentional Divergences

The differential harness normalizes `loc` objects before comparing against the
reference `@joernio/astgen` JSON. Numeric `start`/`end` offsets remain part of
the contract. This keeps byte-position parity gated while avoiding brittle
matches on Babel's inconsistent nested `loc.start`/`loc.end` sparsity for
synthetic TypeScript and JSX nodes.

The loud unmapped counter above is the coverage signal: any construct the
emitter cannot map surfaces as an unmapped tally and fails the coverage gate,
rather than silently dropping it. New emitted node types should track the Babel
shapes that `jssrc2cpg`'s AST creation already consumes, or be deliberately
recorded here.
