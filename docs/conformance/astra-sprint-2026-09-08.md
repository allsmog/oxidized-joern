# First measured Astra parity batch

This is a historical checkpoint. The [second batch](astra-second-batch-2026-09-08.md)
records subsequent fixes and broader whole-project comparisons.

This batch closes two demonstrated C bugs and strengthens the differential
checker. It establishes a reproducible live Joern baseline; it does not
establish full Joern compatibility or a reliable completion date for that
larger project. Work began on 2026-09-08 at approximately 05:40 UTC
(2026-09-07, 22:40 Pacific), using a coordinating Astra agent and three
parallel implementation/validation agents.

Implementation and integrated acceptance completed by 06:30 UTC, roughly
**50 minutes** after that start. This interval includes oracle provisioning,
two primary fixes, regression repairs found in review, and final validation;
documentation review and local closeout followed.

The changes are on `codex/astra-parity-sprint`, based on
`6fcb30a25bbee3ca3c05b5b197cd3ce93ca4d11a`, in the sibling worktree
`.codex-worktrees/joern-oxidized-astra-sprint`. The original checkout's
uncommitted audit/planning documents were preserved. Nothing was pushed or
merged into the original branch.

## Bugs fixed and observable results

| Area | Before | After and evidence |
|---|---|---|
| Braceless C `if` | A consequence such as `if (x > 0) sink(x);` or `return x;` could disappear from the canonical graph. | Consequences receive the Joern-compatible block wrapper. Three graph regressions and a final-finding/sanitizer regression pass; the added corpus agrees with live Joern. |
| C return summaries and witnesses | An assignment on one branch could erase a possible parameter-to-return flow. `if (condition) value = "safe"; return value;` produced an empty summary and no finding through the wrapper. | Method-local reaching definitions preserve the branch where the assignment does not execute. Definite kills, both-branch kills, loop behavior, call boundaries, sanitizers, and recursive witnesses have regression coverage. |
| Differential checker | A failed Rust producer, missing expected method, or missing flow section could pass. Review also found ignored dot-prefixed methods, orphan AST text, and malformed structural edges dropped by grouping. | Seventeen checker tests exercise fail-closed behavior. Strict `--live` requires successful fresh output without fallback or reference mutation. Malformed identities, duplicate/colliding names, and unexpected methods fail. Structural edges are compared in full before grouping. CI runs the checker tests. |

The syntax fix is in [the C lowering](../../cpg-rs/cpg-lang-c/src/exact.rs).
Its assertions live in
[production graph tests](../../cpg-rs/joern-parity/src/production.rs) and
[braceless-if scanner tests](../../cpg-rs/cpg-analysis/tests/canonical_c_braceless_if.rs).
The shared return dependency graph is in
[return_flow.rs](../../cpg-rs/cpg-analysis/src/return_flow.rs), with
[summary and finding tests](../../cpg-rs/cpg-analysis/tests/canonical_c_flow.rs).
The [checker tests](../../cpg-rs/joern-parity/test_check.py) exercise failures
independently of Cargo or a JVM. Workspace graph-shape version 12 invalidates
caches built before the syntax correction.

Independent review caught an introduced mixed-language regression before
integration: graph-wide C authority made a merged Python accessor lose its
parameter flow. The fix additionally requires method-local `SOURCE_FILE`
linkage. The committed Java accessor regression verifies that generic
parameter-to-return flow remains unchanged after merging and guards that
boundary.
This is a convention of today's frontends, and must be revisited if a generic
frontend starts emitting that same provenance edge.

The real-project review then found a source-location regression introduced
by the expanded AST: Lua's existing `sweep2old` assignment
`p = &curr->next` moved from line 1098 to an identical statement in another
function at line 839. The old importer used one file-wide substring cursor.
The repair anchors methods to parsed source ranges, matches complete tokens,
and isolates globals from function bodies. Its tests cover repeated
statements and overlapping parameter, declaration, and expression spans.
Transformed or synthetic nodes may inherit a located AST ancestor's line;
this is an approximate enclosing location, not an exact Joern location claim.
The full Lua probe confirms the assignment's line is restored to 1098, with
the newly included assignment in `separatetobefnz` retaining its own line 981.
Independent review still reproduces a preexisting location limit: a
multiline parameter sharing its function's name can inherit the function
name's line. Token-window matching can also remain quadratic for many
unmatched nodes; the real-project budgets below bound only the measured
projects.

## Live oracle and corpus

The reference is Joern **v4.0.555**, running under JDK 21. The official
2,123,255,628-byte release archive was checked against its published SHA-256:
`12989883d6b5aeacc97b2dc0ecc4e2951bf50e48cb244f8c8f333d11a8be0c7e`.
All 266 extracted runtime files were verified against the complete archive.
The Rust release itself remains native and does not require this JVM; the
JVM is used for differential validation.

