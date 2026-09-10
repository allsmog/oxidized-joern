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
- **musl string-fn pins** (`corpus/memcmp.c`, `corpus/strcmp.c`):
  multi-declarator initialisers emit ALL LOCALs before any assignment
  (`const unsigned char *l=vl, *r=vr;` -> LOCAL l, LOCAL r, then the two
  assignments, CODE = raw init-declarator text `*l=vl`); an empty for-init
  clause is a CODE-less ANY BLOCK placeholder that still receives FOR_INIT;
  a comma update is a BLOCK, so for-clause identity is positional; a
  body-less `for (...);` branches its condition root straight to the update
  entry; `unsigned char` keeps its space in TYPE_FULL_NAME (unlike
  `longunsigned`), and a declaration ALSO registers its decl-specifier type
  — `unsigned char c` registers bare `unsigned`, `const char *p` registers
  `char` (CDT's typeForDeclSpecifier path).

- **Identifier truth tests and loop bodies** (`corpus/control_truth_loops.c`):
  scalar identifiers become integer `!= 0` calls; plain pointer identifiers
  use `!= NULL`. Explicit comparisons, calls, arithmetic, and negation keep
  their existing expression shape. Braceless loop bodies are direct children
  of the control structure. Missing FOR clauses reserve ORDER slots, and
  multiple initializer declarators share one initializer BLOCK after their
  LOCALs. A `do` body ending in unconditional return leaves the condition and
  subsequent tail disconnected; those nodes have no reaching-definition facts.
- **External declarations** (`corpus/external_functions.c`,
  `corpus/external_declarations.c`, `corpus/prototype_a.c`,
  `corpus/prototype_b.c`): unused prototypes remain external METHOD scaffolds;
  repeated ordinary declarations coalesce and a matching ordinary definition
  supersedes its prototype.
  Unnamed parameters keep empty names and pair IN/OUT by position. A `(void)`
  declaration retains a void parameter. Variadic signatures contain `...`,
  while the synthetic `<param>N` parameter takes the preceding parameter's
  type. Unresolved zero-argument calls create a stub with `p0` at ORDER=0.
- **Callable values and lexical scope** (`corpus/function_pointers.c`,
  `corpus/callable_scope.c`): function-pointer object types retain declarator
  shape (`int(*)(int)`), their LOCAL CODE includes the full declaration, and
  their initializer's LHS IDENTIFIER has empty CODE. Address-valued references
  to known functions are METHOD_REF nodes. A block or FOR initializer can
  shadow a function or parameter; leaving that scope restores the previous
  binding. Parenthesized callees use pointerCall even when naming a function.
- **FOR locals in the lone-identifier optimization**
  (`corpus/callable_scope.c`): Joern's Method.local traverses contained BLOCKs
  and their direct LOCAL children. A LOCAL directly under a FOR control
  structure is absent from that list. Thus an otherwise lone initializer
  identifier can be removed from its assignment GEN set, even though the
  declaration exists in the AST. Verified against the pinned runtime's
  MethodMethods and OptimizedReachingDefTransferFunction bytecode and live
  reaching-definition output.
- **Conditional translation-unit declarations**
  (`corpus/conditional_top_level.c`, `corpus/conditional_prototypes.c`): both
  active and inactive declarations retain scaffolding. Inactive function
  bodies retain BLOCK CODE without executable children. Macro definitions and
  `#undef` apply in source order; each method keeps the macro environment at
  its position. Repeated same-file definitions receive distinct identities.
- **INLINED reaching-definition entry edges** (`corpus/macros.c` and the
  `corpus/conditional_macro_order.c` fixture): expansion BLOCKs carry ARGUMENT_INDEX but have
  no ARGUMENT edge. They are excluded from the call's actual argument set.
  A call with arguments does not get a method-entry dependency merely because
  its arguments are literals; a zero-argument macro can retain that dependency.
- **File-scope sized arrays**
  (`tests/fixtures/array-declarations/arrays.c`): uninitialized globals emit
  an `<operator>.arrayInitializer` with dimension arguments. Local arrays
  retain the assignment and `<operator>.alloc` form, including its type
  operand. Global dimensions must not enlarge the local allocation stub's
  arity. Explicit initializers have separate, incompletely matched behavior.
- **Parenthesized declarations and typedefs** (`corpus/mixed_prototypes.c`,
  `corpus/parenthesized_prototypes.c`, `corpus/parenthesized_definitions.c`,
  `corpus/typedef_shapes.c`): parenthesized functions retain unresolved
  namespace identities. An ordinary call can therefore require a separate
  stub. Mixed declarations classify each declarator independently; ordinary
  typedef aliases survive even beside function typedefs that produce no
  alias node. Supplied quoted relative headers contribute ordered macro
  definitions and removals for declaration spelling. These cached header
  effects do not implement caller-conditioned header preprocessing.

- **Macro metadata can precede its current replacement**
  ([complete body macro state fixtures](tests/fixtures/body-macro-state/README.md)):
  c2cpg's macro expansion-event queue is stably sorted by file-local offsets,
  including condition and supplied-header events. An earlier matching event can
  supply synthetic METHOD CODE and its defining-file full-name prefix; first
  registration by full name wins. Current actual arguments and expansion type
  determine arity/type, while source-file ownership stays with the invoking TU.
  Quoted text and `defined` operands do not expand. Object callees retain separate
  argument events unless a still-eligible function macro consumes the following
  parentheses; disabled recursive tokens remain ineligible. This bounded port
  retains a current-metadata fallback when no event matches and documents the
  remaining complete diagnostics; it does not claim full MacroHandler parity.

### Included declarations and macro METHOD pass precedence

The [complete body-include family](tests/fixtures/body-includes/README.md) pins
that selected body-header declarations join the caller's lexical block, while
local typedef TYPE_DECL file properties and expression line coordinates can
refer to the physical header. Caller-file include directives consume dependency
sibling positions even in the retained inactive/guard-skipped controls.

Macro metadata uses still follow lexical include traversal, independently of
the previously observed queue's stable file-local offset ordering. The final
synthetic METHOD record is subject to c2cpg's source-pass then `.h`-pass merge:
a generated header-pass record wins a duplicate full name, with first
registration within each class. CODE and source/parent ownership are selected
together. The repeated N fixture therefore retains one caller-owned macro
method and one header-owned method; merely defining a macro in a header does
not make its method header-owned. The complete repeated fixture still retains
the earlier no-eligible-event/no-wrapper limitation. This is a bounded observed
rule, not full MacroHandler, include-resolution or source-coordinate parity.

### Macro spelling in object array dimensions

The [complete array-dimension controls](tests/fixtures/array-dimension-macros/README.md)
pin a distinction in TYPE_FULL_NAME: a bare macro dimension such as `N`
expands to `3`, while compound dimensions `N+1`, `(N+1)` and `+N` retain
their source spelling. Their numeric partner methods remain exact. This is
an observed type-spelling rule, not general constant folding. Restricting
expansion to a bare identifier restores the three complete controls and the
96 matching zlib records lost by the held tenth candidate.

### Primitive MEMBER declaration spelling

The [complete primitive MEMBER fixtures](tests/fixtures/primitive-members/README.md)
pin declaration-role spelling for field bases: `unsigned short` and `short
unsigned` become `shortunsigned`, while an explicit `int` yields `short unsigned
int`. `signed char` becomes `signedchar`; `unsigned char` keeps its space.
Base `const` is absent from the type, base `volatile` remains, and pointer
qualifiers remain in CODE without changing the pointer type suffix. Each member
of a mixed declaration keeps its own declarator CODE and suffix.

The measured change uses the existing declaration renderer only for field
bases. Enum members, named-type fallback, member suffixes and initializer
construction retain their existing paths. Complete nonprimitive and older
`member_types` diagnostics remain recorded separately from the 18 complete
graph gates, including two retained fixedtables anchors.

### Duplicate function identities and stored metadata

The eight [complete function-identity controls](tests/function_identities.rs)
pin c2cpg's duplicate pass: definitions sort by filename, line and column;
the first full name stays unchanged and later definitions receive
`<duplicate>0`, `<duplicate>1`, and so on. For a duplicate carrying a literal
STATIC modifier, matching CALL NAME and METHOD_FULL_NAME change in its file,
except that the first definition's entire file is excluded. An inherited
static prototype, an `inline static` prefix and a macro spelling expanding to
static do not produce that modifier in these controls. METHOD_REF spelling
and semantic REF targets retain the original name; their physical source
lines still belong to their own originating definitions.

Separate complete saved-CPG observations pin BINDING NAME, METHOD_FULL_NAME,
optional SIGNATURE and distinct BINDS/REF endpoints. The retained array-member
initializer binds from the file-global TYPE_DECL and has no binding signature.
Its modifiers are CONSTRUCTOR/ORDER 2 and STATIC/ORDER 3, both without locations.
Ordinary literal STATIC modifiers retain the observed definition lines.
CPG2 version 2 stores these properties; retained version 1 graphs reopen with
the new modifier property absent.

The canonical oracle omits BINDING properties and MODIFIER_TYPE. The complete
supplemental observations retain IMPORT/DEPENDENCY omissions, unstored columns
and end locations, property-presence limitations, and edge differences.
Canonical equality therefore does not establish complete schema parity.
