# Plan 002: Version, validate, and transactionally save CPG files

> **Historical plan — do not execute on current HEAD.** The main implementation
> landed in commit `561b46b02`. A 2026-08-18 reconciliation found remaining
> decoded-memory, envelope-integrity, and post-publication contract gaps; those
> are tracked in `plans/006-complete-cpg-persistence-safety.md`.

> **Executor instructions**: Follow this plan step by step. Run every
> verification command. If a STOP condition occurs, stop and report; do not
> improvise. When done, update the status row for this plan in
> `plans/README.md` unless a reviewer dispatched you and told you to skip it.
>
> **Drift check (run first)**:
> `git diff --stat 6913b3ac1..HEAD -- cpg-rs/cpg-core cpg-rs/Cargo.toml cpg-rs/Cargo.lock`
> If an in-scope file changed, verify that the `CPG1` unversioned decoder and
> direct `std::fs::write` behavior described below still exist. STOP if the
> persistence contract was independently redesigned.

## Status

- **Status**: STALE (implemented historically; superseded by Plan 006)
- **Depends on**: none
- **Planned at**: commit `6913b3ac1`, 2026-08-13
- **Finding**: SECURITY-02 / CORRECTNESS-01

## Why this matters

Saved graphs are a documented user-facing interface (`build`, `--load`, MCP
cache, merge). The current decoder trusts file-controlled counts, symbol
indices, enum bytes, and edge targets. An invalid edge-kind byte or node target
can panic; large declared counts can request excessive allocations. Release
builds use `panic = "abort"`, so malformed input terminates the CLI or MCP
server. The fixed `CPG1` magic carries no explicit format version or integrity
check, and saves directly overwrite the destination, so an interrupted write
can destroy the last usable graph.

After this plan, new files use an explicit versioned envelope, corrupted or
oversized data is rejected before graph construction without panicking, legacy
CPG1 data is either safely read under a documented bounded compatibility path
or rejected with a stable migration error, and saves replace files atomically.

## Context and evidence

- `cpg-rs/cpg-core/src/persist.rs:10-13` explicitly says the format is not
  versioned.
- `cpg-rs/cpg-core/src/persist.rs:60-71` uses `self.pos + n` without checked
  addition.
- `cpg-rs/cpg-core/src/graph.rs:29` defines `MAGIC` as `CPG1`.
- `cpg-rs/cpg-core/src/graph.rs:368-445` casts file counts to `usize`, allocates
  from them, accepts symbol/file indices without range validation, and indexes
  `in_edges[e.other]` while rebuilding mirrors.
- `cpg-rs/cpg-core/src/schema.rs:108-110` implements `EdgeKind::from_u8` as a
  direct array index.
- `cpg-rs/cpg-core/src/graph.rs:449-458` saves with one `std::fs::write` and
  loads the entire file before decoding.
- `cpg-rs/cpg-core/src/lib.rs:60-97` covers only a valid round trip.
- `cpg-rs/Cargo.toml:34` sets release `panic = "abort"`.
- Error convention: the decoder returns `DecodeError(String)` and `Cpg::load`
  maps it to `io::ErrorKind::InvalidData`. Preserve that public error category.

## Scope

### In scope

- `cpg-rs/cpg-core/src/persist.rs`
- `cpg-rs/cpg-core/src/graph.rs`
- `cpg-rs/cpg-core/src/schema.rs`
- `cpg-rs/cpg-core/src/lib.rs` and/or a new `cpg-core/tests/persistence.rs`
- `cpg-rs/cpg-core/Cargo.toml`, workspace dependency declarations, and
  `cpg-rs/Cargo.lock` only if a small checksum/tempfile dependency is justified
- Focused test/fuzz seed files below `cpg-rs/cpg-core` if added

### Out of scope

- Source-language metadata and workspace source manifests (Plan 003)
- Memory-mapped or streaming query execution
- Cryptographic authentication of graphs. The checksum detects accidental
  corruption; untrusted-input safety comes from bounds and validation.
- General graph-schema redesign or Joern-compatible serialization

## Implementation steps

### 1. Define and document a bounded CPG2 envelope

Keep the graph payload's columnar semantics, but write a `CPG2` envelope with
fixed-width little-endian fields for at least:

- format version (reject unsupported values);
- declared payload length;
- checksum algorithm/version and checksum over the exact payload;
- optional reserved flags that must be zero until defined.

Define named limits for total file bytes and every decoded count class (strings,
nodes, edges, files, individual string/path bytes). Derive conservative values
that permit current benchmarks and real projects, document them next to the
constants, and reject before allocation when either a named ceiling or the
remaining-payload lower bound makes a count impossible. Use checked integer
conversion and arithmetic throughout.

Do not call a non-cryptographic checksum a security signature. Prefer a small,
maintained dependency compatible with Rust 1.97 if adding one; do not hand-roll
a complex hash.

Verification:

```sh
cd cpg-rs
cargo test -p cpg-core --locked envelope
```

Expected: a new valid round trip starts with CPG2; unsupported versions,
reserved flags, length mismatch, checksum mismatch, and trailing data all
return `DecodeError`/`InvalidData` rather than panic.

### 2. Make payload decoding fully validating

