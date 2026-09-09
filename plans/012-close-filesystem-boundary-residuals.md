# Plan 012: Close residual filesystem-boundary gaps

> **Executor instructions**: Preserve the strict fail-closed contract from Plan
> 001. Run every verification and stop if a supported workflow depends on lossy
> path conversion or partial source discovery. Update `plans/README.md` when
> complete.
>
> **Drift check (run first)**:
> `git diff --stat 025d9778c..HEAD -- cpg-rs/cpg-cli/src cpg-rs/cpg-cli/tests cpg-rs/cpg-incremental/src/lib.rs plans/001-fail-closed-filesystem-boundary.md`
> STOP if source paths are already represented losslessly or the CLI command
> surfaces were redesigned.

## Status

- **Status**: TODO
- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: `plans/006-complete-cpg-persistence-safety.md`
- **Category**: correctness / security / tests
- **Planned at**: commit `025d9778c`, 2026-08-18
- **Supersedes residuals from**: `plans/001-fail-closed-filesystem-boundary.md`

## Why this matters

Plan 001 fixed unknown-language fallback, silent discovery failures, symlink
escape, module traversal, and merge-output traversal. Canonical source and cache
paths are still converted through `to_string_lossy`, so distinct non-UTF-8 Unix
paths can collapse and one source can overwrite another in project maps. Some
strict command-boundary promises also have only helper/MCP coverage rather than
black-box coverage on every public surface.

## Current State

- `cpg-cli/src/lib.rs:667-705` collects distinct canonical `PathBuf` keys, then
  converts relative paths with `to_string_lossy` before frontends receive them.
- `cpg-incremental/src/lib.rs:159-162` collects source strings into a map keyed
  by that converted path; a collision overwrites an entry.
- `cpg-cli/src/workspace.rs:205-212` and `:310-324` hash canonical root/module
  paths after lossy conversion and repeat lossy source manifest conversion.
- `cpg-cli/tests/cli_metadata.rs:61-107` proves no output for failing `build`,
  but does not prove source-based `scan -o` leaves no SARIF artifact.
- `cpg-cli/tests/mcp_stdio.rs:173-325` covers MCP traversal and merge output,
  while the equivalent `cpg x merge` black-box path lacks matching coverage.
- MCP intentionally accepts an explicit external CPG at `mcp.rs:148-180`.
  Therefore the correct sandbox claim is root-confined module reads and
  cache-confined merge writes, not all possible MCP graph reads.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---|---|---|
