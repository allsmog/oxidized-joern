# Public release audit — pre-change findings

Date: 2026-08-04
Starting branch: `master` at `69e4801b574d81c1603851bf8cecab72f5e33320`
(that commit no longer resolves: the generalization pass rewrote this lineage.
The equivalent post-generalization revision is `b24a355995f1f1085a7a523f46f93f02cb70e1f8`.)
Status: pre-change audit and implementation map; final verification is pending

## Executive verdict

**NOT READY at the audited starting revision.** The repository contains a small, coherent
set of private terminology and behavioral assumptions in the new Rust analysis engine,
its synthetic service fixtures, and the reachable history that introduced them. The
generalization is tractable without deleting capability: the affected fixture family can
be renamed coherently, the authorization assumptions can become configuration with
backward-compatible defaults, and only the introducing lineage plus necessary descendants
needs rewriting.

The wider non-Rust tree and older upstream history contain raw byte matches that are
legitimate external identifiers or accidental byte sequences. Those must remain intact;
changing them would corrupt vendored artifacts, public APIs, or unrelated upstream work.

This memo records the state before implementation. It does not claim that post-change
tests, object scans, history mapping, repository integrity checks, or cleanliness checks
have completed.

## Scope

The audit covered:

- tracked paths and file contents at the starting revision;
- generated, fixture, and vendored artifacts, including binary files;
- historical paths and blob contents across every reachable object;
- commit subjects, bodies, trailers, authors, committers, and raw headers;
- local tag and reference names, including auxiliary worktree references;
- private behavioral assumptions in detector and rule logic;
- preservation risks in tests, fixtures, dispatch, entry mining, taint analysis, and
  middleware classification.

The reachable-object inventory contained 111,899 objects: 26,531 blobs totaling about
590 MB, 4,954 commits, 80,414 trees, and no tag objects. The forbidden stale revision was
not present. No configured release term appeared in a reference name.

## Methodology

Four read-only audit lenses ran in parallel:

1. Rust tracked-tree terminology and fixture semantics.
2. Non-Rust, generated, vendored, and binary content.
3. Reachable Git objects, paths, references, and metadata.
4. Adversarial semantic-preservation review.

The director then spot-checked the load-bearing citations against the starting tree. Text
searches were case-insensitive. Object inspection was binary-safe and used object type and
length boundaries rather than line-oriented assumptions. Ambiguous short matches were
classified individually; no result was accepted solely because a byte sequence matched.

## Baseline evidence

The following results were established before edits:

- `cargo test --workspace --all-targets` in `cpg-rs`: **PASS**.
- `cargo test --all-targets` in `oxidized`: **PASS**.
- `sbt clean test` under Java 21: **PARTIAL**. Pre-existing failures occurred in
  `csharpsrc2cpg`, `joerncli`, `swiftsrc2cpg`, and `php2cpg`; many other suites passed.

These are baseline results, not final release verification.

## Findings

### High — product assumptions are embedded in authorization classification

**Lens:** semantic preservation
**Evidence:** `cpg-rs/cpg-analysis/src/middleware.rs:27-35`,
`cpg-rs/cpg-analysis/src/middleware.rs:84`, and
`cpg-rs/cpg-analysis/src/middleware.rs:459-470` in the starting tree.
**Failure scenario:** a public user analyzes a service whose trusted-caller marker or server
constructor vocabulary differs from the built-in convention. The census cannot express
that environment, can misclassify a partial check, and cannot disable the inference without
editing engine code.

The existing verdicts and defaults are useful behavior and must remain. The correct change
is a generic configuration object with overridable marker and constructor lists, explicit
empty-list disable behavior, unchanged default output labels, and regression tests for
default, custom, and disabled modes.

### High — a private service fixture family spans multiple semantic layers

