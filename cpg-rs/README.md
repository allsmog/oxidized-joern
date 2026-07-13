# cpg-rs

A from-scratch, **incremental** Code Property Graph engine in Rust — the
rearchitecture worked out in the discussion that precedes it: take the storage
and dataflow ideas that make [Joern](https://github.com/joernio/joern) fast and
the language-contract abstractions that make
[Fraunhofer's CPG](https://github.com/Fraunhofer-AISEC/cpg) clean, and add the
thing neither has — incrementality as a core invariant.

```bash
cd cpg-rs
cargo test                                              # all crates
cargo run --release -p cpg-incremental --example scale  # 1M-LOC benchmark

cargo run --release -p cpg-cli -- build src/ -o p.cpg --lang c   # build + persist
cargo run --release -p cpg-cli -- serve --load p.cpg             # reopen + query
```

## What it does

- **Parses seven languages** — C, Java, Go, JavaScript, Ruby, Rust, Python
  (tree-sitter, tolerant of uncompilable code) — into one language-independent
  graph. Java/Go/JS/Ruby/Rust/Python all run through a *single* generic engine
  (`cpg-lang-ts`, ~350 lines) driven by a per-language spec that is a struct
  literal; adding a language is writing data, not a frontend. All seven pass the
  identical conformance suite and the shared dataflow/taint engine, unchanged.
- **Analyses** control flow, symbol/call resolution, and summary-first
  interprocedural dataflow, then answers **source→sink taint queries with
  witness paths**.
- **Updates incrementally**: editing one file of a 2M-LOC project re-analyses
  that file and ~26 affected summaries in ~100 ms — work tracks the change, not
  the codebase.
- **Persists** the columnar graph to disk and reopens it without reparsing.

## Numbers (1M LOC / 200k functions, 4 cores)

| | |
|---|---|
| Full build (parse + analyse) | ~5.6 s (~35k functions/sec) |
| One-file incremental edit | ~50 ms |
| On disk | ~184 MB (~53 bytes/node) |
| Reload (no parsing) | ~1.4 s |

See [ARCHITECTURE.md](ARCHITECTURE.md) for the design, the rationale behind each
choice, and the honest list of what's simplified and what's next.

## CI

GitHub Actions (`.github/workflows/cpg-rs.yml`) runs on any push or PR that
touches `cpg-rs/`:

- **test** — `cargo build` + `cargo test` for the whole workspace
  (`--locked`, stable toolchain, cargo registry and target dir cached).
- **fmt-clippy** — `cargo fmt --check` and `cargo clippy --workspace -- -D
  warnings`. Currently advisory (`continue-on-error`) until the existing
  formatting/lint debt is paid down.

The **parity** job runs the differential oracle check
(`joern-parity/setup-oracle.sh` then `joern-parity/check.sh`) against the
pinned Joern release (v4.0.555, set in both the workflow and
`setup-oracle.sh`). Because the oracle download is ~2GB it does not run on
every push — trigger it manually from the Actions tab ("Run workflow" /
`workflow_dispatch`); the unpacked distribution is cached keyed on the pinned
version. To bump the oracle, change `JOERN_VERSION` in both places and record
the upgrade in PROGRESS.md.
