# Concatenated string expressions

`strings.c` is a reduced regression for the Lua 5.4.7 build panic exposed
when standalone blocks became visible. In `lstrlib.c:addliteral`, the true
operand of a conditional contains `"0x%" LUA_INTEGER_FRMLEN "x"`.
Tree-sitter represents this as `concatenated_string`; the previous emitter
omitted it and the CFG builder then indexed a nonexistent third operand.
The frozen `5ccc4b0fa` executable also aborts on this fixture. The repair
emits the expression, preserving all conditional operands and CFG branches.
It does not add a conditional-child guard or change the CFG/RD algorithms.

The complete **1,589-line** selected canonical projection (1,557 nonempty records) in `expected.txt`
matches fresh Joern **v4.0.555** under JDK **21.0.12**. This includes every AST,
NODES, EDGES, and FLOWS record produced by `joern-parity/oracle.sc`; no facts
were filtered or expected graphs hand-edited. This is not every Joern property.
The fixture covers narrow, L/u8/u/U and mixed narrow/wide prefixes;
parenthesized, call, return, ternary, and standalone-block contexts; comments;
escaped quotes/backslashes; multiline source; and object/function macro
replacements. Joern types these string nodes as `char*`, including prefixed
strings. Source literals retain their original CODE. Macro expansion
literals join their token contents and retain an encoding prefix. A direct
concatenation replacement types its macro wrapper `char*`; a parenthesized
replacement retains Joern's `ANY` wrapper type.

The three production tests compare the entire graph, require all three Lua
conditional operands and both CFG branches, and check the literal's source
line plus a multiline string and the call following it. The existing main
308-block reference and all 14 standalone-block scanner outcomes remain
unchanged. A native release build of the complete supplied Lua source tree
also succeeds; exact measurements and hashes are in `measurement.json`.
The whole Lua graph is not claimed equivalent to Joern.

## Text transport

The shared oracle now escapes embedded LF in a REACHING_DEF VARIABLE as
literal `\n`, matching its existing AST/CODE and Rust transport. Previously
an embedded LF split one fact across physical output lines. The change does
not filter graph facts. The existing text encoding is not injective: a
physical LF and a literal backslash followed by `n` can have the same escaped
spelling. CR is not escaped. Preserve bytes when extracting records; Python
`read_text()` and `splitlines()` can normalize or split CRLF unexpectedly.
`crlf-diagnostic/` retains the full source, Joern, baseline Rust, candidate
Rust, and unfiltered diff. Its copied `transport-proof.json` records the
original reviewer directory for its relative raw-receipt paths.
Independent byte-preserving probes confirm a preexisting CRLF mismatch:
Rust retains CR in CODE while Joern normalizes it. Literal backslash-n and
physical LF are therefore tested as distinct raw inputs without claiming
lossless transport or CRLF graph parity. The review receipt and raw-value
probe are linked in `measurement.json`. This bounded repair does not migrate
the canonical serialization format. The fixture's `.gitattributes` preserves
CRLF diagnostic bytes and exempts only inert unified-diff context lines from
whitespace lint; graph records remain untouched.

## Retained diagnostics

`unresolved-diagnostic/strings.c` includes an undefined identifier between
string tokens. CDT rejects that initializer and retains an unknown local;
tree-sitter accepts it as a concatenated expression. Its entire live graph,
Rust graph, and diff are retained. This malformed/incompletely preprocessed
case is **not exact** and is not silently removed to make a conditional safe.

`macro-diagnostic/strings.c` is the larger exploratory matrix. Its full
1,730-line live projection, Rust output, and unfiltered diff retain two
separate macro limitations: a comment inside a string macro definition can
prevent registration, and a declared-call-root replacement can receive the
callee's result type instead of Joern's `ANY` wrapper. These are outside the
string-expression repair. More general recursive macro expansion and macro
argument projection are not claimed complete. Independent baseline-bound
probes retain the extra Rust literal argument of a string-valued function
macro, incomplete nested object expansion, and a macro definition truncated
by comment-like string contents. None is claimed fixed; complete before/live/
after receipts are linked in `measurement.json`.

## Replay

Use a unique oracle working directory, with the pinned Joern executable in
`JOERN` and JDK 21 on `PATH`:

```sh
fixture="$PWD/cpg-rs/joern-parity/tests/fixtures/concatenated-strings"
scratch=$(mktemp -d)
(cd "$scratch" && "$JOERN" --script "$fixture/../../../oracle.sc" \
  --param inputPath="$fixture/strings.c" > live.log 2>&1)
sed -n -e 's/^AST|//p' -e '/^NODES|/p' -e '/^EDGES|/p' -e '/^FLOWS|/p' \
  "$scratch/live.log" > "$scratch/graph.txt"
diff -u "$fixture/expected.txt" "$scratch/graph.txt"
cargo test --locked --manifest-path cpg-rs/Cargo.toml \
  -p joern-parity --test concatenated_strings
```

The two diagnostic subdirectories can be replayed the same way, retaining
their complete differences. `measurement.json` binds every tracked fixture
and reference to the saved local raw receipts.
