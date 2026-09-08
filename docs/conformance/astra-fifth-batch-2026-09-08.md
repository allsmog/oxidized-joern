Fifth measured C parity batch — 2026-09-08

The Rust frontend now matches additional Joern cases for field macro expansion, nested casts, typedef aggregate bodies, and recovery of malformed macro expressions. Macro argument binding preserves operator and type tokens that the C parser does not represent as expression children. **Source checkpoint: `aba030923a2eb02a7d542875e4274c1b44271608`.** The final checks and whole-project measurements below describe this frozen source. This remains a bounded C parity increment; neither whole-project graph is exact. The [metrics](astra-fifth-batch-metrics.json) and [acceptance receipt](astra-fifth-batch-acceptance.json) bind the complete comparisons and validation evidence.

Work started at 13:14:11 UTC from checkpoint `d68417922e63cbbdb3da4cd9c05419bca702b442`, whose accepted source is `862e54ee5a4cd6efaff156bb0f844090ebce5e89`. Implementation and independent reviews used isolated worktrees. The original checkout and its 13 dirty audit/planning files remain unchanged. Nothing was pushed or merged.

Joern v4.0.555 under JDK 21 remains the reference. The main 42-file C corpus and new small fixture projects were checked against fresh live outputs. Whole-project comparisons reuse the saved, unfiltered fourth-batch Joern outputs with the same oracle script and unmodified source selections: 26 zlib 1.3.1 files / 22,732 source lines and 61 Lua 5.4.7 files / 30,098 source lines. No compiler definitions, exclusions or input bytes were changed to improve the comparison.

Field-token expansion now handles the pinned direct and nested member/index cases, preserves original invocation CODE where Joern does, and retains the expanded field name. Macro casts use token expansion before expression parsing; declaration spelling and type identity remain separate. Strings, comments, Unicode identifiers and preprocessing numbers have boundary cases, while recursion and work limits prevent unbounded rescanning. The full field/cast interaction suite retains nonexact receiver and type-resolution diagnostics.

Typedef aggregates now retain their bodies, members, aliases, and initializer methods for the pinned tagged, anonymous, pointer and array forms. Macro dimensions use the declaration's source-position context. This restores Lua's `RN.<clinit>` and its correct `L_MAXLENNUM` call site. Repeated anonymous/tagged identities across translation units still expose type/initializer coalescing differences; the complete diagnostic is retained.

Malformed macro arguments involving unresolved tokens between string literals now recover in the pinned declaration and expression contexts. The recovered symbol state follows lexical scope, and known function values retain Joern's recovered identifier behavior. Pending phantom locals follow the last eligible reference order; direct callees and method references do not reorder them. `sizeof` type identifiers use declaration specifiers rather than pointer, array or function declarators. Bare malformed string initializers, malformed for initializers, and an unresolved typedef-name `sizeof` case remain nonexact.

A complete whole-project review found an introduced operand omission even though the earlier candidate passed 428 workspace tests and the main 308-block gate. In `intop(+, v1, v2)`, the parser's named-child argument list omitted the operator-only slot, shifting macro parameters. The repaired implementation scans balanced preprocessing-token arguments for direct and nested invocations, preserving operator/type/empty slots, quoted text and comments. Line splicing precedes comment recognition. Copied expression arguments retain their original macro parameter indices. The full Lua replay restores the specific ADD right operand and its `v2` flow to the parameter-out node, as well as both shift right operands. No reaching-definition solver change was needed for this repair. A shallow endpoint fingerprint had initially confused the missing ADD operand with a similar node in the SHR branch; the final review uses the full ancestor path and source occurrence. Historical failed and pre-repair outputs remain recorded.

| Complete exact fixture family | Isolated projects | Nonempty selected records |
| --- | ---: | ---: |
| field-macros | 10 | 2,309 |
| direct-field-macros | 20 | 5,187 |
| nested-macro-casts | 5 | 3,090 |
| typedef-aggregates | 26 | 3,570 |
| macro-recovery | 48 | 10,787 |
| field-cast-interactions | 3 | 1,270 |
| direct-macro-arguments | 12 | 3,092 |

These are complete outputs of the existing selected AST/NODES/EDGES/FLOWS projection for isolated projects. They are additional tests outside the unchanged main 308-block corpus. Exact fixture counts are not coverage percentages. Nonexact cases are retained with complete outputs and differences; some diagnostics additionally have targeted assertions for repaired operands, without claiming their entire graph is exact.

