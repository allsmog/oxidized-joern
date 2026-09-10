# Compatibility and release contract

Oxidized Joern `0.1.x` supports the bounded native workflows below.
Full Joern compatibility remains incomplete.

For current measured gaps, pending work and engineering estimates, see
[REMAINING_PORT_WORK.md](REMAINING_PORT_WORK.md). The batch descriptions below
are historical checkpoints; later repairs supersede earlier gap statements.

Status meanings:

- **Production preview**: release-blocking deterministic and semantic gates
  exist for the named workflow.
- **Experimental**: the workflow has executable smoke/conformance coverage,
  but no language-specific oracle and real-project quality baseline yet.
- **Unsupported**: no compatibility promise is made.

## Language and workflow matrix

| Language | Build and query | Save/load | Flow and scan | Incremental update | Evidence | Status |
|---|---|---|---|---|---|---|
| C | Yes | Yes | Yes, including SARIF | Yes; correctness-first full-project rebuild | 334/334 Joern v4.0.555 graph blocks; 4,188/4,188 ReachingDef facts; canonical outcome suite; pinned zlib 1.3.1 and Lua 5.4.7 | **Production preview** |
| C++ | Yes | Yes | Yes | Generic frontend | 13/13 live Joern probes; cxxopts and expected deterministic workflows; labeled default rules | **Production preview** |
| Go | Yes | Yes | Yes | File-local incremental path | 13/13 live Joern probes; google/uuid and gjson deterministic workflows; labeled default rules | **Production preview** |
| Java | Yes | Yes | Yes | File-local incremental path | 13/13 live Joern probes; Gson and jsoup deterministic workflows; labeled default rules | **Production preview** |
| JavaScript | Yes | Yes | Yes | Generic frontend | 13/13 live Joern probes; minimist and escape-string-regexp deterministic workflows; labeled default rules | **Production preview** |
| TypeScript/TSX | Yes | Yes | Yes | Generic frontend | 13/13 live Joern probes; ts-pattern and Zod deterministic workflows; labeled default rules | **Production preview** |
| Python | Yes | Yes | Yes | Generic frontend | 13/13 live Joern probes; MarkupSafe and ItsDangerous deterministic workflows; labeled default rules | **Production preview** |
| Ruby | Yes | Yes | Yes | Generic frontend | 13/13 live Joern probes; ruby/json and Rack deterministic workflows; labeled default rules | **Production preview** |
| Rust | Yes | Yes | Yes | Generic frontend | 13/13 live Joern probes; itoa and anyhow deterministic workflows; labeled default rules | **Production preview** |
| Scala | Yes | Yes | Yes | Generic frontend | Shared schema, summary/taint, and persistence acceptance | Experimental |

Scala in this table means that the native Rust executable can parse and
analyse Scala source. The implementation and release contain no Scala runtime,
JVM, Maven, or Scala console.

The eight promoted non-C frontends additionally run a 16-project gate that
repeats build, CPG2 save/load, complete JSON export, CPGQL query, built-in scan,
and SARIF output and compares immutable hashes. Their 30 default rules have
120 positive, near-miss, fixed, and multi-file expectations at 100% committed
precision and recall.

## C production-preview boundary

The release contract covers these C operations:

- project build through the same `CFrontend` and standard analysis pipeline
  used by the parity gate;
- deterministic CPG2 save/load, JSON export, edge output, built-in scan, and
  SARIF output;
- canonical flow facts and final findings for branches, kills, loops, returns,
  globals, pointer/member access, sanitizers, cross-calls, and recursion;
- a labeled 68-expectation, 17-rule default corpus covering command injection,
  unbounded and attacker-sized copies, uncontrolled format strings, `gets`,
  SQL injection, path traversal, dynamic-library loading, attacker-sized
  allocation, network destinations, unchecked critical returns, weak random
  and cryptographic primitives, and legacy string APIs, including near-miss,
  fixed, and cross-file cases at 100% committed precision and recall;
- distinct call identities for same-named translation-unit-local functions;
- Joern duplicate-function naming and call links for the pinned ordinary and
  literal-static controls;
- content-correct project updates whose result equals a clean rebuild;
- pinned zlib and Lua builds under recorded wall-time and peak-RSS ceilings.

The committed C oracle is exact for its corpus, not a claim that every valid C
program has already been compared with Joern. Conditional and advanced macro
expansion plus nested include paths, forced includes, and command-line defines
have pinned coverage. Compiler-informed type resolution, aliasing, and
points-to behavior are pinned for the committed corpus; a newly supported
construct must add another fixture and return the exact differential to zero.

The [first measured sprint](../docs/conformance/astra-sprint-2026-09-08.md)
records the initial syntax and return-flow repairs. The
[second batch](../docs/conformance/astra-second-batch-2026-09-08.md) extends
direct findings, conditions and loops, callable declarations and lexical
scope, and source-ordered preprocessing. Its whole-project comparisons retain
substantial differences on zlib and Lua; exact small fixtures do not establish
whole-language parity.

