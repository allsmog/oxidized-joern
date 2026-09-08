# Inactive declaration macro context

Pinned Joern retains file-scope declarations in inactive preprocessor branches. Their expressions still use macros defined earlier in the executed translation unit. An inactive `#define`, `#undef`, or `#include` does not mutate that state. Definitions made in an active branch are visible when a later inactive branch's declarations are retained.

The repair records immutable macro/type-name snapshots for retained inactive declarations and aggregates without executing their directives or introducing their typedef bindings. Callable prototype lookup separately uses the active items from the existing include-aware traversal. Inactive prototypes remain in the declaration graph and do not become active callable bindings. Inactive function-body lowering is outside this change.

The 17 complete isolated projects contain 2,839 canonical lines including separators, or 2,758 nonempty selected records. Two full projections matched frozen root source `58d287bf...`; 11 match the candidate. No previously exact project lost equality. The production test compares all 11 complete AST/NODES/EDGES/FLOWS references through `Project::build`.

Six complete diagnostics remain:

- `builtin_branch`, `literal_inactive`, `inactive_define`, `inactive_include`: macro CALL expansion is repaired; macro-valued array extent TYPE spellings still differ (`int[N]` versus `int[3]`).
- `later_define`: a later definition is correctly unavailable to the earlier inactive use; existing array extent and unknown global phantom-local differences remain.
- `active_undef`: an active undefinition takes effect before an inactive else. Its full output is unchanged from the baseline and retains an existing unknown global/array scaffold difference.

The second test asserts macro-state properties in those diagnostics. It does not claim their complete graphs are exact. Source, full reference, before/current output, and unfiltered differences remain for every case.

`measurement.json` binds all input/reference/output hashes, four raw oracle groups, source and binary snapshots, and the independent review. The oracle is Joern v4.0.555; its selected projection is unchanged. The extra review transcript includes optional location records outside that projection. Newline escaping in the shared transport is non-injective.

The supplied-header switch controls prove callable activation uses caller include context. Independent aggregate/typedef controls verify inactive declarations retain their expression context without adding inactive typedefs to subsequent active code.

Whole-project replay retains full unmodified-input outputs locally. Zlib exact METHOD ASTs increase from 145 to 149 of 410 live methods, restoring both `LENGTH_CODES:int(0)` and `MAX_MATCH:int(0)` stubs, with no exact-method loss. Lua retains all 1,431 previously exact METHOD ASTs out of 2,274 live methods. These are method AST comparisons, not whole-graph parity or a final semantic-retention verdict.
