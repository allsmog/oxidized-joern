# Public release verification — post-change

Date: 2026-08-11
Revision verified: `f74c237c236971ee4df03e5f82375469b91ca9af` (master), plus the
`release-prep` changes recorded below.

This memo closes the open items left by
[PUBLIC_RELEASE_AUDIT.md](../2026-08-04/PUBLIC_RELEASE_AUDIT.md), which recorded
pre-change findings and explicitly deferred final verification.

## Verdict

**Terminology and history are clean. The engine's own CI gates were red and are
now green.** Remaining risk is concentrated in one place: no adversarial review
has yet proven that the generalization pass preserved detection capability.

## Scrub verification — complete

Scanned the entire object database, not just the current tree, and the live
remote advertisement.

| Surface | Scope | Result |
| --- | --- | --- |
| Working tree and HEAD tree | all tracked files | 0 |
| Commit messages and bodies | all refs | 0 |
| Author / committer headers | 126 distinct identities | 0 |
| Ref names, packed-refs, reflog, `.git/config` | 3,078 local refs | 0 |
| All commit and tag objects (raw) | 6,600 commits, 2,922 tags | 0 |
| All blobs | 32,134 blobs, ~771 MB | 0 |
| Live remote advertisement | 6,029 refs | 0 |

Terms covered: the five configured release terms plus the two engineering
codenames. Zero genuine occurrences of any of them.

Noisy terms were classified exhaustively rather than sampled:

- **`RSC`** — 1,447 blobs, **zero standalone-word occurrences**. 208 distinct
  enclosing tokens, all upstream identifiers (`localDestructorScopes`,
  `astForScalar`, `UNDERSCORE`, `apiuserscache`). Of the six tokens containing
  literal uppercase `RSC`, four are outside `UNDERSCORE`: `SUPERSCRIPT`, two
  base64 digests, and `SRSCtl` — a MIPS CP0 register name in a bundled
  highlight.js grammar inside an upstream reveal.js docs asset.
- **`SPARK`** — 4 blobs, zero standalone. Text matches are the `sparkles` npm
  package; binary matches are V8 "Sparkplug" internals. The single commit-message
  hit is upstream Joern `6ce279526` (`IPrestoSparkServiceFactory`), unrelated
  history that must be retained.
- **`CDM`** — 35 blobs: 26 binary artifacts, 9 upstream npm/yarn lockfiles where
  the sequence sits inside sha512 integrity hashes. No source file in any
  revision contains it; `git log --all -S'CDM'` over the two originally flagged
  files returns zero commits.

Structural results: `git fsck` reports **zero unreachable and zero dangling
objects**, and all 6,600 commit objects are reachable from refs — the superseded
lineage was garbage-collected, not merely orphaned. The pre-rewrite master SHA is
no longer advertised by the remote.

## Credential scan — clean

Scanned all 32,134 blobs for AWS, GitHub (token and PAT), Slack (token and
webhook), OpenAI, Anthropic, Google, npm, PyPI, Stripe credentials, JWTs, and
PEM private-key blocks.

Only hits are upstream Joern's key-detection test fixture
(`-----BEGIN RSA PRIVATE KEY-----\n123456789\n`) and PEM parser error strings
inside a vendored executable fixture. **Zero blobs contain substantive key
material** (no PEM block anywhere has a body over 100 characters). No repository
secrets are configured.

## Build and test verification

The generalization pass touched only `cpg-rs/`; the Scala tree was not modified
by it, so the Rust workspace is the relevant gate.

| Gate | Before | After |
| --- | --- | --- |
| `cargo test --workspace --locked` | 258 passed, 0 failed (27 suites) | 258 passed, 0 failed |
| `cargo fmt --check` | **FAILED** | clean |
| `cargo clippy --workspace -- -D warnings` | **FAILED** (23 errors, 5 crates) | clean |
| `cargo clippy --all-targets -- -D warnings` | **FAILED** | clean |

The lint fixes are behavior-preserving; test counts are identical before and
after. Changes beyond formatting were reviewed individually and are limited to
type aliases, `?`/`while matches!` rewrites, one dead-store removal, a
`&mut Vec` → `&mut [_]` signature, and targeted `#[allow]`s carrying a stated
reason.

## Release configuration — fixed

The fork was configured to publish to Maven Central under **upstream Joern's**
identity (`io.joern`, `joernio/joern`, `joern.io`), with a daily cron invoking
`ciReleaseSonatype`. Any future addition of Sonatype credentials would have
published fork artifacts under upstream's coordinates.

Maven Central publishing is now disabled outright; `organization`, `scmInfo`, and
`homepage` point at this fork. Distribution is via GitHub Releases only.

## CI correctness

- `export-cpg-source` validated the manifest against the **caller's HEAD** while
  the workflow triggers on `cpg-rs/**` — so the job failed on precisely the
  changes it exists to handle. It also archived `HEAD` rather than the embedded
  revision, exporting the wrong tree. Both corrected.
- Third-party actions `dtolnay/rust-toolchain` (was a mutable version branch) and
  `SwiftyLab/setup-swift` (was floating `@latest`) are SHA-pinned with the
  human-readable version in a trailing comment.

Fork-PR safety was already correct: workflows use `pull_request` rather than
`pull_request_target` and gate on `head.repo.full_name == github.repository`.

## Still open

1. **No adversarial review of the generalization.** `b24a35599` changed 705
   lines, 416 of them in `middleware.rs` — exactly where the audit located the
   embedded product assumptions. The test suite passes, but the suite was
   changed in the same commit. Nothing has independently confirmed that no
   detector, rule, fixture path, or assertion lost coverage. **This is the
   highest remaining risk and it is not a leakage risk — it is a silent
   capability-regression risk.**
2. **Scala workspace not re-validated here.** Unaffected by the generalization,
   but the audit recorded pre-existing Java 21 failures that remain unexamined.
3. **Remaining third-party actions** pinned to major-version tags rather than
   SHAs (docker/*, ruby/setup-ruby, sbt/setup-sbt, Swatinem/rust-cache,
   shivammathur/setup-php, softprops/action-gh-release, dieghernan/cff-validator).
   Common practice, but a supply-chain decision worth making deliberately before
   the repository is public.

## Limits

This covers the local object database and everything the remote advertises. It
cannot cover GitHub's internal retention of unreferenced objects, or copies in
clones and forks. At the time of writing the repository is private with zero
forks, so that exposure is nil.
