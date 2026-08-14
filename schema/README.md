# CPG schema

`cpg-schema.json` is the pinned schema snapshot used by the Rust
`oxidized/crates/cpg-schema` validator. The snapshot records node labels,
properties, cardinalities, edge labels, and allowed endpoints.

Schema updates must include the updated JSON snapshot and pass:

```bash
cargo test --manifest-path oxidized/Cargo.toml --locked
```
