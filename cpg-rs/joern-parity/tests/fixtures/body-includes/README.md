# Body-level supplied-header declarations

This bounded increment retains **14 complete isolated Joern 4.0.555 projects**.
The accepted ninth source matches 3; candidate V2 matches 10. All ten exact
projects are production `Project::build` regressions in
[body_includes.rs](../../body_includes.rs). The other four remain complete
nonexact diagnostics. The references contain 5,113 canonical LF lines including
separators, or 5,014 nonempty selected records. These counts are scoped evidence,
not a C-language or whole-Joern completion percentage.

A borrowed original-header view is created for each selected body include.
Its identity includes the parent include occurrence. The same view serves
statement emission, declaration-name and phantom discovery, type sites and
prototype discovery. Declaration contexts are captured at their positions in
the header, so intervening defines and undefs affect the correct declarations.
Included declarations retain the caller's lexical block and sibling order;
macro effects persist independently of C scope. Include guards, pragma-once,
inactive effects and supplied-header resolution use the existing traversal.
Original caller-file include directives also consume dependency sibling slots,
including inactive and guard-skipped directives, as the complete controls show.
This does not claim full IMPORT-node support.

Included local typedef identities distinguish each occurrence from the later
standalone header declaration. Their TYPE_DECL file properties name the physical
header; enclosing methods keep their caller ownership. Compact declaration
ordinal ranges carry the physical source span separately to the importer.
Header roots are excluded from caller-token searches and then located against
the original header tokens, preventing an included `consume(7)` initializer from
stealing the following caller call's line. The production line regression uses
the fresh included and inline sources. All raw LINE/COLUMN/end/OFFSET observations
and the canonical ordinal bridge are retained under `oracle/origin-observations`
and `provenance/source-location-review`. The shared graph currently exposes line
coordinates; this change does not claim column or end-position parity. In
particular, the unchanged anchor's raw included LOCAL is at 2:20 despite its
identifier's apparent physical column 5.

## Macro method ownership and retained failure

Expression expansion uses each include occurrence's current environment. Uses
are replayed in lexical include traversal order; the separate pinned metadata
queue keeps its observed stable file-local offset ordering. A generated macro
METHOD chooses a complete metadata record, including CODE and source owner.
The pinned source pass runs before a separate `.h` pass, whose generated method
records take precedence for duplicate full names; first registration wins within
each pass. This is not blanket ownership by defining file or physical header
use. The repeated fixture pins both the caller-owned `main.c:N:int(0)` and the
header-pass-owned `nested/table.h:N:int(0)`, while LEAF/ESCAPED remain caller-owned.
See [the pinned pass-order proof](provenance/pass-order/review.md).

V1 is retained under `held/v1`, with its exact source, build/input binding and
all twelve full outputs. It introduced two genuine ownership-edge losses by
allowing the newly emitted caller uses to win over the header pass. V2 restores
both. The first replay's malformed diff presentation is preserved alongside an
additive LF-only readable diff; full output bytes were unaffected.

## Remaining complete diagnostics

- `repeated_include_context`: `first()` is a complete exact subtree. In
  `second()`, the existing no-eligible-event fallback still creates N3 wrappers
  where Joern emits plain literals. The standalone header also retains its
  preexisting missing N phantom. Neither difference is filtered from references.
- `tiny_fixedtables_include` and `tiny_fixedtables_inline`: their complete
  `fixedtables` subtrees agree. Both whole projects retain the same seven
  primitive member-type spelling differences (`shortunsigned` versus
  `unsigned short`). This does not close the separate static-method identity
  and CODE differences in the full zlib project.
- `unresolved_header_controls`: this historical input label is not negative
  evidence. Joern resolves the quoted missing header through a supplied decoy;
  the current resolver does not. The angle include behaves differently. No
  basename fallback was guessed or added in this increment.

Multiplicity-preserving raw comparison finds 1,263 newly matching records and
**two raw matching losses**. The two losses are `second#12/#13` EVAL_TYPE ordinal
collisions: accepted nodes are the return expression's receiver and index,
whereas the oracle nodes are the newly included initializer and its literal.
[The complete endpoint/ancestor receipt](provenance/ordinal-collisions.json)
keeps all three full methods and explains the distinct source occurrences.
The raw count remains two rather than being relabeled as zero.

All **76** prior ninth-family references were also replayed against accepted
ninth and this candidate, including the 22 earlier diagnostics: 54 to 55 exact,
25 newly matching records and zero matching losses across successful runs.
`duplicate_clinit_tag` retains its complete reference and failed producers
(accepted signal 6; candidate exit 101). Raw outputs, stderr, statuses and full
diffs are kept under `provenance/prior-family-replay`; the prior committed family
supplies the unchanged reference/input bytes bound by that receipt.

[measurement.json](measurement.json) binds each input, raw CASE extraction,
complete reference and before/current output. `oracle/` keeps the actual raw
Joern transcripts, scripts and runtime/JDK inventories. Parent release and
accepted-checkpoint provenance are included. Earlier offline helper rejection
fixtures remain immutable in ignored preparation directories and are not new
language tests. Worker focused tests and unchanged main308 pass; independent
source/package review and root whole-project/resource acceptance are separate.
