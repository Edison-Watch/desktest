#!/bin/sh
set -eu

# Desktest installer
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/Edison-Watch/desktest/master/install.sh | sh
#   DESKTEST_VERSION=0.2.0 curl -fsSL ... | sh
#   DESKTEST_INSTALL_DIR=/custom/path curl -fsSL ... | sh

REPO="Edison-Watch/desktest"
INSTALL_DIR="${DESKTEST_INSTALL_DIR:-/usr/local/bin}"

main() {
    detect_platform
    resolve_version
    download_and_install
    verify_installation
}

detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Linux)  OS_TAG="unknown-linux" ;;
        Darwin) OS_TAG="apple-darwin" ;;
        *)      err "Unsupported OS: $OS" ;;
    esac

    case "$ARCH" in
        x86_64|amd64)  ARCH_TAG="x86_64" ;;
        aarch64|arm64) ARCH_TAG="aarch64" ;;
        *)             err "Unsupported architecture: $ARCH" ;;
    esac

    # On Linux, prefer musl builds (works on both glibc and musl systems)
    if [ "$OS" = "Linux" ]; then
        TARGET="${ARCH_TAG}-${OS_TAG}-musl"
    else
        TARGET="${ARCH_TAG}-${OS_TAG}"
    fi

    log "Detected platform: ${TARGET}"
}

resolve_version() {
    if [ -n "${DESKTEST_VERSION:-}" ]; then
        VERSION="v${DESKTEST_VERSION#v}"
        log "Using pinned version: ${VERSION}"
    else
        log "Fetching latest release..."
        VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
            | grep '"tag_name"' \
            | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')" \
            || true
        if [ -z "$VERSION" ]; then
            err "Failed to fetch latest release. Set DESKTEST_VERSION to install a specific version."
        fi
        log "Latest version: ${VERSION}"
    fi
}

download_and_install() {
    TARBALL="desktest-${VERSION}-${TARGET}.tar.gz"
    URL="https://github.com/${REPO}/releases/download/${VERSION}/${TARBALL}"
    CHECKSUMS_URL="https://github.com/${REPO}/releases/download/${VERSION}/SHA256SUMS.txt"

    DL_TMPDIR="$(mktemp -d)"
    trap 'rm -rf "$DL_TMPDIR"' EXIT

    log "Downloading ${TARBALL}..."
    curl -fsSL -o "${DL_TMPDIR}/${TARBALL}" "$URL" \
        || err "Download failed. Check that version ${VERSION} exists and has a build for ${TARGET}."

    log "Verifying checksum..."
    curl -fsSL -o "${DL_TMPDIR}/SHA256SUMS.txt" "$CHECKSUMS_URL" \
        || err "Failed to download checksums file."

    EXPECTED="$(grep "${TARBALL}" "${DL_TMPDIR}/SHA256SUMS.txt" | awk '{print $1}')"
    if [ -z "$EXPECTED" ]; then
        err "No checksum found for ${TARBALL} in SHA256SUMS.txt"
    fi

    if command -v sha256sum >/dev/null 2>&1; then
        ACTUAL="$(sha256sum "${DL_TMPDIR}/${TARBALL}" | awk '{print $1}')"
    elif command -v shasum >/dev/null 2>&1; then
        ACTUAL="$(shasum -a 256 "${DL_TMPDIR}/${TARBALL}" | awk '{print $1}')"
    else
        warn "Neither sha256sum nor shasum found, skipping checksum verification"
        ACTUAL="$EXPECTED"
    fi

    if [ "$EXPECTED" != "$ACTUAL" ]; then
        err "Checksum mismatch!
  Expected: ${EXPECTED}
  Actual:   ${ACTUAL}"
    fi
    log "Checksum verified."

    log "Extracting to ${INSTALL_DIR}..."
    tar xzf "${DL_TMPDIR}/${TARBALL}" -C "$DL_TMPDIR"

    if [ -w "$INSTALL_DIR" ] || { [ ! -e "$INSTALL_DIR" ] && [ -w "$(dirname "$INSTALL_DIR")" ]; }; then
        mkdir -p "$INSTALL_DIR"
        mv "${DL_TMPDIR}/desktest" "${INSTALL_DIR}/desktest"
        chmod +x "${INSTALL_DIR}/desktest"
    else
        log "Elevated permissions required to install to ${INSTALL_DIR}"
        sudo mkdir -p "$INSTALL_DIR"
        sudo mv "${DL_TMPDIR}/desktest" "${INSTALL_DIR}/desktest"
        sudo chmod +x "${INSTALL_DIR}/desktest"
    fi
}

verify_installation() {
    if command -v desktest >/dev/null 2>&1; then
        log "Installed desktest $(desktest --version)"
    else
        log "Installed to ${INSTALL_DIR}/desktest"
        if echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
            :
        else
            warn "${INSTALL_DIR} is not in your PATH. Add it with:
  export PATH=\"${INSTALL_DIR}:\$PATH\""
        fi
    fi
}

log() {
    printf '\033[1;32m==>\033[0m %b\n' "$1"
}

warn() {
    printf '\033[1;33mwarning:\033[0m %b\n' "$1" >&2
}

err() {
    printf '\033[1;31merror:\033[0m %b\n' "$1" >&2
    exit 1
}

main
