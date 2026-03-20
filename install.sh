#!/bin/bash
set -euo pipefail

# desktest installer — downloads the latest release binary for your platform.
# Usage: curl -fsSL https://raw.githubusercontent.com/Edison-Watch/desktest/master/install.sh | bash

REPO="${DESKTEST_REPO:-Edison-Watch/desktest}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# Detect OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
  linux)  TARGET_OS="unknown-linux-gnu" ;;
  darwin) TARGET_OS="apple-darwin" ;;
  *)      echo "Error: Unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64)  TARGET_ARCH="x86_64" ;;
  aarch64|arm64) TARGET_ARCH="aarch64" ;;
  *)             echo "Error: Unsupported architecture: $ARCH"; exit 1 ;;
esac

TARGET="${TARGET_ARCH}-${TARGET_OS}"

echo "Detecting platform: ${TARGET}"

# Get latest release tag
LATEST=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | head -1 | cut -d'"' -f4)
if [ -z "$LATEST" ]; then
  echo "Error: Could not determine latest release"
  exit 1
fi

echo "Latest release: ${LATEST}"

# Download
URL="https://github.com/${REPO}/releases/download/${LATEST}/desktest-${LATEST}-${TARGET}.tar.gz"
CHECKSUMS_URL="https://github.com/${REPO}/releases/download/${LATEST}/SHA256SUMS.txt"

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "Downloading desktest ${LATEST} for ${TARGET}..."
curl -fsSL "$URL" -o "$TMPDIR/desktest.tar.gz"
curl -fsSL "$CHECKSUMS_URL" -o "$TMPDIR/SHA256SUMS.txt"

# Verify checksum
echo "Verifying checksum..."
EXPECTED=$(grep "desktest-${LATEST}-${TARGET}.tar.gz" "$TMPDIR/SHA256SUMS.txt" | awk '{print $1}')
if [ -n "$EXPECTED" ]; then
  if command -v sha256sum &>/dev/null; then
    ACTUAL=$(sha256sum "$TMPDIR/desktest.tar.gz" | awk '{print $1}')
  else
    ACTUAL=$(shasum -a 256 "$TMPDIR/desktest.tar.gz" | awk '{print $1}')
  fi
  if [ "$EXPECTED" != "$ACTUAL" ]; then
    echo "Error: Checksum mismatch!"
    echo "  Expected: $EXPECTED"
    echo "  Actual:   $ACTUAL"
    exit 1
  fi
  echo "Checksum verified."
else
  echo "Error: Checksum for desktest-${LATEST}-${TARGET}.tar.gz not found in SHA256SUMS.txt"
  exit 1
fi

# Install
mkdir -p "$INSTALL_DIR"
tar xzf "$TMPDIR/desktest.tar.gz" -C "$INSTALL_DIR"
chmod +x "$INSTALL_DIR/desktest"

echo ""
echo "desktest ${LATEST} installed to ${INSTALL_DIR}/desktest"

# Check if install dir is in PATH
if ! echo "$PATH" | tr ':' '\n' | grep -q "^${INSTALL_DIR}$"; then
  echo ""
  echo "Add ${INSTALL_DIR} to your PATH:"
  echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
fi