| Source tests | `cd cpg-rs && cargo test -p cpg-cli --locked source` | exit 0 |
| CLI tests | `cd cpg-rs && cargo test -p cpg-cli --test cli_metadata --locked` | exit 0 |
| Workspace CLI tests | `cd cpg-rs && cargo test -p cpg-cli --test workspace_cli --locked` | exit 0 |
| MCP tests | `cd cpg-rs && cargo test -p cpg-cli --test mcp_stdio --locked` | exit 0 |
| Workspace | `cd cpg-rs && cargo test --workspace --locked` | exit 0 |
| Lint/format | `cd cpg-rs && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 |
| Diff hygiene | `git diff --check` | no output |

## Scope

**In scope**:

- `cpg-rs/cpg-cli/src/lib.rs`
- `cpg-rs/cpg-cli/src/workspace.rs`
- `cpg-rs/cpg-cli/src/main.rs`
- `cpg-rs/cpg-cli/src/mcp.rs`
- Existing tests under `cpg-rs/cpg-cli/tests/`
- New `cpg-rs/cpg-cli/tests/workspace_cli.rs`
- `plans/001-fail-closed-filesystem-boundary.md` only to narrow its historical
  confinement wording

**Out of scope**:

- Restricting the documented explicit external-CPG MCP workflow
- Supporting arbitrary non-UTF-8 paths through a new encoded public path syntax
- Relaxing UTF-8 source-content requirements
- Cache language/merge metadata; Plan 008 owns it
- Defending against a hostile same-user process swapping files after validation

## Git Workflow

- Use a branch such as `advisor/012-filesystem-residuals`.
- Commit with a focused message such as
  `fix: reject lossy analysis path identities`.
- Do not push or open a PR unless instructed.

## Steps

### 1. Reject paths that cannot be represented losslessly

At the boundary where canonical root, module, source-relative, cache, and output
paths become frontend/manifest strings, use strict `to_str` conversion and
return an error naming the affected displayed path. Do not use replacement
characters and do not silently skip the file.

Audit the identity/I/O conversions in `lib.rs`, `workspace.rs`, `main.rs`, and
`mcp.rs`, including CPG load/save calls. A lossy conversion used only after an
operation for human-readable diagnostics may remain; any conversion used to
choose a source key, digest, manifest path, or filesystem target must be strict.
Centralize the strict conversion helper so cache, MCP, and source collection
produce consistent errors.

Reject before building, saving, caching, or emitting a success report. Normal
UTF-8 paths and Windows separator normalization must remain deterministic.

**Verify**: on Unix, create two distinct source filenames containing invalid
UTF-8 bytes that would render with the same replacement text. Collection/build
must return a controlled error and create no graph. Gate only this test with
`cfg(unix)`; keep all other tests cross-platform.

### 2. Make in-root symlink coverage distinguishing

Change the in-root directory-symlink test so the source is reachable only
through the symlink under test, not simultaneously through an ordinary walked
path. Keep separate tests for cycle termination, outward directory symlinks,
outward file symlinks, and excludes applied to symlink targets.

**Verify**: `cd cpg-rs && cargo test -p cpg-cli --locked symlink` exits 0 and a
temporary implementation that ignores all directory symlinks fails the new
in-root test. Revert the temporary mutation.

### 3. Add black-box no-artifact coverage for source-based scan

Invoke the compiled CLI with invalid language, missing root, empty root, invalid
UTF-8 source content, and escaped source symlink for `scan -o`. Assert non-zero
status, a useful stderr message, and that no SARIF output exists. Preserve
successful scan output behavior.

**Verify**: `cd cpg-rs && cargo test -p cpg-cli --test cli_metadata --locked`
exits 0 with the new cases.

### 4. Cover the CLI workspace merge boundary

Create `cpg-cli/tests/workspace_cli.rs` with black-box tests for the current
`cpg x merge` command using safe
module paths plus traversal, separator, dot, directory target, and symlink
output cases. Assert the same centralized `Workspace::merge_output_path` policy
used by MCP and prove no outside file changes.

**Verify**: `cd cpg-rs && cargo test -p cpg-cli --test workspace_cli --locked`
exits 0 and both MCP and CLI merge tests reject the same invalid-name table.

### 5. Narrow the documented confinement claim

Update the historical overstatement in
`plans/001-fail-closed-filesystem-boundary.md:39-41` to state precisely that
module source reads are confined to the canonical
workspace root and merge writes to the canonical cache. Preserve the explicit
external-CPG target as an intentional read capability requiring a caller-chosen
path. Do not call MCP a complete filesystem sandbox.

**Verify**: `rg -n 'MCP reads|reads/writes cannot escape' plans cpg-rs --glob '*.md'`
returns no unqualified sandbox claim.

## Test Plan

- Unix non-UTF-8 path collision and ordinary UTF-8 success.
- Distinguishing in-root symlink, cycle, and outward escape cases.
- Black-box `scan -o` no-artifact failures.
- Black-box CLI and MCP merge policy parity.
- Full workspace regression after focused tests.

## Done Criteria

- [ ] No source/cache/CPG filesystem identity uses `to_string_lossy`; remaining
      uses are diagnostics only and reviewed as such
- [ ] Non-representable paths fail before graph or SARIF publication
- [ ] In-root symlink behavior is tested without an ordinary-path false pass
- [ ] Source-based scan failures create no success artifact
- [ ] CLI and MCP merge boundaries share black-box traversal/symlink coverage
- [ ] Documentation distinguishes module confinement from explicit CPG reads
- [ ] Full workspace, formatting, locked Clippy, and diff hygiene pass
- [ ] Only in-scope files changed
- [ ] `plans/README.md` marks Plan 012 `DONE`

## STOP Conditions

- A documented supported workflow requires non-UTF-8 source paths and no
  reversible public representation already exists.
- A frontend requires absolute rather than module-relative path identity.
- Correct handling requires weakening root confinement or partial discovery.
- The CLI workspace merge no longer shares the centralized output-path method.

## Maintenance Notes

- Path display is for diagnostics; it must never be reused as identity.
- New source-producing commands must propagate discovery errors and avoid
  publishing output after partial collection.
- Filesystem validation does not prevent same-user TOCTOU replacement; do not
  overstate the threat model in future documentation.
