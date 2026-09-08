These 26 source-to-sink outcomes were measured with Joern v4.0.555 and
JDK 21.0.12. `expected.txt` is the live oracle output; `baseline.txt` records
the scanner output at commit `26b5456b2`. The baseline matched 12 outcomes
and missed 14 possible flows. The `direct_c_matches_live_joern_outcomes`
regression requires all 26 expected outcomes, including the nine negatives.

The fixture covers optional and definite overwrites, both branch orders,
loop-carried values, zero-iteration loops, early returns, nested summaries,
parameter-to-sink handoffs, fields, arrays, dereferences, short-circuit
expressions, and switches. Reachability is may-flow; these results do not
prove a particular control path executes at runtime. Query and stored
sanitizer policies have separate tests in `direct_c_flow.rs`.

To repeat the live check, set `JOERN` to that version's executable and run
from the repository root:

```sh
fixture="$PWD/cpg-rs/cpg-analysis/tests/fixtures/direct-flow"
scratch=$(mktemp -d)
(cd "$scratch" && "$JOERN" --script "$fixture/oracle.sc" \
  --param inputPath="$fixture/direct_flows.c" > oracle.log 2>&1)
rg '^RESULT\|' "$scratch/oracle.log" > "$scratch/results.txt"
diff -u "$fixture/expected.txt" "$scratch/results.txt"
```

A unique working directory keeps Joern's generated workspace isolated.
`measurement.json` records the pinned archive digest and baseline differences.
These outcomes measure this fixture, not overall Joern feature completeness.
