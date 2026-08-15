# Native Joern replacement contract

This is the release boundary for calling the Rust executable a production-ready
Joern replacement for the workflows in `COMPATIBILITY.md`. It does not permit a
universal drop-in claim. A row is complete only when its named command runs in
release CI and the committed evidence is green; prose or a smoke test cannot
promote a row.

The compatibility target is Joern **v4.0.555**. The implementation remains
native Rust. “CPGQL compatible” means the standard CPG traversal language and
query-library operations; it does not mean evaluating arbitrary Scala, loading
JVM plugins, or restoring a Scala console.

| Track | Production acceptance | Required evidence | Current state |
|---|---|---|---|
| 1. Language frontends | C plus every Joern-v4-oracle-backed native frontend has a pinned semantic differential, at least two pinned real projects, deterministic save/load/query/scan output, and labeled security outcomes. A frontend without an upstream Joern source oracle cannot inherit this status. | `acceptance/languages/manifest.json`; `acceptance/language-projects/manifest.json`; committed/live differentials; labeled rule corpora | **Met.** C++ plus Go, Java, JavaScript, TypeScript, Python, Ruby, and Rust each pass 13/13 normalized probes against live Joern v4.0.555. Sixteen immutable real-project snapshots repeat build, CPG2 save/load, full JSON export, native CPGQL query, built-in scan, and SARIF with exact hashes. Their 30 rules pass 120 positive/near-miss/fixed/multi-file labels. C retains its larger exact oracle and zlib/Lua gate. Scala remains explicitly Experimental/native-only because Joern v4.0.555 ships no Scala source frontend; no JVM or Scala implementation code is restored. |
| 2. CPGQL | Every standard node-type, property/filter, AST, call-graph, CFG, data-flow, repeat, complex, and execution step in the committed catalog has differential positive, empty, duplicate, regex, and error cases against v4.0.555. Existing compatible queries run unchanged through `cpg query`. | `acceptance/cpgql/catalog.json`; `acceptance/cpgql/positive.json`; native and Joern result normalizers; zero-diff CI | **Met.** The release gate passes 108/108 source-result cases and 37/37 populated sparse-schema/property/edge cases against live Joern v4.0.555. All 18 malformed or unsafe forms fail closed and carry live classifications: 13 are also rejected by Joern, four Joern-accepted ambiguous or unbounded forms are deliberately stricter natively, and empty input has no oracle expression. `.p` and `.browse` render deterministic annotated terminal rows; `--format json` remains the stable machine output. Arbitrary Scala execution remains outside the native CPGQL boundary. |
| 3. Binary interoperability | The Rust CLI imports current Joern v4 flatgraph `cpg.bin` and exports a graph Joern v4 loads. Both directions preserve the cataloged nodes, properties, edges, overlays, and queries across C plus one JVM and one dynamic-language fixture. | `acceptance/flatgraph/check.sh` using the pinned Joern distribution; round-trip digests and query diffs | **Met.** The release gate covers all 40 Joern v4 node labels, the fixture edge/overlay schema, sparse and multivalue node properties, and string edge properties. C, Java, and Python pass native-to-Joern loading and Joern-to-CPG2-to-Joern round trips with identical normalized BLAKE3 content digests and query probes. |
| 4. C semantic precision | Preprocessing covers conditional expressions, nested/function/variadic macros, stringizing/token pasting, forced includes and include paths. Compiler-informed types, function pointers, aliases, field/array sensitivity, heap objects, and points-to facts match the pinned oracle across fixtures and zlib/Lua. | Expanded `joern-parity` corpus and `acceptance/real-projects`; zero unexplained node/edge/flow diffs | **Met.** The 122 graph blocks and 1,961 ReachingDef facts are exact, including advanced preprocessing, per-translation-unit `compile_commands.json` inputs, local and aliased function pointers/method refs, dynamic pointer calls, static functions, heap allocation/aliases, pointer-to-pointer writes, returned aliases, pointer fields, rebind/kill behavior, out-parameter calls, indirect fields, array aliases, and deallocation semantics. The compiler-input and pinned zlib/Lua differentials are zero-diff against Joern v4.0.555. |
| 5. Security rules | Every rule in the committed native CWE catalog has a positive, near-miss negative, sanitizer/fix negative, and multi-file case. Aggregate and per-rule precision/recall meet the committed thresholds on pinned labeled corpora. | `acceptance/rules/catalog.json`, `acceptance/rules/languages.json`, fixtures, SARIF and real-project hashes | **Met.** The 17-rule C catalog passes 68 labels and the 30-rule promoted-language catalog passes 120 labels, all at 100% committed precision/recall. The 18 real-project gate also pins scanner/SARIF outcomes. Specialized legacy Joern QueryDB heuristics outside the committed native catalog remain an explicit incompatibility, not an untested claim. |

## Non-negotiable release gates

The documented production-replacement label is permitted only when all five rows pass in
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
