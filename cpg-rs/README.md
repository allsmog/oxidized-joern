# cpg-rs

`cpg-rs` is the Rust-native engine and `cpg` command published by Oxidized
Joern. It parses source into a language-independent graph, runs control-flow
and data-flow passes, saves the result, and exposes the graph through CLI,
SARIF, JSON, and MCP interfaces.

## Status

The `0.1.x` release line is a preview, not a drop-in Joern replacement.

The CLI accepts C, C++, Go, Java, JavaScript, TypeScript, Python, Ruby, Rust,
and Scala. Most modes use a shared tree-sitter frontend with a language
specification. C also has a dedicated frontend. Parsing tolerates incomplete
source, but type resolution and graph detail do not yet match a compiler or
Joern across every language.

The standalone C differential harness has exact output parity for its pinned
fixtures. The production engine still needs the convergence work described in
[`ROADMAP.md`](ROADMAP.md) before that result can support a broader parity
claim. [`GOAL.md`](GOAL.md) defines the differential-testing gate.

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

# Serve the saved graph or expose the MCP server over stdio.
cargo run --release -p cpg-cli -- serve --load project.cpg
cargo run --release -p cpg-cli -- mcp --root ./src
```

Run `cargo run -p cpg-cli -- --help` for build, scan, slice, merge, API census,
export, flow, vectors, workspace, and MCP commands.

## Design and compatibility

[`ARCHITECTURE.md`](ARCHITECTURE.md) describes storage, incrementality,
summaries, and known simplifications. [`PROGRESS.md`](PROGRESS.md) records work
against the pinned Joern oracle. The manual parity CI job runs the large
differential suite; regular pull requests run locked build, test, formatting,
Clippy, audit, archive, and container checks.

Treat benchmark results as measurements of their named fixtures. Run the
examples in `cpg-incremental/examples` on your own corpus before making
capacity decisions.
