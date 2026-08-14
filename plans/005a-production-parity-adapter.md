# Plan 005a: Dump the shipped C graph through the parity harness

## Status

- **Status**: DONE
- **Depends on**: 005

Make `joern-parity` depend on `cpg-lang-c` and `cpg-analysis`, construct the
same graph and pass pipeline as the released CLI, and add a deterministic
oracle-format dump plus a reproducible old-versus-production migration report.
Keep the existing standalone gate required until the production dump reaches
96/96. Verify the two affected crates and determinism.

The committed migration baseline is `cpg-rs/joern-parity/production-baseline.txt`.
