# Rust pyastgen

This workspace contains the Rust AST generator track for an oxidized
`pysrc2cpg` frontend. The current Scala frontend still uses the JavaCC parser;
this generator establishes the Rust-side parser contract that the Scala bridge
can consume in follow-up parity slices.

The CLI preserves the native frontend style used by the other oxidized
frontends:

```bash
pyastgen -out <dir> <input-file-or-dir>
pyastgen -version
```

It writes one JSON document per Python source file. Each document contains a
normalized RustPython AST tree with source ranges, source text, scalar
properties, and named child groups.

Useful commands:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```
