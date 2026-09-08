# Method full names containing spaces

Joern v4.0.555 supplies all complete references in this package. The 31 isolated
projects contain 5,334 canonical lines including method separators, or 5,183
nonempty selected AST/NODES/EDGES/FLOWS records. Against accepted source
`e38bf19a044629e9ea6fc67248d249fb7b0321be`, complete equality improves from 3 to 17
projects; no previously exact graph is lost. The other 14 projects retain full
references, actual output, stderr, and unfiltered diffs as diagnostics.

The internal dump parser previously read FULL_NAME as one whitespace-delimited
word. Synthetic methods such as `main.c:VALUE:unsigned int(0)` and methods owned
by `a b.h` therefore lost their method-origin CFG and reaching-definition edges.
If the truncated prefix named an ordinary method, those edges were attached to
that method instead. The repair reads through the next serialized property and
selects the final FULL_NAME marker because METHOD CODE appears before it. CODE
parsing, oracle generation, comparison rules, and dataflow solver behavior are
unchanged.

The production test compares all four complete graph sections for all 17 exact
projects. Additional assertions check the prefix collision and preserve all live
method-origin CFG/RD facts in four complete but nonexact CODE-marker diagnostics.
A full multiset comparison against the baseline/live intersection retains every
previously matching selected record across all 31 projects and adds 23 matching
EDGES records plus 20 matching FLOWS records. `provenance/fact-retention.json`
retains every compared fact, including multiplicity.

An independently reviewed first candidate selected the first FULL_NAME marker.
That lost ten formerly correct facts when valid C comments or string literals
contained the marker. The rejected source patch, source/binary bindings, complete
first-candidate outputs, and four fresh reference cases are preserved under
`provenance/rejected-first/`. Final-marker selection repairs these losses without
changing the preexisting CODE transport differences.

Known bounds remain explicit:

- Five header-filename cases with uppercase property-like markers still abort in
  the importer, as they do on the accepted baseline. The selected text format
  does not quote property values; it cannot distinguish every literal property
  marker inside filenames or CODE. This patch does not claim arbitrary-string
  or injective transport support.
- Seven CODE/comment/string marker projects retain their complete transport
  differences. Their observed method-origin facts are preserved.
- Duplicate static-function naming and an ordinary function-pointer address type
  spelling remain two unrelated complete diagnostics. The spaced global-capture
  control is fully exact after the repair.

`measurement.json` binds every source, reference, output, diff, binary and source
hash. The six numeric references are copied byte-for-byte from the previously
retained unsigned-macro diagnostics; the other 25 references are fresh isolated
projects. Raw producer transcripts and wrappers under `provenance/` retain their
original CASE names and inputs. Their absolute paths are historical provenance;
production tests use the portable source files in `cases/`. This projection is
not the complete Joern schema and is not a claim of whole-language parity.

Replay the production gate from `cpg-rs`:

```sh
cargo test -p joern-parity --test spaced_method_names
cargo test -p cpg-lang-c exact::dump_property_tests
```
