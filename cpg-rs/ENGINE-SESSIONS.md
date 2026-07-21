# Engine capability and validation record

This record captures engine features, design decisions, tests, and aggregate validation
outcomes. Finding-level details and source-specific code paths are deliberately excluded;
they are not needed to reproduce or understand the engine behavior documented here.

Every engine change is attributed with a controlled before/after comparison: rebuild the
previous and candidate binaries, run both on identical frozen inputs (cached CPGs or pinned
trees), and byte-compare their SARIF output. Movement is accepted only after it is split
into input-tree drift versus engine effect and every changed row is classified. The
standing regression corpus spans five codebases: two Go codebases, one large C/C++
codebase, one Python service, and one large Scala codebase (16k+ entry points,
persisted-source mode).

---

## Return-origin taint summaries

FN class closed: a callee that MANUFACTURES taint inside its own body
(`func f() string { return os.Getenv("X") }`) had no summary representation — summaries
modeled param→return flows only, so every "wrapper around a source" pattern was invisible
(env-var wrappers, config readers, Sprintf-builders returning assembled command/query
strings).

Mechanism (spec-independent, cached, fixpoint-transitive):
- `FunctionSummary.call_returns: HashSet<CallReturn { call, via }>` — names of calls whose
  results flow to the return; `via` records a sanitizer when the path is sanitized (never
  raw). Capped at 64 lexicographically-first entries.
- Summary-walker tag origin generalized to `Param(k) | Call(name)`; callee summaries lift
  their own call_returns transitively in the same Jacobi fixpoint (convergence hash
  includes call_returns).
- Query side: when a callee summary's RAW call_returns intersects the spec's sources,
  taint originates at the call site; witness spliced from the callee body by the new
  `source_chain`/`source_expr` walkers (query-time sanitizers honoured — a spec-only
  sanitizer kills the lift).
- External summary JSON gains optional `"callReturns": [{"call": "getenv"}]`.

Validation: all regression corpora were exact except one Go corpus, which gained rows split
by a controlled previous-binary experiment into tree drift versus engine effect; the
engine effect was +10 rows, 0 lost, every one the intended wrapper shape
(Sprintf-assembled strings and an
env-var wrapper reaching exec/query sinks). +6 tests.

## Summary fidelity and handler-interface entry mining

1. Field-read pass-through in summary computation: the field-read lowering emits a
   paren-less Call named after the field; no summary ever exists for a field name, so the
   base's tags died there and `return x.field` accessors lost their param→return flow.
   Fix: a Call whose code text has no `(` propagates its operands' tags and self-tags
   nothing; spelled invocations stay summary-driven.
2. Entry mining: a type registered as a handler (interface-method contract, e.g. Go's
   `ServeHTTP`) is mined as an entry when the type actually defines the method; the
   multi-entry sibling filter now also stops guard functions masquerading as handlers.

Validation: +27 recall rows across three corpora, 0 lost, all the accessor shape; the two
genuinely new sink sites were triaged safe (dest-clamped copies). One witness relocation
under single-witness semantics, verified not a loss. 131 tests.

## Assignment sinks and census methodology

A ten-pass regression-corpus analysis covered exposure joins, deep reads, variant hunts,
and verification passes. Finding-level source details are outside this engineering record.
The engine-relevant results are:

- Pack-authoring caveat: `@k`-qualified sinks never fire when the receiver is
  itself a call result — the fluent-API guard rejects encoder-style idioms
  (`NewEncoder(w).Encode(v)`). Workaround: unqualified sink names. Candidate fix: exempt
  constructor-shaped (`New*`) receivers. Also recorded: the `flow` subcommand mines no
  entries (scan does), so it cannot validate entry-gated packs.
- Assignment sinks shipped: new `=<pattern>` sink spelling
  (`TaintSpec.assign_sinks`) firing when a tainted value is STORED under a matching key,
  at three store shapes — member store, setter call, named argument (keyed struct
  literals lower to the same shape). Plain local rebinds deliberately excluded. Ships as
  the compiled-in `iris:authz-overwrite` pack. By-products: SARIF result location now
  prefers the sink's file for interprocedural findings (was pairing entry file with sink
  line); the coverage report skips `=`-sinks. Sweep methodology: a mechanical
  trusted-context classifier over witness paths killed 62% of raw sites before hand
  triage; detector validated 2/2 on known-true seeds.
- Census methodology gaps recorded for future work: dispatch-level auth is invisible to a
  handler-level census; global router middleware is not attributed to per-route rows;
  exposure class should become a census column.

## Discarded-return analysis and camelCase vocabulary

1. `lhs_bindings`: binding names text-parsed from the `=` statement in TRUE source order.
   Load-bearing for Scala tuple destructuring — wildcard `_` elements never reach the
   graph; only the statement text has them. Text wins when it parses to >= the graph's
   binding count; sorted graph names remain the fallback.
2. Statement-position (total) discard: a vocabulary call sitting as a direct Block child
   discards EVERY return, verdict included; wrapper/last-expression shapes are
   structurally excluded (they sit under Return).
3. camelCase vocabulary added to the auth-discard pack (the Go-cased list matched nothing
   in Scala and missed unexported Go verifiers).

