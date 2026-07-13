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
```

For each accepted ABAP source object the emitter writes one JSON document. The
output filename replaces the final `.abap` extension with `.json`, matching the
reference binary; for example, `z_demo.clas.abap` becomes `z_demo.clas.json`.

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
field), and `start`/`end` positions with `row`/`col`. End columns are
one-past-the-final-character. Full-line `*` comments are emitted as `Comment`
statements with the whole comment line as a single token. Hyphenated ABAP
keywords such as `AUTHORITY-CHECK`, `EDITOR-CALL`, and `CLASS-METHODS` are
tokenized as separate word, `-`, word tokens.

The Scala consumer is implemented by `parser/AbapJsonParser.scala`, which reads
the `file`, `objectType`, and `statements` fields, and per statement the `type`,
`tokens[].str`, and `start`/`end` `row`/`col` positions.

## Coverage Signal

Unexpected classifier fallthroughs are emitted as `Unknown` and counted by the
static `UNCLASSIFIED_COUNT`. The CLI prints a single loud summary line to
**stderr** at the end of a run:

```text
abapastgen: <count> unclassified statement(s)
```

It never reaches stdout/JSON. The coverage test (`tests/coverage.rs`) gates on
this signal; it is the primary parity indicator between differential runs.

Some `Unknown` statement types are reference-compatible rather than unexpected
fallthroughs. The released reference emits `Unknown` for `DELETE DYNPRO` and
`CALL TRANSFORMATION`; the Rust emitter preserves that JSON shape without
incrementing `UNCLASSIFIED_COUNT`, and the Scala parser recovers those token
streams into dedicated CPG calls.

## Known / Intentional Divergences

There are currently no documented Scala-compatible divergences specific to this
frontend beyond the path normalization the differential harness applies. The
Rust JSON emitted for the checked-in fixture corpus is expected to match the
reference after that normalization.
