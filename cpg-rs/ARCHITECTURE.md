# cpg-rs — a from-scratch, incremental Code Property Graph engine in Rust

This is a ground-up rearchitecture of the kind of platform Joern and
Fraunhofer's CPG implement: parse source into a language-independent **Code
Property Graph** and analyse it. It is built around the conclusions of the
architecture review in this thread — it takes the storage and dataflow ideas
that make Joern fast and the framework abstractions that make Fraunhofer's CPG
clean, and adds the one thing **neither** has: incrementality as a core
invariant.

It implements, end-to-end and with passing tests, the four-item roadmap from the
discussion:

1. **Incremental CPG construction, end-to-end** — `cpg-incremental`, proven on two frontends
2. **A cross-language conformance suite** — `conformance`, passed identically by C and Python
3. **Cacheable, invalidatable dataflow summaries** — `cpg-analysis::summaries`
4. **A language-agnostic query server** — `cpg-cli` (`cpg serve`), JSON-over-stdio with live incremental updates

## Why these design choices

| Decision | Rationale (from the review) |
|---|---|
| **Columnar property store + string interning** (`cpg-core::graph`, `intern`) | flatgraph's lesson: a columnar layout with interned strings is what got Joern from 80 GB to 30 GB on the Linux kernel. Hot scans stay cache-friendly; repeated type names cost one `u32`. |
| **Closed schema enum** (`cpg-core::schema`) | A fixed node/edge vocabulary lets storage stay columnar and lets shared passes reason about any language uniformly — versus open string labels. |
| **Trait-based language contract** (`cpg-frontend`) | Fraunhofer's `LanguageTraits` model. Shared passes branch on *capabilities* (`HAS_GENERICS`, `ALLOWS_FORWARD_REFS`), not language identity, so resolution logic is written once instead of re-implemented in twelve frontends. This is the cure for Joern's ~200k LOC of frontend duplication. |
| **Shared builder primitives** (`cpg-core::builder`) | Frontends map their parse tree onto `method()`, `call()`, `add_argument()` — never onto the columnar arrays. A new language is a few hundred lines of mapping. |
| **File-partitioned, mutable adjacency** (`cpg-core::graph`) | The whole graph is partitioned by file with tombstone + free-list recycling, so `remove_file` + rebuild touches only the changed file. This is what makes incrementality `O(change)`. |
| **Passes declare read/write layers** (`cpg-analysis::pass`) | Ordering is derived (topological sort), and incremental re-runs are mechanical: re-run only the files whose layers were invalidated. |
| **Summaries-first dataflow** (`cpg-analysis::summaries`) | Compute each function's input→output flow once, reuse at every call site → roughly linear scaling. Summaries are the natural invalidation boundary, which is what links dataflow to incrementality. |

## Workspace layout

```
cpg-core         columnar graph, schema, interner, builder, query traversal
cpg-frontend     Language + LanguageTraits + Frontend traits (the contract)
cpg-lang-c       C frontend on tree-sitter (335 lines)
cpg-lang-python  Python frontend on tree-sitter (307 lines)
cpg-analysis     pass framework (layer deps) + CFG/symbol/call-graph passes
                 + summaries-first dataflow with a precise invalidation cache
cpg-incremental  the driver: parallel build, then per-edit: hash → delete
                 subgraph → rebuild → re-run only affected files/passes →
                 invalidate only affected summaries
cpg-cli          `cpg serve <dir> [--lang c|python]`: JSON-over-stdio query
                 server with live incremental updates (the `update` command)
conformance      cross-language schema conformance harness + standard cases
```

Both frontends pass the **identical** conformance suite and are served by the
same passes, dataflow engine, and incremental driver with zero engine changes —
each frontend is ~300 lines of grammar mapping. That is the consolidation
contract demonstrated, versus the 20–30k LOC per frontend it replaces.

Dependency direction is strictly downward: frontends and passes depend on
`cpg-core` and nothing depends on a frontend except the driver/tests.

## How incrementality works (roadmap #1)

`Project::update_file` (in `cpg-incremental`):

1. **Hash** the new source. If it matches, return `Unchanged` — zero work.
2. **Delete** exactly that file's subgraph (`Cpg::remove_file`): every node is
   tombstoned, incident edges are unhooked from neighbours, ids go on a free
   list for reuse. Cost is proportional to the file, not the project.
3. **Rebuild** just that file via the frontend.
4. **Re-run the pipeline** on the changed file *plus the caller files that
   reference a method name the file defines or removes* — never the whole
   project. Each pass clears its prior output edges for those files first, so
   re-runs are idempotent (no duplicate edges).
5. **Invalidate summaries** for the changed methods and every transitive caller
   that depended on them (`SummaryStore::update_for_changed_methods`), then
   recompute only those. Everything else is served from cache.

The full build is parallel: workers parse and build standalone per-file
subgraphs concurrently (each with its own frontend instance), and the driver
absorbs them with flat-array id remaps and a per-donor string-sym memo
(`Cpg::absorb` — donor interners already deduped, so each distinct string is
hashed once, not per occurrence). The summary fixpoint is also parallel
(Jacobi rounds against a store snapshot).

The edit path never scans the graph. Four incrementally-maintained indices
serve it: callee name → caller files (which files does this edit affect),
fqn → method node, method name → defining nodes (handed to the call-graph
pass via a borrowed `PassContext`, so call resolution never rebuilds a global
index), and the summary store's reverse-dependency web (callee fqn → caller
fqns), over which transitive invalidation runs as a worklist BFS that touches
only the affected region.

Measured on a synthetic 1M-LOC / 200k-function project, 4 cores
(`FILES=8000 FNS=25 cargo run --release -p cpg-incremental --example scale`):

