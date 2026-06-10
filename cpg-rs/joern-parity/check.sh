#!/usr/bin/env bash
# Differential parity check: compare joern-parity's canonical AST dump against
# Joern's, per method, for every C file in corpus/.
#
#   JOERN=/path/to/joern-cli ./check.sh
#
# Regenerates the oracle from a real Joern install if JOERN is set and reachable;
# otherwise reuses the committed oracle_all.txt. Exits non-zero on any mismatch.
set -uo pipefail
cd "$(dirname "$0")"
HERE="$(pwd)"
ROOT=".."
JOERN="${JOERN:-/tmp/joern-cli-dist/joern-cli}"

ORACLE="oracle_all.txt"
if [ -x "$JOERN/joern" ]; then
  echo "regenerating oracle from $JOERN ..."
  TMP_ORACLE="$(mktemp)"
  if (cd "$JOERN" && rm -rf workspace && ./joern --script "$HERE/oracle.sc" \
       --param inputPath="$HERE/corpus" 2>/dev/null) \
       | grep '^AST|' | sed 's/^AST|//' > "$TMP_ORACLE" && [ -s "$TMP_ORACLE" ]; then
    mv "$TMP_ORACLE" "$ORACLE"
  else
    rm -f "$TMP_ORACLE"
    echo "  (oracle regen failed or empty; using committed $ORACLE)"
  fi
fi

# Build mine: one run over the whole corpus (stubs/globals are project-wide).
MINE="$(mktemp)"
cargo run -q --manifest-path "$ROOT/Cargo.toml" -p joern-parity -- corpus/*.c > "$MINE"

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

OD=$(mktemp -d); MD=$(mktemp -d)
split_methods "$ORACLE" "$OD"
split_methods "$MINE" "$MD"

fail=0; total=0
for m in "$MD"/*; do
  name=$(basename "$m"); total=$((total+1))
  if [ ! -f "$OD/$name" ]; then echo "NO ORACLE for $name"; fail=$((fail+1)); continue; fi
  if diff -q "$OD/$name" "$m" >/dev/null; then
    echo "PASS  $name"
  else
    echo "FAIL  $name"; diff "$OD/$name" "$m" | sed 's/^/      /'; fail=$((fail+1))
  fi
done
echo "----"
echo "$((total-fail))/$total methods byte-identical to Joern"
rm -rf "$MINE" "$OD" "$MD"
exit $fail
