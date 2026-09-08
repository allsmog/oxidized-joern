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

Confirmed remaining C gaps include header/build-definition context,
function-like macros in preprocessor conditions, array initializers, tagged
and alias type resolution, macro expansion-wrapper types, and loop-tail
reaching-definition scheduling. CRLF source rendering also differs in retained
cases. Condition expansion has explicit work/depth bounds. Source locations
use approximate enclosing locations for some transformed nodes. The measured
reports distinguish tested behavior from these remaining limits.

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
