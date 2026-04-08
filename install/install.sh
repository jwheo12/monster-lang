#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${BIN_DIR:-$PREFIX/bin}"
TARGET_PATH="$BIN_DIR/mst"

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo is required to build mst from source" >&2
    exit 1
fi

echo "[mst] building release binary..."
cargo build --release --locked --manifest-path "$PROJECT_ROOT/Cargo.toml"

mkdir -p "$BIN_DIR"
install -m 755 "$PROJECT_ROOT/target/release/mst" "$TARGET_PATH"

echo "[mst] installed to $TARGET_PATH"

if ! command -v clang-18 >/dev/null 2>&1 && ! command -v clang >/dev/null 2>&1; then
    echo "[mst] warning: clang-18 or clang was not found on PATH" >&2
fi

if ! command -v opt-18 >/dev/null 2>&1 && ! command -v opt >/dev/null 2>&1; then
    echo "[mst] warning: opt-18 or opt was not found on PATH" >&2
fi

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
        echo "[mst] note: $BIN_DIR is not on PATH"
        echo "[mst] add this to your shell profile:"
        echo "export PATH=\"$BIN_DIR:\$PATH\""
        ;;
esac

echo "[mst] try: mst --help"
