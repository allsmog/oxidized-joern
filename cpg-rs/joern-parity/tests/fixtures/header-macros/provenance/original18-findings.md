# Supplied-header macro reference probes

All 18 isolated imports passed using unchanged shared oracle.sc and Joern v4.0.555/JDK21. 3992 complete selected records are retained across 37 source files. Each case has input/, expected.txt, raw oracle.stdout/oracle.stderr, run.json, and its own oracle-workspace/. No Rust candidate was executed or source changed.

The only reference extraction removes the AST| prefix and unrelated process logging; every selected AST/NODES/EDGES/FLOWS record, including empty AST separators, remains. The shared projection selects properties/edge kinds and retains its existing noninjective newline transport.

Observed details (each reference points to the full graph, not a filtered expectation):

- Ignored function arguments are absent from the macro AST when unused. DROP(value()) has only a BLOCK containing cast (void)0; value() is not emitted as an evaluated argument. `cases/ignored_argument/expected.txt:28`
- Outer call CODE preserves caller spacing, while expanded expression CODE normalizes it: CAST(value+1) and CAST(value + 1) both expand to (int)(value + 1). Compound caller arguments are not duplicated as wrapper arguments; these wrappers contain only BLOCK. `cases/cast_unspaced/expected.txt:37`
- Simple identifier arguments are retained as wrapper arguments when used: ADD(value) has value at argument1 and an expansion BLOCK at argument2. Caller object macro OFFSET is substituted as literal7. `cases/caller_object_define/expected.txt:29`
- Header origin belongs in METHOD_FULL_NAME: defs.h:CAST:ANY(1), sub/defs.h:CONVERT:ANY(1), and sub/outer.h:APPLY:ANY(1). Relative header paths are preserved. `cases/relative_subdirectory/expected.txt:19`
- A header object macro has a typed zero-argument wrapper: VALUE uses defs.h:VALUE:int(0), with BLOCK argument1 and literal17. `cases/header_object/expected.txt:19`
- Macro visibility is source-position specific. Before a local definition, PICK is an ordinary STATIC_DISPATCH call; after it, PICK is INLINED and originates in main.c. Undefinition reverses that transition. `cases/definition_after_function/expected.txt:24`
- Redefinition changes both expanded value and definition origin: before() expands header +1 with defs.h:PICK:ANY(1); after() expands local +2 with main.c:PICK:ANY(1). `cases/redefinition/expected.txt:13`
- The same guarded header selects different bodies per translation unit: first() adds1 and second() adds2 based on their own MODE definitions. Both wrappers keep the same defs.h:SELECT:ANY(1) method identity. `cases/two_translation_units/expected.txt:14`
- Nested macro expansion is flattened inside the outer macro wrapper. APPLY(value) expands through INNER to multiplication; no extra INLINED INNER wrapper is retained. `cases/nested_relative_headers/expected.txt:19`
- A declared nested call wrapper still has outer macro TYPE_FULL_NAME=ANY; its expanded outer/inner CALLs resolve to int. `cases/nested_call_wrapper/expected.txt:27`
- A missing header does not fail import. ABSENT(value) remains a normal unresolved STATIC_DISPATCH call with TYPE_FULL_NAME=ANY. `cases/missing_include/expected.txt:17`

`verification.json` binds the unchanged input manifest and run inventory. `oracle-runs.json` records exact commands, independent working directories, exit codes, times, section counts and hashes. Replay commands are the run.json command arrays; use a new working directory for each replay.
