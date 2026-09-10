# C format-string argument positions

The built-in C format rule previously treated every argument of `printf`, `fprintf` and `syslog` as a format-string sink. That reported ordinary formatted data and even a `fprintf` stream argument. The corrected selectors are `printf@0`, `fprintf@1` and `syslog@1`, using the same zero-based argument convention as the existing C++ pack.

The eleven source functions exercise the production C frontend, analysis pipeline and scan layer. Four must report a format finding: `printf_format`, `fprintf_format`, `syslog_format` and `macro_format`. Seven must remain clear: `printf_data`, `fprintf_data`, `fprintf_stream`, `syslog_data`, `killed_format`, `nested_data` and `macro_data`.

The first test verifies the selector convention with an explicit rule before relying on it in the built-in pack. Before the correction, that explicit test passed while the built-in rule reported ten methods, including six false positives. After the one-line rule change, both tests report exactly the four expected methods. The negative cases retain constant formats, a definite overwrite and a Lua-style comma-expression macro; they do not suppress macro-expanded format-string positives.

`measurement.json` binds the before/after logs and the frozen frontend snapshot used for validation. This fixture tests scanner policy and does not claim vulnerability verification or whole-Joern query parity. Run from `cpg-rs` with:

```sh
cargo test -p cpg-cli --test c_format_positions
```
