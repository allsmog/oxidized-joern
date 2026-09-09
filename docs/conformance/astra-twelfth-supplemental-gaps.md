The eleven-case function-identity increment still leaves full-graph gaps. The retained Joern snapshots contain **991 nodes and 4,232 edge occurrences**; Rust contains **985 nodes and 3,392 edge occurrences**. The sample comprises eight new identity projects and three retained controls. All eleven canonical projections match; the accepted baseline matched four. The 81 selected Binding, Modifier and MethodRef records now match their observed identities, endpoints and physical reference lines, and every complete Rust snapshot survives save/reopen unchanged. These are bounded results from the [complete capture review](astra-twelfth-evidence/capture-review.json), with [aggregate counts and original snapshot hashes](astra-twelfth-evidence/supplemental-gap-evidence.json).

The executable bytes are unchanged after the final test-only lint correction. The earlier 55-command capture is explicitly reused through [the final continuity review](astra-twelfth-evidence/lint-continuity-review.json). No new execution or whole-project acceptance is claimed here.

Three `IMPORT` nodes and three `DEPENDENCY` nodes are absent from Rust. Two pairs represent separate `#include "api.h"` occurrences in `a.c` and `b.c` of `duplicate_supplied_macro_local`. The third represents the body include `#include "fixed.h"` at line 8 in `tiny_fixedtables_include`. In the observed graph, each import belongs to its caller’s global `NAMESPACE_BLOCK`, including the body include. Each has one `IMPORTS` edge to its own dependency. The two identical `api.h` dependency property sets remain distinct nodes. Complete properties, all incident edges and their endpoints are retained in [the import evidence](astra-twelfth-evidence/supplemental-gap-evidence.json).

The 840-edge aggregate difference decomposes as follows:

| Edge label | Joern | Rust | Difference |
|---|---:|---:|---:|
| AST | 797 | 614 | 183 |
| DOMINATE | 320 | 0 | 320 |
| POST_DOMINATE | 320 | 0 | 320 |
| REACHING_DEF | 670 | 660 | 10 |
| ALIAS_OF | 4 | 0 | 4 |
| IMPORTS | 3 | 0 | 3 |

These are [label counts preserving multiplicity](astra-twelfth-evidence/supplemental-gap-evidence.json), not a complete endpoint-matching proof. The AST count differences occur between file, namespace, type-declaration, method and import nodes. Equal counts for other labels do not establish equal endpoints or edge properties. The full per-case typed differences remain in the capture review directory.

Property coverage also remains incomplete. Joern records column and end locations, explicit `FILENAME`, `IS_EXTERNAL`, `EVALUATION_STRATEGY`, parameter `INDEX`, call `DISPATCH_TYPE`, alias fields and metadata fields that the Rust snapshot does not expose as properties. Rust retains line numbers, but has no column/end/offset storage. Its file partition and dense order/argument-index fields do not preserve the corresponding Joern property’s presence or absence. Edge payloads remain unstored. [The field inventory](astra-twelfth-evidence/supplemental-gap-evidence.json) preserves observed counts and the serializer’s coverage limits; it does not infer missing values.

The next small increment should represent the three observed include occurrences: `IMPORT`, `DEPENDENCY`, their `IMPORTS` links and global-namespace AST ownership, with the actual property values and missingness. Preserve occurrence multiplicity instead of deduplicating by header name. The pinned [AstCreatorHelper.scala](../../cpg-rs/joern-parity/tests/fixtures/body-macro-state/provenance/upstream/AstCreatorHelper.scala#L179) creates one pair per direct caller include; this collection is separate from selecting included declarations.

Use the two observed include projects and the retained inline control as initial acceptance cases, retaining their complete canonical and supplementary snapshots. Before extending that rule to inactive, repeated or unresolved includes, observe those policies with complete supplementary references; this eleven-case sample does not establish them. Require exact import properties, endpoint ownership and save/reopen retention, and keep existing canonical and identity results unchanged. Dominance, the remaining AST/RD relationships and broader property storage remain separate measured work. This is a proposed next increment, not implemented work or a completed Joern port.

Large candidate diffs remain local at the paths recorded by the copied review. Complete Joern reference snapshots are included under the function-identity fixtures; copying the review does not copy every large local artifact.
