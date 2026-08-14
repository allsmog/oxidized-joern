# Plan 005: Make the shipped C engine the parity-validated engine

> **Executor instructions**: This is a staged architecture plan, not a one-shot
> rewrite. Follow the stages in order and keep every intermediate commit green.
> Run every verification command. If a STOP condition occurs, stop and report;
> do not improvise. Update `plans/README.md` only when the entire plan is done,
> unless a reviewer explicitly splits this into numbered child plans first.
>
> **Drift check (run first)**:
> `git diff --stat 6913b3ac1..HEAD -- cpg-rs/cpg-lang-c cpg-rs/cpg-core cpg-rs/cpg-analysis cpg-rs/joern-parity cpg-rs/conformance cpg-rs/README.md README.md`
> Reconcile after Plans 001-004. STOP if `joern-parity` already depends on and
> dumps the shipped `cpg-core` graph; reduce this plan to remaining differences.

## Status

- **Status**: DONE
- **Depends on**: `plans/002-harden-cpg-persistence.md`,
  `plans/004-enforce-release-acceptance-gates.md`
- **Planned at**: commit `6913b3ac1`, 2026-08-13
- **Finding**: ARCHITECTURE-01 / COMPAT-01 / TEST-02

## Why this matters

The repository's 96/96 exact C parity result belongs to a standalone text
dumper, not to the graph built and scanned by the released `cpg` binary. The
standalone implementation depends directly on tree-sitter and constructs its
own records; the production C frontend is a separate, much smaller builder that
collapses unsupported wrappers and omits parity-relevant constructs. Production
taint/summaries also contain name/source-order paths that are not protected by
the exact ReachingDef oracle.

This split is why the Rust-native preview can be release-ready for explicitly
limited workflows but cannot honestly be called a drop-in Joern replacement.
The useful target is narrower: make one shipped C build/scan/flow path consume
the same graph that passes exact oracle diffs, then validate that path on named
real projects and security outcomes. Full CPGQL, Scala console/workspace UX,
plugins, every Joern frontend, and byte-compatible Joern graph serialization
remain explicit non-goals.

## Context and evidence

- `cpg-rs/joern-parity/Cargo.toml:11-13`: the parity binary depends on
  tree-sitter, not `cpg-core` or `cpg-lang-c`.
- `cpg-rs/joern-parity/src/main.rs`: the approximately 4,400-line binary builds
  independent node/edge records and text output.
- `cpg-rs/cpg-lang-c/src/lib.rs:52-79`: the shipped C frontend builds top-level
  function definitions through `cpg-core`.
- `cpg-rs/cpg-lang-c/src/lib.rs:114`: shipped method identity uses a bare name
  and generic `name()` signature.
- `cpg-rs/cpg-lang-c/src/lib.rs:263-267`: `case` is collapsed to a simplified
  control structure because the schema lacks a jump target.
- `cpg-rs/cpg-lang-c/src/lib.rs:438-442`: unknown expression wrappers collapse
  to the first buildable child.
- `cpg-rs/ROADMAP.md:62-92` and `cpg-rs/WHATS_LEFT.md:17-27`: M6 explicitly
  calls for folding the parity frontend onto the production builder and
  rewiring the gate.
- `cpg-rs/cpg-analysis/src/value_flow.rs:36`: sparse flow reads `EdgeKind::Ddg`
  while the standard pipeline emits `EdgeKind::ReachingDef`.
- `cpg-rs/cpg-analysis/src/reaching_def.rs:29`: the production pass documents
  missing parameter-out, jump/type, global-capture, and inline-macro routing.
- `cpg-rs/ROADMAP.md:248-257`: full CPGQL and Scala console/workspace UX are
  explicit non-goals. Do not reverse those decisions in this plan.

## Scope

### In scope

- `cpg-rs/cpg-core` schema/build/deterministic canonical-dump support required
  for the committed C oracle
