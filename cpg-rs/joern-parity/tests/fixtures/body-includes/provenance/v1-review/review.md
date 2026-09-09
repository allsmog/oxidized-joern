# Frozen body-include V1: held for two ownership edges

Reviewed only frozen source `96207d5b7b7b3d4bbfa77c8b93f407132eb0d2525e7e8f7915ad784e167ad406`, binary `2cfaa3551fe0ba0797cf5f52db65f7163b04b772db8c57409bf126d4c7454d7a`, its build binding, and all 12 saved replays. No producer/build or mutable working-source read was performed. Complete expected/baseline/candidate outputs and differences are hash-bound in `verification.json`.

The claimed complete-graph result is correct: **2 baseline exact projects become 7; neither old exact project is lost**. Multiplicity-preserving selected-record comparison finds **1,106 newly matching records and 4 formerly matching raw records lost**. Two losses are genuine semantic ownership regressions; two are address coincidences, fully retained below. Seven exact projects do not close that regression.

## Required repair: repeated macro METHOD ownership

In `repeated_include_context`, live Joern and the accepted baseline have:

```text
EDGES|CONTAINS D:nested/table.h:<global> -> nested/table.h:N:int(0)#0
EDGES|SOURCE_FILE nested/table.h:N:int(0)#0 -> F:nested/table.h
```

V1 replaces both owning endpoints with `main.c`. The complete target METHOD block is byte-identical in all three graphs:

```text
METHOD NAME=N CODE=#define N 4 FULL_NAME=nested/table.h:N:int(0) SIGNATURE=int(0) ORDER=1
  BLOCK TYPE_FULL_NAME=ANY ORDER=1
  METHOD_RETURN CODE=RET TYPE_FULL_NAME=int ORDER=2
```

The FILE and file-global identities are stable named endpoints. This is a previously correct fact becoming wrong, not an ordinal shift or a difference excused by the method's other diagnostics. `ownership-regression.json` retains every owner edge and all complete inputs/graphs.

The other two raw losses are EVAL_TYPE at `second#12` and `second#13`. At #12, old Rust is the unknown last_table identifier in the return, while Joern is first_table's arrayInitializer. At #13, both old Rust and Joern happen to spell literal 3/int, but the old node belongs to return `last_table[3]`; Joern's belongs to the first_table initializer. V1's #12/#13 are yet different nodes. Full ancestor chains are retained; neither row identifies a lost matching source occurrence.

Do not repair ownership by unconditionally assigning macro methods to their definition file or physical header-use file. Live `main.c:N:int(0)` still belongs to main.c even though its first wrappers occur in included array declarations. The existing LEAF and ESCAPED methods are defined in headers but belong to their invoking main.c. The pinned MacroHandler records current AstCreator `filename` (`MacroHandler.scala:155`); `AstCreationPass` retains first entries within an accumulator and merges previous-pass entries into the new accumulator (`34–35`, `63–64`, `110–112`). A later header pass's declaration precedence is a source-grounded hypothesis pending construction/order verification, not yet a proven replacement policy.

## Source review: coherent parts and remaining limits

The new borrowed `IncludedBodyView` and `(parent_view, include_node_id)` lookup preserve original tree/source lifetimes (`frozen exact.rs:978–1140`). The collector creates separate executed occurrences, retains selected header items, and shares sparse Arc states. All five consumers consult those same entries: prototype discovery, type sites, declaration names, phantom walking, and statement emission. File-scope inactive retention is not turned into active body traversal. The exact inactive/guarded/scalar/array controls support that narrow behavior.

Instance-aware keys now cover TypeSites, recovery candidates, macro-use deduplication and project-level typedef identities. Header macro uses are recognized as original-source uses. The emitter splices declarations with caller ORDER and lexical symbol state rather than entering a synthetic block; physical typedef source filename has a separate header path. These changes address the concrete architecture hazards without altering CFG/RD solvers.

The remaining wrapper/metadata mechanism still matters. `resolve_macro_metadata` is unchanged and sorts actual uses by file-local byte offset; adding header uses interleaves distinct lexical occurrences. Only preprocessing events have been shown to use that numeric sort. The implementation agent is correcting actual-use order in a separate candidate. V1's second first_table still gets wrappers where Joern has plain literal 3. Ownership may depend on that correction and on pass precedence; do not hardcode the current first-registration outcome.

Five complete-project diagnostics are retained, not all demanded closed here: the existing compound MEMBER type spelling in inline tiny fixedtables; the same unrelated scaffold family after included fixedtables lowering; nested typedef duplicate allocation order; repeated macro dimensions/wrappers; and the separately diagnosed quoted-header lookup boundary. The two genuine ownership losses are the acceptance blocker among this evidence.

Importer source-line plumbing is unchanged. The prior header-initializer/later-identical-call drift scenario remains unmeasured on this candidate; the root is collecting metadata controls separately. No LINE/COLUMN parity claim follows from these selected graph replays.
