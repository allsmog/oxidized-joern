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

The Rust frontend now builds the local Rust `rust_ast_gen` by default when the
Scala `rust2cpg` project compiles or tests. Set `RUST2CPG_ASTGEN_LEGACY=1` to
use the downloaded upstream `rust-astgen` host artifact instead. The
`rust2cpg` differential against the upstream reference binary and
`sbt "rust2cpg/test"` are CI gates. Fork release assets for `rust-astgen` have
not been published yet.

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

## Rust Frontend / `rust_ast_gen`

The Rust AST generator workspace lives under:

```text
joern-cli/frontends/rust2cpg/rust
```

Useful commands:

```bash
cd joern-cli/frontends/rust2cpg/rust
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Run the differential test when a reference `rust_ast_gen` binary is available:

```bash
RUSTASTGEN_REFERENCE=/path/to/reference/rust_ast_gen \
cargo test --test differential_json -- --nocapture
```

Run the Scala Rust frontend suite against the locally built Rust binary:

```bash
sbt 'rust2cpg/rustAstGenProvision' 'rust2cpg/test'
```

Use the upstream reference binary instead of the local build when debugging
legacy behavior:

```bash
RUST2CPG_ASTGEN_LEGACY=1 sbt 'rust2cpg/rustAstGenProvision' 'rust2cpg/test'
```

Release notes and compatibility details are in:

- [`joern-cli/frontends/rust2cpg/rust/docs/json-contract.md`](joern-cli/frontends/rust2cpg/rust/docs/json-contract.md)
- [`joern-cli/frontends/rust2cpg/rust/docs/release-checklist.md`](joern-cli/frontends/rust2cpg/rust/docs/release-checklist.md)

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

joern>
```

If the installation script fails for any reason, try
```
./joern-install --interactive
```

## Development Requirements
- [java](https://jdk.java.net/)
- [sbt](https://www.scala-sbt.org)

## Run unit and integration tests locally
Unit tests:
```bash
sbt test
```

Integration tests:
```bash
sbt joerncli/stage querydb/createDistribution
python -m pip install requests pexpect # wexpect on Windows
python -u ./testDistro.py
```

There is an experimental partial Bazel build setup. Check bazel.md for further details.

## Docker based execution

```
docker run --rm -it -v /tmp:/tmp -v $(pwd):/app:rw -w /app -t ghcr.io/joernio/joern joern
```

To run joern in server mode:

```
docker run --rm -it -v /tmp:/tmp -v $(pwd):/app:rw -w /app -t ghcr.io/joernio/joern joern --server
```

Almalinux 9 requires the CPU to support SSE4.2. For kvm64 VM use the Almalinux 8 version instead.
```
docker run --rm -it -v /tmp:/tmp -v $(pwd):/app:rw -w /app -t ghcr.io/joernio/joern-alma8 joern
```

## Releases
A new release is [created automatically](.github/workflows/release.yml) once per day. Contributers can also manually run the [release workflow](https://github.com/joernio/joern/actions/workflows/release.yml) if they need the release sooner.

## Developers

### Contribution Guidelines

Thank you for taking time to contribute to Joern! Here are a few guidelines to ensure your pull request will get merged as soon as possible:

* Try to make use of the templates as far as possible, however they may not suit all needs. The minimum we would like to see is:
    - A title that briefly describes the change and purpose of the PR, preferably with the affected module in square brackets, e.g. `[javasrc2cpg] Addition Operator Fix`.
    - A short description of the changes in the body of the PR. This could be in bullet points or paragraphs.
    - A link or reference to the related issue, if any exists.
* Do not:
    - Immediately CC/@/email spam other contributors, the team will review the PR and assign the most appropriate contributor to review the PR. Joern is maintained by industry partners and researchers alike, for the most part with their own goals and priorities, and additional help is largely volunteer work. If your PR is going stale, then reach out to us in follow-up comments with @'s asking for an explanation of priority or planning of when it may be addressed (if ever, depending on quality).
    - Leave the description body empty, this makes reviewing the purpose of the PR difficult.
* Remember to:
    - Remember to format your code, i.e. run `sbt scalafmt Test/scalafmt`
    - Add a unit test to verify your change.

### IDE setup

#### Intellij IDEA
* [Download Intellij Community](https://www.jetbrains.com/idea/download)
* Install and run it
* Install the [Scala Plugin](https://plugins.jetbrains.com/plugin/1347-scala) - just search and install from within Intellij.
* Important: open `sbt` in your local joern repository, run `compile` and keep it open - this will allow us to use the BSP build in the next step
* Back to Intellij: open project: select your local joern clone: select to open as `BSP project` (i.e. _not_ `sbt project`!)
* Await the import and indexing to complete, then you can start, e.g. `Build -> build project` or run a test

#### VSCode
- Install VSCode and Docker
- Install the plugin `ms-vscode-remote.remote-containers`
- Open Joern project folder in VSCode
  - [Option 1](https://docs.microsoft.com/en-us/azure-sphere/app-development/container-build-vscode#build-and-debug-the-project): Visual Studio Code detects the new files and opens a message box saying: `Folder contains a Dev Container configuration file. Reopen to folder to develop in a container.`. Select the `Reopen in Container` button to reopen the folder in the container created by the `.devcontainer/Dockerfile` file.
  - Option 2: press `Ctrl + Shift + P` then select `Dev Containers: Reopen in Container`
- Press `Ctrl + Shift + P` then select `Metals: Import build`
- After `Metals: Import build` succeeds, you are ready to start writing code for Joern

## QueryDB (queries plugin)
Quick way to develop and test QueryDB:
```
sbt stage
./querydb-install.sh
./joern-scan --list-query-names
```
The last command prints all available queries - add your own in querydb, run the above commands again to see that your query got deployed.
More details in the [separate querydb readme](querydb/README.md)
