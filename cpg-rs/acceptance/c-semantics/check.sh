#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$here/../.." && pwd)"
oracle_home="${JOERN:-/tmp/joern-cli-dist/joern-cli}"
c2cpg="$oracle_home/c2cpg.sh"
joern="$oracle_home/joern"
fixture="$here/fixture"

if [ ! -x "$c2cpg" ] || [ ! -x "$joern" ]; then
  echo "pinned Joern v4.0.555 is required at $oracle_home" >&2
  exit 1
fi

scratch="$(mktemp -d "${TMPDIR:-/tmp}/c-semantics.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT

cargo build --release --locked --manifest-path "$repo_root/Cargo.toml" -p cpg-cli

python3 - "$fixture/compile_commands.json" "$scratch/compile_commands.json" "$fixture" <<'PY'
import json
import pathlib
import sys

source, destination, fixture = map(pathlib.Path, sys.argv[1:])
entries = json.loads(source.read_text(encoding="utf-8"))
for entry in entries:
    entry["directory"] = str(fixture.resolve())
destination.write_text(json.dumps(entries, indent=2) + "\n", encoding="utf-8")
PY

"$c2cpg" "$fixture" \
  -o "$scratch/oracle.cpg.bin" \
  --compilation-database "$scratch/compile_commands.json" >/dev/null

"$joern" --script "$here/probe.sc" \
  --param cpgPath="$scratch/oracle.cpg.bin" \
  | grep '^CSEM' | sort > "$scratch/oracle.tsv"

python3 "$here/native.py" \
  --cpg "$repo_root/target/release/cpg" \
  --fixture "$fixture" \
  | sort > "$scratch/native.tsv"

diff -u "$scratch/oracle.tsv" "$scratch/native.tsv"
echo "C compiler-input differential: PASS (4/4, Joern v4.0.555)"
