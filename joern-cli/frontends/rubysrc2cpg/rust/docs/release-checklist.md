# rubyastgen Rust Release Checklist

Use this checklist before publishing a fork release of the oxidized `rubyastgen`
binary used by `rubysrc2cpg`. It mirrors the gosrc2cpg playbook adapted to this
frontend's specifics.

## Version Alignment

Three values must agree:

- `joern-cli/frontends/rubysrc2cpg/src/main/resources/application.conf` sets
  `rubysrc2cpg.rubyastgen_version = "0.1.0"`.
- The Rust crate's `Cargo.toml` version matches that value.
- `rubyastgen --version` prints the same version (the Scala `RubyAstGenRunner`
  compares against `rubysrc2cpg.rubyastgen_version`).

There is **no download URL** in `build.sbt`: the oxidized track builds the Rust
`rubyastgen` binary locally rather than fetching an artifact. The historical
upstream reference (`ruby_ast_gen`, the astgen-monorepo `ruby-astgen` release)
is a **Ruby gem wrapped around the `parser` gem** — published as a gem ZIP, run
through **JRuby**, and **not** a standalone native binary. It is no longer wired
into `build.sbt`. The astgen version is written to the JAR manifest as
`Ruby-AstGen-Version`.

The Rust binaries produced by `rubyAstGenBuildRust` are native executables:
`rubyastgen-win.exe`, `rubyastgen-linux`, `rubyastgen-linux-arm`,
`rubyastgen-macos`, `rubyastgen-macos-arm`.

## Required Local Gates

```bash
cd joern-cli/frontends/rubysrc2cpg/rust
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

`cargo test` includes the coverage gate (`tests/coverage.rs`) and the
self-skipping `tests/differential_json.rs`.

## Gated Differential

There is no native reference binary, so the differential is **local only** and
self-skips by default. To run it, point `RUBYASTGEN_REFERENCE` at a reference
that honours the positional `<input> <output>` interface — typically a JRuby
wrapper around the `ruby_ast_gen` gem, or another `rubyastgen` revision:

```bash
cd joern-cli/frontends/rubysrc2cpg/rust
RUBYASTGEN_REFERENCE=/path/to/ruby_ast_gen-jruby-wrapper \
  cargo test --test differential_json -- --nocapture
```

When `RUBYASTGEN_REFERENCE` is unset the harness prints
`skipping differential JSON test; set RUBYASTGEN_REFERENCE` and passes. Each
mismatch must be classified as a Rust JSON bug, an intentional divergence
documented in `docs/json-contract.md`, or a reference identity difference.

## Scala Integration

Build the local Rust binary into `bin/astgen` and run the frontend tests from
the repository root:

```bash
sbt 'rubysrc2cpg/scalafmtCheck' 'rubysrc2cpg/rubyAstGenBuildRust' 'rubysrc2cpg/test'
```

`rubyAstGenBuildRust` runs `cargo build --release --bin rubyastgen` and installs
the host artifact under `bin/astgen`. The Scala `RubyAstGenRunner` invokes it
with positional arguments: `rubyastgen <input> <output>` (plus any exclude
arguments).

## CI

There is **no CI differential job** for `rubysrc2cpg` by design — the reference
is a JRuby gem, not an executable, so there is nothing to download in CI; run
the differential locally with `RUBYASTGEN_REFERENCE`. The shared `rust-gates`
matrix job in `.github/workflows/oxidized-astgen.yml` still runs
`cargo fmt --check`, `cargo test` (including the zero-unmapped coverage gate),
and `cargo clippy` for the `rubysrc2cpg` crate on `ubuntu-latest`.