```
== full build ==
functions:        200001
approx LOC:       1000001
live nodes:       2808007
build time:       ~5.6s        (~35,000 functions/sec)
  parallel parse+build  ~2.0s
  serial merge          ~1.4s
  passes                ~1.1s
  summaries (parallel)  ~1.1s

== incremental edit (1 file of 8001) ==
files re-analysed:     1
summaries recomputed:  26   (out of 200002)
incremental time:      ~50ms
```

At 2M LOC / 400k functions the same edit still re-analyses 1 file and 26
summaries (~100ms). The work *ratio* — not the absolute time — is the point:
edit cost tracks the change, not the codebase. (The first working serial
build of a 500k-LOC project took 275s and its edit path 460ms; cumulative
improvements: ~100× on builds, ~9× on edits.)

## Dataflow summaries (roadmap #3)

A `FunctionSummary` records flows between signature endpoints (`Param(i)`,
`Return`). For analysable functions they are computed by name-based taint over
the body, using callees' summaries (summaries-first interprocedural, fixpoint
to convergence). For external/unanalysable functions (libc, third-party) they
are loaded from a declarative JSON file mirroring Fraunhofer's
DFG-function-summary format:

```json
[{"functionDeclaration": {"language":"C","methodName":"strdup"},
  "dataFlows":[{"from":"param0","to":"return"}]}]
```

The cache tracks a dependency web (`caller fqn → callee fqns used`) so
invalidation is precise: changing a leaf function re-summarises only its
transitive callers.

**Source→sink taint queries** (`cpg-analysis::taint`) run on top of the cache:
given source and sink function names, the analysis does intraprocedural taint
within each method and lifts it interprocedurally through callee summaries (a
call propagates taint to its result iff the callee's summary maps that argument
to its return). Because it reads the incrementally-maintained summaries, a
query reflects the latest edits with no extra recomputation. End-to-end through
the server:

```
{"cmd":"taint","sources":["getenv"],"sinks":["system"]}
  → {"findings":[{"method":"handle","sink":"system","line":4,"origin":"getenv"}]}
{"cmd":"update","path":"v.c","source":"...wrap now returns a constant..."}
  → {"updated":true,"filesReanalysed":1,"summariesRecomputed":2}
{"cmd":"taint","sources":["getenv"],"sinks":["system"]}
  → {"findings":[]}      # the fix invalidated wrap's summary; the flow is gone
```

## Conformance suite (roadmap #2)

`conformance` expresses assertions against the **language-independent schema**
("a method named `two_params` has two parameters"; "a call resolves to its
definition"). Each language supplies its own source for each case via a
`LangFixture`; the assertions are identical across languages. Adding a frontend
becomes: register a fixture, reuse `standard_cases()`. Passing the suite proves
the new frontend's graph shape is compatible with every shared pass and query —
which is exactly what de-risks consolidating frontend logic.

## Running it

```bash
cd cpg-rs
cargo test                                             # all crates
cargo run --release -p cpg-incremental --example scale # the benchmark
FILES=8000 FNS=25 cargo run --release -p cpg-incremental --example scale

# the query server (one JSON request per stdin line):
cargo run --release -p cpg-cli -- serve path/to/project --lang c
# {"cmd":"stats"}
# {"cmd":"methods","name":"main"}
# {"cmd":"calls","name":"strcpy"}
# {"cmd":"summary","fqn":"wrap"}
# {"cmd":"update","path":"a.c","source":"int f(){...}"}   <- incremental
```

The `update` command demonstrates the whole architecture in one round-trip:
it rebuilds one file's subgraph, re-runs passes on the affected files only,
invalidates the affected summaries through the dependency web, and answers —
a subsequent `summary` query reflects the edit (e.g. a callee that stops
returning its parameter removes the caller's derived flow).

## Honest status — what's built vs. what's next

**Built and tested:** the columnar store with incremental delete/rebuild and
sym-memoised merge (`absorb`); the trait-based frontend contract with **two**
conforming tree-sitter frontends (C and Python, ~300 lines each, tolerant of
uncompilable code); CFG/symbol/call-graph passes on the layer-dependency
framework with batch entry points; parallel summaries-first dataflow with a
reverse-dependency invalidation web and a JSON external-summary loader; an
edit path served entirely by incrementally-maintained indices; the
cross-language conformance harness run against both frontends; a JSON-over-
stdio query server with live incremental updates; and a 1M-LOC benchmark.

**Deliberately simplified, and where to go next** (in priority order):

1. **Parallelise the merge and passes.** The serial `absorb` merge is still
   ~1.4s of the 5.6s build; pre-sizing the columnar arrays from donor totals
   and copying ranges in parallel would shrink it further. Passes mutate the
   graph and run serially; CFG/symbol resolution are per-method and shardable.
2. **`freeze()` to CSR.** Mutable adjacency lists are right for editing but a
   quiescent graph served to many read-only queries wants a compacted CSR
   layout. Freeze on first query, invalidate on edit.
3. **More frontends behind the same contract.** Java/TS/Go via tree-sitter,
   each validated by the conformance suite — and grow the case set (control
   flow shape, field accesses, method overloading guarded by traits) as the
   real specification.
4. **Richer queries + transports.** Source→sink taint over summaries is in
   (`taint` command); next are path *witnesses* (the statement chain, not just
   the finding) and a TCP/HTTP transport around the same request loop.

The CFG pass is a source-order linearisation, symbol resolution is intra-method
by name, and the dataflow taint is name-based rather than SSA/IFDS — all chosen
to exercise the *architecture* (layers, incrementality, summaries, conformance)
end-to-end. Replacing each with a precise implementation is local work behind a
stable interface; none of it changes the structure above.
