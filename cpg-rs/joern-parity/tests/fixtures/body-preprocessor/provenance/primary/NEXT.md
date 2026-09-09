# Next bounded C unit: block-level preprocessor branches

The three missing live `snprintf` calls have one concrete cause. `gzlib.c:203–207`, `:276–280`, and `:556–563` guard the calls in `gz_open`, `gzdopen`, and `gz_error` with `#if !defined(NO_snprintf) && !defined(NO_vsnprintf)`. Statement lowering handles only `preproc_ifdef`; `preproc_if` falls through an empty default arm.

A fresh ten-project Joern4.0.555 run took12.27s. All1,534 canonical lines (1,495 nonempty selected records) are retained. The frozen sixth binary is full-exact on three controls: the plain void-cast snprintf call, equivalent `#ifndef`, and an inactive-call-only negative. Seven graphs remain nonexact. Even `#if1` drops its active return while keeping the later fallback.

The fix must also select branches in phantom collection. That walker currently visits both `#if` arms and condition identifiers, producing phantom LOCALs for inactive `discarded`, `NO_snprintf`, and `NO_vsnprintf`.

The smallest unit is a shared kept-branch traversal for statement emission and phantom discovery, reusing the existing definition-map condition evaluator and proper alternative routing. Keep the existing complete controls and source-position FLAG redefinition, and compare complete graphs for all six dispatch/phantom regressions. Body-local `#define` remains a distinct seventh diagnostic unless separately supported with its full reference.

The genuine calls above are unrelated to the previously removed incorrect23-argument call in inactive `gzprintf<duplicate>0`. Restore the correct calls and derive their stub without bringing that erroneous call back. After the focused repair, require the unchanged main308, existing fixtures, full unchanged-zlib replay and resource budgets.

No production changes or implementation were made. `next-unit.json` binds the exact source/binary, original-source lines, complete fresh references and all output/diff hashes.