| Measurement | Baseline | Expanded corpus |
|---|---:|---:|
| C files / source lines | 16 / 245 | 17 / 268 |
| Method comparison blocks | 79 | 86 |
| Total comparison blocks | 96 | 103 |
| AST records | 1,898 | 2,048 |
| Scaffolding records | 148 | 163 |
| Structural edge records | 3,433 | 3,675 |
| Reaching-definition records | 1,458 | 1,552 |

The baseline freshly generated oracle matched the old committed reference
with zero differing lines. The expanded reference was generated by Joern,
adding 501 records and deleting none; its values were not edited by hand.
Both committed and strict live checks pass for the combined implementation.
The [checker README](../../cpg-rs/joern-parity/README.md) defines the selected
properties and blocks. These counts are not feature-completion percentages.

Reference SHA-256 values:

- Baseline: `5e9673a5784734d0cdbfc9360b420ac19f459be1d710bda8d546490dee740ab2`.
- Expanded: `940cca214483c4acc39ae83426cb3fed4b01696d093eeafdd999a8e547a36ab6`.

A separate live `reachableBy` probe agrees with Rust on **five of five**
return-flow cases: conditional overwrite, zero-or-more loop, and an
alternative raw branch retain flow; definite overwrite and both branches
overwriting remove it. The
[input, Joern script, expected outcomes, and replay instructions](../../cpg-rs/cpg-analysis/tests/fixtures/return-flow/README.md)
are committed together. These five outcomes do not establish general query
or scanner equivalence.

## Integrated validation

- Locked Rust workspace: **314 passed, zero failed or ignored**.
- `cargo fmt --all -- --check` and strict workspace/all-target Clippy passed.
- Checker regressions: **17 passed**, plus eight independent malformed-output
  probes; ShellCheck passed.
- Committed C comparison and fresh Joern comparison: **103/103** each.
- Native release build and native binary acceptance passed, including
  version, build, save/load through the query server, scan/SARIF, and
  controlled malformed-graph rejection.
- Cargo Audit passed against 73 locked dependencies; this batch did not
  change the dependency lockfile.

The final combined code revision for these checks is
`d40b0cc28f3f37413236fd04790b2029f060c153`. Subsequent changes record the
measured acceptance hashes and documentation.

Container, archive-packaging, and cross-platform release jobs were not run
locally.

## Real-project measurements

Both pinned projects produced identical graph, edge, export, and SARIF hashes
across two final builds, and both incremental updates matched clean rebuilds.
All measurements below use the final combined binary. Per-command memory
was measured with macOS `/usr/bin/time -l`; the peak shown includes build,
export, and scan. These are two trials on one host, not a capacity benchmark.

| Project | Source files | Nodes before → after | Build time, two trials | Peak measured RSS | Incremental equivalence |
|---|---:|---:|---:|---:|---:|
| zlib 1.3.1 core | 26 | 12,348 → 13,502 | 1.55–1.92 s | 175.84 MiB | 5.04 s |
| Lua 5.4.7 | 61 | 71,157 → 75,332 | 3.08–3.24 s | 890.11 MiB | 9.85 s |

The existing limits remain **20 seconds** for build time and **512/1,024 MiB**
for zlib/Lua. Both projects still produce zero findings and the same SARIF
hash as baseline; this is a deterministic workflow result, not a claim that
either project is vulnerability-free. The node/content review found no
baseline node content lost when excluding location fields. Restored branches
add 1,154 nodes / 6,822 edges in zlib and 4,175 nodes / 30,295 edges in Lua.
The subsequent location repair changes locations without changing that node
content or the structural edges.

The [manifest](../../cpg-rs/acceptance/real-projects/manifest.json) updates only
the measured node counts and graph/edge/export hashes. Inputs, archive
checksums, findings, SARIF expectations, and resource ceilings are unchanged.
These projects have not been compared in full with Joern.

The standard acceptance runner then passed against that tracked manifest in
25.22 seconds. Its reported peak RSS was 175.5 MiB for zlib and 888.1 MiB for
Lua; incremental equivalence took 4.47 and 9.26 seconds in that run.

## Timing and interpretation

Recorded on macOS ARM64 with Rust 1.97.0 and JDK 21:

| Activity | Recorded wall time | Interpretation |
|---|---:|---|
| Oracle setup to first live baseline | 369.53 s | Includes network/provisioning; other agents worked concurrently. |
| Full oracle archive download and verification | 838.44 s | Overlaps implementation; not additive to the previous row. |
| Baseline live Joern generation | 18.82 s | Warm repeat time depends on the host and corpus. |
| Combined release rebuild | 28.34 s | Dependencies and baseline build already present. |
| Final rebuild after location repair | 30.00 s | Same host and cached dependencies; includes all code changes. |

