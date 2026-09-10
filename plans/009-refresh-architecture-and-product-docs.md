# Plan 009: Make architecture and product documentation truthful

> **Executor instructions**: This is a documentation-contract change, not a
> license to alter runtime behavior. Verify every claim against code or an
> executable acceptance gate. Stop if product direction is ambiguous rather
> than choosing a new mission implicitly. Update `plans/README.md` when done.
>
> **Drift check (run first)**:
> `git diff --stat 025d9778c..HEAD -- README.md cpg-rs/README.md cpg-rs/GOAL.md cpg-rs/ROADMAP.md cpg-rs/PROGRESS.md cpg-rs/ARCHITECTURE.md cpg-rs/ARCHITECTURE_GAPS.md cpg-rs/COMPATIBILITY.md cpg-rs/REPLACEMENT_CONTRACT.md cpg-rs/acceptance .github/workflows`
> Then run `git diff --stat -- README.md cpg-rs/README.md cpg-rs/GOAL.md cpg-rs/ROADMAP.md cpg-rs/PROGRESS.md cpg-rs/ARCHITECTURE.md cpg-rs/ARCHITECTURE_GAPS.md cpg-rs/COMPATIBILITY.md cpg-rs/REPLACEMENT_CONTRACT.md .github/workflows`.
> The planning worktree already had an unstaged `ARCHITECTURE_GAPS.md` change at
> lines 70-77; preserve and reconcile it rather than overwriting it. STOP if any
> other pre-existing worktree change lacks clear ownership, or if a later commit
> established different authoritative documents/product scope.

## Status

- **Status**: TODO
- **Priority**: P1
- **Effort**: M
- **Risk**: LOW
- **Depends on**: Plans 006, 007, 008, 010, and 012
- **Category**: docs / architecture / dx
- **Planned at**: commit `025d9778c`, 2026-08-18

## Why this matters

Current documents describe mutually exclusive goals and different product
states. `GOAL.md` asks for universal byte-identical Joern parity while the
current roadmap deliberately excludes a Scala console, universal CPGQL, and a
drop-in claim. `ARCHITECTURE_GAPS.md` calls shipped query/Flatgraph work pending
and labels unused prototypes as query/storage architecture. Contributors and
agents need one current scope, one acceptance contract, and clearly archived
history.

## Current State

- Root `README.md:14-22` reports 96 graph blocks, 1,458 flow facts, and all
  non-C frontends as experimental. `cpg-rs/README.md:13-22` and
  `COMPATIBILITY.md:17-38` report 122 blocks, 1,961 facts, and eight non-C
  production-preview contracts.
- `GOAL.md:10-14` defines universal 1:1 byte identity as the mission.
  `ROADMAP.md:59-65` explicitly rejects universal parity, full CPGQL semantics,
  Scala console, and JVM plugin recreation.
- `PROGRESS.md:7-41` records current completed tracks, but `:122-173` still says
  to start already-completed M2/M6/M7 work.
- `ARCHITECTURE.md:246-284` describes old frontend counts, linear CFG, and
  unfinished work that later commits replaced.
- `ARCHITECTURE_GAPS.md:46-79` describes the query compiler as a skeleton,
  Flatgraph export as pending, and dynamic languages as broadly staged.
- `FrozenCpg`, `SegmentManifest`, `RelationStore`/provenance, and
  `ScanSubscription` have definitions/re-exports but no production callers.
- At planned commit `025d9778c`, production C imports authoritative DDG from
  `cpg-lang-c/src/exact.rs` and `import.rs`, while the shared
  `cpg_analysis::ReachingDefPass` is not independently gated. This plan depends
  on Plan 010; the executor must replace this historical fact with Plan 010's
  completed independent-gate evidence rather than repeating it as current.

## Authoritative Document Roles

The rewrite must use these roles unless the maintainer explicitly rejects them:

- `COMPATIBILITY.md`: public language/workflow scope and deliberate exclusions
- `REPLACEMENT_CONTRACT.md`: executable evidence required for release claims
- `ROADMAP.md`: current future work and non-goals
- `PROGRESS.md`: dated historical milestone log, not current task instructions
- `ARCHITECTURE.md`: current production architecture and measured tradeoffs
- `ARCHITECTURE_GAPS.md`: only verified active gaps, classified by maturity

