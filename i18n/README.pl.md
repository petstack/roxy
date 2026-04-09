# roxy

---

[English](../README.md) · [Русский](README.ru.md) · [Українська](README.uk.md) · [Беларуская](README.be.md) · **Polski** · [Deutsch](README.de.md) · [Français](README.fr.md) · [Español](README.es.md) · [中文](README.zh-CN.md) · [日本語](README.ja.md)

---

**Wysokowydajny serwer proxy MCP (Model Context Protocol) napisany w Rust.**

roxy łączy klientów MCP (Claude Desktop, Cursor, Zed i inne) z dowolnym handlerem upstream działającym jako backend **FastCGI** (np. PHP-FPM) lub endpoint **HTTP(S)**. Rust zajmuje się wszystkim, co krytyczne dla wydajności — transportem, parsowaniem protokołu, pulą połączeń, współbieżnością — poprzez oficjalny crate [`rmcp`](https://crates.io/crates/rmcp). Twój handler obsługuje tylko mały, uproszczony protokół JSON i zwraca wyniki.

Pozwala to pisać serwery MCP w **dowolnym języku** — PHP, Python, Node, Go, Ruby — bez każdorazowego reimplementowania ramkowania JSON-RPC, transportu, zarządzania sesjami i negocjacji możliwości.

## Spis treści

- [Funkcje](#funkcje)
- [Instalacja](#instalacja)
  - [Homebrew (macOS i Linux)](#homebrew-macos-i-linux)
  - [Skrypt instalacyjny (dowolny Unix)](#skrypt-instalacyjny-dowolny-unix)
  - [Debian / Ubuntu (.deb)](#debian--ubuntu-deb)
  - [Fedora / RHEL / openSUSE (.rpm)](#fedora--rhel--opensuse-rpm)
  - [Alpine / dowolny Linux (statyczny tarball)](#alpine--dowolny-linux-statyczny-tarball)
  - [Ze źródeł](#ze-źródeł)
  - [Weryfikacja instalacji](#weryfikacja-instalacji)
- [Szybki start](#szybki-start)
- [Referencja CLI](#referencja-cli)
  - [Zmienne środowiskowe](#zmienne-środowiskowe)
- [Pisanie handlera upstream](#pisanie-handlera-upstream)
- [Referencja protokołu upstream](#referencja-protokołu-upstream)
  - [Typy żądań](#typy-żądań)
  - [Elicitation (wieloetapowe wprowadzanie danych do narzędzi)](#elicitation-wieloetapowe-wprowadzanie-danych-do-narzędzi)
  - [Odpowiedź z błędem](#odpowiedź-z-błędem)
- [Architektura](#architektura)
- [Rozwój](#rozwój)
- [Licencja](#licencja)

## Funkcje

- **Wiele backendów**: upstreamy FastCGI (TCP lub Unix socket) oraz HTTP(S), automatycznie wykrywane na podstawie formatu URL
- **Transporty**: stdio i HTTP/SSE, oba obsługiwane natywnie przez `rmcp`
- **Funkcje MCP 2025-06-18**: elicitation (wieloetapowe wprowadzanie danych), strukturalne wyjście narzędzi, linki do zasobów w odpowiedziach
- **Pula połączeń** dla FastCGI (przez `deadpool`)
- **TLS przez rustls** — bez zależności od OpenSSL, w pełni statyczne buildy musl
- **Cache’owanie możliwości** — narzędzia/zasoby/prompty pobierane raz przy starcie
- **Niestandardowe nagłówki HTTP**, konfigurowalne timeouty, przekazywanie request/session ID do upstream

## Instalacja

Prekompilowane binaria są publikowane przy każdym otagowanym wydaniu dla **macOS (arm64, x86_64)** oraz **Linux (arm64, x86_64, statyczny musl)**. Wybierz metodę pasującą do Twojej platformy.

### Homebrew (macOS i Linux)

```bash
brew tap petstack/tap
brew install roxy
```

Działa na macOS (Intel i Apple Silicon) oraz Linux (x86_64 i arm64) z zainstalowanym Homebrew.

### Skrypt instalacyjny (dowolny Unix)

```bash
curl -sSfL https://raw.githubusercontent.com/petstack/roxy/main/install.sh | sh
```

Skrypt automatycznie wykrywa Twój system operacyjny i architekturę, pobiera właściwy tarball z GitHub Releases, weryfikuje sumę kontrolną SHA256 i instaluje binarkę w `/usr/local/bin/roxy` (w razie potrzeby używając `sudo`).

Opcje:

```bash
# Instalacja konkretnej wersji
curl -sSfL .../install.sh | sh -s -- --version v0.1.0

# Instalacja do własnego katalogu (bez sudo)
curl -sSfL .../install.sh | sh -s -- --bin-dir $HOME/.local/bin
```

Działają również zmienne środowiskowe `ROXY_REPO`, `ROXY_VERSION`, `ROXY_BIN_DIR`.

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

### Alpine / dowolny Linux (statyczny tarball)

Linuksowe binaria są statycznie linkowane z musl libc, więc działają na **dowolnej** dystrybucji Linuksa bez zależności:

```bash
# Wybierz swoją architekturę
ARCH=x86_64   # lub aarch64
curl -sSfL https://github.com/petstack/roxy/releases/latest/download/roxy-v0.1.0-${ARCH}-unknown-linux-musl.tar.gz | tar -xz
sudo install -m 755 roxy-v0.1.0-${ARCH}-unknown-linux-musl/roxy /usr/local/bin/
```

Działa na Alpine, Debian, Ubuntu, RHEL, Arch, Amazon Linux, Void, NixOS i wszystkim innym z jądrem Linux.

### Ze źródeł

Wymagany [Rust](https://rustup.rs/) (edition 2024, stabilny toolchain):

```bash
git clone https://github.com/petstack/roxy
cd roxy
cargo build --release
# Binarka jest w ./target/release/roxy
```

Lub przez `cargo install`:

```bash
cargo install --git https://github.com/petstack/roxy
```

### Weryfikacja instalacji

```bash
roxy --version
roxy --help
```

## Szybki start

roxy wymaga **jednego argumentu**: `--upstream`, wskazującego na Twój handler. Typ upstream jest **wykrywany automatycznie** z formatu URL:

| Format URL | Typ backendu |
|---|---|
| `http://...` lub `https://...` | HTTP(S) |
| `host:port` | FastCGI TCP |
| `/ścieżka/do/socketu` | FastCGI Unix socket |

### Przykład: backend HTTP

```bash
# Uruchom swój handler HTTP na porcie 8000 (dowolny język, dowolny framework)
# Następnie skieruj do niego roxy:
roxy --upstream http://localhost:8000/mcp
```

### Przykład: backend PHP-FPM

```bash
# Uruchom PHP-FPM
php-fpm --nodaemonize \
    -d "listen=127.0.0.1:9000" \
    -d "pm=static" \
    -d "pm.max_children=4"

# Skieruj roxy
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /absolute/path/to/handler.php
```

### Podłączenie z klienta MCP

Dla Claude Desktop lub dowolnego klienta uruchamiającego serwery MCP jako podprocesy (transport stdio — domyślny):

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

Dla klientów sieciowych łączących się przez HTTP/SSE:

```bash
roxy --transport http --port 8080 --upstream http://localhost:8000/mcp
# Klient łączy się z http://localhost:8080/sse
```

## Referencja CLI

```
roxy [OPTIONS] --upstream <UPSTREAM>
```

| Flaga | Domyślnie | Opis |
|---|---|---|
| `--upstream <URL>` | **wymagana** | URL backendu. Typ wykrywany automatycznie (patrz tabela wyżej) |
| `--transport <MODE>` | `stdio` | Transport klienta MCP: `stdio` lub `http` |
| `--port <PORT>` | `8080` | Port nasłuchu HTTP (tylko z `--transport http`) |
| `--upstream-entrypoint <PATH>` | — | `SCRIPT_FILENAME` wysyłany do backendów FastCGI (wymagane dla PHP-FPM) |
| `--upstream-insecure` | `false` | Pomiń weryfikację certyfikatu TLS dla upstreamów HTTPS |
| `--upstream-timeout <SECS>` | `30` | Timeout żądania HTTP upstream w sekundach |
| `--upstream-header <HEADER>` | — | Niestandardowy nagłówek HTTP, `Name: Value`. Można powtarzać |
| `--pool-size <N>` | `16` | Rozmiar puli połączeń FastCGI |
| `--log-format <FORMAT>` | `pretty` | Format logów: `pretty` lub `json` |

**Poziom** logowania kontrolowany jest przez zmienną środowiskową `RUST_LOG`:

```bash
RUST_LOG=debug roxy --upstream http://localhost:8000/mcp
RUST_LOG=roxy=debug,rmcp=info roxy --upstream ...  # filtry per-moduł
```

### Pełny przykład backendu HTTP

```bash
roxy --upstream https://api.example.com/mcp \
     --upstream-header "Authorization: Bearer $TOKEN" \
     --upstream-header "X-Tenant: acme" \
     --upstream-timeout 60
```

### Pełny przykład FastCGI (PHP-FPM)

```bash
# TCP
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /srv/app/handler.php

# Unix socket
roxy --upstream /var/run/php-fpm.sock --upstream-entrypoint /srv/app/handler.php

# Transport HTTP z upstreamem FastCGI
roxy --transport http --port 8080 \
     --upstream 127.0.0.1:9000 \
     --upstream-entrypoint /srv/app/handler.php
```

### Zmienne środowiskowe

Wszystkie flagi CLI przyjmują odpowiadającą zmienną środowiskową `ROXY_*` jako opcjonalną wartość zapasową. Kolejność rozwiązywania: **CLI > env > default**: flaga podana w wierszu poleceń zawsze wygrywa, zmienna środowiskowa jest sprawdzana tylko przy braku flagi, a wbudowana wartość domyślna jest używana tylko gdy żadna z nich nie jest ustawiona.

| Flaga | Env variable | Przykład |
|---|---|---|
| `--transport` | `ROXY_TRANSPORT` | `stdio` \| `http` |
| `--port` | `ROXY_PORT` | `8080` |
| `--upstream` | `ROXY_UPSTREAM` | `http://localhost:8000/handler` |
| `--upstream-entrypoint` | `ROXY_UPSTREAM_ENTRYPOINT` | `/srv/handler.php` |
| `--upstream-insecure` | `ROXY_UPSTREAM_INSECURE` | `true` \| `false` |
| `--upstream-timeout` | `ROXY_UPSTREAM_TIMEOUT` | `30` |
| `--upstream-header` | `ROXY_UPSTREAM_HEADER` | rozdzielone znakami nowej linii, patrz niżej |
| `--pool-size` | `ROXY_POOL_SIZE` | `16` |
| `--log-format` | `ROXY_LOG_FORMAT` | `pretty` \| `json` |

#### Wiele wartości upstream-header

`ROXY_UPSTREAM_HEADER` przyjmuje wiele linii nagłówków rozdzielonych dosłownymi znakami nowej linii. Naturalnie przekłada się to na blokowy skalar Kubernetes YAML:

```yaml
env:
  - name: ROXY_UPSTREAM_HEADER
    value: |-
      Authorization: Bearer xyz
      X-Trace-Id: abc
```

Z lokalnej powłoki użyj cytowania `$'...'`, aby `\n` stało się prawdziwym znakiem nowej linii:

```bash
ROXY_UPSTREAM_HEADER=$'Authorization: Bearer xyz\nX-Trace-Id: abc' \
  roxy --upstream https://api.example.com/mcp
```

Początkowe i końcowe puste linie są odrzucane przy starcie, więc specyfika blokowego skalara YAML `|-` nie powoduje nieprawidłowych nagłówków. Jeśli `--upstream-header` jest podany w CLI, `ROXY_UPSTREAM_HEADER` jest całkowicie ignorowany — nie ma łączenia dwóch źródeł.

#### Wartości logiczne

`ROXY_UPSTREAM_INSECURE` akceptuje tylko **dokładne ciągi małymi literami** `true` lub `false`. Formy numeryczne (`1`, `0`) i inne warianty pisowni (`TRUE`, `True`, `YES`, `on`) są odrzucane przez parser clap (`SetTrue + env`) i powodują błąd przy starcie. Flaga CLI `--upstream-insecure` (bez wartości) nadal działa jak poprzednio i oznacza `true`.

#### `RUST_LOG`

roxy respektuje standardową zmienną środowiskową `RUST_LOG`, odczytywaną przy starcie przez `tracing_subscriber::EnvFilter`; jest to ortogonalne względem zmiennych `ROXY_*` powyżej i pozostaje bez zmian.

## Pisanie handlera upstream

Twój handler otrzymuje proste żądania JSON i zwraca proste odpowiedzi JSON. **Nigdy nie widzi JSON-RPC, ramkowania MCP ani stanu sesji.** roxy tłumaczy wszystko.

### Dla backendów HTTP

Zadziała dowolny serwer HTTP, który czyta JSON z ciała żądania i zapisuje JSON do odpowiedzi. Przykład w Python/Flask:

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

### Dla backendów FastCGI (PHP-FPM)

Minimalny handler PHP:

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

Pełny przykład z wieloma narzędziami, strukturalnym wyjściem, elicitation i linkami do zasobów — patrz [`examples/handler.php`](../examples/handler.php).

## Referencja protokołu upstream

Każde żądanie od roxy do Twojego upstreamu to obiekt JSON z tymi wspólnymi polami koperty:

```json
{
  "type": "...",
  "session_id": "optional-uuid-or-null",
  "request_id": "uuid-per-request",
  ...
}
```

### Typy żądań

#### `discover`

Wysyłane raz przy starcie roxy. Twój handler musi zwrócić pełny katalog obsługiwanych narzędzi, zasobów i promptów. roxy cache’uje wynik i serwuje go wszystkim klientom MCP bez ponownego odpytywania.

```json
// Odpowiedź
{
  "tools": [
    {
      "name": "tool_name",
      "title": "Human Name",
      "description": "Co robi",
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

Pola `title`, `description`, `mime_type`, `output_schema` są opcjonalne.

#### `call_tool`

Wywołaj narzędzie po nazwie. Żądanie:

```json
{
  "type": "call_tool",
  "name": "tool_name",
  "arguments": { "key": "value" },
  "elicitation_results": [ ... ],   // opcjonalne: patrz sekcja Elicitation poniżej
  "context": { ... }                 // opcjonalne: echo z poprzedniej odpowiedzi elicit
}
```

Odpowiedź sukcesu (zwykłe wyjście tekstowe):

```json
{
  "content": [
    { "type": "text", "text": "wynik" }
  ]
}
```

Odpowiedź sukcesu ze **strukturalną zawartością** (dla narzędzi z `output_schema`):

```json
{
  "content": [{ "type": "text", "text": "5 + 3 = 8" }],
  "structured_content": { "sum": 8, "operands": { "a": 5, "b": 3 } }
}
```

Odpowiedź sukcesu z **linkiem do zasobu** osadzonym w wyjściu:

```json
{
  "content": [
    { "type": "text", "text": "Rezerwacja potwierdzona." },
    {
      "type": "resource_link",
      "uri": "myapp://bookings/1234",
      "name": "booking-1234",
      "title": "Rezerwacja #1234"
    }
  ]
}
```

#### `read_resource`

Odczytaj zasób po URI. Żądanie:

```json
{ "type": "read_resource", "uri": "myapp://status" }
```

Odpowiedź: ten sam format `content` co w `call_tool`.

#### `get_prompt`

Wygeneruj prompt. Żądanie:

```json
{ "type": "get_prompt", "name": "greet", "arguments": { "name": "Alice" } }
```

Odpowiedź: ten sam format `content` co w `call_tool`.

#### `elicitation_cancelled`

Wysyłane, gdy klient MCP anuluje elicitation (patrz poniżej). Twój handler może zalogować/posprzątać; odpowiedź jest ignorowana.

```json
{ "type": "elicitation_cancelled", "name": "tool_name", "action": "decline", "context": {...} }
```

### Elicitation (wieloetapowe wprowadzanie danych do narzędzi)

Narzędzie może **zapytać użytkownika o dodatkowe dane** w trakcie wykonywania. W pierwszym `call_tool` zwróć odpowiedź `elicit` zamiast `content`:

```json
{
  "elicit": {
    "message": "Jaka klasa lotu?",
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

roxy przekaże elicitation do klienta MCP. Gdy użytkownik wypełni dane, roxy wywoła Twoje narzędzie **ponownie**, przekazując zebrane wartości w `elicitation_results` i Twój oryginalny `context`:

```json
{
  "type": "call_tool",
  "name": "book_flight",
  "arguments": { "destination": "Tokio" },
  "elicitation_results": [{ "class": "business" }],
  "context": { "step": 1, "destination": "Tokio" }
}
```

Możesz łańcuchowo zbierać wiele rund elicitation, zwracając kolejny `elicit`, aż wszystkie dane zostaną zebrane.

### Odpowiedź z błędem

Dowolny typ żądania może zwrócić błąd zamiast sukcesu:

```json
{ "error": { "code": 404, "message": "Unknown tool: foo" } }
```

## Architektura

```
Klient MCP (Claude, Cursor, Zed, ...)
       │
       │ JSON-RPC przez stdio lub HTTP/SSE
       ▼
┌──────────────┐
│    rmcp      │  Protokół MCP, transport, sesje
└──────────────┘
       │
       ▼
┌──────────────┐
│  RoxyServer  │  trasowanie metod MCP, cache możliwości
└──────────────┘
       │
       │ uproszczony JSON (UpstreamEnvelope + UpstreamRequest)
       ▼
┌──────────────────────────┐
│    UpstreamExecutor      │  trait z 2 implementacjami
│  ┌────────┬───────────┐  │
│  │FastCgi │  Http     │  │
│  └────────┴───────────┘  │
└──────────────────────────┘
       │                │
       ▼                ▼
   PHP-FPM /        endpoint
   dowolny FastCGI  HTTP(S)
```

### Układ źródeł

```
src/
  main.rs             CLI, logowanie, start transportu, wybór executora
  config.rs           clap Config, UpstreamKind (auto-wykrywanie), FcgiAddress
  protocol.rs         Wewnętrzne typy JSON (UpstreamEnvelope, UpstreamRequest, ...)
  server.rs           RoxyServer: implementacja rmcp ServerHandler + cache discover
  executor/
    mod.rs            Trait UpstreamExecutor
    fastcgi.rs        FastCgiExecutor: deadpool + fastcgi-client
    http.rs           HttpExecutor: reqwest + rustls
examples/
  handler.php         Pełny przykład handlera PHP ze wszystkimi funkcjami
```

### Kluczowe decyzje projektowe

- **rmcp wykonuje ciężką pracę.** Oficjalny crate `rmcp` zajmuje się całą złożonością protokołu MCP (JSON-RPC, negocjacja transportu, zarządzanie sesjami). roxy implementuje tylko `ServerHandler`.
- **Upstream jest wymienny.** Trait `UpstreamExecutor` abstrahuje komunikację z backendem. FastCGI i HTTP to obecne implementacje; dodanie nowego backendu (gRPC, stdio, WebSocket) sprowadza się do implementacji jednego traita.
- **Możliwości są cache’owane.** roxy wywołuje `discover` raz przy starcie i przechowuje narzędzia/zasoby/prompty w pamięci. Klienci MCP otrzymują natychmiastowe odpowiedzi na `initialize` bez dotykania upstreamu.
- **Pula połączeń dla FastCGI.** `deadpool` utrzymuje ciepłe połączenia z PHP-FPM, unikając konfiguracji socketu przy każdym żądaniu.
- **TLS w czystym Rust przez rustls.** Bez OpenSSL, bez bibliotek systemowych. W pełni statyczne buildy Linux, prosta kompilacja krzyżowa, przenośne binarki.
- **Upstream pozostaje prosty.** Twój handler nigdy nie widzi JSON-RPC, ID żądań (poza jako nieprzezroczyste pole koperty), stanu sesji ani ramkowania MCP. Prosty JSON na wejściu, prosty JSON na wyjściu.

## Rozwój

### Build i testy

```bash
cargo build           # debug
cargo build --release # zoptymalizowany
cargo test            # uruchom testy
cargo clippy          # linter
cargo fmt             # formatowanie
```

### Lokalne uruchomienie z przykładowym handlerem PHP

```bash
# Terminal 1: uruchom PHP-FPM
php-fpm --nodaemonize -d "listen=127.0.0.1:9000" -d "pm=static" -d "pm.max_children=4"

# Terminal 2: uruchom roxy z przykładowym handlerem
cargo run -- \
    --upstream 127.0.0.1:9000 \
    --upstream-entrypoint "$(pwd)/examples/handler.php"
```

Następnie połącz się dowolnym klientem MCP lub wysyłaj JSON-RPC ręcznie przez stdio.

### Workflow wydań

Otagowane wydania (`git tag vX.Y.Z && git push origin vX.Y.Z`) uruchamiają `.github/workflows/release.yml`, który:

1. Buduje binaria release dla wszystkich czterech targetów (macOS arm64/x86_64, Linux musl arm64/x86_64)
2. Pakuje je jako `.tar.gz` z sumami kontrolnymi SHA256
3. Buduje pakiety `.deb` i `.rpm` dla obu architektur Linuksa
4. Publikuje GitHub Release ze wszystkimi artefaktami
5. Aktualizuje formułę Homebrew w [`petstack/homebrew-tap`](https://github.com/petstack/homebrew-tap) (jeśli ustawiony jest sekret `HOMEBREW_TAP_TOKEN`)

Konfiguracja tapa — patrz [`packaging/homebrew/README.md`](../packaging/homebrew/README.md).

## Licencja

[AGPL-3.0-only](../LICENSE). Jeśli uruchamiasz zmodyfikowaną wersję roxy jako usługę sieciową, musisz udostępnić swoje modyfikacje użytkownikom tej usługi.
