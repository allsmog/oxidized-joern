# Body-include source location controls

The complete canonical/origin bridge passes: 129 AST view occurrences, 17 TYPE_DECL observations, and 33 direct SOURCE_FILE edge occurrences across three unchanged source projects. All 545 nonempty canonical records (560 lines including separators) are retained. Base64 values were decoded without trimming or normalizing line breaks; `null` denotes an absent property and an empty string denotes a present empty property.

The accepted ninth production binary is byte-exact on the complete inline control (189 records). The include control is nonexact: 237 expected versus 204 actual records. It omits the included initializer and replaces the typed local with an ANY phantom; its complete differences remain available. No candidate implementation or overall acceptance was reviewed here.

## Observed coordinates

These are raw Joern values for the standalone `probe` method view. Parent/ordinal data and the duplicated enclosing global view are preserved in `all-decoded-and-bridged.json`.

| Node / view ordinal | Parent | Included initializer control | Inline control |
| --- | --- | --- | --- |
| LOCAL included #4 | BLOCK #3 | 1:5 | 3:9 |
| Assignment #5 | BLOCK #3 | 1:5 | 3:9 |
| LHS included #6 | Assignment #5 | 1:5 | 3:9 |
| Initializer consume(7) #7 | Assignment #5 | 1:16 | 3:20 |
| Initializer literal 7 #8 | Call #7 | 1:24 | 3:28 |
| Following consume(7) #9 | BLOCK #3 | 4:5 | 4:5 |
| Following literal 7 #10 | Call #9 | 4:13 | 4:13 |
| RETURN #11 | BLOCK #3 | 5:5 | 5:5 |
| Return included #12 | RETURN #11 | 5:12 | 5:12 |

Every node listed above lacks both its own FILENAME property and a direct SOURCE_FILE edge. Its owning METHOD has FILENAME `main.c` and SOURCE_FILE → `main.c`. Thus the header initializer's observed coordinates and caller ownership must remain distinct; the observer does not supply a direct header file identity on these local/expression nodes. End positions and OFFSET/OFFSET_END are also absent on these nodes.

The included project's `api.h:<global>` TYPE_DECL has FILENAME/SOURCE_FILE `api.h`, location 1:1, ORDER 1; `main.c:<global>` has FILENAME/SOURCE_FILE `main.c`, location 1:1, ORDER 2. The inline project's global TYPE_DECL instead has ORDER 1. The `probe` TYPE_DECL belongs to `main.c` at 2:1. This does not generalize the separate local aggregate TYPE_DECL origin behavior from other controls.

The unchanged anchor reports its body-included LOCAL `included` at **2:20**, although the literal header identifier starts at column 5. Both occurrences of that node agree. Its selected canonical reference remains byte-identical to the original anchor. Retain the observed coordinate; these controls do not justify a universal physical-column calculation. The included `probe` METHOD ends at raw 6:18 while the inline METHOD ends at 6:1; both values are preserved without correction.

## Reproduction and evidence

`bridge-review.json` binds the parent release, both successful producer receipts, source snapshots, unchanged canonical script, raw origins script, and 3,687 offline checks. `all-decoded-and-bridged.json` retains every raw encoded record, exact decoded values, canonical row, ancestor chain, and direct SOURCE_FILE observations. Node IDs are used within the origin run only; cross-run matching uses project, method view, ordinal, parent/depth, and all shared selected properties.

`accepted-ninth-baseline/review.json` binds the immutable d9a7 production binary to accepted source commit `00c37613b3074025d3d65aa632cbd79af95d0c46` and eight committed frozen source/Cargo copies. It records both complete runs, inputs before/after, full output, stderr, and multiplicity counts. Use the `complete-readable.diff` files for inspection. The initial diff presentation concatenated hunk rows; that derivative and its original receipt are preserved, with the presentation-only correction recorded separately. No producer was repeated.

Raw files, all five source inputs, and all three complete references are copied in this review directory. No sealed preparation, source implementation, integration worktree, or original producer result was changed. The canonical graph projection does not include these location properties and its existing text/edge encoding is not an injective full-graph representation.
