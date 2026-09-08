`expected.txt` contains the complete canonical graph emitted by Joern
v4.0.555 with JDK 21.0.12 for all 67 cases in `literals.c`. Only the `AST|`
transport prefix is removed. The baseline at `6e50d916b` produced 1,034
diff lines; the repaired graph matches every AST, node, edge and
reaching-definition line.

Coverage includes decimal, hexadecimal, octal and binary integers; upper,
lower and mixed case suffix spellings; unsigned, long and long long types;
decimal and hexadecimal floating constants; long double and signed literal
children. Hexadecimal `e` and `f` digits do not imply floating-point types.
Joern's suffix-based integer typing is retained: even an unsuffixed value
larger than a conventional `int` remains typed `int` in this graph.

The change applies only to literal AST nodes. Primitive declaration type
normalization, preprocessor evaluation and macro wrapper type inference
remain separate and are not claimed to have gained parity here.

To regenerate independently, set `JOERN` to that version's executable and
run from the repository root:

```sh
fixture="$PWD/cpg-rs/joern-parity/tests/fixtures/numeric-literals"
oracle="$PWD/cpg-rs/joern-parity/oracle.sc"
scratch=$(mktemp -d)
(cd "$scratch" && "$JOERN" --script "$oracle" \
  --param inputPath="$fixture/literals.c" > oracle.log 2>&1)
rg '^(AST|NODES|EDGES|FLOWS)\|' "$scratch/oracle.log" \
  | sed 's/^AST|//' > "$scratch/expected.txt"
diff -u "$fixture/expected.txt" "$scratch/expected.txt"
```
