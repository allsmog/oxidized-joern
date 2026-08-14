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

"$c2cpg" "$fixture" \
  -o "$scratch/oracle.cpg.bin" \
  --include "$fixture/include" \
  --define FORCE_ON=1 \
  --define CLI_ON=1 >/dev/null

"$joern" --script "$here/probe.sc" \
  --param cpgPath="$scratch/oracle.cpg.bin" \
  | grep '^CSEM' | sort > "$scratch/oracle.tsv"

python3 "$here/native.py" \
  --cpg "$repo_root/target/release/cpg" \
  --fixture "$fixture" \
  | sort > "$scratch/native.tsv"

diff -u "$scratch/oracle.tsv" "$scratch/native.tsv"
echo "C compiler-input differential: PASS (4/4, Joern v4.0.555)"
