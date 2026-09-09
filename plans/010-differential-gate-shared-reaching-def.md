# Plan 010: Differential-gate the shared ReachingDef pass

> **Executor instructions**: Do not switch production C to the shared pass until
> the new full-corpus differential is exactly zero. Run every verification and
> stop on a STOP condition instead of weakening normalization or dropping oracle
> facts. Update `plans/README.md` when complete.
>
> **Drift check (run first)**:
> `git diff --stat 025d9778c..HEAD -- cpg-rs/cpg-analysis/src/reaching_def.rs cpg-rs/cpg-core/src cpg-rs/cpg-lang-c/src cpg-rs/joern-parity cpg-rs/scripts/check-workflow-contract.py .github/workflows`
> STOP if production C no longer imports authoritative DDG, the FLOWS oracle
> shape changed, or another differential already covers the shared pass.

## Status

- **Status**: TODO
- **Priority**: P1
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: `plans/006-complete-cpg-persistence-safety.md`, `plans/007-bind-release-to-verified-contract.md`
- **Category**: correctness / tests / architecture
- **Planned at**: commit `025d9778c`, 2026-08-18

## Why this matters

The shared `cpg_analysis::ReachingDefPass` is described as a port of a
parity-validated algorithm, but the production 1,961-fact gate covers the
separate canonical C builder/import path. Production imports DDG as
authoritative and skips the shared pass. Before summaries or other languages
can rely on the shared implementation, its output, edge properties, and newer
schema-node behavior must be compared directly with the committed FLOWS oracle.

## Current State

- `cpg-lang-c/src/lib.rs:75-82` builds canonical text and imports it into `Cpg`.
- `cpg-lang-c/src/exact.rs:475-497` generates the FLOWS facts used by production.
- `cpg-lang-c/src/import.rs:197-209` installs those edges and marks DDG
  authoritative. `cpg-analysis/src/pass.rs:124-142` then skips DDG writers.
- `cpg-analysis/src/reaching_def.rs:80-113` computes shared flows but writes
  plain `ReachingDef` edges, discarding `ReachingDefFlow.var` even though the
  graph supports edge properties through `Cpg::add_edge_with_property`.
- `cpg-lang-c/src/import.rs:269-307` cannot serve as the new comparator as-is:
  it derives VARIABLE from the source node and stores rendered flows in a set,
  hiding property differences and multiplicity.
- The shared pass deduplicates only `(src,dst)`, which can collapse distinct
  VARIABLE facts between the same endpoints.
- Its module header says `MethodParameterOut`, `TypeRef`, and `JumpTarget` do not
  exist, but all now exist in `cpg-core/src/schema.rs:47-52`.
- `joern-parity` already depends on `cpg-analysis`, `cpg-core`, and `cpg-lang-c`,
  so it is the correct dependency-neutral home for a direct comparison.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---|---|---|