**Lens:** Rust tracked tree
**Evidence:** `cpg-rs/cpg-analysis/src/callgraph.rs:370-445`,
`cpg-rs/cpg-cli/tests/thrift_stitch.rs:13-146`,
`cpg-rs/cpg-lang-ts/src/engine.rs:78`, and
`cpg-rs/cpg-lang-ts/tests/robustness.rs:193-241` in the starting tree.
**Failure scenario:** a textual cleanup changes only comments or obvious class names. Type
hints, namespace stripping, receiver-field lookup, generated-interface recognition,
virtual dispatch, handler filtering, and entry extraction then disagree, weakening tests or
silently changing resolution behavior.

This family must be renamed as one coordinated synthetic fixture. Every assertion and code
path stays; only the example vocabulary changes.

### High — editing the current tree alone leaves private history reachable

**Lens:** Git history and object exposure
**Evidence:** the two release-term comments at
`cpg-rs/cpg-analysis/src/entries.rs:254` and
`cpg-rs/cpg-cli/tests/thrift_stitch.rs:70` were introduced with the Rust engine lineage;
the same lineage introduced the private service fixture family cited above.
**Failure scenario:** the working tree is clean after a rename, but a public user can still
retrieve the original blobs and names from an earlier commit or an auxiliary reference.

The affected lineage consists of the introducing commit and eleven necessary descendants.
Those commits were unsigned. Unrelated merge parents and older history can remain
byte-identical. A surgical tree-and-parent rewrite can therefore preserve topology,
timestamps, attribution, messages, and all unrelated objects without stripping historical
signatures.

### Medium — several behavioral fixtures carry private operational provenance

**Lens:** semantic preservation
**Evidence:** `cpg-rs/cpg-analysis/src/taint.rs:3582-3604`,
`cpg-rs/cpg-analysis/src/taint.rs:3826-3831`, and
`cpg-rs/cpg-lang-ts/tests/robustness.rs:279-303` in the starting tree.
**Failure scenario:** deleting these cases to remove provenance would drop coverage for Go
initializers, Scala named arguments, assignment sinks, nested field access, persisted
source stitching, and interprocedural task execution.

The examples should become synthetic event-consumer, policy-context, and job-execution
fixtures while preserving the same language shapes, graph paths, sinks, and assertions.

### Medium — engineering records expose local workflow provenance

**Lens:** documentation
**Evidence:** `cpg-rs/ENGINE-SESSIONS.md:1-13`,
`cpg-rs/ENGINE-SESSIONS.md:58-62`, `cpg-rs/PROGRESS.md:25`, and
`cpg-rs/PROGRESS.md:57` in the starting tree.
**Failure scenario:** a public reader encounters local machine paths, private validation
framing, and opaque run identifiers instead of reproducible engineering rationale.

The records should be recast as capability milestones and controlled before/after corpus
validation. All algorithms, design choices, counts, and validation outcomes remain.

### Low — raw acronym matches in upstream and binary material are not private usage

**Lens:** non-Rust and object-level classification
**Evidence:** representative matches occur in vendored executable fixtures, generated
framework metadata, public upstream class names, digest fields, signed commit headers, and
raw tree object identifiers.
**Failure scenario:** treating every raw byte match as private text causes fixture
corruption, upstream API renames, signature loss, or needless rewriting of unrelated
history.

These matches require documented classification, not mutation. They remain subject to the
final binary-safe scan so that every retained occurrence has a rationale.

## Coverage and verdict table

| Lens | Scope | Verdict at starting revision | Evidence |
| --- | --- | --- | --- |
| Rust tracked tree | Engine, CLI, language frontend, tests | FAIL | Private comments, fixture vocabulary, and hard-coded conventions found |
| Non-Rust and vendored artifacts | Text, generated files, executables, archives | PASS | Matches classified as external references or byte-level false positives |
| Git objects and references | Blobs, trees, commits, paths, tags, refs | FAIL | Affected Rust lineage remains reachable until rewritten |
| Semantic preservation | Detectors, rules, dispatch, taint, middleware | PARTIAL | Generic equivalents are clear, but configuration and fixture changes are pending |
| Baseline validation | Rust and Scala workspaces | PARTIAL | Rust baselines pass; Java 21 run has recorded pre-existing failures |
| Documentation provenance | Engineering records and release memo | FAIL | Local workflow framing remains at the starting revision |

## Contextual change map

