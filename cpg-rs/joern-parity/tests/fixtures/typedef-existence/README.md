# Typedef existence after macro expansion

This package pins a bounded compiler-context repair against Joern v4.0.555.
The frontend must expand a typedef using the macro definitions visible at that
statement before deciding whether its declarator establishes a type name.
`typedef UNKNOWN U;` establishes `U`, even when its underlying type cannot be
resolved. `typedef unsigned UNKNOWN U;` does not establish the intended `U`.
Tree-sitter accepts the latter without an ERROR node, so the context collector
also rejects a sized type specifier containing a type identifier. Fresh controls
include signed/long unknown types, a known alias after `unsigned`, `__int128`,
ordinary primitive types, qualifiers, function pointers and a local declaration.

The implementation affects type-name snapshots for source declarations, supplied
headers and local/temporary expression trees. It does not change declaration AST
recovery, type scaffolding, reaching definitions or the independent underlying
header-call return-type resolver. Unknown ordinary aliases remain valid casts;
valid outer casts can contain unresolved inner pointer calls.

All 18 projects retain complete raw Joern output, source bytes, expected graphs,
before/candidate graphs and complete diffs. Seven complete graphs were exact
before and remain exact. Eleven remain explicitly nonexact diagnostics, including
invalid-declaration recovery, unresolved-alias scaffolding, and the two supplied
Lua-header projects. No expected records are removed to assert a narrower result.
The production test gates all seven exact graphs and separately checks the
live-derived cast/pointer-call names and CODE for source methods in all 18 cases.
These scoped assertions do not claim complete graph equality for diagnostics.

`raw/regression` contains the eight initial cause-isolation projects, including
unchanged supplied Lua headers. `raw/boundaries` contains ten fresh primitive and
alias controls. `measurement.json` records every CASE mapping, source/reference/
output hash, status and record count. Expected files are the complete CASE
projection: AST's transport prefix alone is removed; all AST, NODES, EDGES and
FLOWS records remain. Joern's selected text encoding is not a lossless encoding
of arbitrary source properties.

The source-only patch is relative to the frozen combined sixth-batch source
1603202779e066f2a9039f2fa3b075ab5722d9bb806be885e90f0b7c43a26d0d.
The candidate source is 76f0c8bc15a6bff023659427ccc057c4b3f38ec2d9343d11cbc3c24f2b424948;
importer source stays 3fb6695576fe87e463b5e44160bcbe828321bf32149d67abcc2256eb88d11aba.
The standalone earlier macro-cast package remains unchanged.

Validation: 117 prior C/frontend and parity tests, including all 32 prior exact
cast projects and 13 exact combined context projects; two new tests; the unchanged
308-block committed corpus; formatting and strict Clippy. The complete Lua replay
is retained by hash in `validation/lua-comparison.json`. It restores the intended
pointer-call classification in fitsC/isCint and the inner calls in intarith and
luaV_shiftl, preserving valid outer lua_Integer casts. It has no old-exact method
losses. That replay still exposes a separately owned header registry error:
lua_Unsigned is treated as a function returning LUAI_FUNC. This package does not
claim those complete Lua methods or the whole project are exact.
