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
| 1. Language frontends | C, C++, Go, Java, JavaScript, TypeScript/TSX, Python, Ruby, Rust, and Scala each have a pinned Joern differential corpus, at least two pinned real projects, deterministic save/load/query/scan output, and no unexplained graph or security-outcome differences. | `acceptance/languages/<language>/manifest.json`; per-language CI jobs and diff reports | **Not met.** C has the production oracle and two real projects; the other nine only have shared experimental conformance. |
| 2. CPGQL | Every standard node-type, property/filter, AST, call-graph, CFG, data-flow, repeat, complex, and execution step in the committed catalog has differential positive, empty, duplicate, regex, and error cases against v4.0.555. Existing compatible queries run unchanged through `cpg query`. | `acceptance/cpgql/catalog.json`; native and Joern result normalizers; zero-diff CI | **In progress.** A native compiler/CLI and the first catalog tier exist; data-flow expressions, repeat, boolean filters, and several complex steps remain. |
| 3. Binary interoperability | The Rust CLI imports current Joern v4 flatgraph `cpg.bin` and exports a graph Joern v4 loads. Both directions preserve the cataloged nodes, properties, edges, overlays, and queries across C plus one JVM and one dynamic-language fixture. | `acceptance/flatgraph/check.sh` using the pinned Joern distribution; round-trip digests and query diffs | **In progress.** Native import/export now passes a real v4.0.555 C fixture in both directions. Unsupported Joern node/property kinds, overlays, and the JVM/dynamic fixtures still block the full row. |
| 4. C semantic precision | Preprocessing covers conditional expressions, nested/function/variadic macros, stringizing/token pasting, forced includes and include paths. Compiler-informed types, function pointers, aliases, field/array sensitivity, heap objects, and points-to facts match the pinned oracle across fixtures and zlib/Lua. | Expanded `joern-parity` corpus and `acceptance/real-projects`; zero unexplained node/edge/flow diffs | **Partially met.** The 101 graph blocks and 1,481 ReachingDef facts are exact, now including `#if/#elif`, `defined`, undefined identifiers, and function-macro conditions. Variadic/stringize/paste/include and compiler-informed points-to cases remain. |
| 5. Security rules | The native default packs cover the committed querydb/CWE catalog. Every rule has a positive, near-miss negative, sanitizer/fix negative, and multi-file case. Aggregate and per-rule precision/recall meet the committed thresholds on pinned labeled corpora. | `acceptance/rules/catalog.json`, fixtures, corpus manifests, SARIF diffs, precision/recall report | **Not met.** Existing packs are useful but coverage and labeled quality gates are incomplete. |

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
