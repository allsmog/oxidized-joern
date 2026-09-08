# Compatibility and release contract

Oxidized Joern `0.1.x` has a bounded production-preview contract for the C
workflows listed below. It is not a drop-in replacement for every Joern frontend,
CPGQL, the Scala console, plugins, or Joern's binary graph format.

Status meanings:

- **Production preview**: release-blocking deterministic and semantic gates
  exist for the named workflow.
- **Experimental**: the workflow has executable smoke/conformance coverage,
  but no language-specific oracle and real-project quality baseline yet.
- **Unsupported**: no compatibility promise is made.

## Language and workflow matrix

| Language | Build and query | Save/load | Flow and scan | Incremental update | Evidence | Status |
|---|---|---|---|---|---|---|
| C | Yes | Yes | Yes, including SARIF | Yes; correctness-first full-project rebuild | 308/308 Joern v4.0.555 graph blocks; 3,685/3,685 ReachingDef facts; canonical outcome suite; pinned zlib 1.3.1 and Lua 5.4.7 | **Production preview** |
| C++ | Yes | Yes | Yes | Generic frontend | Shared schema, summary/taint, and persistence acceptance | Experimental |
| Go | Yes | Yes | Yes | File-local incremental path | Shared acceptance plus cross-file edit/invalidation tests | Experimental |
| Java | Yes | Yes | Yes | File-local incremental path | Shared acceptance plus cross-file edit/invalidation tests | Experimental |
| JavaScript | Yes | Yes | Yes | Generic frontend | Shared schema, summary/taint, and persistence acceptance | Experimental |
| TypeScript/TSX | Yes | Yes | Yes | Generic frontend | Shared schema, summary/taint, persistence, and dialect tests | Experimental |
| Python | Yes | Yes | Yes | Generic frontend | Shared schema, summary/taint, and persistence acceptance | Experimental |
| Ruby | Yes | Yes | Yes | Generic frontend | Shared schema, summary/taint, and persistence acceptance | Experimental |
| Rust | Yes | Yes | Yes | Generic frontend | Shared schema, summary/taint, and persistence acceptance | Experimental |
| Scala | Yes | Yes | Yes | Generic frontend | Shared schema, summary/taint, and persistence acceptance | Experimental |

Scala in this table means that the native Rust executable can parse and
analyse Scala source. The implementation and release contain no Scala runtime,
JVM, Maven, or Scala console.

## C production-preview boundary

The release contract covers these C operations:

- project build through the same `CFrontend` and standard analysis pipeline
  used by the parity gate;
- deterministic CPG2 save/load, JSON export, edge output, built-in scan, and
  SARIF output;
- canonical flow facts and final findings for branches, kills, loops, returns,
  globals, pointer/member access, sanitizers, cross-calls, and recursion;
- distinct call identities for same-named translation-unit-local functions;
- content-correct project updates whose result equals a clean rebuild;
- pinned zlib and Lua builds under recorded wall-time and peak-RSS ceilings.

The committed C oracle is exact for its corpus, not a claim that every valid C
program has already been compared with Joern. Complex preprocessor behavior,
include/type resolution, aliasing, and points-to precision remain areas where a
new construct can require another fixture and implementation slice.

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


Confirmed remaining C gaps include broader include/build-definition context,
function-like macros in preprocessor conditions,
body includes and runtime control chains split by preprocessor directives,
variadics, stringification, token pasting, unbraced multi-statement replacements,
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

- Joern's Scala console and CPGQL source compatibility;
- JVM/Scala plugins and Maven-based extension workflows;
- loading this project's CPG2 files in Joern, or loading Joern `cpg.bin` files;
- a parity claim for non-C frontends;
- every rule from Joern querydb.

Use the native CLI, JSON/SARIF outputs, stdio query server, or MCP interface as
the supported integration surfaces.

## Release-blocking gates

Every release must pass the locked Rust workspace tests, formatting, Clippy,
dependency audit, 308/308 committed C parity, canonical C scanner outcomes, the
all-language acceptance test, pinned zlib/Lua acceptance, and packaged binary
and container tests. The zlib/Lua suite also runs nightly.
