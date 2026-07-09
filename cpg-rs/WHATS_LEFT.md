# WHATS_LEFT — the plan from here to "delete the Scala"

Synthesis of four parity-gap investigations (C frontend, dataflow/query,
multi-language, Scala-retirement), each grounded in the repo at HEAD. This is
the sequencing document; `ROADMAP.md` holds the per-gap detail and `PROGRESS.md`
holds session-by-session state.

The user's condition for deleting the Scala is **"once we get those byte
parity."** This document says exactly what "those byte parity" means, what
stands between here and there, and in what order — so the deletion becomes a
mechanical consequence of green gates, not a judgement call.

---

## The one-paragraph answer

The Rust side has **one** thing at true byte-parity: the C frontend's AST +
node set + structural/CFG edges + REACHING_DEF (95/95 blocks, 1,458/1,458 flow
facts vs Joern v4.0.555) — but only inside the **standalone `joern-parity`
dumper**, not the real engine. Everything else — the engine's own C frontend,
all 6 other languages, `reachableBy`, the query/scan surface — is at
*architecture-demo* or *conformance* fidelity, not parity. The critical path is
therefore **not** "write more analyses"; it is **M6 convergence**: fold the
parity-validated frontend onto `cpg-core` so the gate guards the *engine*, not a
sidecar. Until that lands, every downstream parity claim rests on a graph whose
shape diverges from Joern's on any real pointer/struct/goto code. After M6, the
work is a known ladder: `reachableBy` parity → scanner ports → per-language
ladders (Java first) → module-by-module Scala deletion.

---

## Two urgent, cheap fixes (do first — they protect everything)

Both were found independently by two agents; both are near-free and both
currently leave the headline parity number **unguarded**.

1. **The gate silently drops FLOWS on oracle regen.** `check.sh:21` greps only
   `^(AST|NODES|EDGES)\|` when regenerating `oracle_all.txt`, then overwrites the
   file — so the next regen against a live Joern **deletes all 1,458 committed
   FLOWS lines**, and `check.sh` has no FLOWS diff block anyway. The verified
   REACHING_DEF byte-parity is enforced by *nothing* today. Fix: add `FLOWS` to
   the regen grep **and** add a per-method FLOWS diff block (already listed as
   `PROGRESS.md` M7(a), never done). ~hours.
2. **Pin the oracle version in `setup-oracle.sh`.** (Already done on the Gap 8
   branch — `JOERN_VERSION=v4.0.555`.) Confirm it's merged; a `latest` fetch can
   shift the spec mid-work. ~done, verify.

---

## The critical path: M6 convergence (Gap 1)

**Why it dominates.** The DDG algorithm in `cpg-analysis/src/reaching_def.rs` is
a verbatim, byte-validated port — but it runs over `cpg-lang-c` (449 lines),
which lowers only calls, `=`, binary ops, control structures, identifiers and
literals. `*p`, `&x`, `p->f`, `a[i]`, casts, `i++`, ternary, comma, sizeof,
goto/labels, macros, **and `Local` nodes** all fall through the "unknown
wrapper → descend to first child" arm, so `x->field` collapses to `x`. The
validated algorithm is thus running on a graph that diverges from Joern's on
exactly the constructs that matter for security dataflow. `check.sh` diffs the
*dumper*, not this graph, so none of it is caught.

**The work (schema + frontend + gate rewire):**
- Fold `joern-parity/src/main.rs`'s frontend onto `cpg-core::builder`; the
  canonical dump becomes a *serializer over the columnar graph*, not a private
  text emitter. The dumper's `method#lineIdx` addressing must be reproduced from
  graph order (fiddly, but the existing line-by-line gate validates it).
- Schema additions this forces, decided here once: `METHOD_PARAMETER_OUT`,
  `Local`, `JumpTarget`, operator full-names (`<operator>.*`), DISPATCH_TYPE/
  INLINED, internal-vs-external/stub method flags, the `<global>` method.