Change enum decoding to a checked API (`Option`/`Result`) and update the decoder.
Before committing a constructed `Cpg`, validate:

- every node kind and edge kind byte;
- every optional symbol index is within the decoded string table;
- every edge target is less than node count;
- file-table identifiers are unique and internally consistent;
- every node file identifier exists (or matches the precisely documented
  sentinel policy, if one already exists);
- liveness bytes use only defined values;
- `next_file` cannot collide with a decoded file identifier;
- the decoder consumes exactly the declared payload.

Avoid allocating all attacker-declared structures and then validating. Validate
counts against limits/remaining bytes first, validate values while reading into
temporary structures, and only rebuild derived indices after cross-references
are known valid. `Cpg::from_bytes` must not panic for any byte slice.

For CPG1 compatibility, choose one of these policies and document it in the
public rustdoc/tests:

1. Preferred: safely decode legacy CPG1 through the same bounded validation,
   while writing only CPG2.
2. If safe compatibility materially duplicates the decoder: reject CPG1 with
   an actionable `legacy CPG1 is unsupported; rebuild the graph` error.

Never retain the current unsafe CPG1 path merely for compatibility.

Verification:

```sh
cd cpg-rs
cargo test -p cpg-core --locked malformed
```

Expected: mutations for invalid enum bytes, symbol indices, file ids, edge
targets, maximum counts, overflow-shaped lengths, truncation at every field
class, and trailing bytes all return errors without unwind/abort.

### 3. Replace direct overwrite with same-directory atomic publication

Implement `Cpg::save` as:

1. serialize completely before touching the destination;
2. create a uniquely named, `create_new` temporary file in the destination's
   directory (not the system temp directory);
3. write all bytes, flush, and `sync_all` the file;
4. atomically replace/rename to the destination using a cross-platform approach
   that supports Linux, macOS, and Windows;
5. sync the parent directory where the platform supports it;
6. clean up the temporary file on every failure.

Do not delete the old destination before a replacement is ready. Preserve its
permissions if current behavior or tests establish a contract; otherwise use a
documented owner-writable default. Return the original I/O error with useful
path context and leave the previous graph readable.

Verification:

```sh
cd cpg-rs
cargo test -p cpg-core --locked atomic
```

Expected: saving over an existing graph yields the new valid graph; induced
pre-publication failures leave the old graph byte-for-byte intact; no temporary
file remains after failure; concurrent writers result in one complete valid
file, never a torn mixture.

### 4. Add broad corruption regression coverage

Follow the existing `Cpg` round-trip construction style in `cpg-core/src/lib.rs`.
Add table-driven tests that call `Cpg::from_bytes` directly for deterministic
mutations and use `catch_unwind` in debug tests as an additional assertion that
no malformed byte vector panics. Include empty input, every truncation point of
a small valid graph, bad magic/version/flags/checksum, impossible counts,
invalid UTF-8, invalid enum values, out-of-range references, duplicate file ids,
and extra bytes.

If adding a fuzz target is straightforward in this repository, add a
`from_bytes_never_panics` target and a seed corpus containing one minimal CPG2
and one legacy CPG1. Do not make installation of a fuzz runner part of normal
CI in this plan; the deterministic regression suite is mandatory.

Verification:

```sh
cd cpg-rs
cargo test -p cpg-core --locked
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cd ..
git diff --check
```

Expected: all commands exit 0. Commit with a repository-style message such as
`fix: harden persisted CPG files`.

## Test plan

- Preserve the current valid graph round trip and add explicit CPG2 assertions.
- Cover each envelope validation independently so one early checksum failure
  does not mask untested payload validation; recompute the checksum after
  deliberate payload mutations meant to reach inner fields.
- Exercise bounded-count failures before large allocations.
- Exercise atomic replacement, cleanup, and concurrent complete-writer behavior
  in filesystem tests.
- Run the full workspace because every CLI build/load/cache flow uses cpg-core.

## Done criteria

- [ ] New graphs have an explicit supported version and verified payload length/checksum
- [ ] Every file-controlled count and index is checked before use/allocation
- [ ] No byte slice can panic `Cpg::from_bytes`
- [ ] CPG1 has an explicit safe-read or explicit-rebuild policy
- [ ] Trailing, truncated, corrupt, oversized, or incompatible input is rejected
- [ ] Save publication is same-directory, durable where supported, and atomic
- [ ] A failed save preserves the previous destination
- [ ] Full formatting, locked Clippy, and locked workspace tests pass
- [ ] Only in-scope files changed
- [ ] `plans/README.md` status row updated, unless reviewer told executor to skip

## STOP conditions

- A supported external consumer writes a different CPG1 dialect not represented
  by this repository; report the compatibility requirement before choosing a
  CPG1 policy.
- Cross-platform atomic replacement requires deleting the destination first.
  Do not weaken the invariant; report the platform/API constraint.
- Current real-project graph sizes exceed the proposed named limits. Report
  measured sizes and revise the constants with evidence before implementation.
- The change requires modifying graph semantics outside serialization.

## Notes for the reviewer

Inspect allocation order, not only error checks. A decoder that returns an error
after `Vec::with_capacity(attacker_count)` still fails the resource-safety goal.
Also verify checksum tests penetrate payload validation by recomputing checksums
for mutations intended to reach inner fields.
