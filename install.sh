#!/usr/bin/env bash
# install.sh — Download and install the latest fuselage release.
#
# Usage:
#   curl -sSfL https://raw.githubusercontent.com/sfkleach/fuselage/main/install.sh | bash
#
# Options (set as environment variables before piping):
#   FUSELAGE_VERSION   Specific version tag to install, e.g. "v0.1.0" (default: latest).
#   FUSELAGE_INSTALL   Installation directory (default: $HOME/.local/bin).
#   FUSELAGE_SETUID    Set to "0" to skip setuid-root installation (default: 1).
#
# Example — install a specific version without setuid to a custom directory:
#   FUSELAGE_VERSION=v0.1.0 FUSELAGE_INSTALL=~/bin FUSELAGE_SETUID=0 \
#     curl -sSfL ... | bash

set -euo pipefail

REPO="sfkleach/fuselage"
INSTALL_DIR="${FUSELAGE_INSTALL:-$HOME/.local/bin}"
SETUID="${FUSELAGE_SETUID:-1}"

# Detect the CPU architecture and map it to the release target triple.
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
    aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
    *)
        echo "ERROR: Unsupported architecture: $ARCH" >&2
        exit 1
        ;;
esac

OS="$(uname -s)"
if [ "$OS" != "Linux" ]; then
    echo "ERROR: fuselage requires Linux (got $OS)." >&2
    exit 1
fi

# Resolve the version to install.
if [ -n "${FUSELAGE_VERSION:-}" ]; then
    VERSION="$FUSELAGE_VERSION"
else
    echo "Fetching latest release version..."
    VERSION="$(curl -sSfL "https://api.github.com/repos/${REPO}/releases/latest" \
        | grep '"tag_name"' \
        | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"
    if [ -z "$VERSION" ]; then
        echo "ERROR: Could not determine latest release version." >&2
        exit 1
    fi
fi

ARCHIVE="fuselage-${VERSION}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE}"
TMPDIR="/tmp/fuselage-install-$$"

echo "Installing fuselage ${VERSION} (${TARGET})..."

mkdir -p "$TMPDIR"
# shellcheck disable=SC2064
trap "rm -rf /tmp/fuselage-install-$$" EXIT

curl -sSfL "$URL" -o "${TMPDIR}/${ARCHIVE}"
tar -xzf "${TMPDIR}/${ARCHIVE}" -C "$TMPDIR"

mkdir -p "$INSTALL_DIR"
cp "${TMPDIR}/fuselage" "${INSTALL_DIR}/fuselage"
chmod 755 "${INSTALL_DIR}/fuselage"

if [ "$SETUID" = "1" ]; then
    echo "Setting setuid-root on ${INSTALL_DIR}/fuselage (requires sudo)..."
    sudo chown root:root "${INSTALL_DIR}/fuselage"
    sudo chmod u+s "${INSTALL_DIR}/fuselage"
    echo "Done. fuselage installed setuid-root to ${INSTALL_DIR}/fuselage."
else
    echo "Done. fuselage installed to ${INSTALL_DIR}/fuselage."
    echo "Note: running without setuid-root — UID remapping will be used."
    echo "      Re-run with FUSELAGE_SETUID=1 to install setuid-root."
fi
