# Rust AST generators

These independent Cargo workspaces provide native structural AST generators
for ABAP, C#, Java, Jimple, Kotlin, PHP, and Swift. They are retained as Rust
components and tested separately from the `cpg-rs` product workspace.

Run the checks for one generator from its directory:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

The `Rust AST generators` GitHub Actions workflow runs these commands for all
seven workspaces.
