# Remaining Joern port work

Updated **2026-09-09** against pinned **Joern 4.0.555**. This is the current
remaining-work inventory and planning estimate. It supplements
[PROGRESS.md](PROGRESS.md) and supersedes older architecture notes as a parity
status report. The full pure Rust port is **incomplete**.

## How much is left?

There is no defensible exact completion percentage. We have exact measurements
for selected C outputs, but no complete inventory of every upstream language,
query, property, script and extension behavior. Passing the current corpus does
not establish whole-product equivalence.

My planning estimate, including implementation, tests, review and integration:

| Target | Remaining engineering effort | What this estimate means |
|---|---:|---|
| Make the existing complete zlib/Lua **selected canonical outputs** match | **12–24 engineer-weeks** | A bounded C milestone; low confidence until the remaining differences are grouped by root cause. This does not include complete raw graph or all-C parity. |
| Broader C compatibility, including required graph properties, analysis and representative query outcomes | **40–80 engineer-weeks** | Roughly 10–20 months for one full-time specialist. Includes the preceding milestone; these two estimates must not be added. Requires an explicit C acceptance matrix. |
| Enumerated Joern frontend and product behavior across languages, queries, interchange and workflows | **250–600 engineer-weeks**, approximately **5–12 engineer-years** | Very rough portfolio budget using 50 working weeks/year. The detailed packages below overlap; this is an order-of-magnitude judgment, not their exact sum. |
| Literal unchanged execution of arbitrary Scala scripts and JVM plugins, while retaining a pure Rust runtime | **Not yet estimable** | A runtime/ecosystem compatibility problem in addition to the port. This remains part of the unresolved 1:1 scope; it is not silently included in the finite range above. |

For a team of four to six experienced engineers with AI assistance, **18–36+
months** is a plausible planning scenario for the enumerated product surface.
That is not a delivery commitment: dependencies, frontend uncertainty and shared
integration work prevent dividing effort by agent count. A useful C product can
arrive much earlier than full Joern equivalence.

These are engineering judgments, not measurements of Astra's future speed. AI
can help implement independent slices, construct differential fixtures and
review changes. We have no calibrated multiplier for the whole remaining port.
Fast individual patches do not measure language coverage, and the next difficult
semantic mismatch can dominate a batch. The estimates should be revised using
accepted root-cause fixes and observed throughput, not tokens, test totals or
number of agents.

## What is measured today

The last completely accepted local checkpoint is the thirteenth batch,
commit `a806866acc83ce9b40b69259fe9e430a3e818dd0`. Its
[metrics](../docs/conformance/astra-thirteenth-batch-metrics.json) and
[report](../docs/conformance/astra-thirteenth-batch-2026-09-09.md) establish:

| Whole C input | Input size | Joern METHOD AST blocks | Exact | Different | Missing from Rust | Extra in Rust | Joern blocks still different or missing |
|---|---:|---:|---:|---:|---:|---:|---:|
| zlib 1.3.1 core | 26 files; 22,732 LF-counted lines | 410 | 160 | 174 | 76 | 16 | **250 / 410** |
| Lua 5.4.7 | 61 files; 30,098 LF-counted lines | 2,274 | 1,442 | 755 | 77 | 24 | **832 / 2,274** |

Combined, **1,082 Joern METHOD AST blocks differ or are missing**, and 40
additional Rust blocks have no matching Joern block. These are **not 1,082
independent bugs**. The denominator includes generated global, operator and
macro methods and external stubs, not just source function bodies. A single
preprocessing or identity fix can change many blocks.

Neither complete project projection matches: **0 of 2 exact**. The recorded
selected flow sections also have substantial differences:

| Input | Joern flow records | Rust flow records | Matching occurrences | Joern-only occurrences | Rust-only occurrences |
|---|---:|---:|---:|---:|---:|
| zlib | 200,374 | 162,011 | 25,556 | 174,818 | 136,455 |
| Lua | 207,820 | 229,251 | 114,223 | 93,597 | 115,028 |

These are serialized record multiset counts. Changed locations, identities or
ordinals can cascade into many differences; the counts do not independently
measure dataflow algorithm defects. The canonical projection omits Binding,
Import and Dependency scaffolding and some properties. Equality of that
projection would still require separate full-graph verification.

The **308/308** committed C comparison blocks pass, including a fresh live
Joern comparison. That is a real regression guarantee for those blocks, not
308 language features or a percentage of Joern.

### Current uncommitted increment

The fourteenth increment adds observed macro Binding nodes and their links.
Its integrated source has passed **492 tests in 83 groups**, formatting, strict
Clippy, release compilation, and **308/308 committed plus 308/308 fresh live**
comparisons. Both complete zlib/Lua canonical outputs are byte-identical to the
accepted thirteenth outputs: the table above remains current, with **zero
canonical gains or losses** from this increment.

This increment is **not yet a completed acceptance checkpoint**. Final candidate
capture/retention review, resource-manifest reconciliation, remaining release
and official acceptance checks, and local commit closeout remain pending. Do
not treat its passing tests as completion of those steps. The macro fixtures
also do not establish full raw Joern graph equality.

