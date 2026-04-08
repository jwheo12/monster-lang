#!/usr/bin/env bash
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${BIN_DIR:-$PREFIX/bin}"
TARGET_PATH="$BIN_DIR/mst"

if [[ ! -e "$TARGET_PATH" ]]; then
    echo "[mst] no installed binary found at $TARGET_PATH"
    exit 0
fi

rm -f "$TARGET_PATH"
echo "[mst] removed $TARGET_PATH"