The worker handoffs reported approximately 11 minutes for the initial syntax
implementation and 17 minutes for the return-flow implementation and its
review fix. These were agent-reported intervals, not separately instrumented
benchmarks, and exclude later integration work and the source-location
repair. Their first validated commits were authored at 05:51:43 UTC
(`1653f86c8`) and 05:57:43 UTC (`42c9f77d6`), respectively.

This demonstrates that parallel Astra work can close concrete semantic gaps
and verify them against an executable oracle in one session. It does not
measure a speedup against a human team, token cost, or the time needed for all
frontends, the query language, plugins, and binary interoperability. This is
the first batch of the proposed calibration sprint, not a completed one- or
two-day sampling period.

## Remaining work and next acceptance criteria

1. Correct direct intraprocedural finding propagation. A follow-up live
   probe confirmed that `value = source(); if (c) { value = "safe"; }
   sink(value);` still produces no Rust finding while Joern reports a flow.
   The fix in this batch covers return summaries and their witnesses; the
   initial statement-based finding generator remains unchanged. Accept this
   next slice only when both engines agree on optional and definite kills,
   joins, loops, and sanitizer negatives in direct source-to-sink cases.
2. Normalize scalar conditions and external symbols against live Joern.
   In a follow-up probe Joern lowers `if (value)` to `value != 0`, while
   Rust keeps the identifier. Rust also introduces phantom `LOCAL` nodes for
   external callees, including a declared prototype, and loses its declared
   return type in the stub. Each needs a minimal fixture, generated reference,
   and production graph regression. Braceless loop bodies remain unpinned.
   Broader flow gaps include pointer/out-parameter and receiver effects,
   aliasing, custom semantics, and nested-call combinations. A possible flow
   witness does not prove full path feasibility.
3. Establish language-specific live oracle suites for each experimental
   frontend. Shared schema and smoke tests do not establish Joern parity.
4. Define whether "1:1" includes CPGQL/Scala console source compatibility,
   JVM plugins, querydb coverage, and Joern binary graph interchange. Those
   are outside the current native product's compatibility promise and have
   not been implemented by this batch.

The next estimate should use several more independently reviewed slices
across syntax, flow semantics, and another frontend. Only then will observed
throughput begin to constrain an overall schedule.

## Local evidence and replay

The integration worktree retains ignored logs under
`.local/astra-sprint/final/`: `workspace-tests.log`, `clippy.log`,
`release-build.log`, `committed-parity.log`, `live-parity.log`, and
`binary-release.log`. Baseline dependency, parity, and real-project logs are
under `.local/astra-sprint/`. Oracle provenance and baseline JSON are in the
original checkout's `.local/astra-sprint/oracle/` (`setup-result.json` and
`runs/baseline/baseline-result.json`). These local artifacts are not part of
the Git commit; the corpus, reference, regression tests, and replay scripts
are tracked.

Final project measurements and the node/location review are retained in
`.local/astra-sprint/real-project-review/final-measurements.json`,
`final-measurements.log`, and `final-graph-review.json`, with replay helpers
`measure.py` and `review-final.py` in that directory. The standard acceptance
runner's log is `.local/astra-sprint/final/real-project-acceptance.log`.
That runner reports a cumulative child-process RSS high-water mark, so its
reported peak is distinct from the per-command measurements above.

The sibling `joern-oxidized-astra-c-syntax` worktree retains the Lua location
probe and its before/introduced/repaired output under
`.local/astra-lines/check-lua.py` and `.local/astra-lines/lua-regression.json`.

The sibling `joern-oxidized-astra-c-flow` worktree retains the five-outcome
live run at `.local/c-flow-oracle/joern-tracked-fixture.log` and
`.local/c-flow-oracle/tracked-results.txt`. Its `.local/next-c-chunks/`
contains `summary.json`, `graph.diff`, `joern-canonical.txt`, `rust.txt`,
`joern-direct.log`, and `rust-direct.jsonl`, supporting the three remaining
gaps above. The setup JSON and baseline result JSON provide the instrumented
oracle timings; `.local/astra-sprint/pre-location/release-build.log` in the
integration worktree records the initial combined release rebuild time.

From the integration worktree:

```sh
cargo test --manifest-path cpg-rs/Cargo.toml --workspace --locked
python3 cpg-rs/joern-parity/test_check.py
bash cpg-rs/joern-parity/check.sh --committed-only
JAVA_HOME=/path/to/jdk21 JOERN=/path/to/joern-cli \
  bash cpg-rs/joern-parity/check.sh --live
python3 cpg-rs/scripts/test-release.py \
  --binary cpg-rs/target/release/cpg --version 0.1.1
python3 cpg-rs/scripts/real-project-acceptance.py
```
