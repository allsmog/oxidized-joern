# Statement macro expansion: pinned reference evidence

These 28 isolated C projects were imported by Joern v4.0.555 using the unchanged shared `oracle.sc` and private workspaces. The 26 projects in `cases/` contain 8,985 complete selected AST/NODES/EDGES/FLOWS records and match the candidate byte for byte; none matched the frozen pre-change baseline. Two nonconformant projects retain all 662 selected records plus complete before/after outputs and diffs in `diagnostics/`. This is bounded construct coverage, not full C or Joern parity.

The family covers brace and empty replacement blocks; do-while, while, if/else, for and return statements; nested macro replacements and blocks; scalar, pointer, qualified and array declarations; local shadowing; repeated invocations; and reduced Lua `SET`/`setobjs2s` field assignments. Joern reuses a compound replacement as the invocation's expansion BLOCK, assigns it the invocation CODE and `void` type, and retains noncompound controls inside a separate `ANY` expansion block. Generated declaration CODE conventions are pinned without normalizing the reference.

`statement_macros.rs` builds every conformant project through the production `Project.build` path and compares the complete projection. Its scanner test asserts all seven live outcomes, including clean, overwritten and unused-argument negatives. Four missing positive findings are restored: brace, do-while, if and while macro bodies. The before measurement imports the frozen baseline graph into the unchanged production analysis; its replay source and output are bound in `measurement.json`. A third test checks that all generated nodes remain at their invocation in two repeated-macro methods and that following calls retain their own source lines.

The source patch preserves the parsed kind of expanded controls in an internal address map used by CFG construction and its reaching-definition input. This is necessary because Joern's do-while CODE is the macro invocation rather than the `do` keyword. It changes no solver rules. The current production schema does not expose a `CONTROL_STRUCTURE_TYPE` property; this patch does not claim property parity beyond the selected projection.

The retained diagnostics are:

- `lua_shape`: the typedef-plus-struct declaration has preexisting global/type scaffold differences; the repaired method body matches the live graph.
- `block_comments`: comment-split macro-definition text leaks a declaration into the file-global graph in the frozen baseline and this bounded candidate. Macro-definition parsing belongs to a separate repair; no graph records are removed here.

`oracle/<case>/` retains raw stdout, stderr and run metadata for every input. `measurement.json` binds all source/reference/raw hashes, the pinned archive and oracle script, the copied-source baseline, candidate binary and patch, and test receipts. The working-tree Git base alone does not identify the baseline: three root source files were independently frozen before this implementation. Only the `AST|` transport prefix is removed when extracting references; all four sections remain complete. Shared newline escaping is noninjective and is unchanged.

The dedicated-target validation passed 278 C frontend, analysis and parity tests, strict Clippy and formatting, the original 308 committed/live comparison blocks, and the five RD scheduling/ordering/old-loop tests copied from integration for validation. Those five existing tests are not duplicated in this fixture commit.

The integrated fourth batch additionally repairs recovered declarations leaking from comment-containing macro directives. `macro_comments_do_not_create_file_scope_declarations` now enforces the complete 263-line `diagnostics/block_comments` graph. Its original nonexact snapshot remains intact; 27 of these 28 retained graphs are now exact. The typedef diagnostic remains nonexact.
