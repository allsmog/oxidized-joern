# Primitive MEMBER spelling

This family retains 19 complete graphs from the pinned c2cpg oracle: 17 new
projects and two unchanged tiny-fixedtables anchors. All 19 references were
admitted together after source, oracle, runtime, JDK, checkpoint and retained
anchor checks. Source and reference bytes are unchanged.

The candidate changes only the base type selected for `field_declaration` in
`exact.rs`: it calls the existing declaration-role renderer. The enum path,
named-type fallback, declarator suffixes, member NAME/CODE/ORDER, `<clinit>`
construction and other type roles are unchanged.

## Measured results

Against accepted tenth source `f235a0f898c4e19fda90998b903f57741b776ec7`,
the 19 projects improve from 2 to 18 complete exact graphs. They retain 3,423
nonempty canonical records (3,493 LF lines), gain 126
live-matching nonempty records and lose none. The production tests in
[primitive_members.rs](../../primitive_members.rs) gate 16 new complete
graphs and the two retained anchors.

| Source field base | Observed TYPE_FULL_NAME base |
| --- | --- |
| `unsigned short`, `short unsigned` | `shortunsigned` |
| `unsigned short int`, `short int unsigned` | `short unsigned int` |
| `signed short` | `shortsigned` |
| `signed char` | `signedchar` |
| `unsigned char` | `unsigned char` |
| `unsigned long long int` | `longlong unsigned int` |
| `const unsigned short` | `shortunsigned` |
| `volatile unsigned short` | `volatile shortunsigned` |

Pointer and array fields retain `*` and `[3]` suffixes. Base and pointer `const`
positions both produce `shortunsigned*`; their distinct `*value` and `* const
value` CODE remains unchanged. Mixed declarations retain independent scalar,
pointer and array declarators.

`nonprimitive_scalar_control` remains a complete, byte-identical diagnostic:
all MEMBER rows match, but 20 type/scaffold records are absent. Its reference,
accepted output, candidate output and complete differences are retained. It
is excluded from complete-parity gates. The older array-initializers
`member_types` diagnostic is also byte-identical to the accepted output.

## Preservation and validation

All 228 planned prior case instances were replayed with the accepted tenth and
frozen candidate binaries, together with the older `member_types` diagnostic.
The 39 primitive-role, 26 typedef-aggregate, 69 array-initializer and four
array-dimension graphs remain complete exact and byte-identical. The 14
body-include cases improve from 10 to 12 exact only through the same two
retained anchors. All 76 body-macro-state outputs are unchanged: 55 are exact,
20 are successful diagnostics and `duplicate_clinit_tag` aborts with exit
`-6` on both release producers. Both failure transcripts are retained; failed
runs are excluded from successful-pair matching-record claims.

The 227 successful prior pairs plus the older `member_types` diagnostic lose
no matching nonempty records. Their 14 gained records belong to the two
duplicated anchors and must not be added again to the 19-project gain count.
No reference or checker was normalized or edited.

The final worker checks passed: 21 focused Rust tests, the unchanged 308-block
main committed oracle, formatting and strict Clippy for `cpg-lang-c` and
`joern-parity` with all targets. The initially frozen test draft listed all
17 new cases; full replay established the nonprimitive diagnostic, so the
final test excludes that case. The initial draft remains in provenance, with
the test-only change bound by the final validation receipt. Production source
and the release binaries stayed frozen throughout the replays.

Root integration, whole-project comparisons and final resource acceptance are
pending. This family establishes the measured field-base spelling and retained
roles; it does not close nonprimitive scaffolding, header lookup or fixedtables
identity differences.

## Evidence layout

- `cases/`: unchanged complete sources and expected graphs, including the
  diagnostic and the two anchors.
- `provenance/oracle-run/`: raw live stdout/stderr, admitted run receipts and
  exact launched source/oracle snapshot, with runtime/JDK inventories.
- `provenance/accepted-baseline/` and `provenance/replay19/`: complete accepted
  and candidate outputs, statuses and differences.
- `provenance/prior-replay/`: all 229 prior runs with both producer outputs,
  full differences, failed output and multiplicity counters.
- `provenance/frozen-v1/` and `provenance/validation/`: source/build/binary
  hashes, frozen initial test, final test, build/check logs and commands.
- `provenance/reviews/`: independent reference admission and measured baseline
  facts; later source/package reviews may be added separately.
- `measurement.json` and `provenance/copy-manifest.json`: portable index and
  filename/byte bindings back to the retained original artifacts.

Historical command receipts retain their original absolute paths. The copied
evidence is addressed by relative paths in the copy manifest; the production
test uses only the fixture tree. Runtime executables and compiled Rust binaries
are not duplicated into this repository.
