# Primitive MEMBER oracle runner draft

Status: prepared, unrun, parent hold active. This copies the frozen V2 primitive-member preparation without changing any of its 17 new projects, two retained anchors or 20 source files. No new expected graph or Rust/test edit is present. The canonical oracle is byte-identical to the reviewed array-dimension oracle.

The runner retains the existing complete Joern/JDK trees, executable paths, modes, symlink identities, launcher commands and override checks before and after each attempt. Host dylibs/frameworks remain an explicit host boundary; this is not a hermetic OS claim. `runtime-bindings.json` is inherited byte-for-byte and checked against the actual installed runtime during preparation-only verification.

Only two adaptations are made: both retained tiny-fixedtables anchors must match complete extracted bytes, and a separate parent release must name the committed tenth acceptance/report/metrics and the pinned final tenth build. The accepted tenth source/docs IDs are both f235a0f898c4e19fda90998b903f57741b776ec7, checked as actual Git objects and against all138 frozen input blobs. Producer release remains absent. Actual git objects, ancestry, frozen input blobs and passed gates remain mandatory. A prepared/verify-only success is not producer authorization.

After static review, the parent creates a separate release using `checkpoint-release.template.json`, with status `PARENT_APPROVED_PRIMITIVE_MEMBER_PRODUCER_RELEASE`, producerHoldReleased=true and the exact prepared-manifest hash. The parent must explicitly supply that receipt's SHA to the launch command. Do not edit this sealed preparation to insert approval.

Preparation check only: `python3 run-oracle.py --verify-only`.

Future parent-owned command (not authorized or run): `python3 run-oracle.py --producer-hold-released --checkpoint-receipt ABS_PARENT_RECEIPT --approved-receipt-sha256 EXACT_PARENT_APPROVED_SHA --run-name first-complete-primitive-member-reference-batch --timeout-seconds 180`.

The run name must be new. Every launched attempt keeps raw stdout/stderr, process status, timeout, snapshots and selected bytes. Any process/inventory/checkpoint/anchor failure remains HELD_NON_REFERENCE_BATCH; selected output is saved as non-reference diagnostics, never as expected graphs. Success is promoted only after the final before/after comparison. Extraction preserves LF-only spelling, whitespace, repeated lines and all selected AST/NODES/EDGES/FLOWS records; only the canonical AST prefix is removed. The two unchanged anchors are provenance controls, not extra new production gates.

Static review must confirm the small runner patch, both anchor checks, the committed-tenth admission contract, actual runtime/preparation bindings, unchanged oracle and input bytes, and failure-path retention. No renderer behavior or expected member spelling is inferred by this runner.

V1 remains intact with absent IDs. V2 pins the now-committed atomic tenth checkpoint and adapts only its actual acceptance field names; committed source identity is proved from frozen file hashes, not a self-referential commit string in the docs. The release template remains unapproved.

V3 closes the actual tenth binary schema: name-to-hash values resolve to the two executables beside the already-bound frozen build receipt. V2 offline Git verification stopped at that inherited field mismatch before any producer; its source and failure log remain preserved.
