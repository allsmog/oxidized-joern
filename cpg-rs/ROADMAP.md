# ROADMAP — what is left to finish the Rust migration

This document is the gap analysis between what `cpg-rs/` is today and a Rust
engine that actually replaces Joern for our workloads. It complements
`GOAL.md` (the rules of the parity effort), `PROGRESS.md` (session-by-session
state), and `ARCHITECTURE.md` (the design and what is already proven).

## Where we are

Two deliberately separate tracks exist, and **the single biggest remaining
step is converging them**:

- **Track A — the byte-parity Joern port** (`joern-parity/`, ~3.7k LOC).
  A pure-Rust tree-sitter C frontend whose output is byte-identical to Joern
  v4.0.555 on the pinned corpus: full AST (M2), full node set (M3), all 15
  structural edge kinds + CFG (M4), three unmodified musl files (M5,
  in progress), and — as of the latest milestone — **REACHING_DEF byte-parity,
  1,458/1,458 flow facts** (M7 Track B). The reaching-def engine is a verbatim
  port of the decompiled v4.0.555 internals (ReachingDefFlowGraph, gen/kill,
  EdgeValidator + DefaultSemantics operator flow table, UsageAnalyzer.isUsing).
  This track knows *what Joern actually computes*, pinned by an oracle.
  It is, however, a standalone text dumper — it does not build a `cpg-core`
  graph.

- **Track B — the greenfield incremental engine** (`cpg-core`, `cpg-frontend`,
  `cpg-lang-*`, `cpg-analysis`, `cpg-incremental`, `cpg-cli`, `conformance`;
  ~4.5k LOC). Columnar store + interning, file-partitioned incremental
  delete/rebuild, layer-declaring passes, summaries-first dataflow with a
  precise invalidation web, persistence, seven languages through one generic
  tree-sitter engine, a JSON-over-stdio query server, and a 1M-LOC benchmark
  (~5.6s cold build, ~50ms incremental edit). This track has the *right
  architecture*, but its analyses are placeholders by design: the CFG pass is
  a source-order linearisation (`cpg-analysis/src/cfg.rs`), symbol resolution
  is intra-method by name, taint is name-based, and method full-names are bare
  names.

Track A has correctness without the architecture; Track B has the
architecture without correctness. Finishing the migration = grafting A's
validated algorithms into B's engine, then closing the surface-area gaps
listed below.

## What "finished" means

Three tiers, in order of ambition. Be explicit about which one is the target
at any given time:

1. **Tier 1 — self-hosted analysis engine (the IRIS target).** cpg-rs can
   build a precise CPG for C (later Java/Python/JS), answer
   `reachableBy`-class taint queries with witness paths and Joern-equivalent
   dataflow semantics, incrementally, behind the query server, with SARIF
   output. Joern is no longer needed at runtime — only as a test oracle.
2. **Tier 2 — Joern replacement for our workflows.** Everything Tier 1, plus
   a scan/rule layer (querydb equivalent), multi-language dataflow parity,
   and CPG export loadable by Joern tooling for migration-period
   interoperability.
3. **Tier 3 — full Joern parity.** All 13 frontends, CPGQL surface
   compatibility, console/workspace UX, export formats. This is explicitly
   **not** the goal; see Non-goals.

The rest of this document is the work list for Tiers 1–2.

## Gap 1 — Converge the tracks (milestone M6, the critical path)

`joern-parity` proved the algorithms; `cpg-core` is where they must live.
This is the highest-leverage remaining work and everything else stacks on it.

- [ ] **Fold the parity C frontend onto `cpg-core::builder`.** The dumper in
  `joern-parity/src/main.rs` builds its own node records; re-target it to
  emit through the shared builder so the same code produces both the
  canonical dump (the parity gate stays!) and a real columnar graph. The
  dump becomes a serializer over the graph, per GOAL.md M6.
- [x] **Replace the placeholder `CfgPass`** (`cpg-analysis/src/cfg.rs`,
  source-order linearisation) with the parity-validated CfgBuilder semantics
  (evaluation-order chaining, transparent statement blocks, loop/switch/
  short-circuit shapes — all already reconstructed and green in Track A).
  *(2026-07-09: done — see cpg-analysis/src/cfg.rs; the C frontend now emits
  a canonical control-structure shape the builder relies on.)*
- [x] **Populate the empty DDG edge slot** with the validated reaching-def
  engine (`reaching_def_flows` in joern-parity): ReachingDefFlowGraph, the
  gen/kill fixpoint, EdgeValidator + the DefaultSemantics operator table,
  UsageAnalyzer.isUsing. This upgrades `cpg-analysis` from name-based taint
  to Joern-grade def-use.
  *(2026-07-09: done — cpg-analysis/src/reaching_def.rs writes
  EdgeKind::ReachingDef edges; paramOut routing and `<global>` capture
  linking are N/A in the simplified schema, divergences documented in the
  module docs. Follow-up: point summaries/taint at the new edges.)*
