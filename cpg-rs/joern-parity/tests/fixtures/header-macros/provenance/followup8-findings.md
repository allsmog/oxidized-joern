# Additional header-macro boundary references

All8 imports passed. 1674 complete selected records are retained. The original18-case manifest and run receipt remain unchanged. No production code or tracked file changed.

- The header re-inclusion control distinguishes pragma-once behavior. With #pragma once, before() and after() both expand VALUE1. Without it, before() expands1 and after() expands2; the second include re-evaluates SEEN. `cases/pragma_once_reinclude/expected.txt:5; cases/reinclude_without_pragma/expected.txt:5`
- M(a+b), M(a + b), M(fn(a,b)) and M(fn(a, b)) all contain only one wrapper child, the expansion BLOCK at ARGUMENT_INDEX1. No original compound-expression argument clone is emitted. `cases/identity_add_unspaced/expected.txt:31; cases/identity_call_unspaced/expected.txt:31`
- Macro invocation CODE preserves caller spacing. Expansion CODE uses a + b and fn(a, b) in both spacing variants. The M wrapper TYPE is ANY even when its expanded fn call resolves to int. `cases/identity_call_unspaced/expected.txt:31`
- Direct offsetof(A, x) lowers to a normal STATIC_DISPATCH CALL named offsetof, METHOD_FULL_NAME offsetof and TYPE_FULL_NAME ANY. Its children are IDENTIFIER A (CODE A, TYPE struct) and IDENTIFIER x (CODE <unknown> x, TYPE ANY), not TYPE_REF/FIELD_IDENTIFIER nodes. Both phantom LOCALs are present. `cases/offsetof_direct/expected.txt:16`
- The OFF macro ternary keeps all three conditional arguments: identifier a; the two-argument offsetof CALL; literal0. The outer macro wrapper retains identifier a and an expansion BLOCK. These references use an unsupplied <stddef.h> include under the pinned default import configuration; they do not establish expanded system-header offsetof behavior. `cases/offsetof_macro_ternary/expected.txt:29`

See verification.json and oracle-runs.json for commands, hashes, source inventory, independent working directories, timing and complete section counts. Expected files retain every selected AST/NODES/EDGES/FLOWS record from unchanged shared oracle.sc.
