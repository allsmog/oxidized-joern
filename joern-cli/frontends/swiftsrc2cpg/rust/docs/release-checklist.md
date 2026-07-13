# SwiftAstGen Rust Release Checklist

Use this checklist before publishing a fork release of the oxidized
`SwiftAstGen` binary used by `swiftsrc2cpg`. It mirrors the gosrc2cpg playbook
adapted to this frontend's specifics.

## Version Alignment

Three values must agree:

- `joern-cli/frontends/swiftsrc2cpg/src/main/resources/application.conf` sets
  `swiftsrc2cpg.astgen_version = "0.4.2"`.
- The Rust crate's `Cargo.toml` version matches that value.
- `SwiftAstGen --version` prints the same version (the Scala
  `AstGenRunner` uses `versionFlag = "--version"` and compares against
  `swiftsrc2cpg.astgen_version`).

`joern-cli/frontends/swiftsrc2cpg/build.sbt` downloads the upstream reference
artifacts from:

```text
https://github.com/joernio/astgen-monorepo/releases/download/swift-astgen/v0.4.2/
```

The release artifacts named in `build.sbt` are:

- `SwiftAstGen-win.exe`
- `SwiftAstGen-linux`
- `SwiftAstGen-linux-arm64`
- `SwiftAstGen-mac`

## Required Local Gates

```bash
cd joern-cli/frontends/swiftsrc2cpg/rust
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

`cargo test` includes the coverage gate (`tests/coverage.rs`) and the
self-skipping `tests/differential_json.rs`. The differential only runs when the
reference binary is provided (see below); without it the test self-skips.

## Gated Differential

The upstream SwiftSyntax reference is a **macOS binary**. Download
`SwiftAstGen-mac` from the release above and run:

```bash
cd joern-cli/frontends/swiftsrc2cpg/rust
SWIFTASTGEN_REFERENCE=/path/to/SwiftAstGen-mac \
  cargo test --test differential_json -- --nocapture
```

The harness normalizes a small set of documented divergences (see
`docs/json-contract.md`) and the checked-in fixture corpus is expected to pass
without mismatches. If a mismatch appears, classify it as one of:

- A Rust JSON compatibility bug.
- An intentional, CPG-irrelevant divergence documented in
  `docs/json-contract.md`.
- A reference identity difference needing a normalizer update with a fixture.

## Scala Integration

Build the local Rust binary into the frontend's `bin/astgen` directory and run
the frontend test suite from the repository root:

```bash
sbt 'swiftsrc2cpg/scalafmtCheck' 'swiftsrc2cpg/swiftAstGenBuildRust' 'swiftsrc2cpg/test'
```

`swiftAstGenBuildRust` runs `cargo build --release --bin SwiftAstGen` and copies
the host artifact into `bin/astgen`. The Scala runner invokes it as
`SwiftAstGen -o <out> [--exclude-regex <regex>]` from the source directory.

## CI

The job `swift-differential` in `.github/workflows/oxidized-astgen.yml` runs on
`macos-14`, downloads `SwiftAstGen-mac` from the release tag above, and runs the
differential with `SWIFTASTGEN_REFERENCE` set. It is `continue-on-error: true`
(informational until byte-identity). The shared `rust-gates` matrix job also
runs `cargo fmt --check`, `cargo test`, and `cargo clippy` for the
`swiftsrc2cpg` crate on `ubuntu-latest`.