| Audited concept | Generic replacement | Preservation contract |
| --- | --- | --- |
| Product-labeled cluster identifier endpoint comment | Generic cluster identifier endpoint | Entry-mining semantics unchanged |
| Private gateway server and interface fixture | `GatewayServer` and `GatewayServerIf` | Same type declaration, inheritance, and callgraph assertions |
| Private file-service interface family | `FileService`, `FileServiceIf`, and `FileServiceNull` | Same generated-interface and null-handler filtering |
| Private file-service implementation fixture | `FileServiceHandler` | Same virtual and inherited dispatch behavior |
| Private client wrapper fixture | `GatewayFileServiceClient` | Same client-wrapper exclusion and entry behavior |
| Private receiver member and namespace | `file_service_client_` and `example::gateway` | Same receiver hinting, namespace stripping, and template peeling |
| Private fixture filenames | `gateway_server.cpp` and `file_service_handler.cpp/.h` | Same file-aware resolution and production/test boundaries |
| Fixed caller-context and constructor vocabulary | `AuthzCensusConfig` with `caller_context_markers` and `framework_server_calls` | Existing wrapper keeps identical defaults; custom and empty lists are tested |
| Private event and policy examples | Synthetic event-consumer and policy-context cases | Same Go and Scala assignment-sink paths and assertions |
| Private data-protection job example | `JobConfig.executionSettings.executionUser` and task-execution vocabulary | Same nested field, persistence, and sink behavior |
| Run-number and local-cache documentation | Capability-oriented milestones and controlled corpus comparisons | Every technical fact, count, and decision retained |
| Affected introducing lineage | Surgical rewrite of only that lineage and necessary descendants | Topology, dates, attribution, merge parents, and unrelated content preserved |

## Intentionally retained occurrence categories

The following categories are legitimate and should not be rewritten:

- standardized cryptographic algorithm labels embedded in vendored executable fixtures;
- generated runtime metadata and public upstream identifiers whose longer names happen to
  contain a configured short byte sequence;
- upstream distributed-computing integration history unrelated to this repository's
  private systems;
- dependency digests, integrity strings, encoded verification keys, and archive payloads;
- raw object-identifier bytes and signature payloads that coincide with short search
  sequences;
- unrelated words and identifiers containing a short sequence across a larger public name.

Retaining these protects fixture validity, reproducible dependencies, upstream
compatibility, object identity, and historical signatures. They are not unresolved private
references.

## Top five fixes by leverage

1. Extract authorization caller-context and server-constructor assumptions into a public
   configuration API and rule-pack schema, preserving existing defaults.
2. Rename the entire private service fixture family coherently across parser, callgraph,
   entry, taint, CLI, and robustness tests.
3. Generalize the event, policy, and job fixtures without changing graph shape, sinks,
   persistence behavior, or assertions.
4. Rewrite engineering records into capability and reproducible-validation language while
   retaining every substantive result.
5. Rewrite only the affected lineage and necessary descendants, update all local refs,
   record the exact mapping, then prune obsolete local objects after verification.

## Not covered and required follow-up

This pre-change audit does not cover completed implementation verification. Before release,
the following still must be demonstrated:

- formatting, linting, unit, integration, Rust workspace, and Java 21 workspace results
  after the generalization;
- adversarial review proving that no detector, rule, fixture path, assertion, or behavioral
  branch was lost;
- zero unresolved private references in the current tree;
- binary-safe scanning of every reachable object and reference after history rewriting;
- exact old-to-new commit and reference mapping;
- clean repository integrity check and clean working tree;
- a lease-protected push command that is documented but not executed.

Hosted-repository verification is intentionally excluded until push authorization. After
an authorized push it must use an isolated temporary mirror, scan every advertised
reference and raw object, confirm superseded identifiers no longer resolve through the
hosting API, and remove all temporary mirrors.

Some exact-match rule selectors look deployment-shaped but have no verified association
with a configured release term. Renaming them on resemblance alone would remove detection
capability. They remain unchanged unless provenance establishes a private mapping or a
backward-compatible generic alias can preserve every match.
