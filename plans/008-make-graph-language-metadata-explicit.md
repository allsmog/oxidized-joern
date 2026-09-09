# Plan 008: Make persisted graph language metadata explicit

> **Executor instructions**: Execute steps in order and run each verification.
> Stop on a STOP condition rather than inventing compatibility behavior. Update
> this plan's status row in `plans/README.md` when complete unless a reviewer
> owns the index.
>
> **Drift check (run first)**:
> `git diff --stat 025d9778c..HEAD -- cpg-rs/cpg-cli/src cpg-rs/cpg-cli/tests cpg-rs/cpg-incremental/src`
> STOP if loaded-project APIs, cache manifests, MCP merge, or language selection
> were independently redesigned.

## Status

- **Status**: TODO
- **Priority**: P0
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: `plans/006-complete-cpg-persistence-safety.md`, `plans/012-close-filesystem-boundary-residuals.md`
- **Category**: correctness / performance / architecture
- **Planned at**: commit `025d9778c`, 2026-08-18
- **Supersedes residuals from**: `plans/003-content-correct-cache-and-reopen.md`

## Why this matters

The cache records language correctly, but callers resolve that metadata after
some language-dependent choices have already been made. A non-C loaded graph
can therefore receive the C default rule pack or C Flatgraph metadata. Merged
graphs discard language identity entirely despite being advertised as directly
scannable. One validated graph-open context must own graph bytes, digest, and
single/mixed language mode so every caller makes the same safe decision.

## Current State

- `cpg-cli/src/lib.rs:293-303` resolves adjacent cache metadata correctly inside
  `open_project`, but returns only `Project`, hiding the resolved language.
- `cpg-cli/src/main.rs:1041-1055` selects a built-in scan pack first and defaults
  to C. `main.rs:613-621` separately defaults Joern export metadata to C.
- `cpg-cli/src/mcp.rs:447-507` merges graphs but discards each input language and
  writes no adjacent metadata. Its descriptor at `:577-583` says the returned
  path is scannable through the `cpg` argument.
- `workspace.rs:641-667` requires explicit language when adjacent cache metadata
  is absent, so merge-to-scan without `lang` currently fails.
- Cache freshness validation at `workspace.rs:580-635` hashes and decodes the
  graph. Language lookup and `open_project` repeat those passes; MCP repeats
  digest/load work again at `mcp.rs:167-178`.
- External summaries are already ordered correctly: reopen first, then load
  externals and recompute at `lib.rs:293-301`. Preserve that behavior.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---|---|---|
