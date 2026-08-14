# Plan 005e: Validate pinned real C projects and scanner outcomes

## Status

- **Status**: DONE
- **Depends on**: 005d

Add two immutable, license-recorded C project fixtures and enforce deterministic
build/export/SARIF output, labeled positive and negative findings, recorded
precision/recall thresholds, save/load and incremental equivalence, and
hardware-normalized time/RSS budgets. Run a small subset on pull requests and
the full suite nightly and before release.
