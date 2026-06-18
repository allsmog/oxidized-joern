Oxidized Joern
===

Oxidized Joern is a Rust-first fork of
[Joern](https://github.com/joernio/joern), the code analysis workbench built
around code property graphs (CPGs).

The goal of this fork is to migrate Joern's frontend and analysis pipeline
toward Rust while preserving the Joern user model: parse source code, emit CPGs,
and query them with the existing Joern tooling. This repository is not the
upstream Joern distribution; it is the rewrite track where Rust replacements are
landed, tested, and released incrementally.

## Current Status

The first released Rust rewrite component is the Go AST generator used by
`gosrc2cpg`.

- Rust `goastgen` release: `go-astgen/v0.2.0`
- Release page:
  `https://github.com/allsmog/oxidized-joern/releases/tag/go-astgen/v0.2.0`
- Release assets:
  `goastgen-linux`, `goastgen-linux-arm64`, `goastgen-macos`,
  `goastgen-macos-arm64`, and `goastgen-windows.exe`
- Scala `gosrc2cpg` integration is wired to download from this fork's release
  URL.

The C/C++ frontend still defaults to upstream Joern's Scala/Eclipse CDT
implementation. The Rust `cxxastgen` workspace under
`joern-cli/frontends/c2cpg/rust` is wired into the Scala frontend as an opt-in
oxidized backend via `--parser-backend oxidized`, with compatibility coverage
against the CDT backend.

The JavaScript/TypeScript frontend still defaults to upstream Joern's Babel
`astgen` binary. The Rust `astgen` workspace under
`joern-cli/frontends/jssrc2cpg/rust` is available as an opt-in oxidized backend
through `jssrc2cpg/jsAstGenBuildRust`; it emits the existing Babel-shaped JSON
contract plus TypeScript `.typemap` sidecars for the Scala frontend.

The Swift frontend still defaults to upstream Joern's SwiftSyntax-based
`SwiftAstGen` binary. The Rust `SwiftAstGen` workspace under
`joern-cli/frontends/swiftsrc2cpg/rust` is available as an opt-in oxidized
backend through `swiftsrc2cpg/swiftAstGenBuildRust`; it emits the existing
SwiftSyntax-shaped JSON contract for the first simple declaration/function
and expression slices.

## Rewrite Priorities

1. Keep the upstream Joern CLI and CPG behavior usable while replacing
   implementation pieces with Rust.
2. Replace language-frontends behind stable contracts before changing the user
   interface.
3. Verify every Rust replacement with compatibility tests, differential tests,
   real corpus tests, and release artifact smoke tests.
4. Move from component rewrites toward a Rust-native core only after frontend
   compatibility is boring and repeatable.

## Go Frontend / `goastgen`

The Rust `goastgen` workspace lives under:

```text
joern-cli/frontends/gosrc2cpg/rust
```

Useful commands:

```bash
cd joern-cli/frontends/gosrc2cpg/rust
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Run the optional legacy differential test when a reference `goastgen` binary is
available:

```bash
GOASTGEN_REFERENCE=/path/to/legacy/goastgen \
cargo test -p goastgen --test differential_json -- --nocapture
```

Run the Scala Go frontend suite against the locally built Rust binary:

```bash
sbt 'gosrc2cpg/goAstGenBuildRust' 'gosrc2cpg/test'
```

Release notes and compatibility details are in:

- [`joern-cli/frontends/gosrc2cpg/rust/README.md`](joern-cli/frontends/gosrc2cpg/rust/README.md)
- [`joern-cli/frontends/gosrc2cpg/rust/docs/json-contract.md`](joern-cli/frontends/gosrc2cpg/rust/docs/json-contract.md)
- [`joern-cli/frontends/gosrc2cpg/rust/docs/release-checklist.md`](joern-cli/frontends/gosrc2cpg/rust/docs/release-checklist.md)

## JavaScript/TypeScript Frontend / Rust `astgen`

The Rust JavaScript AST generator workspace lives under:

```text
joern-cli/frontends/jssrc2cpg/rust
```

Useful commands:

```bash
cd joern-cli/frontends/jssrc2cpg/rust
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Build and install the Rust binary into the location used by the existing
`jssrc2cpg` SBT project:

```bash
sbt 'jssrc2cpg/jsAstGenBuildRust'
```

Run the Scala frontend suite against the locally built Rust binary:

```bash
sbt 'jssrc2cpg/jsAstGenBuildRust' 'jssrc2cpg/test'
```

## Swift Frontend / Rust `SwiftAstGen`

The Rust Swift AST generator workspace lives under:

```text
joern-cli/frontends/swiftsrc2cpg/rust
```

Useful commands:

```bash
cd joern-cli/frontends/swiftsrc2cpg/rust
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Build and install the Rust binary into the location used by the existing
`swiftsrc2cpg` SBT project:

```bash
sbt 'swiftsrc2cpg/swiftAstGenBuildRust'
```

## Requirements

- JDK 21
- sbt
- Rust stable toolchain
- Go toolchain for Go dependency-resolution integration tests
- Optional: gcc and g++ for upstream C/C++ system-header discovery

## Upstream Joern

Joern remains the base project and compatibility target.

- Website: https://joern.io
- Documentation: https://docs.joern.io
- CPG specification: https://cpg.joern.io
- Upstream repository: https://github.com/joernio/joern

When in doubt, preserve upstream behavior first and make Rust-specific behavior
explicit in this fork's docs and tests.
