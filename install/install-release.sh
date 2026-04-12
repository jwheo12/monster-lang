#!/usr/bin/env bash
set -euo pipefail

REPO="BitIntx/monster-lang"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${BIN_DIR:-$PREFIX/bin}"
STD_DIR="${STD_DIR:-$(dirname "$BIN_DIR")/share/mst/std}"
APT_LLVM_INSTALL_CMD="apt-get install -y clang-18 llvm-18 llvm-18-tools"

need_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "error: required command not found: $1" >&2
        exit 1
    fi
}

resolve_version() {
    if [[ -n "${MST_VERSION:-}" ]]; then
        printf '%s\n' "$MST_VERSION"
        return
    fi

    need_cmd python3

    curl -fsSL "https://api.github.com/repos/$REPO/releases" | python3 -c '
import json
import sys

releases = json.load(sys.stdin)
for release in releases:
    if not release.get("draft"):
        print(release["tag_name"])
        break
else:
    raise SystemExit("no published releases found")
'
}

detect_asset_name() {
    local version="$1"
    local os
    local arch

    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux) os="linux" ;;
        Darwin) os="darwin" ;;
        *)
            echo "error: unsupported operating system: $os" >&2
            echo "error: install-release.sh currently supports Linux and macOS" >&2
            exit 1
            ;;
    esac

    case "$arch" in
        x86_64 | amd64) arch="x86_64" ;;
        arm64 | aarch64) arch="aarch64" ;;
        *)
            echo "error: unsupported architecture: $arch" >&2
            echo "error: install-release.sh currently supports x86_64 and arm64" >&2
            exit 1
            ;;
    esac

    if [[ "$os" == "linux" ]]; then
        printf 'mst-%s-%s-%s-gnu.tar.gz\n' "$version" "$os" "$arch"
    else
        printf 'mst-%s-%s-%s.tar.gz\n' "$version" "$os" "$arch"
    fi
}

have_backend_tools() {
    if ! command -v clang-18 >/dev/null 2>&1 && ! command -v clang >/dev/null 2>&1; then
        return 1
    fi

    if ! command -v opt-18 >/dev/null 2>&1 && ! command -v opt >/dev/null 2>&1; then
        return 1
    fi

    return 0
}

is_debian_like() {
    if [[ ! -f /etc/os-release ]]; then
        return 1
    fi

    # shellcheck disable=SC1091
    . /etc/os-release

    case " ${ID:-} ${ID_LIKE:-} " in
        *" debian "* | *" ubuntu "*)
            return 0
            ;;
    esac

    return 1
}

print_backend_tool_help() {
    if is_debian_like; then
        echo "[mst] install LLVM tools with:" >&2
        echo "sudo apt-get update && sudo $APT_LLVM_INSTALL_CMD" >&2
    elif [[ "$(uname -s)" == "Darwin" ]]; then
        echo "[mst] install LLVM tools and make sure clang and opt are on PATH" >&2
        if command -v brew >/dev/null 2>&1; then
            echo "[mst] with Homebrew:" >&2
            echo "brew install llvm" >&2
            echo 'export PATH="$(brew --prefix llvm)/bin:$PATH"' >&2
        fi
    else
        echo "[mst] install LLVM tools and make sure one of each is available on PATH:" >&2
        echo "[mst]   clang-18 or clang" >&2
        echo "[mst]   opt-18 or opt" >&2
    fi
}

install_backend_tools_with_apt() {
    local elevate=()

    if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
        if ! command -v sudo >/dev/null 2>&1; then
            echo "[mst] warning: sudo is required for automatic LLVM installation" >&2
            print_backend_tool_help
            return 0
        fi

        elevate=(sudo)
    fi

    echo "[mst] installing LLVM toolchain..."
    "${elevate[@]}" apt-get update
    "${elevate[@]}" apt-get install -y clang-18 llvm-18 llvm-18-tools
}

maybe_install_backend_tools() {
    local reply

    if have_backend_tools; then
        return 0
    fi

    echo "[mst] warning: clang-18/clang and opt-18/opt are required for 'mst build' and 'mst run'" >&2

    if ! is_debian_like; then
        echo "[mst] automatic installation is only available on Ubuntu/Debian" >&2
        print_backend_tool_help
        return 0
    fi

    if ! { exec 3<>/dev/tty; } 2>/dev/null; then
        echo "[mst] non-interactive environment detected; skipping automatic LLVM installation" >&2
        print_backend_tool_help
        return 0
    fi

    printf 'LLVM tools are missing. Install now with apt? [Y/n] ' >&3
    read -r reply <&3 || reply="n"
    exec 3>&-
    exec 3<&-

    case "$reply" in
        "" | [Yy] | [Yy][Ee][Ss])
            install_backend_tools_with_apt
            ;;
        *)
            print_backend_tool_help
            ;;
    esac
}

verify_checksum() {
    local checksum_file="$1"
    local expected_hash
    local actual_hash

    expected_hash="$(awk '{print $1}' "$TMP_DIR/$checksum_file")"

    if command -v sha256sum >/dev/null 2>&1; then
        actual_hash="$(sha256sum "$TMP_DIR/$ASSET_NAME" | awk '{print $1}')"
    elif command -v shasum >/dev/null 2>&1; then
        actual_hash="$(shasum -a 256 "$TMP_DIR/$ASSET_NAME" | awk '{print $1}')"
    else
        echo "error: required command not found: sha256sum or shasum" >&2
        exit 1
    fi

    if [[ "$expected_hash" != "$actual_hash" ]]; then
        echo "error: checksum verification failed for $ASSET_NAME" >&2
        exit 1
    fi

    echo "$ASSET_NAME: OK"
}

need_cmd curl
need_cmd tar

VERSION="$(resolve_version)"
ASSET_NAME="$(detect_asset_name "$VERSION")"
CHECKSUM_NAME="${ASSET_NAME}.sha256"
DOWNLOAD_BASE="https://github.com/$REPO/releases/download/$VERSION"
PACKAGE_DIR="${ASSET_NAME%.tar.gz}"
TMP_DIR="$(mktemp -d)"

cleanup() {
    rm -rf "$TMP_DIR"
}

trap cleanup EXIT

echo "[mst] downloading $ASSET_NAME..."
curl -fsSL -o "$TMP_DIR/$ASSET_NAME" "$DOWNLOAD_BASE/$ASSET_NAME"
curl -fsSL -o "$TMP_DIR/$CHECKSUM_NAME" "$DOWNLOAD_BASE/$CHECKSUM_NAME"

echo "[mst] verifying checksum..."
verify_checksum "$CHECKSUM_NAME"

echo "[mst] extracting release..."
tar -xzf "$TMP_DIR/$ASSET_NAME" -C "$TMP_DIR"

mkdir -p "$BIN_DIR"
install -m 755 "$TMP_DIR/$PACKAGE_DIR/mst" "$BIN_DIR/mst"

if [[ -d "$TMP_DIR/$PACKAGE_DIR/std" ]]; then
    rm -rf "$STD_DIR"
    mkdir -p "$(dirname "$STD_DIR")"
    cp -R "$TMP_DIR/$PACKAGE_DIR/std" "$STD_DIR"
    echo "[mst] installed std to $STD_DIR"
else
    echo "[mst] warning: release package does not contain std/" >&2
fi

echo "[mst] installed to $BIN_DIR/mst"
maybe_install_backend_tools

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
        echo "[mst] note: $BIN_DIR is not on PATH"
        echo "[mst] add this to your shell profile:"
        echo "export PATH=\"$BIN_DIR:\$PATH\""
        ;;
esac

echo "[mst] try: mst --help"