| Existing contract | `cd cpg-rs/joern-parity && ./check.sh --committed-only` | all existing blocks pass |
| Shared-flow contract | `cd cpg-rs/joern-parity && ./check-shared-reaching-def.sh --committed-only` | exact zero diff |
| Analysis tests | `cd cpg-rs && cargo test -p cpg-analysis --locked reaching_def` | exit 0 |
| Workflow contract | `python cpg-rs/scripts/check-workflow-contract.py` | exit 0 with the shared gate required |
| Workspace | `cd cpg-rs && cargo test --workspace --locked` | exit 0 |
| Lint/format | `cd cpg-rs && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 |
| Diff hygiene | `git diff --check` | no output |

The new script name may differ if extending `check.sh` with an explicit
`--shared-reaching-def` mode is simpler. It must remain a separate named gate.

## Scope

**In scope**:

- `cpg-rs/cpg-analysis/src/reaching_def.rs`
- Focused tests under `cpg-rs/cpg-analysis/`
- `cpg-rs/cpg-core/src/graph.rs` only for edge-property/layer-authority APIs
- `cpg-rs/cpg-lang-c/src/import.rs` for the test/harness import mode and a
  property-aware serializer used only by the independent shared gate
- `cpg-rs/joern-parity/src/`, its `Cargo.toml`, focused check scripts,
  `oracle.sc`, `oracle_all.txt`, `QUIRKS.md`, and `corpus/` fixtures when pinned
  live evidence is required
- `.github/workflows/cpg-rs.yml`, `release-contract.yml`, and the reusable
  contract created by Plan 007 to require the gate
- `cpg-rs/scripts/check-workflow-contract.py` to assert the new dependency

**Out of scope**:

- Switching the production C graph producer to the shared pass
- Editing oracle output by hand or normalizing away differences
- Changing source-language lowering to make the pass easier
- Migrating summary/taint semantics; Plan 011 owns that
- Claiming non-C parity from a C-only differential

## Git Workflow

- Use a branch such as `advisor/010-shared-reaching-def-gate`.
- Commit the harness before semantic fixes when possible, keeping the existing
  release gate green. Use a message such as `test: gate shared reaching definitions`.
- Do not push or open a PR unless instructed. Without operator-owned remote CI
  proof from Step 6, leave the plan `BLOCKED (awaiting operator CI)`.

## Steps

### 1. Build an exact graph fixture without imported DDG

Add a harness-only import path that uses the exact canonical C nodes, AST,
symbol, call, and CFG facts but omits FLOWS and does not mark DDG authoritative.
Do not clone and mutate a production graph in a way that leaves hidden DDG
properties or authoritative state. Keep this API non-public or clearly test
oriented.

Assert that all non-DDG normalized graph blocks match production and that the
fixture contains zero ReachingDef edges before the shared pass runs.

**Verify**: a focused `joern-parity` test proves exact non-DDG identity and zero
initial DDG.

### 2. Emit complete shared ReachingDef facts

Preserve `ReachingDefFlow.var` when writing edges: intern non-empty VARIABLE
values and call `add_edge_with_property`. Never deduplicate by endpoint pair.
Initially preserve the generated fact multiset; if the live oracle proves that
identical `(src,dst,var)` proposals collapse, implement that exact triple-level
deduplication and add a fixture pinning it. Ensure pass clearing removes mirrored
property-bearing edges idempotently.

Add unit tests for two distinct variable facts with the same endpoints, empty
entry variables, save/load property preservation, and incremental rerun.

**Verify**: `cd cpg-rs && cargo test -p cpg-analysis --locked reaching_def`
exits 0 and property assertions are explicit.

### 3. Compare shared output with every committed FLOWS fact

Run the shared pass over the full canonical corpus and serialize facts with the
same stable addresses and sorting used by the oracle. Implement a dedicated
property-aware, multiplicity-preserving serializer: read VARIABLE from
`HalfEdge.property`, render empty properties as `[]`, collect into a `Vec`, and
stable-sort without `BTreeSet`/endpoint-only deduplication. Do not call the
existing `flow_variable(cpg, source)` fallback in this gate. Compare the
complete block, not sampled methods or counts. The harness must fail for
missing, extra, property-different, or duplicate facts.

Keep this as a separate output/gate so production-import parity cannot
accidentally satisfy the shared-pass check.

**Verify**: introduce one temporary shared-flow mutation and prove only the new
gate fails. Revert it, then require exact zero diff.

### 4. Close schema divergences exposed by the differential

Implement `MethodParameterOut`, `TypeRef`, `JumpTarget`, captured identifiers,
and any other current-schema behavior only when an existing or newly generated
Joern oracle fixture demonstrates the required edge. Add the smallest corpus
fixture for each previously unrepresented behavior and regenerate the oracle
only through pinned live Joern.

Do not infer Joern behavior from old comments. Update the module header to list
only real remaining divergences after the gate is green.

**Verify**: both committed production parity and shared-flow parity are exact
zero diff after each behavior family.

### 5. Require the independent gate in CI

Add shared-flow parity to cheap committed checks and to the required release
contract. It must not download Joern in committed mode. Live regeneration may
reuse the verified oracle from Plan 007 when available.

**Verify locally**: `python cpg-rs/scripts/check-workflow-contract.py` exits 0
and asserts that `linux-package` transitively requires the shared-flow gate.

### 6. Obtain operator-owned CI proof

Ask an authenticated operator to run the implementation in GitHub without
creating a release. Record one green run and one disposable forced-failure run
where the shared-flow gate is deliberately broken. The latter must make the
required `linux-package` context fail or remain blocked. The operator must
revert the deliberate failure before merge.

Without both links, do not mark the plan done; set
`BLOCKED (awaiting operator CI)`. The executor must not push unless instructed.

**Verify**: both run links identify the implementation commit(s), and the forced
failure cannot produce a successful `linux-package` result.

## Test Plan

- Unit-test edge VARIABLE storage and idempotent removal/recomputation.
- Differential-test every committed C FLOWS fact through the shared pass.
- Add focused oracle fixtures for parameter-out, type/jump nodes, captures, and
  same-endpoint/different-variable cases where Joern emits them.
- Preserve existing production C and Flatgraph round-trip gates.

## Done Criteria

- [ ] Shared-pass fixture starts with exact non-DDG graph and no imported DDG
- [ ] Shared edges preserve VARIABLE property and full duplicate semantics
- [ ] Full committed corpus has exact zero shared-pass FLOWS diff
- [ ] Current schema-node divergences are implemented or explicitly oracle-proven absent
- [ ] New shared gate is independent of production imported-FLOWS gate
- [ ] Required CI fails if either production or shared gate fails
- [ ] Workflow-contract script includes the shared-gate dependency
- [ ] Operator supplied green and forced-failure GitHub run links
- [ ] Full workspace, formatting, locked Clippy, and diff hygiene pass
- [ ] Production graph producer has not been switched in this plan
- [ ] `plans/README.md` marks Plan 010 `DONE`

## STOP Conditions

- The harness cannot construct exact non-DDG input without changing production
  graph semantics.
- An apparent difference cannot be resolved from the pinned live oracle.
- ReachingDef VARIABLE cannot be represented losslessly in current graph edge
  properties without a broader schema redesign.
- Making the shared gate green requires filtering or editing inconvenient facts.
- The work begins changing summaries or final finding policy.

## Maintenance Notes

- Keep production-import and shared-computation gates separate until an explicit
  convergence plan proves they are interchangeable.
- Every new node/edge schema family that participates in DDG should add a shared
  differential fixture.
- "Ported from a parity implementation" is not the same claim as "this pass is
  parity-gated"; documentation must preserve that distinction.
