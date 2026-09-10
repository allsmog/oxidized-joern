# First twelve body-include references: independent diagnosis

The complete fresh references and frozen ninth-batch Rust outputs are bound in `verification.json`. All 12 raw CASE groups extract byte-for-byte to the supplied references: **4,676 canonical lines including separators, 4,588 nonempty selected records**. All 24 input files match the producer snapshot and baseline input hashes. Only `used_scalar_inline` and `primitive_static_array_inline` are complete exact projects on this baseline. No producer, build, source edit, or runtime-inventory re-audit was performed.

Every AST/NODES/EDGES/FLOWS record and its multiplicity remains in the original bound reference/output and complete diff. `source-facts.json` contains explanatory complete method blocks and their selected edge/flow records; it is not an exactness filter. This diagnoses the baseline and does not accept an implementation.

| Existing case | Caller global ORDER, live / baseline | Measured behavior |
|---|---:|---|
| body_include_macro_only | 2 / 1 | Header contributes `included` LOCAL ORDER1; return moves to ORDER2. FLAG already selects the correct caller branch on the baseline. |
| used_scalar_include | 2 / 1 | Header contributes LOCAL `included:int`, assignment `included = 7`, then return, at ORDER1/2/3. |
| used_scalar_inline | 1 / 1 | Complete exact scalar control. |
| primitive_static_array_include | 2 / 1 | Header contributes real `table:int[2]`, allocation and `{1, 2}` initializer before the indexed return. |
| primitive_static_array_inline | 1 / 1 | Complete exact static primitive-array control. |
| tiny_fixedtables_include | 2 / 1 | Both real Code arrays, nested initializers and correctly typed final field-assignment operands appear in the live method. |
| tiny_fixedtables_inline | 1 / 1 | The entire `fixedtables` method AST is already exact; seven missing/seven extra records isolate an existing MEMBER/type-scaffold spelling difference. |
| inactive_body_include | 3 / 1 | Both inactive include occurrences consume global-order slots, but neither executes declarations or LEAK effects in caller bodies. |
| repeated_include_context | 3 / 1 | Each caller include expands the nested header separately; first-table dimensions differ by caller state, last-table dimensions reflect header redefinition. |
| guarded_nested_relative_include | 4 / 1 | Three direct caller directives count, while only one `guarded` declaration/initializer appears after guard and once suppression. |
| nested_scope_typedef_macro | 2 / 1 | Included typedef/object belong to the inner caller block; macro ESCAPED persists outside it and into the later function. |
| unresolved_header_controls | 3 / 1 | The quoted decoy **is resolved** by this Joern run; it injects `wrong = 9` and selects `return wrong`. No `wrong_system` declaration is injected into this caller. |

## Selection and import order must stay separate

`inactive_body_include` differs in exactly one complete selected record: the caller global TYPE_DECL ORDER is 3 rather than 1. All AST, other NODES, EDGES and FLOWS records are otherwise byte-identical. `choose` retains only the active `included = x` and returns it; `ignored` retains an empty body; `later` returns x. The supplied header still has its independent file-global graph, which is not a leak into the caller.

In the guarded case, three direct caller includes produce ORDER4 although only one LOCAL/assignment pair is inserted. The nested include belongs to `headers/api.h`; that header's independent global TYPE_DECL has ORDER2. In the repeated case the caller has ORDER3 for its two direct includes, while `repeat.h` independently has ORDER2 for its nested directive. Counting emitted declarations or successful expansions would therefore be wrong in these measured cases. This agrees with the previously bound pinned import helper's containing-file distinction.

## Repeated inclusion pins expression state and wrapper absence

Both methods emit first_table LOCAL ORDER1, allocation ORDER2, explicit initializer ORDER3, last_table LOCAL ORDER4, allocation ORDER5, initializer ORDER6 and return ORDER7.

- In `first`, first_table is `int[2]`, last_table is `int[4]`. All four N uses have INLINED wrappers targeting `main.c:N:int(0)`; that METHOD's CODE is `#define N 2`, including wrappers whose literal child is 4.
- In `second`, first_table is `int[3]`, and its allocation dimension and initializer are **plain LITERAL 3 nodes, without N wrappers**. last_table remains `int[4]`; its two N wrappers target `nested/table.h:N:int(0)`, whose METHOD CODE is `#define N 4`.

The complete graphs establish these shapes, not a new causal CDT trace. They make the old implementation's fallback-wrapper behavior unsafe to assume for newly emitted header nodes. Correct macro values and array types alone do not close this control. Preserve file-local event selection, distinct include occurrences, and the observed absent-wrapper case.

## Type scope and physical ownership are independently observable

Inside `choose`'s nested block, `Local` is a TYPE_DECL, `inside` is a real LOCAL, and `(Local)(x)` is `<operator>.cast` with TYPE_REF Local. Outside the block and in `later`, `Local(x)` remains the ordinary declared function call. ESCAPED expands to literal 4 in both outer and later returns.

The two selected TYPE_DECL scaffold records, FULL_NAME `Local` and `Local<duplicate>0`, both have **FILENAME=scope.h**. The first is emitted inside caller choose; the header's independent global view holds the duplicate. This refines the earlier source-derived architecture sketch: keeping caller `Ctx.file` for method ownership is appropriate, but TYPE_DECL physical filename must come from the included source independently. Do not use a single file field for both roles.

## Existing compound primitive diagnostic

The inline tiny fixedtables graph has exactly seven live-only and seven baseline-only records. Live spells the Entry.value MEMBER type `shortunsigned`; Rust spells it `unsigned short`. The remaining six replacements are its TYPE/TYPE_DECL and four structural type edges. The complete fixedtables method AST itself is identical. Retain this diagnostic unchanged; an include-splicing patch must not claim full tiny-project equality merely because the arrays are restored, or disguise this unrelated gap with type-record filtering.

## Correct the negative control's interpretation, not its evidence

`unresolved_header_controls` is not a proven unresolved-quoted-header negative. The supplied `decoys/missing.h` is the unique source of the injected `int wrong = 9`; FOUND selects the live caller's `return wrong`, while the baseline returns x. `decoys/missing-system.h` still has its standalone file-global graph, but its `wrong_system` declaration is absent from the caller body. The simultaneous FOUND definitions do not independently prove every detail of angle-header macro resolution.

This one graph does not establish a general basename-search rule or collision precedence. Preserve its original name, inputs and complete outputs with this corrective annotation. Do not add fallback search to the implementation just to make this case pass. If resolver scope is deliberately expanded later, a quoted include with two competing basenames and an exact-relative target is needed before choosing precedence. A genuinely absent quoted target with no supplied matching file is the minimal replacement negative; it can remain an additive, proposed control until requested.

No additional broad corpus is required by this diagnosis. The already proposed header-initializer/later-identical-caller-call metadata pair is still useful for the separately identified source-location cursor risk. These twelve references contain no LINE_NUMBER/COLUMN_NUMBER projection, so no location parity follows from their full selected-graph comparisons.
