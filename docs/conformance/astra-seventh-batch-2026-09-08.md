# Seventh measured C parity batch — 2026-09-08

Source `8cb96c205cecc8d993b847ebf1a3ae00a0453a3b` preserves complete METHOD FULL_NAME values containing spaces. It passes **458 workspace tests** and the unchanged **308/308 committed and fresh Joern v4.0.555 comparisons**. The port remains incomplete: neither full zlib nor Lua projection is exact. [Acceptance evidence](astra-seventh-batch-acceptance.json) binds the source, tests, raw references, binaries and reviews.

A method such as `VALUE:unsigned int(0)` previously ended at `unsigned` during graph-edge construction. A header named `a b.h` was reduced to `a`, which could attach edges to an unrelated function named `a`. The reader now preserves the final serialized FULL_NAME property through its following property boundary. Existing CODE parsing and the CFG/dataflow solvers are unchanged; cache shape advances from 17 to 18.

The new family retains **31 complete graphs**, with **3→17 exact** and no previously matching selected record lost. All 17 exact projects have full graph regression comparisons. Independent replay verifies 23 matching structural and 20 matching reaching-definition records gained, preserving duplicates. Three production tests and one direct parser test cover complete graphs, the prefix collision and the introduced comment/string boundary. The remaining 14 full diagnostics are retained, including five preexisting importer aborts on filenames containing property markers. These are explicitly outside the exact-case count. [Fixture sources and scope](../../cpg-rs/joern-parity/tests/fixtures/spaced-method-names/README.md) retain all 5,334 canonical lines / 5,183 nonempty reference records.

The first candidate selected a marker inside valid C comments or strings and lost 10 previously matching facts. Independent review caught this before root integration; selecting the final serialized property closes the regression. Its source, binary and full failing outputs remain recorded. An initial review receipt sampled a mutable freeze path after replacement; the original receipt is preserved alongside a corrected binding and an independent replay of the retained first binary. The final review uses immutable copies.

On the unchanged 26-file zlib and 61-file Lua inputs, the complete before/after difference is **four added CFG edges and no removed records**. All four edges match the pinned Joern reference: zlib's BASE macro and Lua's RANLIMIT, MAXUNICODE and MAXUTF macros now connect their METHOD entry to METHOD_RETURN. AST, NODES and FLOWS bytes are unchanged, preserving every earlier repaired fact. Independent producers reproduce the final outputs byte for byte. Whole-project Joern output is reused from the sixth batch; the main 308-block gate was regenerated live in this batch.

| Complete projected AST measure | Zlib | Lua |
|---|---:|---:|
| Exact METHOD ASTs, including stubs |149/410|1,431/2,274|
| Exact primary source METHOD ASTs |32|510|
| Exact primary function bodies |40|537|
| Exact nonempty primary bodies |23|515|

These measures are unchanged and do not represent overall port percentages. [Complete counters](astra-seventh-batch-metrics.json) retain all selected records and multiplicities. Zlib's BASE METHOD still has a preexisting trailing-comment CODE difference despite its restored edge.

All final gates pass: 458 workspace tests with no failed, ignored or filtered tests; 17 checker tests; formatting; strict all-target Clippy; the audit of 73 locked dependencies; committed/live 308 comparisons; native macOS ARM64 release and extracted archive checks; and official real-project acceptance. No container or other-platform run is claimed.

The first resource attempt failed the unchanged 20-second zlib limit at 21.15 s; its second trial took 14.94 s with the same output hashes. Two attempted retries stopped at their process prechecks before launching the helper. After three idle samples, the unchanged binaries passed repeated builds: zlib 6.55/6.60 s with maximum 485.19 MiB, and Lua 5.85/5.83 s with maximum 462.06 MiB. Build ceilings remain 20 s and 512/1,024 MiB. Clean/update equivalence passes for 26/61 files; its separate memory measurements are 636.78/673.77 MiB and are not compared with build ceilings. Both failed and passing resource evidence remain recorded; host contention is a possible contributor, not a proved explanation.

Only six expected graph/edge/JSON-export hashes change in the real-project manifest. Source counts, node counts, archives, licenses, exclusions, budgets and zero-finding SARIF expectations stay unchanged. The official acceptance output is retained verbatim in the receipt.

This checkpoint follows source `e38bf19a044629e9ea6fc67248d249fb7b0321be` and remains local on `codex/astra-parity-sprint`, with no push or merge. The original checkout's 13 dirty files retain their bytes. Small fixtures and full references are committed; large validation artifacts remain local with bound paths and hashes.

Next: block-level `#if/#elif` selection. Ten fresh complete Joern examples isolate six dispatch/phantom-variable failures, three exact controls and one separate body-local macro-state diagnostic. This explains the three still-missing live zlib snprintf calls; the previously removed erroneous inactive 23-argument stub is a different issue. Broader preprocessing, parser, schema, query and non-C gaps remain in the [compatibility boundaries](../../cpg-rs/COMPATIBILITY.md).