| Complete selected AST comparison | zlib baseline → final | Lua baseline → final |
| --- | ---: | ---: |
| All exact method ASTs, including stubs | 127 → 127 | 1,108 → 1,132 |
| Exact primary source-method ASTs | 14 → 14 | 197 → 214 |
| Exact primary body BLOCK subtrees | 25 → 27 | 226 → 249 |
| Exact nonempty primary body BLOCK subtrees | 8 → 10 | 204 → 227 |

The oracle has 410 total method ASTs for zlib and 2,274 for Lua. All-method counts include macro/operator stubs and scaffolding. Primary comparisons require matching method identity and source CODE and exclude preprocessor directives and global scaffolding; complete body comparisons include the owning direct BLOCK and every descendant. No previously exact all-method AST, primary method AST or primary body is lost against the fourth baseline. A complete method AST match does not prove that all incident edges and flows are exact. Raw edge addresses shift with AST order, so raw record intersections and differences are not semantic bug counts. The metrics preserve duplicate multiplicity and report AST separators separately from nonempty graph records.

All three stubs lost in the fourth batch are restored at their correct call sites: `liolib.c:L_MAXLENNUM:int(0)` from `RN.<clinit>`, and `lua_tointeger` / `lua_tonumber` from recovered `g_write`. All four earlier repaired reaching-definition facts in `checkclosemth`, `checktoclose`, `freereg` and `singlevar` remain preserved. `intarith` and `luaV_shiftl` still have typedef-cast/pointer-call differences; repaired operands do not make those complete methods exact.

Final validation passes all 432 workspace tests with no failed, ignored or filtered tests, all 17 checker tests, formatting, strict Clippy, and the dependency audit over 73 crates. The main corpus remains exact at 308/308 against committed and fresh live references, including 3,685 reaching-definition facts. Native release acceptance, the extracted macOS ARM64 archive and official real-project acceptance pass. The cache shape is version 16. No container or other-platform execution is claimed for this batch. The earlier checker and dependency logs remain applicable because their code and Cargo.lock are unchanged; production tests, build and live differential gates were rerun after the final repair.

| Final measured operation | zlib 1.3.1 | Lua 5.4.7 |
| --- | ---: | ---: |
| Build time, two runs (s) | 6.44–6.89 | 4.96–4.97 |
| Build peak RSS, two runs (MiB) | 482.98–485.20 | 471.12–472.03 |
| JSON export peak RSS (MiB) | 53.69–54.11 | 76.80–77.11 |
| Existing build ceiling | 20 s / 512 MiB | 20 s / 1,024 MiB |
| Update-equivalence time / RSS | 19.33 s / 638.69 MiB | 14.90 s / 674.06 MiB |
| Saved nodes / scan findings | 66,237 / 0 | 119,201 / 0 |

Repeated graph, edge, JSON export and SARIF bytes match, and incremental update equals clean rebuild for all 26/61 files. The manifest refresh changes only expected node counts and graph/edge/export hashes after independent review. Input archives, source selection, licenses, exclusions, scan expectations and build budgets remain unchanged. Per-command RSS comes from `/usr/bin/time -l`; the official gate reports a cumulative child-process peak, so its Lua printed peak may include the earlier zlib update. Update RSS is reported separately and is outside the existing build RSS ceilings. Concurrent work makes these host observations unsuitable as a model-speed benchmark.

The port still lacks whole-project C equality, full preprocessing and build-context support, general typedef/member resolution, full schema coverage, and proven parity for every analysis/query behavior. Supplied quoted relative headers are supported only within the provided file set; external include paths, angle includes and build definitions are broader gaps. Function-like macros in preprocessor conditions, block directives, variadics, stringification and token pasting remain incomplete. A source parser can truncate or omit unsupported invocations before the token scanner sees them, including the retained trailing-empty and split-comment/operator diagnostics. Expansion bounds do not guarantee complete parsing. The selected projection omits column numbers and some control metadata; newline escaping in flow labels is not injective or lossless. Joern binary compatibility, Scala console/CPGQL, JVM plugins, the complete query library and non-C frontend parity are not delivered by this batch.

The integration and acceptance window ends at 14:46 UTC on 2026-09-08. Its duration includes parallel work, reviews, validation and infrastructure interruptions; it is neither person-hours nor a delivery-time forecast. Source and small reference fixtures are committed locally. This report, counters and the acceptance receipt record the checkpoint for review. Large whole-project outputs, frozen executables, detailed reviews and command logs remain at hash-bound ignored local paths, so a fresh clone does not include those raw artifacts. The five typedef-aggregate raw producer logs also remain in hash-bound scratch paths; that family’s source inputs and complete expected graphs are portable, while the other six new fixture families bundle their raw oracle output. The next increment should resolve the remaining typedef-cast context in the retained Lua and zlib examples, while continuing complete graph comparisons and source-occurrence checks for lost facts.
