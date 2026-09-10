# Plan 007: Bind every release to a verified acceptance contract

> **Executor instructions**: Follow every step and verification gate. If a STOP
> condition occurs, stop and report; do not weaken or skip a gate. When done,
> update this plan's status row in `plans/README.md` unless a reviewer owns it.
>
> **Drift check (run first)**:
> `git diff --stat 025d9778c..HEAD -- .github/workflows cpg-rs/joern-parity cpg-rs/acceptance cpg-rs/scripts`
> STOP if `linux-package` is no longer the protected required context, release
> publication topology changed materially, or oracle v4.0.555 was deliberately
> replaced without updated committed evidence.

## Status

- **Status**: TODO
- **Priority**: P0
- **Effort**: L
- **Risk**: MED
- **Depends on**: `plans/006-complete-cpg-persistence-safety.md`
- **Category**: security / tests / release
- **Planned at**: commit `025d9778c`, 2026-08-18
- **Supersedes residuals from**: `plans/004-enforce-release-acceptance-gates.md`

## Why this matters

Branch protection correctly requires `linux-package`, and the current PR gate
is broad. Tag publication does not invoke that complete contract for the exact
tag SHA, however, and the multi-architecture container is pushed only after the
GitHub release exists. Required CI also executes a downloaded Joern archive
without a committed digest, while the live C check can silently fall back to
committed output. A release claim must bind artifact identity, semantic gates,
native packages, and containers to one verified commit.

## Current State

- GitHub branch protection for `master` is strict and requires exactly
  `linux-package`; preserve that context without a settings migration.
- `.github/workflows/release-contract.yml:178-200` correctly uses `always()` and
  explicit prerequisite-result checks before `linux-package` can succeed.
- `.github/workflows/release.yml:59-86` runs Rust quality and committed C parity
  for tags, but `publish` at `:152-193` does not depend on the current live
  CPGQL, Flatgraph, language, rule, or real-project contract.
- The container job at `release.yml:194-235` needs `publish`, so it cannot block
  publication and does not run functional acceptance on the image it pushes.
- `joern-parity/setup-oracle.sh:7-17` downloads and executes the release archive
  by version only. `REPLACEMENT_CONTRACT.md` promises an artifact digest.
- `joern-parity/check.sh:27-38` falls back to `oracle_all.txt` after failed live
  regeneration. That is correct for an offline convenience mode but incorrect
  for a job named `oracle-differentials`.
- The HEAD PR contract succeeded on all current jobs. This plan strengthens
  identity and topology; it is not a response to a red baseline.

## Commands You Will Need

| Purpose | Command | Expected on success |
|---|---|---|
| Offline parity | `cd cpg-rs/joern-parity && ./check.sh --committed-only` | all committed blocks pass |
| Live parity | `cd cpg-rs/joern-parity && ./check.sh --live` | live regeneration succeeds and diff is zero |
| Package acceptance | `cd cpg-rs && cargo build --release --locked -p cpg-cli && python scripts/test-release.py --binary target/release/cpg --version 0.1.1` | acceptance passes |
| Python syntax | `python -m py_compile cpg-rs/scripts/package-release.py cpg-rs/scripts/test-release.py` | exit 0 |
| Workflow contract | `python cpg-rs/scripts/check-workflow-contract.py` | exit 0 |
| Protected context | `gh api repos/allsmog/oxidized-joern/branches/master/protection/required_status_checks --jq '.contexts[]'` | authenticated output is `linux-package` |
| Workspace quality | `cd cpg-rs && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked` | exit 0 |
| Diff hygiene | `git diff --check` | no output |

Derive the version from `cpg-rs/Cargo.toml` in committed scripts/workflows; the
literal `0.1.1` above is only the current local verification value.

## Scope

**In scope**:

- `.github/workflows/release-contract.yml`
- `.github/workflows/release.yml`
- `.github/workflows/cpg-rs.yml` only to keep manual/live behavior consistent
- One new reusable workflow under `.github/workflows/` if needed
- `cpg-rs/joern-parity/setup-oracle.sh`
- `cpg-rs/joern-parity/check.sh`
- A committed oracle checksum manifest under `cpg-rs/joern-parity/`
- `cpg-rs/scripts/package-release.py` and `test-release.py` only if required by
  container preflight reuse
