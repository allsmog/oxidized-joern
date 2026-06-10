# PROGRESS — 1:1 Joern port (see GOAL.md for the rules)

Single source of truth across sessions. Update in the same commit as the work.

## Current state

- **Milestone:** M4 — edge parity beyond AST (CFG, REF, CALL, ARGUMENT,
  EVAL_TYPE, CONTAINS, SOURCE_FILE)
- **Oracle:** Joern v4.0.555 (`setup-oracle.sh` fetches latest; if the version
  drifts and output changes, record it here and in QUIRKS.md)
- **Gate:** `joern-parity/check.sh` — green, 57/57 blocks byte-identical:
  13 user methods + pair.<clinit> + 9 file-globals + <includes>:<global> +
  32 operator stubs + the scaffolding-nodes section (FILE, NAMESPACE_BLOCK,
  NAMESPACE, META_DATA, TYPE_DECL incl. IS_EXTERNAL entries, TYPE). Corpus:
  add.c, ops.c, loop.c, unary.c, forloop.c, switch.c, exprs.c, structs.c,
  order.c

## Next task (start here)

M2, in this order — one corpus file + diff-to-zero per line:

- [x] unary operators (corpus/unary.c)
- [x] postfix/prefix inc/dec (corpus/unary.c)
- [x] pointer ops: indirection/addressOf, pointer decl/param types (corpus/unary.c)
- [x] `for` loop (corpus/forloop.c)
- [x] `do`-while (corpus/forloop.c)
- [x] ternary `?:` → `<operator>.conditional` (corpus/forloop.c)
- [x] `switch`/`case`/`break`/`continue` (corpus/switch.c)
- [x] compound assignment ops, incl. plural `<operators>.` quirk (corpus/exprs.c)
- [x] casts, sizeof, comma operator (corpus/exprs.c)
- [x] string/char/float literals (corpus/exprs.c)
- [x] arrays → `<operator>.indirectIndexAccess`, NOT indexAccess (corpus/structs.c)
- [x] structs: fieldAccess / indirectFieldAccess (corpus/structs.c)
- [x] multiple declarators, globals (phantom ORDER=0 LOCALs), prototypes
  (corpus/structs.c)

**M2 COMPLETE. M3 COMPLETE.** M4 next — edge parity:

1. Extend `oracle.sc` with an `EDGES|` section. Suggested canonical form: for
   each method (sorted by FULL_NAME), dump CFG edges as
   `EDGES|CFG <methodFullName> <srcLabel>:<srcCode>@<order-path> -> <dst...>`
   or simpler: assign each AST node its dump line index within the method
   block and print edges as index pairs — deterministic on both sides since
   the AST dumps are already byte-identical.
2. Drive edge kinds to zero one at a time, in this order: CONTAINS,
   SOURCE_FILE, REF (locals/params), ARGUMENT, CALL, EVAL_TYPE, then CFG
   (port Joern's CfgCreationPass semantics: statement chaining, short-circuit
   &&/||, loop back-edges, switch fallthrough, break/continue targets).
3. New corpus cases as needed: short-circuit conditions, nested loops with
   break/continue, goto/label (also closes the M2 goto gap).

## Done

- **M3 part 2 (2026-06-10):** Non-method scaffolding parity + new pins, 57/57.
  NODES| oracle section (META_DATA, FILE incl. <includes>/<unknown>,
  NAMESPACE_BLOCK, NAMESPACE, TYPE_DECL — internal structs with EMPTY
  AST_PARENT_* strings, per-method TYPE_DECLs parented TYPE_DECL-><file
  global>, per-file <global> ones, IS_EXTERNAL=true entries under
  <includes>:<global> with no ORDER — and TYPE, exactly the set of
  TYPE_FULL_NAME strings emitted anywhere). order.c pinned: struct between
  functions (source-order slots), global with initialiser (LOCAL + void
  assignment in the global BLOCK, plain `g` CODE there vs `<global> g` inside
  methods), MEMBER CODE = declarator text (`*ptr`, `arr[4]`), sized arrays
  type as `int[4]`, and the `<clinit>` synthetic method
  (pair.<clinit>:pair(): property-less BLOCK + <operator>.arrayInitializer
  per sized member + two bare MODIFIERs + RET typed as the struct).
- **M3 part 1 (2026-06-10):** Method-set scaffolding parity, 51/51. Emitter
  restructured: multi-file invocation, per-method dump buffers sorted by
  FULL_NAME, project-wide stub tracking (called-but-undefined, max arity),
  file-global wrapper + TYPE_DECL/MEMBER emitters. check.sh keys blocks by
  FULL_NAME and runs the binary once over corpus/*.c.
- **M2 (2026-06-10):** Full C statement/expression AST coverage, 11/11 methods
  byte-identical. Corpus: switch.c (switch/JUMP_TARGET flattening,
  break/continue), exprs.c (compound assigns, cast/TYPE_REF, sizeof + phantom
  type LOCAL, comma→BLOCK, literal typing), structs.c (field access pair,
  indirectIndexAccess, struct-tag-stripped types, `int[]` arrays, multi
  declarators, `<global>` identifier CODE + phantom global LOCALs, prototypes).
- **M1 (2026-06-10):** Parity harness built; pure-Rust tree-sitter C frontend
  byte-identical to Joern on toy corpus (functions, params, locals, nested
  calls, +,-,*,<,>,= operators, if/else, while). Crate:
  `joern-parity/`. Key files: `oracle.sc` (Joern-side dump), `src/main.rs`
  (Rust frontend + canonical dump), `check.sh` (per-method differ).

## Stuck / deferred

(none)

## Architecture notes for future sessions

- Frontend strategy is **pure Rust** (tree-sitter), decided early; do not
  re-litigate. Joern's runtime is used only as the test oracle.
- Canonical dump format: `LABEL k=v ...` with keys in the fixed order NAME,
  CODE, TYPE_FULL_NAME, FULL_NAME, METHOD_FULL_NAME, SIGNATURE, ORDER,
  ARGUMENT_INDEX, DISPATCH_TYPE; children sorted by ORDER; newlines in CODE
  escaped as `\n`; methods sorted by FULL_NAME; blank line between methods.
- M6 will fold this onto `cpg-core`'s graph schema — until then the dumper is
  deliberately standalone to keep the parity loop fast.
