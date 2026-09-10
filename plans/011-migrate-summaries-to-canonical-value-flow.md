# Plan 011: Make canonical value flow the C analysis spine

> **Executor instructions**: This is a high-risk semantic migration. Complete
> Plan 010 first, preserve all release-blocking outcome gates, and stop on any
> unexplained precision/recall change. Do not weaken expected findings to make
> the migration green. Update `plans/README.md` when done.
>
> **Drift check (run first)**:
> `git diff --stat 025d9778c..HEAD -- cpg-rs/cpg-analysis cpg-rs/cpg-cli/src/lib.rs cpg-rs/cpg-cli/src/scan.rs cpg-rs/cpg-cli/tests cpg-rs/cpg-incremental/src/lib.rs cpg-rs/acceptance/rules cpg-rs/scripts/real-project-acceptance.py cpg-rs/ARCHITECTURE_GAPS.md cpg-rs/ROADMAP.md cpg-rs/COMPATIBILITY.md cpg-rs/REPLACEMENT_CONTRACT.md`
> Confirm Plans 008 and 010 are `DONE` and the shared full-corpus FLOWS gate is green.
> STOP if it is absent, if the scanner contract changed, or if production C no
> longer marks canonical DDG authoritative.

## Status

- **Status**: TODO
- **Priority**: P2
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: `plans/008-make-graph-language-metadata-explicit.md`, `plans/010-differential-gate-shared-reaching-def.md`
- **Category**: direction / correctness / architecture
- **Planned at**: commit `025d9778c`, 2026-08-18

## Why this matters

Production C has exact reaching-definition facts, and `SparseValueFlow` builds
intra/interprocedural reachability from them. Final taint still originates in a
large name/source-order analysis and uses sparse flow only to reject a subset of
ordinary call-source findings. Function summaries are also name-based. Making
canonical DDG the C analysis spine can improve witness auditability and reduce
false name matches, but it must preserve intentionally independent scanner
models and all labeled outcomes.

## Current State

- `cpg-analysis/src/value_flow.rs:41-102` builds sparse flow from ReachingDef
  edges, resolved targets, external summaries, and global bridges.
- `SparseValueFlow::from_cpg` currently consumes `SummaryStore`; it cannot be
  called recursively as the source of those same summaries without separating
  summary-independent edges from previous-round summary bridges.
- `cpg-analysis/src/taint.rs:901-905` creates it only for authoritative DDG.
  At `:1095-1137` it post-filters findings already produced by `run_analysis`.
- `cpg-analysis/src/summaries.rs:524-599` computes param/call-to-return summaries
  with a source-ordered `HashMap<String, ...>` taint model.
- Entry-point, out-parameter, persistence, assignment, receiver, shell-policy,
  guard, authz, and confinement models are intentionally not equivalent to
  ordinary DDG call-source reachability. Preserve them unless a separately
  labeled test proves a new contract.
- `cpg-analysis/tests/canonical_c_flow.rs` checks final C outcomes but does not
  isolate authoritative-DDG on/off behavior or a name-based false path.
- C has 68 labeled rule outcomes and real-project gates. Promoted non-C
  languages do not all have a parity-grade authoritative DDG, so this plan must
  not silently switch their semantics.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---|---|---|
| Shared flow gate | `cd cpg-rs/joern-parity && ./check-shared-reaching-def.sh --committed-only` | exact zero diff |
| Canonical C outcomes | `cd cpg-rs && cargo test -p cpg-analysis --test canonical_c_flow --locked` | exit 0 |
| C rule quality | `cd cpg-rs && cargo test -p cpg-cli --test rule_quality --locked` | all labels pass |
| Other languages | `cd cpg-rs && cargo test -p cpg-cli --test language_rule_quality --locked` | all labels pass unchanged |
| Committed contract | `cd cpg-rs && ./joern-parity/check.sh --committed-only && ./acceptance/cpgql/check.sh --committed-only` | all pass |
| Real C projects | `cd cpg-rs && cargo build --release --locked -p cpg-cli -p joern-parity && ./scripts/real-project-acceptance.py` | zlib/Lua acceptance passes |
| Flatgraph | `cpg-rs/joern-parity/setup-oracle.sh && cd cpg-rs && ./acceptance/flatgraph/check.sh` | verified Joern setup and live round trips pass |
| Workspace quality | `cd cpg-rs && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked` | exit 0 |

If Plan 010 chose a different shared-gate command, use the command recorded in
that completed plan and update this file before execution.

## Scope

**In scope**:

- `cpg-rs/cpg-analysis/src/value_flow.rs`
- `cpg-rs/cpg-analysis/src/summaries.rs`
- `cpg-rs/cpg-analysis/src/taint.rs`
- Focused analysis tests, especially `tests/canonical_c_flow.rs`
- `cpg-rs/cpg-cli/src/scan.rs` only for strategy selection/reporting
- `cpg-rs/cpg-cli/src/lib.rs` to select strategy from Plan 008's validated
  single-language mode
- `cpg-rs/cpg-incremental/src/lib.rs` to carry an explicit summary strategy
- Existing C rule fixtures where a new, oracle-backed distinction is required
- `cpg-rs/scripts/real-project-acceptance.py` only to expose measure-mode build
  and incremental timing fields used by the before/after comparison
- `cpg-rs/ARCHITECTURE_GAPS.md`, `ROADMAP.md`, `COMPATIBILITY.md`, and
  `REPLACEMENT_CONTRACT.md` only to update the shipped analysis boundary

**Out of scope**:

- Reimplementing reaching definitions; Plan 010 owns parity
- Switching non-authoritative/non-C graphs away from their current fallback
- Removing persistence, entry-point, authz, guard, confinement, receiver,
  shell-policy, or specialized scanner semantics
- Broad rule-catalog expansion unrelated to the migration
- Updating expected findings without a minimal fixture and reviewer-approved
  explanation of the old false positive/negative

## Git Workflow

- Use a branch such as `advisor/011-canonical-value-flow`.
- Keep characterization, shadow computation, summary switch, and finding switch
  as separate green commits. Suggested final message:
  `feat: drive C summaries through canonical value flow`.
- Do not push or open a PR unless instructed.

## Steps

### 0. Capture a pre-migration baseline before analysis edits

First make a measurement-only change to `real-project-acceptance.py`: include
each project's maximum of the two build elapsed values as `buildSecondsMax` and
the update-equivalence elapsed value as `incrementalSeconds` in `--measure` JSON.
Do not add those unstable timings to deterministic expected manifests or normal
hash comparisons. Unit-test/compile-check the script, then build release
binaries and run `--measure`, capturing output to a temporary file named with
the base SHA. Also run canonical C outcome and C rule-quality tests. Record
graph, edge, export, SARIF hashes, findings, both timing fields, and peak RSS.

Use:

```sh
baseline="${TMPDIR:-/tmp}/cpg-plan-011-025d9778c.txt"
cd cpg-rs
cargo build --release --locked -p cpg-cli -p joern-parity
./scripts/real-project-acceptance.py --measure >"$baseline" 2>&1
cargo test -p cpg-analysis --test canonical_c_flow --locked
cargo test -p cpg-cli --test rule_quality --locked
```

Do not commit measured output as a new expected baseline. Existing manifest
hashes/budgets and explicit characterization assertions remain authoritative.

**Verify**: `python -m py_compile cpg-rs/scripts/real-project-acceptance.py`
exits 0. The baseline file exists, names both projects, and contains findings,
hashes, `buildSecondsMax`, `incrementalSeconds`, and peak RSS before analysis
implementation begins.

### 1. Pin the current hybrid boundary with paired tests

Add fixtures that run the same graph with authoritative DDG enabled and with a
controlled non-authoritative fallback. Include branch kills, overwritten names,
same-name locals, sanitizer barriers, interprocedural param-to-return flow,
external summaries, globals, recursion, and one path the name walker currently
over-approximates.

Assert which ordinary configured call-source findings are DDG-gated and which
specialized scanner semantics intentionally bypass that gate. These are
characterization tests, not approval of every legacy result.

**Verify**: `cd cpg-rs && cargo test -p cpg-analysis --test canonical_c_flow --locked`
passes with explicit paired assertions.

### 2. Add a DDG-derived summary builder in shadow mode

First refactor value-flow construction into explicit phases:

1. A summary-independent base graph containing ReachingDef, C-global bridges,
   and resolved argument-to-formal edges.
2. Immutable external-summary bridges.
3. Computed-summary bridges supplied from the previous Jacobi round.

Then implement method-summary derivation by mapping formal parameters to
reachable method returns and sanitizer states over those phases. Start computed
summaries empty, preserve immutable external summaries, and run Jacobi rounds
until no summary changes. Recursive calls use only the previous round. Never
seed canonical rounds from legacy name-based computed summaries. Maintain
dependency tracking by resolved callee FQN so incremental invalidation still
recomputes transitive callers only.

