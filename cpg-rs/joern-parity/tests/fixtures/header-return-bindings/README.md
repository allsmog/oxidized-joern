# Supplied-header return bindings

This follow-up resolves callable return bindings through typedefs at the header declaration's source position. Declaration display types retain their aliases. Unknown targets produce `ANY`; pointer suffixes apply only to known targets. The same include walk supplies caller definitions, header macro state, and earlier typedef bindings. Cyclic/forward unresolved aliases stay unknown without recursive expansion.

Included callable names now come from a complete, error-free, macro-expanded declaration. The raw parser can recover `API Unknown declared(int x);` as a function named `Unknown` with an error containing `declared`; that recovery name must not become a supplied-header callable. This change affects header binding lookup and does not rewrite ordinary METHOD or parameter displays.

## Complete references and limits

The 31 isolated projects contain 5,309 canonical lines including separators, or 5,142 nonempty selected records. Four complete graphs matched the preceding declaration-feature snapshot; 17 match this repair. All four previously exact graphs remain exact. `header_return_bindings.rs` compares those 17 complete AST/NODES/EDGES/FLOWS projections through production `Project::build` and the canonical exporter.

All 14 nonexact projects retain complete source, Joern reference, before/current output, and unfiltered differences:

- `header_macro_parenthesized`: synthesized prototype CODE still differs.
- `forward_tag`, `macro_alias`, `struct_same`, `unknown_alias`, `unknown_macro_alias`, `unknown_pointer_alias`, `late_alias`, `pointer_alias`: return CALL binding matches; separate typedef/tag/TYPE scaffold differences remain in this isolated baseline.
- `callback_alias`: a function-pointer typedef return still resolves to `ANY` instead of `int(*)(int)`, alongside existing typedef scaffolding differences.
- `unresolved_prefix`: unresolved prefix recovery in ordinary header AST emission still differs. It does not create the incorrect return-type callable in the importing function.
- `review_invalid_redeclaration`: invalid `typedef unsigned UNKNOWN T` does not erase the preceding valid `T` binding; ordinary invalid-declaration scaffolding still differs.
- `recovery_valid_then_error`, `recovery_error_then_valid`: an individually valid typedef inside a macro expansion retains its binding despite other erroneous declaration text. Their CALL return bindings match; full declaration scaffolding remains nonexact.

The focused binding tests include some nonexact scaffold projects and are explicitly property assertions. It does not promote those complete graphs to conformance. No expected record is filtered or rewritten to pass a comparison.

## Provenance

`measurement.json` binds all input bytes, complete outputs, exact/diagnostic classifications, raw producer transcripts, scripts, source hashes, and frozen binaries. Six fresh raw oracle groups are retained under `provenance/`. They use pinned Joern v4.0.555 and the shared oracle projection unchanged; the wrapper invokes it separately for each isolated project and records the case name. The shared newline escape transport is non-injective and does not represent every Joern property.

The baseline is declaration-feature commit `3737641c7f1d8083cfc90c6af4c116ddd5939ddb`, with exact.rs `5c3edebb...` and importer `3fb66955...`. This is a repair of that feature's header lookup, not a comparison against the earlier fifth-batch baseline. The earlier fifth/combined-v1 investigative runs remain in ignored local evidence.

`provenance/whole-lua/` binds the complete unmodified Lua producer output retained locally. The entire 61-line `getendpos` METHOD AST is exact again (11 unified diff lines before, zero after). All 32 canonical CALL records for `luaS_newlstr` retain `TString*`; both `luaG_concaterror` CALL records retain `void`. These counts include nested duplicated method views. The whole Lua graph remains nonexact, and its `luaS_newlstr` definition signature remains a separately documented mismatch.

This patch does not supply missing system headers, alter the predefined-macro environment, or decide whether malformed typedefs introduce cast type names. Those are separate integration concerns.

Independent review rejected an intermediate whole-expanded-root validity check: fresh Joern preserves an individually valid typedef binding despite erroneous sibling expansion text. The final repair retains that recovery behavior and rejects only the measured primitive-modifier/type-name combination. Both rejected candidate outputs and review receipts remain in local evidence; the full two recovery diagnostics are portable here.
