# pyastgen Rust Release Checklist

Use this checklist before publishing a fork release of the oxidized `pyastgen`
binary used by `pysrc2cpg`. It mirrors the gosrc2cpg playbook adapted to this
frontend's specifics.

> Note: `pysrc2cpg`'s **production** parser is the in-tree JavaCC grammar
> (`pythonGrammar.jj` → `io.joern.pythonparser`), driven from the JVM — there is
> **no separate upstream binary** to differential against. The oxidized Rust
> `pyastgen` crate is an alternative backend selectable via
> `Py2CpgOnFileSystem`'s `PythonParserBackend` (`JavaCc` vs `Oxidized`). Because
> the only reference is the in-tree JavaCC parser, the cross-binary differential
> is **N/A** and the **coverage gate is the standing parity guard**.

## Version Alignment

- `joern-cli/frontends/pysrc2cpg/src/main/resources/application.conf` sets
  `pysrc2cpg.pyastgen_version = "0.1.0"`.
- The Rust crate's `Cargo.toml` version matches that value.
- `pyastgen -version` prints the same version (the Scala `PyAstGenRunner` uses
  `versionFlag = "-version"`).

There is **no astgen download URL** in `build.sbt`. The JavaCC parser is
compiled in-tree by the `javaCCTask` SBT task; the Rust `pyastgen` binary is
built locally by `pyAstGenBuildRust` (a compile-time dependency), not downloaded.

## Required Local Gates

```bash
cd joern-cli/frontends/pysrc2cpg/rust
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

`cargo test` runs `tests/cli_contract.rs`, the coverage gate
(`tests/coverage.rs`), and the self-skipping `tests/differential_json.rs`.

## Differential (N/A — coverage gate only)

There is no separate upstream reference binary, so the differential self-skips
by design. The harness does accept a reference via `PYASTGEN_REFERENCE` (and a
real-corpus override via `PYASTGEN_REAL_CORPUS`) if you want to diff against
another `pyastgen` revision locally:

```bash
cd joern-cli/frontends/pysrc2cpg/rust
PYASTGEN_REFERENCE=/path/to/other-pyastgen \
  cargo test --test differential_json -- --nocapture
```

The primary correctness gate is `tests/coverage.rs`, which fails if any node
`kind` is an error/unknown marker (`Unknown`, `Unmapped`, `Unsupported`,
`Error`, `Invalid`, `NotHandled`, `Placeholder`) and asserts presence of the
expected Python construct kinds.

## Scala Integration

Build the local Rust binary into `bin/astgen` and run the frontend tests from
the repository root:

```bash
sbt 'pysrc2cpg/scalafmtCheck' 'pysrc2cpg/pyAstGenBuildRust' 'pysrc2cpg/test'
```

`pyAstGenBuildRust` builds the Rust crate and installs the host artifact under
`bin/astgen`. The Scala `PyAstGenRunner` invokes it as
`pyastgen -out <dir> <input>` when the `Oxidized` backend is selected; the
default `JavaCc` backend parses Python source directly via
`io.joern.pythonparser.PythonParser` (no JSON, no external process).

## CI

There is **no CI differential job** for `pysrc2cpg` by design — the reference is
the in-tree JavaCC parser, so there is nothing external to differential against.
The shared `rust-gates` matrix job in `.github/workflows/oxidized-astgen.yml`
runs `cargo fmt --check`, `cargo test` (including the coverage gate), and
`cargo clippy` for the `pysrc2cpg` crate on `ubuntu-latest`.
