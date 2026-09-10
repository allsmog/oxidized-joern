# Production-readiness implementation plans

Preserved from the 2026-08-18 audit of `025d9778c` during the 2026-09-09
integration. The statuses below describe that audit, not the combined current
implementation. Recheck each finding against current code before scheduling work. Each executor must read its plan completely, run its
drift check first, honor STOP conditions, and update only its status row unless
a reviewer says they own this index.

These plans target a production-grade Rust-native CLI for explicitly declared
workflows. They do not redefine the product as a universal Joern replacement;
Scala console/workspace UX, JVM plugins, every QueryDB rule, and unrestricted
Scala/CPGQL execution remain outside the documented product boundary.

## Current Execution Order

| Plan | Title | Priority | Effort | Depends on | Status |
|---|---|---|---|---|---|
| [006](006-complete-cpg-persistence-safety.md) | Complete the persisted CPG safety contract | P0 | L | none | TODO |
| [012](012-close-filesystem-boundary-residuals.md) | Close residual filesystem-boundary gaps | P1 | S | 006 | TODO |
| [008](008-make-graph-language-metadata-explicit.md) | Make persisted graph language metadata explicit | P0 | L | 006, 012 | TODO |
| [007](007-bind-release-to-verified-contract.md) | Bind every release to a verified acceptance contract | P0 | L | 006 | TODO |
| [010](010-differential-gate-shared-reaching-def.md) | Differential-gate the shared ReachingDef pass | P1 | L | 006, 007 | TODO |
| [009](009-refresh-architecture-and-product-docs.md) | Make architecture and product documentation truthful | P1 | M | 006, 007, 008, 010, 012 | TODO |
| [011](011-migrate-summaries-to-canonical-value-flow.md) | Make canonical value flow the C analysis spine | P2 | L | 008, 010 | TODO |

Execute 006 -> 012 -> 008 to avoid overlapping persistence/workspace/path APIs.
Plan 007 follows 006 because both edit CI. Plan 010 follows 006 and 007 because
it touches graph edge persistence and the required workflow. Plan 009 runs
after the current implementation plans so its rewrite reflects their final
state. Plan 011 requires Plans 008 and 010; it must update the authoritative
documents established by Plan 009 if Plan 009 has already landed.

## Historical Plans

Plans 001-004 described real defects and have corresponding implementation
commits, but their local documents were never committed while this index linked
to them. They are retained as historical evidence and marked `STALE` because
current HEAD has evolved beyond their original done criteria. Do not execute
them; use their named replacement plans.

| Plan | Historical result | Implementation | Current status | Replacement |
|---|---|---|---|---|
| [001](001-fail-closed-filesystem-boundary.md) | Closed language/source/MCP boundaries | `93290be8b` | STALE | 012 |
| [002](002-harden-cpg-persistence.md) | Added bounded CPG2 validation and atomic save | `561b46b02` | STALE | 006 |
| [003](003-content-correct-cache-and-reopen.md) | Added content manifests and corrected reopen ordering | `2c83a8a4f` | STALE | 008 |
| [004](004-enforce-release-acceptance-gates.md) | Strengthened required/package acceptance | `a01efb67a` | STALE | 007 |
| [005](005-converge-production-c-engine.md) | Converged the shipped C engine | child plans below | DONE | none |
| [005a](005a-production-parity-adapter.md) | Dumped shipped C graph through parity | repository history | DONE | none |
| [005b](005b-converge-c-schema.md) | Converged C schema and AST semantics | repository history | DONE | none |
| [005c](005c-canonical-production-flow.md) | Routed production analysis through canonical facts | repository history | DONE | 010/011 deepen this boundary |
| [005d](005d-remove-duplicate-parity-builder.md) | Switched the production compatibility path | repository history | DONE | none |
| [005e](005e-real-project-acceptance.md) | Added pinned real-C acceptance | repository history | DONE | none |
| [005f](005f-compatibility-matrix.md) | Published workflow compatibility matrix | repository history | DONE | 009 refreshes drifted prose |

## Dependency Notes

- Plan 006 should land before any future persisted-column extension because it
  establishes aggregate decoded-memory and envelope-version rules.
- Plan 012 follows 006 because both can touch persisted-path APIs and workspace
  shape/version code.
- Plan 007 follows 006 and preserves the protected context name `linux-package`.
- Plan 008 owns graph language identity. Plans touching merge, reopen, default
  rule selection, or loaded export should not invent parallel metadata. It
  follows 006 and 012 to avoid overlapping workspace changes.
- Plan 009 establishes document ownership: `COMPATIBILITY.md` for public scope,
  `REPLACEMENT_CONTRACT.md` for acceptance, `ROADMAP.md` for future work, and
  `PROGRESS.md` for history.
- Plan 010 proves the shared ReachingDef implementation independently. It runs
  after 006/007 because it touches graph properties and required CI.
- Plan 011 may consume shared flow only after exact zero diff and uses Plan 008's
  explicit graph-language mode to avoid changing non-C semantics.

## Findings Considered And Deferred

- **Universal Scala console/JVM plugin/full CPGQL recreation**: rejected as a
  current plan because it contradicts the bounded native product contract.
- **Integrate FrozenCpg solely because it exists**: deferred. The current query
  executor uses mutable `Cpg`; require a named workload and benchmark first.
- **Adopt SegmentManifest, RelationStore/provenance, or ScanSubscription now**:
  deferred. They have no production consumers and contain latent correctness or
  identity gaps. Plan 009 must label them prototypes. Any future consumer needs
  its own plan with correctness tests before integration.
- **Skip heavy live PR gates through path filtering**: deferred. First complete
  Plan 007. Any optimization must preserve an always-reporting, fail-closed
  `linux-package` context and use a separately reviewed relevance classifier.
- **Promote every remaining language equally**: rejected. Future precision work
  should select a language from a named user/workflow and add its own oracle,
  real-project, and labeled-security evidence.

Status values: `TODO`, `IN PROGRESS`, `DONE`, `BLOCKED`, `REJECTED`, `STALE`.