- **Rewire the gate through the graph-backed path** so a divergence in the
  engine graph fails `check.sh`. This is the step that converts "ported" into
  "guarded."
- CPG binary export (`cpg.bin` / flatgraph zip) loadable by `joern --script` —
  independent correctness check and the Tier-2 interop seam.

**Exit:** `check.sh` (AST/NODES/EDGES/CFG/FLOWS) green *through the graph-backed
dump*, and a Joern script over the exported `.cpg` returns identical query
results on the corpus. **Effort: the single largest item — ~2–3 sessions for the
fold, plus the schema work; call it the multi-week centrepiece.** Nothing
downstream can be parity-validated on real C until this is done.

---

## Track A — finish C so `c2cpg` (for C) is deletable

Depends on M6. From the C-frontend investigation, all genuinely still open
(verified in code, not just PROGRESS):

- **Preprocessor long tail** (the hard part): `#if`/`#elif` expression
  evaluation, nested macro expansion, `##`/`#` paste/stringize, varargs macros.
  Next corpus target: zlib `adler32.c`. Must match CDT's preprocessor
  byte-for-byte. ~1–2 sessions, real risk.
- **Statement/decl long tail** (mechanical): `extern`, initializer lists
  `{1,2}`, in-function struct defs, braceless if/while bodies, undefined-libc
  stub shape, varargs functions. One corpus file each. ~1–2 sessions.
- **Include-resolution decision** (second hard item): fuzzy-parse vs real
  include paths, matching c2cpg's CDT binding-resolution behaviour for
  TYPE_FULL_NAMEs on real code. Prerequisite to any multi-file project. ~1
  session to decide + pin basics; risk of discovering CDT type behaviour
  tree-sitter can't cheaply reproduce.
- **M5 exit: two real projects at zero diff** (zlib, then lua/musl-as-project).
  Corpus-grind, parallelisable across sessions. ~3–6 sessions.

**"c2cpg-for-C deletable" total: ~10–17 sessions.** Note this does **not**
cover C++: `c2cpg` is C *and* C++ (CDT, templates, classes, C++17/20 test
suites). Treat C++ as a **separate ladder** (its own M1 toy corpus,
tree-sitter-cpp vs CDT) — order-of-magnitude 15–30+ sessions, name/type/template
resolution the long pole — and define an intermediate "c2cpg-for-C deletable"
criterion that does not wait on it. Do not couple C++ to Tier 1.

---

## Track B — dataflow/query parity, to retire `dataflowengineoss`

Depends on M6 (needs the real graph + `METHOD_PARAMETER_OUT` + `Local`). From
the dataflow investigation:

- **`reachableBy` parity is the genuinely hard core.** Joern's is demand-driven
  backward exploration with a call-site stack (context-sensitive down-calls),
  out-arg→callee-return splicing, paramOut routing, external/stub arg-tainting,
  and three explosion valves (`maxCallDepth=4`, `maxArgsToAllow`,
  `maxOutputArgsExpansion`). Today the engine has **no reachability query at
  all** — the ReachingDef edges are written but `summaries.rs`/`taint.rs` never
  read them (still name-based over the AST).
- **Steps:** (a) a `reachableBy` oracle probe in `oracle.sc` (reached-source
  addresses per source/sink spec — the `#`-addressing resolves them for free);
  (b) intraprocedural closure over `EdgeKind::ReachingDef` with the
  EdgeValidator/semantics gate factored out of `reaching_def.rs`; (c)
  interprocedural with the call-site stack + `SourcesToStartingPoints` source
  expansion (globals/literals).
- **Pin an achievable parity definition:** reached-**set** byte-parity for
  `reachableBy`; **behaviour** parity (not byte) for `reachableByFlows` paths —
  Joern's final path dedup is task-scheduling-order-sensitive and not worth
  chasing byte-for-byte.
