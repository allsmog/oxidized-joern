# Streaming JSON export measurement

The JSON serializer now borrows the existing graph and serializes node and edge sequences directly into the writer. It preserves the previous alphabetical object keys, node/edge order, null properties, escaping, pretty indentation and absent trailing newline. Existing node/edge selection and its compact ID/edge vectors are unchanged.

All eight whole-graph exports (two repetitions per implementation for each project) match the saved frozen-a14 JSON byte for byte:

| Saved graph | Nodes | JSON bytes | Before peak RSS | Streaming peak RSS |
| --- | ---: | ---: | ---: | ---: |
| zlib 1.3.1 core | 61,974 | 45,488,869 | 822.63–823.38 MiB | 48.30–48.44 MiB |
| Lua 5.4.7 | 108,247 | 73,247,250 | 1332.86–1334.38 MiB | 69.83–69.92 MiB |

These are per-command macOS `wait4` high-water measurements against the same saved graph files. They are not cumulative child-process resource values. Export times were 0.327–0.613 s before and 0.103–0.526 s after for zlib, and 0.665–0.667 s before and 0.220–0.227 s after for Lua. The graph builder, acceptance budgets and references are unchanged.

The 72 CLI tests passed, including the existing export integration tests and four new tests pinning exact empty/escaped/null JSON bytes and partial-write error propagation. Strict Clippy and formatting passed. The new serializer retains an underlying writer's I/O error kind. Independent review reproduced a failure in the first streaming draft: buffering its final tail could mask a late write failure. The final production dispatch explicitly flushes before reporting success or updating statistics, with a regression test for a failure that occurs only on flush.

`measurement.json` binds the input graph hashes, all prior/current export hashes, binaries, source, replay script and validation logs. The binaries and large outputs remain at their receipt paths rather than being duplicated in Git. Replay from the recorded worktree using `python3 .local/stream-export/final/measure.py` after moving or removing its prior `measurements` output directory; the script refuses an unsuccessful producer or changed export hash.
