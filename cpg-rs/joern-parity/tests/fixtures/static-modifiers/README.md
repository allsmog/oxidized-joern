This fixture contains the complete AST, NODES, EDGES, and FLOWS projection
emitted by Joern v4.0.555 under JDK 21 for `static.c`, `storage.c`, and
`spliced.c`.
Only the `AST|` transport prefix is removed in `expected.txt`.

A leading `static` token adds a modifier after the body and shifts the
METHOD_RETURN order. The pinned frontend does not emit this modifier for
`inline static`, `const static`, `int static`, or a definition whose earlier
prototype was static. Inactive explicitly static definitions still receive
the modifier. Comments, newlines, physical line continuations, and return types named `static_type` and
`static$type` pin the token boundary, including the accepted dollar-sign
identifier extension. This is observed Joern graph behavior, not a rule for C
linkage semantics. MODIFIER_TYPE is outside the selected oracle projection.

To regenerate independently, set `JOERN` to the pinned executable, use JDK 21,
and run from the repository root:

```sh
fixture="$PWD/cpg-rs/joern-parity/tests/fixtures/static-modifiers"
oracle="$PWD/cpg-rs/joern-parity/oracle.sc"
scratch=$(mktemp -d)
(cd "$scratch" && "$JOERN" --script "$oracle" \
  --param inputPath="$fixture" > oracle.log 2>&1)
rg '^(AST|NODES|EDGES|FLOWS)\|' "$scratch/oracle.log" \
  | sed 's/^AST|//' > "$scratch/expected.txt"
diff -u "$fixture/expected.txt" "$scratch/expected.txt"
```
