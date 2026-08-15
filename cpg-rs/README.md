# cpg-rs

`cpg-rs` is the Rust-native engine and `cpg` command published by Oxidized
Joern. It parses source into a language-independent graph, runs control-flow
and data-flow passes, saves the result, and exposes the graph through CLI,
SARIF, JSON, and MCP interfaces.

## Status

The `0.1.x` release line is production-ready for its documented native
workflows, but it is not a universal drop-in Joern replacement.

The CLI accepts C, C++, Go, Java, JavaScript, TypeScript, Python, Ruby, Rust,
and Scala. C's shipped build/analysis path is the same path guarded by 122/122
exact Joern v4.0.555 corpus blocks, including 1,961/1,961 ReachingDef facts.
Its deterministic build, persistence, export, flow, scan, SARIF, and update
workflows are also gated on pinned zlib and Lua releases and labeled security
outcomes. C++, Go, Java, JavaScript, TypeScript, Python, Ruby, and Rust also
carry production-preview contracts backed by live Joern semantic probes, two
pinned real projects each, and labeled default-rule outcomes. Scala remains a
native-only experimental frontend because Joern v4.0.555 has no Scala source
frontend to serve as an oracle.

[`COMPATIBILITY.md`](COMPATIBILITY.md) is the authoritative language/workflow
matrix and states the exact production boundary. [`ROADMAP.md`](ROADMAP.md)
lists the remaining work without treating Scala/JVM compatibility as a goal.

## Install

Release archives contain only the native `cpg` executable, this document, and
the Apache 2.0 license. From the repository root:

```bash
sh cpg-install.sh
cpg --version
```

The installer verifies the published SHA-256 checksum before installing the
binary. Native archives are available for Linux and macOS on x86-64 and ARM64,
and Windows on x86-64.

## Build and test

```bash
cd cpg-rs
cargo build --workspace --locked
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

## CLI examples

```bash
# Build and save a graph.
cargo run --release -p cpg-cli -- \
  build ./src -o project.cpg --lang rust

# Scan with the built-in rules for that language and emit SARIF.
cargo run --release -p cpg-cli -- \
  scan --load project.cpg --lang rust -o findings.sarif

# Run an ad-hoc source-to-sink query.
cargo run --release -p cpg-cli -- \
  flow 'getenv' 'exec*' --load project.cpg -o flows.json

# Run native CPGQL as JSON, or use `.p` for annotated terminal rows.
cargo run --release -p cpg-cli -- \
  query --load project.cpg --lang rust --query 'cpg.method.name' --format json

# Serve the saved graph or expose the MCP server over stdio.
cargo run --release -p cpg-cli -- serve --load project.cpg
cargo run --release -p cpg-cli -- mcp --root ./src
```

Run `cargo run -p cpg-cli -- --help` for build, scan, slice, merge, API census,
export, flow, vectors, workspace, and MCP commands.

## Design and compatibility

[`ARCHITECTURE.md`](ARCHITECTURE.md) describes storage, incrementality,
summaries, and known simplifications. [`PROGRESS.md`](PROGRESS.md) records work
against the pinned Joern oracle. Pull requests run the committed parity and
semantic outcome gates; releases additionally run pinned zlib/Lua and 16
non-C real-project workflows, archive tests, and container tests.

Treat benchmark results as measurements of their named fixtures. Run the
examples in `cpg-incremental/examples` on your own corpus before making
capacity decisions.
