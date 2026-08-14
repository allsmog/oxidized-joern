# PROGRESS — 1:1 Joern port (see GOAL.md for the rules)

Single source of truth across iterations. Update in the same commit as the work.

## Current state

- **Native query and Flatgraph replacement track started (2026-08-14).**
  `cpg query` now compiles a committed 64-case CPGQL tier to native Rust
  logical plans, with a 27-case zero-diff gate against Joern v4.0.555,
  including path-producing `reachableByFlows`.
  `cpg import-joern` decodes Joern v4 Flatgraph and `cpg export-joern` emits
  it; the pinned gate passes C, Java, and Python fixtures in both directions,
  including content-exact Joern-to-CPG2-to-Joern round-trip digests. Full
  CPGQL, wider C semantics, rules, and per-language promotions remain open in
  `REPLACEMENT_CONTRACT.md`.
- **Production C convergence complete (2026-08-14).** The released
  `CFrontend`/`Project`/`standard_pipeline` path is the 107/107 committed Joern
  v4.0.555 oracle path, including 1,545/1,545 ReachingDef facts. Canonical
  scanner outcomes cover branches, kills, loops, returns, globals,
  pointer/member access, sanitizers, cross-calls, recursion, persistence, and
  duplicate translation-unit-local identities. Pinned zlib 1.3.1 and Lua 5.4.7
  builds gate deterministic graph/edge/export/SARIF output, resource budgets,
  and incremental-vs-clean equivalence. See `COMPATIBILITY.md` for the release
  boundary.
- **All-language acceptance complete (2026-08-14).** C, C++, Go, Java,
  JavaScript, TypeScript, Python, Ruby, Rust, and Scala pass the shared schema,
  interprocedural summary/taint, and save/load contract. Only C is promoted to
  production preview; the remaining frontends are explicitly experimental.

## Historical milestone log

- **M6/Gap 1 partial (2026-07-09): the engine's CFG + DDG passes are real.**
  `cpg-analysis/src/cfg.rs` now ports the parity-validated CfgBuilder
  semantics (evaluation-order chaining, if/else + loop back-edges +
  break/continue + switch dispatch/fallthrough + short-circuit shapes) onto
  the cpg-core graph; the C frontend emits a canonical control-structure
  shape (cond first, arms as Block wrappers, four positional for-clause
  Blocks) to carry the roles Joern encodes with condition/order edges. New
  `cpg-analysis/src/reaching_def.rs` ports `reaching_def_flows` (gen/kill
  fixpoint over the CFG, ReachingDefFlowGraph first-body-node quirk, lone
  identifiers, DefaultSemantics operator table with token normalisation,
  EdgeValidator + UsageAnalyzer.isUsing) and writes `EdgeKind::ReachingDef`
  edges; registered in `standard_pipeline()` with Ast+Cfg reads / Ddg write
  so incremental re-runs clear + recompute per file. Divergences (no
  METHOD_PARAMETER_OUT/JUMP_TARGET/TYPE_REF/INLINED, no `<global>` capture)
  are documented in the two modules' headers. joern-parity untouched, gate
  still 95/95. Follow-up: point summaries.rs/taint.rs at ReachingDef edges.
- **Milestone:** M5 (real-world corpus) + M7 (dataflow REACHING_DEF — **DONE,
  byte-zero**) — M1-M4 done.
  Goal reframed (see GOAL.md): self-hosted IRIS on cpg-rs.
  Two parallel tracks: A = byte-parity expansion (gate), B = dataflow + IRIS loop.