Run old and new summary builders side by side for authoritative C graphs in
tests/diagnostic mode. Produce a deterministic diff of param-to-return,
call-return, sanitizer, and dependency facts. Do not switch consumers yet.

**Verify**: add summary tests for branch kill, recursion/fixpoint, overload/static
identity, external summaries, and incremental invalidation. All deterministic
shadow diffs must be empty or backed by a minimal expected-correction fixture.

### 3. Switch authoritative C summaries only

Add an internal `SummaryStrategy` (or equivalent) carried by `Project`; do not
infer language from `Cpg` alone. Plan 008's validated `Single(Language::C)` open
path and fresh C project constructor select `CanonicalValueFlow`; every other
language plus mixed/unknown read-only projects select `NameBased`. The canonical
strategy must also require authoritative DDG and fail closed if that invariant
is absent. Do not expose a user flag that creates undocumented production modes.

Preserve the public `FunctionSummary` shape and external-summary composition so
callers and persisted workflows do not need compatibility glue.

**Verify**: direct build, save/load/reopen, external-summary, incremental update,
and recursive summary tests produce identical approved facts/findings.

### 4. Generate ordinary C source-to-sink witnesses from value flow

Replace the generate-then-post-filter path for ordinary configured call sources
and sinks with direct sparse reachability and witness reconstruction. Respect
sanitizer barriers during traversal, preserve stable ordering, and include
interprocedural summary/global edge provenance in each witness.

Leave specialized policy models on their existing paths. Merge their findings
with canonical DDG findings through one stable deduplication identity that
includes sink file/location and relevant path identity.

**Verify**: tests prove a killed/name-shadowed path is never generated, a valid
cross-call path contains its internal hops, sanitizer traversal is blocked, and
specialized scanner findings remain present.

### 5. Prove semantic and performance acceptance before removing shadow code

Run all C labels, promoted-language labels, CPGQL/reachableBy, Flatgraph, zlib,
Lua, save/load, and incremental equivalence gates. Compare finding counts,
stable hashes, witness paths, wall time, and peak RSS with the pre-migration
baseline captured in Step 0. Existing committed hashes/resource ceilings remain
the pass/fail authority. A greater-than-20% wall-time or RSS increase across two
cold reruns requires explanation and reviewer approval even if it remains under
the ceiling. Pin witness paths through explicit Step 1 assertions rather than a
generated snapshot. Explain every approved semantic delta with a minimal fixture.

Remove the C shadow computation only after all acceptance is green. Keep the
name-based fallback for non-authoritative graphs and document that boundary.

**Verify**: all commands in Commands You Will Need pass. Resource use remains
inside committed budgets; no expected catalog was weakened.

## Test Plan

- Paired authority on/off characterization tests.
- DDG summary unit tests for kills, branches, recursion, externals, globals,
  sanitizer provenance, and incremental invalidation.
- Direct witness tests for source, interprocedural hops, and sink identity.
- Full C rule-quality and real-project acceptance.
- Full non-C labeled suite to prove fallback semantics did not change.

## Done Criteria

- [ ] Plan 010 shared full-corpus FLOWS gate remains exact
- [ ] Authoritative C summaries derive from canonical value flow
- [ ] Non-authoritative graphs retain an explicit tested fallback
- [ ] Ordinary C findings are generated from DDG rather than post-filtered
- [ ] Specialized scanner models remain independently tested and unchanged
- [ ] Witness identity includes sink file/location and stable flow provenance
- [ ] All labeled, real-project, save/load, update, query, and Flatgraph gates pass
- [ ] No unexplained finding/hash/resource-budget delta remains
- [ ] Full workspace formatting, locked Clippy, and tests pass
- [ ] Architecture and compatibility documents describe the final strategy boundary
- [ ] `plans/README.md` marks Plan 011 `DONE`

## STOP Conditions

- Plan 008 or Plan 010 is not complete, or Plan 010 is not exact zero diff on
  the full committed corpus.
- A proposed summary cannot be derived without reintroducing name-based guesses
  into the authoritative path.
- A labeled precision/recall result changes without a minimal fixture proving
  the previous result wrong.
- Non-C semantics must change to complete the C migration.
- Real-project wall time or RSS exceeds committed budgets after one cold and one
  warm rerun.

## Maintenance Notes

- Authoritative DDG is a semantic capability, not merely the presence of some
  ReachingDef edges.
- Keep specialized security policy models separate from generic value flow so
  their intentional over-approximations remain reviewable.
- Future language promotion to DDG summaries requires its own oracle/outcome
  gate; C's contract cannot be inherited automatically.
