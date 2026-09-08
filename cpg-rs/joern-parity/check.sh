#!/usr/bin/env bash
# Differential parity check: compare joern-parity's canonical AST dump against
# Joern's, per method, for every C file in corpus/.
#
#   JOERN=/path/to/joern-cli ./check.sh
#   ./check.sh --committed-only
#   JOERN=/path/to/joern-cli ./check.sh --live
#
# Regenerates the oracle from a real Joern install if JOERN is set and reachable;
# otherwise reuses the committed oracle_all.txt. --committed-only guarantees
# that no locally installed oracle can rewrite the committed reference.
# --live requires successful regeneration and never rewrites the committed oracle.
# Exits non-zero on any mismatch or failed producer.
set -euo pipefail
cd "$(dirname "$0")" || exit 1
HERE="$(pwd)"
ROOT=".."
COMMITTED_ONLY=0
LIVE=0
if [ "${1:-}" = "--committed-only" ]; then
  COMMITTED_ONLY=1
  shift
elif [ "${1:-}" = "--live" ]; then
  LIVE=1
  shift
fi
if [ "$#" -ne 0 ]; then
  echo "usage: $0 [--committed-only|--live]" >&2
  exit 2
fi
JOERN="${JOERN:-/tmp/joern-cli-dist/joern-cli}"
TEMP_ROOT=$(mktemp -d)
trap 'rm -rf "$TEMP_ROOT"' EXIT

require_sections() {
  local reference="$1"
  for section in AST NODES EDGES FLOWS; do
    if ! grep -q "^${section}|." "$reference"; then
      echo "incomplete oracle: missing ${section} section in $reference" >&2
      return 1
    fi
  done
  if ! grep -q '^AST|METHOD ' "$reference"; then
    echo "incomplete oracle: missing method AST blocks in $reference" >&2
    return 1
  fi
}

ORACLE="oracle_all.txt"
require_sections "$ORACLE"
if [ "$LIVE" -eq 1 ] && [ ! -x "$JOERN/joern" ]; then
  echo "--live requires an executable oracle at $JOERN/joern" >&2
  exit 1
fi
if [ "$COMMITTED_ONLY" -eq 0 ] && [ -x "$JOERN/joern" ]; then
  JOERN=$(cd "$JOERN" && pwd)
  echo "regenerating oracle from $JOERN ..."
  TMP_ORACLE="$TEMP_ROOT/oracle.txt"
  mkdir "$TEMP_ROOT/workspace"
  if (cd "$TEMP_ROOT/workspace" && "$JOERN/joern" --script "$HERE/oracle.sc" \
       --param inputPath="$HERE/corpus" > "$TEMP_ROOT/joern.stdout" 2> "$TEMP_ROOT/joern.stderr") \
       && grep -E '^(AST|NODES|EDGES|FLOWS)\|' "$TEMP_ROOT/joern.stdout" > "$TMP_ORACLE" \
       && require_sections "$TMP_ORACLE"; then
    if [ "$LIVE" -eq 1 ]; then
      ORACLE="$TMP_ORACLE"
    else
      mv "$TMP_ORACLE" "$ORACLE"
    fi
  else
    if [ "$LIVE" -eq 1 ]; then
      cat "$TEMP_ROOT/joern.stderr" >&2
      echo "live oracle regeneration failed; refusing committed-reference fallback" >&2
      exit 1
    fi
    echo "  (oracle regen failed or empty; using committed $ORACLE)"
  fi
fi

