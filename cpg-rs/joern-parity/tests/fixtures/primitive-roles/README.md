These fixtures pin Joern v4.0.555's primitive C type spellings separately for
definition returns, prototype returns, parameters, locals and call results.
Each directory is imported as a separate project. An unrelated declaration
therefore cannot hide an extra or missing TYPE node in a shared type pool.

The 35 primitive cases include signed and unsigned integer widths, explicit
`int` spellings, reordered specifiers, floating types, `_Bool`, `const` and
`volatile`. The pointer fixture covers return pointers, qualifiers,
function-pointer objects and function-pointer parameters. Every `expected.txt`
contains the complete AST, NODES, EDGES and FLOWS projection from the live
oracle, with only the `AST|` transport prefix removed.

Three independent review fixtures also pin parenthesized function calls,
dereferenced callback parameters and a loop-local callback that shadows an
outer prototype. The 39 isolated graphs contain 12,523 selected records;
complete agreement improves from 6 to 39 graphs against baseline `c5ea712db`.
The existing main corpus remains unchanged and passes all 297 comparison
blocks with both its committed reference and a fresh live oracle.

The differing spellings are observable graph properties, rather than a choice
of preferred C syntax:

| Source type | Definition return | Declaration type | Call result |
| --- | --- | --- | --- |
| `unsigned long` | `unsigned long` | `longunsigned` | `unsigned longint` |
| `unsigned long int` | `unsigned longint` | `long unsigned int` | `unsigned longint` |
| `_Bool` | `bool` | `_Bool` | `_Bool` |
| `volatile unsigned long` | `volatile unsigned long` | `volatile longunsigned` | `volatile unsigned longint` |

A function-pointer parameter retains its declaration spelling, even though
an indirect call resolves its result independently. A function-pointer object
uses the expression spelling for its return and parameter components.
Base `volatile` survives; base `const` and qualifiers on the pointer itself
are absent from these selected type properties in the live reference.

[measurement.json](measurement.json) records the input and reference hashes
and the before/after full-graph comparisons. Run the public regressions with:

```sh
cd cpg-rs
cargo test --locked -p joern-parity --test primitive_roles
```

To regenerate each isolated graph, set `JOERN` to the pinned executable,
set `JAVA_HOME` to JDK 21, then run from the repository root:

```sh
fixture="$PWD/cpg-rs/joern-parity/tests/fixtures/primitive-roles"
scratch=$(mktemp -d)
(cd "$scratch" && "$JOERN" --script "$fixture/isolated-oracle.sc" \
  --param inputPath="$fixture" > live.stdout 2> live.stderr)
```

The driver uses the unchanged projection from `joern-parity/oracle.sc` inside
a loop that opens a fresh project per directory. `CASE|directory` separates
graphs; select every AST, NODES, EDGES and FLOWS record within that case,
remove only the leading `AST|`, then compare the result with its `expected.txt`.

This does not establish full C type parity. Typedef expansion, aggregate and
cast type rendering, literal typing, header resolution and macro-wrapper
inference remain outside this change. The existing extra decl-specifier TYPE
registration for an uninitialized `unsigned char` local or global also
remains: `void f(void) { unsigned char value; }` adds a Rust `unsigned` TYPE
that is absent from Joern. Initialized declarations in this suite separately
pin the registration Joern actually emits for those expressions.

A separate diagnostic using function-pointer initializers from method
references has correct type properties after this change, but retains a
preexisting two-edge reaching-definition mismatch at method exit. Its full
live output and diff are retained with the sprint's scratch receipts; it is
not counted among the conformant fixtures above.
