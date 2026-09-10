# Supplied-header macro conformance references

This fixture set retains 29 isolated Joern v4.0.555 imports: 57 input files and 6,129 complete selected records. The 27 projects under `cases/` contain 5,685 records and match the frozen candidate in full. The accepted b8c2 baseline matches two of those 27. The remaining two projects, under `diagnostics/`, contain 444 reference records and remain nonconformant.

Every `expected.txt` preserves all AST, NODES, EDGES and FLOWS records from the unchanged shared oracle, removing only the `AST|` prefix. Nothing inside a graph is filtered to obtain equality. The oracle selects properties and edge kinds; this is not a comparison of every Joern property. Its existing newline escaping remains noninjective. `oracle/` retains all 29 raw stdout/stderr pairs and the actual command, working directory, exit status, timing and reference hashes. `provenance/` preserves the original 18-case, eight-case and three-case manifests and verification receipts.

The conformant cases cover supplied object/function macros, ignored and retained arguments, spacing, statement/return/initializer use, nested expansion, header-relative paths, caller defines, definition/redefinition/undefinition timing, include guards, pragma-once behavior, separate translation units and disabled recursive macros. Missing includes retain unresolved calls. Each Project.build input keeps its relative path, including `sub/inner.h`; each case is built independently to prevent shared types or macro state from hiding differences.

Observed details include:

- Complex original macro arguments are not cloned into the wrapper. Expanded expressions normalize spacing while invocation CODE preserves it.
- An unused argument is absent. For `SECOND(a,b)`, the retained b has ORDER=1 and ARGUMENT_INDEX=2, while the expansion BLOCK also has ARGUMENT_INDEX=2. This duplicate index is the pinned graph.
- Header macro method identities retain the defining path. Redefinition in a source file changes that origin at the definition point.
- The same guarded header expands differently in two translation units with different caller defines. `#pragma once` suppresses re-evaluation; the control without it changes VALUE from one to two.
- Recursive macros retain one outer wrapper and a disabled identifier inside rather than recursively creating wrappers.

The two offsetof diagnostics retain their full inputs, live references, baseline stdout/stderr, candidate stdout/stderr and complete differences. Both still have typedef/anonymous-struct and phantom-local differences. The baseline macro-ternary producer aborts with signal 6; its empty stdout is retained as such, not described as a completed baseline graph. The candidate exits successfully for both diagnostics but does not match their complete graphs. These cases use an unsupplied `<stddef.h>` include under the pinned default import configuration; they do not establish expanded system-header offsetof behavior.

`header_macros.rs` adds one complete-graph Project.build test over all 27 conformant projects and one parity-CLI test. The CLI test checks both relative paths and absolute paths from an unrelated working directory for two nested-header projects. Both tests pass against the frozen e001da70 candidate source in a separate snapshot and dedicated Cargo target. This commit contains fixtures/tests only; the implementation dependency is recorded in `measurement.json`.

Replay from the Rust workspace after applying that implementation:

```sh
cargo test -p joern-parity --test header_macros
```

For an oracle replay, use the unchanged `oracle.sc`, an input directory from one case, JDK21, the pinned Joern runtime, and a fresh working directory. Exact original commands and hashes are retained in each `oracle/<case>/run.json`. The candidate and baseline measurements, complete comparison counts, source snapshot and validation receipts are bound in `measurement.json`. No whole-project or 29/29 parity claim is made.
