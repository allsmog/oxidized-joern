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
