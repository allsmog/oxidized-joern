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

1. **Incremental CPG construction on one frontend, end-to-end** — `cpg-incremental` + `cpg-lang-c`
2. **A cross-language conformance suite** — `conformance`
3. **Cacheable, invalidatable dataflow summaries** — `cpg-analysis::summaries`
4. **A query surface that can back a server** — `cpg-core::traversal` (the server binary is the documented next step)

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
   that depended on them (`SummaryStore::update_for_changed_files`), then
   recompute only those. Everything else is served from cache.

The full build is parallel: workers parse and build standalone per-file
subgraphs concurrently (each with its own frontend instance), and the driver
absorbs them with a flat-array id/string remap (`Cpg::absorb`). The summary
fixpoint is also parallel (Jacobi rounds against a store snapshot). Edit-impact
analysis is served by a reverse-dependency index (callee name → caller files),
so finding affected files is O(affected), not a graph scan.

Measured on a synthetic 1M-LOC / 200k-function project, 4 cores
(`FILES=8000 FNS=25 cargo run --release -p cpg-incremental --example scale`):

```
== full build ==
functions:        200001
approx LOC:       1000001
live nodes:       2808007
build time:       ~7.1s        (~28,000 functions/sec)
  parallel parse+build  ~2.0s
  serial merge          ~2.3s
  passes                ~1.4s
  summaries (parallel)  ~1.4s

== incremental edit (1 file of 8001) ==
files re-analysed:     1
summaries recomputed:  26   (out of 200002)
incremental time:      ~340ms
```

A one-file edit recomputes 26 of 200,002 summaries. That ratio — not the
absolute time — is the point: edit cost tracks the change, not the codebase.
(Before the parallel build and the batch call-graph index, the same 500k-LOC
build took 275s; it is now ~3s — a ~90× cumulative improvement.)

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
```

## Honest status — what's built vs. what's next

**Built and tested:** the columnar store with incremental delete/rebuild and
parallel merge (`absorb`); the trait-based frontend contract with **two**
conforming tree-sitter frontends (C and Python, ~300 lines each, tolerant of
uncompilable code); CFG/symbol/call-graph passes on the layer-dependency
framework with batch entry points; parallel summaries-first dataflow with a
precise invalidation cache and a JSON external-summary loader; a reverse
dependency index on the edit path; the cross-language conformance harness run
against both frontends; and a 1M-LOC benchmark.

**Deliberately simplified, and where to go next** (in priority order):

1. **Parallelise the merge and passes.** The serial `absorb` merge is now the
   largest single-threaded chunk (~2.3s of the 7.1s build). Options: pre-size
   the columnar arrays from donor totals and copy ranges in parallel, or shard
   the graph by file groups. Passes mutate the graph and currently run
   serially; CFG/symbol resolution are per-method and shardable.
2. **Make the edit path fully O(affected).** Summary invalidation still builds
   an fqn→node map by scanning all methods, and the call-graph pass rebuilds
   its name index per update — both O(project) with small constants (the 1M-LOC
   edit is ~340ms; ~146ms at 500k). Persistent, incrementally-maintained
   indices remove the remaining scans.
3. **`freeze()` to CSR.** Mutable adjacency lists are right for editing but a
   quiescent graph served to many read-only queries wants a compacted CSR
   layout. Freeze on first query, invalidate on edit.
4. **More frontends behind the same contract.** Java/TS/Go via tree-sitter,
   each validated by the conformance suite — and grow the case set (control
   flow shape, field accesses, method overloading guarded by traits) as the
   real specification.
5. **Server.** Wrap `cpg-core::traversal` in a request/response API with
   non-Scala clients (the roadmap's item #4). The query surface is already
   separable from construction.

The CFG pass is a source-order linearisation, symbol resolution is intra-method
by name, and the dataflow taint is name-based rather than SSA/IFDS — all chosen
to exercise the *architecture* (layers, incrementality, summaries, conformance)
end-to-end. Replacing each with a precise implementation is local work behind a
stable interface; none of it changes the structure above.
