# phpastgen JSON Contract

This document freezes the standalone Rust `phpastgen` compatibility contract.
The emitted JSON is compared with the pinned reference implementation.

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

Consumers can rely on the node type, nested fields, and position attributes
documented above. `endFilePos` is emitted in the raw reference format.

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
The historical downstream implementation treated `endFilePos` as the emitted
value plus one; emit the raw PHP-Parser value, not the incremented form. There
are otherwise no documented reference-compatible divergences. The loud
unmapped counter is the coverage signal: any construct the emitter cannot map
fails the coverage gate rather than silently dropping. New node kinds should
track the pinned JSON contract or be deliberately recorded here.
