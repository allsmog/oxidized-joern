# Sixth measured C parity batch — 2026-09-08

Source checkpoint `e38bf19a044629e9ea6fc67248d249fb7b0321be` passes 454 workspace tests and the unchanged 308-block committed and fresh Joern v4.0.555 gates. Whole-project exact method ASTs improve from 127 to 149 on zlib and 1,132 to 1,431 on Lua, with no previously exact method lost. **Neither complete project projection is exact, and the port remains incomplete.**

The baseline is accepted source `aba030923a2eb02a7d542875e4274c1b44271608`, reached through documentation checkpoint `4b6a7ebb6531bd4371112efc27f8993d33f92670`. Work used isolated worktrees. The original checkout remains at `6fcb30a25bbee3ca3c05b5b197cd3ce93ca4d11a`; its 13 dirty files retain their original bytes. This checkpoint is local, with no push or merge. [Acceptance and provenance](astra-sixth-batch-acceptance.json) bind the source, binaries, commands and reviews.

The frontend now carries typedef-name visibility through supplied quoted headers, lexical scopes and temporary macro parse trees. This distinguishes casts from calls when tree-sitter represents both as `(T)(value)`. Typedef-name existence is separate from resolving its underlying type: a plain unknown alias can establish a type name, while an invalid sized-type combination must not replace an earlier valid binding. Header return resolution preserves individually valid typedef fragments beside unrelated parse errors. Callable identity instead comes from a complete valid expanded function declaration, preventing recovered type tokens from becoming false function names.

Declaration macros use the macro state at each source position. Expanded return types and parameters retain their required source spelling, and generated parameter names participate in typedef shadowing. Macro-expanded sizeof rendering preserves malformed siblings while matching pinned spacing and parentheses. Location recovery uses verified source spans when an empty leading macro disappears from METHOD CODE. One inactive declaration still retains the baseline API-prefix line 7 where Joern reports line 8; that test preserves source location behavior without claiming location equality.

The pinned C parser supplies `__STDC__=1`, `__STDC_VERSION__=199901L`, and `__STDC_HOSTED__=1`. These defaults, their explicit overrides, and direct/nested expansion shapes now match the retained references. Literal macro roots use suffix-aware types; parenthesized and signed expression wrappers retain their observed ANY type. Inactive declarations keep macros visible at their source position, while their directives, includes and callable/type bindings do not become active. This repairs zlib's retained inactive array declarations without leaking inactive definitions into later code.

The nine fixture families contain 256 entries representing 252 distinct filename/source mappings. The frozen fifth binary matches 45 unique complete projections; the final binary matches 180, a gain of 135 with no losses. All 180 exact unique projects have full regression gates. The table uses the same accepted fifth baseline for every family, unlike some historical standalone package measurements.

| Fixture family | Entries | Fifth → final exact entries | Full gated projects |
|---|---:|---:|---:|
| sizeof-expansion | 39 | 11 → 23 | 23 |
| macro-cast-context | 46 | 6 → 36 | 32 |
| declaration-macros | 36 | 6 → 29 | 29 |
| sixth-context-interactions | 18 | 0 → 14 | 14 |
| typedef-existence | 18 | 1 → 7 | 7 |
| header-return-bindings | 31 | 6 → 19 | 19 |
| predefined-c-macros | 25 | 11 → 25 | 25 |
| numeric-macro-roots | 26 | 4 → 20 | 20 |
| inactive-declaration-context | 17 | 0 → 11 | 11 |

Four sizeof projects appear in both the cast and interaction families. Their duplicate exact entries explain 184 exact entries versus 180 unique exact projects; they are gated once each. Three other graphs became exact only in the combined implementation and are covered by `sixth_integration_promotions.rs`. All 72 remaining unique nonexact projects retain their complete expected graphs and differences. No reference records were removed to assert a narrower outcome. [The fixture receipt](astra-sixth-batch-acceptance.json) records the independent raw CASE extractions, all 504 baseline/current producer runs, deduplication and gate extension.

Fresh whole-project Joern runs under JDK 21 reproduce every selected record of the earlier references, preserving order and duplicate multiplicity. Inputs remain 26 zlib 1.3.1 files / 22,732 source lines and 61 Lua 5.4.7 files / 30,098 lines. No input bytes, supplied compiler definitions, exclusions or comparison rules were changed to improve the result. The unchanged fourth-batch oracle script was used for these fresh runs.