## Remaining work packages

The ranges below are planning priors for a senior engineer familiar with Rust,
compilers and graph analysis. One engineer-week means five focused working
days. They assume a frozen reference version and working reference tooling.
Dependencies overlap; **do not add every row to the milestone estimates above**.

| ID | Work remaining and current boundary | Completion evidence | Effort; confidence |
|---|---|---|---|
| R1 | Enumerate upstream frontends, versions/configurations, schema, queries, scripts, plugins and CLI contracts. There is no full tested denominator yet. | Versioned matrix maps each required behavior to an upstream control, Rust test, owner and pass/fail/unknown state. Unsupported routes stay visible. | **2–4 weeks; medium** |
| R2 | Close C lowering and preprocessing differences: build definitions/include search; function-like macros in conditions; repeated body includes; runtime control chains split by directives; variadics, stringification and token pasting; nested expansion/recovery; remaining initializer/type forms and locations. Existing support is substantial but partial. | Source-derived AST, type, name and location projections match on both projects and retained diagnostics; new repository/configuration controls expose no silent truncation. Derived relations/flows belong to R3; full-schema coverage belongs to R4. | **12–28 weeks; low** |
| R3 | Close analysis differences on correct graphs: CFG/reaching definitions, aliases, call resolution, interprocedural flow, library semantics and query outcomes. Production analysis already exists, including authoritative relations emitted through the C frontend. Broader Joern equivalence is not demonstrated. | Compare upstream graph relations and end-to-end reachable-flow/query results on isolated controls and complete projects, preserving multiplicity and errors. Classify defects by functionality and actual producer; do not count a source-lowering defect again as an analysis defect. | **12–32 weeks; low** |
| R4 | Complete schema/property/edge representation and extraction contracts. End coordinates, offsets and edge payloads are absent; argument-index presence remains unknown. DOMINATE, POST_DOMINATE and ALIAS_OF are absent from the edge enum. Public JSON emits only six node fields. | Inventory the pinned schema; represent required typed values and absence; compare complete node properties and edge occurrences; preserve them through save/load and export. Frontend-specific fact derivation belongs to R2/R5/R6. | **10–24 weeks; medium-low** |
| R5 | Bring existing non-C routes to measured parity: **C++, Go, Java, JavaScript, TypeScript/TSX, Python, Ruby, Rust and Scala**. All nine use the shared tree-sitter frontend; generic signatures, ANY returns and conservative branch lowering remain. C++ is not covered by the C campaign. | Per-language differential corpora and complete repositories cover names/types, imports, build context, inheritance, closures, overloads, dispatch and analysis. Common structural tests alone do not close this. | **60–160 weeks across the portfolio; low** |
| R6 | Resolve and implement missing upstream routes: **ABAP, C#, Kotlin, PHP, Swift, JVM bytecode/Jimple and native binaries/Ghidra**. The pinned runtime has launchers; the Rust CLI has no corresponding routes. | First admit real reference runs and dependency/input contracts, then differential frontend and repository gates for every required route. Launcher presence alone does not prove the upstream tool operates here. | **80–200 weeks; very low** |
| R7 | Expand query compatibility. Current QueryCompiler accepts method/call scans and exact `.name("...")` filters. The CLI JSON dispatcher is a separate small command set. Arbitrary CPGQL, traversal/path behavior and all querydb rules are not implemented. | A pinned query/querydb corpus matches results, ordering, multiplicity and errors against Joern. Reuse working graph/dataflow algorithms; do not count their repair again here. | **16–40 weeks; low**, excluding a general Scala runtime |
| R8 | Implement defined console, script, workspace and extension workflows. Native CLI, JSON and MCP interfaces already exist; Scala console/plugin compatibility does not. | Port and exercise an explicit workflow/script/extension corpus. Record the unresolved arbitrary Scala/JVM runtime contract separately; a Rust plugin API cannot make existing plugins compatible. | **8–24 weeks; low** for bounded workflows; arbitrary plugins unestimated |
| R9 | Add production Joern saved-graph interoperability. CPG2 **v3**, older CPG2 readers and legacy CPG1 support are this project's formats. The separate Java reference observer is not a production Rust reader. | Rust-produced graphs load in pinned Joern and preserve properties, relationships and query results; reverse interchange passes too. A lossless exchange format alone does not close direct binary compatibility. | **6–16 weeks; medium-low**, after schema contracts |
| R10 | Complete platform, installation, repository-scale and release validation. Five target configurations and release tooling exist; current accepted local evidence is macOS ARM64. | Successful artifacts and acceptance results for the exact final commit on each declared platform/container; clean/update equivalence, resource limits and failure handling on expanded repositories. | **4–10 weeks; medium**, excluding fixes charged to other packages |

The highest uncertainty lies in R5/R6 and arbitrary extension compatibility.
Schema, query and language work interact; implementing missing enum tags or
parsers alone will not satisfy their completion criteria.

