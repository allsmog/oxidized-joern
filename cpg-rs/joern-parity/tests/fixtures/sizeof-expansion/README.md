# Macro-expanded sizeof

These 39 isolated C projects retain the complete selected AST, NODES, EDGES,
and FLOWS output from pinned Joern 4.0.555. The guarded candidate matches 23
complete graphs, up from 11 at source revision `aba030923a2eb02a7d542875e4274c1b44271608`.
All 23 are exercised by `tests/sizeof_expansion.rs`; the other 16 remain
diagnostics with full expected, baseline, candidate, and diff files.

The repair adds Joern's spacing to macro-expanded `sizeof` and preserves
parenthesis depth while discarding inner comment trivia. Ordinary source
spelling follows the existing path. Parenthesis normalization requires exactly
one non-comment child, so recovered error siblings keep their source text.
Two targeted assertions preserve the malformed operands that an earlier
candidate truncated. They do not claim that those malformed graphs match Joern.
All seven malformed diagnostics retain the baseline's complete edge and flow
records.

`oracle/` contains the three raw producer runs and scripts. `measurement.json`
binds every case and the separately frozen baseline and guarded candidate.
Only CASE framing and the AST transport prefix are removed from the references;
empty method separators are retained. This existing text projection does not
cover every CPG property, and its property transport is not injective or lossless.

`provenance/final-review.json` records the independent guarded review.
`evidence/first-candidate/` and `provenance/first-review.json` preserve the failed
candidate's content-loss evidence. The two `earlier-*.log` files belong to that
earlier source, not the final guarded candidate; combined integration gates are
recorded separately in the sixth-batch report.

Remaining diagnostics include array, function-pointer, and tagged-type display;
the `sizeof(char)-1` cast ambiguity; complex or malformed operands; and other
preexisting graph differences. No expected graph or comparison gate was relaxed.
