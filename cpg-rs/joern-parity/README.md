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

The released-engine migration is observable separately while convergence is in
progress:

```bash
cargo run -p joern-parity -- --production corpus/*.c
cargo run -p joern-parity -- --migration-report corpus/*.c
```

Both commands construct `cpg-lang-c` through the same incremental project and
standard analysis pipeline used by `cpg build --lang c`. The historical
standalone path remains the required oracle until that production report is
exact; differences are not normalised away.

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

The committed corpus is byte-identical to Joern v4.0.555 across 122 graph
blocks and 1,961 ReachingDef facts. It covers methods and global scaffolding,
preprocessing, compiler inputs, CFG/REF/CALL and schema edges, structs, arrays,
heap objects, indirect fields, local and aliased function pointers,
pointer-to-pointer writes, returned aliases, pointer fields, rebind/kill
behavior, out-parameter calls, and deallocation semantics. Pinned zlib and Lua
projects provide the real-code acceptance layer. New C constructs extend the
same corpus and must drive the exact node/edge/flow diff back to zero.