| CLI unit tests | `cd cpg-rs && cargo test -p cpg-cli --locked language` | exit 0 |
| Reopen tests | `cd cpg-rs && cargo test -p cpg-cli --locked reopen` | exit 0 with matching tests |
| MCP tests | `cd cpg-rs && cargo test -p cpg-cli --test mcp_stdio --locked` | exit 0 |
| CLI integration | `cd cpg-rs && cargo test -p cpg-cli --test cli_metadata --locked` | exit 0 |
| Workspace | `cd cpg-rs && cargo test --workspace --locked` | exit 0 |
| Lint/format | `cd cpg-rs && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 |
| Diff hygiene | `git diff --check` | no output |

## Scope

**In scope**:

- `cpg-rs/cpg-cli/src/lib.rs`
- `cpg-rs/cpg-cli/src/main.rs`
- `cpg-rs/cpg-cli/src/workspace.rs`
- `cpg-rs/cpg-cli/src/mcp.rs`
- `cpg-rs/cpg-cli/src/scan.rs` only to accept graph/summaries from the read-only wrapper
- Existing affected files under `cpg-rs/cpg-cli/tests/`
- `cpg-rs/cpg-incremental/src/lib.rs` for a read-only reopened project and
  explicit update refusal when no single frontend exists

**Out of scope**:

- Changing graph node/edge semantics or language frontend output
- Guessing language from arbitrary filenames
- Choosing one default built-in rule pack for a mixed-language graph
- Changing external-summary semantics
- General CPG2 decoding; Plan 006 owns it
- Distributed cache services or broad workspace cache redesign

## Git Workflow

- Use a branch such as `advisor/008-explicit-graph-language`.
- Prefer one logical commit with a message such as
  `fix: make persisted graph language metadata explicit`.
- Do not push or open a PR unless instructed.

## Steps

### 1. Define a validated loaded-graph context and language mode

Introduce a small internal type representing `Single(Language)`,
`Mixed(Vec<Language>)`, or `Unknown`. Keep `Language` as the closed parser and
canonical-name authority. Add a loaded-graph context containing the decoded
`Cpg`, content digest, path, and language mode. This context should be produced
once and consumed by `open_project`, MCP, scan, export, serve, and update paths.

For existing cache manifests, map their canonical language to `Single`. An
explicit `--lang` must agree with `Single`; it must not silently collapse
`Mixed`. An arbitrary graph with neither validated metadata nor explicit input
remains `Unknown` and must fail where a frontend or default rule pack is needed.

**Verify**: focused unit tests cover every canonical alias, agreement,
conflict, mixed, unknown, malformed metadata, and graph-digest mismatch.

### 2. Persist generic graph metadata independently of cache freshness

Add a versioned generic document at `<graph>.meta.json` for graph identity and
language mode. It must include at least metadata version, graph format/shape
version, graph digest, and sorted unique canonical languages. Keep the existing
`<graph>.manifest.json` source manifest cache-specific; do not fabricate source
entries for imports or merges.

Resolution precedence is mandatory:

1. Decode/hash the graph once.
2. If `.meta.json` exists, it must parse and match the digest; malformed or
   stale generic metadata is an error, not a reason to fall back.
3. If generic metadata is absent, a valid legacy cache manifest may provide a
   `Single` language for backward compatibility.
4. If both documents exist, their graph digest and single-language claim must
   agree; conflict is an error.
5. Explicit `--lang` may supply identity when metadata is absent or refine
   validated `Unknown` for the current operation. It must agree with `Single`,
   cannot collapse `Mixed`, and must not rewrite `Unknown` metadata implicitly.

Publish graph first and metadata last using the existing atomic-file pattern.
The metadata must never advertise an incomplete graph. Existing cache manifests
remain readable during migration; new cache builds should also publish generic
metadata so all open paths converge. User `build`, Joern import, and merge
outputs must publish metadata when their language identity is known.

Do not accept metadata whose digest does not match the graph. Do not use mtimes
for identity.

**Verify**: `cd cpg-rs && cargo test -p cpg-cli --locked graph_metadata` exits 0
for single, mixed, unknown, corrupt, stale, missing-half, and interrupted-publish
cases.

### 3. Resolve language before selecting scan or export behavior

Change `open_project` to return a small `OpenedGraph` wrapper (name may vary)
with two variants: `Single { project, language }` and `ReadOnly { cpg,
summaries, mode }`. Keep `Project::build` and its `BuildStats` signature
unchanged. Move/readjust scan helpers so custom-rule scan, query, and export can
consume `&Cpg` plus `&SummaryStore` from either variant. Expose update/build only
on `Single`; commands receiving `ReadOnly` return an actionable
`FullRebuildRequired`/read-only error before calling `Project`. Do not construct
a fake C frontend for `Mixed` or unresolved `Unknown`.

In `scan_cmd`, resolve/open first, then choose a built-in pack.
A built-in pack requires `Single`; `Mixed` requires an explicit rules file and
`Unknown` requires explicit language or rules as appropriate. Custom rules must
continue to work without pretending the graph is C.

In `export_joern_cmd`, remove the caller-level C default for loaded graphs.
Use the recorded single language. For `Mixed`, fail with an actionable message
unless the caller explicitly supplies the export language and the command
documents that this controls Joern metadata only. Never silently emit C.

Apply the same ordering to every other caller that selects frontend/rules from
`--lang` before opening the graph.

**Verify**: black-box tests load a Python cache without `--lang` and prove the
Python pack/Flatgraph metadata is selected; conflicting and mixed cases fail
without producing output artifacts.

### 4. Preserve and enforce merged-graph language mode

During MCP and CLI merge, collect each validated input mode. Save `Single` when
all inputs have the same language and `Mixed` for more than one. Reject unknown
input identity unless the caller supplied and validated it. For direct
`cpg merge`, add one optional `--lang <language>` applying only to metadata-free
inputs; every metadata-bearing input must still agree with its own record. If
metadata-free inputs need different languages, require callers to create
metadata first rather than adding positional inference. MCP workspace inputs
already return validated language and need no new argument. Publish metadata
alongside the merged graph.

The documented MCP merge-to-scan workflow must succeed for a single-language
merge without an extra `lang`. A mixed merge must scan with explicit rules but
must not select one language's built-in pack. Incremental update must return a
typed/actionable refusal for mixed or unknown reopened graphs rather than
running one frontend over another language's files.

**Verify**: MCP stdio tests cover same-language merge-to-scan, mixed merge with
custom rules, mixed default-pack rejection, metadata tampering, and update
refusal.

### 5. Reuse one validated decode per operation

Return the already decoded `Cpg` and computed digest from graph validation.
Remove redundant `graph_content_digest`, `Cpg::load`, and manifest-validation
passes in `language_for_cpg`, `open_project`, and MCP. Keep MCP's in-memory
freshness behavior by comparing the validated digest before replacing its
cached project.

Add test instrumentation or a reader abstraction proving one logical open does
one decode. Do not weaken digest verification merely to improve speed.

**Verify**: a focused test asserts one validation/decode on a cache hit and MCP
reuse, while stale graph bytes still invalidate the in-memory project.

## Test Plan

- Model metadata corruption/publication tests on existing workspace manifest
  tests in `cpg-cli/src/workspace.rs`.
- Add black-box CLI tests, not just helpers, for default scan pack and Joern
  export metadata.
- Add MCP end-to-end tests for returned merge paths.
- Preserve direct-versus-reopened external-summary equivalence.
- Test generic user graphs without adjacent metadata still require explicit
  language where frontend semantics are needed.

## Done Criteria

- [ ] Every loaded graph has validated `Single`, `Mixed`, or `Unknown` mode
- [ ] No loaded command defaults language-dependent behavior to C
- [ ] Generic graph metadata is digest-bound and atomically published
- [ ] Same-language merge-to-scan works without undocumented arguments
- [ ] Mixed graphs cannot select a built-in pack or incremental frontend silently
- [ ] Each open operation hashes/decodes the graph only once
- [ ] External summaries remain equivalent across source build and reopen
- [ ] Full workspace, formatting, and locked Clippy pass
- [ ] Only in-scope files changed
- [ ] `plans/README.md` marks Plan 008 `DONE`

## STOP Conditions

- Joern Flatgraph cannot represent mixed metadata and a supported workflow
  requires silently labelling mixed output as one language.
- Existing external consumers require filename-based language inference.
- Generic metadata publication cannot be made graph-digest-consistent with the
  existing atomic primitives.
- Correctness requires changing frontend graph semantics.
- A valid workflow depends on updating a mixed graph with one frontend.

## Maintenance Notes

- Cache source freshness and generic graph identity are separate concerns; do
  not merge them back into one ambiguous document.
- Any new graph-producing command must publish language metadata or explicitly
  produce `Unknown`.
- Review caller ordering: opening/resolving must happen before rule, frontend,
  or export-language selection.
