These two independently imported projects check interactions between the C
declaration, primitive type, numeric literal, and static modifier repairs.
Their complete AST, NODES, EDGES, and FLOWS references come from Joern
v4.0.555 under JDK 21. Only the `AST|` transport prefix is removed.

`numeric-static` combines static definitions and prototypes, unsigned return
and call types, an unsigned long long literal, a hexadecimal long double,
`_Bool`, `volatile`, and a parenthesized function definition.
`static-parentheses` declares same-named parenthesized static definitions in
two translation units and pins their distinct unresolved method identities
and ordinary call targets as emitted by the reference.

Together they contain 876 selected records and 20 method AST blocks. Both
complete graphs differ at baseline `c5ea712db` and match after the combined
repairs. [measurement.json](measurement.json) binds inputs and references to
the retained live outputs. These are finite graph comparisons, not a claim
of full C type or linkage compatibility.

Run `cargo test --locked -p joern-parity --test combined_c_types` from
`cpg-rs`. To replay live, run `joern-parity/oracle.sc` separately against each
case directory with pinned Joern and JDK 21, from a scratch working directory.
Select every AST, NODES, EDGES, and FLOWS record, remove only `AST|`, and
compare the complete result with that case's `expected.txt`.
