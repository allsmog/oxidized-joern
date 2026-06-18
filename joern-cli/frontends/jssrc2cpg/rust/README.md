# Rust jsastgen

This workspace is the Rust AST generator track for `jssrc2cpg`.

The binary is named `astgen` because the existing Scala JavaScript frontend
already resolves and invokes an `astgen` executable. The current implementation
parses JavaScript, TypeScript, JSX/TSX, and Vue single-file components with
tree-sitter and emits the Babel-shaped JSON contract that `BabelJsonParser` and
the Scala CPG creation passes consume. For TypeScript inputs it also emits
`.typemap` sidecars consumed by the Scala type-recovery pass.

This is intentionally opt-in while parity is being built:

```bash
cd joern-cli/frontends/jssrc2cpg/rust
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release --bin astgen
```

The mapper covers the JavaScript and TypeScript constructs exercised by the
Scala `jssrc2cpg` suite, including modules, classes, decorators, destructuring,
control-flow, JSX, Vue template/script offsets, TypeScript declarations, and
type-map generation.

Build and test the Scala frontend against this Rust binary from the repository
root with:

```bash
sbt 'jssrc2cpg/jsAstGenBuildRust' 'jssrc2cpg/test'
```