- `cpg-rs/cpg-lang-c` construct-by-construct convergence
- `cpg-rs/cpg-analysis` traversal over canonical ReachingDef/CFG edges
- `cpg-rs/joern-parity` as a thin production-graph oracle adapter and its C corpus
- `cpg-rs/conformance` and new pinned C outcome/real-project harnesses
- Compatibility/status documentation after executable gates are truthful

### Out of scope

- Restoring or adding Scala implementation code
- Full Joern CPGQL, console, workspace, plugin, server, or exporter compatibility
- Promoting non-C languages to stable; each needs a separate acceptance ladder
- Rewriting all frontends at once
- Marketing as a drop-in Joern replacement

## Implementation strategy

Before source changes, split this plan into reviewable child plans aligned to
the stages below. Each child must keep the production CLI usable and must add
oracle coverage before deleting standalone logic. Do not copy the standalone
builder wholesale into `cpg-lang-c`; use it as behavioral evidence and express
the canonical model in shared schema/frontend APIs.

### Stage 1. Dump the current production graph in oracle format

Add a deterministic canonical dump adapter that takes `cpg_core::Cpg` and emits
the exact sections compared by `joern-parity/check.sh` (AST, nodes, edge kinds,
CFG, REACHING_DEF, and method partitions). Put generic graph ordering/rendering
in `cpg-core` only when it is not C/Joern-specific; keep Joern text conventions
in `joern-parity`.

Change `joern-parity` to depend on `cpg-lang-c` and run the same standard
pipeline as `cpg build --lang c`. Add a diagnostic mode that compares old
standalone and new production dumps during migration. At this stage failures
are expected and must be recorded by corpus block/section; do not weaken oracle
files or normalize away semantic differences.

Gate for the stage:

```sh
cd cpg-rs
cargo test -p joern-parity --locked
cargo test -p cpg-lang-c --locked
```

Expected: the adapter is deterministic and the migration report is reproducible;
the required release gate remains on the last green standalone path until the
production path reaches 96/96.

### Stage 2. Converge schema and C AST in corpus-sized slices

Port missing behavior in small vertical slices ordered by security/dataflow
impact: method/file/type identity and signatures; full expression wrappers;
field/member/pointer forms; labels/jump targets and switch/case; macro/inline
metadata; globals/captures; remaining Joern scaffolding nodes and edges.

For every slice:

1. add or identify the failing oracle block;
2. add the minimal shared schema representation without C-only hacks in generic
   graph code;
3. implement it in `cpg-lang-c`;
4. update production analysis consumers if a new canonical node/edge replaces a
   fallback;
5. make that block exact on nodes, properties, order, and edges;
6. run full workspace tests before the next slice.

Never resolve an ambiguous call to the first same-named method. Introduce
qualified identity/signatures and either a justified target set or unresolved
state. Preserve deterministic graph/output ordering.

Gate per slice:

