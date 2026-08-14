# Plan 005d: Switch the gate and remove the duplicate C builder

## Status

- **Status**: TODO
- **Depends on**: 005c

After the shipped graph passes all 96 committed oracle blocks, make it the
required parity path and remove the independent AST/CFG/ReachingDef builder.
Retain only corpus/oracle acquisition, canonical rendering, and diff reporting.
Add a dependency-level regression proving the harness calls the shipped
frontend and standard production pass pipeline.
