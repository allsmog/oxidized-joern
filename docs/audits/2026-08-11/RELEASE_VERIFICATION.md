# Public release verification — post-change

Date: 2026-08-11
Revision verified: `f74c237c236971ee4df03e5f82375469b91ca9af` (master), plus the
`release-prep` changes recorded below.

This memo closes the open items left by
[PUBLIC_RELEASE_AUDIT.md](../2026-08-04/PUBLIC_RELEASE_AUDIT.md), which recorded
pre-change findings and explicitly deferred final verification.

## Verdict

**DO NOT make this repository public yet.** Terminology and history are clean in
every ref and every reachable object, and the engine's CI gates are now green —
but pre-rewrite commits carrying the removed codename are **still retrievable
from GitHub by direct SHA**, and one of those SHAs is published in a sibling
repository. See "Blocker: unreferenced objects retained by GitHub" below.

## Blocker: unreferenced objects retained by GitHub

The 2026-08-04 history rewrite removed nine `Co-Authored-By` trailers carrying
the codename and pruned the superseded commits locally. Local pruning was
verified complete (`git fsck`: zero unreachable, zero dangling). **GitHub did not
garbage-collect its copies.**

Demonstrated, not inferred:

```
$ git fetch origin 9ab43859f0c3342c734ff38ea6366711d216c8fd    # succeeds
$ git log 9ab43859 --format='%b' | grep -cif codename.txt      # 9
```

(The scan term is kept in an untracked file on purpose: this memo must not
reintroduce the very string the rewrite removed.)

`9ab43859f0c3342c734ff38ea6366711d216c8fd` is not reachable from any of the
6,029 advertised refs, yet GitHub's API resolves it and `git fetch` retrieves it
along with its full ancestry — which includes all nine unsanitized commits:

| Commit | Date | Subject |
| --- | --- | --- |
| `eabf0303873d7edcc701e344f4f36c650cd9786a` | 2026-07-21 | docs: add sanitized engine session log (s115-s130) |
| `4ae2de6cffdaba1048dd4105159b3c68b01acd75` | 2026-07-21 | analysis: deterministic witness choice + receiver-modeled summaries |
| `8fd33550c4ed1e92f93595197bc641a3bef0553c` | 2026-07-21 | analysis: test-file demotion for persisted reads |
| `f4573dc2b5193500539275e42867bd6580bf1227` | 2026-07-21 | taint: field-sensitive object flow via dotted taint keys |
| `25070007d55216f09834a4e8d52bdea4d372a4e2` | 2026-07-21 | analysis: discarded-return v2 |
| `f9048b1712307877dc2464c61d5b687bea675cd8` | 2026-07-21 | analysis: structural rule kinds |
| `f3bb6322c6683d62707ec8347fa0620c793da9ec` | 2026-07-20 | taint: assignment sinks |
| `e61db21b2859cf0913dce26bcebd0735b00615c1` | 2026-07-20 | cpg-analysis: returns-tainted summaries |
| `75ab94722e4128831843ca770693d37382c99154` | 2026-07-20 | cpg-rs: Rust CPG engine |

The trees of those commits are clean; the exposure is in **commit messages**.

This is not a theoretical SHA-guessing concern. The revision is published in a
sibling repository's `Cargo.toml`, `deny.toml`, and `CONTRIBUTING.md` as a git
dependency pin. Anyone reading those files has the SHA.

### Required remediation, in order

1. **Re-pin the dependent repository** to a revision reachable from `master`.
   Its four `cpg-*` git dependencies currently pin `9ab43859…`, which exists
   only as an unreferenced object — that build works today *because* GitHub has
   not collected it, and will break the moment GitHub does.
2. **Ask GitHub Support to garbage-collect unreferenced objects** in this
   repository. Force-pushing does not do this; GitHub documents Support
   involvement for removing data that remains reachable by SHA.
3. **Re-verify** that `git fetch origin 9ab43859…` fails and the API returns 404
   for each of the nine commits above.
4. Only then flip the repository to public.

Note the ordering constraint: steps 1 and 2 are coupled in the opposite
direction from intuition — collecting the objects is what breaks the dependent
build, so re-pin first.

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

This covers the local object database and everything the remote advertises.
GitHub's retention of unreferenced objects was probed directly rather than
assumed, and **was found to be non-empty** — see the blocker section above. It
cannot cover copies in third-party clones. At the time of writing the repository
is private with zero forks, so that exposure is nil.
