`expected.txt` contains the complete canonical graph emitted by Joern
v4.0.555 with JDK 21.0.12 for `arrays.c`, with only the `AST|` transport
prefix removed. The initial Rust graph at `f02273b86` produced 632 diff
lines because file-scope arrays incorrectly synthesized local allocations.
The repaired graph matches every AST, node, edge and reaching-definition
line. Local multidimensional arrays retain their type operand and dimensions.

To regenerate independently, set `JOERN` to that version's executable and
run from the repository root:

```sh
fixture="$PWD/cpg-rs/joern-parity/tests/fixtures/array-declarations"
oracle="$PWD/cpg-rs/joern-parity/oracle.sc"
scratch=$(mktemp -d)
(cd "$scratch" && "$JOERN" --script "$oracle" \
  --param inputPath="$fixture/arrays.c" > oracle.log 2>&1)
rg '^(AST|NODES|EDGES|FLOWS)\|' "$scratch/oracle.log" \
  | sed 's/^AST|//' > "$scratch/expected.txt"
diff -u "$fixture/expected.txt" "$scratch/expected.txt"
```

This regression was exposed by zlib's newly retained file-scope tables
`crc_braid_table[W][256]` and `crc_braid_big_table[W][256]`: their extra
allocation argument enlarged the shared `<operator>.alloc` method stub.
The fixture covers plain dimensions separately from existing macro and
declaration-type limitations; it does not claim complete zlib graph parity.
Explicit initializers keep their previous lowering. Independent before/after
probes found no changes there; brace initializers and sized local arrays
initialized from strings still have preexisting differences from Joern.
