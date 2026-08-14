# Plan 005b: Converge production C schema and AST semantics

## Status

- **Status**: DONE
- **Depends on**: 005a

Close production-oracle differences in small gated slices: qualified method
identity and signatures, expression wrappers, field/member/pointer forms,
labels and jump targets, switch/case, macros, globals/captures, and remaining
scaffolding. Ambiguous same-name calls must remain unresolved or retain all
justified targets; never select the first candidate arbitrarily.
