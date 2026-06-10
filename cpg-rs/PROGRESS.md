# PROGRESS — 1:1 Joern port (see GOAL.md for the rules)

Single source of truth across sessions. Update in the same commit as the work.

## Current state

- **Milestone:** M3 — full node-set parity (scaffolding: <global> methods,
  <operator> stubs, TYPE_DECL/TYPE/NAMESPACE_BLOCK/FILE/META_DATA, METHOD_REF)
- **Oracle:** Joern v4.0.555 (`setup-oracle.sh` fetches latest; if the version
  drifts and output changes, record it here and in QUIRKS.md)
- **Gate:** `joern-parity/check.sh` — green, 51/51 methods byte-identical
  (11 user methods + 8 file-globals + <includes>:<global> + 31 operator
  stubs; corpus: add.c, ops.c, loop.c, unary.c, forloop.c, switch.c,
  exprs.c, structs.c)

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

**M2 COMPLETE.** M3 progress:

- [x] Widened `oracle.sc` (all methods, scaffolding included); 51/51 green:
  `<includes>:<global>`, per-file `<global>` wrappers (nested METHOD dumps,
  TYPE_REF/LOCAL/METHOD_REF slot BLOCK), all 31 `<operator>`/`<operators>`
  stub methods (arity = max use; stable-sort child layout).
- [x] TYPE_DECL + MEMBER under the file-global (struct point pinned).
- [x] METHOD_REF bindings under the file-global BLOCK.
- [ ] Non-method scaffolding nodes: NAMESPACE_BLOCK / FILE / META_DATA / TYPE
  nodes are NOT covered by the method-walk oracle. Add a second dump section
  to oracle.sc (e.g. `NODES|` lines for cpg.file, cpg.namespaceBlock,
  cpg.typeDecl not under a method, cpg.typ, cpg.metaData with
  AST_PARENT_TYPE/AST_PARENT_FULL_NAME/FILENAME keys), extend check.sh to
  diff that section, then emit it from the Rust side.
- [ ] Pin underdetermined cases while here: TYPE_DECL ORDER with a struct
  declared *between* functions; multi-declarator globals; global with
  initialiser (`int g = 5;`); struct member pointers/arrays.

## Done

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
