# Declaration, typedef and sizeof interactions

These 18 complete projects cover interactions between supplied-header declarations, source-position typedef knowledge, macro-expanded method parameters/returns, casts and `sizeof`. They retain 5,123 canonical lines, including 4,989 nonempty records, from pinned Joern v4.0.555. The accepted fifth-batch producer matches none of these complete graphs. The first combined sixth-batch producer matches 11; the reviewed combined producer matches 13. Every expected graph keeps all AST, NODES, EDGES and FLOWS facts within its CASE. The selected protocol does not include every Joern property and its newline escaping is not injective.

Two fresh cases exposed an integration defect. Method emission expanded declaration macros and correctly named a parameter `T`, while the type-context visitor read the original macro token as the parameter name. With `#define ARG int (*T)(int)`, the visitor wrongly treated `(T)(x)` as a typedef cast. With `#define PARAM int T`, it created a typed phantom for `sizeof(T)` instead of referring to the value parameter. The integration amendment uses the same resolved function header and macro snapshot for both parameter emission and lexical type shadowing. Both complete references now match without changing expected outputs.

The other exact projects cover include-position typedef visibility, forward header-call resolution, isolated translation units, primitive macro return spelling, adjacent method state, local compound-macro typedefs and restoration after temporary expansion trees. Four projects promote unchanged references from the separate `macro-cast-context` package: `sizeof_typedef`, `sizeof_shadow`, `sizeof_minus` and `sizeof_grouping`. Their isolated cast-candidate diagnostics remain immutable in that earlier package; combined `sizeof` and typedef-context repairs make their complete graphs exact here.

`sixth_context_interactions.rs` compares all 13 exact production graphs. Five full diagnostics are preserved:

- `header_alias_return`: CALL result typing retains alias `T` instead of Joern's underlying `int`; all other selected facts match.
- `argument_typedef_scope` and `block_restoration`: existing local pointer-call type and source-CODE differences remain.
- `comment_type_argument`: cast and copied TYPE_REF identity are correct, but expanded comment whitespace remains different.
- `declaration_macro_later_cast`: a typedef introduced by a standalone declaration macro does not enter later original-source type snapshots. The complete candidate output remains identical to the accepted baseline.

Each project retains its source, complete live reference, accepted baseline, first combined output, final combined output and every full difference. The first combined outputs preserve the failed-before parameter-shadow evidence. `measurement.json` binds source and graph hashes plus the frozen producer source/binary identities. `provenance/` records the static-review finding and source bindings. `oracle/` retains complete fresh raw runs and scripts. Its primary/boundaries runs are reused intact from the standalone cast package, including other CASE outputs; the four promoted projects still contain every fact from their individual references. No expected records were removed or rewritten.

The test artifact was generated after complete replay against the frozen combined production producer. Its compilation and the integrated workspace gates are owned by the integration worktree; this package makes no standalone cast-only test claim for cross-feature cases.
