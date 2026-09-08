# Third measured Astra parity batch

The combined Rust implementation passes **381 workspace tests** and **308/308
committed and fresh live Joern comparisons**. On the selected Lua project,
exact method AST blocks improve from **417 to 516**; zlib retains **93**.
No previously exact method AST regressed. Both whole-project graph projections
still differ substantially from Joern, so these results do not establish a
complete port or a completion percentage.

This checkpoint uses source `b8c2ae132edc2d0317df853673e5d94ac7ca40a5`, compared
with the [second batch](astra-second-batch-2026-09-08.md) source
`c5ea712dbfd03d114af4450f79c89629a360b6a6`. Joern **v4.0.555** under JDK
**21.0.12** remains the executable reference. Integration began at **08:36 UTC
on 2026-09-08**; final acceptance and semantic review completed at **09:42:07
UTC**, a window of about **66 minutes**. Numeric-literal and static-modifier
preparation overlapped the preceding batch's closeout. Parallel implementation
and review also contributed. This window is not from-scratch implementation
time, person-hours, or a forecast for completing Joern.

The changes remain local on `codex/astra-parity-sprint` in the sibling
`joern-oxidized-astra-sprint` worktree. The original checkout's uncommitted
audit and planning files were preserved. Nothing was pushed or merged into
the original branch. Documentation review and the checkpoint commit follow
the source acceptance timestamp.

## Changes and their measured boundaries

| Area | Verified result | Retained evidence |
|---|---|---|
| Numeric literals | Exact ASTs for the 67 literal functions improve from 27 to 67. The complete 3,420-line graph projection is exact. Radix, unsigned/long suffixes, and hexadecimal floating literals have separate cases. | [Fixture and measurement](../../cpg-rs/joern-parity/tests/fixtures/numeric-literals/README.md), [production tests](../../cpg-rs/joern-parity/tests/numeric_literals.rs). |
| Static modifiers | The three-file projection is exact across 1,161 lines and 29 method blocks, up from 13 exact blocks. Explicit leading `static` emits the modifier and corresponding child order. Token boundaries, comments, and line splices are tested. | [Fixture and measurement](../../cpg-rs/joern-parity/tests/fixtures/static-modifiers/README.md), [tests](../../cpg-rs/joern-parity/tests/static_modifiers.rs). |
| Primitive type roles | Complete exact isolated graphs improve from 6/39 to 39/39, covering 12,523 lines. Definitions, declarations, call results, parameters, locals, and function pointers keep their observed role-specific spellings. | [Cases and measurement](../../cpg-rs/joern-parity/tests/fixtures/primitive-roles/README.md), [tests](../../cpg-rs/joern-parity/tests/primitive_roles.rs). |
| Combined type/literal/static behavior | Two complete graphs, totaling 876 lines and 20 method blocks, are exact. The preceding accepted binary differed by 562 and 214 unified-diff lines. | [Combined cases](../../cpg-rs/joern-parity/tests/fixtures/combined-c-types/README.md), [tests](../../cpg-rs/joern-parity/tests/combined_c_types.rs). |
| Reaching-definition identities | Identifier and parameter names no longer share the same definition-key namespace as call-expression code. Distinct pointer left-hand sides and unnamed parameters stop killing unrelated definitions; reassignment still kills by name. Eleven new main-corpus methods retain the repair. | [Corpus](../../cpg-rs/joern-parity/corpus/method_address_definitions.c), [tests](../../cpg-rs/cpg-lang-c/tests/method_address_definitions.rs). |
| Standalone blocks | Standalone compound statements now retain their AST and control flow. The 1,708-line exact fixture passes; the expanded scanner suite improves from 3/14 to 14/14, repairing seven false negatives and four false positives. | [Graphs, outcomes, and diagnostic](../../cpg-rs/joern-parity/tests/fixtures/standalone-blocks/README.md), [graph tests](../../cpg-rs/joern-parity/tests/standalone_blocks.rs), [scanner tests](../../cpg-rs/cpg-analysis/tests/canonical_c_standalone_blocks.rs). |
| Primitive type registration | On the same 47-case accepted set, complete exact graphs improve from 22 to 47, totaling 5,708 lines. Explicit initializers trigger the additional primitive prefix registration only for the relevant ordinary-object declarator. | [Cases and 15 retained nonexact diagnostics](../../cpg-rs/joern-parity/tests/fixtures/declaration-registration/README.md), [tests](../../cpg-rs/joern-parity/tests/declaration_registration.rs). |
| Concatenated strings | Adjacent strings and the covered macro-generated concatenations produce literal expressions. The complete 1,589-line fixture, including 255 reaching-definition facts, is exact. This fixes the Lua build failure exposed by preserving standalone blocks. | [Fixture, measurement, and diagnostics](../../cpg-rs/joern-parity/tests/fixtures/concatenated-strings/README.md), [tests](../../cpg-rs/joern-parity/tests/concatenated_strings.rs). |