The [third batch](../docs/conformance/astra-third-batch-2026-09-08.md) adds
primitive type roles and registration, numeric literal types, static
modifiers, standalone blocks, distinct reaching-definition identities, and
concatenated strings. Separate complete fixtures and scanner outcomes cover
these changes; retained nonexact diagnostics mark their boundaries.

The [fourth batch](../docs/conformance/astra-fourth-batch-2026-09-08.md) extends
supported array initializer forms, supplied quoted-header macro context,
logical directive recovery, expression and statement macro expansion, source
locations, and reaching-definition scheduling. Separate complete fixtures and
scanner outcomes establish those cases. Primary function-body matches are
reported separately from operator and macro stubs. JSON export preserves the
tested output bytes with lower memory use; C format-string rules now check
the intended argument positions. Whole-project graphs remain nonexact.

The [fifth batch](../docs/conformance/astra-fifth-batch-2026-09-08.md) adds
field-token macros, nested casts, typedef aggregate bodies and initializer
methods, malformed macro recovery, pending-reference order, sizeof specifiers,
and preprocessing-token argument binding. Seven additional fixture families
retain 124 complete exact isolated projections and full nonexact diagnostics.
Whole-project source-method and body matches improve; both full projects still
differ from Joern. The final direct-argument repair restores the identified Lua
operand/flow losses at their original source occurrences. Typedef casts can
still parse as pointer calls, and the source parser can truncate or omit
unsupported macro invocations before argument binding runs.

The [sixth batch](../docs/conformance/astra-sixth-batch-2026-09-08.md) extends
source-position typedef casts, declaration macro expansion, supplied-header
return bindings, macro sizeof rendering, pinned default C macros, numeric macro
root types, and inactive declaration context. Inactive declarations retain
visible expression macros while their directives and callable bindings remain
inactive. Nine fixture families plus a combined regression test gate 180 exact
isolated projections; all complete nonexact references remain available.
Whole-project strict primary method matches improve to 32 on zlib and 510 on
Lua. These counts describe the selected AST projection, not language coverage
or complete graph equality. Full-name parsing at spaces remained open at this
checkpoint; six numeric macro diagnostics and eight additional complete controls
pinned the next repair. No non-C frontend or Joern
binary/console compatibility is promoted by this batch.

The [seventh batch](../docs/conformance/astra-seventh-batch-2026-09-08.md)
preserves spaces in parsed METHOD FULL_NAME values and prevents truncated names
from attaching CFG and ReachingDef edges to an unrelated method. Of 31 complete
fixture projects, 17 are exact and fully gated; all 14 nonexact diagnostics
remain available. The whole-project change restores four CFG edges without
changing AST, node or flow records. Property-like text in CODE and filenames
still exposes separate transport limitations, including five retained importer
abort cases. Neither complete real-project projection is exact.

The [eighth batch](../docs/conformance/astra-eighth-batch-2026-09-08.md)
extends selected function-body preprocessor branches across statement emission,
phantom discovery, declaration shadows and typedef context. It also recognizes
spaced `undef` directives and retains inactive function-header snapshots.
Thirty of 34 complete new fixture projects are exact and fully gated; four
complete diagnostics remain. All three real zlib `snprintf` calls and the
proper stub are restored. Whole-project exact method ASTs improve to 160 on
zlib and 1,434 on Lua, with no formerly exact method or primary body lost.
A separate complete diagnostic pins runtime `else`/`if` chains crossing an
`#endif`: the newly emitted first branch remains incompletely connected.
Its ordinary exact control was replayed, not added as a production gate.
Supplied body includes remain incompletely lowered; the `fixedtables` array
and identifier type/CODE gaps predate this batch. Both full project graphs
still differ from Joern.

The [ninth batch](../docs/conformance/astra-ninth-batch-2026-09-08.md) shares
source-order body macro context across emission and discovery consumers,
while preserving lexical typedef scope and explicit recovery contexts.
Synthetic macro metadata uses the pinned expansion-event ownership in the
covered cases, including literal opacity and object-callee token eligibility.
Of 76 complete new projects, 54 are exact and fully gated; all 22 diagnostics
remain, including one importer failure. Two additional complete macro-METHOD
assertions do not make their enclosing projects exact. Whole zlib output
repairs 12 function-reference types and their corresponding type edges;
Lua output is unchanged, with no formerly matching record lost in either
project. Neither complete project projection is exact. When the modeled
metadata queue has no eligible event, a compatibility fallback remains;
this is not a complete port of upstream macro handling.


The [tenth batch](../docs/conformance/astra-tenth-batch-2026-09-08.md) lowers
selected supplied body-header declarations through separate include-occurrence
views, preserving caller scope and measured header line origins. Synthetic
macro metadata follows the pinned source/header pass precedence. Fourteen of
18 complete new projects are exact and fully gated; four diagnostics remain.
Bare macro array dimensions expand while compound dimensions retain source
spelling. A pass-through dataflow allocation repair preserves the measured
relation and brings repeated builds under the unchanged resource ceilings.
Whole zlib/Lua matching records gain 8,799/260 with no matching losses; exact
method ASTs are 160/410 and 1,438/2,274. Both full projections still differ
from Joern. The complete fixedtables initializer topology is now present,
while identity, long CODE, flow-label and linking gaps remain. Repeated body
includes and quoted-header lookup retain separate diagnostics. Final workspace,
live-oracle, native and official real-project gates pass; no non-C, schema,
binary-format, console or query compatibility is promoted by this batch.

