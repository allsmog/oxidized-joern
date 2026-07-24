# Architecture gap implementation map

This document maps the full performance-oriented architecture to concrete code in
this branch. The goal is to make every gap category code-owned: storage, derived
facts, sparse value flow, query planning, scan subscriptions, and auditability.

## Implemented in this branch

### In-process substrate

The existing `cpg-rs` workspace already uses in-process frontends and the shared
Rust `Cpg` graph. This branch does not change the frontend contract; it adds the
missing storage/query/fact layers around it.

### CSR freeze for query workloads

`cpg-core::freeze` adds `FrozenCpg`, a dense CSR read snapshot built from the
mutable graph. The mutable adjacency list remains the editing representation; the
frozen view is the query-heavy representation and can be rebuilt after edits.

### Segment manifest for content-addressed workspaces

`cpg-core::segments` adds `SegmentManifest`, `SegmentDescriptor`, and stable file
source digests. That is the first storage seam for per-file/function segments and
future mmap/Arrow-compatible persistence.

### Derived relation catalog

`cpg-analysis::relations` adds named relations and tuple insertion for base and
derived facts. `derive_transitive_closure` provides a small semi-naive-style
binary closure primitive that query and value-flow code can reuse.

### Auditable provenance graph

`cpg-analysis::provenance` gives every fact a `FactId`, support list, and rule
name. `invalidated_by` walks dependent facts so later invalidation can retract by
support chain instead of whole-layer clearing.

### Sparse value-flow view

`cpg-analysis::value_flow` adds `SparseValueFlow`, a sparse DDG-backed view with
reverse reachability. This is the production-facing abstraction needed to replace
statement-order/name-based taint once the parity reaching-def builder is moved
from `joern-parity` into `cpg-analysis`.

### Query compiler skeleton

`cpg-analysis::query` adds `QueryCompiler`, `LogicalPlan`, and `QueryExecutor`.
It supports a small compatibility subset now (`cpg.method`, `cpg.call`, and
`.name(...)`) and gives the codebase a real logical plan type to lower into
relation rules later.

### Scan subscription primitive

`cpg-analysis::scan` adds `ScanSubscription` and `ScanDelta`, a materialized view
of findings that returns added/removed deltas after edits. This is the core API
for a future daemon, LSP, CI adapter, or SARIF emitter.

### Sanitizer-aware taint

`TaintSpec` now distinguishes sources, sinks, and sanitizers. Sanitizers are
explicit barriers for the current name-based taint engine, and regression coverage
pins the behavior.

## Still intentionally staged behind these seams

- Joern-loadable binary export remains M6 work in `GOAL.md`; the segment/freeze
  APIs are the storage substrate for it.
- Full CPGQL compatibility remains a compiler expansion task over `LogicalPlan`.
- DDG-backed taint still depends on porting the byte-parity CFG/reaching-def code
  from `joern-parity` into `cpg-analysis`; `SparseValueFlow` is the landing zone.
- Dynamic-language precision remains frontend/rule-pack work; the relation and
  provenance APIs give those rule packs a place to materialize facts.
