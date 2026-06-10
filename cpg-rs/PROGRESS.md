# PROGRESS — 1:1 Joern port (see GOAL.md for the rules)

Single source of truth across sessions. Update in the same commit as the work.

## Current state

- **Milestone:** M5 (real-world corpus) / M7 (dataflow oracle) — M1-M4 done
- **Oracle:** Joern v4.0.555 (`setup-oracle.sh` fetches latest; if the version
  drifts and output changes, record it here and in QUIRKS.md)
- **Gate:** `joern-parity/check.sh` — green, 76/76 blocks byte-identical:
  57 method blocks + scaffolding nodes + 15 edge kinds incl. CFG (~2,300
  edges; corpus now includes logic.c: short-circuit &&/||, if-without-else,
  nested loops with break/continue, switch fallthrough + no-default).
  Previously:
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

**M2, M3, M4 COMPLETE** (AST + node set + structural edges + CFG).

Next, in preference order:

1. **M5 — real-world corpus.** Vendor a small real C project (suggest: a
   single-file lib like miniz/stb subset, or zlib's adler32.c+crc32.c to
   start), run both sides, triage every diff into either a minimal new
   corpus pin + fix, or a QUIRKS.md entry. This will surface the long tail:
   typedefs, enums, unions, static/extern, multi-dim arrays, function
   pointers, varargs, preprocessor output constructs, goto/label (KNOWN GAP:
   CFG builder has a goto/label TODO; AST side also unpinned).
2. **M7 — dataflow oracle.** Add REACHING_DEF, CDG, DOMINATE, POST_DOMINATE
   to oracle kinds (they're already in the importCode graph; the addressing
   scheme works as-is). DOMINATE/POST_DOMINATE are computable from our
   now-identical CFG (standard dominator tree); CDG from post-dominance
   frontiers; REACHING_DEF is the big one (Joern's ReachingDefPass: def-use
   over the CFG with its specific gen/kill conventions).
3. **M6 — real graph output** (fold onto cpg-core schema + binary export
   loadable by joern) can proceed in parallel with either.

## Done

- **M4 part 2 (2026-06-10):** CFG parity, 76/76 blocks, 340+ CFG edges on the
  base corpus plus logic.c pins — all matched. CfgCreationPass semantics
  reconstructed from the oracle and implemented as a generic builder over our
  own dump blocks (shared line addresses): evaluation-order chaining (args
  then call), statement BLOCKs transparent vs comma BLOCKs (CALL children) as
  CFG nodes, LOCALs/params/MODIFIERs invisible, stub bodies skipped
  (METHOD->METHOD_RETURN direct), condition roots branch to both arms, loop
  back-edges to the condition's first leaf, do-while entry at body, for:
  init->cond->body->update->cond with continue->update, switch: cond root ->
  every JUMP_TARGET (+ continuation when no default), case values chained
  after their JUMP_TARGETs, natural fallthrough, break -> after construct,
  ternary + short-circuit &&/|| branch shapes. logic.c passed first-run —
  the semantics model is predictive now.
- **M4 part 1 (2026-06-10):** Structural edge parity, 71/71. EDGES| oracle
  section with deterministic addressing: every node = <homeMethod>#<dumpLineIdx>;
  METHOD/TYPE_DECL/MEMBER addresses resolve first-wins across sorted method
  walks (so `main` = add.c:<global>#12 but `add` = add#0 — pinned oracle
  behaviour); T:/F:/NB:/NS:/D: prefixes for non-walk nodes. Rust line-writer
  instrumented to emit ARGUMENT (CALL/RETURN -> children), CALL (-> callee
  incl. stubs), EVAL_TYPE (exactly the TYPE_FULL_NAME emissions), CONTAINS
  (ContainsEdgePass destination list — LOCALs/params/MODIFIER/MEMBER excluded;
  sources METHOD/TYPE_DECL/FILE), REF (identifiers -> phantom locals/params;
  METHOD_REFs -> methods; fieldAccess -> MEMBER for value receivers only —
  p->y stays unresolved, a CDT quirk; TYPE -> TYPE_DECL; NB -> NS),
  PARAMETER_LINK, SOURCE_FILE (methods, TYPE_DECL population, NBs),
  CONDITION/TRUE_BODY/FALSE_BODY/DO_BODY/FOR_INIT/FOR_BODY/FOR_UPDATE
  (while/switch bodies are TRUE_BODY; FOR_INIT targets the init assignment).
  Stubs/<includes>:<global> now emitted through the instrumented writer.
  check.sh diffs each edge kind as its own block.
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
