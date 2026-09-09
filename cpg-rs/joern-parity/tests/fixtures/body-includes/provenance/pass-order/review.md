# Pinned macro METHOD ownership: header-pass precedence

The previously stated pass-order hypothesis is now confirmed by pinned source, without running a graph producer. `bindings.json` records downloaded commit sources and unchanged committed upstream files for Joern v4.0.555, commit `d95237aeaf3d12cb4e63336def3a4d9d7315dfb4`. This supplements the held V1 review; it does not change that receipt or its raw losses.

The construction sequence is explicit: a source pass runs first, followed by a separate header pass carrying its accumulated state. FunctionDeclNodePass then consumes the latter pass's declarations. The second pass selects `.h`; C++ header extensions belong to the first extension group. [C2Cpg.scala](https://raw.githubusercontent.com/joernio/joern/d95237aeaf3d12cb4e63336def3a4d9d7315dfb4/joern-cli/frontends/c2cpg/src/main/scala/io/joern/c2cpg/C2Cpg.scala), local lines29–43 and48–51; [FileDefaults.scala](https://raw.githubusercontent.com/joernio/joern/d95237aeaf3d12cb4e63336def3a4d9d7315dfb4/joern-cli/frontends/c2cpg/src/main/scala/io/joern/c2cpg/parser/FileDefaults.scala), local lines11–17.

The decisive merge is in local `AstCreationPass.scala:110–112`: the new accumulator merges the previous accumulator into itself. `mergeWith:63–64` inserts a previous METHOD declaration only when that full name is absent. `registerMethodDeclaration:34–35` also keeps the first entry within an accumulator. Therefore a header-pass METHOD declaration wins over a same-full-name source-pass declaration. This is precedence of complete MethodInfo records, not an independent file-property override.

`MacroHandler.scala:123–129` derives the macro stub's AST parent from the current file-global TYPE_DECL; line155 stores the current AstCreator `filename`. The distinct defining file builds the full name at102–103. `FunctionDeclNodePass.scala:129–143` copies the selected MethodInfo properties, including filename and AST parent, onto METHOD. There is no rule here that always assigns a macro stub to the physical invocation file or defining file.

## Why the measured owners differ

The full live header-global blocks and ownership edges are saved in `live-header-competitors.json`.

| Full name | Independently generated header-pass competitor? | Winning observed owner |
|---|---|---|
| main.c:N:int(0) | No: standalone headers do not have the caller's N2 definition. | main.c |
| nested/table.h:N:int(0) | Yes: the standalone nested/table.h global contains an actual N4 macro CALL in last_table's initializer. | nested/table.h |
| headers/leaf values.h:LEAF:int(0) | No: that header defines LEAF but initializes guarded with literal3. Its macro is used by caller methods. | main.c |
| scope.h:ESCAPED:int(0) | No: that header defines ESCAPED but does not invoke it in a header-global expression. Its macro is used by caller methods. | main.c |

This explains both stable caller-owned controls and the two newly wrong V1 edges. V1 emits main.c first and its new included uses fill the single global macro registry. Later header emission cannot replace those entries. The accepted baseline did not emit those included uses, so the header's own registration was previously uncontested. Correcting lexical use order is still necessary for wrapper and definition selection; it does not replace the independently established pass-precedence rule.

## Bounded implementation shape

Retain source-pass versus `.h`-pass provenance for each actually generated macro MethodInfo registration. Preserve first registration within each class. At finalization, prefer a generated header-pass entry for the same complete full name over a source-pass entry; otherwise retain the source entry. Choose the full metadata tuple together, including CODE, filename and parent ownership. Do not choose merely because a definition lives in a header: LEAF/ESCAPED disprove that shortcut.

This can be a priority-aware registration helper or two small maps. It does not require changing expression state, sorting the entire emitter into a new file order, or making header-origin source nodes pretend to be another translation unit. A wrapper's emitted full-name/return/arity remains independently determined by its selected metadata and expression role. The already frozen twelve graphs exercise the required N2/N4 competition and the noncompeting controls.

The source proves inter-pass precedence. It does not settle arbitrary ordering among several files in the same parallel pass, nor justify new C++ support claims. Preserve the current measured within-class order unless another retained case demonstrates a defect. Do not repair the two edges with a filename-specific exception.
