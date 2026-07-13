# dotnetastgen Rust Release Checklist

Use this checklist before publishing a fork release of the oxidized
`dotnetastgen` binary used by `csharpsrc2cpg`. It mirrors the gosrc2cpg playbook
adapted to this frontend's specifics.

## Version Alignment

Three values must agree:

- `joern-cli/frontends/csharpsrc2cpg/src/main/resources/application.conf` sets
  `csharpsrc2cpg.dotnetastgen_version = "0.43.0"`.
- The Rust crate's `Cargo.toml` version matches that value.
- `dotnetastgen --version` prints the same version (the Scala
  `DotNetAstGenRunner` compares against `csharpsrc2cpg.dotnetastgen_version`).

`joern-cli/frontends/csharpsrc2cpg/build.sbt` downloads the upstream reference
artifacts from:

```text
https://github.com/joernio/astgen-monorepo/releases/download/dotnet-astgen/v0.43.0/
```

The release artifacts named in `build.sbt` are:

- `dotnetastgen-win.exe`
- `dotnetastgen-linux`
- `dotnetastgen-linux-arm64`
- `dotnetastgen-macos`

## Required Local Gates

```bash
cd joern-cli/frontends/csharpsrc2cpg/rust
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

`cargo test` includes the coverage gate (`tests/coverage.rs`) and the
self-skipping `tests/differential_json.rs` (it self-skips without a reference).

## Gated Differential

The upstream reference is the native `dotnetastgen-linux` binary from the
release above. Download it and run:

```bash
cd joern-cli/frontends/csharpsrc2cpg/rust
DOTNETASTGEN_REFERENCE=/path/to/dotnetastgen-linux \
  cargo test --test differential_json -- --nocapture
```

Each mismatch must be classified as one of:

- A Rust JSON compatibility bug.
- An intentional, CPG-irrelevant divergence documented in
  `docs/json-contract.md`.
- A reference identity difference needing a normalizer update with a fixture.

## Scala Integration

Build the local Rust binary into `bin/astgen` and run the frontend tests from
the repository root:

```bash
sbt 'csharpsrc2cpg/scalafmtCheck' 'csharpsrc2cpg/dotNetAstGenBuildRust' 'csharpsrc2cpg/test'
```

`dotNetAstGenBuildRust` builds the Rust crate and installs the host artifact
under `bin/astgen`. The Scala `DotNetAstGenRunner` invokes it as
`dotnetastgen -i <in> -o <out>`.

## CI

The job `csharpsrc2cpg-differential` in
`.github/workflows/oxidized-astgen.yml` runs on `ubuntu-latest`, downloads
`dotnetastgen-linux` from the release tag above, and runs the differential with
`DOTNETASTGEN_REFERENCE` set. It is `continue-on-error: true` (informational
until byte-identity). The shared `rust-gates` matrix job also runs
`cargo fmt --check`, `cargo test`, and `cargo clippy` for the `csharpsrc2cpg`
crate.
