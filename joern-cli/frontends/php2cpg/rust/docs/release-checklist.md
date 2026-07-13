# phpastgen Rust Release Checklist

Use this checklist before publishing a fork release of the oxidized `phpastgen`
binary used by `php2cpg`. It mirrors the gosrc2cpg playbook adapted to this
frontend's specifics.

## Version Alignment

`php2cpg` does **not** have a `src/main/resources/application.conf` version key.
The reference version is pinned in `project/Versions.scala`:

- `Versions.phpParser = "4.15.10"` (the upstream `joernio/PHP-Parser` release).
- The Rust crate's `Cargo.toml` carries the `phpastgen` binary version
  independently; keep it aligned with releases of the oxidized binary.

`joern-cli/frontends/php2cpg/build.sbt` downloads the upstream reference (a PHP
archive, **not** a native binary) from:

```text
https://github.com/joernio/PHP-Parser/releases/download/v4.15.10/php-parser.phar
```

The oxidized Rust binaries built/installed by `build.sbt` are native
executables (installed under `bin/php-parser`):

- `phpastgen-win.exe`
- `phpastgen-linux`
- `phpastgen-linux-arm`
- `phpastgen-macos`
- `phpastgen-macos-arm`

## Required Local Gates

```bash
cd joern-cli/frontends/php2cpg/rust
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

`cargo test` includes the coverage gate (`tests/coverage.rs`) and the
self-skipping `tests/differential_json.rs`.

## Gated Differential

The reference is the `php-parser.phar` archive, which **requires a PHP runtime**
on `PATH` (the harness executes the `.phar` through `php`; override with the
`PHP_BIN` env var). Download it and run:

```bash
cd joern-cli/frontends/php2cpg/rust
PHPASTGEN_REFERENCE=/path/to/php-parser.phar \
  cargo test --test differential_json -- --nocapture
```

Each mismatch must be classified as one of:

- A Rust JSON compatibility bug.
- An intentional, CPG-irrelevant divergence documented in
  `docs/json-contract.md`.
- A reference identity difference needing a normalizer update with a fixture.

## Scala Integration

Build the local Rust binary and run the frontend tests from the repository root:

```bash
sbt 'php2cpg/scalafmtCheck' 'php2cpg/phpAstGenBuildRust' 'php2cpg/test'
```

`phpAstGenBuildRust` runs `cargo build --release --bin phpastgen` and installs
the host artifact under `bin/php-parser`. The Scala consumer (`PhpParser.scala`)
invokes the parser with `--with-recovery --resolve-names --json-dump`.

## CI

The job `php2cpg-differential` in `.github/workflows/oxidized-astgen.yml` runs
on `ubuntu-latest`, provisions PHP 8.3 (`shivammathur/setup-php`), downloads
`php-parser.phar` from the release tag above, and runs the differential with
`PHPASTGEN_REFERENCE` set. It is `continue-on-error: true` (informational until
byte-identity). The shared `rust-gates` matrix job also runs `cargo fmt
--check`, `cargo test`, and `cargo clippy` for the `php2cpg` crate (no PHP setup
there).
