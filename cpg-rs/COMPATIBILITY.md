# Compatibility and release contract

Oxidized Joern `0.1.x` is production-ready for the C workflows explicitly
listed below. It is not a drop-in replacement for every Joern frontend,
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
| C | Yes | Yes | Yes, including SARIF | Yes; correctness-first full-project rebuild | 103/103 Joern v4.0.555 graph blocks; 1,552/1,552 ReachingDef facts; canonical outcome suite; pinned zlib 1.3.1 and Lua 5.4.7 | **Production preview** |
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

The [2026-09-08 measured sprint](../docs/conformance/astra-sprint-2026-09-08.md)
records the live-oracle provenance and the specific syntax and return-flow
gaps closed by the current corpus expansion.

Confirmed remaining C gaps include missed direct source-to-sink findings
after an optional safe overwrite, scalar-condition normalization, and
external function/prototype classification. The return-summary fix does not
yet replace the statement-based generator of direct findings. Source
locations also use approximate enclosing locations for some transformed
nodes. These limits are detailed with follow-up acceptance criteria in that
report.

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
dependency audit, 103/103 committed C parity, canonical C scanner outcomes, the
all-language acceptance test, pinned zlib/Lua acceptance, and packaged binary
and container tests. The zlib/Lua suite also runs nightly.
