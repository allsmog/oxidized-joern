# CPG conformance

The Rust conformance suite checks language-independent graph properties across
frontends. Each language supplies source for the same cases, and the assertions
operate on the shared `cpg-core` graph.

Run it as part of the main workspace:

```bash
cargo test --manifest-path cpg-rs/Cargo.toml --locked -p conformance
```

The current cases cover method parameters, calls and arguments,
intraprocedural call resolution, nested calls, calls inside branches, and
multiple top-level methods. Add a fixture for every supported frontend before
claiming conformance for that language.

The [first Astra sprint report, 2026-09-08](astra-sprint-2026-09-08.md) records a
live Joern baseline, two C fixes, independent review findings, integrated
validation, and the remaining limits of those measurements. The
[C differential checker](../../cpg-rs/joern-parity/README.md) documents its
selected graph projection and strict live-oracle mode.

The [second Astra batch](astra-second-batch-2026-09-08.md) records callable
declarations, loops and conditions, preprocessing, direct findings, and
whole-project comparisons with the same pinned Joern. Its
[metrics](astra-second-batch-metrics.json) retain complete projection counts,
input and binary provenance, and any regressions in previously exact methods.

The [third Astra batch](astra-third-batch-2026-09-08.md) records primitive and
literal types, static modifiers, standalone blocks, definition identities,
and concatenated strings. Its [metrics](astra-third-batch-metrics.json)
compare both the previous checkpoint and the combined build with fresh Joern
outputs; the [acceptance receipt](astra-third-batch-acceptance.json) binds
validation logs, binaries, fixture measurements, and real-project workflows.
