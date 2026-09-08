# Source-order body macros and macro method metadata

This bounded C increment retains **76 complete isolated Joern 4.0.555 projects**:
54 match the candidate exactly, compared with 11 on accepted eighth source
`26f4cef3775c0f77988187e04bc1693e8d43fc0d`. The other 22 remain complete
diagnostics. All 54 exact projects are explicit production `Project::build`
gates in [body_macro_state.rs](../../body_macro_state.rs); two additional
assertions compare complete ARG macro METHOD blocks in otherwise nonexact
projects. The references contain 13,694 canonical lines including separators,
or 13,294 nonempty selected records. Multiplicity-preserving comparison found
555 newly matching selected records and zero formerly matching record losses.
These are scoped measurements, not a language or whole-Joern completion rate.

The candidate uses one source-order macro context for body emission, phantom
and declaration discovery, type sites, and local prototype discovery. Macro
effects persist across C blocks and later functions and supplied includes;
lexical typedef scope remains separate. Inactive effects stay inactive. Original
source-buffer ownership, enclosing body ranges and explicit recovery/copy
suppression prevent snapshots from leaking into temporary expansion trees or
intentionally empty recovery contexts. Local typedef underlying-type
registration uses the same bounded declaration expansion.

## Observed metadata ownership

The pinned c2cpg [MacroHandler.scala](provenance/upstream/MacroHandler.scala)
uses a destructive expansion-event stream sorted by file-local numeric offset,
with stable input order for equal offsets. A condition expansion before a use
can supply that use's earlier directive text; header offsets participate in the
same ordering. `defined(NAME)` operands and quoted literals supply no expansion
event. The first registration for each synthetic full name wins. The selected
event supplies METHOD CODE and the defining-file prefix in its synthetic full
name. The invocation's current expansion still determines type and actual
argument count; METHOD source-file ownership remains the invoking translation
unit. The source-order replay also covers globals rendered after methods.

Generated full-name edits use explicit current-translation-unit dump indices,
property byte spans and edge indices. Ordinary source strings/comments with
similar full-name text are unchanged. Object-like callees that resolve to an
ordinary function keep separate argument events. A function macro consuming
the following parentheses owns the whole invocation instead. Final token
eligibility is carried through the existing bounded recursive expansion while
its disabled-name set is active; a token produced during its own expansion
remains unavailable. Empty replacements and comments preserve the preceding
significant token. The retained CDT traces and complete control graphs document
these distinctions; this is not a complete port of MacroHandler. When the
modeled queue has no eligible event, current metadata remains a compatibility
fallback rather than reproducing upstream's no-wrapper path.

## Evidence and retained failures

[measurement.json](measurement.json) indexes every source byte hash, complete
reference, baseline/candidate exit status and output/diff hash, raw CASE group,
held candidate status, and raw matching-record counter. Each `cases/` directory
contains unchanged sources/headers and expected.txt, with full baseline and
candidate output, stderr and diff. `oracle/` holds all 17 raw batch transcripts,
unchanged oracle scripts and run receipts. `provenance/` binds source snapshots,
build logs, earlier checks, pinned upstream files and direct CDT trace runs.
Absolute paths in copied receipts identify original producers; portable copies
are indexed by `copiedEvidence`. No reference rows, edge kinds or flow facts are
filtered. A narrow `-text` rule preserves evidence bytes at commit.

V1/V2 were held after a previously correct PICK METHOD CODE was replaced by
current-definition text. V4's metadata replay fixed that boundary but incorrectly
recognized macro names inside quoted strings; three formerly exact controls
failed. V5 restored those controls and scoped deferred edits to explicit current
translation-unit fields. V6 restored direct object-callee argument events but
still misclassified a disabled final function token; V7 separately fixed local
prototype branch discovery. V8 carries token eligibility during recursion and
closes that metadata loss. All earlier source snapshots, build/check receipts
and available full failed outputs are retained; V6's 333 focused tests and 308
main comparisons completed before V7 edits. Only the final worker checks are
reported as final validation. Root integration owns full release, workspace,
whole-project and resource acceptance.

## Remaining complete diagnostics

- `inactive_typedef_shadow`: existing `T=0` CODE property-marker transport loss;
  `spaced_inactive_typedef_shadow` is an additive exact control.
- `body_include_macro_only`: macro effects work, but declarations from an active
  body include are not fully lowered.
- `body_typedef_does_not_escape`: absent unused local alias TYPE registration;
  macro persistence and lexical typedef separation are independently pinned.
- `formal_list_redefined`, `condition_function_and_defined`: function-like
  preprocessor-condition evaluation remains unsupported.
- `nested_function_control`, `nested_function_macro_persistence`: GNU nested
  function lowering remains incomplete. Enclosing-body state lookup is repaired.
- `duplicate_clinit_tag`: the accepted baseline and candidate both fail in the
  importer on duplicate class-initializer identity. Nonzero status, stderr and
  the complete reference are preserved; this is not a production exact gate.
- `object_call`, `object_function_alias`, `object_nested_argument`,
  `object_redefinition`, `cast_type`, `sizeof_array`, `sizeof_type`, `comma_tail`,
  `parenthesized_tail`, `disabled_function_tail`, `object_cycle`,
  `trailing_comment`, `transitive_function`, `transitive_ordinary`: broader
  object-callee, type-position or expression lowering differences remain. The
  full diffs are retained even where a metadata METHOD is now exact.

The previously retained [guarded else-chain diagnostic](../guarded-else-chain-diagnostic/README.md)
remains outside this repair. Selected CODE transport is not injective; whole
canonical equality does not certify every raw Joern property or source column.