# Build mine: one run over the whole corpus (stubs/globals are project-wide).
MINE="$TEMP_ROOT/mine.txt"
cargo run -q --locked --manifest-path "$ROOT/Cargo.toml" -p joern-parity -- corpus/*.c > "$MINE"

# Each side splits into method walk (AST), scaffolding nodes (NODES), edges (EDGES).
OAST="$TEMP_ROOT/oracle.ast"; ONODES="$TEMP_ROOT/oracle.nodes"; MAST="$TEMP_ROOT/mine.ast"; MNODES="$TEMP_ROOT/mine.nodes"
OEDGES="$TEMP_ROOT/oracle.edges"; MEDGES="$TEMP_ROOT/mine.edges"; OFLOWS="$TEMP_ROOT/oracle.flows"; MFLOWS="$TEMP_ROOT/mine.flows"
sed -n 's/^AST|//p' "$ORACLE" > "$OAST"
sed -n 's/^NODES|//p' "$ORACLE" > "$ONODES"
sed -n 's/^EDGES|//p' "$ORACLE" > "$OEDGES"
sed -n 's/^FLOWS|//p' "$ORACLE" > "$OFLOWS"
grep -vE '^(NODES|EDGES|FLOWS)\|' "$MINE" > "$MAST" || true
sed -n 's/^NODES|//p' "$MINE" > "$MNODES"
sed -n 's/^EDGES|//p' "$MINE" > "$MEDGES"
sed -n 's/^FLOWS|//p' "$MINE" > "$MFLOWS"

# Split a dump file into per-method blocks keyed by FULL_NAME (NAME collides:
# every file has a <global> method).
split_methods() { # $1 = file, $2 = outdir
  awk -v out="$2" '
    /^METHOD / { match($0, /FULL_NAME=[^ ]+/);
                 name=substr($0, RSTART+10, RLENGTH-10);
                 gsub(/\//, "_", name);
                 file=out "/" name; }
    NF>0 && file { print > file }
    /^$/ { file="" }
  ' "$1"
}

OD="$TEMP_ROOT/oracle-methods"; MD="$TEMP_ROOT/mine-methods"
mkdir "$OD" "$MD"
split_methods "$OAST" "$OD"
split_methods "$MAST" "$MD"

fail=0; total=0
total=$((total+1))
if diff -q "$ONODES" "$MNODES" >/dev/null; then
  echo "PASS  (scaffolding nodes: FILE/NAMESPACE/TYPE_DECL/TYPE/META_DATA)"
else
  echo "FAIL  (scaffolding nodes)"; diff "$ONODES" "$MNODES" | sed 's/^/      /' | head -40 || true
  fail=$((fail+1))
fi

# Edge parity, one block per edge kind.
for kind in $( (cut -d' ' -f1 "$OEDGES"; cut -d' ' -f1 "$MEDGES") | sort -u); do
  total=$((total+1))
  OK="$TEMP_ROOT/oracle-kind"; MK="$TEMP_ROOT/mine-kind"
  grep "^$kind " "$OEDGES" > "$OK" || true
  grep "^$kind " "$MEDGES" > "$MK" || true
  if diff -q "$OK" "$MK" >/dev/null; then
    echo "PASS  (edges: $kind, $(wc -l < "$OK"))"
  else
    echo "FAIL  (edges: $kind)"; diff "$OK" "$MK" | sed 's/^/      /' | head -30 || true
    fail=$((fail+1))
  fi
  rm -f "$OK" "$MK"
done
# Dataflow parity: REACHING_DEF (DDG) flows. Diffed as one exact block so every
# fact is compared (a per-method split risks silently dropping lines whose home
# can't be parsed); on failure the head-capped diff localises the divergence.
total=$((total+1))
if diff -q "$OFLOWS" "$MFLOWS" >/dev/null; then
  echo "PASS  (flows: REACHING_DEF, $(wc -l < "$OFLOWS"))"
else
  echo "FAIL  (flows: REACHING_DEF)"
  diff "$OFLOWS" "$MFLOWS" | sed 's/^/      /' | head -40 || true
  fail=$((fail+1))
fi

shopt -s nullglob
for m in "$OD"/*; do
  name=${m##*/}
  if [ ! -f "$MD/$name" ]; then
    echo "MISSING METHOD  $name"; total=$((total+1)); fail=$((fail+1))
  fi
done
for m in "$MD"/*; do
  name=${m##*/}; total=$((total+1))
  if [ ! -f "$OD/$name" ]; then echo "NO ORACLE for $name"; fail=$((fail+1)); continue; fi
  if diff -q "$OD/$name" "$m" >/dev/null; then
    echo "PASS  $name"
  else
    echo "FAIL  $name"; diff "$OD/$name" "$m" | sed 's/^/      /' || true
    fail=$((fail+1))
  fi
done
echo "----"
echo "$((total-fail))/$total comparison blocks byte-identical to Joern"
if [ "$fail" -ne 0 ]; then
  exit 1
fi