Validation: all Go baselines retained exactly; +5 Go rows, 3 of them mechanical
rediscoveries of previously hand-confirmed findings (the detector now finds what hand
review found). New Scala lane: 1 real hygiene row + 1 documented FP class (Unit-returning
throwers whose only calling convention IS statement position — per-callee triage as
designed). A controlled before/after run attributed an apparent regression to tree drift,
not the change. 146 tests.

Documented decision: no mechanical Unit-return suppression — the frontend stamps all
method returns untyped; capturing return types is a graph-shape bump plus a
persisted-corpus cache rebuild. FP class documented in the pack instead; measured cost is
per-callee, not per-site.

## Field-sensitive object flow with dotted taint keys

Precision + recall pair: member stores whole-tainted the base object (`obj.a = t` made
`sink(obj.b)` fire), and a CLEAN member store erased the whole object's taint (a quiet FN
generator — "laundering").

Field taint encoded as dotted keys in the existing String-keyed taint maps — zero
signature churn:
- `member_store_path`: full dotted LHS when every segment is identifier-shaped; None for
  subscript/deref bases (those keep whole-object taint). Fixes a latent bug where
  single-char fields were treated as plain rebinds, dissolving aliases.
- `read_path_taint`: whole-object prefix wins; exact path; or containment (a stored key
  extending the read path — the read returns a struct holding the tainted field).
- `remove_subtree`: plain rebind clears object + all field keys; a clean member store
  clears ONLY its own path (the laundering fix).
- `spread_field_to_aliases`: a field store through an alias taints only that field on the
  aliased object.

One recall bug found DURING validation and fixed: dead-path lookup on inner chain links
was short-circuiting source origination; fix walks inner links for origination only
(generic descent would resurrect the sibling FP). Validation: the large Scala corpus was
byte-identical; +5 recall rows across three corpora, 0 removals anywhere; the
sibling-field FP class (killed by construction, fixture-proven) had no real-code instance
in current baselines — latent, not rampant. +6 tests → 152.

## Test-file demotion, Point::Recv, and full-corpus validation

1. Test-file demotion for persisted-source read sites: `call_is_source` now demotes a
   persisted-source read whose read-site file matches the callgraph test-path detector —
   persisted phase-2 keys are generic getter names, so test harnesses reading the same
   key originated real-looking flows. Explicit spec sources unaffected. Validation on the
   large Scala corpus removed exactly the three known test-witness duplicate rows; the
   production row at the same line is retained.
2. `Point::Recv` — receiver-modeled summaries: receiver pass-through was unconditional
   even WITH a summary (the standing FP source: `tainted.len()` taints the count). Flows
   are positive facts, so absence of Recv→Return only suppresses pass-through when the
   summary POSITIVELY declares receiver knowledge (`"receiverModeled": true`, or any
   declared recv flow). Sanitized recv flows don't pass raw taint. Computed summaries
   never claim modeling — implicit-`this` field access is invisible to the body walk, so
   a computed non-flow claim would be unsound. Narrowing also requires a typed receiver
   (the untrusted-dynamic-dispatch guard). CLI: `--summaries <file>` loaded before the
   fixpoint so computed summaries compose. Zero behavioral delta without pack entries —
   confirmed byte-identical on all frozen baselines.
3. Full-corpus re-run: every count exact versus recorded baselines; the hypothesized
   laundering-suppressed FN class did not occur inside the narrow-scope packs (it lived
   in the builtin packs, already harvested by the preceding field-sensitive change). One
   witness-path improvement was attributed by controlled comparison to that preceding
   change, not the receiver-modeling change.

158 tests. Engine-gap ledger CLOSED: all recorded gaps shipped or deliberately parked.

## First receiver-modeled pack and validation-discovered determinism fix

1. `iris/summaries/recv-clean.json`: 37 receiver-modeled entries — length/size/count/
   capacity family plus boolean predicates and comparators — results are numeric/boolean
   by overwhelming convention, so tainted-receiver AND tainted-arg pass-through are both
   suppressed for these names. Soundness guards: untyped receivers ignore the summaries
   entirely; in-repo functions that collide with these names resolve to their own
   computed summaries. Measured effect on every current baseline: ZERO (byte-identical
   with and without) — a forward precision contract for typed-receiver-heavy corpora,
   not a count change today.
2. Determinism defect (pre-existing, found by the pack's own validation): with multiple
   tainted arguments feeding a callee whose summary has several param→return flows,
   `flows_to_return` iterated a HashSet and the reported witness flipped between runs of
   the same binary. Since byte-identical rerun comparison is the backbone of controlled
   before/after validation, both walkers now iterate param indices in sorted order (the
   call-returns loop already did); verified 3× byte-identical post-fix; a fixture test
   pins the lowest-index-wins contract. SARIF produced before this fix may show a one-time
   deterministic witness change at multi-tainted-argument sites; finding counts are
   unaffected.
3. Test hygiene: the five member-store count tests now hold the persistence lock — their
   fixtures store AND read the same fields, so a concurrent test's persist window could
   stitch phase-2 findings into their exact counts (~1-in-4 full-suite flake, gone in
   10/10 runs after).

159 tests.
