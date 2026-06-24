# abapgen JSON Contract

This document freezes the compatibility contract between the oxidized `abapgen`
binary and the Scala `abap2cpg` pipeline. The Rust implementation is expected to
preserve this contract before any Scala-side CPG construction changes are
considered.

## CLI Contract

The binary is invoked by `AbapAstGenRunner.runAstGenNative` with positional
arguments:

```bash
abapgen <input> <output>
abapgen --version
```

For each accepted ABAP source object the emitter writes one JSON document.

## JSON Envelope

The root document is a flat program object with a statement list:

```json
{
  "file": "<path>",
  "objectType": "<string>",
  "statements": [
    {
      "type": "<statement kind>",
      "tokens": [ { "str": "<token text>" }, ... ],
      "start": { "row": <int>, "col": <int> },
      "end":   { "row": <int>, "col": <int> }
    }
  ]
}
```

Each statement carries a `type`, a `tokens` array (token objects with a `str`
field), and `start`/`end` positions with `row`/`col`.

The Scala consumer is implemented by `parser/AbapJsonParser.scala`, which reads
the `file`, `objectType`, and `statements` fields, and per statement the `type`,
`tokens[].str`, and `start`/`end` `row`/`col` positions.

## Coverage Signal

Statements that cannot be classified are emitted as `Unknown` and counted by the
static `UNCLASSIFIED_COUNT`. The CLI prints a single loud summary line to
**stderr** at the end of a run:

```text
abapastgen: <count> unclassified statement(s)
```

It never reaches stdout/JSON. The coverage test (`tests/coverage.rs`) gates on
this signal; it is the primary parity indicator between differential runs.

## Known / Intentional Divergences

There are currently no documented Scala-compatible divergences specific to this
frontend beyond the path normalization the differential harness applies. The
loud unclassified counter above is the coverage signal: any statement form the
emitter cannot classify surfaces in the tally and fails the coverage gate rather
than being silently mismapped. New statement classifications should track what
`abap2cpg`'s AST creation consumes, or be deliberately recorded here.
