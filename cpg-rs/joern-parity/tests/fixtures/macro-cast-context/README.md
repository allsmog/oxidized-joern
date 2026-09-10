# Typedef cast context

These 46 isolated projects retain every AST, NODES, EDGES and FLOWS record from three fresh Joern v4.0.555 runs: 10,881 canonical lines, including 10,616 nonempty records. The accepted fifth-batch production binary matches six complete graphs. This isolated cast-context candidate matches 32; no previously exact project becomes nonexact. The selected text protocol does not cover every Joern property and its newline escaping is not injective.

Tree-sitter parses `(T)(value)` as a call through a parenthesized identifier even when a preceding declaration defines `T` as a typedef. The production frontend now keeps immutable, source-position type-name snapshots through available quoted headers and lexical scopes. Temporary macro parse trees receive the invocation's type-name state, and restore the original tree's state before disposal. Parameters, local declarations, block prototypes, for initializer declarations and enum constants can shadow type names. Existing conditional selection prevents inactive local typedefs from entering the state. Header type names become visible at the include position; unrelated translation units remain independent.

A complete, single, noncomment known type in the parenthesized callee becomes a cast with a TYPE_REF and the original value operands. Recovered syntax with extra/error children is not reinterpreted. Comma operands retain the pinned BLOCK shape. A directly supplied type argument, as in `CAST(T, value)`, can copy a TYPE_REF at its original argument index. Cast types retain the alias spelling; ordinary typedef scaffolding separately registers the underlying expression type. Local typedefs retain source ownership and distinct duplicate declarations in the pinned scope cases. A typedef written within a declaration macro remains a type binding within that expansion tree; this change does not export a newly introduced macro typedef into later original-source snapshots.

The complete `lua_intop` project, previously a retained fifth-batch diagnostic, now matches the live reference including the outer `lua_Integer` and inner `lua_Unsigned` casts, correct `v1`/`v2` and shift operands, copied argument slots, and all selected edges and reaching-definition facts. This reduced result is not a claim of complete Lua or general C cast parity.

`macro_cast_context.rs` compares all 32 exact production graphs. A separate production assertion checks that `sizeof(T)` distinguishes a visible typedef from a same-named parameter while preserving the full nonexact reference files. The existing 308-case gate remains unchanged.

Fourteen complete diagnostics remain in this isolated source candidate:

- `sizeof_typedef`, `sizeof_shadow`, `sizeof_minus`, and `sizeof_grouping`: separately owned expanded `sizeof` formatting and expression grouping differences. Type-vs-value identity is repaired; these files are not labeled exact here.
- `cast_chain` and `unary_casts`: broader expression precedence and chained typedef-cast parsing remain incomplete.
- `local_preprocessor_else`: the existing statement emitter omits a typedef inside the selected `#else` arm. The following cast receives the selected type-name state, but the complete declaration AST remains nonexact.
- `enum_shadow`: the cast/call distinction follows the local enum binding, but existing enum source-CODE/phantom spelling differs. Candidate output is byte-identical to the baseline for this project.
- `cast_recovered_name` and `cast_recovered_separator`: malformed input has different recovery ASTs in CDT and tree-sitter. Candidate output is byte-identical to the baseline; no extra recovered children are dropped by the new classifier.
- `global_pointer_shadow`, `global_function_shadow`, and `global_enum_shadow`: invalid same-scope namespace conflicts retain Joern's typedef-cast interpretation. Preexisting global declaration/enum scaffolding differs; no unsupported global-shadow rule was inferred from local scoping.
- `local_before_global`: the existing global registration pass allocates a later global alias before a same-named earlier local alias. Both declarations and casts are retained, but duplicate full-name ordering differs.

Each project contains unchanged source bytes, the full fresh reference, frozen baseline and candidate outputs, and their unfiltered differences. `oracle/` preserves complete raw runs and the exact scripts; reference extraction removes only the `AST|` prefix within each CASE. `measurement.json` binds all input/output files and frozen source/binary hashes. Absolute paths describe historical execution; the tests use portable embedded fixture files. The shared CASE script can replay a group after arranging that group's source files into its original per-case directories.

Run the production tests from `cpg-rs` with a target dedicated to the checkout:

```sh
cargo test -p joern-parity --test macro_cast_context
```