Each fixture measurement records its own baseline and candidate. Numeric
preparation originally used `6e50d916b`; the integration also remeasured
numeric, static, and combined fixtures with the common `c5ea712db` binary.
Registration's 22-to-47 comparison starts after the primitive-role and
definition-identity repairs. These measurements are not additive work units.
The [acceptance receipt](astra-third-batch-acceptance.json) binds all seven
fixture measurement files and the common-baseline replay by SHA-256.

The numeric fixture's 67 literal functions are a subset of its 71 total
method blocks. Static behavior follows the pinned Joern output, including
cases where `static` appears after another token and no modifier is emitted;
`MODIFIER_TYPE` is outside the selected projection. Primitive types likewise
retain Joern's differing spellings: an `unsigned long` definition, declaration,
and call result can use `unsigned long`, `longunsigned`, and `unsigned longint`.
This is observed compatibility, not a general C type-normalization rule.

The standalone-block scanner suite includes four negative outcomes. Its
expanded 2,042-line graph diagnostic still differs by **five preexisting
reaching-definition facts**: four missing loop-tail facts and one extra
decrement-to-exit fact. A counterexample without standalone blocks reproduces
the same differences. The block repair leaves the solver unchanged. Possible
flow after a terminating block can agree with Joern without establishing
execution-path feasibility.

## Regressions found and repaired before acceptance

Preserving standalone blocks exposed an unsupported concatenated-string
operand inside Lua's `addliteral` ternary, at `lstrlib.c:1190–1192`. The
intermediate Lua build panicked while constructing the conditional CFG
because one operand had been omitted. The repair emits the missing literal
expression and handles the covered macro-generated adjacent strings. The
final build succeeds on all 61 selected Lua files. The original failure log
and binary hashes remain under the local third-batch evidence paths.

Independent review also caught a primitive-registration change that removed
an existing tagged-struct base type. The final condition applies only to
primitive ordinary-object declarations; retained tag and mixed-callback
tests protect the prior behavior. Static-token review caught identifier
boundaries and physical line-splice cases before integration. Full workspace
tests run against the combined source, in addition to the isolated reviews.

The concatenated-string work exposed physical newlines inside reaching-
definition variable labels. The shared [oracle driver](../../cpg-rs/joern-parity/oracle.sc)
now escapes LF as literal `\n` before sorting flow records, as the text
projection already does for other code fields. **This encoding is not
injective or lossless:** a physical newline and a literal backslash-n may
collide. CRLF rendering differences remain in a retained diagnostic, with
byte-preserving fixtures and a separate transport proof.

Both whole-project Joern outputs were therefore regenerated from the same
verified input bytes using the corrected driver. Independent replay recovered
all prior multiline labels and confirmed that LF escaping alone explains the
flow transport changes: **368 zlib** and **388 Lua** multiline records, with
all non-FLOWS records unchanged in order. Both old and new Rust binaries are
compared against these fresh outputs. Historical second-batch flow counters
retain their original transport; the before/after counters in this report's
metrics use the corrected transport throughout. Expected graph values were
not hand-edited, and no differences were filtered away.

