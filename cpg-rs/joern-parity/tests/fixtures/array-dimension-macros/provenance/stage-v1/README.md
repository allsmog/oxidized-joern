# Array dimension macro spelling

Four complete Joern 4.0.555 references pin a regression introduced by the held tenth V2 candidate. Each project pairs a macro dimension with a numeric-spelling control. The observed TYPE_FULL_NAME behavior is:

| Source dimension | Joern type suffix | Accepted ninth | Held tenth V2 |
| --- | --- | --- | --- |
| N, where N is 3 | [3] | Nonexact | Exact |
| N+1 | [N+1] | Exact | Nonexact |
| (N+1) | [(N+1)] | Exact | Nonexact |
| +N | [+N] | Exact | Nonexact |

Every paired numeric-control method is fully exact in both versions. The held candidate expands macros inside every size expression; that loses previously correct compound-array type spellings in zlib. The measured repair boundary is to expand a bare identifier while preserving the existing spelling/normalization behavior for the other expression shapes. This does not establish general constant folding or behavior for unmeasured dimensions.

The four cases under cases/ are intended for one complete-graph production test after the repaired candidate is verified. That test and the repaired candidate are pending; this package adds no Rust implementation or test source. cases/*/baseline contains the accepted ninth output, and cases/*/held-v2 preserves the regressed candidate with full stdout, stderr, actual status, and LF-only complete differences. No expected graph was filtered, trimmed, deduplicated, or hand-edited.

The original five-project live batch is retained whole under oracle/, including the unchanged body_include_macro_only anchor. The anchor already belongs to ../body-includes/cases/body_include_macro_only and is preserved here only as raw/binding provenance; it is not a fifth case or another gate. Four new references contain 2,012 canonical lines including separators and 1,977 nonempty records. The complete live batch including the anchor has 2,135 lines and 2,096 nonempty records.

An independent comparison of the saved outputs and sorted filename/source-hash identities gives 18 unique projects across the existing 14 body-include cases and these four cases: six are exact on accepted ninth, and eleven on held V2. Existing-family V2 results come from its frozen worker binary; the four new results use the separately bound integrated root V2 release binary. Both source/binary provenance chains are retained. These counts are not final repaired-candidate results.

provenance/ includes the full live launch/binding observations, runtime inventories, parent release, exact source-input snapshots, frozen source and binary identities, baseline and held output receipts, and the independent e48d1acd review. Original receipts retain their historical paths unchanged; measurement.json maps each copied artifact to its portable location. Installed Joern/JDK executables and graph binaries are identified by hashes rather than shipped here. The inherited runtime record explicitly describes its host dependency boundary.

The original controls and existing 14-case package remain unchanged. The first review-script syntax error stopped before any graph producer and is retained with its log; the corrected run completed all ten graph replays. No new producer, source edit, build, resource measurement, manifest change, or acceptance decision occurred while assembling this package.
