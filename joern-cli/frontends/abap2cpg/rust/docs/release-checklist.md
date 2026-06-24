# abapgen Rust Release Checklist

Use this checklist before publishing a fork release of the oxidized `abapgen`
binary used by `abap2cpg`. It mirrors the gosrc2cpg playbook adapted to this
frontend's specifics.

## Version Alignment

Three values must agree:

- `joern-cli/frontends/abap2cpg/src/main/resources/application.conf` sets
  `abap2cpg.abapgen_version = "0.3.0"`.
- The Rust crate's `Cargo.toml` version matches that value.
- `abapgen --version` prints the same version (the Scala `AbapAstGenRunner`
  compares against `abap2cpg.abapgen_version`).

`joern-cli/frontends/abap2cpg/build.sbt` downloads the upstream reference
artifacts from:

```text
https://github.com/joernio/astgen-monorepo/releases/download/abap-astgen/v0.3.0/
```

The release artifacts named in `build.sbt` are:

- `abapgen-win.exe`
- `abapgen-linux`
- `abapgen-linux-arm`
- `abapgen-macos`
- `abapgen-macos-arm`

## Required Local Gates

```bash
cd joern-cli/frontends/abap2cpg/rust
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

`cargo test` includes the coverage gate (`tests/coverage.rs`) and the
self-skipping `tests/differential_json.rs`.

## Gated Differential

The reference is the native `abapgen-linux` binary from the release above.
Download it and run:

```bash
cd joern-cli/frontends/abap2cpg/rust
ABAPASTGEN_REFERENCE=/path/to/abapgen-linux \
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
sbt 'abap2cpg/scalafmtCheck' 'abap2cpg/abapgenBuildRust' 'abap2cpg/test'
```

`abapgenBuildRust` runs `cargo build --release --bin abapgen` and installs the
host artifact under `bin/astgen`. The Scala `AbapAstGenRunner` invokes it with
positional arguments: `abapgen <input> <output>`.

## CI

The job `abap2cpg-differential` in `.github/workflows/oxidized-astgen.yml` runs
on `ubuntu-latest`, downloads `abapgen-linux` from the release tag above, and
runs the differential with `ABAPASTGEN_REFERENCE` set. It is
`continue-on-error: true` (informational until byte-identity). The shared
`rust-gates` matrix job also runs `cargo fmt --check`, `cargo test`, and
`cargo clippy` for the `abap2cpg` crate.
