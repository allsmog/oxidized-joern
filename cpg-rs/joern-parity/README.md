# C differential parity

This harness compares the shipped Rust C graph with Joern **v4.0.555** on the
committed C corpus. The default `joern-parity` executable constructs the same
`CFrontend`, `Project`, and standard analysis pipeline as `cpg build --lang c`.
There is no separate production implementation waiting to replace the oracle
path.

## Checks

From this directory:

```bash
# Compare Rust output with the committed reference; no JVM is needed.
./check.sh --committed-only

# Require a fresh Joern run and compare Rust with its output.
JAVA_HOME=/path/to/jdk21 JOERN=/path/to/joern-cli ./check.sh --live

# Exercise gate failure handling without a Rust build or JVM.
python3 test_check.py
```

`--live` fails if Joern is missing, exits unsuccessfully, or omits a required
output section. It never falls back to the committed reference and never
rewrites it. Joern runs in a temporary working directory, leaving existing
workspaces intact. Both modes fail when the Rust producer fails, a method is
missing or unexpected, or a compared block differs.

For an intentional corpus extension, the legacy command without a mode can
regenerate `oracle_all.txt` from an available Joern installation:

```bash
JAVA_HOME=/path/to/jdk21 JOERN=/path/to/joern-cli ./check.sh
```

That convenience mode can fall back to the committed reference after failed
regeneration. It is **not evidence of a live comparison**. Verify an updated
reference with `--live`, inspect its diff, and commit the corpus and generated
reference together. Never edit oracle values by hand.

## What parity establishes

`oracle.sc` emits method ASTs (including global and operator methods), selected
scaffolding nodes, fifteen structural edge kinds, and reaching-definition
facts. The AST projection compares nine named properties. `check.sh` compares
the complete selected output in blocks: one per method, one per structural
edge kind, one scaffolding block, and one reaching-definition block. The
reported count is **comparison blocks**, not languages, programs, or a feature
completion percentage.

This is a bounded C graph comparison. It does not establish equivalence of all
Joern schema properties, arbitrary C programs, final `reachableBy` results,
scanner rules, other frontends, CPGQL, plugins, or binary graph formats.
Production scanner outcomes have separate tests under
`cpg-analysis/tests`. The zlib/Lua acceptance script checks deterministic
workflows and resource budgets; it does not compare those whole projects with
Joern. See [COMPATIBILITY.md](../COMPATIBILITY.md) for the product boundary.

`--lowering` dumps the exact C lowering before graph import, and
`--migration-report` compares that dump with the shared graph projection.
These diagnostic modes are not substitutes for the default production gate.