- New `cpg-rs/scripts/check-workflow-contract.py`
- `CONTRIBUTING.md` and `cpg-rs/joern-parity/README.md` for exact contributor commands

**Out of scope**:

- Changing branch-protection settings or the `linux-package` context name
- Updating Joern beyond v4.0.555
- Weakening or narrowing committed acceptance catalogs
- Publishing a release or container while implementing this plan
- Adding mutable action tags; existing immutable SHA pins remain mandatory
- Optimizing away live PR gates before a separately reviewed relevance policy

## Git Workflow

- Use a branch such as `advisor/007-verified-release-contract`.
- Keep workflow refactoring and oracle hardening reviewable as separate commits
  if both remain green. Use messages such as `ci: verify the complete release contract`.
- Do not tag, publish, push, or open a PR unless instructed. If the operator does
  not provide remote verification, leave this plan `BLOCKED (awaiting operator
  CI)` rather than marking it `DONE`.

## Steps

### 1. Pin and verify the Joern oracle archive bytes

Record the SHA-256 of the exact v4.0.555 `joern-cli.zip` release asset in a
small committed manifest containing version, URL, digest algorithm, and digest.
Use an explicit trust-on-first-use review: the executor records a digest from a
clean TLS download, and a reviewer independently downloads the same pinned URL
and confirms the digest before merge. If reviewer confirmation is unavailable,
leave the plan blocked instead of describing the digest as independently sourced.
Update `setup-oracle.sh` to download with failure-reporting curl options, verify
the archive before extraction, extract into a temporary directory, launch-check
it, and atomically replace the destination.

Cache the archive, keyed by version and digest, rather than trusting an
unverifiable extracted directory. Reverify cache hits before use. Never print
credentials or relax TLS verification.

**Verify**: run setup once from an empty temporary destination and once from the
verified cached archive; both exit 0. A copied archive with one changed byte
must fail before extraction or execution.

### 2. Separate committed and mandatory-live parity modes

Keep `--committed-only` deterministic and unable to rewrite tracked output. Add
an explicit `--live` mode that requires an executable oracle, successful
regeneration, non-empty AST/NODES/EDGES/FLOWS sections, and exact comparison.
Any live regeneration failure must exit non-zero; it must never continue with
the committed file. Make default mode either print usage or retain documented
developer convenience, but CI must always pass an explicit mode.

Update `oracle-differentials` and manual oracle-integrity jobs to use `--live`.
Keep ordinary committed jobs on `--committed-only`.

**Verify**: with `JOERN` pointing at a missing directory, `./check.sh --live`
fails and `./check.sh --committed-only` still passes.

### 3. Make the complete acceptance graph reusable for an exact SHA

Extract the current semantic-quality, committed parity/rule, live oracle,
real-C, real-language, Linux package, and source/container acceptance topology
into a reusable `workflow_call` contract, or an equivalent shared topology that
does not duplicate command lists between PR and tag workflows.

The pull-request caller must retain a final job whose context is exactly
`linux-package`. It must run under `always()` and explicitly fail unless every
required reusable result is `success`. Do not replace it with a context whose
rendered GitHub name changes branch protection.

The tag workflow must invoke the same contract at `github.sha`. Native matrix
build and publication jobs must need its successful result. Ancestry and Cargo
version checks remain necessary but are not substitutes for acceptance.

Add a focused Linux/macOS/Windows job for Plan 006's `cpg-core` persistence
tests. This job must run overwrite and concurrent-writer coverage on every
supported OS while keeping Unix-only permission/directory-sync assertions
narrowly gated.

**Verify locally**: `python cpg-rs/scripts/check-workflow-contract.py` exits 0
and asserts the reusable dependencies plus caller-level `linux-package` result
check. Remote failure propagation is completed by the operator in Step 6.

### 4. Preflight containers before publishing any release artifact

