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
- **Edge addressing** (oracle design, pinned): nodes are addressed
  `<homeMethod>#<dumpLineIndex>`; METHOD nodes resolve first-wins across
  method walks sorted by fullName, so a method whose file-global sorts first
  is addressed inside it (`main` = `add.c:<global>#12`) while one that sorts
  earlier is its own `#0` (`add`).
- **CONTAINS** (`corpus/*`): sources are METHOD, TYPE_DECL, and FILE;
  destinations exclude LOCAL, parameters, METHOD_RETURN, MODIFIER, MEMBER.
  The per-file `<global>` TYPE_DECL contains the file-global METHOD and the
  file's method TYPE_DECLs; `F:<includes>` contains the includes-global
  method and every external TYPE_DECL.
- **REF resolution** (`corpus/structs.c`, `corpus/order.c`): identifiers REF
  the phantom ORDER=0 LOCAL (not the file-global LOCAL); `q.x` fieldAccess
  CALLs REF the struct MEMBER for value receivers, but `p->y` through a
  pointer stays unresolved (CDT quirk); every TYPE REFs its TYPE_DECL; every
  NAMESPACE_BLOCK REFs NS:<global> and SOURCE_FILEs its file.
- **Misc edge shapes**: ARGUMENT edges come from CALLs AND RETURNs to all
  direct children; while/switch bodies use TRUE_BODY (no WHILE_BODY/
  SWITCH_BODY kinds); FOR_INIT targets the init assignment CALL, not the
  LOCAL; EVAL_TYPE exists for exactly the nodes that carry TYPE_FULL_NAME
  (stub params/blocks included).
- **CFG shapes** (`corpus/*`, `corpus/logic.c`): evaluation order is args
  then call; METHOD -> first leaf; RETURN -> METHOD_RETURN. Statement BLOCKs
  are invisible but a comma BLOCK (child of a CALL) is a CFG node after its
  children — while stub method bodies, despite carrying ARGUMENT_INDEX=1,
  are invisible (stubs are METHOD -> METHOD_RETURN direct). Condition roots
  branch to both arm entries; back-edges target the condition's FIRST LEAF;
  do-while enters at the body; for-loop continue -> update entry; switch
  dispatches cond root -> every JUMP_TARGET (plus the continuation iff no
  default), case-value LITERALs are CFG nodes chained after their
  JUMP_TARGET, and fallthrough is natural chaining. Ternary: cond root ->
  arm entries, arms -> the conditional CALL. &&/||: lhs root -> rhs entry
  AND directly -> the call (short-circuit), rhs -> call.
- **goto/label** (`corpus/gotos.c`): a label flattens like a switch case —
  JUMP_TARGET (NAME = label, CODE = the WHOLE labeled statement) then the
  statement as a sibling consuming the next ORDER slot; `goto L;` is a
  childless CONTROL_STRUCTURE with a CFG edge to the JUMP_TARGET.
- **typedef** (`corpus/types2.c`): a TYPE_DECL *inside* the file-global
  BLOCK (consuming a slot; CODE keeps the whole `typedef ...;` statement),
  internal in the TYPE_DECL population; its UNDERLYING type registers as a
  used TYPE with its raw source spelling (`unsigned int` — NOT normalised,
  unlike variable types which become `longunsigned`).
- **enum** (`corpus/types2.c`): TYPE_DECL with one ANY-typed MEMBER per
  enumerator (CODE keeps `GREEN = 5`); initialised enumerators produce a
  <clinit> (phantom ANY LOCALs at ORDER=0 + one void assignment per
  initialiser). References to enumerators get plain-CODE ANY phantoms.
- **union typing** (`corpus/types2.c`): `union value v` types as
  `unionvalue` (concatenated!) while struct/enum strip the keyword — so the
  use-type is an IS_EXTERNAL TYPE_DECL while the definition TYPE_DECL
  (`value`) is internal. Both exist.
- **Function pointers** (`corpus/types2.c`): the param types as just the
  base/return type (`int` for `int (*fn)(int,int)`); a call through a
  pointer symbol becomes `<operator>.pointerCall` with DYNAMIC_DISPATCH —
  receiver at ORDER=1 with NO ARGUMENT_INDEX, args shifted to ORDER=2../
  INDEX=1..; ARGUMENT edges only to indexed children; NO CALL edge (dynamic),
  but the stub method still exists with arity = number of indexed args.
- **Sized-array locals** (`corpus/types2.c`): `int grid[2][3];` lowers to a
  void assignment whose CODE is the declarator text, wrapping
  `<operator>.alloc` typed `int[2][3]` with the TYPE NAME AS AN IDENTIFIER
  argument (no phantom local) followed by the dimension literals.
- **Macros** (`corpus/macros.c`): an invocation is an INLINED CALL — NAME =
  macro name, CODE = the original invocation text, METHOD_FULL_NAME and
  SIGNATURE = `<file>:<name>:<retType>(<nparams>)` with retType inferred
  from the expansion root; arguments first, then an ANY BLOCK (ORDER/INDEX =
  n+1) wrapping the expansion with parameters substituted. Each USED macro
  becomes a METHOD whose CODE is the `#define` directive itself (ORDER=1,
  params p1..pn, empty ANY BLOCK without ARGUMENT_INDEX, RET typed as the
  expansion); unused macros produce nothing. `#ifdef`/`#ifndef` content is
  spliced or dropped at parse level. Edge quirks: INLINED arguments carry no
  REF edge (expansion identifiers do); the expansion BLOCK gets no ARGUMENT
  edge and is CFG-invisible; CFG runs args -> call -> expansion content with
  both the call node and the expansion exit flowing to the continuation;
  macro methods get SOURCE_FILE and CONTAINS-from-file-global-TYPE_DECL but
  no method TYPE_DECL.
- **Real-world pins** (`corpus/bsearch.c`, musl, unmodified): pointer return
  types come from pointer levels above the function declarator
  (`void *bsearch` -> void*); NULL is an unresolved identifier — CODE
  `<unknown> NULL`, ANY, with a phantom ORDER=0 LOCAL (the general rule for
  fully unresolved identifiers); `(char *)x` casts type as the BASE type
  `char` only, while the TYPE_REF CODE keeps the raw `char *`; `else if`
  wraps the nested if in a synthetic CODE-less ANY-typed BLOCK; each
  `#include` becomes an IMPORT slot, pushing the file-global TYPE_DECL's
  ORDER to 1 + #includes; directive names and dropped #ifdef branches never
  produce phantoms.
