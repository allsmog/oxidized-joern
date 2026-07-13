# rust_ast_gen Rust Release Checklist

Use this checklist before publishing a fork release of the oxidized
`rust_ast_gen` binary used by `rust2cpg`. It mirrors the gosrc2cpg playbook
adapted to this frontend's specifics.

## Version Alignment

Three values must agree:

- `joern-cli/frontends/rust2cpg/src/main/resources/application.conf` sets
  `rust2cpg.rust_ast_gen_version = "0.8.1"`.
- The Rust crate's `Cargo.toml` version matches that value.
- `rust_ast_gen --version` prints the same version (the Scala
  `RustAstGenRunner` compares against `rust2cpg.rust_ast_gen_version`).

`joern-cli/frontends/rust2cpg/build.sbt` builds the local Rust `rust_ast_gen`
by default. Set `RUST2CPG_ASTGEN_LEGACY=1` to download the upstream reference
artifact for the current host instead. The legacy artifacts come from:

```text
https://github.com/joernio/astgen-monorepo/releases/download/rust-astgen/v0.8.1/
```

The release artifacts named in `build.sbt` are:

- `rust_ast_gen-win.exe`
- `rust_ast_gen-win-arm.exe`
- `rust_ast_gen-linux`
- `rust_ast_gen-linux-arm`
- `rust_ast_gen-macos`
- `rust_ast_gen-macos-arm`

## Required Local Gates

```bash
cd joern-cli/frontends/rust2cpg/rust
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

The HIR-based semantic enrichment loads a sysroot, so the `rust-src` toolchain
component must be installed (`rustup component add rust-src`). `cargo test`
includes the coverage gate (`tests/coverage.rs`) and the self-skipping
`tests/differential_json.rs`.

## Gated Differential

The reference is the native `rust_ast_gen-linux` binary from the release above.
Download it and run:

```bash
cd joern-cli/frontends/rust2cpg/rust
RUSTASTGEN_REFERENCE=/path/to/rust_ast_gen-linux \
  cargo test --test differential_json -- --nocapture
```

Each mismatch must be classified as one of:

- A Rust JSON compatibility bug.
- An intentional, CPG-irrelevant divergence documented in
  `docs/json-contract.md`.
- A reference identity difference needing a normalizer update with a fixture.

## Scala Integration

Provision the Rust binary into `bin/astgen` and run the frontend tests from the
repository root:

```bash
sbt 'rust2cpg/scalafmtCheck' 'rust2cpg/rustAstGenProvision' 'rust2cpg/test'
```

By default, `rustAstGenProvision` runs:

```bash
cargo build --release --bin rust_ast_gen
```

It then installs the host artifact under `bin/astgen`. With
`RUST2CPG_ASTGEN_LEGACY=1`, it installs the downloaded upstream host artifact
instead. The Scala `RustAstGenRunner` invokes it as
`rust_ast_gen -i <in> -o <out>`.

## CI

The job `rust2cpg-differential` in `.github/workflows/oxidized-astgen.yml` runs
on `ubuntu-latest` with the `rust-src` component installed, downloads
`rust_ast_gen-linux` from the release tag above, and runs the differential with
`RUSTASTGEN_REFERENCE` set. This job is gating: any differential mismatch fails
CI. The shared `rust-gates` matrix job also runs `cargo fmt --check`, `cargo
test`, and `cargo clippy` (with `rust-src`) for the `rust2cpg` crate. The
`rust2cpg-scala-integration` job runs `sbt "rust2cpg/test"` against the local
build default.