The [eleventh batch](../docs/conformance/astra-eleventh-batch-2026-09-09.md)
uses the existing declaration renderer for primitive MEMBER bases. Sixteen
new projects and two retained tiny-fixedtables anchors now match complete
live graphs; the nonprimitive control retains 20 missing type-registration
records. All prior diagnostics and the importer abort remain. Whole zlib/Lua
matching records gain 21/11 with no losses; method exactness and complete flow
sections are unchanged. Both full projects remain nonexact. Duplicate function
identity/linking, long fixedtables CODE and the missing inflate body remain
separate gaps. Final workspace, live-oracle, native and official resource gates
pass; schema, binary-format, console, query and non-C scope is unchanged.

The [twelfth batch](../docs/conformance/astra-twelfth-batch-2026-09-09.md)
pins duplicate function names, literal STATIC call fixups, physical reference
locations, method bindings and modifier properties. Eight new complete
canonical graphs match Joern. Eleven full snapshots of the Rust graph survive
save/reopen unchanged, and 81 selected stored metadata occurrences match the
separately observed Joern graphs. The canonical oracle and these stored-property
checks cover different scopes: complete cross-producer snapshots still differ.
At the twelfth checkpoint, IMPORT/DEPENDENCY nodes in include controls,
unstored coordinates, property presence, and edge properties and relationships
remained open. The thirteenth checkpoint repaired the observed include pairs;
broader property and relationship parity remains incomplete.

The native writer emits CPG2 version 4. Readers retain the native v1 schema
(including sparse properties), parity v2/v3 tags and columns, and legacy CPG1.
Version 4 combines modifier and include metadata, explicit ORDER presence,
expanded schema tags and sparse external properties without reinterpreting old
tags. Fixed old-writer fixtures and mixed-property upgrades exercise this boundary.
The reserved optional-line sentinel cannot represent
`Some(u32::MAX)`; saving that value returns an error before replacing a file.
CPG2 is this project's storage format; the separate Flatgraph import/export
commands provide the bounded Joern binary interoperability described below.

Explicit compiler inputs now select active top-level declarations; default
parsing retains CDT's inactive declaration headers with empty bodies. Include
paths, forced includes, per-file compilation database definitions, function-like
conditions, variadics, stringification and token pasting have focused controls.
Remaining C gaps include broader combinations of build context, repeated body
includes, runtime control chains split by directives, unbraced multi-statement replacements,
unpinned initializer and field-designator forms, tagged and alias type
resolution, nested macro expansion and recovery-context behavior, and further
reaching-definition boundaries. Unused local alias registration and GNU
nested-function lowering retain complete nonexact diagnostics. Complete whole-project differences and
retained nonexact diagnostics record these limits. Expansion work/depth bounds
do not guarantee complete parsing; unsupported syntax can yield partial graphs.
Some CRLF source rendering differs, and some transformed nodes use approximate
enclosing locations. The selected graph projection omits column numbers and
some control metadata. Its newline-escaped text transport is not injective or
lossless. The measured reports distinguish repaired cases from remaining gaps.

## Deliberate incompatibilities

The following are unsupported in `0.1.x`:

- Joern's Scala console and full CPGQL source compatibility; `cpg query`
  implements the native subset cataloged in `acceptance/cpgql`: 108 source
  expressions and 37 populated sparse-schema/property/edge expressions pass
  at zero diff against Joern v4.0.555, while 18 malformed or unsafe forms are
  classified live and rejected fail-closed. `.p` and `.browse` provide
  annotated terminal output; JSON remains the machine interface;
- JVM/Scala plugins and Maven-based extension workflows;
- loading internal CPG2 files directly in Joern. The explicit
  `cpg export-joern` and `cpg import-joern` conversions use Joern v4's current
  Flatgraph format; C, Java, and Python pass bidirectional load probes and
  content-exact persisted round-trip digests;
- whole-graph byte-identity claims for non-C frontends; their production
  preview boundary is the normalized live differential, pinned real-project
  workflows, and labeled security outcomes listed above;
- every rule from Joern querydb.

Use the native CLI, JSON/SARIF outputs, stdio query server, or MCP interface as
the supported integration surfaces.

## Release-blocking gates

Every release must pass the locked Rust workspace tests, formatting, Clippy,
dependency audit, 334/334 committed C parity, canonical C scanner outcomes,
the 188-label default-rule quality gates, the all-language acceptance test,
pinned zlib/Lua and 16-project non-C acceptance, and packaged binary and
container tests. Release CI also reruns the live Joern C, cross-language,
CPGQL, compiler-input, and Flatgraph differentials. The real-project suites
also run on ordinary CI changes.

All five bounded replacement tracks are green in `REPLACEMENT_CONTRACT.md`.
The deliberate incompatibilities above remain outside that release claim.
