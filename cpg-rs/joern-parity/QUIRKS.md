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
- **Pointer rendering** (`corpus/unary.c`): TYPE_FULL_NAME and SIGNATURE
  normalise `int *p` to `int*`, but CODE keeps the source form (`int *p` for
  the param, `int *q` for the LOCAL). The declaration assignment's CODE keeps
  the star too: `*q = &v` — while its lhs IDENTIFIER is plain `q`.
- **Unary lowering** (`corpus/unary.c`): `- ! ~ * &` →
  `<operator>.minus/.logicalNot/.not/.indirection/.addressOf`; `++`/`--` →
  pre/postIncrement/Decrement by operator position. Single child has
  ORDER=1 ARGUMENT_INDEX=1.
- **for CODE rebuilt** (`corpus/forloop.c`): CONTROL_STRUCTURE CODE is
  `for (init;cond;update)` — reconstructed, with NO space after the
  semicolons, unlike the source.
- **for-init ARGUMENT_INDEX** (`corpus/forloop.c`): the init declaration is
  flattened into the for's children (LOCAL ORDER=1, assignment ORDER=2) and
  the assignment carries ARGUMENT_INDEX=1; the condition, update, and body
  BLOCK carry none.
- **do-while CODE** (`corpus/forloop.c`): the entire statement including the
  trailing semicolon (while = header only, if = full statement, for =
  rebuilt header — all four differ). Children: BLOCK ORDER=1, condition
  ORDER=2.
- **Ternary** (`corpus/forloop.c`): `<operator>.conditional` with three
  children at ORDER/ARGUMENT_INDEX 1,2,3.