| Complete projected AST comparison | Zlib, fifth → final | Lua, fifth → final |
|---|---:|---:|
| All METHOD ASTs, including scaffolding and stubs | 127 → 149 of 410 | 1,132 → 1,431 of 2,274 |
| Strict primary source METHOD ASTs | 14 → 32 | 214 → 510 |
| Primary function BLOCK ASTs | 27 → 40 | 249 → 537 |
| Nonempty primary function BLOCK ASTs | 10 → 23 | 227 → 515 |

Primary comparisons require the same method identity and complete source CODE and exclude global/macro scaffold methods. A body comparison includes the complete first immediate BLOCK subtree. No formerly exact primary method or body is lost. These counts are selected-projection results, not language-coverage percentages or proof that every incident edge matches. [Complete counters](astra-sixth-batch-metrics.json) retain every AST, NODES, EDGES and FLOWS comparison, including multiplicity. The final outputs still differ substantially: the Rust sides contain 498,779 nonempty selected records on zlib and 892,781 on Lua.

The first combined candidate passed 443 workspace tests and the main committed/live gate but regressed the previously exact Lua methods fitsC, isCint and getendpos. Independent review traced these to malformed typedef visibility and header callable resolution. The second candidate restored Lua but lost the zlib MAX_MATCH and LENGTH_CODES macro stubs when newly inactive declarations received empty macro contexts. Both rejected candidates, full outputs and measurements remain recorded. The final source restores both stub ASTs and all 10 affected matching graph facts.

All 49 earlier operand/flow repairs remain at unique source occurrences with matching complete endpoint and ancestor records. The three earlier lost stubs remain restored, and the reviewed gz_init allocation-call types match Joern. Forty previously matching snprintf scaffold fragments disappear when an incorrect inactive 23-argument call is removed. The three actual Joern snprintf call sites were already absent in the fifth output and remain a parser limitation. This is recorded explicitly in the full review, with an erratum correcting an earlier manual 13/37 split to the mechanically verified 10/40 split.

All final checks pass: 454 workspace tests with no failed, ignored or filtered tests; 17 checker tests; formatting; strict all-target Clippy; the audit of 73 locked dependencies; 308 committed and fresh live comparisons; native macOS ARM64 release and extracted archive checks; and official real-project acceptance. Cache shape is 17. Container and other-platform execution were not run in this batch.

| Repeated final build measurement | Zlib | Lua |
|---|---:|---:|
| Two build times | 7.00 / 6.56 s | 6.47 / 10.27 s |
| Maximum build RSS | 485.81 MiB | 463.48 MiB |
| Unchanged build limits | 20 s / 512 MiB | 20 s / 1,024 MiB |
| Separate clean/update equivalence | 26 files, 30.26 s | 61 files, 31.54 s |
| Separate update RSS | 644.08 MiB | 682.80 MiB |

Both repeated graph, edge, JSON export and SARIF hashes are deterministic. Update RSS is measured separately from the build ceilings. The official gate also passes with its own timings and RSS, retained verbatim in the acceptance receipt. Only eight expected manifest values change: node counts and graph/edge/export hashes for the two projects. Inputs, archive pins, licenses, exclusions, zero-finding SARIF expectations and all budgets are unchanged. Independent review verified actual artifacts and the fresh Project build path.

The batch ran from 2026-09-08T14:57:08.343230+00:00 through final validation at 2026-09-08T17:03:21.393601+00:00, about 126.2 elapsed minutes including parallel work and rejected candidates. This is measured batch time, not an estimate of the remaining port. Small fixture sources and full references/differences are committed. Large whole-project outputs, frozen binaries and validation logs remain local with recorded paths and hashes.

The next bounded repair is method full-name parsing: six existing and eight fresh complete graphs expose 12 missing CFG edges and one method-origin ReachingDef edge because spaces truncate FULL_NAME values. The retained diagnosis proposes preserving the complete property without changing CODE parsing or solver behavior. Broader include/build context, preprocessing, parser recovery, type/member resolution, schema and query behavior remain incomplete. The text projection omits properties and uses non-injective escaping. Joern binary compatibility, the Scala console/CPGQL, JVM plugins, the complete query library and proven non-C frontend parity remain outside this checkpoint. [Compatibility boundaries](../../cpg-rs/COMPATIBILITY.md) remain explicit.
