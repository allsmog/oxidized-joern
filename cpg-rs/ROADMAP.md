# Roadmap after native frontend convergence

The Rust-native `0.1.x` line has release-blocking production-preview contracts
for the workflows in [`COMPATIBILITY.md`](COMPATIBILITY.md). C passes the exact
oracle; eight additional frontends combine normalized live differentials with
two real projects and labeled security outcomes apiece.

This roadmap is for expanding capability and confidence. None of these items
requires restoring Scala implementation code, Maven, a JVM runtime, or the
Joern Scala console.

## 1. Expand C language confidence

- Add corpus slices for complex conditional preprocessing, nested macro
  expansion, token pasting/stringizing, variadic macros, and include-path
  behavior.
- Add compiler-informed type, alias, and points-to facts where measured
  security outcomes justify the precision cost.
- Grow the pinned real-project set beyond zlib and Lua, with immutable source
  hashes, deterministic outputs, and per-project resource ceilings.
- Add more labeled vulnerability corpora so scanner precision/recall is
  measured by rule family instead of inferred from graph parity.

## 2. Grow the native rule and query surface

- Expand the built-in C rule packs and version their source/sink/sanitizer
  models with regression fixtures.
- Add the query operations required by real users, while keeping the native
  CLI/JSON/MCP APIs explicit instead of promising CPGQL source compatibility.
- Improve alias-aware witness paths and cross-global/cross-container precision
  where the current conservative model creates confirmed false positives.
- Benchmark build, load, flow, and scan against larger named repositories on
  fixed CI hardware.

## 3. Expand promoted-language confidence

C++, Go, Java, JavaScript, TypeScript, Python, Ruby, and Rust have completed
the current promotion ladder:

1. pin a representative language-specific corpus and oracle;
2. qualify namespaces, packages, types, imports, overloads, and dispatch;
3. add labeled positive/negative scanner outcomes;
4. add at least two immutable real-project fixtures with performance budgets;
5. publish the resulting workflow boundary in `COMPATIBILITY.md`.

Future work grows those corpora and moves from normalized probes toward wider
whole-graph comparisons. Scala remains Experimental until an independently
maintained source oracle exists; shared smoke coverage alone does not promote
it.

## 4. Optional interoperability

- Extend the current Joern-v4 Flatgraph import/export corpus when a real
  migration workflow needs another language, property, or overlay.
- Add network transport around the existing stdio/MCP surfaces only when a
  multi-client deployment requires it.
- Consider additional frontends only with a named user and acceptance corpus.

## Deliberate non-goals

- Restoring the Scala implementation, Maven build, or JVM runtime.
- Recreating Joern's Scala console, full CPGQL semantics, or JVM plugin model.
- Claiming full Joern parity from a finite C corpus.
- Promoting a parser without language-specific oracle, real-project, and
  security evidence.
- Matching internal overlays that do not affect documented output or findings.
