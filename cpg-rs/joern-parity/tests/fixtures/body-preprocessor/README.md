# Selected function-body preprocessor branches

The compiler now uses one kept-branch traversal for statement emission, pending
reference discovery, declaration shadow discovery, and typedef context. It
handles `#if`, `#elif`, `#else`, `#ifdef`, and `#ifndef` using the method's existing
source-position macro snapshot. Directive condition identifiers and inactive
local declarations do not become or suppress active phantom locals.

The 34 complete projects retain 4,371 canonical lines including method
separators, or 4,247 nonempty selected AST/NODES/EDGES/FLOWS records. Against the
accepted seventh source, complete equality improves from nine to 30 projects,
with no formerly exact graph lost. All 30 exact projects are production gates.
The original ten raw references are unchanged; 23 additional fresh projects pin
nested branches, header state, declaration shadows, directive whitespace, and
inactive headers. One project reuses the already retained conditional
source/LOC reference to verify an inactive-method location improvement.

Whole-project preview found two interacting defects that the small initial
fixtures did not expose. First, spaced `#    undef W` was ignored. A shared
`undef` token recognizer now accepts whitespace after `#`, with active, inactive,
unknown-name and source-order controls. Second, inactive function declarations
had no macro snapshot, producing the return type `local` for a macro-prefixed
header. Their header snapshots are now retained without executing their bodies
or applying inactive directives. Every earlier candidate and complete failure
is retained in `provenance/` and the bound immutable scratch locations.

The unchanged zlib input restores all three live `snprintf` call records in
`gz_open`, `gzdopen`, and `gz_error`, plus the proper stub. Exact method ASTs
improve from 149 to 160 of 410 live methods; Lua improves from 1,431 to 1,434 of
2,274. No previously exact method AST is lost. The full whole-project outputs
remain nonexact; these method counts are not language-parity percentages.
Whole-output hashes, complete affected method blocks and producer receipts are
retained alongside the earlier rejected `byte_swap` comparisons.

The inactive conditional duplicate's METHOD and METHOD_RETURN now report line 8,
matching the retained live LOC records. The earlier test deliberately preserved
an incorrect baseline line 7. Only that assertion and its explanation change;
canonical references and the importer are unchanged.

Body-local `#define` state remains unsupported. `local_definition` retains its
complete nonexact reference and actual graph: the current compiler chooses
`return 0` where Joern chooses `return x`. One previously matching raw flow line
is removed because ordinal #5 referred to METHOD_RETURN in the baseline but to
IDENTIFIER x in Joern. The parameter-to-METHOD_RETURN fact survives at #6.
`provenance/ordinal-collision.json` records complete endpoints and ancestor paths;
this is explicitly not a claim of zero raw-record losses. Three additional
directive-token diagnostics (`comment_between`, `formfeed_space`, and
`spliced_keyword`) retain complete graphs byte-identical to the baseline. Their
comment-separated directive, form-feed whitespace, and spliced keyword remain
unsupported; the original source bytes and full raw transcripts are preserved.

The final source passes 333 C/frontend/analysis/parity tests, strict Clippy and
format checks, and all 308 committed and fresh live main comparison blocks. The
expanded focused tests cover all 30 exact projects and the location assertion.

`measurement.json` binds every source, complete reference, output and diff.
Absolute producer paths are historical provenance; tests read the portable
`cases/` sources. The selected projection is not the complete Joern schema, and
other preprocessing expression/build-context limits are outside this increment.

```sh
cargo test -p joern-parity --test body_preprocessor --test declaration_macros
```
