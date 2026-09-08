# Second measured Astra parity batch

This batch extends the tested C frontend and direct-flow scanner behavior.
It follows the [first measured batch](astra-sprint-2026-09-08.md), using
`26b5456b2` as the comparison baseline and Joern **v4.0.555** under JDK 21
as the executable reference. The code revision at draft time is
`c5ea712db`. Work began at **06:32 UTC on 2026-09-08**. Integrated acceptance
completed at **08:26:51 UTC**, approximately **115 minutes** after that start;
documentation review and local closeout followed. These results do not establish full Joern compatibility.

The changes remain local on `codex/astra-parity-sprint` in the sibling
`joern-oxidized-astra-sprint` worktree. The original checkout's uncommitted
audit and planning files were preserved. Nothing was pushed or merged into
the original branch.

| Area | Verified change | Retained evidence |
|---|---|---|
| External declarations and callable scope | Declared external signatures and return types survive lowering; direct callees no longer acquire phantom locals. Function-pointer objects and lexical visibility receive separate handling. | [External declarations](../../cpg-rs/joern-parity/corpus/external_declarations.c), [callable scope](../../cpg-rs/joern-parity/corpus/callable_scope.c), [function pointers](../../cpg-rs/joern-parity/corpus/function_pointers.c), and [production graph tests](../../cpg-rs/joern-parity/tests/external_functions.rs). |
| Conditions and loops | Bare identifier conditions use Joern's integer/pointer truth-test forms. Explicit comparisons and negation retain their expression. Braceless loop bodies, omitted `for` clauses, multiple initializers, and unreachable loop tails receive graph and flow coverage. | [Control graph tests](../../cpg-rs/cpg-lang-c/tests/control_truth.rs), [loop scanner tests](../../cpg-rs/cpg-analysis/tests/canonical_c_braceless_loops.rs), and [live corpus](../../cpg-rs/joern-parity/corpus/control_truth_loops.c). |
| Translation-unit preprocessing | Conditional declarations are retained, while only active function bodies contain executable children, matching the pinned oracle. Macro environments follow source order; condition evaluation covers object replacement, `defined`, operators, ternaries, character literals, comments, and line splices. Duplicate definitions retain distinct identities. | [Production graph tests](../../cpg-rs/joern-parity/src/production.rs), [conditional corpus](../../cpg-rs/joern-parity/corpus/conditional_top_level.c), and [scanner/location assertions](../../cpg-rs/cpg-analysis/tests/canonical_c_preprocessor.rs). |
| Direct source-to-sink findings | Canonical reaching definitions preserve possible flows through joins and loops, while definite kills and sanitizer cuts remain covered. The measured outcome suite improves from **12/26 to 26/26**; all 14 baseline mismatches were false negatives. | [Inputs, live oracle, baseline, and replay](../../cpg-rs/cpg-analysis/tests/fixtures/direct-flow/README.md), [measurement](../../cpg-rs/cpg-analysis/tests/fixtures/direct-flow/measurement.json), and [finding/witness tests](../../cpg-rs/cpg-analysis/tests/direct_c_flow.rs). |

The 26 direct-flow outcomes include nine negatives. They establish agreement
on those possible-flow cases, not general path feasibility or scanner
equivalence. Query sanitizers, summary sanitizers, mixed-language graphs,
recursive ancestry, repeated handoffs, and definite kills have additional
assertions in the linked tests.

The committed reference contains **41 C files / 544 source lines**, up from
17 / 268 after the first batch. It has **280 method blocks and 17 section
blocks: 297 comparison blocks**. The reference was generated from live
Joern; expected graph values were not edited to make Rust pass.

| Reference records | Count |
|---|---:|
| Nonempty AST records | 5,186 |
| NODES records | 515 |
| EDGES records | 9,224 |
| FLOWS records | 3,587 |

Reference SHA-256:
`64086e4bfcffc635ed3b008e1e955680a97fd27a1213c1dbd7dc13f9233e0b0f`.
The [checker contract](../../cpg-rs/joern-parity/README.md) defines the selected
properties. Committed and fresh strict-live checks passed **297/297** at
`c5ea712db`. The 33-file corpus in place before the final declaration repairs retained
its complete output through those repairs; the first-batch baseline had 17 files. A separate unfiltered
[562-record array fixture](../../cpg-rs/joern-parity/tests/fixtures/array-declarations/README.md)
keeps global dimensions and local allocation arity distinguishable.

