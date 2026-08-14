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

scratch="$(mktemp -d "${TMPDIR:-/tmp}/cpg-flatgraph.XXXXXX")"
cleanup() {
  if [[ -n "${scratch:-}" && "$scratch" == "${TMPDIR:-/tmp}"/cpg-flatgraph.* ]]; then
    rm -rf -- "$scratch"
  fi
}
trap cleanup EXIT

cargo build --manifest-path "$repo_root/Cargo.toml" --release --locked -p cpg-cli
cpg="$repo_root/target/release/cpg"

# Each row is oracle-language|native-language|fixture|two expected methods.
# C plus one JVM and one dynamic language is the minimum replacement contract;
# every row is exercised in both directions.
cases=(
  "newc|c|$repo_root/acceptance/flatgraph/fixtures/c|add|main"
  "javasrc|java|$repo_root/acceptance/flatgraph/fixtures/java|main|twice"
  "pythonsrc|python|$repo_root/acceptance/flatgraph/fixtures/python|main|twice"
)

for case_index in "${!cases[@]}"; do
  IFS='|' read -r oracle_lang native_lang fixture expected_a expected_b <<<"${cases[$case_index]}"
  prefix="$scratch/$native_lang"

  # Joern -> native: decode the oracle's actual v4 Flatgraph, persist CPG2,
  # then run native queries over the imported supported schema.
  "$joern_parse" "$fixture" --language "$oracle_lang" --output "$prefix.joern.bin" >/dev/null
  "$cpg" import-joern "$prefix.joern.bin" -o "$prefix.native.cpg"
  imported_names="$("$cpg" query --load "$prefix.native.cpg" --lang "$native_lang" --query "cpg.method(\"$expected_a|$expected_b\").name")"
  rg -q "\"$expected_a\"" <<<"$imported_names"
  rg -q "\"$expected_b\"" <<<"$imported_names"

  # Native -> Joern: export the language frontend's graph and load it directly
  # with Joern's v4.0.555 CpgLoader (no workspace conversion).
  "$cpg" export-joern "$fixture" --lang "$native_lang" -o "$prefix.export.bin"
  probe="$("$joern" --nocolors --script "$repo_root/acceptance/flatgraph/probe.sc" --param "cpgPath=$prefix.export.bin")"
  rg -q 'FLATGRAPH_OK methods=[1-9][0-9]* calls=[1-9][0-9]* files=[1-9][0-9]*' <<<"$probe"
  rg -q "METHOD_NAMES=.*$expected_a" <<<"$probe"
  rg -q "METHOD_NAMES=.*$expected_b" <<<"$probe"
done

echo "Flatgraph interoperability: PASS (3/3 languages, both directions, Joern v4.0.555)"
