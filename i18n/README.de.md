# roxy

---

[English](../README.md) · [Русский](README.ru.md) · [Українська](README.uk.md) · [Беларуская](README.be.md) · [Polski](README.pl.md) · **Deutsch** · [Français](README.fr.md) · [Español](README.es.md) · [中文](README.zh-CN.md) · [日本語](README.ja.md)

---

**Hochperformanter MCP-Proxy-Server (Model Context Protocol), geschrieben in Rust.**

roxy verbindet MCP-Clients (Claude Desktop, Cursor, Zed etc.) mit beliebigen Upstream-Handlern, die als **FastCGI**-Backend (z. B. PHP-FPM) oder **HTTP(S)**-Endpoint laufen. Rust übernimmt alles Performance-Kritische — Transport, Protocol-Parsing, Connection-Pooling, Concurrency — über den offiziellen [`rmcp`](https://crates.io/crates/rmcp) Crate. Dein Handler arbeitet nur mit einem kleinen, vereinfachten JSON-Protokoll und liefert Ergebnisse zurück.

So kannst du MCP-Server in **jeder Sprache** schreiben — PHP, Python, Node, Go, Ruby — ohne jedes Mal JSON-RPC-Framing, Transport, Session-Management und Capability-Negotiation neu zu implementieren.

## Inhaltsverzeichnis

- [Features](#features)
- [Installation](#installation)
  - [Homebrew (macOS und Linux)](#homebrew-macos-und-linux)
  - [Installations-Skript (jedes Unix)](#installations-skript-jedes-unix)
  - [Debian / Ubuntu (.deb)](#debian--ubuntu-deb)
  - [Fedora / RHEL / openSUSE (.rpm)](#fedora--rhel--opensuse-rpm)
  - [Alpine / jedes Linux (statisches Tarball)](#alpine--jedes-linux-statisches-tarball)
  - [Aus dem Quellcode](#aus-dem-quellcode)
  - [Installation prüfen](#installation-prüfen)
- [Schnellstart](#schnellstart)
- [CLI-Referenz](#cli-referenz)
- [Upstream-Handler schreiben](#upstream-handler-schreiben)
- [Upstream-Protokoll-Referenz](#upstream-protokoll-referenz)
  - [Request-Typen](#request-typen)
  - [Elicitation (mehrstufige Tool-Eingabe)](#elicitation-mehrstufige-tool-eingabe)
  - [Fehlerantwort](#fehlerantwort)
- [Architektur](#architektur)
- [Entwicklung](#entwicklung)
- [Lizenz](#lizenz)

## Features

- **Multi-Backend**: FastCGI (TCP oder Unix-Socket) und HTTP(S)-Upstreams, automatische Erkennung aus dem URL-Format
- **Transporte**: stdio und HTTP/SSE, beides nativ über `rmcp`
- **MCP-2025-06-18-Features**: Elicitation (mehrstufige Tool-Eingabe), strukturiertes Tool-Output, Resource-Links in Tool-Antworten
- **Connection-Pooling** für FastCGI (via `deadpool`)
- **TLS über rustls** — keine OpenSSL-Abhängigkeit, vollständig statische musl-Builds
- **Capability-Caching** — Tools/Resources/Prompts werden einmal beim Start ermittelt
- **Benutzerdefinierte HTTP-Header**, konfigurierbare Timeouts, Weitergabe von Request-/Session-IDs an den Upstream

## Installation

Vorgefertigte Binaries werden bei jedem getaggten Release für **macOS (arm64, x86_64)** und **Linux (arm64, x86_64, statisch mit musl)** veröffentlicht. Wähle die Methode, die zu deiner Plattform passt.

### Homebrew (macOS und Linux)

```bash
brew tap petstack/tap
brew install roxy
```

Funktioniert auf macOS (Intel und Apple Silicon) und Linux (x86_64 und arm64) mit installiertem Homebrew.

### Installations-Skript (jedes Unix)

```bash
curl -sSfL https://raw.githubusercontent.com/petstack/roxy/main/install.sh | sh
```

Das Skript erkennt automatisch dein Betriebssystem und deine Architektur, lädt das passende Tarball von GitHub Releases herunter, prüft die SHA256-Prüfsumme und installiert nach `/usr/local/bin/roxy` (bei Bedarf mit `sudo`).

Optionen:

```bash
# Bestimmte Version installieren
curl -sSfL .../install.sh | sh -s -- --version v0.1.0

# In ein eigenes Verzeichnis installieren (kein sudo nötig)
curl -sSfL .../install.sh | sh -s -- --bin-dir $HOME/.local/bin
```

Die Umgebungsvariablen `ROXY_REPO`, `ROXY_VERSION`, `ROXY_BIN_DIR` funktionieren ebenfalls.

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

### Alpine / jedes Linux (statisches Tarball)

Die Linux-Binaries sind statisch gegen musl libc gelinkt und laufen deshalb auf **jeder** Linux-Distribution ohne Abhängigkeiten:

```bash
# Architektur wählen
ARCH=x86_64   # oder aarch64
curl -sSfL https://github.com/petstack/roxy/releases/latest/download/roxy-v0.1.0-${ARCH}-unknown-linux-musl.tar.gz | tar -xz
sudo install -m 755 roxy-v0.1.0-${ARCH}-unknown-linux-musl/roxy /usr/local/bin/
```

Funktioniert auf Alpine, Debian, Ubuntu, RHEL, Arch, Amazon Linux, Void, NixOS und allem anderen mit Linux-Kernel.

### Aus dem Quellcode

Benötigt [Rust](https://rustup.rs/) (Edition 2024, stable Toolchain):

```bash
git clone https://github.com/petstack/roxy
cd roxy
cargo build --release
# Binary liegt in ./target/release/roxy
```

Oder via `cargo install`:

```bash
cargo install --git https://github.com/petstack/roxy
```

### Installation prüfen

```bash
roxy --version
roxy --help
```

## Schnellstart

roxy braucht **ein Argument**: `--upstream`, das auf deinen Handler zeigt. Der Upstream-Typ wird **automatisch** aus dem URL-Format abgeleitet:

| URL-Format | Backend-Typ |
|---|---|
| `http://...` oder `https://...` | HTTP(S) |
| `host:port` | FastCGI TCP |
| `/pfad/zum/socket` | FastCGI Unix-Socket |

### Beispiel: HTTP-Backend

```bash
# Starte deinen HTTP-Handler auf Port 8000 (jede Sprache, jedes Framework)
# Dann zeige roxy darauf:
roxy --upstream http://localhost:8000/mcp
```

### Beispiel: PHP-FPM-Backend

```bash
# PHP-FPM starten
php-fpm --nodaemonize \
    -d "listen=127.0.0.1:9000" \
    -d "pm=static" \
    -d "pm.max_children=4"

# roxy draufzeigen
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /absolute/path/to/handler.php
```

### Verbindung von einem MCP-Client

Für Claude Desktop oder jeden Client, der MCP-Server als Subprocess startet (stdio-Transport — der Default):

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

Für Netzwerk-Clients, die über HTTP/SSE verbinden:

```bash
roxy --transport http --port 8080 --upstream http://localhost:8000/mcp
# Client verbindet sich mit http://localhost:8080/sse
```

## CLI-Referenz

```
roxy [OPTIONS] --upstream <UPSTREAM>
```

| Flag | Default | Beschreibung |
|---|---|---|
| `--upstream <URL>` | **erforderlich** | Backend-URL. Typ wird automatisch erkannt (siehe Tabelle oben) |
| `--transport <MODE>` | `stdio` | Transport für den MCP-Client: `stdio` oder `http` |
| `--port <PORT>` | `8080` | HTTP-Listen-Port (nur mit `--transport http`) |
| `--upstream-entrypoint <PATH>` | — | `SCRIPT_FILENAME`, der an FastCGI-Backends gesendet wird (erforderlich für PHP-FPM) |
| `--upstream-insecure` | `false` | TLS-Zertifikatsprüfung für HTTPS-Upstreams überspringen |
| `--upstream-timeout <SECS>` | `30` | HTTP-Upstream-Request-Timeout in Sekunden |
| `--upstream-header <HEADER>` | — | Benutzerdefinierter HTTP-Header, `Name: Value`. Wiederholbar |
| `--pool-size <N>` | `16` | Größe des FastCGI-Connection-Pools |
| `--log-format <FORMAT>` | `pretty` | Log-Format: `pretty` oder `json` |

Das **Log-Level** wird über die Umgebungsvariable `RUST_LOG` gesteuert:

```bash
RUST_LOG=debug roxy --upstream http://localhost:8000/mcp
RUST_LOG=roxy=debug,rmcp=info roxy --upstream ...  # Filter pro Modul
```

### Vollständiges HTTP-Backend-Beispiel

```bash
roxy --upstream https://api.example.com/mcp \
     --upstream-header "Authorization: Bearer $TOKEN" \
     --upstream-header "X-Tenant: acme" \
     --upstream-timeout 60
```

### Vollständiges FastCGI-Beispiel (PHP-FPM)

```bash
# TCP
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /srv/app/handler.php

# Unix-Socket
roxy --upstream /var/run/php-fpm.sock --upstream-entrypoint /srv/app/handler.php

# HTTP-Transport mit FastCGI-Upstream
roxy --transport http --port 8080 \
     --upstream 127.0.0.1:9000 \
     --upstream-entrypoint /srv/app/handler.php
```

## Upstream-Handler schreiben

Dein Handler erhält einfache JSON-Requests und gibt einfache JSON-Antworten zurück. **Er sieht nie JSON-RPC, MCP-Framing oder Session-State.** roxy übersetzt alles.

### Für HTTP-Backends

Jeder HTTP-Server, der JSON aus dem Request-Body liest und JSON in die Antwort schreibt, funktioniert. Beispiel in Python/Flask:

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

### Für FastCGI-Backends (PHP-FPM)

Ein minimaler PHP-Handler:

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

Ein vollständiges Beispiel mit mehreren Tools, strukturiertem Output, Elicitation und Resource-Links — siehe [`examples/handler.php`](../examples/handler.php).

## Upstream-Protokoll-Referenz

Jeder Request von roxy an deinen Upstream ist ein JSON-Objekt mit diesen gemeinsamen Envelope-Feldern:

```json
{
  "type": "...",
  "session_id": "optional-uuid-or-null",
  "request_id": "uuid-per-request",
  ...
}
```

### Request-Typen

#### `discover`

Wird einmal beim Start von roxy gesendet. Dein Handler muss den kompletten Katalog der unterstützten Tools, Resources und Prompts zurückgeben. roxy cached das Ergebnis und liefert es an alle MCP-Clients aus, ohne den Upstream erneut zu befragen.

```json
// Antwort
{
  "tools": [
    {
      "name": "tool_name",
      "title": "Human Name",
      "description": "Was es macht",
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

Die Felder `title`, `description`, `mime_type`, `output_schema` sind optional.

#### `call_tool`

Ein Tool per Name ausführen. Request:

```json
{
  "type": "call_tool",
  "name": "tool_name",
  "arguments": { "key": "value" },
  "elicitation_results": [ ... ],   // optional: siehe Elicitation unten
  "context": { ... }                 // optional: Echo aus einer früheren Elicit-Antwort
}
```

Erfolgsantwort (normale Textausgabe):

```json
{
  "content": [
    { "type": "text", "text": "Ergebnis" }
  ]
}
```

Erfolgsantwort mit **strukturiertem Inhalt** (für Tools mit `output_schema`):

```json
{
  "content": [{ "type": "text", "text": "5 + 3 = 8" }],
  "structured_content": { "sum": 8, "operands": { "a": 5, "b": 3 } }
}
```

Erfolgsantwort mit einem in die Ausgabe eingebetteten **Resource-Link**:

```json
{
  "content": [
    { "type": "text", "text": "Buchung bestätigt." },
    {
      "type": "resource_link",
      "uri": "myapp://bookings/1234",
      "name": "booking-1234",
      "title": "Buchung #1234"
    }
  ]
}
```

#### `read_resource`

Eine Resource per URI lesen. Request:

```json
{ "type": "read_resource", "uri": "myapp://status" }
```

Antwort: dasselbe `content`-Format wie bei `call_tool`.

#### `get_prompt`

Einen Prompt generieren. Request:

```json
{ "type": "get_prompt", "name": "greet", "arguments": { "name": "Alice" } }
```

Antwort: dasselbe `content`-Format wie bei `call_tool`.

#### `elicitation_cancelled`

Wird gesendet, wenn der MCP-Client eine Elicitation abbricht (siehe unten). Dein Handler kann loggen/aufräumen; die Antwort wird ignoriert.

```json
{ "type": "elicitation_cancelled", "name": "tool_name", "action": "decline", "context": {...} }
```

### Elicitation (mehrstufige Tool-Eingabe)

Ein Tool kann **während der Ausführung zusätzliche Eingaben vom Nutzer anfordern**. Beim ersten `call_tool` gibst du statt `content` eine `elicit`-Antwort zurück:

```json
{
  "elicit": {
    "message": "Welche Flugklasse?",
    "schema": {
      "type": "object",
      "properties": {
        "class": { "type": "string", "enum": ["economy", "business", "first"] }
      },
      "required": ["class"]
    },
    "context": { "step": 1, "destination": "Tokio" }
  }
}
```

roxy leitet die Elicitation an den MCP-Client weiter. Sobald der Nutzer die Daten ausgefüllt hat, ruft roxy dein Tool **erneut** auf und übergibt die gesammelten Werte in `elicitation_results` sowie deinen ursprünglichen `context` zurück:

```json
{
  "type": "call_tool",
  "name": "book_flight",
  "arguments": { "destination": "Tokio" },
  "elicitation_results": [{ "class": "business" }],
  "context": { "step": 1, "destination": "Tokio" }
}
```

Du kannst mehrere Elicitation-Runden verketten, indem du weitere `elicit`-Antworten zurückgibst, bis alle Daten gesammelt sind.

### Fehlerantwort

Jeder Request-Typ kann statt eines Erfolges einen Fehler zurückgeben:

```json
{ "error": { "code": 404, "message": "Unknown tool: foo" } }
```

## Architektur

```
MCP-Client (Claude, Cursor, Zed, ...)
       │
       │ JSON-RPC über stdio oder HTTP/SSE
       ▼
┌──────────────┐
│    rmcp      │  MCP-Protokoll, Transport, Sessions
└──────────────┘
       │
       ▼
┌──────────────┐
│  RoxyServer  │  MCP-Methoden-Routing, Capabilities-Cache
└──────────────┘
       │
       │ vereinfachtes JSON (UpstreamEnvelope + UpstreamRequest)
       ▼
┌──────────────────────────┐
│    UpstreamExecutor      │  Trait mit 2 Implementierungen
│  ┌────────┬───────────┐  │
│  │FastCgi │  Http     │  │
│  └────────┴───────────┘  │
└──────────────────────────┘
       │                │
       ▼                ▼
   PHP-FPM /        HTTP(S)-
   beliebiges       Endpoint
   FastCGI
```

### Quellcode-Layout

```
src/
  main.rs             CLI, Logging, Transport-Start, Executor-Auswahl
  config.rs           clap Config, UpstreamKind (Auto-Erkennung), FcgiAddress
  protocol.rs         Interne JSON-Typen (UpstreamEnvelope, UpstreamRequest, ...)
  server.rs           RoxyServer: rmcp-ServerHandler-Implementierung + Discover-Cache
  executor/
    mod.rs            UpstreamExecutor-Trait
    fastcgi.rs        FastCgiExecutor: deadpool + fastcgi-client
    http.rs           HttpExecutor: reqwest + rustls
examples/
  handler.php         Vollständiges PHP-Handler-Beispiel mit allen Features
```

### Wichtige Design-Entscheidungen

- **rmcp übernimmt die Schwerstarbeit.** Der offizielle `rmcp`-Crate kümmert sich um die gesamte MCP-Protokoll-Komplexität (JSON-RPC, Transport-Aushandlung, Session-Management). roxy implementiert nur den `ServerHandler`.
- **Upstream ist pluggable.** Der `UpstreamExecutor`-Trait abstrahiert die Backend-Kommunikation. FastCGI und HTTP sind die aktuellen Implementierungen; ein neues Backend (gRPC, stdio, WebSocket) hinzufügen heißt, einen Trait zu implementieren.
- **Capabilities werden gecacht.** roxy ruft `discover` einmal beim Start auf und hält Tools/Resources/Prompts im Speicher. MCP-Clients erhalten sofortige Antworten auf `initialize`, ohne den Upstream zu berühren.
- **Connection-Pooling für FastCGI.** `deadpool` hält Verbindungen zu PHP-FPM warm und vermeidet so Socket-Setup bei jedem Request.
- **Pure-Rust-TLS via rustls.** Kein OpenSSL, keine System-Libraries. Vollständig statische Linux-Builds, einfache Cross-Compilation, portable Binaries.
- **Der Upstream bleibt dumm.** Dein Handler sieht nie JSON-RPC, Request-IDs (außer als opakes Envelope-Feld), Session-State oder MCP-Framing. Einfaches JSON rein, einfaches JSON raus.

## Entwicklung

### Build & Tests

```bash
cargo build           # Debug
cargo build --release # optimiert
cargo test            # Tests ausführen
cargo clippy          # Linter
cargo fmt             # Formatierung
```

### Lokal mit dem Beispiel-PHP-Handler starten

```bash
# Terminal 1: PHP-FPM starten
php-fpm --nodaemonize -d "listen=127.0.0.1:9000" -d "pm=static" -d "pm.max_children=4"

# Terminal 2: roxy mit dem Beispiel-Handler starten
cargo run -- \
    --upstream 127.0.0.1:9000 \
    --upstream-entrypoint "$(pwd)/examples/handler.php"
```

Dann mit einem beliebigen MCP-Client verbinden oder JSON-RPC manuell über stdio senden.

### Release-Workflow

Getaggte Releases (`git tag vX.Y.Z && git push origin vX.Y.Z`) triggern `.github/workflows/release.yml`, das:

1. Release-Binaries für alle vier Targets baut (macOS arm64/x86_64, Linux musl arm64/x86_64)
2. Sie als `.tar.gz` mit SHA256-Prüfsummen verpackt
3. `.deb`- und `.rpm`-Pakete für beide Linux-Architekturen baut
4. Ein GitHub Release mit allen Artefakten veröffentlicht
5. Die Homebrew-Formel in [`petstack/homebrew-tap`](https://github.com/petstack/homebrew-tap) bumpt (wenn das `HOMEBREW_TAP_TOKEN`-Secret gesetzt ist)

Tap-Setup — siehe [`packaging/homebrew/README.md`](../packaging/homebrew/README.md).

## Lizenz

[AGPL-3.0-only](../LICENSE). Wenn du eine modifizierte Version von roxy als Netzwerkdienst betreibst, musst du deine Änderungen den Nutzern dieses Dienstes zur Verfügung stellen.
