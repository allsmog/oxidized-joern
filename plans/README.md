# Production-readiness implementation plans

These plans were produced non-interactively from the highest-leverage audited
findings because the user asked to proceed without another selection round.
They target a production-grade Rust-native CLI for explicitly declared
workflows. They do **not** redefine the project as a full drop-in Joern
replacement; full CPGQL, Scala console/workspace UX, plugins, and every Joern
frontend remain outside the documented product boundary.

## Recommended order

| Plan | Title | Status | Depends on |
|---|---|---|---|
| [001](001-fail-closed-filesystem-boundary.md) | Fail closed at the source and MCP filesystem boundary | DONE | — |
| [002](002-harden-cpg-persistence.md) | Version, validate, and transactionally save CPG files | DONE | — |
| [003](003-content-correct-cache-and-reopen.md) | Make cache reuse and graph reopening content-correct | DONE | 001, 002 |
| [004](004-enforce-release-acceptance-gates.md) | Enforce semantic and packaged-binary release gates | DONE | —; rerun after 001–003 |
| [005](005-converge-production-c-engine.md) | Make the shipped C engine the parity-validated engine | DONE | 002, 004 |
| [005a](005a-production-parity-adapter.md) | Dump the shipped C graph through the parity harness | DONE | 005 |
| [005b](005b-converge-c-schema.md) | Converge production C schema and AST semantics | DONE | 005a |
| [005c](005c-canonical-production-flow.md) | Route production analysis through canonical flow facts | DONE | 005b |
| [005d](005d-remove-duplicate-parity-builder.md) | Switch the gate and remove the duplicate C builder | DONE | 005c |
| [005e](005e-real-project-acceptance.md) | Validate pinned real C projects and scanner outcomes | DONE | 005d |
| [005f](005f-compatibility-matrix.md) | Publish the language/workflow compatibility matrix | DONE | 005e |

Plans 001, 002, and 004 are independent at the planning level. For a single
reviewable release-hardening branch, execute 001, then stack 002, then 003, then
004. Plan 005 is split into child stages 005a through 005f; execute them in
order and keep every stage green.

Status values: `TODO`, `IN PROGRESS`, `DONE`, `BLOCKED`, `REJECTED`, `STALE`.
An executor should read its entire plan, run the drift check, satisfy every done
criterion, and update only its status row unless a reviewer explicitly says to
skip the index edit.
