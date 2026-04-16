#!/usr/bin/env bash
# scripts/mcpb.sh — Package roxy as .mcpb bundles (one per platform).
#
# Usage:
#   ./scripts/mcpb.sh                          # build all targets
#   ./scripts/mcpb.sh --target aarch64-apple-darwin
#   ./scripts/mcpb.sh --platform darwin        # both darwin archs
#   ./scripts/mcpb.sh --platform darwin-arm64  # single platform+arch
#   ./scripts/mcpb.sh --platform linux,darwin-x64
#   ./scripts/mcpb.sh --from-release v0.1.0    # download from GitHub release
#   ./scripts/mcpb.sh --bin path/to/roxy       # use a local binary (single target)
#
# Requires: cargo (unless --from-release or --bin), zip, jq (optional, for validation).

set -euo pipefail

# ── defaults ──────────────────────────────────────────────────────────
REPO="petstack/roxy"
MANIFEST_VERSION="0.3"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
OUT_DIR="$ROOT_DIR/target/mcpb"

ALL_TARGETS=(
  aarch64-apple-darwin
  x86_64-apple-darwin
  x86_64-unknown-linux-musl
  aarch64-unknown-linux-musl
)

# ── parse args ────────────────────────────────────────────────────────
TARGETS=()
FROM_RELEASE=""
LOCAL_BIN=""
NAME=""
DISPLAY_NAME=""

# Expand a --platform value (e.g. "darwin", "darwin-arm64", "linux-x64") to
# one or more rust targets from ALL_TARGETS. Accepts comma-separated lists.
platform_to_targets() {
  local spec="$1"
  local item
  local -a out=()
  IFS=',' read -ra items <<< "$spec"
  for item in "${items[@]}"; do
    case "$item" in
      all)           out+=("${ALL_TARGETS[@]}") ;;
      darwin)        out+=(aarch64-apple-darwin x86_64-apple-darwin) ;;
      linux)         out+=(x86_64-unknown-linux-musl aarch64-unknown-linux-musl) ;;
      darwin-arm64)  out+=(aarch64-apple-darwin) ;;
      darwin-x64)    out+=(x86_64-apple-darwin) ;;
      linux-arm64)   out+=(aarch64-unknown-linux-musl) ;;
      linux-x64)     out+=(x86_64-unknown-linux-musl) ;;
      *)
        echo "Unknown --platform value: $item" >&2
        echo "Valid: all, darwin, linux, darwin-arm64, darwin-x64, linux-arm64, linux-x64" >&2
        exit 1
        ;;
    esac
  done
  printf '%s\n' "${out[@]}"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)       TARGETS+=("$2"); shift 2 ;;
    --platform)
      while IFS= read -r t; do TARGETS+=("$t"); done < <(platform_to_targets "$2")
      shift 2
      ;;
    --from-release) FROM_RELEASE="$2"; shift 2 ;;
    --bin)          LOCAL_BIN="$2"; shift 2 ;;
    --out)          OUT_DIR="$2"; shift 2 ;;
    --name)         NAME="$2"; shift 2 ;;
    --display-name) DISPLAY_NAME="$2"; shift 2 ;;
    -h|--help)
      cat <<USAGE
Usage: $0 [options]

  --target TARGET      rust target triple (repeatable)
  --platform SPEC      shortcut, comma-separated. Values:
                         all, darwin, linux,
                         darwin-arm64, darwin-x64,
                         linux-arm64, linux-x64
  --from-release TAG   download prebuilt binaries from GitHub release
  --bin PATH           use a local binary (single target, host-detected)
  --out DIR            output directory (default: target/mcpb)
  --name SLUG          override extension name slug (default: roxy)
                         affects manifest "name" and bundle filename
  --display-name STR   override extension display name
                         (default: derived from --name or "Roxy — MCP Proxy")
USAGE
      exit 0
      ;;
    *) echo "Unknown flag: $1" >&2; exit 1 ;;
  esac
done

if [[ ${#TARGETS[@]} -eq 0 && -z "$LOCAL_BIN" ]]; then
  TARGETS=("${ALL_TARGETS[@]}")
fi

# ── read version from Cargo.toml ─────────────────────────────────────
VERSION="$(grep '^version' "$ROOT_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)"/\1/')"

# ── resolve names ─────────────────────────────────────────────────────
NAME="${NAME:-roxy}"
if [[ -z "$DISPLAY_NAME" ]]; then
  if [[ "$NAME" == "roxy" ]]; then
    DISPLAY_NAME="Roxy — MCP Proxy"
  else
    DISPLAY_NAME="$NAME"
  fi
fi
echo "==> $NAME version: $VERSION"

# ── helpers ───────────────────────────────────────────────────────────
target_to_platform() {
  case "$1" in
    *-apple-darwin)        echo "darwin"  ;;
    *-unknown-linux-musl)  echo "linux"   ;;
    *-windows-*)           echo "win32"   ;;
    *) echo "unknown" ;;
  esac
}

target_to_arch() {
  case "$1" in
    aarch64-*) echo "arm64"  ;;
    x86_64-*)  echo "x64"    ;;
    *) echo "unknown" ;;
  esac
}

binary_name() {
  case "$1" in
    *-windows-*) echo "roxy.exe" ;;
    *)           echo "roxy"     ;;
  esac
}

