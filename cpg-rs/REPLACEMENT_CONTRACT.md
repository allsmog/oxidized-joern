# Native Joern replacement contract

This is the release boundary for calling the Rust executable a production,
drop-in Joern replacement. It is stricter than the `0.1.x` C production-preview
contract in `COMPATIBILITY.md`. A row is complete only when its named command
runs in release CI and the committed evidence is green; prose or a smoke test
cannot promote a row.

The compatibility target is Joern **v4.0.555**. The implementation remains
native Rust. “CPGQL compatible” means the standard CPG traversal language and
query-library operations; it does not mean evaluating arbitrary Scala, loading
JVM plugins, or restoring a Scala console.

| Track | Production acceptance | Required evidence | Current state |
|---|---|---|---|
| 1. Language frontends | C, C++, Go, Java, JavaScript, TypeScript/TSX, Python, Ruby, Rust, and Scala each have a pinned Joern differential corpus, at least two pinned real projects, deterministic save/load/query/scan output, and no unexplained graph or security-outcome differences. | `acceptance/languages/manifest.json`; committed and live-oracle CI differentials; per-language real-project reports | **In progress.** C++ plus Go, Java, JavaScript, TypeScript, Python, Ruby, and Rust each pass 13/13 normalized semantic probes against live Joern v4.0.555, with committed results enforced on every change. C retains its larger production oracle and two real projects. Scala remains native-only because Joern v4.0.555 ships no Scala source frontend. Two pinned real projects per promoted non-C language, wider graph/security differentials, and a documented Scala substitute oracle remain. |
| 2. CPGQL | Every standard node-type, property/filter, AST, call-graph, CFG, data-flow, repeat, complex, and execution step in the committed catalog has differential positive, empty, duplicate, regex, and error cases against v4.0.555. Existing compatible queries run unchanged through `cpg query`. | `acceptance/cpgql/catalog.json`; native and Joern result normalizers; zero-diff CI | **In progress.** All 108 executable catalog cases are now generated from one manifest and pass a live Joern v4.0.555 differential; the same complete native run is digest-gated in ordinary and release CI. The compiler also rejects 17 malformed or unsafe forms fail-closed. Positive fixtures for every sparse schema/edge step, live oracle classification of remaining error dimensions, and annotated terminal rendering remain. Arbitrary Scala execution is intentionally outside the native CPGQL boundary. |
| 3. Binary interoperability | The Rust CLI imports current Joern v4 flatgraph `cpg.bin` and exports a graph Joern v4 loads. Both directions preserve the cataloged nodes, properties, edges, overlays, and queries across C plus one JVM and one dynamic-language fixture. | `acceptance/flatgraph/check.sh` using the pinned Joern distribution; round-trip digests and query diffs | **Met.** The release gate covers all 40 Joern v4 node labels, the fixture edge/overlay schema, sparse and multivalue node properties, and string edge properties. C, Java, and Python pass native-to-Joern loading and Joern-to-CPG2-to-Joern round trips with identical normalized BLAKE3 content digests and query probes. |
| 4. C semantic precision | Preprocessing covers conditional expressions, nested/function/variadic macros, stringizing/token pasting, forced includes and include paths. Compiler-informed types, function pointers, aliases, field/array sensitivity, heap objects, and points-to facts match the pinned oracle across fixtures and zlib/Lua. | Expanded `joern-parity` corpus and `acceptance/real-projects`; zero unexplained node/edge/flow diffs | **Partially met.** The 114 graph blocks and 1,702 ReachingDef facts are exact, including advanced preprocessing, per-translation-unit `compile_commands.json` inputs, local function-pointer types/method refs, dynamic pointer calls, static functions, heap allocation/aliases, indirect fields, array aliases, and deallocation semantics. The compiler-input differential is zero-diff against Joern v4.0.555. A broader compiler-derived points-to corpus remains. |
| 5. Security rules | The native default packs cover the committed querydb/CWE catalog. Every rule has a positive, near-miss negative, sanitizer/fix negative, and multi-file case. Aggregate and per-rule precision/recall meet the committed thresholds on pinned labeled corpora. | `acceptance/rules/catalog.json`, fixtures, corpus manifests, SARIF diffs, precision/recall report | **In progress.** All ten default C rules now have a release-blocking 40-label quality gate at 100% committed precision/recall, including cross-file flows, argument-position near misses, bounded scanf formats, insecure temporary-file APIs, and unbounded format output. Mapping the wider querydb/CWE catalog and adding equivalent labeled corpora for every promoted language remain. |

## Non-negotiable release gates

The production-replacement label is permitted only when all five rows pass in
one commit and the ordinary release workflow also passes formatting, locked
workspace tests, Clippy with warnings denied, dependency audit, packaged-binary
acceptance, and container acceptance. A failing or skipped oracle download,
unsupported query, unreadable graph, missing corpus, timeout, or result
truncation fails closed.

## Compatibility accounting

Each differential manifest records:

- the exact oracle release and artifact digest;
- source repository revision and source/archive digest;
- normalized node, edge, property, query, finding, timing, and memory results;
- every intentional divergence, with an explicit public compatibility decision;
- the command that regenerates the evidence.

Removing a test or narrowing a corpus is a contract change and must be reviewed
as such. Experimental language support cannot inherit C's production status.