## Commands You Will Need

| Purpose | Command | Expected on success |
|---|---|---|
| Count source CPGQL cases | `jq '[.tiers[].cases[]] | length' cpg-rs/acceptance/cpgql/catalog.json` | current catalog count |
| Count populated cases | `jq '[.tiers[].cases[]] | length' cpg-rs/acceptance/cpgql/positive.json` | current catalog count |
| Committed acceptance | `cd cpg-rs && ./joern-parity/check.sh --committed-only && ./acceptance/languages/check.sh --committed-only && ./acceptance/cpgql/check.sh --committed-only` | all pass |
| Docs consistency | `python cpg-rs/scripts/check-doc-contract.py` | exit 0 |
| Current-task wording | `rg -n 'start here|Always push' cpg-rs/{GOAL,PROGRESS}.md` | no current instruction; historical matches are explicitly labelled |
| Prototype callers | `rg -n 'FrozenCpg|SegmentManifest|RelationStore|ScanSubscription' cpg-rs --glob '*.rs'` | only definitions, re-exports, or tests until evidence says otherwise |
| Diff hygiene | `git diff --check` | no output |

## Scope

**In scope**:

- `README.md`
- `cpg-rs/README.md`
- `cpg-rs/GOAL.md`
- `cpg-rs/ROADMAP.md`
- `cpg-rs/PROGRESS.md`
- `cpg-rs/ARCHITECTURE.md`
- `cpg-rs/ARCHITECTURE_GAPS.md`
- `cpg-rs/COMPATIBILITY.md` and `REPLACEMENT_CONTRACT.md` only to correct
  unsupported evidence claims, not expand product scope
- New `cpg-rs/scripts/check-doc-contract.py`
- `.github/workflows/release-contract.yml` and the reusable contract workflow
  created by Plan 007, if separate, to invoke the script in `committed-parity`
  or another existing cheap prerequisite

**Out of scope**:

- Runtime code changes
- Promoting a language or compatibility surface without new executable evidence
- Restoring the Scala console, JVM plugins, or universal CPGQL
- Claiming whole-project Joern differential evidence for native-only zlib/Lua
  workflow checks
- Rewriting historical plan files beyond links/status references

## Git Workflow

- Use a branch such as `advisor/009-truthful-architecture-docs`.
- Commit as one documentation contract change, for example
  `docs: reconcile architecture and replacement scope`.
- Do not push or open a PR unless instructed.
- If the operator cannot run CI, leave the plan `BLOCKED (awaiting operator CI)`
  rather than marking the CI done criterion complete.

## Steps

### 1. Establish one current mission and archive obsolete instructions

Make the bounded native replacement described by `COMPATIBILITY.md` and
`REPLACEMENT_CONTRACT.md` the explicit current mission. Convert `GOAL.md` into
clearly labelled historical context or move its still-useful parity methodology
under an archived heading. Remove instructions that tell autonomous sessions to
push or restart completed milestones.

Convert the stale "Next task" portion of `PROGRESS.md` into historical dated
material. Keep useful investigation notes, but nothing below the current state
may masquerade as the present starting task.

**Verify**: run the Current-task wording command. Any match must sit under an
explicit `Historical`/`Archived` heading; otherwise the step fails.

### 2. Synchronize public status from executable evidence

Update the root README to match the authoritative matrix without copying
fast-changing numeric counts where a link is clearer. Correct the description
of promoted non-C languages. Preserve the prominent limitation that this is not
a universal drop-in Joern replacement.

Correct `REPLACEMENT_CONTRACT.md` wording that calls zlib/Lua native workflow
and incremental-equivalence tests zero-diff Joern differentials. Keep their real
value: immutable sources, deterministic outputs, security findings, resource
budgets, and clean-rebuild equivalence.

**Verify**: `python cpg-rs/scripts/check-doc-contract.py` exits 0 and reports the
derived catalog counts it checked.

### 3. Rewrite the architecture map by maturity

Organize `ARCHITECTURE_GAPS.md` into:

- production-used and release-gated;
- implemented prototypes with no production consumer;
- active gaps with executable acceptance targets;
- deliberate non-goals.

Classify Frozen CSR, segments, relations/provenance, and scan subscriptions as
prototypes. State that the current query executor uses mutable `Cpg` directly
and Flatgraph export does not use freeze/segments. Describe the query and
Flatgraph contracts as shipped within their bounded catalogs.

Describe `SparseValueFlow` accurately: it post-filters ordinary source/sink
findings on authoritative DDG, while summary generation remains name-based.
Distinguish production C's exact/imported DDG from the shared
`cpg_analysis::ReachingDefPass`; describe Plan 010 as completed independent
coverage and link only Plan 011 as the active semantic migration.

**Verify**: run the Prototype callers command, record each result in the changed
architecture map, and confirm production claims with cited `path:line` callers.

### 4. Refresh the long-form architecture document

Remove obsolete line counts, language counts, linear-CFG claims, and future
items already implemented. Keep benchmarks only with their exact fixture and
date, and do not generalize synthetic measurements into production capacity.

Document actual production paths: canonical C project build/import, generic
frontends, CPG2 persistence, native query execution, Flatgraph conversion,
hybrid taint/DDG behavior, and cache manifests. Link to acceptance scripts
instead of duplicating mutable totals.

**Verify**: `python cpg-rs/scripts/check-doc-contract.py` exits 0 and every
production component named in `ARCHITECTURE.md` has an inline `path` link to its
implementation or acceptance gate.

### 5. Add a small documentation-contract check

Create `scripts/check-doc-contract.py` to derive stable counts/status markers
from acceptance JSON and detect known contradictory phrases or stale hard-coded
counts in public docs. Keep it narrow: this is not a prose linter. Invoke it in
an existing cheap committed-contract job.

At minimum, check that public language statuses match the compatibility matrix,
catalog counts used in release claims match JSON, and deliberate non-goals are
not contradicted by current-mission wording.

**Verify**: the script exits 0 on the reconciled tree and fails after a
temporary one-count/status mismatch. Revert the temporary mismatch.

### 6. Obtain operator-owned CI proof

Ask an authenticated operator to run the branch/PR after local review. Record a
run link showing the cheap prerequisite executes `check-doc-contract.py` and
the required `linux-package` context succeeds for the same commit. The executor
must not push unless instructed and must not mark this plan done without the
run link.

**Verify**: the recorded run shows the documentation-contract step executed
(not skipped) and exited successfully for the implementation SHA.

## Test Plan

- Run committed parity/language/CPGQL scripts to ensure documented evidence is
  still executable.
- Run the new docs-contract script directly and in CI.
- Manually trace production/prototype classifications through call-site search.
- Check all relative Markdown links in changed documents with the repository's
  available link checker, or a small local path-only check if none exists.

## Done Criteria

- [ ] One current bounded mission is explicit; obsolete goals are historical
- [ ] Root and workspace READMEs agree with `COMPATIBILITY.md`
- [ ] `PROGRESS.md` contains no stale current-task instruction
- [ ] Architecture separates production, prototype, active gap, and non-goal
- [ ] Shared versus production ReachingDef paths are described accurately
- [ ] Flatgraph, CPGQL, and language status claims match executable contracts
- [ ] Native zlib/Lua gates are not mislabeled as Joern differentials
- [ ] Docs-contract script passes and runs in CI
- [ ] Operator supplied a successful CI run link for the docs-contract step
- [ ] Only in-scope files changed and `git diff --check` is clean
- [ ] `plans/README.md` marks Plan 009 `DONE`

## STOP Conditions

- Maintainers have not decided between universal Joern parity and the bounded
  native replacement as current product direction.
- An advertised production capability lacks any executable committed gate.
- Correcting a claim would require changing runtime behavior in this plan.
- A compatibility count cannot be derived reliably from committed evidence.

## Maintenance Notes

- Prefer links and generated checks over repeating fast-changing counts.
- A prototype becomes production architecture only after a caller, equivalence
  test, and relevant performance/correctness gate exist.
- Update compatibility, replacement contract, and public README together when a
  language or workflow changes status.