- [ ] **Keep the byte-parity gate wired through the new path.** `check.sh`
  must diff the graph-backed dump, not a parallel legacy code path;
  otherwise the gate silently stops guarding the engine.
- [ ] **CPG binary export** (flatgraph zip or proto `cpg.bin`) validated by
  `joern --script` loading it. This is the Tier-2 interoperability seam and
  also an independent correctness check.

## Gap 2 — Dataflow query layer (reachableBy and richer summaries)

What exists: REACHING_DEF byte-parity (intraprocedural, Track A) and
name-based summaries-first taint with witness paths (Track B). What's
missing to answer real security queries:

- [ ] **`reachableBy` = demand-driven transitive closure over REACHING_DEF**,
  lifted interprocedurally through summaries. Validate against a Joern
  `reachableBy` probe on the corpus (the oracle machinery from Track A
  extends naturally — this is M7 step (b) in PROGRESS.md).
- [ ] **Widen the summary payload.** `FunctionSummary`/`Flow` is
  `{Param(i)|Return} → {Param(i)|Return}` — too poor to express sanitisation,
  field/container flow, or taint kinds. Extend `Flow` with a label/sanitizer
  slot and teach both `expr_taint` walkers in `cpg-analysis/src/taint.rs` to
  respect it. Without this, sanitizer-aware analysis (and any useful
  LLM-adjudicated summary tier) has nowhere to store its result.
- [ ] **Deterministic summary computation as an invariant.** The
  `recompute` fixpoint detects convergence by `prev.flows != new.flows`; any
  non-deterministic summary source (an LLM tier, a timeout-bounded solver)
  must be memoised per (function-hash, callee-summary-state) within a
  fixpoint or convergence detection breaks silently.
- [ ] **Provenance on findings.** `Trace` carries origin + witness steps;
  add per-step provenance (which pass/summary/model produced the edge) so a
  finding is auditable — a must once any non-computed summary tier (external
  JSON today, LLM later) can influence results.
- [ ] **Cross-callee witness paths.** Today the witness shows the call hop,
  not the callee's internal path; stitch callee-internal traces through the
  summary boundary.
- [ ] **External summary corpus.** The JSON external-summary loader exists;
  actually populate it for libc / common stdlib surface (Joern ships
  DefaultSemantics + `semantics` files to crib from).

## Gap 3 — C frontend completeness (finish M5)

The parity corpus pins most of C, but the real-world long tail is open.
From PROGRESS.md, still unpinned:

- [ ] `#if`/`#elif` expression evaluation; nested macro expansion; token
  pasting/stringizing; varargs macros (next corpus target: zlib `adler32.c`).
- [ ] `extern`, calls to undefined functions (printf stub shape),
  initializer lists `{1,2}`, struct defs inside functions, braceless
  if/while bodies.
- [ ] **Two real projects at zero diffs** (GOAL.md M5 exit criterion:
  e.g. zlib + lua, vendored/pinned). Every diff becomes a corpus case or a
  QUIRKS.md entry.
- [ ] Header/include resolution strategy (today `<includes>:<global>` and
  IS_EXTERNAL type stubs are pinned; real projects need a decision on
  include-path handling vs. fuzzy parsing, matching c2cpg behaviour).

## Gap 4 — Language coverage beyond C

The pinned upstream Joern reference ships 13 frontends: c2cpg,
csharpsrc2cpg, ghidra2cpg, gosrc2cpg, javasrc2cpg, jimple2cpg, jssrc2cpg,
kotlin2cpg, php2cpg, pysrc2cpg, rubysrc2cpg, swiftsrc2cpg, x2cpg (plus
rust2cpg landing upstream). cpg-rs has seven languages at
*conformance-suite* fidelity — structurally sound graphs, but:

- [ ] **Method full-names are bare names** — no namespace/package
  qualification, so cross-file call resolution is name-collision-prone.
  This is the single biggest precision gap in the generic engine.
- [ ] **No type resolution** in the generic-engine languages (and Python/JS
  name resolution is the honest long pole — Joern spends most of its
  frontend effort exactly here). Plan: per-language rule packs behind the
  `LanguageTraits` capability model, starting with Java (static types,
  cheapest win) then Python (MRO/imports).
- [ ] **Per-language parity ladders (M8+).** GOAL.md's rule stands: one
  language at a time, own corpus, own oracle diff, don't start the next
  while the current gate is red. Suggested order after C: Java (IRIS
  benchmark relevance), then JavaScript, then Python.
- [ ] **Scope decision needed:** which of the 13 frontends are actually
  required? Recommendation: C/C++, Java, JS/TS, Python, Go = Tier 2 scope;
  Kotlin/Swift/C#/PHP/Ruby/Ghidra/Jimple only on demand. (C++ is a real
  question mark: tree-sitter-cpp vs CDT parity is substantially harder than
  C — budget it separately, don't let it block Tier 1.)

