# Declaration macro and supplied-header type context

These 36 isolated projects were imported by Joern **v4.0.555** with JDK21. The accepted fifth-batch binary matches 6 complete projections; this candidate matches 29. The references contain 5,019 canonical lines including method separators, or 4,872 nonempty selected records. Each project is compared independently so another project's TYPE pool cannot hide missing or additional records.

`cases/` contains the 29 exact projects, and `diagnostics/` retains the seven nonexact projects. Every directory includes the original relative source paths, complete `expected.txt`, complete `before.txt` and `final.txt`, and both full unified differences. No graph facts are removed from a diagnostic to turn it into an exact case. `measurement.json` binds all source/reference/output bytes, the frozen binaries and sources, and four complete raw oracle runs under `provenance/`. The primary historical run's `inputHashes` hashes serialized source mappings; the measurement's per-case `inputs` fields are the authoritative source-byte hashes.

The implementation evaluates declaration specifiers in the existing immutable macro environment at their source positions. It preserves the separate definition-return, declaration, and expression-result spellings. Supplied quoted headers contribute callable declarations under each caller's current macros; pinned Joern resolves these callable declarations even for calls before a later include. Unknown or missing headers retain the earlier unresolved behavior. Original parameter CODE remains intact. Nonempty leading declaration macros use the observed generated specifier/declarator CODE, while ordinary leading qualifiers retain raw CODE. Empty leading object macros have source-verified method spans, including multiline and same-name conditional definitions.

The exact fixtures cover object/function macros, recursive and chained object definitions, define/undef order, empty and attribute prefixes, pointer/array parameters, qualifiers, comments, quoted literals, Unicode and UCN negative controls, relative subdirectory headers, include guards, and two callers that select different header return types. The missing-argument fixture pins Joern's observed recovery for an unused missing formal; it is not a general claim about invalid C recovery.

The retained gaps are:

- `width_macro`: a local macro declaration needs its INLINED declaration wrapper and expanded local type.
- `local_prototype`: a prototype inside a function still lacks the enclosing declaration-macro context.
- `replacement_call_boundary`: an object replacement supplying a function-macro name needs a further rescan across the original `()` boundary.
- `header_pointer`: the original parser scaffolding for a macro-prefixed tagged pointer declaration differs from Joern.
- `header_alias_pointer` and `plain_alias_pointer`: underlying typedef CALL types and named-type pointer definition returns remain different.
- `function_macro_extra`: the original parser drops an invalid extra-argument declaration that Joern retains as UNKNOWN.

Whole Lua remains nonexact. The retained whole-project run confirms `luaG_concaterror` changes from `l_noret` to `void` for its signature and from ANY to void for its two canonical CALL records. All 32 canonical `luaS_newlstr` CALL records become TString*, matching Joern; those counts include repeated nested method views. Its definition still returns TString* in Rust versus TString in Joern. The package does not claim full schema, complete C preprocessing, or whole-project parity.

The conditional location control preserves the preexisting inactive METHOD/METHOD_RETURN line 7 (the raw API prefix); live Joern reports line 8. Active method, parameter and return-statement locations match the checked source lines. General source-location parity is not claimed.

The selected oracle's newline escaping is unchanged and is not injective. No reference bytes are synthesized from Rust output.

Run the production assertions with:

```sh
cargo test --locked --manifest-path cpg-rs/Cargo.toml -p joern-parity --test declaration_macros
```
