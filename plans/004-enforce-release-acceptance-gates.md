# Plan 004: Enforce semantic and packaged-binary release gates

> **Historical plan — do not execute on current HEAD.** The main implementation
> landed in commit `a01efb67a`. The acceptance contract subsequently expanded.
> Remaining tag-publication, live-oracle, artifact-integrity, and container
> preflight work is tracked in
> `plans/007-bind-release-to-verified-contract.md`.

> **Executor instructions**: Follow this plan step by step. Run every
> verification command. If a STOP condition occurs, stop and report; do not
> improvise. When done, update `plans/README.md` unless a reviewer dispatched
> you and told you to skip it.
>
> **Drift check (run first)**:
> `git diff --stat 6913b3ac1..HEAD -- .github/workflows cpg-rs/scripts cpg-rs/joern-parity`
> If the required check name, release job topology, packaging script, or parity
> harness changed, reconcile this plan against the current files before editing.

## Status

- **Status**: STALE (implemented historically; superseded by Plan 007)
- **Depends on**: none (rerun after Plans 001-003 to exercise their new cases)
- **Planned at**: commit `6913b3ac1`, 2026-08-13
- **Finding**: CI-01 / TEST-01 / RELEASE-01 / TEST-02

## Why this matters

The protected branch currently requires the `linux-package` context from the
release-contract workflow. That job builds, packages, verifies a checksum, and
runs container `--version`, but it does not test, lint, format-check, audit, or
exercise a real analysis workflow. Those checks exist elsewhere in a
path-filtered workflow and are not the protected required context. A PR can
therefore satisfy the merge gate while the Rust workspace tests fail.

The exact 96-block committed C parity oracle is manual-only even though using
the already committed oracle does not require downloading Joern. Release assets
on Linux, macOS, and Windows are only asked to print their version. Finally, any
matching `v*` tag starts publishing without proving the tag commit belongs to
the protected branch.

After this plan, the stable required context depends on semantic Rust gates,
the committed parity regression runs automatically, every packaged native
binary performs a compact real workflow, and release tags must identify a
reviewed protected-branch commit.

## Context and evidence

- `.github/workflows/release-contract.yml:20-55`: required `linux-package` has
  installer syntax, release build/archive/checksum, Docker build, and
  `--version` only.
- `.github/workflows/cpg-rs.yml:24-80`: tests, fmt/Clippy, and dependency audit
  are separate jobs in a path-filtered workflow.
- `.github/workflows/cpg-rs.yml:82-110`: parity is guarded by
  `github.event_name == 'workflow_dispatch'` and downloads/caches Joern.
- `cpg-rs/joern-parity/check.sh:31-105`: ordinary comparisons run the local
  Rust dumper against committed oracle outputs; live Joern regeneration happens
  only when the oracle distribution is available.
- `.github/workflows/release.yml:3-6,24-40`: a `v*` tag is checked against the
  Cargo version but not against `origin/master` ancestry.
- `cpg-rs/scripts/package-release.py:95-111`: extracted assets run only
  `cpg --version`.
- `.github/workflows/release.yml:68-125`: five native targets use that same
  version-only package smoke test.
- Keep action pins immutable as the repository currently does; do not replace
  commit SHAs with mutable tags.

## Scope

### In scope

- `.github/workflows/release-contract.yml`
- `.github/workflows/cpg-rs.yml`
- `.github/workflows/release.yml`
- `cpg-rs/scripts/package-release.py`
- A new cross-platform acceptance script and tiny source/rule fixture under
  `cpg-rs/scripts` or `cpg-rs/tests/fixtures`
- Focused Python unit tests for packaging/acceptance helpers if appropriate

### Out of scope

- Changing GitHub branch-protection settings. Preserve the existing required
  `linux-package` job name so the current rule becomes stronger without a
  settings change.
- Downloading the approximately 2 GB Joern oracle on every PR
- Publishing, tagging, or changing release visibility
- Defining full real-project accuracy corpora; Plan 005 owns that larger gate

## Implementation steps

### 1. Make the stable required context depend on Rust quality

Restructure `release-contract.yml` so the final job named `linux-package` cannot
succeed unless the released `cpg-rs` workspace has passed:

- `cargo fmt --all --check`;
- `cargo clippy --workspace --all-targets --locked -- -D warnings`;
- `cargo test --workspace --locked`;
- `cargo audit --deny warnings --file cpg-rs/Cargo.lock` with the repository's
  pinned cargo-audit version;
- the committed parity check from Step 2.

These can be prerequisite jobs for concurrency, but the final required job name
must remain exactly `linux-package` and must use `needs` so a failed/skipped
prerequisite prevents success. The workflow must report on every PR to
`master|main`, including docs-only PRs, because a path-skipped required context
can strand merges. Use Rust 1.97.0 and locked dependencies.

Do not remove the existing `cpg-rs.yml` checks in this plan; deduplication can be
considered only after the strengthened required workflow is observed in GitHub.

Verification:

```sh
python - <<'PY'
from pathlib import Path
text = Path('.github/workflows/release-contract.yml').read_text()
for token in ['linux-package:', 'cargo fmt --all --check',
              'cargo clippy --workspace --all-targets --locked -- -D warnings',
              'cargo test --workspace --locked', 'cargo audit --deny warnings']:
    assert token in text, token
PY
```

