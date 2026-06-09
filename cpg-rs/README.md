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

- **Parses** C and Python (tree-sitter, tolerant of uncompilable code) into one
  language-independent graph. Each frontend is ~300 lines of grammar mapping;
  everything else is shared.
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
