These outcomes were measured with Joern v4.0.555 and JDK 21. The Rust
`canonical_c_matches_live_joern_return_flow_outcomes` test uses the same C
input and expected output. They assert possible source-to-sink flow, including
the path on which a conditional assignment or loop does not execute.

To repeat the oracle check from the repository root, set `JOERN` to that
version's executable, then run:

```sh
fixture="$PWD/cpg-rs/cpg-analysis/tests/fixtures/return-flow"
"$JOERN" --script "$fixture/oracle.sc" --param inputPath="$fixture/return_flows.c" > /tmp/cpg-return-flow-oracle.log 2>&1
rg '^RESULT\|' /tmp/cpg-return-flow-oracle.log > /tmp/cpg-return-flow-results.txt
diff -u "$fixture/expected.txt" /tmp/cpg-return-flow-results.txt
```

The Joern command creates its own project under `workspace/` in the current
directory. The scanner's sanitizer policy has separate Rust regressions;
this fixture does not add custom Joern semantics.
