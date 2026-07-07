# Architecture gap implementation map

This document tracks the delta between the existing `cpg-rs` engine and the full
performance-oriented architecture described in the design thread: in-process
frontends, columnar facts, derived relations, sparse summary-based dataflow,
compiled queries, and scan-as-subscription.

## Landed in this branch

### Sanitizer-aware taint specs

`TaintSpec` now distinguishes sources, sinks, and sanitizers. Sanitizer calls are
explicit taint barriers: if a configured sanitizer wraps tainted input, its return
is treated as clean unless that same call is also configured as a source. This is
the smallest safe step toward richer LLM/human summary verdicts because it avoids
silently pruning flows unless the scan configuration names the sanitizer.

### Scan subscription primitive

`cpg-analysis::ScanSubscription` materializes a finding set for a standing taint
spec and returns only added/removed findings after a caller applies an edit. It is
transport-agnostic, so the current stdio server, a future daemon, an LSP, and a CI
adapter can all share the same diffing semantics.

### Regression coverage

`cpg-analysis/tests/taint_sanitizers.rs` builds a minimal graph for
`source -> clean -> sink`, proves the finding exists without a sanitizer, and
proves it disappears when `clean` is configured as a sanitizer barrier.

## Remaining implementation deltas

### Rich summary payloads

`FunctionSummary` is still a compact `HashSet<Flow>` where `Flow` is only
`Param(i) -> Return`. The next step is to add sidecar metadata rather than
breaking the public `Flow` shape immediately: `FlowEffect`, `SummaryTier`, and
`FlowProvenance` keyed by `(fqn, Flow)`. That metadata should later let external,
model, and human-reviewed summaries say whether a flow preserves taint, sanitizes
it, or requires proof before suppression.

### Derived facts instead of mutating overlays

The pass framework already declares read/write layers, but passes still mutate
edges directly. The next architecture slice is a relation catalog for pass output
facts with stable fact IDs and provenance. Once pass outputs are facts, graph
edges can be views over those facts, and invalidation can retract/rederive by
support chain rather than by clearing all edges of one kind for a file.

### Sparse value-flow path

The parity harness has byte-level reaching-definition work, but the production
`cpg-analysis` path still uses a deliberately simple CFG and name-based taint.
The next code move is to port the parity CFG/reaching-def builder into
`cpg-analysis`, populate `EdgeKind::Ddg`, and make taint consume the sparse
value-flow/DDG layer instead of statement-order variable maps.

### Query compiler / rule engine

The current query surface is still command-shaped JSON and library calls. The
full architecture needs a query compiler: a relation catalog, logical plans,
join/index selection, and eventually CPGQL/querydb compatibility or compilation
to the same derived-fact engine.

### mmap/CSR/content-addressed storage

`cpg-core` is columnar and persisted, but not yet mmap/Arrow-compatible or
content-addressed by file/function segment. The next storage slice is `freeze()`
to compact read-only CSR, followed by a versioned, mmap-friendly persistence
header and content-addressed segment manifests.

### Production scan daemon

`ScanSubscription` is the core primitive; the product layer still needs a daemon
or server command that keeps subscriptions alive across updates and emits SARIF or
CI-friendly deltas. The stdio server can wrap this without changing the analysis
crate.
