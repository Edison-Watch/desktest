#!/bin/bash
# Build the a11y-helper binary for macOS.
#
# Usage:
#   ./build.sh          # Release build
#   ./build.sh debug     # Debug build
#
# Output: .build/release/a11y-helper (or .build/debug/a11y-helper)

set -euo pipefail
cd "$(dirname "$0")"

CONFIG="${1:-release}"
if [ "$CONFIG" = "debug" ]; then
    swift build
else
    swift build -c release
fi

BINARY=".build/$CONFIG/a11y-helper"
if [ -f "$BINARY" ]; then
    echo "Built: $BINARY"
else
    echo "error: expected binary not found at $BINARY" >&2
    exit 1
fi
