# CPG Schema

`cpg-schema.json` is generated from the Scala CPG schema classes on the build
classpath. Do not edit it by hand.

Regenerate it from the repository root:

```bash
sbt "semanticcpg/runMain io.joern.oxidized.schema.SchemaDump"
```

The dump records the CPG dependency version, node labels, per-node properties,
property value types and cardinalities, edge labels, and allowed edge endpoints
derived from the generated `New*` node validators.
