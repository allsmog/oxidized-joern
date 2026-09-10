# Numeric macro roots

Fresh Joern v4.0.555 references cover object and function macros expanding to
integer suffixes, decimal floating suffixes, hexadecimal integers and floats,
parenthesized literals and signed literals. Literal root wrappers use the
literal type. A leading sign is a unary expression with wrapper type ANY,
including when tree-sitter folds the sign into its number node. Parenthesized
wrappers also retain ANY.

All 26 projects preserve complete raw Joern output, source and reference bytes,
baseline/candidate graphs and complete diffs: 3,392 canonical lines including
separators and 3,284 nonempty selected records. Four baseline graphs were exact;
20 are exact after the repair, with no previously exact losses. The production
test gates the complete AST/NODES/EDGES/FLOWS projection for all 20 exact cases.

The six unsigned cases remain full diagnostics. Their wrapper and literal types
match Joern, but a CFG edge is missing on synthetic macro methods whose full name
contains a space in its unsigned return type. These expected graphs retain that
edge and are not presented as complete parity. `previous-candidate.*` preserves
the earlier numeric-typing candidate, before the signed-wrapper repair.

`measurement.json` binds the frozen binary and source; `validation/` contains an
independent review and full replay receipt. Absolute paths in producer receipts
are historical provenance. This selected text projection does not cover every
Joern graph property or all numeric macro expression inference.