```sh
cd cpg-rs
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

### Stage 3. Make dataflow and scanners consume canonical edges

Make production `flow`, taint rules, and summaries consume the parity-validated
CFG/ReachingDef representation rather than source-line ordering or a separate
DDG convention. Define one adapter if analyses need a sparse logical view, but
its facts must derive from `EdgeKind::ReachingDef` and tested call/return routing.

Add differential source-to-sink fixtures covering branches and kills, loops,
returns, globals, pointer/member access, sanitizers, recursive/mutually recursive
calls, and same-named functions in different scopes. Compare both graph facts
and final findings to the pinned Joern oracle where Joern has equivalent
semantics; for independent scanner rules, use labeled expected outcomes instead
of inventing parity.

Gate for the stage:

```sh
cd cpg-rs
cargo test -p cpg-analysis --locked reaching_def
cargo test -p cpg-cli --locked scan
./joern-parity/check.sh
```

Expected: all 96 committed blocks pass through the production graph and the
security outcome fixtures pass.

### Stage 4. Switch the required gate and remove duplicate implementation

Only after production reaches 96/96:

- change the automatic parity gate from Plan 004 to the production adapter;
- remove the standalone AST/CFG/ReachingDef builder code from
  `joern-parity`, retaining only corpus/oracle acquisition, canonical dumping,
  and diff reporting;
- prove `cpg build` and `joern-parity` invoke the same frontend/pipeline by API,
  not by duplicated configuration;
- keep a test that fails if the parity crate stops depending on production.

Gate:

```sh
cd cpg-rs/joern-parity
./check.sh
cd ..
cargo test --workspace --locked
```

Expected: 96/96 exact production-path blocks and all workspace tests pass with
no independent parity builder remaining.

### Stage 5. Add named real-project and outcome acceptance

Create a manifest with immutable revision, source URL, license, fixture scope,
and expected graph/finding hashes for at least two representative C projects.
Use vendored minimal licensed snapshots or reproducible pinned fetches; PR CI
must not depend on mutable branches. Add:

- deterministic repeated build/export/SARIF assertions;
- zero-diff canonical output where the existing ROADMAP requires it;
- labeled positive and negative security cases with recall/precision baseline;
- build/scan wall-time and peak-RSS budgets with hardware-normalized tolerance;
- save/load and incremental-update equivalence;
- a small PR subset and full nightly/pre-release suite.

Do not call the engine production-ready for C until the selected workflow matrix
has named thresholds and passes. Do not generalize this result to another
language.

### Stage 6. Reconcile public compatibility documentation

Replace contradictory progress prose with one compatibility matrix keyed by
language, command/workflow, graph layer, engine path, oracle/corpus, and
stability. Mark C stable only for the exact gated workflows. Mark other
languages experimental until equivalent language-specific ladders exist.
Retain the explicit statement that full Joern/CPGQL/Scala-console compatibility
is not provided.

## Test plan

- Exact committed oracle diff must run through the same `cpg-lang-c` and
  standard pipeline used by the CLI.
- Each convergence slice begins with a failing fixture and ends with exact
  node/property/order/edge output.
- Production scan/flow outcomes cover branch/kill, call/return, global,
  pointer/member, sanitizer, recursion, and name-collision cases.
- Two pinned representative C projects cover determinism, real-project
  correctness, persistence/update equivalence, and resource budgets.
- A required test prevents reintroduction of a duplicate standalone builder.

## Done criteria

- [ ] `joern-parity` constructs and dumps the shipped production C graph
- [ ] The independent standalone builder is removed only after production is 96/96
- [ ] Production scan/flow consumes canonical CFG/ReachingDef facts
- [ ] Ambiguous same-name symbols are never resolved by arbitrary first match
- [ ] Two pinned real C projects meet exact/deterministic acceptance criteria
- [ ] Labeled outcome corpus meets recorded recall/precision thresholds
- [ ] PR subset and full nightly/pre-release gates are automatic
- [ ] Compatibility docs name the stable workflow without claiming drop-in Joern
- [ ] No Scala implementation is restored or added
- [ ] Full formatting, locked Clippy, locked workspace tests, and parity pass
- [ ] Only in-scope files changed
- [ ] `plans/README.md` status row updated

## STOP conditions

- A proposed normalization makes a failing semantic oracle diff disappear
  without making the production graph equivalent.
- A schema change cannot preserve/migrate Plan 002's documented persistence
  contract; resolve the format version first.
- A corpus license/revision cannot be made reproducible for public CI.
- A stage requires restoring Scala code or implementing a declared non-goal.
- Exact Joern behavior would reduce security correctness for the independently
  defined scanner contract. Record the divergence and test both explicitly.

## Git workflow

Create one commit per child plan or construct family with repository-style
messages such as `feat: emit parity graph through cpg-core` and
`fix: route C member access through reaching defs`. Never combine the whole
multi-stage migration into one review. Each commit must pass its stage gate.

## Notes for the reviewer

The decisive invariant is not that two code paths print the same fixture; it is
that the released CLI and parity harness instantiate the same frontend,
pipeline, and graph. Reject duplicated configuration or compatibility shims
that allow the sidecar to stay green while production diverges.