- **M7/Track B (2026-06-10): REACHING_DEF byte-parity COMPLETE — all 1,458
  FLOWS facts byte-identical to Joern v4.0.555, AST/NODES/EDGES unchanged.**
  The reaching-def engine in `reaching_def_flows` is now a verbatim port of the
  decompiled v4.0.555 internals. Load-bearing facts pinned by the byte-parity
  investigation
  (progression 31→19→11→10→9→7→4→2→1→0):
  - `MemberAccess.isFieldAccess` (the GEN exclusion in `initGen`) is BROAD:
    memberAccess + all variants + `indirection` + `getElementPtr` + `sizeOf`.
    So indirection calls have **empty gen** (a def only via their parent).
  - `MemberAccess.isGenericMemberAccessName` (the KILL skip in `initKill`):
    same set but with `addressOf`+`pointerShift`, no `sizeOf`. `&v` gens a v
    def but kills no prior v def.
  - `UsageAnalyzer.isUsing` = sameVariable ∥ isContainer ∥ isPart ∥ isAlias,
    all via `nodeToString` (NAME for ident/param, CODE for expr) with
    `Option.contains` = **exact** equality (not substring). isAlias =
    same-access-path expressions (`*l`~`*l`).
  - `OptimizedReachingDefTransferFunction.loneIdentifiers`: an identifier arg
    that is not a param/local, not used in a return, and unique by name (e.g.
    `int[2][3]` operand of `alloc`) is removed from gen AND gets a direct
    lone→exit edge (`addEdgesFromLoneIdentifiersToExit`).
  - `addEdgesToMethodParameterOut` filters in(paramOut) by isUsing, so `*l`
    (indirection of l) reaches paramOut l.
  - `addEdgeForBlock`: a comma-operator/expression BLOCK argument (a CFG node,
    unlike statement/INLINED/stub blocks) routes its last child into the call;
    BLOCK is an `isDdgNode`. Empty block `code` renders `<empty>`.
  - exit/exit_in must be the method's OWN METHOD_RETURN (the `<global>` dump
    embeds nested methods whose returns precede it).
  - `addEdgesToCapturedIdentifiersAndParameters`: a `<global>` identifier (a
    global) links to its first same-name usage in each method-ref-captured
    method (`g` → `first#8`) — the only interprocedural edge.
  - Tooling: `rd_probe2.sc`/`rd_probe3.sc` dump Joern's actual gen/in/lone for
    ground truth; Ddg, EV2, UA, RDTF, OPT, and MemberAccess were decompiled via
    CFR from the pinned artifacts.
- **Oracle:** Joern v4.0.555 (`setup-oracle.sh` now pins v4.0.555 by default;
  override with `JOERN_VERSION=vX.Y.Z` only as a deliberate, recorded upgrade)
