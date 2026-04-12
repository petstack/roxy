# roxy

---

[English](../README.md) · [Русский](README.ru.md) · [Українська](README.uk.md) · [Беларуская](README.be.md) · [Polski](README.pl.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · **Español** · [中文](README.zh-CN.md) · [日本語](README.ja.md)

---

**Servidor proxy MCP (Model Context Protocol) de alto rendimiento escrito en Rust.**

roxy conecta clientes MCP (Claude Desktop, Cursor, Zed, etc.) con cualquier handler upstream que corra como backend **FastCGI** (por ejemplo, PHP-FPM) o como endpoint **HTTP(S)**. Rust se encarga de todo lo crítico en rendimiento — transporte, parsing del protocolo, pooling de conexiones, concurrencia — a través del crate oficial [`rmcp`](https://crates.io/crates/rmcp). Tu handler solo maneja un protocolo JSON pequeño y simplificado, y devuelve resultados.

Esto te permite escribir servidores MCP en **cualquier lenguaje** — PHP, Python, Node, Go, Ruby — sin reimplementar cada vez el framing JSON-RPC, el transporte, la gestión de sesiones y la negociación de capacidades.

## Tabla de contenidos

- [Características](#características)
- [Instalación](#instalación)
  - [Homebrew (macOS y Linux)](#homebrew-macos-y-linux)
  - [Script de instalación (cualquier Unix)](#script-de-instalación-cualquier-unix)
  - [Debian / Ubuntu (.deb)](#debian--ubuntu-deb)
  - [Fedora / RHEL / openSUSE (.rpm)](#fedora--rhel--opensuse-rpm)
  - [Alpine / cualquier Linux (tarball estático)](#alpine--cualquier-linux-tarball-estático)
  - [Desde el código fuente](#desde-el-código-fuente)
  - [Verificar la instalación](#verificar-la-instalación)
- [Inicio rápido](#inicio-rápido)
- [Referencia del CLI](#referencia-del-cli)
  - [Variables de entorno](#variables-de-entorno)
- [Escribir un handler upstream](#escribir-un-handler-upstream)
- [Referencia del protocolo upstream](#referencia-del-protocolo-upstream)
  - [Tipos de peticiones](#tipos-de-peticiones)
  - [Elicitation (entrada multi-turno para herramientas)](#elicitation-entrada-multi-turno-para-herramientas)
  - [Respuesta de error](#respuesta-de-error)
- [Arquitectura](#arquitectura)
- [Desarrollo](#desarrollo)
- [Licencia](#licencia)

## Características

- **Multi-backend**: upstreams FastCGI (TCP o socket Unix) y HTTP(S), auto-detectados a partir del formato de la URL
- **Transportes**: stdio y Streamable HTTP, ambos soportados de forma nativa vía `rmcp`
- **Funcionalidades MCP 2025-06-18**: elicitation (entrada multi-turno), salida estructurada de herramientas, enlaces a recursos en respuestas
- **Pooling de conexiones** para FastCGI (vía `deadpool`)
- **TLS vía rustls** — sin dependencia de OpenSSL, builds musl totalmente estáticos
- **Caché de capacidades** — tools/resources/prompts descubiertos una sola vez al arranque
- **Cabeceras HTTP personalizadas**, timeouts configurables, propagación de IDs de petición/sesión al upstream

## Instalación

Se publican binarios precompilados en cada release taggeado para **macOS (arm64, x86_64)** y **Linux (arm64, x86_64, musl estático)**. Elige el método que se adapte a tu plataforma.

### Homebrew (macOS y Linux)

```bash
brew tap petstack/tap
brew install roxy
```

Funciona en macOS (Intel y Apple Silicon) y Linux (x86_64 y arm64) con Homebrew instalado.

### Script de instalación (cualquier Unix)

```bash
curl -sSfL https://raw.githubusercontent.com/petstack/roxy/main/install.sh | sh
```

El script detecta automáticamente tu sistema operativo y arquitectura, descarga el tarball correcto desde GitHub Releases, verifica el checksum SHA256 e instala en `/usr/local/bin/roxy` (usando `sudo` si hace falta).

Opciones:

```bash
# Instalar una versión específica
curl -sSfL .../install.sh | sh -s -- --version v0.1.0

# Instalar en un directorio personalizado (sin sudo)
curl -sSfL .../install.sh | sh -s -- --bin-dir $HOME/.local/bin
```

También funcionan las variables de entorno `ROXY_REPO`, `ROXY_VERSION`, `ROXY_BIN_DIR`.

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

### Alpine / cualquier Linux (tarball estático)

Los binarios de Linux están enlazados estáticamente contra musl libc, así que funcionan en **cualquier** distribución Linux sin dependencias:

```bash
# Elige tu arquitectura
ARCH=x86_64   # o aarch64
curl -sSfL https://github.com/petstack/roxy/releases/latest/download/roxy-v0.1.0-${ARCH}-unknown-linux-musl.tar.gz | tar -xz
sudo install -m 755 roxy-v0.1.0-${ARCH}-unknown-linux-musl/roxy /usr/local/bin/
```

Funciona en Alpine, Debian, Ubuntu, RHEL, Arch, Amazon Linux, Void, NixOS y cualquier otro sistema con kernel Linux.

### Desde el código fuente

Requiere [Rust](https://rustup.rs/) (edition 2024, toolchain estable):

```bash
git clone https://github.com/petstack/roxy
cd roxy
cargo build --release
# El binario está en ./target/release/roxy
```

O vía `cargo install`:

```bash
cargo install --git https://github.com/petstack/roxy
```

### Verificar la instalación

```bash
roxy --version
roxy --help
```

## Inicio rápido

roxy necesita **un solo argumento**: `--upstream`, apuntando a tu handler. El tipo de upstream se **detecta automáticamente** a partir del formato de la URL:

| Formato URL | Tipo de backend |
|---|---|
| `http://...` o `https://...` | HTTP(S) |
| `host:port` | FastCGI TCP |
| `/ruta/al/socket` | FastCGI socket Unix |

### Ejemplo: backend HTTP

```bash
# Arranca tu handler HTTP en el puerto 8000 (cualquier lenguaje, cualquier framework)
# Luego apunta roxy a él:
roxy --upstream http://localhost:8000/mcp
```

### Ejemplo: backend PHP-FPM

```bash
# Arrancar PHP-FPM
php-fpm --nodaemonize \
    -d "listen=127.0.0.1:9000" \
    -d "pm=static" \
    -d "pm.max_children=4"

# Apuntar roxy
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /absolute/path/to/handler.php
```

### Conectar desde un cliente MCP

Para Claude Desktop o cualquier cliente que arranque servidores MCP como subprocesos (transporte stdio — el predeterminado):

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

Para clientes de red que se conectan por Streamable HTTP:

```bash
roxy --transport http --port 8080 --upstream http://localhost:8000/mcp
# El cliente se conecta a http://localhost:8080/mcp
```

## Referencia del CLI

```
roxy [OPTIONS] --upstream <UPSTREAM>
```

| Flag | Por defecto | Descripción |
|---|---|---|
| `--upstream <URL>` | **requerido** | URL del backend. El tipo se detecta automáticamente (ver tabla arriba) |
| `--transport <MODE>` | `stdio` | Transporte del cliente MCP: `stdio` o `http` |
| `--port <PORT>` | `8080` | Puerto de escucha HTTP (solo con `--transport http`) |
| `--upstream-entrypoint <PATH>` | — | `SCRIPT_FILENAME` enviado a los backends FastCGI (requerido para PHP-FPM) |
| `--upstream-insecure` | `false` | Omitir la verificación de certificado TLS para upstreams HTTPS |
| `--upstream-timeout <SECS>` | `30` | Timeout de la petición HTTP al upstream en segundos |
| `--upstream-header <HEADER>` | — | Cabecera HTTP estática adjuntada a cada petición al upstream HTTP, `Name: Value`. Se puede repetir. Solo para upstreams HTTP — ignorada en FastCGI |
| `--pool-size <N>` | `16` | Tamaño del pool de conexiones FastCGI |
| `--log-format <FORMAT>` | `pretty` | Formato de logs: `pretty` o `json` |

El **nivel** de log se controla con la variable de entorno `RUST_LOG`:

```bash
RUST_LOG=debug roxy --upstream http://localhost:8000/mcp
RUST_LOG=roxy=debug,rmcp=info roxy --upstream ...  # filtros por módulo
```

### Ejemplo completo de backend HTTP

```bash
roxy --upstream https://api.example.com/mcp \
     --upstream-header "Authorization: Bearer $TOKEN" \
     --upstream-header "X-Tenant: acme" \
     --upstream-timeout 60
```

### Ejemplo completo de FastCGI (PHP-FPM)

```bash
# TCP
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /srv/app/handler.php

# Socket Unix
roxy --upstream /var/run/php-fpm.sock --upstream-entrypoint /srv/app/handler.php

# Transporte HTTP con upstream FastCGI
roxy --transport http --port 8080 \
     --upstream 127.0.0.1:9000 \
     --upstream-entrypoint /srv/app/handler.php
```

### Reenvío de cabeceras del cliente

Con `--transport http`, cada cabecera del cliente MCP entrante se reenvía automáticamente al backend upstream — sin necesidad de configuración. Las cabeceras hop-by-hop (RFC 7230 §6.1: `Connection`, `Keep-Alive`, `Proxy-Authenticate`, `Proxy-Authorization`, `TE`, `Trailer`, `Transfer-Encoding`, `Upgrade`) y las cabeceras que gestiona roxy internamente (`Host`, `Content-Type`, `Content-Length`) se filtran. Todo lo demás — `Authorization`, `Cookie`, `X-Forwarded-For`, cabeceras `X-*` personalizadas, `mcp-session-id` — llega al upstream sin modificaciones. Esto refleja el comportamiento predeterminado de nginx `fastcgi_pass` / `proxy_pass` y existe para que tu backend upstream pueda autenticar al cliente final (validar tokens bearer, inspeccionar cookies de sesión) sin que roxy necesite entender el esquema de autenticación.

| Upstream | Forma del transporte |
|---|---|
| HTTP | Se reenvían como cabeceras HTTP reales en la petición. Las cabeceras con múltiples valores (p. ej., dos entradas `X-Forwarded-For`) se preservan. |
| FastCGI | Se traducen a parámetros CGI `HTTP_*` según RFC 3875 §4.1.18 — los handlers PHP los leen desde `$_SERVER['HTTP_AUTHORIZATION']`, `$_SERVER['HTTP_X_FORWARDED_FOR']`, etc. Las cabeceras con múltiples valores se unen con `", "` para coincidir con la semántica de `$http_*` de nginx. |

`--upstream-header` sigue funcionando como antes para upstreams HTTP — suministra la **propia** identidad estática de roxy hacia el upstream (token de servicio, `X-Client-Id` fijo, etc.). Cuando una cabecera reenviada del cliente colisiona con un `--upstream-header` estático para el mismo nombre, el valor reenviado **gana**: la identidad por petición del llamante es más específica que el valor predeterminado de roxy, tal como suele comportarse un proxy inverso. `--upstream-header` es actualmente un no-op para upstreams FastCGI — usa el reenvío automático en su lugar.

Con `--transport stdio` no hay petición HTTP entrante, por lo que no se reenvían cabeceras; las entradas estáticas de `--upstream-header` siguen aplicándose a los upstreams HTTP con normalidad.

### Variables de entorno

Todos los flags del CLI aceptan una variable de entorno `ROXY_*` correspondiente como valor de respaldo opcional. El orden de resolución es **CLI > env > default**: un flag pasado en la línea de comandos siempre gana, la variable de entorno se consulta solo si el flag está ausente, y el valor predeterminado integrado se usa únicamente cuando ninguno de los dos está presente.

| Flag | Env variable | Ejemplo |
|---|---|---|
| `--transport` | `ROXY_TRANSPORT` | `stdio` \| `http` |
| `--port` | `ROXY_PORT` | `8080` |
| `--upstream` | `ROXY_UPSTREAM` | `http://localhost:8000/handler` |
| `--upstream-entrypoint` | `ROXY_UPSTREAM_ENTRYPOINT` | `/srv/handler.php` |
| `--upstream-insecure` | `ROXY_UPSTREAM_INSECURE` | `true` \| `false` |
| `--upstream-timeout` | `ROXY_UPSTREAM_TIMEOUT` | `30` |
| `--upstream-header` | `ROXY_UPSTREAM_HEADER` | separados por saltos de línea, ver abajo |
| `--pool-size` | `ROXY_POOL_SIZE` | `16` |
| `--log-format` | `ROXY_LOG_FORMAT` | `pretty` \| `json` |

#### Múltiples valores de upstream-header

`ROXY_UPSTREAM_HEADER` acepta múltiples líneas de cabecera separadas por saltos de línea literales. Esto encaja de forma natural en un escalar de bloque YAML de Kubernetes:

```yaml
env:
  - name: ROXY_UPSTREAM_HEADER
    value: |-
      Authorization: Bearer xyz
      X-Trace-Id: abc
```

Desde una shell local, usa el quoting `$'...'` para que `\n` se convierta en un salto de línea real:

```bash
ROXY_UPSTREAM_HEADER=$'Authorization: Bearer xyz\nX-Trace-Id: abc' \
  roxy --upstream https://api.example.com/mcp
```

Las líneas en blanco al inicio y al final se descartan al arrancar, por lo que las particularidades del escalar de bloque YAML `|-` no producen cabeceras malformadas. Si `--upstream-header` se pasa en el CLI, `ROXY_UPSTREAM_HEADER` se ignora por completo — no hay fusión de ambas fuentes.

#### Valores booleanos

`ROXY_UPSTREAM_INSECURE` solo acepta las **cadenas exactas en minúsculas** `true` o `false`. Las formas numéricas (`1`, `0`) y otras grafías (`TRUE`, `True`, `YES`, `on`) son rechazadas por el parser de clap (`SetTrue + env`) y provocan un error al arrancar. El flag CLI `--upstream-insecure` (sin valor) continúa funcionando como antes y simplemente significa `true`.

#### `RUST_LOG`

roxy respeta la variable de entorno estándar `RUST_LOG`, leída al arrancar por `tracing_subscriber::EnvFilter`; esto es ortogonal a las variables `ROXY_*` anteriores y no ha cambiado.

## Escribir un handler upstream

Tu handler recibe peticiones JSON simples y devuelve respuestas JSON simples. **Nunca ve JSON-RPC, framing MCP ni estado de sesión.** roxy traduce todo.

### Para backends HTTP

Cualquier servidor HTTP que lea JSON del cuerpo de la petición y escriba JSON en la respuesta funcionará. Ejemplo en Python/Flask:

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

### Para backends FastCGI (PHP-FPM)

Un handler PHP mínimo:

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

Para un ejemplo completo con varias herramientas, salida estructurada, elicitation y enlaces a recursos — ver [`examples/handler.php`](../examples/handler.php).

## Referencia del protocolo upstream

Cada petición de roxy hacia tu upstream es un objeto JSON con estos campos comunes de envelope:

```json
{
  "type": "...",
  "session_id": "optional-uuid-or-null",
  "request_id": "uuid-per-request",
  ...
}
```

### Tipos de peticiones

#### `discover`

Se envía una vez al arrancar roxy. Tu handler debe devolver el catálogo completo de herramientas, recursos y prompts que soporta. roxy cachea el resultado y lo sirve a todos los clientes MCP sin volver a preguntar.

```json
// Respuesta
{
  "tools": [
    {
      "name": "tool_name",
      "title": "Human Name",
      "description": "Qué hace",
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

Los campos `title`, `description`, `mime_type`, `output_schema` son opcionales.

#### `call_tool`

Ejecuta una herramienta por nombre. Petición:

```json
{
  "type": "call_tool",
  "name": "tool_name",
  "arguments": { "key": "value" },
  "elicitation_results": [ ... ],   // opcional: ver sección Elicitation abajo
  "context": { ... }                 // opcional: eco de una respuesta elicit anterior
}
```

Respuesta de éxito (salida de texto normal):

```json
{
  "content": [
    { "type": "text", "text": "resultado" }
  ]
}
```

Respuesta de éxito con **contenido estructurado** (para herramientas con `output_schema`):

```json
{
  "content": [{ "type": "text", "text": "5 + 3 = 8" }],
  "structured_content": { "sum": 8, "operands": { "a": 5, "b": 3 } }
}
```

Respuesta de éxito con un **enlace a recurso** incrustado en la salida:

```json
{
  "content": [
    { "type": "text", "text": "Reserva confirmada." },
    {
      "type": "resource_link",
      "uri": "myapp://bookings/1234",
      "name": "booking-1234",
      "title": "Reserva #1234"
    }
  ]
}
```

#### `read_resource`

Lee un recurso por URI. Petición:

```json
{ "type": "read_resource", "uri": "myapp://status" }
```

Respuesta: mismo formato `content` que `call_tool`.

#### `get_prompt`

Genera un prompt. Petición:

```json
{ "type": "get_prompt", "name": "greet", "arguments": { "name": "Alice" } }
```

Respuesta: mismo formato `content` que `call_tool`.

#### `elicitation_cancelled`

Se envía cuando el cliente MCP cancela una elicitation (ver abajo). Tu handler puede loguear/limpiar; la respuesta se ignora.

```json
{ "type": "elicitation_cancelled", "name": "tool_name", "action": "decline", "context": {...} }
```

### Elicitation (entrada multi-turno para herramientas)

Una herramienta puede **pedir más entrada al usuario** durante su ejecución. En el primer `call_tool`, devuelve una respuesta `elicit` en lugar de `content`:

```json
{
  "elicit": {
    "message": "¿Qué clase de vuelo?",
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

roxy reenvía la elicitation al cliente MCP. Cuando el usuario completa los datos, roxy llama a tu herramienta **de nuevo** pasando los valores recogidos en `elicitation_results` y tu `context` original:

```json
{
  "type": "call_tool",
  "name": "book_flight",
  "arguments": { "destination": "Tokio" },
  "elicitation_results": [{ "class": "business" }],
  "context": { "step": 1, "destination": "Tokio" }
}
```

Puedes encadenar varias rondas de elicitation devolviendo más `elicit` hasta que se hayan recogido todos los datos.

### Respuesta de error

Cualquier tipo de petición puede devolver un error en lugar de éxito:

```json
{ "error": { "code": 404, "message": "Unknown tool: foo" } }
```

## Arquitectura

```
Cliente MCP (Claude, Cursor, Zed, ...)
       │
       │ JSON-RPC sobre stdio o Streamable HTTP
       ▼
┌──────────────┐
│    rmcp      │  Protocolo MCP, transporte, sesiones
└──────────────┘
       │
       ▼
┌──────────────┐
│  RoxyServer  │  enrutado de métodos MCP, caché de capacidades
└──────────────┘
       │
       │ JSON simplificado (UpstreamEnvelope + UpstreamRequest)
       ▼
┌──────────────────────────┐
│    UpstreamExecutor      │  trait con 2 implementaciones
│  ┌────────┬───────────┐  │
│  │FastCgi │  Http     │  │
│  └────────┴───────────┘  │
└──────────────────────────┘
       │                │
       ▼                ▼
   PHP-FPM /        endpoint
   cualquier        HTTP(S)
   FastCGI
```

### Estructura del código fuente

```
src/
  main.rs             CLI, logging, arranque del transporte, selección del executor
  lib.rs              Raíz del crate de biblioteca (reexportaciones para benchmarks y tests)
  config.rs           Config clap, UpstreamKind (auto-detección), FcgiAddress
  protocol.rs         Tipos JSON internos (UpstreamEnvelope, UpstreamRequest, ...)
  server.rs           RoxyServer: implementación de rmcp ServerHandler + caché de discover
  executor/
    mod.rs            Trait UpstreamExecutor
    fastcgi.rs        FastCgiExecutor: deadpool + fastcgi-client
    http.rs           HttpExecutor: reqwest + rustls
examples/
  handler.php         Handler PHP de ejemplo con todas las funcionalidades
  echo_upstream.rs    Backend HTTP echo mínimo para pruebas de carga
  bench_client.rs     Cliente de carga end-to-end para profiling
```

### Decisiones clave de diseño

- **rmcp hace el trabajo pesado.** El crate oficial `rmcp` se encarga de toda la complejidad del protocolo MCP (JSON-RPC, negociación de transporte, gestión de sesiones). roxy solo implementa `ServerHandler`.
- **El upstream es enchufable.** El trait `UpstreamExecutor` abstrae la comunicación con el backend. FastCGI y HTTP son las implementaciones actuales; añadir un backend nuevo (gRPC, stdio, WebSocket) es cuestión de implementar un trait.
- **Las capacidades se cachean.** roxy llama a `discover` una vez al arrancar y mantiene tools/resources/prompts en memoria. Los clientes MCP reciben respuestas instantáneas a `initialize` sin tocar el upstream.
- **Pooling de conexiones para FastCGI.** `deadpool` mantiene calientes las conexiones a PHP-FPM, evitando configurar el socket en cada petición.
- **TLS puro Rust vía rustls.** Sin OpenSSL, sin bibliotecas del sistema. Builds Linux totalmente estáticos, cross-compilation fácil, binarios portables.
- **El upstream se mantiene simple.** Tu handler nunca ve JSON-RPC, IDs de petición (salvo como campo opaco del envelope), estado de sesión o framing MCP. JSON simple a la entrada, JSON simple a la salida.

## Desarrollo

### Build y tests

```bash
cargo build           # debug
cargo build --release # optimizado
cargo test            # ejecutar tests
cargo clippy          # linter
cargo fmt             # formato
```

### Ejecución local con el handler PHP de ejemplo

```bash
# Terminal 1: arranca PHP-FPM
php-fpm --nodaemonize -d "listen=127.0.0.1:9000" -d "pm=static" -d "pm.max_children=4"

# Terminal 2: arranca roxy con el handler de ejemplo
cargo run -- \
    --upstream 127.0.0.1:9000 \
    --upstream-entrypoint "$(pwd)/examples/handler.php"
```

Luego conéctate con cualquier cliente MCP, o envía JSON-RPC manualmente por stdio.

### Workflow de release

Los releases taggeados (`git tag vX.Y.Z && git push origin vX.Y.Z`) disparan `.github/workflows/release.yml`, que:

1. Compila binarios de release para los cuatro targets (macOS arm64/x86_64, Linux musl arm64/x86_64)
2. Los empaqueta como `.tar.gz` con checksums SHA256
3. Construye paquetes `.deb` y `.rpm` para ambas arquitecturas de Linux
4. Publica una GitHub Release con todos los artefactos
5. Actualiza la fórmula de Homebrew en [`petstack/homebrew-tap`](https://github.com/petstack/homebrew-tap) (si el secreto `HOMEBREW_TAP_TOKEN` está configurado)

Configuración del tap — ver [`packaging/homebrew/README.md`](../packaging/homebrew/README.md).

## Licencia

[Apache-2.0](../LICENSE).
