# Plan 006: Complete the persisted CPG safety contract

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving on. If a
> STOP condition occurs, stop and report; do not improvise. When done, update
> this plan's status row in `plans/README.md` unless a reviewer owns the index.
>
> **Drift check (run first)**:
> `git diff --stat 025d9778c..HEAD -- cpg-rs/cpg-core/src/graph.rs cpg-rs/cpg-core/src/persist.rs cpg-rs/cpg-cli/src/workspace.rs .github/workflows/cpg-rs.yml`
> If an in-scope persistence API, envelope layout, graph-shape version, or CI
> topology changed, compare it with Current state. STOP if the described gaps
> no longer exist or the CPG2 reader was independently redesigned.

## Status

- **Status**: TODO
- **Priority**: P0
- **Effort**: L
- **Risk**: MED
- **Depends on**: none
- **Category**: security / correctness / tests
- **Planned at**: commit `025d9778c`, 2026-08-18
- **Supersedes residuals from**: `plans/002-harden-cpg-persistence.md`

## Why this matters

CPG2 validates individual counts, references, and checksums, but the accepted
counts can still expand into several GiB of decoded Rust containers. A hostile
or corrupt graph can therefore abort the release binary during allocation.
Semantic envelope flags are also outside checksum coverage, and `save` can
return an error after the destination was already replaced. This plan closes
those gaps without dropping bounded CPG1 or current CPG2 compatibility.

## Current state

- `cpg-rs/cpg-core/src/graph.rs:54-68` permits a 1 GiB encoded file, 25 million
  nodes, 100 million edges, and 250 million passthrough values.
- `graph.rs:815-1050` allocates many node-sized columns, two adjacency-vector
  tables, sparse property maps, and derived indexes after checking encoded
  lower bounds but without an aggregate decoded-memory budget.
- `graph.rs:951-993` checks aggregate passthrough counts but does not prove each
  declared value vector fits the remaining payload before capacity allocation.
- `graph.rs:424-433` computes CRC32 over only the payload. Flags at `:38-51`
  control authoritative layers and passthrough decoding but are not covered.
- `graph.rs:1387-1392` publishes the destination before syncing its parent. A
  parent-sync error therefore means "committed, durability uncertain", not
  "old destination preserved".
- The repository keeps compatibility explicit: new files may use a newer CPG2
  envelope version, but current CPG2 version 1 and bounded CPG1 must have tested
  read policies. New writes never need to emit an old version.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---|---|---|
