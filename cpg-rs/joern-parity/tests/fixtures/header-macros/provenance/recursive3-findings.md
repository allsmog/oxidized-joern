# Recursive macro and unused-argument references

All three pinned Joern imports pass; all 463 complete selected records are retained. The previous 18+8 source manifests and run receipts remain unchanged.

- `self_recursive_object`: one `defs.h:VALUE:int(0)` INLINED wrapper, then a BLOCK containing IDENTIFIER VALUE with CODE `<global> VALUE` and TYPE `int`. A matching phantom LOCAL appears. The macro is disabled during its own expansion, so the identifier is preserved rather than recursively wrapping itself.
- `indirect_recursive_object`: one `defs.h:FIRST:ANY(0)` INLINED wrapper, then BLOCK → IDENTIFIER FIRST with CODE `<unknown> FIRST` and TYPE `ANY`. FIRST is intentionally undeclared in this minimal source. No nested FIRST/SECOND macro wrappers appear.
- `second_argument_only`: the only cloned original argument is b, with ORDER=1 and ARGUMENT_INDEX=2. The expansion BLOCK has ORDER=2 and ARGUMENT_INDEX=2 as well. This duplicate argument index is the pinned graph, not a transport mistake. The macro wrapper has TYPE int and signature `defs.h:SECOND:int(2)`.

Every case contains input/, expected.txt, raw oracle.stdout/oracle.stderr, run.json and a unique oracle-workspace/. The shared oracle script is unchanged. verification.json and oracle-runs.json bind all sources, full selected projections, commands, timestamps, section counts and hashes.
