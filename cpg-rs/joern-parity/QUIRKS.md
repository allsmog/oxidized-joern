# QUIRKS — c2cpg conventions discovered via the oracle

Joern is the spec, including its quirks. Each entry names the corpus file that
pins it, so a regression shows up as a diff.

- **Operator lowering** (`corpus/add.c`, `corpus/ops.c`): binary operators
  become CALL nodes named `<operator>.addition` etc., with
  `METHOD_FULL_NAME` = the same name and `DISPATCH_TYPE=STATIC_DISPATCH`.
  The CALL's own TYPE_FULL_NAME is `ANY` even when operand types are known.
- **Declaration split** (`corpus/add.c`): `int x = e;` lowers to a LOCAL
  (CODE `int x`) plus an `<operator>.assignment` CALL.
- **void vs ANY assignments** (`corpus/ops.c`): the assignment generated from
  a declaration initialiser has TYPE_FULL_NAME=`void`; a *bare* assignment
  statement (`r = e;`) has TYPE_FULL_NAME=`ANY`.
- **if vs while CODE** (`corpus/ops.c`, `corpus/loop.c`): an `if`'s
  CONTROL_STRUCTURE CODE is the entire statement including both branches; a
  `while`'s CODE is only the header `while (cond)`.
- **else nesting** (`corpus/ops.c`): `else` is its own CONTROL_STRUCTURE
  (CODE exactly `else`, ORDER 3 under the `if`), wrapping the alternative
  BLOCK at ORDER 1.
- **Synthetic method children** (`corpus/add.c`): every method gets a
  METHOD_RETURN (CODE `RET`) and, per parameter, a METHOD_PARAMETER_OUT
  mirroring the METHOD_PARAMETER_IN with the same ORDER. Body BLOCK has
  TYPE_FULL_NAME=`void`; ORDER runs params(1..n), block(n+1), return(n+2).
- **RETURN children** (`corpus/add.c`): the returned expression has ORDER=1
  and no ARGUMENT_INDEX (unlike call arguments, which carry both).