Complete selected C project equality is a joint R2/R3 milestone. The C importer
marks several relation layers authoritative, so a flow mismatch is not evidence
by itself of a defect in a shared analysis pass. Charge each root-cause fix once.

The nine non-C routes are local CLI choices, not nine independently validated
upstream frontends. In particular, the pinned launcher inventory has no Scala
source launcher. R1 must establish Scala's reference/target contract; an extra
native language feature must not inflate the required Joern-port denominator.

## Execution order and stopping criteria

1. Finish acceptance of the current macro Binding increment and preserve the
   existing green corpus. Do not claim a whole-project gain from it.
2. Complete R1 while continuing bounded C repairs. Classify the **1,082**
   differing/missing blocks into root-cause groups with minimal reproducers.
   Retained diagnostics include missing `inflate` body lowering and remaining
   `fixedtables` CODE differences; their identity/linking repairs already landed.
3. Drive both complete C project projections to zero differences, preserving
   all prior exact cases. Add full schema/edge comparison alongside this;
   do not replace the existing gate with a weaker projection.
4. Complete the required C graph, flow, query and interoperability contracts.
   Broaden repositories and build configurations before declaring C complete.
5. Bring each remaining frontend through the same differential milestones,
   then finish product workflows and the full release matrix. Compatible
   arbitrary Scala/JVM execution remains open until it has a concrete pure Rust
   design and executable acceptance contract.

Re-estimate after R1 and an initial tranche of root-cause fixes. Report newly
closed behavior families, complete project differences, unresolved controls and
actual implementation/review time. Do not infer velocity from quick builds or
fixture volume. Full 1:1 requires every required matrix row to pass; any scoped
exclusion must narrow the claim explicitly.

## Evidence and maintenance

The table values come from `projects[].current` in the committed
[thirteenth metrics](../docs/conformance/astra-thirteenth-batch-metrics.json)
(SHA-256 `35388becaf04fd5a3ba680a480a3848dd4b6a417d4f44e44e99b82eb13e8457e`).
The current candidate's complete-output continuity is recorded in the local
[fourteenth run](../.local/astra-sprint/fourteenth-batch/whole-continuity-v1/run.json)
(`24ed619e53ed8d4e564f31f9eff459502db4fbba4fe09aa8c4087721ed5b87bf`).
It reuses the pinned saved sixth-batch Joern references; no fresh whole-project
Joern run is claimed. Its source-bound successful
[build result](../.local/astra-sprint/fourteenth-batch/final-release-v1/build-result.json)
is `22226e60e948718e1b31a0f90cd3aef39fc606aff74a855798375e3afa3f77d5`.
Current test and comparison logs are in
[final-v1](../.local/astra-sprint/fourteenth-batch/final-v1/).
These `.local` receipts exist in the integration worktree and are not portable
committed artifacts; the accepted metrics remain the portable baseline.

Source anchors, with line numbers at this snapshot:

- Language routes: [CLI lib.rs](cpg-cli/src/lib.rs), lines 48–74 and 108–130;
  generic lowering: [engine.rs](cpg-lang-ts/src/engine.rs), lines 206, 278 and
  1021; local structural suite: [conformance lib.rs](conformance/src/lib.rs),
  lines 56, 86–187 and 456. Missing-route launcher inventory is from the pinned
  oracle installation, not a claim of successful runs for those routes.
- C boundaries: [COMPATIBILITY.md](COMPATIBILITY.md) and the
  [twelfth report](../docs/conformance/astra-twelfth-batch-2026-09-09.md).
  Production graph authority is in [import.rs](cpg-lang-c/src/import.rs),
  line 320; shared analysis is in [analysis lib.rs](cpg-analysis/src/lib.rs),
  lines 15, 35 and 46. Older prose saying analysis must first move out of the
  parity dumper is stale and is not a current work item.
- Schema: [schema.rs](cpg-core/src/schema.rs), lines 12–67 and 111–157;
  missing property/storage declarations: [supplemental.rs](joern-parity/src/supplemental.rs),
  lines 109–115; public JSON: [export.rs](cpg-cli/src/export.rs), lines 320
  and 339–356. Selected Import/Dependency and macro Binding repairs do not
  close these wider gaps.
- Query subset: [query.rs](cpg-analysis/src/query.rs), lines 36–57 and 83–102;
  JSON commands: [CLI lib.rs](cpg-cli/src/lib.rs), lines 522–682;
  executable commands: [main.rs](cpg-cli/src/main.rs), lines 30–78.
- Persistence: [graph.rs](cpg-core/src/graph.rs), lines 33–35 and 720–745.
  Release targets: [release.yml](../.github/workflows/release.yml), lines
  95–115. A configured workflow is not a successful run receipt.

Update this file when an acceptance checkpoint changes these measurements or
closes a work package. Keep accepted results, uncommitted candidates and
engineering estimates distinct. A new passing fixture is evidence for its
behavior, not a reason to assign an overall completion percentage.
