# Compatibility and release contract

Oxidized Joern `0.1.x` is production-ready for the language workflows
explicitly listed below. It is not yet a universal drop-in replacement for
the full CPGQL/Scala-console/plugin surface or every Joern graph property.

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
dependency audit, 122/122 committed C parity, canonical C scanner outcomes,
the 188-label default-rule quality gates, the all-language acceptance test,
pinned zlib/Lua and 16-project non-C acceptance, and packaged binary and
container tests. Release CI also reruns the live Joern C, cross-language,
CPGQL, compiler-input, and Flatgraph differentials. The real-project suites
also run on ordinary CI changes.

All five bounded replacement tracks are green in `REPLACEMENT_CONTRACT.md`.
The deliberate incompatibilities above remain outside that release claim.