Build and run `scripts/test-release.py` against the exact tag container inputs
before the GitHub release and registry push. Cover both published Linux
architectures using native execution or QEMU. The preflight must execute
version, build/save/load, scan/SARIF, and malformed-graph rejection, not only
container startup.

Make `publish` depend on contract, native builds, and container preflight. Make
the registry push depend on the same preflight and publish only the already
validated build definition/artifacts. Do not require a published GitHub release
before testing the container.

**Verify locally**: run functional acceptance against every preflight image that
the host can execute. Remote publish-dependency behavior is completed by the
operator in Step 6.

### 5. Keep the contract observable and current

Add `cpg-rs/scripts/check-workflow-contract.py` to verify the required
job names, `needs` relationships, explicit parity modes, and oracle digest
reference. Avoid brittle checks for unrelated YAML formatting.

Update contributor commands to use `--committed-only` for offline work and
describe how maintainers run verified live acceptance. Do not duplicate parity
counts in the plan or workflow when catalogs can provide them.

**Verify**: the assertion script exits 0, workflow YAML is accepted by GitHub,
and `git diff --check` has no output.

### 6. Obtain operator-owned GitHub proof before marking done

Ask an authenticated operator to push the reviewed branch or open the PR. The
operator must run the protected-context command from Commands You Will Need and
provide links to two non-publishing workflow runs:

1. an ordinary green run where the reusable contract, `linux-package`, and
   container preflight all succeed;
2. a disposable commit/run with one prerequisite intentionally failing, proving
   `linux-package`, native publication prerequisites, and container publication
   prerequisites all fail or remain blocked.

The operator must revert the deliberate failure before merge. Never create a
tag or release for this test. Without both run links, set the index status to
`BLOCKED (awaiting operator CI)`.

**Verify**: both run links identify the implementation commit(s), and the GitHub
API still reports exactly `linux-package` as the protected context.

## Test Plan

- Shell-test checksum success, checksum mismatch, missing archive, failed
  extraction, and failed Joern launch.
- Test `check.sh --live` and `--committed-only` as distinct contracts.
- Exercise reusable workflow failure and skip propagation before merge.
- Run package acceptance on all five native release matrix outputs in CI.
- Run Plan 006's focused persistence tests on Linux, macOS, and Windows.
- Run the same functional acceptance against amd64 and arm64 preflight images.
- Confirm the protected context remains exactly `linux-package` through the
  GitHub API or repository settings after the workflow lands.

## Done Criteria

- [ ] Every downloaded or cached Joern archive is digest-verified before use
- [ ] Live C regeneration cannot silently fall back to committed output
- [ ] PR and tag workflows execute one shared complete contract for exact SHAs
- [ ] Protected context remains exactly `linux-package` and fails closed
- [ ] GitHub release and registry publication require container preflight
- [ ] Native packages and both Linux container architectures run functional acceptance
- [ ] Focused persistence tests pass on Linux, macOS, and Windows
- [ ] Tag ancestry and Cargo-version checks remain enforced
- [ ] Offline parity, full workspace quality, Python syntax, and diff hygiene pass
- [ ] Reviewer independently confirmed the pinned oracle archive digest
- [ ] Operator supplied green and forced-failure GitHub run links
- [ ] Only in-scope files changed
- [ ] `plans/README.md` marks Plan 007 `DONE`

## STOP Conditions

- GitHub reports a required context other than `linux-package`.
- A reusable workflow necessarily changes the rendered required context and no
  caller-level aggregator can preserve it.
- A reviewer cannot confirm the recorded v4.0.555 archive digest from a separate
  clean download; leave the plan blocked.
- QEMU cannot execute one published container architecture within the current
  release budget; report timings before narrowing coverage.
- The proposed topology can publish after any failed or skipped contract job.

## Maintenance Notes

- Oracle version upgrades must change version, archive digest, cache key, and
  committed differential evidence in one reviewed change.
- Review the actual GitHub job graph and failure behavior, not merely whether
  command strings appear in YAML.
- Heavy PR-gate relevance optimization is intentionally deferred until it has
  an explicit, fail-closed change classifier.