- **Semantics corpus:** port Joern's ~34 libc `cFlows` verbatim into the
  (working but empty) external-JSON slot; unify the representation to a
  tri-state pass-through / receiver-index-0 / regex form so query-time arg-index
  gating matches. The two-state `Some/None` model diverges the moment `%` or
  array initializers appear in a flow.

**Retire `dataflowengineoss` when:** graph-backed FLOWS gate green;
`reachableBy` reached-set parity on corpus + 3 musl files + two real projects;
the 14 querydb C scanners finding-set-identical to `joern-scan`; Juliet
precision/recall ≥ the Scala baseline. **C-only realistic total: ~6–10 focused
weeks**, dominated by M6 and interprocedural `reachableBy`.

---

## Track C — the query/scan surface, to retire `querydb` + `joern-scan`

- Grow `cpg serve`'s JSON vocabulary from ~6 commands to the **~25-step subset**
  the real querydb C scanners actually use (measured: `.method`, `.argument`,
  `.code`, `.callIn`, `.filter`, `.reachableBy`, operator-extension steps, …),
  plus selector-based rule packs (upgrade rules from name-lists to selector
  expressions — e.g. `UseAfterFree` needs `methodReturn.reachableBy(arg)`, a
  non-call sink today's `TaintSpec` can't express).
- Port the **14 querydb C scanners** as the deliverable *and* the acceptance
  suite. Drop the android/kotlin/php/ghidra/java rule packs until those
  languages reach parity.
- Full CPGQL and the interactive Scala/`scala-repl-pp` console stay an explicit
  **non-goal** — the query surface is `cpg serve` + a documented "CPGQL step →
  server query" migration map; humans who want the classic REPL use the
  downloaded oracle Joern. ~1–2 weeks after Track B.

---

## Track D — second language to byte-parity (Java first)

The 7 engine languages pass only the **6 structural conformance assertions** —
no CODE/ORDER/FULL_NAME/CFG/dataflow bytes are checked. Verified blocker in all
three frontends: `FULL_NAME = bare NAME`, `SIGNATURE = name()`; call resolution,
symbols and summaries are all name-keyed, so every interprocedural result is
name-collision-unsound at project scale. **FQN + type resolution is the
centrepiece of any language's parity, not a detail.**

- **Harness generalization first** (cheap, ~1–2 sessions): `oracle.sc` is
  already ~90% language-agnostic (`importCode` auto-dispatches). Add
  `corpus/<lang>/`, a `--lang` flag, per-language `oracle_<lang>.txt` /
  `QUIRKS-<lang>.md`, and a per-language dumper (a serializer over the graph
  post-M6). This is the enabling infra for every language.