Independent review found and repaired several regressions during the batch:

- Conditional functions initially lacked source anchors, suppressing scanner
  findings. The importer now anchors conditional and identical duplicate
  definitions by source name and occurrence. Active and inactive methods keep
  locations, with findings asserted only for active executable paths.
- A final-file macro table let a later `#undef` erase an earlier function's
  expansion. Source-position macro snapshots preserve earlier uses and later
  redefinitions. Live comparisons also caught wrong branch selection from
  replacement precedence and comments, and missing conditional-include order.
  The corrected probes compare in full across AST, NODES, EDGES, and FLOWS.
- Synthetic truth-test literals could consume unrelated source tokens.
  They now retain an enclosing location without advancing the source search.
  Macro reaching-definition entry handling was also corrected to follow
  argument edges, including zero-argument expansions.
- The first direct-flow implementation caused a Lua scan to exceed 120
  seconds. The final repair caches immutable method dependency graphs and
  completed parameter queries, keyed by method, parameter, depth, and
  recursion ancestors. It preserves the original two walkers and their order.
  Review rejected a combined-walk variant that let an earlier unrelated
  policy sink hide a later parameter-reachable sink; the retained regression
  verifies both findings. Each query and persistence phase gets fresh caches.

On the same saved **72,414-node Lua graph**, measured warm scans take **0.306–0.410 seconds**
with the final cache repair versus **0.172–0.234 seconds** at baseline. These
are host-specific repeated measurements, and the final scanner remains
slower in this comparison. The optimization preserves the original traversal
semantics; it does not establish identical performance. The
[acceptance receipt](astra-second-batch-acceptance.json) retains the six runs
and graph/binary hashes under `isolatedCacheRepairMeasurement`. That earlier
isolated measurement uses different binaries and a different graph from the
final 75,998-node Lua acceptance run below.

Whole-project comparisons expose substantial remaining differences that the
small exact corpus does not cover. The final measurements use the combined
frontend at `c5ea712db` on all **26 selected zlib 1.3.1 core files / 22,732
lines** and **61 Lua 5.4.7 files / 30,098 lines**. The
[tracked metrics](astra-second-batch-metrics.json) retain input, oracle,
baseline, and candidate hashes, complete record counters, and the history of
three regressions found and repaired during this batch. `final_accepted` in
that file means successful comparison producers and no regressions among
previously exact AST blocks; both `completeProjectionExact` values remain
false. These comparisons reuse retained live Joern outputs. The fresh
strict-live gate above applies to the selected corpus.

| Whole-project method blocks | Joern | Rust baseline → current | Identical baseline → current | Shared but changed now | Joern-only now | Rust-only now |
|---|---:|---:|---:|---:|---:|---:|
| zlib core | 410 | 242 → 313 | 66 → 93 | 186 | 131 | 34 |
| Lua | 2,274 | 1,806 → 2,024 | 237 → 417 | 1,209 | 648 | 398 |

| Current whole-project records | Joern | Rust | Exact records in common |
|---|---:|---:|---:|
| zlib AST | 85,865 | 33,664 | 27,431 |
| zlib NODES | 785 | 673 | 521 |
| zlib EDGES | 184,740 | 66,957 | 20,187 |
| zlib FLOWS | 200,374 | 47,511 | 3,612 |
| Lua AST | 252,489 | 141,421 | 113,123 |
| Lua NODES | 3,229 | 2,966 | 2,759 |
| Lua EDGES | 504,016 | 268,365 | 98,685 |
| Lua FLOWS | 207,820 | 140,024 | 26,367 |

Neither whole-project projection is exact. Counts compare serialized records,
including identities, order, and addresses; differences are not independent
bug counts or feature-completion percentages. An exact method block compares
its selected AST properties, without establishing exact incident edges.

