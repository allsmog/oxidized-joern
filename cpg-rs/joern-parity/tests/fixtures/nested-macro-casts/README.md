# Nested macro type arguments and cast rendering

Five isolated C projects retain complete selected graphs from Joern v4.0.555:
3,157 canonical lines including separators, or 3,090 nonempty selected records.
The accepted `862e54ee` baseline matches one full project; the candidate matches
all five. The projects contain 24 source functions. The first three projects
alone contain 2,236 canonical lines / 2,189 nonempty records; the earlier 2,236
“selected records” label counted separators and is corrected here.

Function-macro arguments are preprocessing tokens. Parsing
`cast(union GCUnion *, (o))` as an ordinary C expression first recovers an
ERROR/comma expression; parsing `((rawtt((o))) & 0x0F)` first can recover a cast.
The candidate recognizes known function-macro invocations before that C parse.
Quotes/comments are opaque to delimiter recognition, and existing macro
source environments, disabled names and work budgets remain in force.
Expanded cast CODE uses the measured descriptor spelling while TYPE_FULL_NAME
retains its separate meaning. Ordinary source-cast rendering is unchanged.

The tests cover union/struct/enum and primitive casts, a typedef alias,
qualifiers, rawtt-style tag tests, ordinary similarly named calls, conditional
arguments, disabled self-recursion and quoted macro-looking text. Independent
review exposed two introduced defects: matching the `M` suffix inside the
ordinary Unicode identifier `éM`, and overlapping descriptor replacements when
an array bound contains a cast. Both are fixed and have complete production
regressions. Composite descriptors are rendered once, including Joern's omitted
array bounds, separated pointer tokens and preserved pointer qualifiers;
nested casts in the value expression still render independently.

`cases/` preserves each complete live graph, source, frozen accepted-baseline
output and full difference. `oracle/` preserves raw live stdout/stderr, command
receipts and the unchanged selected-projection script. Only the AST transport
prefix is removed. No graph facts are filtered; the shared newline transport
remains noninjective. `measurement.json` binds the final source/release binary,
input/reference hashes, checks, prior failed receipts and full Lua replay.

The first public-test replay also exposed repeated rescanning of an unchanged
self-recursive function-macro name. Rescanning now requires a changed callee
supplied by an object macro. The normal test-thread replay passes; its original
stack-overflow log remains bound in the measurement. The review diagnostics
retain the original Unicode failure and composite abort. The original composite
and expanded probes additionally contain `union U;`: their complete final
graphs remain nonexact because the forward-tag scaffold is missing, a separate
pre-existing declaration gap. Their full sources, live references, accepted
baseline and candidate outputs/differences remain under `diagnostics/review-*`.
The exact composite fixture does not need that separate forward declaration:
its pointer casts introduce the incomplete tag directly.

The Lua files in `diagnostics/` are complete method AST excerpts, excluding the
empty method separator; they are explicitly not complete project graphs.
Complete unfiltered before/live/after project outputs are bound by path/hash.
The nested cast operands and rawtt subtrees are repaired. `luaD_seterrorobj`
still differs in sizeof spacing and luaS_newlstr's return type;
`luaG_concaterror` retains l_noret return-type differences. Full Lua or full
Joern parity is not claimed.

Final validation passes 417 workspace tests, including all five production
Project.build comparisons, formatting, strict workspace/all-targets Clippy,
and 308/308 committed plus fresh live main-corpus blocks. Run the focused
production checks from `cpg-rs` with
`cargo test --locked -p joern-parity --test nested_macro_casts`.
