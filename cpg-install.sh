#!/usr/bin/env sh
set -eu

REPOSITORY="allsmog/oxidized-joern"
RELEASE_VERSION=""
INSTALL_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/oxidized-joern"
BIN_DIR="${HOME}/.local/bin"

usage() {
  cat <<'EOF'
Install the Rust-native Oxidized Joern cpg preview.

Usage: sh cpg-install.sh [options]

Options:
  --version TAG       Install a release tag such as v0.1.0 (default: latest)
  --install-dir PATH  Store the installed binary under PATH
  --bin-dir PATH      Create the cpg command under PATH
  -h, --help          Show this help
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version)
      [ "$#" -ge 2 ] || { echo "--version needs a value" >&2; exit 2; }
      RELEASE_VERSION=$2
      shift 2
      ;;
    --version=*) RELEASE_VERSION=${1#*=}; shift ;;
    --install-dir)
      [ "$#" -ge 2 ] || { echo "--install-dir needs a value" >&2; exit 2; }
      INSTALL_DIR=$2
      shift 2
      ;;
    --install-dir=*) INSTALL_DIR=${1#*=}; shift ;;
    --bin-dir)
      [ "$#" -ge 2 ] || { echo "--bin-dir needs a value" >&2; exit 2; }
      BIN_DIR=$2
      shift 2
      ;;
    --bin-dir=*) BIN_DIR=${1#*=}; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
done

for tool in curl mktemp; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "$tool is required" >&2
    exit 1
  fi
done

case "$(uname -s)" in
  Linux*) os=linux; archive_extension=tar.gz; executable=cpg ;;
  Darwin*) os=macos; archive_extension=tar.gz; executable=cpg ;;
  MINGW*|MSYS*|CYGWIN*) os=windows; archive_extension=zip; executable=cpg.exe ;;
  *) echo "unsupported operating system: $(uname -s)" >&2; exit 2 ;;
esac

case "$(uname -m)" in
  x86_64|amd64) architecture=amd64 ;;
  arm64|aarch64) architecture=arm64 ;;
  *) echo "unsupported architecture: $(uname -m)" >&2; exit 2 ;;
esac

if [ "$os" = windows ] && [ "$architecture" != amd64 ]; then
  echo "Windows ARM64 releases are not available" >&2
  exit 2
fi

platform="${os}-${architecture}"
package="oxidized-joern-cpg-${platform}"
archive="${package}.${archive_extension}"
if [ -n "$RELEASE_VERSION" ]; then
  release_url="https://github.com/${REPOSITORY}/releases/download/${RELEASE_VERSION}"
else
  release_url="https://github.com/${REPOSITORY}/releases/latest/download"
fi

temporary_directory=$(mktemp -d "${TMPDIR:-/tmp}/cpg-install.XXXXXX")
trap 'rm -rf "$temporary_directory"' EXIT HUP INT TERM

curl --fail --location --retry 3 --proto '=https' --tlsv1.2 \
  "${release_url}/${archive}" -o "${temporary_directory}/${archive}"
curl --fail --location --retry 3 --proto '=https' --tlsv1.2 \
  "${release_url}/${archive}.sha256" -o "${temporary_directory}/${archive}.sha256"

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$temporary_directory" && sha256sum --check "${archive}.sha256")
elif command -v shasum >/dev/null 2>&1; then
  (cd "$temporary_directory" && shasum -a 256 --check "${archive}.sha256")
else
  echo "sha256sum or shasum is required" >&2
  exit 1
fi

if [ "$archive_extension" = zip ]; then
  command -v unzip >/dev/null 2>&1 || { echo "unzip is required" >&2; exit 1; }
  unzip -q "${temporary_directory}/${archive}" -d "$temporary_directory"
else
  command -v tar >/dev/null 2>&1 || { echo "tar is required" >&2; exit 1; }
  tar -xzf "${temporary_directory}/${archive}" -C "$temporary_directory"
fi

source_binary="${temporary_directory}/${package}/${executable}"
[ -f "$source_binary" ] || { echo "release archive does not contain ${executable}" >&2; exit 1; }

mkdir -p "${INSTALL_DIR}/bin" "$BIN_DIR"
cp "$source_binary" "${INSTALL_DIR}/bin/${executable}"
chmod 0755 "${INSTALL_DIR}/bin/${executable}"

if [ "$os" = windows ]; then
  cp "${INSTALL_DIR}/bin/${executable}" "${BIN_DIR}/${executable}"
else
  ln -sf "${INSTALL_DIR}/bin/${executable}" "${BIN_DIR}/cpg"
fi

"${INSTALL_DIR}/bin/${executable}" --version
echo "Installed ${BIN_DIR}/${executable}"
