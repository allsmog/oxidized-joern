# javaastgen

This workspace contains the standalone Rust Java AST generator. It provides a
parser binary, a stable JSON envelope, and coverage gates without a JVM build.

```bash
cd astgen-rs/javasrc2cpg
cargo test
cargo run -p javaastgen-cli --bin javaastgen -- -out /tmp/javaastgen-json path/to/project
```

The generated JSON contract is documented in `docs/json-contract.md`.