## Gap 5 — Query and rule surface

The server (`cpg-cli serve`) speaks six JSON commands (stats, methods,
calls, summary, taint, update). Joern's users live in CPGQL + joern-scan.
To finish the migration for humans, not just for IRIS:

- [ ] **A scan/rule layer** (querydb equivalent): named rules =
  parameterised taint/query specs (sources, sinks, sanitizers, CWE metadata)
  loaded from declarative files; `cpg scan` runs the pack and emits SARIF.
  Because summaries are incrementally maintained, this is naturally
  "scan as subscription" — findings deltas after each `update`.
- [ ] **SARIF output** (also required by the IRIS loop, Gap 7).
- [ ] **Grow the query vocabulary** toward the CPGQL step set that our
  scripts actually use (method/call/parameter/literal filters, AST/CFG/DDG
  neighbours, `reachableBy`). Full CPGQL compatibility is a non-goal
  (traversal-order semantics make it a tarpit); a documented migration map
  "CPGQL step → server query" is the pragmatic substitute.
- [ ] **TCP/HTTP transport** around the existing request loop (stdio is fine
  for tooling, not for a daemon shared by editors/CI).
- [ ] **Export formats** on demand: graphml/dot via a small serializer over
  the columnar graph (cheap, unblocks graph tooling users).

## Gap 6 — Engine hardening (from ARCHITECTURE.md "what's next")

None of these block correctness; all block "replace Joern at scale":

- [ ] Parallelise the serial `absorb` merge (~1.4s of the 5.6s 1M-LOC
  build) and shard the per-method passes.
- [ ] `freeze()` to CSR for quiescent read-mostly graphs + on-disk
  compression (persistence is uncompressed, ~53 bytes/node).
- [ ] Persist summaries (today recomputed on every `--load`).
- [ ] Crash-safety/versioning for the persistence format (schema version
  header, checksums) before anyone trusts `.cpg` files across upgrades.
- [ ] Memory/perf benchmarks vs Joern on identical real corpora (linux,
  zlib, a large Java project) — the migration's headline claim needs
  numbers against the incumbent, not only synthetic scale runs.

## Gap 7 — The IRIS loop (the reason Tier 1 exists)

From PROGRESS.md M7 steps (d)–(e):

- [ ] `cpg-cli` IRIS driver: LLM proposes CWE-specific
  sources/sinks/sanitizers (extend `TaintSpec`), engine runs taint, LLM
  triages findings, SARIF out.
- [ ] LLM-adjudicated summary tier: wrap `compute_method`, cache verdicts
  by function content-hash in the existing external-summary slot, subject
  to the determinism + provenance requirements in Gap 2.
- [ ] Evaluation on a Juliet C/C++ subset: precision/recall engine-alone vs
  engine+LLM. This is the acceptance test for Tier 1.

## Gap 8 — Infrastructure

- [ ] **CI**: run `cargo test --workspace` + `joern-parity/check.sh` on
  every push (today the gate is enforced only by session discipline).
  Cache the pinned Joern oracle (~2GB download per fresh environment) and
  the tree-sitter build artifacts.
- [ ] **Oracle pinning**: `setup-oracle.sh` fetches *latest* Joern; pin to
  v4.0.555 explicitly so a release can't silently shift the spec mid-work
  (drift becomes a deliberate, recorded upgrade per GOAL.md).
- [ ] **Branch hygiene**: the cpg-rs work lives on a branch with an
  unrelated history to `master` (upstream Joern). Decide whether cpg-rs
  should move to its own repository or be merged as a subtree; the current
  split makes the work invisible from the default branch.

## Suggested sequencing

Dependencies, not dates:

1. **M6 convergence** (Gap 1) — everything else builds on the engine
   carrying validated algorithms. Do it while the parity code is fresh.
2. **reachableBy + SARIF + rule specs** (Gaps 2, 5-partial) — the minimum
   query surface for IRIS.
3. **IRIS driver + Juliet eval** (Gap 7) — Tier 1 acceptance.
4. **M5 completion on zlib/lua** (Gap 3) in parallel with 2–3; it's
   corpus-grind, parallelisable across sessions.
5. **Java ladder + FQN/type resolution** (Gap 4) — first non-C language to
   real fidelity.
6. **Hardening + scan layer + export** (Gaps 5–6) — Tier 2.

## Non-goals (explicit cut lines)

- Full CPGQL surface compatibility and the Scala console/REPL/workspace UX.
- Frontends beyond the Tier-2 five, until demanded.
- Overlay-for-overlay parity of Joern internals not observable in output
  (the oracle defines the spec; internals only need to match where the
  bytes say so).
- Points-to at high context sensitivity as pure infrastructure — revisit
  only if IRIS evaluation shows summary-based taint is the precision
  bottleneck.
