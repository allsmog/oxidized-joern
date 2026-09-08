# Pinned default C macros

The pinned Joern v4.0.555 C frontend supplies `__STDC__=1`,
`__STDC_VERSION__=199901L`, and `__STDC_HOSTED__=1` before processing
source directives. This setup does not define `__GNUC__` or `__cplusplus`.
Source `#undef` removes a default and `#define` replaces it. These defaults
feed conditional selection, declaration expansion and body macro expansion.

Direct uses emit the same INLINED CALL, expansion BLOCK and LITERAL as other
object macros. The synthetic METHOD has an explicitly empty CODE property for
a builtin, and the actual directive for a source redefinition. Builtins inside
another macro expand to literals within that macro's expansion. Numeric literal
macro roots use the literal's suffix type; parenthesized expression wrappers
retain their independently observed ANY type.

All 25 projects retain complete raw producer output and scripts, unchanged
source bytes, full expected graphs, baseline and candidate graphs and complete
diffs. Eleven baseline graphs were exact; all 25 are exact after the repair:
5,344 canonical lines including separators and 5,192 nonempty selected records.
The production test compares every selected AST, NODES, EDGES and FLOWS record.
Raw output also preserves location metadata. This selected text projection is
not a lossless serialization of all Joern graph properties.

`measurement.json` binds the frozen candidate and baseline. `provenance/` also
retains the earlier 22/25 candidate and actual pinned JAR bytecode evidence;
those earlier comparisons remain historical evidence. The defaults are bounded
to this verified C configuration and do not establish all CDT/GNU builtin or
compiler build-context parity. Whole-project acceptance is reported separately.
