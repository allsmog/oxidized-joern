# Logical macro definitions and wrapper types

These references cover six isolated header projects and one 16-function macro-type project. All seven match the frozen candidate across their complete selected graphs: 2,424 records. Two additional Lua macro invocation projects remain nonconformant diagnostics, preserving another 1,386 reference records and both before/after differences.

Every expected graph comes from pinned Joern v4.0.555 with JDK21. Only the `AST|` transport prefix is removed. All AST, NODES, EDGES and FLOWS records emitted by the oracle are retained, including scaffolding. The selected properties and edge kinds do not represent every Joern property; the existing newline encoding is noninjective.

The six header projects cover:

- Byte-exact `checkstackp` and `luaV_fastgeti` directives from Lua 5.4.7, with ordinary `p` and `slot` identifiers that must remain identifiers.
- A reduced continued macro whose formal parameter was incorrectly read as its definition name.
- Continued comments whose original directive CODE must remain complete.
- A quoted formal parameter that must remain literal text during substitution.
- Object macros in cast types, retained as an already conformant boundary.

The `pi-types` project distinguishes macro wrapper types from expansion-child types. Parenthesized replacements such as `(3.14)`, `(ID(3.14))` and an object chain ending in `(17)` have wrapper type `ANY`. Unparenthesized replacements and recursively expanded object chains retain literal types: `3.14` is `double`, and `A -> B -> 17` is `int`. Expansion literals retain their own types. Return, initializer and expression-statement contexts are included.

Pinned `MacroHandler.scala` obtains the wrapper type through `typeFor(node)`. `TypeNameProvider.scala` handles literal and identifier nodes separately; other expression nodes delegate to `ASTTypeUtil.getNodeType`. Inspection of the shipped CDT implementation confirms that its generic expression fallback is an empty string, which Joern maps to `ANY`. Source URLs and hashes are recorded in `provenance/upstream.json`; the original review also binds the inspected CDT jar and bytecode.

`diagnostics/lua_checkstackp_call` and `diagnostics/lua_fastgeti_call` exercise actual invocation expansions with small supporting declarations. The frozen candidate still differs from the complete references by 401 and 666 unified-diff lines respectively. These are retained diagnostics, not passing or ignored tests. Ordinary-name recovery does not establish complete statement-macro expansion parity.

`oracle/` preserves the raw live stdout/stderr and unchanged oracle scripts. The multiline-name case was imported in a separate recorded run. `provenance/` retains the original oracle receipts and the immutable implementation snapshot. Every case preserves its complete expected graph; the six header cases and two diagnostics also retain the earlier e001da70 candidate output and differences. That earlier candidate is not the accepted 06c3 baseline.

This commit contains only tests and evidence. It depends on the root implementation snapshot with `exact.rs` SHA256 `a14d21b4b49580f1bd825ed11d3c89ec3f323cb6a99a6ec4ee772b40a51d472b`, paired with the frozen release producer recorded in `measurement.json`. Production tests are validated in a copied workspace and dedicated target; the implementation files in this test-only worktree remain unchanged.

After applying the implementation dependency, run from `cpg-rs`:

```sh
cargo test -p joern-parity --test macro_definitions
```

For a live replay, run `oracle/oracle.sc` with `inputPath` set to one case's `input/` directory, using the pinned runtime, JDK21 and a fresh working directory. Compare its complete canonical output after removing only `AST|`. The two diagnostic projects should also be replayed without filtering their differences.
