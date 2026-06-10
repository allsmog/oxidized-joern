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
- **switch flattening** (`corpus/switch.c`): the switch body BLOCK holds, as
  flat siblings: JUMP_TARGET (NAME=`case`/`default`, CODE `case 1:` /
  `default:`), then for `case` the value as a bare child with ORDER but NO
  ARGUMENT_INDEX, then the statements. `break;`/`continue;` are childless
  CONTROL_STRUCTUREs whose CODE includes the semicolon. The switch condition
  is unwrapped (no parens) at ORDER=1; body BLOCK ORDER=2.
- **Signed literals** (`corpus/switch.c`): CDT lowers `-1` to
  `<operator>.minus` applied to LITERAL `1` (tree-sitter folds the sign in).
- **Plural `<operators>.`** (`corpus/exprs.c`): assignmentModulo, ShiftLeft,
  ArithmeticShiftRight, And, Or, Xor use prefix `<operators>.` while
  assignmentPlus/Minus/Multiplication/Division use `<operator>.`.
- **Type renderings** (`corpus/exprs.c`, `corpus/structs.c`):
  `unsigned long` → `longunsigned`; `struct point` → `point` (tag dropped,
  const dropped); arrays → `int[]`; CODE keeps source spellings.
- **Phantom ORDER=0 LOCALs** (`corpus/exprs.c`, `corpus/structs.c`): atop the
  method body BLOCK, one LOCAL per sizeof(T) type name (NAME=CODE=TYPE=T) and
  per referenced unshadowed global (CODE `<global> name`). A global's
  IDENTIFIER also gets CODE `<global> name`.
- **cast/sizeof/comma** (`corpus/exprs.c`): `(T)e` → `<operator>.cast` typed T
  with TYPE_REF arg 1; `sizeof(T)` → `<operator>.sizeOf` with T as an
  IDENTIFIER typed as itself; `(a, b)` → a CODE-less BLOCK typed ANY whose
  children carry ORDER but no ARGUMENT_INDEX.
- **Literal typing** (`corpus/exprs.c`): `1.5` → double, `'x'` → char,
  `"hi"` → char*.
- **Field/array access** (`corpus/structs.c`): `.` → `<operator>.fieldAccess`,
  `->` → `<operator>.indirectFieldAccess`, member as FIELD_IDENTIFIER with
  CODE only (no NAME); `a[i]` → `<operator>.indirectIndexAccess` (c2cpg does
  not emit plain indexAccess here).
- **Multi-declarator CODE** (`corpus/structs.c`): `int a, b = 1;` yields
  LOCALs with rebuilt CODE `int a` and `int b` (decl-specifier + that
  declarator only), then the `b = 1` assignment as a separate sibling.
- **Operator stub methods** (whole corpus): one METHOD per
  called-but-undefined name, FULL_NAME=NAME, ORDER=0, no CODE/SIGNATURE;
  params p1..pn (n = max arity seen anywhere in the project), all ANY. Child
  layout is Joern's stable sort by ORDER over insertion order
  [IN p1..pn, BLOCK(O1, ARGUMENT_INDEX=1), RET(O2), OUT p1..pn]: so
  `IN p1, BLOCK, OUT p1, IN p2, RET, OUT p2, IN p3, OUT p3, ...`
  (arity 1: `IN p1, BLOCK, OUT p1, RET`).
- **File-global wrapper** (`corpus/add.c`, `corpus/structs.c`): METHOD
  NAME/CODE=`<global>`, FULL_NAME=`<file>:<global>`, ORDER=1. Children in
  source order: TYPE_DECLs and full nested METHOD dumps (each ORDER=1
  regardless of position), then a CODE-less BLOCK (ANY, ORDER=1) holding one
  slot per top-level construct in source order — TYPE_REF (CODE = whole
  struct text) for a struct def, LOCAL per global object declarator,
  METHOD_REF (CODE=TYPE=METHOD_FULL_NAME=name) per function definition;
  prototypes consume NO slot — then METHOD_RETURN (RET, ANY, ORDER=2).
  A bare `<includes>:<global>` method (BLOCK + RET only) always exists.
- **TYPE_DECL/MEMBER** (`corpus/structs.c`): TYPE_DECL NAME=FULL_NAME=tag
  (struct keyword dropped), CODE = full struct source text; MEMBERs in
  declaration order with CODE = just the member name.
- **Scaffolding nodes** (`corpus/*`): META_DATA LANGUAGE=NEWC; FILE nodes for
  every source file (ORDER=0) plus `<includes>` (ORDER=1) and `<unknown>`
  (ORDER=0); one NAMESPACE_BLOCK per file plus `<global>`@`<unknown>` and
  `<includes>:<global>`; a single NAMESPACE `<global>` with no ORDER.
- **TYPE_DECL population** (`corpus/*`): internal structs carry EMPTY-string
  AST_PARENT_TYPE/AST_PARENT_FULL_NAME; every defined function gets a
  TYPE_DECL with AST_PARENT_TYPE=TYPE_DECL parented at `<file>:<global>`;
  each file gets a `<global>` TYPE_DECL parented at its NAMESPACE_BLOCK; every
  other referenced type (builtins, pointers, arrays, ANY) becomes
  IS_EXTERNAL=true under `<includes>:<global>` with NO ORDER property. TYPE
  nodes exist for exactly the set of TYPE_FULL_NAME strings emitted anywhere.
- **<clinit>** (`corpus/order.c`): a struct with a sized-array member gains a
  synthetic METHOD `<clinit>` (FULL_NAME `tag.<clinit>:tag()`) after its
  MEMBERs: a BLOCK with NO properties at all, one `<operator>.arrayInitializer`
  CALL (CODE = `arr[4]`, arg = the size literal) per sized member, two bare
  MODIFIERs (ORDER 2,3), and METHOD_RETURN typed as the struct. It is also a
  top-level method in the method set (and spawns the arrayInitializer stub).
- **Members are declarators** (`corpus/order.c`): MEMBER CODE is the
  declarator text (`*ptr`, `arr[4]`), and sized arrays keep the size in the
  type: `int[4]` (vs `int[]` for an unsized param).
- **Global initialisers** (`corpus/order.c`): `int g = 5;` lowers inside the
  file-global BLOCK exactly like a method-body declaration — LOCAL slot, then
  a void-typed assignment CALL slot — and the lhs IDENTIFIER there is plain
  `g`, while references inside methods use CODE `<global> g`.