| Baseline RSS | `cd cpg-rs && FILES=8000 FNS=25 /usr/bin/time -v cargo run --release -p cpg-incremental --example scale` | exit 0; record maximum RSS before selecting the budget |
| Focused tests | `cd cpg-rs && cargo test -p cpg-core --locked persistence` | exit 0 |
| Workspace tests | `cd cpg-rs && cargo test --workspace --locked` | exit 0 |
| C acceptance | `cd cpg-rs && cargo build --release --locked -p cpg-cli -p joern-parity && ./scripts/real-project-acceptance.py` | zlib/Lua hashes and workflows pass |
| Language acceptance | `cd cpg-rs && ./scripts/language-project-acceptance.py` | all pinned language projects pass |
| Lint | `cd cpg-rs && cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 |
| Format | `cd cpg-rs && cargo fmt --all --check` | exit 0 |
| Diff hygiene | `git diff --check` | no output |

## Scope

**In scope**:

- `cpg-rs/cpg-core/src/graph.rs`
- `cpg-rs/cpg-core/src/persist.rs` only if a checked reader helper belongs there
- `cpg-rs/cpg-cli/src/workspace.rs` only for a graph-shape/cache version bump
- `cpg-rs/acceptance/real-projects/manifest.json` and
  `cpg-rs/acceptance/language-projects/manifest.json` only for raw CPG hashes
  changed by the new envelope version

**Out of scope**:

- Joern Flatgraph format changes
- Compression, mmap, streaming queries, or cryptographic signatures
- Removing bounded CPG1/current-CPG2 read compatibility
- Raising limits merely to make a malformed regression pass
- General cache metadata or language resolution; Plan 008 owns those

## Git Workflow

- Use a focused branch such as `advisor/006-cpg-persistence-safety`.
- Commit one logical persistence unit with a repository-style message such as
  `fix: complete persisted CPG safety checks`.
- Do not push or open a PR unless instructed.

## Steps

### 1. Enforce an aggregate decoded-memory budget before large allocations

Run the Baseline RSS command before editing and record its maximum RSS in the PR
description. Add a checked budget helper near the persistence limits. Start
with a named 2-GiB decoded-state ceiling only if the baseline remains below 1
GiB; if it does not, STOP and report the command, graph size, and RSS before
choosing another limit. On non-GNU hosts use the platform's equivalent
`/usr/bin/time` RSS flag and record the exact command.

The estimate must conservatively include string bytes, every node column,
`Vec<HalfEdge>` table overhead for both directions, two stored `HalfEdge`
values per edge, external labels, per-node passthrough maps, passthrough values,
file maps, node lists, and free-list capacity. Use `size_of` plus checked
arithmetic, not hand-waved byte constants. Charge progressively where counts
arrive later in the payload and reject before `with_capacity`/collection.

Use `try_reserve` for direct attacker-sized vector/map capacities where the
standard collection API exposes it, mapping reservation errors to
`DecodeError`/`InvalidData`. The budget's security guarantee is that file
declarations cannot request unbounded decoded state; it does not promise
recovery from unrelated whole-process allocator exhaustion. Do not rely on
`catch_unwind`; allocation aborts are not unwindable in the release profile.

**Verify**: `cd cpg-rs && cargo test -p cpg-core --locked decoded_budget` exits
0. Tests must include a compact payload whose legal encoded counts exceed the
decoded budget and prove rejection before graph construction.

### 2. Validate passthrough lengths before reserving values

For every passthrough kind, call a checked remaining-bytes helper before
allocating or collecting values: 4 bytes per optional string/int and 1 byte per
bool. Also prove enough bytes remain for each property header before reading
its label, kind, and count. Keep the aggregate property/value ceilings as a
second independent bound.

Extend malformed tests to cover external labels, edge-property symbols,
unknown property kinds, duplicate labels, boolean bytes, oversized per-property
counts, and counts that fit the global ceiling but not the remaining payload.

**Verify**: `cd cpg-rs && cargo test -p cpg-core --locked passthrough` exits 0
without panic or excessive allocation.

### 3. Protect semantic envelope metadata with a new readable version

Keep `CPG2` magic, advance its explicit format version, and make new writes
checksum the semantic header fields plus payload. Reserved/unknown flags must
still fail before graph construction. Retain a bounded reader for current CPG2
version 1 using its historical payload-only CRC contract, and continue writing
only the new version.

Add fixed golden fixtures for CPG1, CPG2 version 1, and the new version. Test a
bit flip in every authoritative-layer flag: the new version must reject it via
integrity validation. Bump `GRAPH_SHAPE_VERSION` because cache identity says
persistence layout changes invalidate old cache artifacts.

Do not call CRC32 authentication; it detects accidental corruption. Do not add
a new checksum dependency unless the existing algorithm cannot express the
versioned coverage.

**Verify**: `cd cpg-rs && cargo test -p cpg-core --locked envelope` exits 0 and
all three documented compatibility fixtures have explicit assertions.

Because the envelope bytes change, regenerate acceptance actuals with both
project scripts' `--measure` mode. Update only each manifest's raw
`graphSha256` when node counts, edge/export/SARIF hashes, findings, and source
counts are unchanged. Any non-graph delta is a STOP condition requiring root
cause, not a baseline update.

**Verify**: the C and language acceptance commands pass in normal non-measure
mode after the reviewed raw graph-hash update.

### 4. Document and test the post-publication save outcome

Keep pre-publication failures preserving the previous file. Once rename/persist
succeeds, document that a later parent-sync error can mean the new graph is
visible with uncertain crash durability. Do not claim the old destination is
preserved after commit.

Refactor the existing injectable atomic-write helper only enough to inject a
post-publication sync failure. Assert that the returned error includes the
destination and states that publication completed, the destination decodes as
the new graph, and no temporary file remains. Unsupported directory syncing on
non-Unix remains an explicit platform limitation rather than simulated safety.

**Verify**: `cd cpg-rs && cargo test -p cpg-core --locked atomic` exits 0.

### 5. Leave focused tests ready for the release matrix

Keep platform-specific assertions narrowly gated so the focused persistence
tests can run unchanged on Linux, macOS, and Windows. Plan 007 owns adding and
remotely verifying the cross-platform workflow matrix after this plan lands.

**Verify**: all focused tests pass locally and no ordinary overwrite/concurrent
writer assertion is Unix-only without a documented platform reason.

## Test Plan

- Extend the table-driven mutation tests in `cpg-core/src/graph.rs` rather than
  creating a second decoder test style.
- Add budget tests that use small byte vectors with large declared counts; do
  not allocate multi-GiB fixtures.
- Add golden read tests for every supported wire version and assert new writes
  use only the newest version.
- Add pre-commit failure, post-commit sync failure, overwrite, cleanup, and
  concurrent-writer assertions.
- Run the full workspace because every CLI load/cache/export path uses CPG2.

## Done Criteria

- [ ] Every attacker-controlled allocation is covered by remaining-byte,
      per-class, aggregate-count, and decoded-memory checks before reservation
- [ ] Direct attacker-sized capacity reservations use fallible APIs where available
- [ ] New semantic flags are integrity-covered under a new explicit version
- [ ] CPG1 and current CPG2 version 1 have bounded golden compatibility tests
- [ ] Raw graph hash changes are reviewed; all semantic/output hashes stay unchanged
- [ ] Pre-publication and post-publication save failures have truthful,
      separately tested contracts
- [ ] Focused persistence tests are platform-portable and ready for Plan 007 CI
- [ ] Formatting, locked Clippy, and locked workspace tests pass
- [ ] Only in-scope files changed
- [ ] `plans/README.md` marks Plan 006 `DONE`

## STOP Conditions

- A supported real graph exceeds the proposed decoded budget; report encoded
  size and measured peak RSS before revising the ceiling.
- Supporting current CPG2 version 1 requires retaining an unbounded decoder.
- A checksum coverage change would make existing version-1 files ambiguous
  rather than dispatchable by version.
- Cross-platform replacement requires deleting the old destination first.
- The work requires changing graph semantics outside persistence.

## Maintenance Notes

- Any new persisted column or passthrough kind must update the encoded lower
  bound, decoded-memory estimate, integrity fixtures, and malformed tests.
- Review allocation order, not just eventual errors.
- Cache shape/version and wire-format version are separate concepts even when
  one change requires bumping both.