## Main corpus and whole-project comparisons

The main committed reference now contains **42 C files / 563 source lines**,
with **291 method blocks and 17 section blocks: 308 comparison blocks**.
The definition-identity corpus adds 11 methods; the other new fixtures above
remain separate complete comparisons exercised by workspace tests.

| Main reference records | Count |
|---|---:|
| Nonempty AST records | 5,363 |
| NODES records | 534 |
| EDGES records | 9,532 |
| FLOWS records | 3,685 |

The complete canonical reference has 19,405 lines including AST separators.
Its SHA-256 is
`0cd3403a367ae297f2ddf60b70684f0b84cd81c9e2ca0c149f6663749df70abf`.
Committed and fresh strict-live checks both pass **308/308**. The
[checker contract](../../cpg-rs/joern-parity/README.md) defines the nine AST
properties, 15 structural edge kinds, and reaching-definition projection.

Whole-project comparisons use all **26 selected zlib 1.3.1 core files /
22,732 lines** and **61 Lua 5.4.7 files / 30,098 lines**, without source edits
or added compiler defines. [Complete metrics](astra-third-batch-metrics.json)
bind the inputs, oracle, binaries, raw outputs, full differences, record
counters, and identities of newly exact methods.

| Whole-project method blocks | Joern | Rust baseline → current | Identical baseline → current | Shared but changed now | Joern-only now | Rust-only now |
|---|---:|---:|---:|---:|---:|---:|
| zlib core | 410 | 313 → 313 | 93 → 93 | 186 | 131 | 34 |
| Lua | 2,274 | 2,024 → 2,039 | 417 → 516 | 1,120 | 638 | 403 |

| Current whole-project records | Joern | Rust | Exact records in common |
|---|---:|---:|---:|
| zlib AST | 85,865 | 33,664 | 27,610 |
| zlib NODES | 785 | 681 | 537 |
| zlib EDGES | 184,740 | 66,980 | 20,224 |
| zlib FLOWS | 200,374 | 47,521 | 3,624 |
| Lua AST | 252,489 | 155,212 | 126,099 |
| Lua NODES | 3,229 | 2,974 | 2,767 |
| Lua EDGES | 504,016 | 293,512 | 105,613 |
| Lua FLOWS | 207,820 | 157,081 | 29,776 |

There are **99 newly exact Lua method ASTs and zero previously exact method
AST regressions** across both projects. Neither complete projection is exact.
Method counts include scaffold, operator, and macro methods; exact ASTs do
not imply exact incident graph edges. Record intersections preserve duplicate
counts. Edge addresses use AST line indices, so changed AST lines can amplify
edge and flow differences. These are not feature-completion percentages or
independent bug counts.

## Production graph review and acceptance

Independent review compared complete production JSON exports before and
after the batch. All **141 zlib and 1,130 Lua source-body methods**, and all
**394 zlib and 1,796 Lua declaration-code locals**, were retained with their
owning methods and multiplicity. Lua gains 149 locals. Retained calls preserve
source locations and target edges; retained locals preserve reference edges.
The four repeated `p = &curr->next` assignments in `lgc.c` still retain their
individual owning methods and lines 839, 981, 1098, and 1144.

All 16 removed Lua CALL records have corresponding replacements from the
adjacent-empty-string spelling repair, with the same ownership, locations,
and multiplicities. All 771 new Lua modifiers attach to leading-static
methods. The 15 new Lua methods comprise seven macro scaffolds and eight
external stubs. Removed zlib type spellings and two synthetic allocation
type operands have classified replacements. This retention review explains
the changes; whole-project Joern equivalence is assessed separately above.
The acceptance receipt binds the independent review and its replay script
through the review file's hashes.

