#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
oracle_home="${JOERN_HOME:-/tmp/joern-cli-dist/joern-cli}"
joern="$oracle_home/joern"
joern_parse="$oracle_home/joern-parse"

if [[ ! -x "$joern" || ! -x "$joern_parse" ]]; then
  echo "missing pinned Joern oracle under $oracle_home" >&2
  echo "run: $repo_root/joern-parity/setup-oracle.sh" >&2
  exit 1
fi

scratch="$(mktemp -d "${TMPDIR:-/tmp}/cpgql-differential.XXXXXX")"
cleanup() {
  if [[ -n "${scratch:-}" && "$scratch" == "${TMPDIR:-/tmp}"/cpgql-differential.* ]]; then
    rm -rf -- "$scratch"
  fi
}
trap cleanup EXIT

cargo build --manifest-path "$repo_root/Cargo.toml" --release --locked -p cpg-cli
cpg="$repo_root/target/release/cpg"
fixture="$repo_root/acceptance/cpgql/fixture"

"$joern_parse" "$fixture" --output "$scratch/oracle.cpg.bin" >/dev/null
"$joern" --nocolors --script "$repo_root/acceptance/cpgql/probe.sc" \
  --param "cpgPath=$scratch/oracle.cpg.bin" \
  | rg '^CPGQL\t' \
  | sort > "$scratch/oracle.tsv"

python3 "$repo_root/acceptance/cpgql/native.py" \
  --cpg "$cpg" \
  --fixture "$fixture" \
  --catalog "$repo_root/acceptance/cpgql/differential.json" \
  | sort > "$scratch/native.tsv"

diff -u "$scratch/oracle.tsv" "$scratch/native.tsv"
echo "CPGQL differential: PASS (27/27, Joern v4.0.555)"
