# kotlinastgen

Rust AST generator for the oxidized Kotlin frontend path.

The binary walks `.kt` and `.kts` inputs, parses them with `tree-sitter-kotlin`,
and writes one JSON document per source file. The emitted tree is intentionally
structural and span-preserving so Scala lowering can be implemented feature by
feature against a stable contract.

Run locally:

```sh
cargo test
cargo run --bin kotlinastgen -- -out /tmp/kotlinastgen-out /path/to/kotlin/project
```
