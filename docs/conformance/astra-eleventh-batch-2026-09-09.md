# Eleventh measured C parity batch — 2026-09-09 UTC

Primitive C MEMBER bases now use the existing declaration-type renderer. This repairs measured spelling such as `unsigned short` → `shortunsigned`, while preserving declarator suffixes, CODE, order and initializer construction. The production change is one call site, plus cache shape **21→22** so cached graphs rebuild. Full Joern parity remains incomplete. The [acceptance record](astra-eleventh-batch-acceptance.json) binds the tested source, binaries and checks; [complete counters](astra-eleventh-batch-metrics.json) retain the whole-project differences.

The fresh pinned Joern 4.0.555 run contains **17 new projects and two unchanged tiny-fixedtables anchors**. Together they improve **2→18 exact complete graphs**, gaining **126 matching nonempty records and losing none**. Tests gate all 18 exact graphs, including the two retained anchors. Sources, raw oracle output and complete references remain unchanged. The references contain 3,423 nonempty records across 3,493 canonical LF lines. The [fixture report](../../cpg-rs/joern-parity/tests/fixtures/primitive-members/README.md) records numeric, signed, qualified, pointer/array and named-type controls.

The nineteenth graph remains nonexact. Its MEMBER rows already match, but it lacks 20 external type-registration/scaffold records. Its entire output is byte-identical before and after this change. The older `member_types` diagnostic is also unchanged. The initially frozen test draft included the new diagnostic; full replay established the gap, and the final test classifies it separately. That draft and the full diagnostic remain in provenance; no previously passing gate was removed. [Independent source/package review](../../cpg-rs/joern-parity/tests/fixtures/primitive-members/provenance/independent-source-package-review/package-review.json) verifies this boundary.

All **228 planned prior case instances plus the older diagnostic** were replayed: **203→205 exact**. The 228 successful pairs gain 14 matching records with no losses, entirely from the same two tiny anchors already counted above. These gains are not added twice. The sole importer failure returns **−6/−6**, with empty stdout and both differing stderr transcripts retained. It is excluded from successful-pair semantic counts. Although the replay helper contains CR normalization, all 458 actual raw stdout files are byte-identical to their stored text, and independent differences use LF-only splitting. [The prior review](../../cpg-rs/joern-parity/tests/fixtures/primitive-members/provenance/independent-prior-review/summary-review.json) preserves every complete result and failure.

Whole-project comparison uses the unchanged **26 zlib /61 Lua files** and explicitly reuses the sixth batch's saved live Joern outputs. This is not a new whole-project Joern run. Zlib gains **21 matching records**, Lua **11**, with **zero matching losses**. Raw additions/removals remain counted: **+25/−20 zlib**, **+19/−19 Lua**. All 12 changed MEMBER type properties match the corresponding live source occurrences; six complete MEMBER rows become exact, while six retain existing ORDER differences. Complete flow sections and every other AST property/topology are unchanged. [The source-occurrence review](../../.local/astra-sprint/eleventh-batch/final/whole-review.json) binds all changed type/scaffold edges and full source context.

| Retained exact AST measure | Zlib | Lua |
|---|---:|---:|
| METHOD trees, including stubs |160 of 410|1,438 of 2,274|
| Primary source METHOD trees |42|517|
| Primary function bodies |42|544|
| Nonempty primary bodies |23|522|

All 61 tracked repair facts, 19 selected method trees/stubs and three actual zlib `snprintf` calls remain. Neither full project projection is exact. Both 2,219-node `fixedtables` trees are byte-identical to the accepted tenth build. Each retains six AST records with long CODE differences; the METHOD header also has the wrong FULL_NAME. Identity/linking and the absent `inflate` body remain separate gaps. The [same review](../../.local/astra-sprint/eleventh-batch/final/whole-review.json) retains complete trees and incident evidence.

Final validation passes **470 workspace tests /77 result groups**, **308/308** committed and fresh live Joern comparisons, 17 checker tests, formatting, strict Clippy, locked release/recheck and the 73-dependency audit. Native macOS ARM64 release/archive and official real-project acceptance also pass. All **145 frozen Rust/Cargo files**, including immutable fixture provenance copies, and both release binaries stay unchanged. No container or other-platform execution is claimed. [Final gates](../../.local/astra-sprint/eleventh-batch/final/gates.json) bind these results.

| Repeated build measurement | Zlib | Lua |
|---|---:|---:|
| Seconds, first /second |2.78 /2.80|5.94 /5.77|
| Peak RSS MiB, first /second |439.66 /490.03|463.41 /463.13|
| Stored nodes |70,248|119,088|

Both repetitions satisfy the unchanged 20-second and 512/1,024 MiB build ceilings and produce identical graph, edge, export and zero-finding SARIF bytes. Clean/update equivalence passes for 26/61 files in 8.41/17.42 seconds, with separate RSS 644.63/717.30 MiB. Update RSS is not compared with build ceilings; the official helper's cumulative child RSS is also a different measurement. Known task build/graph producers had finished before the repetitions; unrelated host activity was not controlled. [Independent resource review](../../.local/astra-sprint/eleventh-batch/final/resource-manifest-review.json) verifies the raw artifacts and budgets.

Exactly **seven reviewed expectations** change: zlib's node count and both projects' graph/edge/export hashes. Source/archive/license/exclusion/budget/SARIF contracts remain fixed. Official acceptance passes with that exact manifest. The original checkout and its 13 pre-existing dirty files remain unchanged. This is a local checkpoint on `codex/astra-parity-sprint`; no push or merge is claimed. Fixture READMEs preserve their worker-stage status; the [ROOT_ACCEPTANCE note](../../cpg-rs/joern-parity/tests/fixtures/primitive-members/ROOT_ACCEPTANCE.md) links to this final result.

Next: run the five proposed duplicate-function controls covering ordinary, literal-static, supplied macro-local and late-prototype identities and links. Those controls are still unrun. Broader preprocessing, type registration/recovery, source transport, dataflow, complete schema and binary compatibility, Scala queries/console/plugins and non-C parity remain open; these fixture counts do not estimate overall port completion. [Compatibility scope](../../cpg-rs/COMPATIBILITY.md) records those limits.