# ── acquire binary for a target ───────────────────────────────────────
acquire_binary() {
  local target="$1" dest="$2"

  if [[ -n "$FROM_RELEASE" ]]; then
    local tag="$FROM_RELEASE"
    local archive="roxy-${tag}-${target}.tar.gz"
    local url="https://github.com/${REPO}/releases/download/${tag}/${archive}"
    echo "    Downloading $url"
    local tmp
    tmp="$(mktemp -d)"
    curl -sSfL "$url" -o "$tmp/$archive"
    tar -xzf "$tmp/$archive" -C "$tmp"
    cp "$tmp"/roxy-*/roxy "$dest"
    rm -rf "$tmp"
  else
    echo "    Building cargo --release --target $target"
    cargo build --release --locked --target "$target" --manifest-path "$ROOT_DIR/Cargo.toml"
    cp "$ROOT_DIR/target/$target/release/roxy" "$dest"
  fi

  chmod +x "$dest"
}

# ── generate manifest.json ────────────────────────────────────────────
generate_manifest() {
  local platform="$1" arch="$2" bin_name="$3"

  cat <<EOF
{
  "manifest_version": "${MANIFEST_VERSION}",
  "name": "${NAME}",
  "display_name": "${DISPLAY_NAME}",
  "version": "${VERSION}",
  "description": "High-performance MCP proxy server. Bridges MCP clients to FastCGI or HTTP backends so you can write MCP handlers in any language.",
  "author": {
    "name": "petstack",
    "url": "https://github.com/${REPO}"
  },
  "license": "Apache-2.0",
  "repository": {
    "type": "git",
    "url": "https://github.com/${REPO}"
  },
  "keywords": ["proxy", "mcp", "fastcgi", "http", "php", "python"],
  "server": {
    "type": "binary",
    "entry_point": "bin/${bin_name}",
    "mcp_config": {
      "command": "\${__dirname}/bin/${bin_name}",
      "args": [
        "--transport", "stdio",
        "--upstream", "\${user_config.upstream}",
        "--upstream-header", "\${user_config.upstream_header}"
      ],
      "env": {}
    }
  },
  "compatibility": {
    "platforms": ["${platform}"]
  },
  "user_config": {
    "upstream": {
      "type": "string",
      "title": "Upstream URL",
      "description": "Backend URL. http(s)://… for HTTP, host:port for FastCGI TCP, /path/to/sock for FastCGI Unix.",
      "required": true
    },
    "upstream_header": {
      "type": "string",
      "title": "Upstream Header",
      "description": "Custom HTTP header for upstream requests, e.g. \"Authorization: Bearer token\". For multiple headers, separate with newlines.",
      "required": false,
      "default": ""
    }
  }
}
EOF
}

# ── pack one bundle ───────────────────────────────────────────────────
pack_bundle() {
  local target="$1"
  local platform arch bin_name
  platform="$(target_to_platform "$target")"
  arch="$(target_to_arch "$target")"
  bin_name="$(binary_name "$target")"

  local bundle_name="${NAME}-${VERSION}-${platform}-${arch}"
  local stage="$OUT_DIR/stage/${bundle_name}"
  local mcpb_file="$OUT_DIR/${bundle_name}.mcpb"

  echo "  ── $bundle_name ──"

  rm -rf "$stage"
  mkdir -p "$stage/bin"

  # binary
  if [[ -n "$LOCAL_BIN" ]]; then
    echo "    Using local binary: $LOCAL_BIN"
    cp "$LOCAL_BIN" "$stage/bin/$bin_name"
    chmod +x "$stage/bin/$bin_name"
  else
    acquire_binary "$target" "$stage/bin/$bin_name"
  fi

  # manifest
  generate_manifest "$platform" "$arch" "$bin_name" > "$stage/manifest.json"

  # optional assets
  [[ -f "$ROOT_DIR/icon.png" ]] && cp "$ROOT_DIR/icon.png" "$stage/"
  cp "$ROOT_DIR/LICENSE" "$stage/" 2>/dev/null || true
  cp "$ROOT_DIR/README.md" "$stage/" 2>/dev/null || true

  # zip → .mcpb
  rm -f "$mcpb_file"
  (cd "$stage" && zip -r9 "$mcpb_file" .)

  # sha256
  shasum -a 256 "$mcpb_file" > "${mcpb_file}.sha256"

  local size
  size="$(wc -c < "$mcpb_file" | tr -d ' ')"
  echo "    Created: $mcpb_file ($(( size / 1024 )) KiB)"
}

# ── main ──────────────────────────────────────────────────────────────
mkdir -p "$OUT_DIR"

echo ""
echo "==> Packaging ${NAME} ${VERSION} as .mcpb"
echo ""

if [[ -n "$LOCAL_BIN" ]]; then
  # single bundle — detect current platform
  uname_s="$(uname -s)"
  uname_m="$(uname -m)"
  case "$uname_s" in
    Darwin) plat="darwin" ;;
    Linux)  plat="linux"  ;;
    *)      plat="unknown" ;;
  esac
  case "$uname_m" in
    arm64|aarch64) arch="arm64" ;;
    x86_64)        arch="x64"   ;;
    *)             arch="unknown" ;;
  esac
  # synthesize a target for the local binary
  case "${plat}-${arch}" in
    darwin-arm64)  TARGETS=("aarch64-apple-darwin")  ;;
    darwin-x64)    TARGETS=("x86_64-apple-darwin")   ;;
    linux-arm64)   TARGETS=("aarch64-unknown-linux-musl") ;;
    linux-x64)     TARGETS=("x86_64-unknown-linux-musl")  ;;
    *)
      echo "Cannot detect target for $(uname -sm). Use --target explicitly." >&2
      exit 1
      ;;
  esac
fi

for t in "${TARGETS[@]}"; do
  pack_bundle "$t"
done

echo ""
echo "==> Done. Bundles:"
ls -lh "$OUT_DIR"/*.mcpb 2>/dev/null || true
echo ""
