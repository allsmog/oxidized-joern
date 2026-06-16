# Rust cxxastgen

This workspace is the experimental Rust AST generator track for `c2cpg`.
It is not wired into production CPG generation yet.

The current binary provides the stable shell of the future backend:

```bash
cxxastgen [-include <path>] [-define <name[=value]>] [-compilation-database <compile_commands.json>] \
  [-skip-function-bodies] -out <dir> <input>
cxxastgen -version
```

For now it writes an experimental JSON envelope per C/C++ input file. The Scala
`c2cpg` frontend still defaults to Eclipse CDT and the hidden
`--parser-backend oxidized` route fails clearly until this workspace emits
CPG-compatible translation units.

Useful commands:

```bash
cd joern-cli/frontends/c2cpg/rust
cargo fmt --check
cargo test
```
