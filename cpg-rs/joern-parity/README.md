# joern-parity — driving a pure-Rust C frontend to byte-for-byte parity with Joern

This is the first milestone of a 1:1 Joern port (frontend strategy: pure Rust),
done the only way a port like this can be verified — **differential testing
against a real Joern install as the oracle**, not by eyeballing.

## Method

1. `oracle.sc` runs inside Joern (`importCode` → c2cpg) and dumps each
   user-defined method's AST in a canonical text format (label + a fixed set of
   properties, children ordered by `ORDER`).
2. `src/main.rs` is a pure-Rust C frontend (tree-sitter) that reproduces Joern's
   `c2cpg`/`x2cpg` lowering conventions and emits the **same** canonical format.
3. `check.sh` runs both over `corpus/*.c` and diffs per method. Exit 0 ⇔ every
   method is byte-identical to Joern.

```bash
JOERN=/path/to/joern-cli ./check.sh   # regenerate oracle from Joern, then diff
```

## Conventions reproduced (verified byte-identical)

- Operators lowered to `<operator>.*` CALL nodes (`addition`, `subtraction`,
  `multiplication`, `lessThan`, `greaterThan`, `assignment`, …) with
  `DISPATCH_TYPE=STATIC_DISPATCH` and `METHOD_FULL_NAME`.
- A declaration `T x = e;` split into a `LOCAL` plus an `<operator>.assignment`
  CALL; the init-assignment is typed `void`, a *bare* assignment statement `ANY`.
- Synthetic `METHOD_RETURN` (CODE `RET`) and mirrored `METHOD_PARAMETER_OUT`
  nodes; `ORDER` sequencing across params → block → return; per-call
  `ARGUMENT_INDEX`.
- `if`/`else` and `while` as `CONTROL_STRUCTURE` nodes — including the c2cpg
  quirks that an `if`'s CODE is the whole statement while a `while`'s CODE is
  only its header, and that `else` is itself a nested `CONTROL_STRUCTURE`.
- Type resolution for this corpus (`int`/`void`/`ANY`, call-return types,
  identifier types from a per-method symbol table).

## Status & honest scope

Byte-identical to Joern on the `corpus/` programs (functions, params, locals,
nested calls, 6 operators, if/else, while). This proves the **method**: the
oracle is the spec, the diff is the gate. It is **not** yet a full c2cpg port —
no pointers/structs/`indirectFieldAccess`, no preprocessor, no `<global>`/
`<operator>` scaffolding methods, no CFG/REF/CALL graph edges (AST layer only),
and type resolution is corpus-grade, not CDT-grade. Each is added the same way:
extend the corpus, regenerate the oracle, drive the new diff to zero.
