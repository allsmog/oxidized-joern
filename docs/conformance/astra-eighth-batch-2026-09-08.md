# Eighth measured C parity batch — 2026-09-08

Source `26f4cef3775c0f77988187e04bc1693e8d43fc0d` extends function-body preprocessor branch selection. It passes **460 workspace tests** and the unchanged **308/308 committed and fresh Joern v4.0.555 comparisons**. The port remains incomplete: neither full zlib nor Lua projection is exact. [Acceptance evidence](astra-eighth-batch-acceptance.json) binds the source, references, binaries, tests and reviews.

Statement emission, pending-reference discovery, declaration-shadow discovery and typedef context now use the same selected preprocessor branch. `#if/#elif/#else` conditions use the existing evaluator and source-position method macro snapshot. Directive condition identifiers and inactive declarations no longer create or suppress active phantom locals. The change also recognizes whitespace after `#` in existing `undef` readers and retains macro/type snapshots for inactive function headers without activating their bodies. Cache shape advances from 18 to 19.

The primary new family retains **34 complete reference graphs: 9→30 exact**, with no formerly exact graph lost. All 30 exact projects have full regression comparisons; four complete diagnostics remain for body-local macro updates and comment-separated, form-feed and spliced-keyword directives. The references contain 4,371 canonical lines /4,247 nonempty selected records. Independent replay gains 623 raw matching records and loses one raw ordinal coincidence: the parameter-to-method-exit fact remains at its new address. [Fixture sources and boundaries](../../cpg-rs/joern-parity/tests/fixtures/body-preprocessor/README.md) retain every full before/current/reference difference.

Earlier candidates exposed inactive-local shadowing and a previously exact zlib `byte_swap` regression. Shared branch discovery, spaced `undef` recognition and the inactive-header snapshot repair address those causes. Candidate source, binaries and full failures remain recorded. The existing inactive-duplicate location assertion changes from line 7 to line 8 because both METHOD and METHOD_RETURN now match the retained live Joern location; its canonical reference is unchanged.

On the unchanged 26-file zlib and 61-file Lua inputs, all three actual `snprintf` call records in `gz_open`, `gzdopen` and `gz_error`, plus the proper stub, are restored. This is separate from the erroneous inactive 23-argument scaffold removed in the sixth batch. Independent whole producers reproduce the integrated binary's output byte for byte.

| Complete projected AST measure | Zlib, before → current | Lua, before → current |
|---|---:|---:|
| Exact METHOD ASTs, including stubs |149 → 160 of 410|1,431 → 1,434 of 2,274|
| Exact primary source METHOD ASTs |32 → 42|510 → 513|
| Exact primary function bodies |40 → 42|537 → 540|
| Exact nonempty primary bodies |23 → 23|515 → 518|

No formerly exact method or primary body is lost. These counts do not represent overall port percentages. The raw matching-line counters gain 1,377/3,304 and lose 1,501/509 on zlib/Lua. Source-occurrence review preserves full endpoint/ancestor context and duplicate multiplicities while separating address coincidences from semantic changes; the earlier priority repairs remain. [Complete counters and independent findings](astra-eighth-batch-metrics.json) retain the full comparison. Whole-project Joern references reuse the sixth batch's pinned live outputs; the main 308-block comparison was regenerated live for this batch.

Two boundaries remain visible. Lua `math_log` exposes a runtime `else` split from its following `if` by `#endif`. A [separate two-project diagnostic](../../cpg-rs/joern-parity/tests/fixtures/guarded-else-chain-diagnostic/README.md) retains complete fresh graphs: the ordinary control is exact in both binaries but is not a new production gate, while the guarded chain remains nonexact. In the reduced method, prior correct CFG/RD source-occurrence facts are retained and 23 are gained; five live facts remain missing, and an incorrect edge from the first assignment to the second condition remains. Supplied body includes also remain incompletely lowered in zlib `fixedtables`; its array declarations and identifier type/CODE gaps predate this batch.

A provisional review incorrectly described the existing `fixedtables` `<unknown>` identifier CODE as a new regression. Direct comparison disproved it: all four complete identifier records in both source files are byte-identical, with only addresses moving. The rejected claim, failed assertion and corrected evidence remain recorded. An initial root check matched no Rust methods because it used unqualified names; it is explicitly superseded by a check requiring both qualified methods and all four records. No source changes or reruns were made on the basis of that claim.

All final gates pass: 460 workspace tests with no failed, ignored or filtered tests; 17 checker tests; formatting; strict all-target Clippy; the audit of 73 locked dependencies; committed/live 308 comparisons; native macOS ARM64 release and extracted archive checks; and official real-project acceptance. No container or other-platform run is claimed.

Two fresh repeated builds pass unchanged limits: zlib 6.53/6.47 s, maximum 488.80 MiB; Lua 5.84/5.87 s, maximum 458.31 MiB. Build ceilings remain 20 s and 512/1,024 MiB. Clean/update equivalence passes for 26/61 files; its separate memory measurements are 645.45/737.48 MiB and are not compared with build ceilings. All graph/edge/JSON/SARIF hashes repeat identically, with zero scanner findings.

Exactly eight expected manifest fields change: node count and graph/edge/JSON-export hashes for each project. Node counts become 65,856 zlib and 119,090 Lua. Source counts and bytes, archives, licenses, exclusions, budgets and zero-finding SARIF expectations stay unchanged. The official acceptance output is retained verbatim in the receipt.

This checkpoint follows source `8cb96c205cecc8d993b847ebf1a3ae00a0453a3b` and remains local on `codex/astra-parity-sprint`, with no push or merge. The original checkout's 13 dirty files retain their bytes. Small fixtures and full references are committed; large validation artifacts remain local with bound paths and hashes.

Next: body-local macro state. Seventeen complete fresh references, two currently exact, pin definitions and undefinitions across statements, C scopes, later functions and includes. The proposed shared source-order snapshot pass must preserve separate typedef scopes and explicit recovery contexts. The runtime else-chain, body-include, parser, schema, query and non-C gaps remain in the [compatibility boundaries](../../cpg-rs/COMPATIBILITY.md).
