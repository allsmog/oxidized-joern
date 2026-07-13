# javaastgen JSON Contract

This document freezes the first oxidized Java parser contract. The current
Scala `javasrc2cpg` pipeline consumes JavaParser nodes directly; the Rust
generator emits a lossless syntax-tree JSON envelope that a Scala bridge can
consume without reparsing source text.

## CLI Contract

The binary is invoked as:

```bash
javaastgen -out <output-dir> <input-file-or-directory>
javaastgen --version
```

For each accepted `.java` source file the emitter writes one JSON document at
the matching relative output path with `.json` appended to the source filename.
For example, `src/Foo.java` becomes `<output-dir>/src/Foo.java.json`.

`-out` and `-version` are accepted as aliases for `--out` and `--version` to
match the existing astgen runner conventions.

## JSON Envelope

Each emitted document has this shape:

```json
{
  "fullName": "<canonical source path when available>",
  "relativeName": "<path relative to input root>",
  "ast": {
    "kind": "program",
    "fieldName": null,
    "named": true,
    "missing": false,
    "extra": false,
    "hasError": false,
    "startByte": 0,
    "endByte": 42,
    "start": { "line": 1, "column": 1 },
    "end": { "line": 2, "column": 1 },
    "code": "class Foo {}",
    "children": []
  }
}
```

Every node preserves:

- the tree-sitter `kind`;
- its parent field name when tree-sitter exposes one;
- whether it is named, missing, extra, or error-containing;
- byte start/end offsets into the original UTF-8 source;
- one-based line/column positions from tree-sitter points;
- exact source text for the node;
- ordered children.

The source text and byte offsets are the compatibility anchor. The future Scala
bridge should derive CPG code strings and offsets from these fields instead of
invoking JavaParser.
