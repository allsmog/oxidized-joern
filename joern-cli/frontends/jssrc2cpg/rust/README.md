# Rust jsastgen

This workspace is the Rust AST generator track for `jssrc2cpg`.

The binary is named `astgen` because the existing Scala JavaScript frontend
already resolves and invokes an `astgen` executable. The current implementation
parses JavaScript with tree-sitter and emits the Babel-shaped JSON contract that
`BabelJsonParser` and the Scala CPG creation passes consume.

This is intentionally opt-in while parity is being built:

```bash
cd joern-cli/frontends/jssrc2cpg/rust
cargo test
cargo build --release --bin astgen
```

The supported JSON subset currently covers core script constructs such as
variable declarations, literals, identifiers, expression statements, function
declarations, return statements, binary expressions, assignments, member access,
and calls. Unsupported syntax is emitted as `Noop` nodes so the contract stays
well-formed while the mapper grows toward full Babel parity.
