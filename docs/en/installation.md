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
