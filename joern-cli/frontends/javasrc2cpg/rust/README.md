# javaastgen

This workspace contains the Rust AST generator track for an oxidized
`javasrc2cpg` frontend. The current Scala frontend is JavaParser-based; this
workspace establishes the Rust parser binary, JSON envelope, and coverage gates
that the Scala consumer can be wired to behind an oxidized backend.

```bash
cd joern-cli/frontends/javasrc2cpg/rust
cargo test
cargo run -p javaastgen-cli --bin javaastgen -- -out /tmp/javaastgen-json path/to/project
```

The generated JSON contract is documented in `docs/json-contract.md`.
