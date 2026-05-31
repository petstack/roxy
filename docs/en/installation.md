---
title: Installation & first run
description: Install roxy on macOS, Linux, or Windows, then connect your first backend and an MCP client in two steps.
---

# Installation & first run

## Installing roxy

Pick whichever method matches your setup. All methods install the same `roxy` binary.

### Homebrew (macOS or Linux)

```
brew tap petstack/tap
brew install roxy
```

### One-line install script (any Unix-like system)

```
curl -sSfL https://raw.githubusercontent.com/petstack/roxy/main/install.sh | sh
```

You can pin a specific version or change the install directory:

```
curl -sSfL https://raw.githubusercontent.com/petstack/roxy/main/install.sh | sh -s -- --version v0.1.0 --bin-dir $HOME/.local/bin
```

### Debian / Ubuntu

```
curl -sSfLO https://github.com/petstack/roxy/releases/latest/download/roxy_0.1.0-1_amd64.deb
sudo dpkg -i roxy_0.1.0-1_amd64.deb
```

### Fedora / RHEL / openSUSE

```
sudo rpm -i https://github.com/petstack/roxy/releases/latest/download/roxy-0.1.0-1.x86_64.rpm
```

### Static tarball (Alpine or any Linux)

```
curl -sSfL https://github.com/petstack/roxy/releases/latest/download/roxy-v0.1.0-x86_64-unknown-linux-musl.tar.gz | tar -xz
sudo install -m 755 roxy-v0.1.0-x86_64-unknown-linux-musl/roxy /usr/local/bin/
```

### Windows (amd64 or arm64)

**Scoop** (recommended — picks the right architecture automatically):

```powershell
scoop bucket add petstack https://github.com/petstack/scoop-bucket
scoop install roxy
```

**Portable `.exe` or `.zip`** — download directly from the [Releases](https://github.com/petstack/roxy/releases) page. Each release ships a `roxy-vX.Y.Z-x86_64-pc-windows-msvc.zip` / `…-aarch64-pc-windows-msvc.zip` and a bare `roxy.exe` for each architecture (with matching `.sha256` files):

```powershell
# Example: amd64 zip
Invoke-WebRequest -Uri https://github.com/petstack/roxy/releases/latest/download/roxy-v0.1.0-x86_64-pc-windows-msvc.zip -OutFile roxy.zip
Expand-Archive roxy.zip -DestinationPath .
# roxy.exe is inside roxy-v0.1.0-x86_64-pc-windows-msvc\
```

> [!WARNING]
> On Windows, FastCGI upstreams must use a **TCP address** (`host:port`). Unix-domain-socket upstreams are a Unix-only feature — use HTTP or TCP FastCGI instead. (PHP-FPM on Windows listens over TCP anyway.)

### Check it worked

```
roxy --version
roxy --help
```

---

## Your first run in 2 steps

The shortest path from zero to a working roxy.

**Step 1 — Pick a backend.** For a quick try, use one of the bundled examples:

```
python3 examples/handler.py
```

This starts a small HTTP server on `http://127.0.0.1:8000/`.

**Step 2 — Connect your AI client.** For Claude Desktop, edit `~/.config/Claude/claude_desktop_config.json` (or its equivalent on your OS) and add:

```json
{
  "mcpServers": {
    "my-tools": {
      "command": "roxy",
      "args": ["--upstream", "http://127.0.0.1:8000/"]
    }
  }
}
```

Restart Claude Desktop. Your tools will appear in the MCP panel. Done.

Claude Desktop spawns `roxy` for you as a subprocess — you don't need to run it in a terminal yourself. If you want to run roxy as a long-lived network server instead (for remote clients or a team deployment), see [Transport modes](configuration.md#transport-modes--stdio-vs-http).

---

## Packaging roxy as a Claude Desktop extension (`.mcpb`)

An `.mcpb` bundle is a single file that installs roxy into **Claude Desktop** as a managed extension — no hand-editing of `claude_desktop_config.json`. The bundle ships the `roxy` binary plus a manifest, and Claude Desktop renders a small setup form (your **Upstream URL** and an optional **Upstream Header**) when you install it. It's the friendliest way to hand roxy to someone who isn't comfortable editing JSON.

> [!NOTE]
> Releases don't ship `.mcpb` files today — you build the bundle yourself with the script below. A bundle is **platform-specific**: each one pins a single OS + CPU, so build (or hand out) the bundle that matches the target machine.

### Build a bundle

The helper script is [`scripts/mcpb.sh`](https://github.com/petstack/roxy/blob/main/scripts/mcpb.sh). It writes one `.mcpb` per platform/architecture into `target/mcpb/`, each next to a `.sha256` checksum.

It can obtain the `roxy` binary three ways:

```bash
# 1. Build from source with cargo (needs the Rust toolchain for the target)
./scripts/mcpb.sh --platform darwin-arm64

# 2. Download a prebuilt binary from a GitHub release (no toolchain needed)
./scripts/mcpb.sh --from-release vX.Y.Z --platform darwin-arm64

# 3. Wrap a binary you already have (single bundle, host platform auto-detected)
./scripts/mcpb.sh --bin ./target/release/roxy
```

With no arguments it builds **every** supported target — macOS arm64/x64, Linux musl arm64/x64, and Windows arm64/x64:

```bash
./scripts/mcpb.sh
```

Choose targets with `--platform` (friendly, comma-separated) or `--target` (exact Rust triple, repeatable):

| `--platform` value | Builds for |
|---|---|
| `all` | every supported target |
| `darwin` / `linux` / `windows` | both architectures of that OS |
| `darwin-arm64`, `darwin-x64` | one macOS architecture |
| `linux-arm64`, `linux-x64` | one Linux (musl-static) architecture |
| `windows-arm64`, `windows-x64` | one Windows (MSVC) architecture |

```bash
# Several at once — e.g. both Linux arches plus 64-bit Windows
./scripts/mcpb.sh --from-release vX.Y.Z --platform linux,windows-x64
```

Other flags:

- `--out DIR` — output directory (default `target/mcpb`)
- `--name SLUG` — override the extension name and bundle filename (default `roxy`)
- `--display-name STR` — override the name shown in Claude Desktop (default `Roxy — MCP Gateway`)
- `-h`, `--help` — full usage

**Requirements:** `zip`, plus `cargo` (unless you use `--from-release` or `--bin`) and `curl` (for `--from-release`). `jq` is optional, used only for manifest validation.

> [!TIP]
> `--from-release` is the quickest path: it pulls the already-compiled binaries from the [Releases](https://github.com/petstack/roxy/releases) page, so you can produce bundles for every OS from a single machine without any cross-compilation toolchains.

### Install the bundle

1. Open **Claude Desktop → Settings → Extensions**.
2. Add the `.mcpb` file — drag it onto the window, or use the **Install extension…** control and pick the file.
3. Fill in the **Upstream URL** (for example `http://127.0.0.1:8000/` for an HTTP backend, or `127.0.0.1:9000` for FastCGI over TCP) and an optional **Upstream Header**, then enable the extension.

That's the same configuration you'd otherwise pass via `--upstream` / `--upstream-header` — the bundle just wires it into a form for you.
