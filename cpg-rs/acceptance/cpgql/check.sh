#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
committed_only=0
if [[ "${1:-}" == "--committed-only" ]]; then
  committed_only=1
  shift
fi
if [[ "$#" -ne 0 ]]; then
  echo "usage: $0 [--committed-only]" >&2
  exit 2
fi
oracle_home="${JOERN_HOME:-/tmp/joern-cli-dist/joern-cli}"
joern="$oracle_home/joern"
joern_parse="$oracle_home/joern-parse"

if [[ "$committed_only" -eq 0 ]]; then
  if [[ ! -x "$joern" || ! -x "$joern_parse" ]]; then
    echo "missing pinned Joern oracle under $oracle_home" >&2
    echo "run: $repo_root/joern-parity/setup-oracle.sh" >&2
    exit 1
  fi
  version="$({ printf 'exit\n' | "$joern" --nocolors 2>/dev/null || true; } | sed -n 's/^Version: //p' | head -1)"
  if [[ "$version" != "4.0.555" ]]; then
    echo "expected Joern v4.0.555, found ${version:-unknown}" >&2
    exit 1
  fi
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
catalog="$repo_root/acceptance/cpgql/catalog.json"
expected="$(jq '[.tiers[].cases[]] | length' "$catalog")"

"$cpg" build "$fixture" --lang c -o "$scratch/native-source.cpg" >/dev/null
python3 "$repo_root/acceptance/cpgql/native.py" \
  --cpg "$cpg" \
  --graph "$scratch/native-source.cpg" \
  --catalog "$catalog" \
  | sort > "$scratch/native-source.tsv"
source_actual="$(wc -l < "$scratch/native-source.tsv" | tr -d ' ')"
if [[ "$source_actual" -ne "$expected" ]]; then
  echo "incomplete committed CPGQL run: expected $expected cases, found $source_actual" >&2
  exit 1
fi
source_digest="$(python3 -c 'import hashlib, sys; print(hashlib.sha256(open(sys.argv[1], "rb").read()).hexdigest())' "$scratch/native-source.tsv")"
expected_digest="$(tr -d '[:space:]' < "$repo_root/acceptance/cpgql/expected.sha256")"
if [[ "$source_digest" != "$expected_digest" ]]; then
  echo "committed CPGQL result changed: expected $expected_digest, found $source_digest" >&2
  exit 1
fi
if [[ "$committed_only" -eq 1 ]]; then
  echo "CPGQL committed contract: PASS ($source_actual/$expected native cases)"
  exit 0
fi

"$joern_parse" "$fixture" --output "$scratch/oracle.cpg.bin" >/dev/null
"$cpg" import-joern "$scratch/oracle.cpg.bin" -o "$scratch/oracle.cpg" >/dev/null
python3 "$repo_root/acceptance/cpgql/generate_probe.py" \
  --catalog "$catalog" \
  --output "$scratch/probe.sc"
"$joern" --nocolors --script "$scratch/probe.sc" \
  --param "cpgPath=$scratch/oracle.cpg.bin" \
  | rg '^CPGQL\t' \
  | sort > "$scratch/oracle.tsv"

python3 "$repo_root/acceptance/cpgql/native.py" \
  --cpg "$cpg" \
  --graph "$scratch/oracle.cpg" \
  --catalog "$catalog" \
  | sort > "$scratch/native.tsv"

actual="$(wc -l < "$scratch/native.tsv" | tr -d ' ')"
if [[ "$actual" -ne "$expected" ]]; then
  echo "incomplete native CPGQL differential: expected $expected cases, found $actual" >&2
  exit 1
fi
diff -u "$scratch/oracle.tsv" "$scratch/native.tsv"
echo "CPGQL differential: PASS ($actual/$expected, Joern v4.0.555)"
