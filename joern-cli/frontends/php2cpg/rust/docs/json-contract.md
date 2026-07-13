# phpastgen JSON Contract

This document freezes the compatibility contract between the oxidized
`phpastgen` binary and the Scala `php2cpg` pipeline. The Rust implementation is
expected to preserve this contract before any Scala-side CPG construction
changes are considered.

The reference is the upstream `joernio/PHP-Parser` `php-parser.phar` (a fork of
nikic/PHP-Parser), run through PHP with `--with-recovery --resolve-names
--json-dump`.

## JSON Envelope

The emitted document is a JSON **array** of node objects (the PHP-Parser
`--json-dump` shape). Each node object has:

```json
{
  "nodeType": "<kind>",          // e.g. "Stmt_Class", "Expr_MethodCall", "Scalar_LNumber"
  "attributes": {
    "startLine": <int>,
    "endLine": <int>,
    "startFilePos": <int>,
    "endFilePos": <int>,
    "kind": <int>                // optional
  },
  "<type-specific fields>": ...  // e.g. "name", "stmts", "expr", "value", "left", "right"
}
```

Nested nodes appear as nested objects or arrays under the type-specific fields.

The Scala consumer is implemented by:

- `parser/PhpParser.scala`, which runs the parser and reads the JSON via ujson
  (`PhpParseResult(fileName, parseResult, infoLines)`).
- `parser/Domain.scala`, whose `object Domain` (`fromJson`) decodes nodes via
  upickle. `PhpAttributes` reads `startLine`, `endLine`, `kind`, `startFilePos`,
  and `endFilePos` (note: `endFilePos` is read as the JSON value `+ 1`).

## Coverage Signal

tree-sitter node kinds that cannot be mapped to a PHP-Parser node are tallied in
a thread-local `UNMAPPED_KINDS` map and surfaced through
`with_unmapped_summary`, which renders a single loud line to **stderr**:

```text
phpastgen: <total> unmapped node(s): <kind>(x<n>), ...
```

It never reaches stdout/JSON. The coverage test (`tests/coverage.rs`,
`corpus_lowers_with_zero_unmapped_nodes`) panics if any unmapped nodes are
found over the fixture corpus (classes, traits, namespaces, match expressions,
heredoc). This is the primary parity signal between differential runs.

## Known / Intentional Divergences

Beyond the path normalization the differential harness applies, note that the
Scala `Domain` reads `endFilePos` as the emitted value `+ 1` — emit the raw
PHP-Parser `endFilePos`, not the incremented form. There are otherwise no
documented Scala-compatible divergences specific to this frontend. The loud
unmapped counter is the coverage signal: any construct the emitter cannot map
fails the coverage gate rather than silently dropping. New node kinds should
track what `php2cpg`'s `Domain`/AST creation consumes, or be deliberately
recorded here.