| Integrated gate | Result at `b8c2ae132` |
|---|---|
| Locked workspace tests | 381 passed; zero failed |
| Formatting and strict workspace/all-target Clippy | Passed |
| Checker tests; committed and fresh strict-live parity | 17 tests; 308/308 in each parity check |
| Dependency audit | Passed for 73 locked dependencies |
| Native release and extracted macOS ARM64 archive acceptance | Passed |
| Repeated zlib/Lua outputs and incremental-vs-clean equivalence | Passed |
| Official real-project manifest gate | Passed with unchanged inputs and budgets |
| Container and other-platform tests | Not run in this batch |

| Final CLI measurement | zlib core | Lua |
|---|---:|---:|
| Graph nodes | 18,076 | 82,939 |
| Graph edges | 131,773 | 529,750 |
| Build time, two runs | 5.157–5.974 s | 3.340–3.434 s |
| Maximum measured build RSS | 148.42 MiB | 381.23 MiB |
| Maximum measured export RSS | 255.34 MiB | 1,010.27 MiB |
| Scan time, two runs | 0.052–0.082 s | 0.408–0.411 s |
| Incremental-vs-clean equivalence, measured run | Passed, 15.44 s | Passed, 9.71 s |
| Built-in scan findings | 0 | 0 |

Both repetitions produce identical graph, edge, JSON export, and SARIF
hashes. Independent re-exports using the final CLI also match exactly.
The official acceptance run separately reports incremental equivalence in
22.24 seconds for zlib and 10.20 seconds for Lua. Its RSS is a cumulative
child-process high-water measurement; the table uses per-command RSS.
Timings are host-specific diagnostic measurements, not a Joern performance
comparison. Zero built-in findings establish deterministic output, not the
absence of vulnerabilities.

Only eight expected values changed in the
[real-project manifest](../../cpg-rs/acceptance/real-projects/manifest.json):
node count and graph, edge, and export hashes for each project. Inputs,
releases, licenses, exclusions, findings, SARIF hashes, and budgets remain
unchanged. The build ceiling remains 20 seconds; memory ceilings remain
512 MiB for zlib and 1,024 MiB for Lua. Incremental checks have their own
larger time budget. Lua export is close to its memory ceiling. Cached C graph
shape advances from version 13 to **14** so saved graph caches rebuild after
these structural changes.

The [acceptance receipt](astra-third-batch-acceptance.json) binds source,
reference, fixture measurements, binary and archive hashes, final logs,
manifest refresh, and semantic review. Local logs are under
`.local/astra-sprint/third-batch/final/`; fresh whole-project oracles are under
`third-batch/fresh-real-oracles/`, and complete comparison outputs are under
`third-batch/final-real-differential/`. Repeated production artifacts are under
`.local/astra-sprint/real-project-review/*/third-final-{1,2}/`. These ignored
artifacts remain local to this host. Committed fixtures, generated references,
tests, metrics, and the receipt provide the tracked evidence.

## Remaining work

The next bounded repair should target the five retained loop-tail
reaching-definition differences. It needs a faithful account of the pinned
solver's definition universe, parameter-out scheduling, and exit states,
with both the standalone-block and no-block counterexamples retained. The
existing 14 scanner outcomes, especially the four negatives, must remain
intact, followed by full-corpus and whole-project comparison.

Other confirmed C gaps include header and build-definition context,
function-like macros in preprocessor conditions, macro comment collection
and some nested expansion or wrapper types, array initializers, tagged and
alias types, `(void)` callback signatures, and CRLF source rendering. The
registration suite retains 15 nonexact diagnostic cases; the string suite
retains macro, unresolved-expression, and CRLF diagnostics. Some unresolved
expression probes are invalid C and are labeled accordingly. Preprocessor
expansion has explicit work and depth bounds.

Beyond C, experimental frontends still need their own live Joern suites.
Full schema/property coverage, CPGQL and Scala-console source compatibility,
JVM plugins, Joern binary graph interchange, and query-library coverage remain
separate work. Passing the bounded C checks does not make the project a 1:1
Joern replacement, and this batch does not support a reliable completion date.
