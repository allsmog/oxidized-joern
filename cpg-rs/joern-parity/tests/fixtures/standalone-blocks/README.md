# Standalone compound statements

`blocks.c` and `expected.txt` pin the complete existing canonical projection
to a fresh Joern **v4.0.555** run under JDK **21.0.12**. All **1,708 lines**
match, including AST, NODES, EDGES, and FLOWS. No graph properties or edges
were filtered to make this fixture pass. This is the selected projection in
`joern-parity/oracle.sc`, not every Joern property.

The fixture covers ordinary, nested, empty, labeled, switch-case, and loop
blocks; parameter/local shadowing; callable prototype and function-pointer
scope; and blocks whose children return, break, or continue. A standalone
block occupies one parent AST position and is a CFG node after its children.
Method and control-body blocks retain their existing CFG behavior.

`standalone_blocks.rs` tests the full graph, CFG block participation, and
restoration of REF bindings and pointer dispatch. The production scanner
test `canonical_c_standalone_blocks.rs` uses the larger diagnostic input
below and requires **14/14** live `reachableBy` outcomes, including four
negatives. The frozen baseline matched **3/14**: seven false negatives and
four false positives. Separate tests preserve optional flows, definite
kills, and both query and stored-summary sanitizer cuts.

Joern reports a possible flow when both `source()` and `sink` appear after
a block-contained return, break, or continue. It reports no source-to-sink
flow when the source precedes those terminating blocks and the sink follows
them. Both sets of outcomes are retained. They do not establish that a
reported path can execute.

## Retained reaching-definitions diagnostic

`loop-tail-diagnostic/blocks.c` adds three source-before-terminator cases.
Its entire **2,042-line** live projection is retained in
`expected-joern.txt`, with the Rust output in `actual-rust.txt` and the
unfiltered `complete.diff`. AST, NODES, and structural edges match; five
REACHING_DEF facts differ: four missing loop-tail call/argument-to-exit
facts and one extra decrement-to-exit fact. This larger graph is **not
claimed exact**, although all 14 final scanner outcomes match.

`loop-tail-diagnostic/preexisting.c` and its complete Joern/Rust outputs reproduce those same
five differences without standalone blocks. The frozen baseline and the
candidate produce byte-identical output for that input. The repair leaves
the reaching-definitions solver unchanged.

A separate pinned solver probe showed that Joern initializes GEN sets for
disconnected calls outside its reverse-postorder iteration and can read
them through loop predecessors. Its parameter-out/exit result also differs
from the current Rust exit approximation. These require separate solver
work; the complete counterexamples remain available for that work.

## Repeat the comparisons

From the repository root, with `JOERN` set to the pinned executable and JDK
21 on `PATH`, use a unique working directory for each oracle run:

```sh
fixture="$PWD/cpg-rs/joern-parity/tests/fixtures/standalone-blocks"
scratch=$(mktemp -d)
(cd "$scratch" && "$JOERN" --script "$fixture/../../../oracle.sc" \
  --param inputPath="$fixture/blocks.c" > graph.log 2>&1)
sed -n -e 's/^AST|//p' -e '/^NODES|/p' -e '/^EDGES|/p' -e '/^FLOWS|/p' \
  "$scratch/graph.log" > "$scratch/graph.txt"
diff -u "$fixture/expected.txt" "$scratch/graph.txt"

flow_scratch=$(mktemp -d)
(cd "$flow_scratch" && "$JOERN" --script "$fixture/outcomes.sc" \
  --param inputPath="$fixture/loop-tail-diagnostic/blocks.c" > flow.log 2>&1)
rg '^RESULT\|' "$flow_scratch/flow.log" > "$flow_scratch/outcomes.txt"
diff -u "$fixture/loop-tail-diagnostic/outcomes.txt" "$flow_scratch/outcomes.txt"

cargo test --locked --manifest-path cpg-rs/Cargo.toml \
  -p joern-parity --test standalone_blocks \
  -p cpg-analysis --test canonical_c_standalone_blocks
```

`measurement.json` binds the inputs, references, baseline binaries, and
local raw receipts. The main committed corpus and `oracle_all.txt` are
unchanged by this repair.
