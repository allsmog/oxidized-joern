# Plan 001: Fail closed at the source and MCP filesystem boundary

> **Historical plan — do not execute on current HEAD.** The main implementation
> landed in commit `93290be8b`. A 2026-08-18 reconciliation found only residual
> path-identity and command-boundary test gaps; those are tracked in
> `plans/012-close-filesystem-boundary-residuals.md`.

> **Executor instructions**: Follow this plan step by step. Run every
> verification command. If a STOP condition occurs, stop and report; do not
> improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer dispatched you and told you to skip that
> index edit.
>
> **Drift check (run first)**:
> `git diff --stat 6913b3ac1..HEAD -- cpg-rs/cpg-cli/src cpg-rs/cpg-cli/tests`
> If any in-scope file changed, compare the diff with the facts and call sites
> below. Continue only when the described defects still exist and the named
> APIs have not materially changed; otherwise STOP and report the drift.

## Status

- **Status**: STALE (implemented historically; superseded by Plan 012)
- **Depends on**: none
- **Planned at**: commit `6913b3ac1`, 2026-08-13
- **Finding**: SECURITY-01 / CORRECTNESS-02

## Why this matters

The CLI is a security-analysis tool, so a successful command must mean it
analyzed the requested files with the requested frontend. Today an unknown
language silently selects C, missing or unreadable directories silently yield
no files, matching files that cannot be read as UTF-8 are silently discarded,
and directory symlinks are followed without a root or cycle policy. A scan can
therefore succeed with an empty or incomplete graph and falsely reassure the
caller.

The MCP workspace has a related trust-boundary defect. It describes module
paths as root-relative, but `Workspace::module_dir` accepts absolute paths,
`..`, and symlinks resolving outside the root. The state-changing `merge` tool
also interpolates unrestricted `out_name` text into a cache path and then saves
there. An MCP caller can read outside the declared workspace and can direct a
`.cpg` write outside the cache.

After this plan, language selection is closed and typed, source discovery is
complete-or-error, every traversed source remains within the requested root,
and MCP reads/writes cannot escape the workspace/cache boundary.

## Context and evidence

- `cpg-rs/cpg-cli/src/lib.rs:43-90`: `make_project` returns the C frontend from
  a catch-all `_` arm. This arm currently covers both `c` and every typo.
- `cpg-rs/cpg-cli/src/lib.rs:185-213`: `read_dir` failures return silently,
  `entries.flatten()` drops entry errors, `path.is_dir()` follows symlinks, and
  failed `read_to_string` calls are ignored.
- `cpg-rs/cpg-cli/src/lib.rs:107-125`: `build_project_ext` reports how many
  sources were built but cannot return discovery errors or reject zero files.
- `cpg-rs/cpg-cli/src/main.rs:311-327`: `cpg build` saves and reports success
  after the tolerant build path.
- `cpg-rs/cpg-cli/src/workspace.rs:70-84`: `module_dir` canonicalizes the joined
  path but does not require the result to start with the canonical workspace
  root.
- `cpg-rs/cpg-cli/src/mcp.rs:451-504`: `merge_tool` accepts unrestricted
  `out_name`, joins `format!("{out_name}.cpg")` to the cache, and saves it.
- `cpg-rs/cpg-cli/tests/mcp_stdio.rs`: the current end-to-end MCP coverage has
  ordinary in-root cases but no traversal, absolute-path, symlink-escape, or
  output-name cases.
- Existing error style: public CLI helpers return `Result<_, String>` where an
  operation can fail (`open_project`), command boundaries print the error and
  exit non-zero, and MCP tool handlers convert failures into JSON-RPC errors.
- Language aliases currently supported by the CLI must remain supported:
  `c`, `python`, `java`, `go`, `javascript|js`, `ruby|rb`, `rust|rs`, `scala`,
  `typescript|ts`, and `cpp|c++|cxx`.

## Scope

### In scope

- `cpg-rs/cpg-cli/src/lib.rs`
- `cpg-rs/cpg-cli/src/main.rs`
- `cpg-rs/cpg-cli/src/workspace.rs`
- `cpg-rs/cpg-cli/src/mcp.rs`
- Existing `cpg-rs/cpg-cli/src/*` call sites that must propagate the new
  fallible APIs
- Existing `cpg-rs/cpg-cli/tests/*` call sites and new boundary/regression tests

### Out of scope

- Changing graph shape, language frontend semantics, rule packs, taint logic,
  or Joern parity output
- Changing workspace cache freshness or cache-key design; that is Plan 003
- Changing the `.cpg` binary format or atomic-save implementation; that is
  Plan 002
- Adding a permissive `--allow-partial` mode. Strict behavior is the initial
  production contract; a tolerant mode requires separate product design.

## Implementation steps

### 1. Replace catch-all language selection with a closed language parser

Create a small public language type in `cpg-cli/src/lib.rs` (an enum or an
equivalent closed representation) that:

- parses only the aliases listed in Context;
- returns an error such as `unsupported language 'pyhton'` for everything else;
- exposes one canonical name, owned extensions, and the corresponding frontend
  constructor;
- represents C explicitly rather than through a catch-all branch.

Change `make_project` to return `Result<(Project, extensions), String>` (or make
it accept the already-validated type), and make `build_project`,
`build_project_filtered`, and `build_project_ext` fallible. Update every CLI,
workspace, MCP, unit-test, and integration-test call site. Do not retain any
fallback to C for an unrecognized string.

Verification:

```sh
cd cpg-rs
cargo test -p cpg-cli --locked language
```

