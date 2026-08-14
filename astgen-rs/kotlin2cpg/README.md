# kotlinastgen

Rust AST generator for the oxidized Kotlin frontend path.

The binary walks `.kt` and `.kts` inputs, parses them with `tree-sitter-kotlin`,
and writes one structural, span-preserving JSON document per source file.

Run locally:

```sh
cargo test
cargo run --bin kotlinastgen -- -out /tmp/kotlinastgen-out /path/to/kotlin/project
```
