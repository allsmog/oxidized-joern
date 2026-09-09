# Guarded else-if chain: complete read-only reduction

The guarded chain is nonexact in both the seventh baseline and the integrated eighth candidate. The ordinary C control is completely exact in both. The eighth candidate adds the first `if`, but emits it as a sibling of the following `if`, losing the `else` connection across `#endif`.

This reduction finds **no loss of formerly Joern-matching source-occurrence CFG or reaching-definition facts** in `choose`. It gains23 matching facts and still misses five live facts: the CFG edge from `result = x` to the returned `result`, plus two RD exit facts for the first `base` identifier and two RD facts from the first assignment's `result` identifier to the returned value/exit. Those source occurrences were absent in the seventh body. The eighth graph therefore remains partial control/flow support; this is not evidence that the entire Lua `math_log` graph is exact.

The current first assignment instead reaches the second condition, allowing the later assignment to overwrite its result. Joern nests the second condition under the first `else`. The missing connection is at AST construction: `kept_preproc_children` returns individual parser children (`exact.rs:7759`), `emit_stmt` emits each selected child independently (`:3084`), and `emit_if` follows only the parsed node's `alternative` field (`:3357`). It has no reconstruction step joining a trailing runtime `else` across the directive boundary to the next statement. A source-aware joined statement view or preprocessing/reparse mapping would be needed to lower the full construct. No fix is implemented here.

`source-occurrence-review.json` retains every compared node, full raw records, complete ancestor chains, all CFG/RD facts, missing/gained facts and raw-ordinal counter changes. Endpoint keys ignore ORDER and disambiguate repeated leaves with their nearest CALL/RETURN owner; all used keys are unique. The equality fixtures and raw references themselves are unchanged and unfiltered. This source-occurrence result is distinct from raw ordinal-line equality.

Raw Joern4.0.555 output, input hashes, exit0 producer receipt, two complete expected graphs, and old7/current8 complete outputs/diffs are preserved. The current release binary/source binding is copied from root's frozen eighth build; old7 uses the frozen seventh release. The independent ordinary control proves the basic non-preprocessor chain is already supported.

All Java and Rust producers are idle. Eighth source and the separate ninth macro-state diagnosis remain untouched.
