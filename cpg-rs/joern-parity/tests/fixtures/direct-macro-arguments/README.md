# Direct preprocessing-token macro arguments

These 16 isolated C projects retain 4,531 canonical lines (4,435 nonempty records) from pinned Joern v4.0.555. The frozen fifth-batch baseline matches 4 complete graphs; the repaired production frontend matches 12. Every expected graph contains every AST, NODES, EDGES and FLOWS record from its CASE, with only the `AST|` prefix removed. The existing selected-text projection does not compare all Joern properties and its newline escaping is not injective.

The original direct invocation and phantom-collection paths used named C syntax children as macro arguments. An operator-only actual such as `+` could disappear, shifting `v1` and `v2` into the wrong formal slots. Nested token expansion then received an empty final operand. The repair shares the preprocessing-token argument reader with nested expansion, before expression parsing. Parentheses nest; quoted tokens and comments do not expose argument separators; comments become whitespace; physical line splices are removed before lexical recognition. Original source offsets remain available to the nested replacement loop. Direct macro invocation CODE is retained.

Copied argument subtrees keep the original actual index even when an earlier operator or type argument produces no copied expression. For `APPLY(+, a, b)`, the copied nodes have argument indices 2 and 3. The pinned comma-expression actual is not copied. The expansion BLOCK index continues to follow Joern's descendant ARGUMENT-edge count. Only expanded comma and unary expression spelling is normalized by this change; ordinary source expression CODE is unchanged.

The exact projects cover first and middle operator slots, unary operators, shifts, type-token arguments, nested macros, nested comma expressions, unknown operand collection, quoted commas/parentheses/comment delimiters/escaped quotes, whitespace and continued line comments. `direct_macro_arguments.rs` compares all 12 complete production graphs. Its second test uses the typed Lua diagnostic to require both original operands and copied argument slots without asserting full typedef-cast parity. Unit tests separately pin empty lexical slots, malformed unclosed lists, LF/CRLF splice offsets, and comment delimiters formed by splices.

Four complete diagnostics remain, with source, full live graph, baseline graph, candidate graph and unfiltered differences:

- `empty_slots`: the source parser does not retain the trailing-empty `PICK(,value,)` invocation as a call. The earlier `PREFIX(,value)` invocation now has the correct copied argument.
- `spliced_operator`: a physical splice inside `>>` prevents the outer source parser from retaining the invocation. Candidate and baseline complete output are identical.
- `spliced_block_comment`: comment delimiters formed by physical splices make the source parser truncate the invocation before the shared argument reader sees it. The existing outer parser defect remains; the expanded operand within its truncated wrapper improves.
- `lua_intop`: the correct `v1`/`v2` and shift operands are restored, but temporary macro parsing still treats known typedef casts as pointer calls. No full graph equality is claimed for this diagnostic.

The unmodified 61-file Lua replay also restores the actual ADD-branch `v2` reaching-definition fact and both shift right operands. Node addresses changed, so the review follows the owning RETURN occurrence and complete ancestor chain rather than matching a repeated identifier/parent pair. The complete replay and independent semantic receipt are linked from `measurement.json`. Whole-project Lua remains nonexact.

`oracle/` retains the three complete fresh runs and their scripts. Each script is the shared projection in a sorted, isolated per-directory import loop. No reference facts were changed to achieve equality. `provenance/` retains pinned upstream MacroArgumentExtractor/MacroHandler source, validation logs and the source/binary-bound independent review. `evidence/comma-spacing/` preserves the full intermediate mismatch before the narrowly scoped spelling correction. `measurement.json` binds every source and graph file, frozen source/binary hashes and raw oracle outputs. Absolute scratch paths describe historical execution; compiled tests use only the portable files here.

Replay one oracle group from a fresh working directory with JDK 21:

```sh
JAVA_HOME=/path/to/jdk21 PATH=/path/to/jdk21/bin:$PATH \
  /path/to/joern-cli/joern --script /absolute/path/to/oracle.sc \
  --param inputPath=/absolute/path/to/isolated-project-directories
```

Run the production and lexical tests with a target dedicated to the checkout:

```sh
cargo test -p joern-parity --test direct_macro_arguments
cargo test -p cpg-lang-c macro_token_tests
```
