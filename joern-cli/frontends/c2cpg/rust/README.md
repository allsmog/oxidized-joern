# Rust cxxastgen

This workspace contains the Rust AST generator track for the oxidized `c2cpg`
backend. The Scala frontend still defaults to Eclipse CDT, while this backend is
available through the hidden `--parser-backend oxidized` compatibility route.

The current binary preserves the existing native frontend style:

```bash
cxxastgen [-include <path>] [-define <name[=value]>] [-compilation-database <compile_commands.json>] \
  [-skip-function-bodies] -out <dir> <input>
cxxastgen -version
```

It writes one oxidized JSON document per C/C++ input file. The Scala
`c2cpg` oxidized pass consumes those documents and builds CPGs through the same
post-processing passes used by the rest of the frontend.

Useful commands:

```bash
cd joern-cli/frontends/c2cpg/rust
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Run the Scala compatibility and full frontend suites from the repository root:

```bash
sbt 'c2cpg/testOnly io.joern.c2cpg.compat.CdtCompatibilitySnapshotTests io.joern.c2cpg.compat.BackendParitySnapshotTests io.joern.c2cpg.compat.OxidizedCompatibilitySnapshotTests io.joern.c2cpg.compat.OxidizedVirtualDispatchTests'
sbt 'c2cpg/test'
```
