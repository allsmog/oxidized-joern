# Compatibility and release contract

Oxidized Joern `0.1.x` is production-ready for the C workflows explicitly
listed below. It is not yet a drop-in replacement for every Joern frontend,
the full CPGQL surface, the Scala console, plugins, or every Joern graph
property.

Status meanings:

- **Production preview**: release-blocking deterministic and semantic gates
  exist for the named workflow.
- **Experimental**: the workflow has executable smoke/conformance coverage,
  but no language-specific oracle and real-project quality baseline yet.
- **Unsupported**: no compatibility promise is made.

## Language and workflow matrix

| Language | Build and query | Save/load | Flow and scan | Incremental update | Evidence | Status |
|---|---|---|---|---|---|---|
| C | Yes | Yes | Yes, including SARIF | Yes; correctness-first full-project rebuild | 122/122 Joern v4.0.555 graph blocks; 1,961/1,961 ReachingDef facts; canonical outcome suite; pinned zlib 1.3.1 and Lua 5.4.7 | **Production preview** |
| C++ | Yes | Yes | Yes | Generic frontend | 13/13 live Joern v4.0.555 semantic probes plus shared acceptance | Experimental |
| Go | Yes | Yes | Yes | File-local incremental path | 13/13 live Joern v4.0.555 semantic probes plus cross-file update tests | Experimental |
| Java | Yes | Yes | Yes | File-local incremental path | 13/13 live Joern v4.0.555 semantic probes plus cross-file update tests | Experimental |
| JavaScript | Yes | Yes | Yes | Generic frontend | 13/13 live Joern v4.0.555 semantic probes plus shared acceptance | Experimental |
| TypeScript/TSX | Yes | Yes | Yes | Generic frontend | 13/13 live Joern v4.0.555 semantic probes plus dialect tests | Experimental |
| Python | Yes | Yes | Yes | Generic frontend | 13/13 live Joern v4.0.555 semantic probes plus shared acceptance | Experimental |
| Ruby | Yes | Yes | Yes | Generic frontend | 13/13 live Joern v4.0.555 semantic probes plus shared acceptance | Experimental |
| Rust | Yes | Yes | Yes | Generic frontend | 13/13 live Joern v4.0.555 semantic probes plus shared acceptance | Experimental |
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
- a labeled 68-expectation, 17-rule default corpus covering command injection,
  unbounded and attacker-sized copies, uncontrolled format strings, `gets`,
  SQL injection, path traversal, dynamic-library loading, attacker-sized
  allocation, network destinations, unchecked critical returns, weak random
  and cryptographic primitives, and legacy string APIs, including near-miss,
  fixed, and cross-file cases at 100% committed precision and recall;
- distinct call identities for same-named translation-unit-local functions;
- content-correct project updates whose result equals a clean rebuild;
- pinned zlib and Lua builds under recorded wall-time and peak-RSS ceilings.

The committed C oracle is exact for its corpus, not a claim that every valid C
program has already been compared with Joern. Conditional and advanced macro
expansion plus nested include paths, forced includes, and command-line defines
have pinned coverage. Compiler-informed type resolution, aliasing, and
points-to behavior are pinned for the committed corpus; a newly supported
construct must add another fixture and return the exact differential to zero.

## Deliberate incompatibilities

The following are unsupported in `0.1.x`:

- Joern's Scala console and full CPGQL source compatibility; `cpg query`
  currently implements the 108-case native subset cataloged in
  `acceptance/cpgql/catalog.json`, with all 108 expressions at
  zero diff against Joern v4.0.555;
- JVM/Scala plugins and Maven-based extension workflows;
- loading internal CPG2 files directly in Joern. The explicit
  `cpg export-joern` and `cpg import-joern` conversions use Joern v4's current
  Flatgraph format; C, Java, and Python pass bidirectional load probes and
  content-exact persisted round-trip digests;
- production parity claims for non-C frontends; eight frontends have a pinned
  13-probe source-semantic differential, but not the required real-project and
  security-outcome corpora yet;
- every rule from Joern querydb.

Use the native CLI, JSON/SARIF outputs, stdio query server, or MCP interface as
the supported integration surfaces.

## Release-blocking gates

Every release must pass the locked Rust workspace tests, formatting, Clippy,
dependency audit, 122/122 committed C parity, canonical C scanner outcomes,
the labeled default-C-rule quality gate, the all-language acceptance test,
pinned zlib/Lua acceptance, and packaged binary and container tests. The
zlib/Lua suite also runs nightly.

The stricter requirements for promoting the project from C production preview
to a production Joern replacement are tracked in `REPLACEMENT_CONTRACT.md`.
