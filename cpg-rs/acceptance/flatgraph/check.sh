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

# Joern -> native: parse with the pinned oracle, decode its actual v4
# Flatgraph, persist CPG2, and execute a native query over the imported graph.
"$joern_parse" "$repo_root/joern-parity/corpus/add.c" --output "$scratch/joern.cpg.bin" >/dev/null
"$cpg" import-joern "$scratch/joern.cpg.bin" -o "$scratch/native.cpg"
imported_names="$("$cpg" query --load "$scratch/native.cpg" --lang c --query 'cpg.method.name("add|main").name')"
rg -q '"add"' <<<"$imported_names"
rg -q '"main"' <<<"$imported_names"

# Native -> Joern: emit Flatgraph from the production C frontend and load it
# directly with Joern's v4.0.555 CpgLoader (no workspace conversion).
"$cpg" export-joern "$repo_root/joern-parity/corpus" --lang c -o "$scratch/native.cpg.bin"
probe="$($joern --nocolors --script "$repo_root/acceptance/flatgraph/probe.sc" --param "cpgPath=$scratch/native.cpg.bin")"
rg -q 'FLATGRAPH_OK methods=[1-9][0-9]* calls=[1-9][0-9]* files=[1-9][0-9]*' <<<"$probe"
rg -q 'METHOD_NAMES=.*main' <<<"$probe"

echo "Flatgraph interoperability: PASS (Joern v4.0.555, both directions)"
