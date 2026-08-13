# Contributing

Thank you for improving Oxidized Joern. Changes are accepted under the Apache
License 2.0. By submitting a contribution, you agree that it can be
distributed under that license.

## Before opening a pull request

Use Rust 1.97.0 and keep generated files and build output out of commits.

Run these checks from the Rust workspace you changed:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo audit --deny warnings
```

For `cpg-rs` release or packaging changes, also build the native archive and
container contract:

```bash
cargo build --manifest-path cpg-rs/Cargo.toml --release --locked -p cpg-cli
version="$(sed -n 's/^version = "\([0-9][0-9.]*\)"$/\1/p' cpg-rs/Cargo.toml)"
python3 cpg-rs/scripts/package-release.py \
  --binary cpg-rs/target/release/cpg \
  --output ci \
  --platform linux-amd64 \
  --version "$version"
docker build --file ci/Dockerfile --tag oxidized-joern-cpg:contract .
docker run --rm oxidized-joern-cpg:contract --version
```

## Compatibility

Do not describe an engine path or language as Joern-compatible until its
differential gate passes. Changes to a parser need a focused Rust fixture and
an assertion for the resulting graph shape. Keep the compatibility documents
current when a gate changes.

## Pull requests

Keep each pull request focused. Explain user-visible behavior, compatibility
impact, and the exact verification you ran. Add tests for behavior changes and
update public docs when commands, supported languages, release assets, or
security properties change.

Use public issues for ordinary bugs and feature requests. Use the private path
in [`SECURITY.md`](SECURITY.md) for vulnerabilities.