All three previously exact blocks that regressed in an intermediate candidate
are exact again. zlib's `<operator>.alloc` recovered after file-scope arrays
stopped using local allocation lowering. Lua's `ldo.h:<global>` recovered
through per-declarator typedef handling, retaining ordinary aliases while
omitting function aliases as observed in Joern. `lualib.h:<global>` recovered
through parenthesized prototype recognition and declaration macro spelling.
The final comparison has **zero regressions among previously exact method
blocks**. Complete earlier and final differences remain available.

Independent review of the declaration repair also caught lost `#undef`
effects across supplied headers and missing call edges for duplicate
parenthesized functions. The retained tests now preserve macro removal and
both call edges. Header lookup respects the importing file's path and does
not fall back to a same-named file in another directory. These assertions
are in [declaration_shapes.rs](../../cpg-rs/cpg-lang-c/tests/declaration_shapes.rs);
parenthesized and mixed declarations also have generated live corpus cases.
Header effects cover supplied quoted relative headers, not general
caller-conditioned preprocessing or external build configuration.

| Integrated gate | Result at `c5ea712db` |
|---|---|
| Locked workspace tests; formatting; strict Clippy | 357 tests passed; formatting and strict Clippy passed |
| Checker tests; committed and strict-live corpus checks | 17 checker tests passed; 297/297 blocks passed in each corpus check |
| Native release build and binary acceptance | Passed, including the extracted native macOS ARM64 archive |
| Dependency audit | Passed for 73 locked dependencies |
| zlib/Lua deterministic builds, scans, incremental equivalence, and unchanged resource budgets | Passed in repeated per-command measurements and the official manifest gate |
| Completion timestamp and elapsed batch time | 08:26 UTC; approximately 115 minutes |

| Final CLI measurement | zlib core | Lua |
|---|---:|---:|
| Graph nodes | 18,068 | 75,998 |
| Build time, two runs | 4.921–5.213 s | 2.803–2.849 s |
| Maximum measured build RSS | 145.27 MiB | 371.31 MiB |
| Maximum measured export RSS | 255.70 MiB | 920.89 MiB |
| Scan time, two runs | 0.044–0.045 s | 0.323–0.326 s |
| Incremental vs. clean equivalence | Passed, 15.02 s | Passed, 8.37 s |
| Built-in scan findings | 0 | 0 |

Both runs produced identical graph, edge, JSON export, and SARIF hashes on
each project. These zero-finding outputs are determinism checks, not evidence
that the projects are vulnerability-free. Inputs and budgets remain fixed:
20 seconds per build and 512 MiB / 1,024 MiB for zlib / Lua. Per-command
measurements include exports; their memory use is higher than graph builds.
Final hashes are recorded in the
[real-project manifest](../../cpg-rs/acceptance/real-projects/manifest.json).

The [acceptance receipt](astra-second-batch-acceptance.json) binds these
results to the code revision, binaries, archive, reference, manifest, and log
hashes. Container and other-platform release tests were not run in this batch.

Final logs are retained under
`.local/astra-sprint/second-batch/post-repair/`; whole-project raw outputs,
comparison JSON, and complete diffs are under
`.local/astra-sprint/second-batch/final-real-differential/`. Earlier candidate
evidence remains under `second-batch/real-differential/`. These ignored artifacts
are local evidence. The corpus, generated reference, tests, and linked
metrics are the intended tracked evidence. Acceptance-runner RSS is a
cumulative child-process high-water mark and must remain distinct from
per-command RSS measurements.

The next acceptance work should pin primitive types separately for method
definitions, prototypes, call results, parameters, and locals. Numeric literal
types, static modifiers, function-like macros in `#if`, header/build-definition
context, and macro identities or expansion-wrapper types remain material C
gaps. General brace initializers and sized local string initialization also
have retained live counterexamples. Condition expansion also has
explicit work/depth bounds; this is not a complete C preprocessor.

Beyond C, experimental frontends still need their own live suites. Complete
schema/property compatibility, CPGQL/Scala DSL source compatibility, Joern
binary graph interchange, JVM plugins, and query-library coverage remain
separate compatibility work. This batch provides no full 1:1 port claim or
completion estimate for those surfaces.