Expected: tests cover every documented alias, canonicalization, explicit C,
and at least two invalid spellings; all pass.

### 2. Make source discovery root-confined and complete-or-error

Replace the `out: &mut Vec<_>` recursion with a fallible discovery operation.
It must:

- canonicalize and validate the requested root as an existing directory;
- return an error for `read_dir`, directory-entry, metadata, and matching-file
  read/UTF-8 failures, with the affected path in the message;
- maintain a visited set of canonical directories so symlink cycles terminate;
- canonicalize followed symlink targets and reject any target outside the
  canonical requested root;
- never follow a non-source symlink merely because its target happens to be a
  directory outside the root;
- de-duplicate canonical matching files and produce deterministic path order;
- preserve the existing exclude filters and Go-generated-file policy;
- return an error when zero matching readable source files remain.

Do not silently print a warning and continue: if an in-scope matching source
cannot be accounted for, the operation fails. Return structured information
internally if useful, but keep concise human-readable error strings at the
current CLI boundary.

Verification:

```sh
cd cpg-rs
cargo test -p cpg-cli --locked source
```

Expected: tests cover a missing root, an empty root, an unreadable or invalid
UTF-8 matching source, deterministic ordering, an in-root symlink, an
out-of-root symlink, and a directory-symlink cycle. On platforms where file
permissions cannot reliably create an unreadable file, gate only that assertion
with an explicit platform/permission check; do not omit the other cases.

### 3. Propagate strict failures through every command surface

Update `build`, source-based `scan`, `serve`, `slice`, `apis`, `export`, `flow`,
`vectors`, workspace build/reuse, and MCP project loading so invalid languages
and source-discovery failures return non-zero CLI status or a JSON-RPC tool
error. No command may save a graph or report zero findings after discovery
failed or found zero matching files.

Keep errors on stderr for the CLI and do not emit a success artifact. Preserve
the existing successful output formats exactly.

Verification:

```sh
cd cpg-rs
cargo test -p cpg-cli --locked
```

Expected: all existing tests plus new black-box invalid-language, missing-root,
and empty-root command tests pass.

### 4. Enforce workspace and cache containment for MCP operations

In `Workspace::module_dir`:

- reject absolute input and any non-normal relative component (`..`, root, or
  platform prefix); `.` remains the only whole-root spelling;
- canonicalize the resolved directory;
- require the canonical path to be the root or `starts_with` the canonical
  root, so outward-pointing symlinks are rejected.

Create a centralized workspace method for merge output paths. Validate
`out_name` as a single filename identifier: non-empty, not `.` or `..`, and no
path separator, root, prefix, or NUL component. Ensure the cache directory is
canonical after creation, ensure any existing target is a regular non-symlink
file, and return a path whose parent is exactly that cache. Use this method from
both MCP merge and any equivalent `cpg x merge` path instead of joining raw
input.

Verification:

```sh
cd cpg-rs
cargo test -p cpg-cli --locked workspace
cargo test -p cpg-cli --test mcp_stdio --locked
```

Expected: relative traversal, absolute paths, outward symlinks, `out_name`
values containing `/` or `\\`, `.`/`..`, and a symlink output target are all
rejected; ordinary nested in-root modules and safe output names still work.

### 5. Run the full repository gate and commit

```sh
cd cpg-rs
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cd ..
git diff --check
```

Expected: every command exits 0. Review `git diff --stat` and confirm every
changed file is in Scope. Commit the logical unit with a repository-style
message such as `fix: fail closed at analysis filesystem boundaries`.

## Test plan

- Unit-test language aliases and invalid values at the parser, not only through
  a command wrapper.
- Unit-test source discovery for success, zero files, missing directory,
  decoding failure, deterministic ordering, symlink escape, and symlink cycle.
- Unit-test `Workspace::module_dir` and merge-output validation on separator,
  absolute, traversal, and symlink cases.
- Add black-box command tests proving invalid input exits non-zero and does not
  create the requested output graph.
- Add MCP stdio tests proving traversal attempts are tool errors and no file is
  created outside the cache.
- Preserve successful build and MCP merge tests to avoid over-restriction.

## Done criteria

- [ ] Unknown `--lang` values fail; explicit `c` and all documented aliases work
- [ ] Missing, empty, unreadable, invalid-UTF-8, or escaped source inputs cannot
      produce a successful graph/scan
- [ ] Source discovery is deterministic, cycle-safe, and root-confined
- [ ] MCP module reads cannot escape the declared workspace root
- [ ] MCP and `cpg x` merge outputs cannot escape or traverse the cache
- [ ] No success artifact is created after a boundary failure
- [ ] Formatting, locked Clippy, and locked workspace tests pass
- [ ] Only in-scope files changed
- [ ] `plans/README.md` status row updated, unless reviewer told executor to skip

## STOP conditions

- The current API has already been replaced by a typed language/discovery layer
  on the executor base commit; report the drift instead of building a second one.
- Correct confinement would require accepting arbitrary absolute module paths,
  contradicting the MCP descriptor's root-relative contract.
- A required valid workflow demonstrably depends on silently accepting zero
  source files; report that workflow and do not add an implicit exception.
- More than the scoped `cpg-cli` crate must change to propagate these errors.

## Notes for the reviewer

Pay special attention to TOCTOU gaps around symlinks and the merge output. This
plan promises path-policy confinement for the current local process; it does
not claim a hostile same-user process cannot swap filesystem entries between
validation and open. Plan 002's transactional write reduces the output side of
that risk.
