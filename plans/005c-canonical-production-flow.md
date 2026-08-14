# Plan 005c: Route production analysis through canonical flow facts

## Status

- **Status**: TODO
- **Depends on**: 005b

Make sparse flow, CLI flow, taint rules, and summaries consume facts derived
from the production CFG and `EdgeKind::ReachingDef`. Add outcome fixtures for
branches and kills, loops, returns, globals, pointer/member access, sanitizers,
recursion, and same-name scopes. Keep independent scanner policy differences
explicit and labeled rather than disguising them as Joern parity.
