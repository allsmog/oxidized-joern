#!/usr/bin/env bash
# Fetch and unpack the Joern release used as the differential-testing oracle.
# The remote execution environment is ephemeral, so every session re-runs this.
set -euo pipefail
# The byte-parity spec is pinned to this Joern release (see PROGRESS.md).
# Override with JOERN_VERSION=vX.Y.Z only as a deliberate, recorded upgrade.
JOERN_VERSION="${JOERN_VERSION:-v4.0.555}"
DEST="${1:-/tmp/joern-cli-dist}"
if [ -x "$DEST/joern-cli/joern" ]; then
  echo "oracle already present at $DEST/joern-cli"
  exit 0
fi
echo "downloading Joern $JOERN_VERSION release (~2GB) ..."
curl -sL -o /tmp/joern-cli.zip "https://github.com/joernio/joern/releases/download/$JOERN_VERSION/joern-cli.zip"
unzip -q -o /tmp/joern-cli.zip -d "$DEST"
rm -f /tmp/joern-cli.zip
"$DEST/joern-cli/joern" --help >/dev/null 2>&1 || { echo "joern failed to launch"; exit 1; }
echo "oracle ready: $DEST/joern-cli ($JOERN_VERSION; record in PROGRESS.md if it changed)"