Expected: the script exits 0, and YAML parsing/linting used by the executor also
succeeds.

### 2. Split cheap committed parity from expensive oracle regeneration

Add an automatic job/step that builds `joern-parity` and runs `check.sh` against
the committed oracle without installing Joern. Run it for relevant PRs, pushes,
and release validation. Keep a separate manual (and optionally scheduled)
oracle-integrity job that installs pinned Joern v4.0.555 and regenerates/checks
the committed oracle. Name the two jobs clearly so maintainers do not confuse
regression checking with upstream-oracle regeneration.

Ensure `check.sh` cannot silently regenerate from an unpinned/random local
oracle during the cheap job; explicitly omit/unset the oracle distribution path
or add a check-only flag.

Verification:

```sh
cd cpg-rs/joern-parity
./check.sh
```

Expected: `96/96` blocks pass using committed fixtures with no network or Joern
installation.

### 3. Add one cross-platform packaged-binary acceptance flow

Create a Python 3 script callable with `--binary <path>` that uses a temporary
directory and the extracted executable to perform, at minimum:

1. `--version` and exact version assertion;
2. build a tiny committed or generated C fixture into a `.cpg`;
3. load that graph and execute `stats` or a deterministic export;
4. scan the fixture with a tiny explicit rule pack and write SARIF;
5. parse the SARIF as JSON and assert the expected rule/finding/location;
6. pass a malformed graph and assert a controlled non-zero exit (after Plan
   002 lands; before then, at least bad-magic input must be controlled).

Invoke it from `package-release.py` after extraction, so every release target
tests the bytes users download. Avoid shell-only behavior; paths with spaces
must work on Windows. Keep fixtures tiny enough for every PR/release.

Also run the same flow inside the built release-contract container instead of
only `--version`.

Verification:

```sh
cd cpg-rs
cargo build --release --locked -p cpg-cli
python scripts/test-release.py --binary target/release/cpg --version 0.1.0
```

Expected: the real build/load/scan/SARIF flow passes and intentional malformed
input exits cleanly.

### 4. Bind release tags to protected-branch commits and gates

In `release.yml`, fetch enough history to validate ancestry and fail
`validate-tag` unless the tagged commit is reachable from `origin/master` (or
the repository's detected protected default branch). Keep the exact Cargo
version check. Add the cheap committed parity gate to the `rust` validation job
or as a hard prerequisite of every `build`/`publish` job. Ensure publish cannot
run if semantic quality, parity regression, or native packaged acceptance fails.

Do not introduce an automatic bypass. An emergency release can use a reviewed
commit on the protected branch.

Verification:

```sh
git merge-base --is-ancestor HEAD origin/master
```

Expected on the implementation base: exit 0. The workflow must perform the
equivalent check against the tag's commit, not the runner's shallow assumption.

### 5. Validate locally and commit

```sh
cd cpg-rs
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
./joern-parity/check.sh
cargo build --release --locked -p cpg-cli
python scripts/test-release.py --binary target/release/cpg --version 0.1.0
cd ..
python -m py_compile cpg-rs/scripts/package-release.py cpg-rs/scripts/test-release.py
git diff --check
```

Expected: all commands exit 0. Commit with a repository-style message such as
`ci: require semantic release acceptance`.

## Test plan

- Run the exact quality commands used by contributors.
- Run committed parity offline and prove the automatic job does not download
  Joern.
- Directly execute the packaged-binary Python acceptance on a local release
  binary.
- Exercise paths with spaces in the Python temporary directory/fixture names.
- Validate workflow YAML and inspect job dependencies so `linux-package` and
  `publish` cannot report success after a prerequisite failure.
- Let GitHub's five-platform release matrix provide the final native proof after
  merge/tag; do not fake cross-platform success locally.

## Done criteria

- [ ] Required `linux-package` cannot succeed unless fmt, Clippy, tests, audit,
      committed parity, packaging, and functional acceptance pass
- [ ] The required context reports on every PR
- [ ] Committed parity runs automatically without a Joern download
- [ ] Live pinned-oracle regeneration remains available separately
- [ ] Extracted Linux, macOS, and Windows assets execute build/load/scan/SARIF
- [ ] The container executes the same real flow
- [ ] Release tags must point to a protected-branch commit
- [ ] Publish depends on all release acceptance gates
- [ ] Local verification and `git diff --check` pass
- [ ] Only in-scope files changed
- [ ] `plans/README.md` status row updated, unless reviewer told executor to skip

## STOP conditions

- GitHub confirms the required check context is not `linux-package`; report the
  actual context before renaming/restructuring jobs.
- The committed parity check cannot run without network despite no live oracle
  regeneration; report the exact dependency instead of downloading 2 GB on PRs.
- A native platform cannot execute the shipped artifact on its runner; narrow
  only that matrix row and document why.
- The compact functional flow exceeds the existing 30/45-minute workflow
  budgets after one cache-warm and one cache-cold measurement.

## Notes for the reviewer

Required-check behavior depends on job names and skip semantics. Inspect the
actual workflow graph, not only whether commands appear in YAML. A final job
that runs under `if: always()` must explicitly fail when any prerequisite did
not succeed; otherwise it can accidentally turn red prerequisites green.