- **Gate:** `joern-parity/check.sh` — green, 107/107 blocks byte-identical.
  The newest fixtures cover source-ordered `#if/#elif`, `defined`, undefined
  identifiers, function-macro conditions, inactive-branch exclusion, nested
  and variadic expansion, stringizing, token pasting, zero-arg external stubs,
  and `(void)` parameter lowering.
  **(2026-07-10: check.sh now diffs the FLOWS section — the 1,458 REACHING_DEF
  facts are a guarded block, and oracle regen preserves FLOWS lines; the M7(a)
  "extend check.sh with a FLOWS diff block" task is done. Previously 95/95 with
  FLOWS informational-only.)**
  Corpus includes THREE unmodified real-world musl files: bsearch.c,
  memcmp.c, strcmp.c — AST, nodes, and all 15 edge kinds incl. CFG. Previously 88/88 with
  macros.c (#define expansion as INLINED calls + #ifdef). Previously 84/84 with gotos.c
  and types2.c (goto/label, typedef, enum + <clinit>, union, static,
  function pointers/pointerCall, multi-dim arrays/alloc).
  Previously 76/76 with logic.c (short-circuit &&/||, if-without-else,
  nested break/continue, switch fallthrough). Previously:
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

1. **M5 — real-world corpus, continued.** Three musl files GREEN
   (bsearch.c, memcmp.c, strcmp.c). Next: a macro-heavy real file (zlib
   adler32.c) to stress nested macro expansion and #if evaluation. Still
   `#if/#elif` expression evaluation is now pinned. Still unpinned: nested
   macro expansion, token pasting/stringizing, varargs, extern, calls to undefined
   functions (printf stub shape), initializer lists `{1,2}`, struct defs
   inside functions, braceless if/while bodies (for-with-; is pinned).
2. **M7 / Track B — dataflow + IRIS (STARTED).** FLOWS| oracle section now
   live (REACHING_DEF with VARIABLE property); addressing confirmed to carry
   over (1458 flow facts on the corpus). NEXT, in order:
   (a) Implement reaching-definitions in joern-parity to byte-match the
       FLOWS| section — reuse the CfgBuilder's CFG (already correct) + Joern's
       ReachingDefPass gen/kill. Observed conventions to reproduce (from
       add.c): params flow from METHOD entry (`add#0 -> add#1`, var `[]` or
       name); a def reaches uses tagged with the VARIABLE (`[a]`, `[a + b]`);
       `<RET>` variable flows the returned expr -> METHOD_RETURN
       (`add#6 -> add#10`); interprocedural flows go file-global#callsite ->
       callee param-use. Extend check.sh with a FLOWS diff block (per-method,
       like edges).
   (b) reachableBy = transitive closure over REACHING_DEF (+ summaries for
       interproc). Validate against a Joern reachableBy probe.
   (c) Port validated CFG + reaching-defs from joern-parity into cpg-core
       passes (replace the placeholder linear CfgPass in cpg-analysis/cfg.rs;
       populate the existing-but-empty Ddg edge slot). Wire taint.rs onto it.
   (d) IRIS driver on cpg-cli: LLM CWE->sources/sinks/sanitizers (extend
       TaintSpec), run taint, LLM triage, SARIF out.
   (e) Evaluate on Juliet C/C++ subset: precision/recall vs engine-alone.
3. **M6 — real graph output** (fold onto cpg-core schema + binary export
   loadable by joern) can proceed in parallel with either.

## Done

- **M7/Track B — flow-graph routing partially implemented, 82->65 (2026-06-10):**
  Implemented the exit-routing fix (exit/param_out in-set = out(lastActualCfgNode)
  = earliest cfg-pred of METHOD_RETURN, not the union of all returns). Dropped
  the bypass-return param defs: 82->65, no regressions. REMAINING ~30 (bsearch
  /strcmp/memcmp): branch-defs (e.g. `nel/=2`, a unique-var call never killed)
  reach the exit via the loop back-edge in a raw-CFG fixpoint, but Joern's probe
  shows in(return-try) excludes them. So the body-internal liveness ALSO needs
  Joern's ReachingDefFlowGraph, not just the exit routing. FINISH = build the
  full ReachingDefFlowGraph (decompiled initPred/initSucc: param chain
  method->p1..->body; body cfg; returns/lastNodeOfBody->paramOut chain->exit;
  param-in killed by identifier-uses) and run the fixpoint over it, validating
  every node's in/out against rd_solver_probe.sc. Plus the ~14 residual access
  cases (field/index substring is in; indirection `*l` def-use still needs the
  exact isUsing). Tooling ready: rd_solver_probe.sc dumps Joern's per-node
  in/out; the required classes can be re-extracted from the pinned artifact.
- **M7/Track B — loop-liveness ROOT CAUSE found (2026-06-10):** The blocker is
  NOT irreducible. By scripting Joern (rd_solver_probe.sc — constructs
  ReachingDefProblem + DataFlowSolver and dumps in/out per node) I obtained the
  ground-truth solver state for bsearch. ROOT CAUSE: Joern's reaching-def runs
  over ReachingDefFlowGraph, NOT the raw CFG. Two structural differences:
  (1) params are a chain method->p1->..->pN->body (and each param is killed by
  its own identifier-use, e.g. param nel killed by `nel` in `nel>0`);
  (2) the EXIT's predecessor chain is return->paramOut_1->..->paramOut_N->exit,
  and ONLY the loop-internal `return try` (the lastActualCfgNode) feeds it — the
  post-loop `return NULL` does NOT connect to the exit in the flow graph. So the
  zero-iteration bypass param defs that a raw-CFG fixpoint propagates to the exit
  simply don't exist in Joern's graph. THE FIX (implementable, validated by the
  probe): build ReachingDefFlowGraph (param chain + paramOut chain + return
  routing per the decompiled initPred/initSucc) and run the fixpoint over it
  instead of cfg_index_edges. Captured in(exit) for bsearch =
  {5,8,13,14,15,16,18,19,20,21,24,25,26,27,30,31,33} (flow-graph def numbers;
  rerun the probe for the mapping). This + the residual access-substring cases
  (~38, gated to field/index uses) closes the remaining 82. The decompiled
  classes are reproducible with CFR from the dataflowengineoss jar.
- **M7/Track B faithful isValidEdge + liveness wall (2026-06-10):** Added the
  decompiled v4.0.555 EdgeValidator.isValidEdge as a push-filter on every edge
  (rd_valid_edge in main.rs): 94->85. Cast operands generate defs (pinned: `l`
  in `(uc*)l` reaches exit) while indirection/addressOf operands don't: 107->94.
  Field/index-access substring isUsing (`p->y` uses `p`): 85->82. 11 methods
  byte-exact. Flow diff now 82/1458 (~94%). Track A green.
  IRREDUCIBLE REMAINDER (~44 of 82): loop-exit liveness in bsearch/strcmp/
  memcmp. CHARACTERIZED precisely: Joern's def live at METHOD_RETURN for each
  variable is its FIRST-loop-body use (base@25, width@27, nel@29 — the loop's
  first statement), NEVER the parameter nor the branch reassignments (53,42,…).
  Standard reaching-def over the byte-identical CFG keeps param + branch defs
  live on the loop-bypass / last-iteration-exit path, so our fixpoint emits
  them; Joern's DataFlowSolver merge kills them. This is a non-standard solver
  behaviour not derivable from the oracle — needs replicating Joern's actual
  DataFlowSolver worklist/merge from the pinned class artifact. The
  remaining ~38 are residual access def-use (agg q.x container, macros). NOTE:
  plain/indirection-gated substring isUsing OVER-produces strcmp `*l` uses
  without an even more exact isValidEdge — field/index gating is the safe subset.
- **M7/Track B breakthrough — semantics-gated edges (2026-06-10):** Decompiled
  the v4.0.555 dataflow classes with CFR and found
  the REAL mechanism: every REACHING_DEF edge is gated by EdgeValidator.
  isValidEdge using Joern's DefaultSemantics flow mappings. Extracted the exact
  operator flow table (operator_semantics() in main.rs, verbatim from
  DefaultSemantics.operatorFlows). Rules now PRINCIPLED, not heuristic:
  arg->call always valid; arg->sibling-arg valid iff (srcIdx->dstIdx) is in the
  operator's flow mappings (pass-through when the operator has no semantics);
  write-only args (defined-not-used, e.g. plain-= LHS) derived from semantics.
  This resolved the long-standing mystery (addition has semantics -> no
  arg->arg cross; subtraction/comparison have none -> cross allowed; assignment
  (2->1) -> RHS->LHS only). Flow diff 165 -> 111; 10 methods byte-exact (add,
  main, classify, pick, unary, sum, logic, gotos, helper, order). Track A green.
  REMAINING ~111: pointer-code loop-exit liveness (which l/r defs are live at
  METHOD_RETURN after a loop) and isUsing's access-path container/part/alias
  (for fieldAccess `q.x`/indexAccess `vals[0]`/indirection `*l` def-use across
  statements). The decompiled isUsing (sameVariable via `contains`, isContainer,
  isPart, isAlias over access paths) is captured in the decompiled Ddg
  implementation; the
  fieldAccess gen nuance (fieldAccess calls excluded from defsForCalls but
  included as Call-args in the parent's gen) is the agg/struct gap. These are a
  faithful-port finish, not more heuristics.
- **M7/Track B reaching-def engine (2026-06-10):** Built a full reaching-def
  engine in joern-parity (parse_dump_block -> index CFG -> GEN/KILL fixpoint ->
  DdgGenerator edge routines) emitting the FLOWS| section. Drove the diff vs
  the 1458-fact oracle from 904 -> 165 (~1384/1458 = 95% of flows correct);
  7 methods byte-exact (add, main, classify, pick, unary, gotos, helper) and
  all simple/medium control flow. Rules pinned: GEN at params + non-access
  calls ({call} u Call/Identifier args), access-like calls (indirection/
  addressOf/cast) gen only {call}, KILL on reassignment, per-method own-node
  restriction, entry edges (excl. assignment LHS), arg->call, literal->sibling
  and call-arg/condition cross-arg, isUsing access-path strip (`*l`->`l`),
  return/param-out/exit routines.
  HONEST STATUS — NOT byte-matched. The remaining ~165 are in pointer-
  arithmetic-in-loops (bsearch/memcmp/strcmp), casts, and macros. Empirical
  local rules PLATEAU here: e.g. classify `n-1` has a literal->ident cross-edge
  but sum `i+1` does not, with no local discriminator — these edges are
  EMERGENT from Joern's exact dataflow fixpoint + UsageAnalyzer.isUsing
  (container/part/alias over access paths), not from construction heuristics.
  NEXT (the right approach, not more rules): port the ACTUAL algorithm faithfully
  — ReachingDefProblem (Definition numbering, gen/kill), the DataFlowSolver
  worklist, and UsageAnalyzer.isUsing with real access-path matching — from
  joernio/joern dataflowengineoss, validated against FLOWS|. Source skew note:
  master DdgGenerator differs from v4.0.555 in places; trust the oracle.
  check.sh still gates Track A only (FLOWS is informational until matched);
  Track A stays green 95/95.
- **M7/Track B start (2026-06-10):** Reframed toward self-hosted IRIS on
  cpg-rs (see plan). Added the FLOWS| oracle section to oracle.sc:
  REACHING_DEF edges dumped with their VARIABLE property via the existing
  `#`-addressing, which resolves them with ZERO new addressing work (the
  plan's flagged risk is retired). 1458 flow facts captured on the corpus;
  Track A byte-parity gate stays green at 95/95 (FLOWS is additive — check.sh
  only diffs AST/NODES/EDGES). This is the "get the oracle first" foundation;
  reaching-def implementation in joern-parity is the next unit.
- **M5 musl string fns (2026-06-10):** memcmp.c + strcmp.c byte-identical,
  95/95. New pins: multi-declarator initialisers emit ALL LOCALs first, then
  all assignments (`T *l=vl, *r=vr;`); empty for-init clause becomes a
  CODE-less ANY BLOCK placeholder that still receives the FOR_INIT edge;
  comma updates are BLOCKs, so for-clause classification is positional
  ([init, cond, update, body?] after skipping LOCALs); body-less
  `for (...);` loops branch cond -> update entry directly; `unsigned char`
  keeps its space in types (vs longunsigned) and declarations register the
  decl-SPECIFIER type separately — `unsigned char c` also registers bare
  `unsigned`, a pointer decl registers its base (CDT's typeForDeclSpecifier
  divergence).
- **M5 first real file (2026-06-10):** musl bsearch.c byte-identical, 91/91.
  New pins from real code: pointer return types (`void *bsearch` ->
  SIGNATURE void*(...), ret from pointer levels above the function
  declarator); NULL (tree-sitter `null` node) -> IDENTIFIER with CODE
  `<unknown> NULL` + phantom ORDER=0 LOCAL (general rule: fully unresolved
  identifiers phantom with `<unknown>` CODE); cast quirk — `(char *)x` types
  as the BASE type `char` only while TYPE_REF CODE keeps `char *`; else-if
  wraps the nested if in a synthetic CODE-less ANY BLOCK; #include
  directives become IMPORT slots so the file-global TYPE_DECL's ORDER is
  1 + #includes; phantom scanning is preprocessor-aware (directive names and
  dropped #ifdef branches contribute nothing).
- **M5 preprocessor (2026-06-10):** In-file macro parity, 88/88. CDT model
  pinned by corpus/macros.c: an invocation is a CALL with DISPATCH=INLINED,
  NAME = macro, CODE = original invocation text, MFN/SIGNATURE =
  <file>:<name>:<retType>(<nparams>) where retType is the expansion root's
  type; arguments first (ORDER/INDEX 1..n), then an ANY BLOCK (ORDER/INDEX
  n+1) wrapping the expansion parsed from the parameter-substituted body
  (whole-word textual substitution, re-parsed with tree-sitter). Each USED
  macro also becomes a METHOD whose CODE is the #define directive (params
  p1..pn, empty ANY BLOCK, RET typed as expansion) — unused macros get
  nothing. #ifdef/#ifndef evaluated against the macro table, guarded
  statements spliced or dropped. Quirks: INLINED call arguments carry NO REF
  edge (expansion identifiers do); the expansion BLOCK gets no ARGUMENT edge
  despite its index, and is CFG-invisible; CFG threads args -> call ->
  expansion content with BOTH the call and the expansion exit flowing onward;
  macro methods get SOURCE_FILE + CONTAINS from the file-global TYPE_DECL
  but no TYPE_DECL of their own.
- **M5 prep (2026-06-10):** Long-tail language pins, 84/84. gotos.c: labels
  flatten like switch cases (JUMP_TARGET CODE = whole labeled stmt, then the
  stmt as sibling), goto = childless CONTROL_STRUCTURE with CFG edge to its
  JUMP_TARGET. types2.c: typedef -> TYPE_DECL inside the global BLOCK (CODE
  incl. semicolon) and its UNDERLYING type registers as a used type with raw
  spelling (`unsigned int`, NOT normalised); enum -> TYPE_DECL with ANY-typed
  MEMBERs (CODE `GREEN = 5`) + <clinit> holding phantom enumerator LOCALs and
  void assignments; union types render CONCATENATED (`unionvalue`) so the
  use-type is external while the TYPE_DECL `value` is internal; enumerator
  refs get plain-CODE ANY phantoms; fn-pointer params type as just the return
  type; calls through pointer symbols -> <operator>.pointerCall with
  DYNAMIC_DISPATCH (receiver ORDER=1 no ARGUMENT_INDEX, args shifted; NO CALL
  edge but a stub exists with arity = indexed args); `int grid[2][3];` lowers
  to <operator>.alloc(int[2][3], 2, 3) with the type as an IDENTIFIER;
  ARGUMENT edges refined to indexed children only. decl_suffix fixed for
  multi-dim source order.
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

## Architecture notes for future work

- Frontend strategy is **pure Rust** (tree-sitter), decided early; do not
  re-litigate. Joern's runtime is used only as the test oracle.
- Canonical dump format: `LABEL k=v ...` with keys in the fixed order NAME,
  CODE, TYPE_FULL_NAME, FULL_NAME, METHOD_FULL_NAME, SIGNATURE, ORDER,
  ARGUMENT_INDEX, DISPATCH_TYPE; children sorted by ORDER; newlines in CODE
  escaped as `\n`; methods sorted by FULL_NAME; blank line between methods.
- M6 will fold this onto `cpg-core`'s graph schema — until then the dumper is
  deliberately standalone to keep the parity loop fast.
