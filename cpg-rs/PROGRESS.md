# PROGRESS — 1:1 Joern port (see GOAL.md for the rules)

Single source of truth across sessions. Update in the same commit as the work.

## Current state

- **Milestone:** M2 — full C statement/expression coverage (AST layer)
- **Oracle:** Joern v4.0.555 (`setup-oracle.sh` fetches latest; if the version
  drifts and output changes, record it here and in QUIRKS.md)
- **Gate:** `joern-parity/check.sh` — green, 4/4 methods byte-identical
  (corpus: add.c, ops.c, loop.c)

## Next task (start here)

M2, in this order — one corpus file + diff-to-zero per line:

- [ ] unary operators: `-x !x ~x` → `<operator>.minus/.logicalNot/.not`
- [ ] postfix/prefix inc/dec: `x++ x-- ++x --x`
- [ ] pointer ops: `*p` (indirection), `&x` (addressOf), pointer decl types
- [ ] `for` loop (check Joern's CODE convention — `while` was header-only)
- [ ] `do`-while
- [ ] ternary `?:` → `<operator>.conditional`
- [ ] `switch`/`case`/`break`/`continue`
- [ ] compound assignment ops (`+=` etc. — assignment_name() exists, unpinned)
- [ ] casts, sizeof, comma operator
- [ ] string/char/float literals (check TYPE_FULL_NAME conventions)
- [ ] arrays + `<operator>.indexAccess`
- [ ] structs: fieldAccess / indirectFieldAccess, member decl
- [ ] multiple declarators (`int a, b = 1;`), global variables, prototypes

Then mark M2 done here and start M3 (widen oracle.sc: drop the
`filterNot(_.name.startsWith("<"))` filter, add scaffolding nodes).

## Done

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
