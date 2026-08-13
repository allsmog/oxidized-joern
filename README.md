# Oxidized Joern

Oxidized Joern is a Rust-native code property graph CLI for repository
analysis. The public release contains one executable, `cpg`. It can build and
save graphs, run source-to-sink analysis, produce SARIF, export graph views,
serve saved graphs, and expose an MCP server.

This project is an independent fork of
[Joern](https://github.com/joernio/joern). It is not an official Joern
distribution.

## Release status

The current `0.1.x` line is a preview. It is useful for evaluation and
development, but it is not yet a drop-in Joern replacement.

The CLI accepts C, C++, Go, Java, JavaScript, TypeScript, Python, Ruby, Rust,
and Scala source. Language support currently shares a tree-sitter-based graph
builder, so coverage and type resolution vary by language. The differential
test harness has exact output parity for its standalone C fixture path. That
validated path has not yet been integrated into every production engine path
or extended to every language.

See [`cpg-rs/README.md`](cpg-rs/README.md) for commands and compatibility
details.

## Install a preview release

Linux and macOS:

```bash
curl --fail --location --remote-name \
  https://github.com/allsmog/oxidized-joern/releases/latest/download/cpg-install.sh
sh cpg-install.sh
```

The installer selects the host archive, verifies its SHA-256 checksum, and
places `cpg` under `~/.local/bin` by default. Pass `--help` to see version and
path options. Windows users can download the release archive directly
from the repository's
[Releases](https://github.com/allsmog/oxidized-joern/releases) page.

The preview container contains the same Rust binary:

```bash
docker run --rm -v "$PWD:/workspace" -w /workspace \
  ghcr.io/allsmog/oxidized-joern:preview --version
```

## Build from source

Requirements: Rust 1.97.0 and Cargo.

```bash
git clone https://github.com/allsmog/oxidized-joern.git
cd oxidized-joern/cpg-rs
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --locked -p cpg-cli
./target/release/cpg --version
```

Build a graph and run a scan:

```bash
./target/release/cpg build ../path/to/source -o project.cpg --lang rust
./target/release/cpg scan --load project.cpg --lang rust -o findings.sarif
```

Run `./target/release/cpg --help` for the complete command list.

## Release integrity

Tagged releases publish native archives for Linux and macOS on x86-64 and
ARM64, plus Windows on x86-64. Each archive has a SHA-256 checksum and GitHub
build attestations. The multi-architecture container includes attestations and an
SBOM.

Only the Rust-native `cpg` executable is packaged. Other source retained in
the repository is migration and reference material, not part of the binary
release.

## Contributing and security

Start with [`CONTRIBUTING.md`](CONTRIBUTING.md). Report vulnerabilities through
the private path in [`SECURITY.md`](SECURITY.md), not a public issue.

This fork retains upstream attribution under the Apache License 2.0. New Rust
work uses the same license. See [`LICENSE`](LICENSE) and
[`CITATION.cff`](CITATION.cff).