- **Java ladder** (M1→M7, own oracle, one rung = zero diffs in that check.sh
  section): dominated by **M3** (real FQNs/signatures `p.C.m:int(int,String)`,
  import resolution, a static type inferencer reproducing JavaParser's symbol
  solver's *observable* results). **Order-of-magnitude 35–65 sessions.** Java
  first because static types make FQN resolution the cheapest win, it has IRIS
  relevance, and javasrc2cpg is the largest single Scala frontend (30k LOC) —
  biggest deletion payoff.
- **Then Python / JS-TS (~40–80 sessions each — the program's long pole:**
  import graphs, MRO/prototype chains, pysrc2cpg type-propagation, jssrc2cpg TS
  support + desugaring, all matched byte-for-byte in FULL_NAME/TYPE_FULL_NAME).
  **Go ~15–30 sessions** (no overloading, static types — cheapest real
  language). **C++ separate/unbounded.**

**Scope cut (matches ROADMAP):** in-scope = **C/C++, Java, JS/TS, Python, Go**
(≈95k Scala LOC across 5 frontends). Out-of-scope-until-demanded = Kotlin,
Swift, C#, PHP, Ruby, Rust, Jimple, Ghidra, ABAP (≈135k LOC). Note `master` now
ships **15** frontends (rust2cpg + abap2cpg landed) — the oracle is a moving
target; don't treat the growing ones as parity goals.

---

## Track E — the actual Scala deletion (module-by-module)

Two structural facts make this safe and incremental:
- **The gate is deletion-proof.** `setup-oracle.sh` downloads Joern `v4.0.555`;
  nothing in `cpg-rs/` imports repo Scala. You can `rm -rf` any Scala directory
  today and the gate stays green.
- **The core stack comes down top-down** (dependents first):
  `querydb`/`joern-cli` → `console` → {`x2cpg`, `dataflowengineoss`, frontends}
  → `semanticcpg` → `project`/`build.sbt`. `console` has a compile-dep on
  `rubysrc2cpg` (patch out or delete Ruby last of the out-of-scope set).

**Deletion order:**
1. **Now, risk-free: the ~110k LOC of out-of-scope frontends** (Kotlin, Swift,
   C#, PHP, Ghidra, Jimple, ABAP; Ruby after the console dep is cut). Record the
   scope decision. This is the first, immediate tranche — nothing in Rust or the
   gate depends on them.
2. **Per in-scope frontend:** delete its Scala when its Rust ladder hits M5
   (own corpus, two real projects at zero diff). C first.
3. **`querydb` + `macros`:** when the C rules are re-expressed as `cpg scan`
   rule packs (Track C).
4. **`joern-cli/src` entrypoints:** when `cpg build` writes Joern-loadable
   `cpg.bin` and the drop-list (export/slice/vectors) is signed off.
5. **`console` + `semanticcpg` DSL:** when the server vocabulary + migration map
   exist and IRIS runs end-to-end on `cpg-cli` (Tier-1 acceptance). This is a
   **conscious drop** of the REPL/workspace/plugin UX, pre-authorized by the
   non-goals.
6. **`dataflowengineoss`:** when Track B's retirement condition holds.
7. **`semanticcpg` core, then `project`/`build.sbt`/`*.sbt`/`linter-rules`/CI
   workflows/root symlinks/`Dockerfile`/install scripts** in one final sweep —
   at which point hoist `cpg-rs/` to repo root (the "branch hygiene" decision:
   the cpg-rs work currently lives on a branch with history unrelated to
   `master`).

**Honest no-replacement list (dropped, not ported):** interactive CPGQL REPL +
HTTP CPGQL server + workspace + plugins (`console/`), `joern-export` (4 formats
/6 representations), `joern-slice`, `joern-vectors`, semanticcpg dot/code-dumper
generators, non-C querydb rules, and 8–9 of the 15 frontends. If any of these
turns out to be needed, it re-enters scope explicitly.

---

## Dependency-ordered sequencing (the whole program)

```
0. Gate fixes (FLOWS diff + oracle pin)                    ~hours       ← do now
1. M6 convergence (Gap 1): frontend→cpg-core, gate rewire  weeks        ← critical path
   └─ unblocks everything below
2. C long tail + M5 (zlib, lua) → c2cpg-for-C deletable    ~10–17 sess  ┐ parallel
3. reachableBy parity + semantics corpus (Track B)         ~6–10 wks    ┘ after M6
4. Query vocabulary + 14 scanner ports (Track C)           ~1–2 wks     ← after 3
5. IRIS Tier-1 acceptance (Juliet eval)                    ─            ← after 3,4
6. Harness generalization + Java ladder (Track D)          ~35–65 sess  ← after M6
7. Python / JS-TS / Go ladders                             ~long pole
8. Scala deletion, module-by-module (Track E)              mechanical   ← gated by 2–7
   └─ tranche 1 (out-of-scope frontends, ~110k LOC) can go NOW
```

**Two things most likely to be underestimated** (both agents flagged): the C
**frontend long tail** (CDT preprocessor + binding resolution on real code) and
`SourcesToStartingPoints` **source-expansion** behaviour parity. Everything else
is proven-methodology grind on top of the M6 fold.
