# Plan 003: Make cache reuse and graph reopening content-correct

> **Historical plan — do not execute on current HEAD.** The main implementation
> landed in commit `2c83a8a4f`. A 2026-08-18 reconciliation found remaining
> loaded-language, merged-graph metadata, and duplicate validation gaps; those
> are tracked in `plans/008-make-graph-language-metadata-explicit.md`.

> **Executor instructions**: Follow this plan step by step. Run every
> verification command. If a STOP condition occurs, stop and report; do not
> improvise. When done, update `plans/README.md` unless a reviewer dispatched
> you and told you to skip that index edit.
>
> **Drift check (run first)**:
> `git diff --stat 6913b3ac1..HEAD -- cpg-rs/cpg-cli/src cpg-rs/cpg-incremental/src cpg-rs/cpg-cli/tests cpg-rs/Cargo.toml cpg-rs/Cargo.lock`
> This plan depends on Plans 001 and 002. Run it only from a commit containing
> their approved changes. Reconcile names/signatures against those changes; STOP
> if either dependency is absent.

## Status

- **Status**: STALE (implemented historically; superseded by Plan 008)
- **Depends on**: `plans/001-fail-closed-filesystem-boundary.md`,
  `plans/002-harden-cpg-persistence.md`
- **Planned at**: commit `6913b3ac1`, 2026-08-13
- **Finding**: CORRECTNESS-01 / CORRECTNESS-03

## Why this matters

Workspace cache freshness currently asks only whether a surviving source has an
mtime newer than the `.cpg`. Deleted/renamed files, timestamp-preserving edits,
and path-key collisions can reuse a graph for different or stale code. That can
make `cpg x` and MCP report findings for code that no longer exists or miss new
code. Reopening has a separate semantic defect: external summaries are loaded
before `Project::reopen`, which immediately replaces the summary store, so
`scan --load ... --summaries ...` silently loses declared library behavior.

After this plan, cache identity is based on canonical path plus a sorted content
manifest, cache publication couples graph and manifest safely, arbitrary loaded
graphs cannot silently assume the C frontend, and external summaries behave the
same for source builds and reopened graphs.

## Context and evidence

- `cpg-rs/cpg-cli/src/workspace.rs:86-97`: cache keys replace path separators
  with `_`, allowing distinct paths such as `a/b_c` and `a_b/c` to collide.
- `cpg-rs/cpg-cli/src/workspace.rs:100-128`: freshness is only `newest source
  mtime > cache mtime`.
- `cpg-rs/cpg-cli/src/workspace.rs:295-317`: the scan tracks only the newest
  surviving mtime; deletion produces no invalidation.
- `cpg-rs/cpg-cli/src/workspace.rs:400-432`: tests cover reuse and a future
  mtime but not deletion, rename, preserved mtime, or collisions.
- `cpg-rs/cpg-cli/src/lib.rs:161-166`: `open_project` loads external summaries
  and then calls `reopen`.
- `cpg-rs/cpg-incremental/src/lib.rs:108-124`: `reopen` resets summaries to a
  new store before recomputing.
- `cpg-rs/cpg-cli/src/lib.rs:154`: loaded graphs default to language `c` when
  `--lang` is omitted.
- `cpg-rs/cpg-cli/src/workspace.rs:167-180`: `lang_of_cpg` guesses language by
  splitting a filename and falls back to C; a generic `name.cpg` can be
  misclassified.

## Scope

### In scope

- `cpg-rs/cpg-cli/src/workspace.rs`
- `cpg-rs/cpg-cli/src/lib.rs`
- `cpg-rs/cpg-cli/src/mcp.rs`
- `cpg-rs/cpg-incremental/src/lib.rs` only if summary preservation belongs in
  `Project::reopen`; prefer ordering the caller when that is sufficient
- `cpg-rs/cpg-cli/tests/*` and focused unit tests
- `cpg-rs/cpg-cli/Cargo.toml`, workspace dependency declarations, and lockfile
  for a maintained content-digest/serialization dependency if needed

### Out of scope

- Changing analysis/frontends or graph shape
- Distributed caches or cache sharing across machines
- Peak-memory optimization during builds
- General persistence validation/atomic file primitive, provided by Plan 002

## Implementation steps

### 1. Define a deterministic source manifest and collision-resistant key

Define a serializable manifest containing at least:

- manifest schema version and CPG graph-format/shape version;
- canonical module path digest (do not expose the full absolute path in the
  filename);
- canonical language name;
- sorted root-relative source paths and a content digest for each included file;
- the effective exclude policy identifier/content so policy changes invalidate.

Use the strict source-discovery result from Plan 001 so the manifest and built
source set are identical. Use a maintained collision-resistant digest (for
example BLAKE3 or SHA-256); do not use `DefaultHasher`, mtimes, or a hand-rolled
separator replacement for identity. Serialize deterministically.

