# roxy

---

**English** · [Русский](i18n/README.ru.md) · [Українська](i18n/README.uk.md) · [Беларуская](i18n/README.be.md) · [Polski](i18n/README.pl.md) · [Deutsch](i18n/README.de.md) · [Français](i18n/README.fr.md) · [Español](i18n/README.es.md) · [中文](i18n/README.zh-CN.md) · [日本語](i18n/README.ja.md)

---

**High-performance MCP (Model Context Protocol) proxy server written in Rust.**

roxy bridges MCP clients (Claude Desktop, Cursor, Zed, etc.) to any upstream handler running as a **FastCGI** backend (e.g. PHP-FPM) or an **HTTP(S)** endpoint. Rust handles everything performance-critical — transport, protocol parsing, connection pooling, concurrency — via the official [`rmcp`](https://crates.io/crates/rmcp) crate. Your handler only deals with a small, simplified JSON protocol and returns results.

This lets you write MCP servers in **any language** — PHP, Python, Node, Go, Ruby — without reimplementing JSON-RPC framing, transport, session management, or capability negotiation every time.

## Table of contents

- [Features](#features)
- [Installation](#installation)
  - [Homebrew (macOS and Linux)](#homebrew-macos-and-linux)
  - [Install script (any Unix)](#install-script-any-unix)
  - [Debian / Ubuntu (.deb)](#debian--ubuntu-deb)
  - [Fedora / RHEL / openSUSE (.rpm)](#fedora--rhel--opensuse-rpm)
  - [Alpine / any Linux (static tarball)](#alpine--any-linux-static-tarball)
  - [From source](#from-source)
  - [Verify the installation](#verify-the-installation)
- [Quick start](#quick-start)
- [CLI reference](#cli-reference)
- [Writing an upstream handler](#writing-an-upstream-handler)
- [Upstream protocol reference](#upstream-protocol-reference)
  - [Request types](#request-types)
  - [Elicitation (multi-turn tool input)](#elicitation-multi-turn-tool-input)
  - [Error response](#error-response)
- [Architecture](#architecture)
- [Development](#development)
- [License](#license)

## Features

- **Multi-backend**: FastCGI (TCP or Unix socket) and HTTP(S) upstreams, auto-detected from URL format
- **Transports**: stdio and HTTP/SSE, both supported natively via `rmcp`
- **MCP 2025-06-18 features**: elicitation (multi-turn tool input), structured tool output, resource links in tool responses
- **Connection pooling** for FastCGI (via `deadpool`)
- **TLS via rustls** — no OpenSSL dependency, fully static musl builds
- **Capability caching** — tools/resources/prompts discovered once at startup
- **Custom HTTP headers**, configurable timeouts, request/session IDs propagated to upstream

## Installation

Prebuilt binaries are published on every tagged release for **macOS (arm64, x86_64)** and **Linux (arm64, x86_64, musl-static)**. Pick whichever method suits your platform.

### Homebrew (macOS and Linux)

```bash
brew tap petstack/tap
brew install roxy
```

Works on macOS (Intel and Apple Silicon) and Linux (x86_64 and arm64) with Homebrew installed.

### Install script (any Unix)

```bash
curl -sSfL https://raw.githubusercontent.com/petstack/roxy/main/install.sh | sh
```

The script auto-detects your OS and architecture, downloads the right tarball from GitHub Releases, verifies the SHA256 checksum, and installs to `/usr/local/bin/roxy` (using `sudo` if needed).

Options:

```bash
# Install a specific version
curl -sSfL .../install.sh | sh -s -- --version v0.1.0

# Install to a custom directory (no sudo needed)
curl -sSfL .../install.sh | sh -s -- --bin-dir $HOME/.local/bin
```

Environment variables `ROXY_REPO`, `ROXY_VERSION`, `ROXY_BIN_DIR` work too.

### Debian / Ubuntu (.deb)

```bash
# amd64
curl -sSfLO https://github.com/petstack/roxy/releases/latest/download/roxy_0.1.0-1_amd64.deb
sudo dpkg -i roxy_0.1.0-1_amd64.deb

# arm64
curl -sSfLO https://github.com/petstack/roxy/releases/latest/download/roxy_0.1.0-1_arm64.deb
sudo dpkg -i roxy_0.1.0-1_arm64.deb
```

### Fedora / RHEL / openSUSE (.rpm)

```bash
# x86_64
sudo rpm -i https://github.com/petstack/roxy/releases/latest/download/roxy-0.1.0-1.x86_64.rpm

# aarch64
sudo rpm -i https://github.com/petstack/roxy/releases/latest/download/roxy-0.1.0-1.aarch64.rpm
```

### Alpine / any Linux (static tarball)

The Linux binaries are statically linked against musl libc, so they run on **any** Linux distribution without dependencies:

```bash
# Pick your architecture
ARCH=x86_64   # or aarch64
curl -sSfL https://github.com/petstack/roxy/releases/latest/download/roxy-v0.1.0-${ARCH}-unknown-linux-musl.tar.gz | tar -xz
sudo install -m 755 roxy-v0.1.0-${ARCH}-unknown-linux-musl/roxy /usr/local/bin/
```

Works on Alpine, Debian, Ubuntu, RHEL, Arch, Amazon Linux, Void, NixOS, and anything else with a Linux kernel.

### From source

Requires [Rust](https://rustup.rs/) (edition 2024, stable toolchain):

```bash
git clone https://github.com/petstack/roxy
cd roxy
cargo build --release
# Binary is at ./target/release/roxy
```

Or via `cargo install`:

```bash
cargo install --git https://github.com/petstack/roxy
```

### Verify the installation

```bash
roxy --version
roxy --help
```

## Quick start

roxy needs **one argument**: `--upstream`, pointing at your handler. The upstream type is **auto-detected** from the URL format:

| URL format | Backend type |
|---|---|
| `http://...` or `https://...` | HTTP(S) |
| `host:port` | FastCGI TCP |
| `/path/to/socket` | FastCGI Unix socket |

### Example: HTTP backend

```bash
# Start your HTTP handler on port 8000 (any language, any framework)
# Then point roxy at it:
roxy --upstream http://localhost:8000/mcp
```

### Example: PHP-FPM backend

```bash
# Start PHP-FPM
php-fpm --nodaemonize \
    -d "listen=127.0.0.1:9000" \
    -d "pm=static" \
    -d "pm.max_children=4"

# Point roxy at it
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /absolute/path/to/handler.php
```

### Connect from an MCP client

For Claude Desktop or any client that spawns MCP servers as subprocesses (stdio transport — the default):

```json
{
  "mcpServers": {
    "my-server": {
      "command": "roxy",
      "args": ["--upstream", "http://localhost:8000/mcp"]
    }
  }
}
```

For network clients that connect over HTTP/SSE:

```bash
roxy --transport http --port 8080 --upstream http://localhost:8000/mcp
# Client connects to http://localhost:8080/sse
```

## CLI reference

```
roxy [OPTIONS] --upstream <UPSTREAM>
```

| Flag | Default | Description |
|---|---|---|
| `--upstream <URL>` | **required** | Backend URL. Auto-detects type (see table above) |
| `--transport <MODE>` | `stdio` | MCP client transport: `stdio` or `http` |
| `--port <PORT>` | `8080` | HTTP listen port (only with `--transport http`) |
| `--upstream-entrypoint <PATH>` | — | `SCRIPT_FILENAME` sent to FastCGI backends (required for PHP-FPM) |
| `--upstream-insecure` | `false` | Skip TLS certificate verification for HTTPS upstreams |
| `--upstream-timeout <SECS>` | `30` | HTTP upstream request timeout in seconds |
| `--upstream-header <HEADER>` | — | Custom HTTP header, `Name: Value`. Repeatable |
| `--pool-size <N>` | `16` | FastCGI connection pool size |
| `--log-format <FORMAT>` | `pretty` | Log output: `pretty` or `json` |

Log **level** is controlled via the `RUST_LOG` environment variable:

```bash
RUST_LOG=debug roxy --upstream http://localhost:8000/mcp
RUST_LOG=roxy=debug,rmcp=info roxy --upstream ...  # per-module filters
```

### Full HTTP backend example

```bash
roxy --upstream https://api.example.com/mcp \
     --upstream-header "Authorization: Bearer $TOKEN" \
     --upstream-header "X-Tenant: acme" \
     --upstream-timeout 60
```

### Full FastCGI (PHP-FPM) example

```bash
# TCP
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /srv/app/handler.php

# Unix socket
roxy --upstream /var/run/php-fpm.sock --upstream-entrypoint /srv/app/handler.php

# HTTP transport with FastCGI upstream
roxy --transport http --port 8080 \
     --upstream 127.0.0.1:9000 \
     --upstream-entrypoint /srv/app/handler.php
```

## Writing an upstream handler

Your handler receives simple JSON requests and returns simple JSON responses. **It never sees JSON-RPC, MCP framing, or session state.** roxy translates everything.

### For HTTP backends

Any HTTP server that reads JSON from the request body and writes JSON to the response will work. Example in Python/Flask:

```python
from flask import Flask, request, jsonify
app = Flask(__name__)

@app.post("/mcp")
def handler():
    req = request.json
    match req["type"]:
        case "discover":
            return jsonify({
                "tools": [{"name": "echo", "description": "Echo", "input_schema": {...}}],
                "resources": [],
                "prompts": [],
            })
        case "call_tool":
            return jsonify({"content": [{"type": "text", "text": req["arguments"]["message"]}]})
        case _:
            return jsonify({"error": {"code": 400, "message": "unknown"}}), 200
```

### For FastCGI (PHP-FPM) backends

A minimal PHP handler:

```php
<?php
$req = json_decode(file_get_contents('php://input'), true);
header('Content-Type: application/json');

echo json_encode(match ($req['type']) {
    'discover' => [
        'tools' => [[
            'name' => 'echo',
            'description' => 'Echoes back input',
            'input_schema' => [
                'type' => 'object',
                'properties' => ['message' => ['type' => 'string']],
                'required' => ['message'],
            ],
        ]],
        'resources' => [],
        'prompts' => [],
    ],
    'call_tool' => [
        'content' => [['type' => 'text', 'text' => $req['arguments']['message']]],
    ],
    default => ['error' => ['code' => 400, 'message' => 'unknown type']],
});
```

See [`examples/handler.php`](examples/handler.php) for a full example with multiple tools, structured output, elicitation, and resource links.

## Upstream protocol reference

Every request from roxy to your upstream is a JSON object with these common envelope fields:

```json
{
  "type": "...",
  "session_id": "optional-uuid-or-null",
  "request_id": "uuid-per-request",
  ...
}
```

### Request types

#### `discover`

Sent once at roxy startup. Your handler must return the full catalog of tools, resources, and prompts it supports. roxy caches the result and serves it to all MCP clients without re-querying.

```json
// Response
{
  "tools": [
    {
      "name": "tool_name",
      "title": "Human Name",
      "description": "What it does",
      "input_schema": { "type": "object", "properties": {...}, "required": [...] },
      "output_schema": { "type": "object", "properties": {...} }
    }
  ],
  "resources": [
    {
      "uri": "myapp://resource-id",
      "name": "display-name",
      "title": "Human Name",
      "description": "...",
      "mime_type": "application/json"
    }
  ],
  "prompts": [
    {
      "name": "prompt_name",
      "title": "Human Name",
      "description": "...",
      "arguments": [
        { "name": "arg", "title": "Arg", "description": "...", "required": true }
      ]
    }
  ]
}
```

All `title`, `description`, `mime_type`, `output_schema` fields are optional.

#### `call_tool`

Execute a tool by name. Request:

```json
{
  "type": "call_tool",
  "name": "tool_name",
  "arguments": { "key": "value" },
  "elicitation_results": [ ... ],   // optional: see Elicitation below
  "context": { ... }                 // optional: echoed from a previous elicit response
}
```

Success response (regular text output):

```json
{
  "content": [
    { "type": "text", "text": "result" }
  ]
}
```

Success response with **structured content** (for tools that define `output_schema`):

```json
{
  "content": [{ "type": "text", "text": "5 + 3 = 8" }],
  "structured_content": { "sum": 8, "operands": { "a": 5, "b": 3 } }
}
```

Success response with a **resource link** embedded in the output:

```json
{
  "content": [
    { "type": "text", "text": "Booking confirmed." },
    {
      "type": "resource_link",
      "uri": "myapp://bookings/1234",
      "name": "booking-1234",
      "title": "Booking #1234"
    }
  ]
}
```

#### `read_resource`

Read a resource by URI. Request:

```json
{ "type": "read_resource", "uri": "myapp://status" }
```

Response: same `content` format as `call_tool`.

#### `get_prompt`

Generate a prompt. Request:

```json
{ "type": "get_prompt", "name": "greet", "arguments": { "name": "Alice" } }
```

Response: same `content` format as `call_tool`.

#### `elicitation_cancelled`

Sent when the MCP client cancels an elicitation (see below). Your handler can log/cleanup; the response is ignored.

```json
{ "type": "elicitation_cancelled", "name": "tool_name", "action": "decline", "context": {...} }
```

### Elicitation (multi-turn tool input)

A tool can **request more input from the user** mid-execution. On the first `call_tool`, return an `elicit` response instead of `content`:

```json
{
  "elicit": {
    "message": "Which flight class?",
    "schema": {
      "type": "object",
      "properties": {
        "class": { "type": "string", "enum": ["economy", "business", "first"] }
      },
      "required": ["class"]
    },
    "context": { "step": 1, "destination": "Tokyo" }
  }
}
```

roxy forwards the elicitation to the MCP client. When the user fills it in, roxy calls your tool **again** with the collected values in `elicitation_results` and your original `context` echoed back:

```json
{
  "type": "call_tool",
  "name": "book_flight",
  "arguments": { "destination": "Tokyo" },
  "elicitation_results": [{ "class": "business" }],
  "context": { "step": 1, "destination": "Tokyo" }
}
```

You can chain multiple elicitation rounds by returning another `elicit` until all data is collected.

### Error response

Any request type can return an error instead of success:

```json
{ "error": { "code": 404, "message": "Unknown tool: foo" } }
```

## Architecture

```
MCP Client (Claude, Cursor, Zed, ...)
       │
       │ JSON-RPC over stdio or HTTP/SSE
       ▼
┌──────────────┐
│    rmcp      │  MCP protocol, transport, sessions
└──────────────┘
       │
       ▼
┌──────────────┐
│  RoxyServer  │  routes MCP methods, caches capabilities
└──────────────┘
       │
       │ simplified JSON (UpstreamEnvelope + UpstreamRequest)
       ▼
┌──────────────────────────┐
│    UpstreamExecutor      │  trait with 2 implementations
│  ┌────────┬───────────┐  │
│  │FastCgi │  Http     │  │
│  └────────┴───────────┘  │
└──────────────────────────┘
       │                │
       ▼                ▼
   PHP-FPM /        HTTP(S)
   any FastCGI      endpoint
```

### Source layout

```
src/
  main.rs             CLI, logging, transport startup, executor selection
  config.rs           clap Config, UpstreamKind (auto-detect), FcgiAddress
  protocol.rs         Internal JSON types (UpstreamEnvelope, UpstreamRequest, ...)
  server.rs           RoxyServer: rmcp ServerHandler impl + discover caching
  executor/
    mod.rs            UpstreamExecutor trait
    fastcgi.rs        FastCgiExecutor: deadpool + fastcgi-client
    http.rs           HttpExecutor: reqwest + rustls
examples/
  handler.php         Full example PHP handler with all features
```

### Key design decisions

- **rmcp does the heavy lifting.** The official `rmcp` crate handles all MCP protocol complexity (JSON-RPC, transport negotiation, session management). roxy only implements `ServerHandler`.
- **Upstream is pluggable.** The `UpstreamExecutor` trait abstracts backend communication. FastCGI and HTTP are the current implementations; adding a new backend (gRPC, stdio, WebSocket) is a matter of implementing one trait.
- **Capabilities are cached.** roxy calls `discover` once at startup and caches tools/resources/prompts in memory. MCP clients get instant `initialize` responses without hitting the upstream.
- **Connection pooling for FastCGI.** `deadpool` keeps connections to PHP-FPM warm, avoiding per-request socket setup.
- **Pure-Rust TLS via rustls.** No OpenSSL, no system libraries. Fully static Linux builds, easy cross-compilation, portable binaries.
- **Upstream stays dumb.** Your handler never sees JSON-RPC, request IDs (other than as an opaque envelope field), session state, or MCP framing. It's plain JSON in, plain JSON out.

## Development

### Build & test

```bash
cargo build           # debug
cargo build --release # optimized
cargo test            # run tests
cargo clippy          # lint
cargo fmt             # format
```

### Running locally with the example PHP handler

```bash
# Terminal 1: start PHP-FPM
php-fpm --nodaemonize -d "listen=127.0.0.1:9000" -d "pm=static" -d "pm.max_children=4"

# Terminal 2: run roxy with the example handler
cargo run -- \
    --upstream 127.0.0.1:9000 \
    --upstream-entrypoint "$(pwd)/examples/handler.php"
```

Then connect with any MCP client, or send JSON-RPC manually over stdio.

### Release workflow

Tagged releases (`git tag vX.Y.Z && git push origin vX.Y.Z`) trigger `.github/workflows/release.yml`, which:

1. Builds release binaries for all four targets (macOS arm64/x86_64, Linux musl arm64/x86_64)
2. Packages them as `.tar.gz` with SHA256 checksums
3. Builds `.deb` and `.rpm` packages for both Linux architectures
4. Publishes a GitHub Release with all artifacts
5. Bumps the Homebrew formula in [`petstack/homebrew-tap`](https://github.com/petstack/homebrew-tap) (if `HOMEBREW_TAP_TOKEN` secret is set)

See [`packaging/homebrew/README.md`](packaging/homebrew/README.md) for tap setup.

## License

[AGPL-3.0-only](LICENSE). If you run a modified version of roxy as a network service, you must make your modifications available to users of that service.
