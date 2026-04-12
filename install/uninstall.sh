#!/usr/bin/env bash
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${BIN_DIR:-$PREFIX/bin}"
STD_DIR="${STD_DIR:-$(dirname "$BIN_DIR")/share/mst/std}"
TARGET_PATH="$BIN_DIR/mst"

if [[ -e "$TARGET_PATH" ]]; then
    rm -f "$TARGET_PATH"
    echo "[mst] removed $TARGET_PATH"
else
    echo "[mst] no installed binary found at $TARGET_PATH"
fi

if [[ -e "$STD_DIR" ]]; then
    rm -rf "$STD_DIR"
    echo "[mst] removed $STD_DIR"
fi
