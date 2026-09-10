# Array dimension macro spelling

All four complete Joern 4.0.555 projects now pass with the frozen V4 candidate and the [production test](../../array_dimension_macros.rs). Each project pairs a macro dimension with a numeric-spelling control. The observed TYPE_FULL_NAME behavior is:

| Source dimension | Joern type suffix | Accepted ninth | Held tenth V2 | Repaired V4 |
| --- | --- | --- | --- | --- |
| N, where N is 3 | [3] | Nonexact | Exact | Exact |
| N+1 | [N+1] | Exact | Nonexact | Exact |
| (N+1) | [(N+1)] | Exact | Nonexact | Exact |
| +N | [+N] | Exact | Nonexact | Exact |

Every paired numeric-control method stays completely exact. V4 expands a bare identifier dimension and retains the original expression for the other shapes. This closes the three complete-project regressions and the 96 previously correct whole-zlib records lost by V2. It does not establish general constant folding or all unmeasured dimensions.

The independent PTF allocation repair is preserved separately under provenance/performance-v3, including its inert source patch, before/after release bindings, exhaustive membership check, Rust tests, original308-block gate, fourteen byte-identical full graph replays and one diagnostic zlib pilot. That pilot kept graph/edge bytes identical while using 488.65625 MiB and 3.629123 seconds. It is not repeated resource acceptance. V4's dimension-only patch, source/binaries and current checks are under provenance/candidate-v4; the unchanged parent V2 evidence stays in the original body-includes package. The original V4 build receipt is preserved alongside an explicit prose clarification; its source and binary bindings are unchanged.

All four new full graph outputs, stderr, exit statuses and complete differences are retained in cases/*/candidate-v4. Accepted ninth and held V2 outputs remain in baseline/ and held-v2/. The exact pre-repair README and measurement are preserved under provenance/stage-v1. No expected graph was filtered, trimmed, deduplicated, or edited.

The original five-project live batch remains whole under oracle/, including the unchanged body_include_macro_only anchor. That anchor already belongs to the existing body-includes family; it is not a fifth new gate. The four new references contain 2,012 canonical lines including separators and 1,977 nonempty records. With the unchanged14 body-includes projects there are18 unique projects:6 exact on accepted ninth,11 on held V2,14 on repaired V4, with4 complete diagnostics retained. The two original raw ordinal-collision losses remain explicitly classified in the unchanged body-includes package; no zero-raw-loss claim is made for this18-case union. All76 prior body-macro-state outputs remain byte-identical to V2, including the previously disclosed duplicate-clinit abort.

Full controlled whole-zlib/Lua outputs remain bound by filename and hash in the copied run receipts; complete raw whole-project files are retained in the worker evidence directory rather than being small fixture gates. Whole-zlib raw matching records versus accepted ninth improve by8,799 with none lost, and Lua by260 with none lost. Independent source-occurrence, exact-method and priority-fact review remains separate from these counters. Whole projections are still nonexact.

Root's final workspace/live gates and repeated resource acceptance are pending. This package does not mark the tenth increment or the C port complete. Installed Joern/JDK executables and graph binaries are identified by hashes rather than shipped here; inherited runtime receipts state the host-dependency boundary.