Make the cache filename include the module-path digest, canonical language, and
graph shape version. Keep any human-readable prefix short and non-authoritative.

Verification:

```sh
cd cpg-rs
cargo test -p cpg-cli --locked manifest
```

Expected: order-independent discovery yields identical manifests; content,
path, language, excludes, or shape-version changes yield different identities;
the former separator-collision pair no longer collides.

### 2. Reuse only an exact manifest match and publish graph plus metadata safely

Store deterministic metadata adjacent to each cached graph. Reuse only when:

- both graph and metadata exist and decode successfully;
- every version/language/root field matches;
- a freshly computed source manifest matches exactly;
- the CPG2 graph passes Plan 002 validation.

Missing, malformed, mismatched, orphaned, or stale metadata must trigger a
rebuild, not an error that permanently bricks the workspace. Build to temporary
paths and publish the graph before the manifest so a manifest never advertises
an incomplete graph. Use a per-cache-key lock or exclusive creation protocol so
concurrent builders cannot interleave graph/manifest pairs. A process finding a
fresh pair after acquiring the lock should reuse it rather than rebuild.

Verification:

```sh
cd cpg-rs
cargo test -p cpg-cli --locked cache
```

Expected: edits with unchanged/older mtimes, deletion, rename, exclusion-policy
change, corrupt graph, corrupt manifest, missing half of a pair, key collision,
and two concurrent builders all produce a correct complete cache result.

### 3. Make language on reopen explicit and validated

Stop inferring arbitrary `.cpg` language as C. Make filename inference return
`Option<Language>` and accept only recognized workspace-cache metadata. For a
user-supplied `--load`, either obtain language from the validated adjacent
manifest produced by this CLI or require explicit `--lang`; return an actionable
error when neither is available. Validate explicit language against manifest
language when both exist.

Merged graphs may contain multiple languages and are read-only analysis inputs;
represent that explicitly in metadata or require the caller to choose the
frontend before an incremental `update`. Never choose a bogus language from an
arbitrary output filename.

Verification:

```sh
cd cpg-rs
cargo test -p cpg-cli --locked reopen
```

Expected: a non-C cached graph reopens with its recorded language; a generic
graph without metadata requires `--lang`; conflicting explicit/recorded values
fail; incremental updates cannot run with unknown/multi-language metadata.

### 4. Preserve external summaries across reopen

Fix `open_project` so `--load` first reopens/reindexes the graph and then loads
external summaries, followed by whatever summary fixpoint is required to
compose computed and declared entries. Alternatively, change `Project::reopen`
to preserve an already-loaded external store, but do not preserve stale computed
summaries from a prior graph.

Add a regression using an external `Param(0) -> Return` summary and a tiny
source fixture. Compare source-build findings with build/save/load findings
under the same summary JSON. They must be identical and include the flow that
requires the external summary.

Verification:

```sh
cd cpg-rs
cargo test -p cpg-cli --locked external_summaries
```

Expected: direct and reopened scans produce the same finding set.

### 5. Run full gates and commit

```sh
cd cpg-rs
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cd ..
git diff --check
```

Expected: all commands exit 0. Commit with a repository-style message such as
`fix: make workspace CPG caches content-correct`.

## Test plan

- Unit-test deterministic manifest serialization and digest inputs.
- Regression-test deletion, rename, timestamp preservation, path collisions,
  corrupted/missing pair members, and concurrent cache construction.
- Test canonical language aliases, recorded language, conflict errors, generic
  load behavior, and multi-language update refusal.
- Test source-build versus save/load equivalence with external summaries.
- Preserve fast unchanged-cache reuse and ordinary MCP workspace behavior.

## Done criteria

- [ ] Cache identity cannot collide through separator replacement
- [ ] Reuse requires exact content/path/language/exclude/version manifest match
- [ ] Deleted, renamed, or timestamp-preserved changes invalidate correctly
- [ ] Concurrent/cache-crash states cannot publish or reuse a torn pair
- [ ] Arbitrary graph loads never silently select C
- [ ] External summaries survive reopen and produce equivalent findings
- [ ] Full formatting, locked Clippy, and locked workspace tests pass
- [ ] Only in-scope files changed
- [ ] `plans/README.md` status row updated, unless reviewer told executor to skip

## STOP conditions

- Plan 001's strict discovery result or Plan 002's safe/atomic persistence is
  absent from the executor base commit.
- The repository has a documented requirement to keep cache filenames stable
  across the shape-version bump; report it before creating an implicit migration.
- A language cannot be represented for merged graphs without changing graph
  analysis semantics; keep merge read-only and report the limitation.
- Correct cache publication requires a new cross-process service or database.

## Notes for the reviewer

Verify the digest covers the exact bytes passed to frontends after exclusions
and generated-Go filtering. Hashing a broader or narrower set than the builder
reintroduces false reuse/rebuild behavior. Also inspect the lock failure and
stale-lock policy for bounded recovery.
