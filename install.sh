#!/bin/sh
# roxy installer
#
# Usage:
#   curl -sSfL https://raw.githubusercontent.com/petstack/roxy/main/install.sh | sh
#   curl -sSfL .../install.sh | sh -s -- --version v0.1.0
#   curl -sSfL .../install.sh | sh -s -- --bin-dir $HOME/.local/bin
#
# Environment variables:
#   ROXY_REPO     — override GitHub repo (default: petstack/roxy)
#   ROXY_VERSION  — override version tag (default: latest)
#   ROXY_BIN_DIR  — override install directory (default: /usr/local/bin)

set -eu

REPO="${ROXY_REPO:-petstack/roxy}"
VERSION="${ROXY_VERSION:-}"
BIN_DIR="${ROXY_BIN_DIR:-/usr/local/bin}"
BIN_NAME="roxy"

usage() {
    cat <<EOF
roxy installer

Usage: install.sh [options]

Options:
  --version <tag>    Install a specific version (e.g. v0.1.0). Defaults to latest.
  --bin-dir <path>   Install to custom directory. Default: /usr/local/bin
  -h, --help         Show this help

You can also set ROXY_REPO, ROXY_VERSION, ROXY_BIN_DIR as environment variables.
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version)
            [ $# -ge 2 ] || { echo "error: --version requires a value" >&2; exit 1; }
            VERSION="$2"; shift 2
            ;;
        --bin-dir)
            [ $# -ge 2 ] || { echo "error: --bin-dir requires a value" >&2; exit 1; }
            BIN_DIR="$2"; shift 2
            ;;
        -h|--help)
            usage; exit 0
            ;;
        *)
            echo "error: unknown argument: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

require() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "error: required tool '$1' not found in PATH" >&2
        exit 1
    }
}

require uname
require tar
require mktemp

# Prefer curl, fall back to wget
if command -v curl >/dev/null 2>&1; then
    DL() { curl -sSfL "$1" -o "$2"; }
    DL_STDOUT() { curl -sSfL "$1"; }
elif command -v wget >/dev/null 2>&1; then
    DL() { wget -q -O "$2" "$1"; }
    DL_STDOUT() { wget -q -O - "$1"; }
else
    echo "error: neither curl nor wget found" >&2
    exit 1
fi

# Detect OS
OS="$(uname -s)"
case "$OS" in
    Darwin) OS_TAG="apple-darwin" ;;
    Linux)  OS_TAG="unknown-linux-musl" ;;
    *)
        echo "error: unsupported OS: $OS" >&2
        exit 1
        ;;
esac

# Detect arch
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64) ARCH_TAG="x86_64" ;;
    aarch64|arm64) ARCH_TAG="aarch64" ;;
    *)
        echo "error: unsupported architecture: $ARCH" >&2
        exit 1
        ;;
esac

TARGET="${ARCH_TAG}-${OS_TAG}"

# Guard against target combinations we don't publish
case "$TARGET" in
    aarch64-apple-darwin|x86_64-apple-darwin|x86_64-unknown-linux-musl|aarch64-unknown-linux-musl) ;;
    *)
        echo "error: no prebuilt binary available for $TARGET" >&2
        echo "       supported: aarch64-apple-darwin, x86_64-apple-darwin, x86_64-unknown-linux-musl, aarch64-unknown-linux-musl" >&2
        exit 1
        ;;
esac

# Resolve version if not pinned
if [ -z "$VERSION" ]; then
    echo "Resolving latest release..."
    VERSION="$(DL_STDOUT "https://api.github.com/repos/${REPO}/releases/latest" \
        | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' \
        | head -1)"
    if [ -z "$VERSION" ]; then
        echo "error: failed to resolve latest version from GitHub API" >&2
        exit 1
    fi
fi

ARCHIVE="roxy-${VERSION}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE}"
SHA_URL="${URL}.sha256"

echo "Installing ${BIN_NAME} ${VERSION} (${TARGET})"
echo "  from: $URL"
echo "  to:   ${BIN_DIR}/${BIN_NAME}"

TMP="$(mktemp -d 2>/dev/null || mktemp -d -t roxy-install)"
trap 'rm -rf "$TMP"' EXIT INT TERM

echo "Downloading archive..."
DL "$URL" "$TMP/$ARCHIVE"

echo "Downloading checksum..."
DL "$SHA_URL" "$TMP/$ARCHIVE.sha256"

# Verify checksum
echo "Verifying checksum..."
(
    cd "$TMP"
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 -c "$ARCHIVE.sha256" >/dev/null
    elif command -v sha256sum >/dev/null 2>&1; then
        sha256sum -c "$ARCHIVE.sha256" >/dev/null
    else
        echo "warning: no shasum/sha256sum available, skipping checksum verification" >&2
    fi
)

echo "Extracting..."
tar -xzf "$TMP/$ARCHIVE" -C "$TMP"

STAGING="$TMP/roxy-${VERSION}-${TARGET}"
if [ ! -f "$STAGING/$BIN_NAME" ]; then
    echo "error: binary not found at $STAGING/$BIN_NAME" >&2
    exit 1
fi

# Ensure target dir exists
if [ ! -d "$BIN_DIR" ]; then
    echo "Creating $BIN_DIR..."
    if ! mkdir -p "$BIN_DIR" 2>/dev/null; then
        sudo mkdir -p "$BIN_DIR"
    fi
fi

# Install with sudo if needed
if [ -w "$BIN_DIR" ]; then
    install -m 755 "$STAGING/$BIN_NAME" "$BIN_DIR/$BIN_NAME"
else
    echo "No write permission to $BIN_DIR, using sudo..."
    sudo install -m 755 "$STAGING/$BIN_NAME" "$BIN_DIR/$BIN_NAME"
fi

echo ""
echo "Installed successfully:"
"$BIN_DIR/$BIN_NAME" --version

# Warn if BIN_DIR isn't on PATH
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
        echo ""
        echo "Warning: $BIN_DIR is not on your PATH."
        echo "Add it by appending this to your shell profile:"
        echo "  export PATH=\"$BIN_DIR:\$PATH\""
        ;;
esac
