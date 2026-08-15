#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
here="$repo_root/acceptance/languages"
committed_only=0
selected=""
for arg in "$@"; do
  case "$arg" in
    --committed-only)
      committed_only=1
      ;;
    --*)
      echo "unknown option: $arg" >&2
      exit 2
      ;;
    *)
      if [[ -n "$selected" ]]; then
        echo "only one language id may be selected" >&2
        exit 2
      fi
      selected="$arg"
      ;;
  esac
done
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

scratch="$(mktemp -d "${TMPDIR:-/tmp}/language-differential.XXXXXX")"
cleanup() {
  if [[ -n "${scratch:-}" && "$scratch" == "${TMPDIR:-/tmp}"/language-differential.* ]]; then
    rm -rf -- "$scratch"
  fi
}
trap cleanup EXIT

cargo build --manifest-path "$repo_root/Cargo.toml" --release --locked -p cpg-cli
cpg="$repo_root/target/release/cpg"

passed=0
while IFS=$'\t' read -r id native_language oracle_language relative_fixture; do
  if [[ -n "$selected" && "$selected" != "$id" ]]; then
    continue
  fi
  fixture="$here/$relative_fixture"
  native_graph="$scratch/$id.cpg"
  oracle_graph="$scratch/$id.bin"
  native_results="$scratch/$id.native.tsv"
  oracle_results="$scratch/$id.oracle.tsv"
  expected_results="$here/expected/$id.tsv"

  if [[ ! -f "$expected_results" ]]; then
    echo "missing committed language differential: $expected_results" >&2
    exit 1
  fi

  "$cpg" build "$fixture" --lang "$native_language" -o "$native_graph" >/dev/null
  python3 "$here/native.py" \
    --cpg "$cpg" \
    --graph "$native_graph" \
    --language "$native_language" \
    --queries "$here/queries.json" | sort > "$native_results"

  if [[ "$committed_only" -eq 1 ]]; then
    if ! diff -u "$expected_results" "$native_results"; then
      echo "$id frontend differs from the committed Joern v4.0.555 result" >&2
      exit 1
    fi
  else
    "$joern_parse" "$fixture" --language "$oracle_language" --output "$oracle_graph" >/dev/null
    "$joern" --nocolors --script "$here/probe.sc" --param "cpgPath=$oracle_graph" \
      | rg '^LANGUAGE\t' | sort > "$oracle_results"
    if ! diff -u "$expected_results" "$oracle_results"; then
      echo "$id committed result is not reproducible with Joern v4.0.555" >&2
      exit 1
    fi
    if ! diff -u "$oracle_results" "$native_results"; then
      echo "$id frontend differs from Joern v4.0.555" >&2
      exit 1
    fi
  fi
  printf 'PASS %s: 13/13 semantic probes\n' "$id"
  passed=$((passed + 1))
done < <(jq -r '.differentials[] | [.id, .nativeLanguage, .oracleLanguage, .fixture] | @tsv' "$here/manifest.json")

expected=8
if [[ -n "$selected" ]]; then
  expected=1
fi
if [[ "$passed" -ne "$expected" ]]; then
  echo "unknown or incomplete language differential selection: ${selected:-all}" >&2
  exit 1
fi
if [[ "$committed_only" -eq 1 ]]; then
  printf 'Language frontend differential: PASS (%d/%d committed Joern v4.0.555 results)\n' "$passed" "$expected"
else
  printf 'Language frontend differential: PASS (%d/%d live oracle-backed languages, Joern v4.0.555)\n' "$passed" "$expected"
fi
