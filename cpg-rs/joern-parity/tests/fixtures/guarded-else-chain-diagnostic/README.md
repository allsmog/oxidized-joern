# Runtime else-if chain crossing a preprocessor boundary

This separate diagnostic package preserves two complete fresh Joern4.0.555 graphs and frozen seventh/eighth Rust outputs. It does not change Rust tests or the sealed body-preprocessor fixture package.

`ordinary_chain` is a replayed complete exact control in both Rust snapshots, **not a new production gate**. `guarded_chain` remains nonexact in both snapshots. Together the complete references contain 668 canonical lines including separators /658 nonempty selected records.

The valid guarded source conditionally includes an `if (...) ... else`, closes `#endif`, then continues with another `if`. Joern joins the runtime else-if chain. Seventh Rust omitted the guarded first branch; eighth Rust emits it but leaves the subsequent `if` as a sibling. This is partial AST/control support and does not establish full preprocessing support.

Within the reduced `choose` method, all previously Joern-matching CFG/reaching-definition **source-occurrence** facts are retained: zero lost and 23 gained. Five live facts remain absent, all involving occurrences in the newly added first branch: the CFG edge from `result = x` to the returned `result`, two first-`base` exit RD facts, and two first-assignment-`result` RD facts. The current first assignment instead reaches the second condition. The full raw graphs are retained, including this wrong edge.

Three previously matching raw ordinal lines disappear. They were coincidences between different endpoints: baseline `result`/literal0 occurrences used the same ordinals as Joern's second-condition `base`/literal10 occurrences. This is not a claim of zero raw-line changes. `provenance/source-occurrence-review.json` retains every node, full ancestors, every compared edge, raw counters and all five missing facts. Endpoint identity ignores ORDER and uses the nearest CALL/RETURN owner to disambiguate repeated leaves; every used endpoint key is unique.

The source boundary is in the integrated eighth candidate `exact.rs` SHA256 `8b132fab7d079a97e774b10d5b1f14c1a1df4f093ef8f437ebfcaadfdde33b3b`: `kept_preproc_children` at 7759 returns parser children; `emit_stmt` at 3084 emits them separately; `emit_if` at 3357 follows only the parsed alternative field. There is no source-aware reconstruction joining the dangling runtime `else` across the directive boundary. No implementation is included.

`measurement.json` binds the complete source/reference/output/diff bytes. Raw transcripts and both frozen release source/binary bindings are under `provenance/`. The ordinary control does not replace the guarded diagnostic, and the unrelated macro-wrapper differences in whole Lua `math_log` are outside this reduction.
