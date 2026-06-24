# astgen (JavaScript) Rust Release Checklist

Use this checklist before publishing a fork release of the oxidized JavaScript
`astgen` binary used by `jssrc2cpg`. It mirrors the gosrc2cpg playbook adapted
to this frontend's specifics.

## Version Alignment

Three values must agree:

- `joern-cli/frontends/jssrc2cpg/src/main/resources/application.conf` sets
  `jssrc2cpg.astgen_version = "3.46.0"`.
- The Rust crate's `Cargo.toml` version matches that value.
- `astgen --version` prints the same version (the Scala `AstGenRunner` compares
  against `jssrc2cpg.astgen_version`).

`joern-cli/frontends/jssrc2cpg/build.sbt` downloads the upstream reference
artifacts from:

```text
https://github.com/joernio/astgen-monorepo/releases/download/javascript-astgen/v3.46.0/
```

The release artifacts named in `build.sbt` are:

- `astgen-win.exe`
- `astgen-linux`
- `astgen-linux-arm`
- `astgen-macos`
- `astgen-macos-arm`

The **upstream reference** (`@joernio/astgen`) is **Node-based** and needs a
Node runtime; the oxidized crate compiles to native platform binaries with the
same names.

## Required Local Gates

```bash
cd joern-cli/frontends/jssrc2cpg/rust
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

`cargo test` includes the coverage gate (`tests/coverage.rs`) and the
self-skipping `tests/differential_json.rs`.

## Gated Differential

The reference is the Node-based `astgen` binary from the release above (so a
Node runtime must be installed). Download it and run:

```bash
cd joern-cli/frontends/jssrc2cpg/rust
JSASTGEN_REFERENCE=/path/to/astgen \
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
sbt 'jssrc2cpg/scalafmtCheck' 'jssrc2cpg/jsAstGenBuildRust' 'jssrc2cpg/test'
```

`jsAstGenBuildRust` runs `cargo build --release --bin astgen` and installs the
host artifact under `bin/astgen`. The Scala `AstGenRunner` invokes it as
`astgen -t ts -o <out>` for JS/TS (and `-t vue` for Vue files).

## CI

The job `jssrc2cpg-differential` in `.github/workflows/oxidized-astgen.yml`
runs on `ubuntu-latest`, provisions Node 20 (the reference is Node-based),
downloads `astgen-linux` from the release tag above, and runs the differential
with `JSASTGEN_REFERENCE` set. It is `continue-on-error: true` (informational
until byte-identity). The shared `rust-gates` matrix job also runs
`cargo fmt --check`, `cargo test`, and `cargo clippy` for the `jssrc2cpg` crate.
