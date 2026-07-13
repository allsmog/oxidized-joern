# rubyastgen JSON Contract

This document freezes the compatibility contract between the oxidized
`rubyastgen` binary and the Scala `rubysrc2cpg` pipeline. The Rust
implementation is expected to preserve this contract before any Scala-side CPG
construction changes are considered. The reference shape follows the
`ruby_ast_gen` gem (a wrapper around the `parser` gem).

## CLI Contract

The binary is invoked by `RubyAstGenRunner.runAstGenNative` with positional
arguments:

```bash
rubyastgen <input> <output>
rubyastgen --version
```

For each accepted Ruby source file the emitter writes one JSON document.

## JSON Envelope

The root document is a `begin` node carrying file metadata and a body:

```json
{
  "type": "begin",
  "file_path": "<absolute path>",
  "rel_file_path": "<path relative to input root>",
  "meta_data": {
    "code": "<source snippet>",
    "start_line": <int>, "start_column": <int>,
    "end_line": <int>,   "end_column": <int>,
    "offset_start": <int>, "offset_end": <int>
  },
  "body": [ <child nodes> ]
}
```

Each node carries a `type` (the `parser` gem node name, e.g. `lvasgn`, `send`,
`class`, `if`), a `meta_data` location object, and node-type-specific fields
(`lhs`/`rhs`, `receiver`/`name`/`arguments`, `body`, etc.).

The Scala consumer is implemented by:

- `parser/RubyJsonParser.scala` (`RubyJsonParser.readFile`), which reads
  `file_path` (the absolute `filePath`) and `rel_file_path` (`relFilePath`) and
  loads the source content.
- `parser/RubyJsonToNodeCreator.scala`, which walks the tree from `visitProgram`
  and dispatches on the node `type` via the `AstType` enum.
- `parser/RubyJsonAst.scala`, whose `ParserKeys` object enumerates the field
  names (`file_path`, `rel_file_path`, `type`, `meta_data`, `code`, `start`,
  `end`, `value`, `children`, `arguments`, `body`, `receiver`, `name`, ...).

## Coverage Signal

Node variants that cannot be mapped are recorded by parser-gem node name in a
thread-local `UNKNOWN_NODES` map. `take_unknown_node_summary()` drains it into a
single loud line written to **stderr** at the end of a run:

```text
rubyastgen: <total> unmapped node(s): <name>(x<n>), ...
```

It never reaches stdout/JSON. The coverage test (`tests/coverage.rs`) gates on
this signal; it is the primary parity indicator (the CI differential job is
absent by design, so the coverage gate is the standing guard).

## Known / Intentional Divergences

There are currently no documented Scala-compatible divergences specific to this
frontend beyond the path normalization the differential harness applies. The
loud unmapped counter is the coverage signal: any `parser` node variant the
emitter cannot map surfaces in the tally and fails the coverage gate rather than
being silently dropped. New mapped node types should track what
`RubyJsonToNodeCreator` consumes, or be deliberately recorded here.
