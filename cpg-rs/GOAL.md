# GOAL: 1:1 Joern port in pure Rust, proven by differential testing

You are one session in a long-running autonomous effort. Many sessions ran
before you; many will run after. Your job is NOT to finish the port in one
session — it is to move the ratchet forward by at least one verified increment
and leave the repo in a state where the next session can continue without you.

## Mission

Port Joern (https://github.com/joernio/joern) to pure Rust, 1:1, in `cpg-rs/`.
"1:1" has exactly one meaning here: **for the same input, the Rust port's
output is byte-identical to a real Joern install's output**, as checked by the
differential harness in `cpg-rs/joern-parity/`. Joern is the spec. The diff is
the gate. Nothing counts as done until its diff is zero.

## Non-negotiable invariants

1. **Never weaken the gate.** You may not edit oracle output by hand, filter
   inconvenient diff lines, drop a corpus file to make a failure disappear, or
   loosen `check.sh`'s byte-identity requirement. If Joern's output looks
   wrong, it is still the spec — match it and note the quirk in
   `joern-parity/QUIRKS.md`.
2. **Never end a session red.** `joern-parity/check.sh` must pass on every
   commit you push. If your in-progress feature can't reach zero before the
   session ends, commit it behind the gate (corpus case added, oracle
   regenerated, failure documented in PROGRESS.md as the next session's first
   task) — but `check.sh` over the *previously passing* corpus must stay green.
3. **Always push.** The environment is ephemeral. Commit small, push every
   commit to the current `claude/...` branch (`git push -u origin <branch>`).
   Unpushed work is lost work.
4. **Always update `cpg-rs/PROGRESS.md`** in the same commit as the work it
   describes. It is the only memory the next session has.
5. **Never block on the user.** If genuinely stuck after 3 distinct attempts
   on one item, write a minimal repro + findings into PROGRESS.md under
   "Stuck", mark the item deferred, and pick the next item. There is always a
   next item.

## How to orient (first 10 minutes of every session)

1. Read `cpg-rs/PROGRESS.md` top to bottom. It tells you the current
   milestone, the next task, and anything the last session left half-done.
2. Run `cpg-rs/joern-parity/setup-oracle.sh` (downloads Joern to
   `/tmp/joern-cli-dist`; ~2GB, the environment is fresh each session).
3. Run `cd cpg-rs/joern-parity && ./check.sh`. It must be green before you
   build anything new. If it is red on a fresh checkout, fixing that IS your
   task — diagnose whether the oracle version drifted (new Joern release
   changed output: record old/new version + diff in QUIRKS.md, then update the
   port to match the new oracle) or a regression was pushed (fix it).
4. Then do the next task from PROGRESS.md.

## The loop (one increment)

Every increment, no matter the milestone, is the same shape:

1. Pick the **smallest** next unit of behavior (one statement kind, one
   operator family, one node/edge kind, one scaffolding structure).
2. Write a minimal C program exercising it into `joern-parity/corpus/`.
3. Regenerate the oracle (`check.sh` does this when `$JOERN` is reachable).
4. Run the diff. Read what Joern actually emits — do not guess conventions.
5. Implement in Rust until the diff is zero. Match quirks exactly; document
   each quirk in `QUIRKS.md` with the corpus file that pins it.
6. Run the FULL `check.sh` (no regressions), commit, push, update PROGRESS.md.
7. Go to 1. Repeat until you run out of context or session time.

## Milestone ladder

Work strictly in order; a milestone is complete only when PROGRESS.md says so
with the evidence listed.

- **M1 — C AST parity, toy corpus. DONE** (functions, params, locals, nested
  calls, 6 binary operators, if/else, while — 4/4 methods byte-identical to
  Joern v4.0.555).
- **M2 — Full C statement/expression coverage (AST layer).** for, do-while,
  switch/case, break/continue/goto/label, ternary, all unary/postfix ops
  (`-x`, `!x`, `~x`, `*p`, `&x`, `x++`, `x--`), all binary + compound
  assignment ops, casts, sizeof, comma operator, string/char/float literals,
  arrays + indexAccess, structs + fieldAccess/indirectFieldAccess, pointers,
  typedef'd types, multiple declarators per declaration, function prototypes,
  global variables. Done when each has a pinning corpus file and check.sh is
  green over all of them.
- **M3 — Full node-set parity.** Emit everything Joern emits, not just
  user methods: `<global>` methods (both `<includes>:<global>` and per-file),
  `<operator>.*` stub methods, TYPE_DECL / TYPE / NAMESPACE_BLOCK / FILE /
  META_DATA nodes, METHOD_REF bindings. Extend `oracle.sc` to dump these
  (remove the `filterNot(_.name.startsWith("<"))` filter and widen the key
  set); drive to zero.
- **M4 — Edge parity beyond AST.** Extend `oracle.sc` to dump edges (CFG,
  REF, CALL, ARGUMENT, EVAL_TYPE, CONTAINS, SOURCE_FILE) in a canonical
  format; implement Joern's CFG construction (`CfgCreationPass`) and linking
  passes in Rust; drive each edge kind to zero separately, in that order.
- **M5 — Real-world corpus.** Run both sides over small real C projects
  (e.g. zlib, lua — vendor exact versions into corpus/ or pin by URL+hash).
  Triage every diff: each becomes either a minimal new corpus case + fix, or
  a documented QUIRKS.md entry. Done at zero diffs on at least two projects.
- **M6 — Real graph output.** Stop being a text dumper: build the graph in
  `cpg-core`'s schema, make the canonical dump a serializer over that graph,
  and add a flatgraph/CPG binary export validated by `joern --script` loading
  it. The parity gate stays the same; only the internals change.
- **M7 — Dataflow layer.** Port reachingDef / dataflow passes; oracle =
  `joern-flow` / reachableBy results on corpus programs; drive to zero.
- **M8+ — Next frontends, same harness.** One language at a time
  (suggested order: JavaScript via tree-sitter, then Python), each starting
  from its own M1 toy corpus and climbing the same ladder. Then the query
  layer. Do not start a new frontend while the current one has a red gate.

## Definition of "implemented"

The goal is reached when, for each ported language, the parity harness shows
zero diffs against the pinned Joern release across all milestone layers
(nodes, edges, dataflow) on both the pinning corpus and the real-world corpus,
and PROGRESS.md's checklist is fully checked. Until then, there is always a
next increment: re-read PROGRESS.md, pick it, and keep going.
