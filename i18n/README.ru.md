# roxy

---

[English](../README.md) · **Русский** · [Українська](README.uk.md) · [Беларуская](README.be.md) · [Polski](README.pl.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · [Español](README.es.md) · [中文](README.zh-CN.md) · [日本語](README.ja.md)

---

**Высокопроизводительный прокси-сервер MCP (Model Context Protocol) на Rust.**

roxy соединяет MCP-клиенты (Claude Desktop, Cursor, Zed и другие) с любым upstream-обработчиком, работающим как **FastCGI**-бэкенд (например, PHP-FPM) или **HTTP(S)**-эндпоинт. Rust берёт на себя всё, что критично для производительности — транспорт, парсинг протокола, пулинг соединений, конкурентность — через официальный крейт [`rmcp`](https://crates.io/crates/rmcp). Ваш обработчик работает с простым, упрощённым JSON-протоколом и возвращает результаты.

Это позволяет писать MCP-серверы на **любом языке** — PHP, Python, Node, Go, Ruby — не переизобретая каждый раз JSON-RPC framing, транспорт, управление сессиями и согласование возможностей.

## Оглавление

- [Возможности](#возможности)
- [Установка](#установка)
  - [Homebrew (macOS и Linux)](#homebrew-macos-и-linux)
  - [Скрипт установки (любой Unix)](#скрипт-установки-любой-unix)
  - [Debian / Ubuntu (.deb)](#debian--ubuntu-deb)
  - [Fedora / RHEL / openSUSE (.rpm)](#fedora--rhel--opensuse-rpm)
  - [Alpine / любой Linux (статический tarball)](#alpine--любой-linux-статический-tarball)
  - [Из исходников](#из-исходников)
  - [Проверка установки](#проверка-установки)
- [Быстрый старт](#быстрый-старт)
- [CLI-справочник](#cli-справочник)
  - [Переменные окружения](#переменные-окружения)
- [Написание upstream-обработчика](#написание-upstream-обработчика)
- [Справочник upstream-протокола](#справочник-upstream-протокола)
  - [Типы запросов](#типы-запросов)
  - [Elicitation (многошаговый ввод для инструментов)](#elicitation-многошаговый-ввод-для-инструментов)
  - [Ответ с ошибкой](#ответ-с-ошибкой)
- [Архитектура](#архитектура)
- [Разработка](#разработка)
- [Лицензия](#лицензия)

## Возможности

- **Мульти-бэкенд**: FastCGI (TCP или Unix socket) и HTTP(S) upstream'ы, автоопределение по формату URL
- **Транспорты**: stdio и Streamable HTTP, оба поддерживаются нативно через `rmcp`
- **Возможности MCP 2025-06-18**: elicitation (многошаговый ввод), структурированный вывод инструментов, ссылки на ресурсы в ответах
- **Пулинг соединений** для FastCGI (через `deadpool`)
- **TLS через rustls** — нет зависимости от OpenSSL, полностью статические musl-сборки
- **Кеширование возможностей** — tools/resources/prompts опрашиваются один раз при старте
- **Произвольные HTTP-заголовки**, настраиваемые таймауты, передача request/session ID в upstream

## Установка

Предсобранные бинарные файлы публикуются на каждый тегированный релиз для **macOS (arm64, x86_64)** и **Linux (arm64, x86_64, статика musl)**. Выбирайте способ по своей платформе.

### Homebrew (macOS и Linux)

```bash
brew tap petstack/tap
brew install roxy
```

Работает на macOS (Intel и Apple Silicon) и Linux (x86_64 и arm64) с установленным Homebrew.

### Скрипт установки (любой Unix)

```bash
curl -sSfL https://raw.githubusercontent.com/petstack/roxy/main/install.sh | sh
```

Скрипт автоматически определяет вашу ОС и архитектуру, скачивает нужный tarball из GitHub Releases, проверяет контрольную сумму SHA256 и устанавливает бинарь в `/usr/local/bin/roxy` (при необходимости через `sudo`).

Опции:

```bash
# Установить конкретную версию
curl -sSfL .../install.sh | sh -s -- --version v0.1.0

# Установить в свою директорию (sudo не нужен)
curl -sSfL .../install.sh | sh -s -- --bin-dir $HOME/.local/bin
```

Также работают переменные окружения `ROXY_REPO`, `ROXY_VERSION`, `ROXY_BIN_DIR`.

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

### Alpine / любой Linux (статический tarball)

Linux-бинарники статически слинкованы с musl libc, поэтому работают на **любом** Linux-дистрибутиве без зависимостей:

```bash
# Выберите свою архитектуру
ARCH=x86_64   # или aarch64
curl -sSfL https://github.com/petstack/roxy/releases/latest/download/roxy-v0.1.0-${ARCH}-unknown-linux-musl.tar.gz | tar -xz
sudo install -m 755 roxy-v0.1.0-${ARCH}-unknown-linux-musl/roxy /usr/local/bin/
```

Работает на Alpine, Debian, Ubuntu, RHEL, Arch, Amazon Linux, Void, NixOS и всём остальном с Linux-ядром.

### Из исходников

Требуется [Rust](https://rustup.rs/) (edition 2024, stable toolchain):

```bash
git clone https://github.com/petstack/roxy
cd roxy
cargo build --release
# Бинарь будет в ./target/release/roxy
```

Или через `cargo install`:

```bash
cargo install --git https://github.com/petstack/roxy
```

### Проверка установки

```bash
roxy --version
roxy --help
```

## Быстрый старт

roxy требует **один аргумент**: `--upstream`, указывающий на ваш обработчик. Тип upstream **определяется автоматически** по формату URL:

| Формат URL | Тип бэкенда |
|---|---|
| `http://...` или `https://...` | HTTP(S) |
| `host:port` | FastCGI TCP |
| `/путь/к/сокету` | FastCGI Unix socket |

### Пример: HTTP-бэкенд

```bash
# Запустите свой HTTP-обработчик на порту 8000 (любой язык, любой фреймворк)
# Затем направьте на него roxy:
roxy --upstream http://localhost:8000/mcp
```

### Пример: PHP-FPM бэкенд

```bash
# Запуск PHP-FPM
php-fpm --nodaemonize \
    -d "listen=127.0.0.1:9000" \
    -d "pm=static" \
    -d "pm.max_children=4"

# Направляем roxy
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /absolute/path/to/handler.php
```

### Подключение из MCP-клиента

Для Claude Desktop или любого клиента, запускающего MCP-серверы как подпроцессы (транспорт stdio — по умолчанию):

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

Для сетевых клиентов, подключающихся по Streamable HTTP:

```bash
roxy --transport http --port 8080 --upstream http://localhost:8000/mcp
# Клиент подключается к http://localhost:8080/mcp
```

## CLI-справочник

```
roxy [OPTIONS] --upstream <UPSTREAM>
```

| Флаг | По умолчанию | Описание |
|---|---|---|
| `--upstream <URL>` | **обязателен** | URL бэкенда. Тип определяется автоматически (см. таблицу выше) |
| `--transport <MODE>` | `stdio` | Транспорт MCP-клиента: `stdio` или `http` |
| `--port <PORT>` | `8080` | Порт HTTP-сервера (только при `--transport http`) |
| `--upstream-entrypoint <PATH>` | — | `SCRIPT_FILENAME`, отправляемый в FastCGI-бэкенд (обязателен для PHP-FPM) |
| `--upstream-insecure` | `false` | Пропускать проверку TLS-сертификатов для HTTPS-upstream'ов |
| `--upstream-timeout <SECS>` | `30` | Таймаут HTTP-upstream запроса в секундах |
| `--upstream-header <HEADER>` | — | Статический HTTP-заголовок, добавляемый к каждому запросу к HTTP-upstream'у, `Name: Value`. Можно указывать несколько раз. Только для HTTP-upstream'ов — для FastCGI игнорируется |
| `--pool-size <N>` | `16` | Размер пула соединений FastCGI |
| `--log-format <FORMAT>` | `pretty` | Формат логов: `pretty` или `json` |

**Уровень** логирования контролируется через переменную окружения `RUST_LOG`:

```bash
RUST_LOG=debug roxy --upstream http://localhost:8000/mcp
RUST_LOG=roxy=debug,rmcp=info roxy --upstream ...  # фильтры по модулям
```

### Полный пример HTTP-бэкенда

```bash
roxy --upstream https://api.example.com/mcp \
     --upstream-header "Authorization: Bearer $TOKEN" \
     --upstream-header "X-Tenant: acme" \
     --upstream-timeout 60
```

### Полный пример FastCGI (PHP-FPM)

```bash
# TCP
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /srv/app/handler.php

# Unix socket
roxy --upstream /var/run/php-fpm.sock --upstream-entrypoint /srv/app/handler.php

# Транспорт HTTP с FastCGI-upstream
roxy --transport http --port 8080 \
     --upstream 127.0.0.1:9000 \
     --upstream-entrypoint /srv/app/handler.php
```

### Проброс клиентских заголовков

При использовании `--transport http` каждый входящий HTTP-заголовок MCP-клиента автоматически пробрасывается в upstream-бэкенд — никакой настройки не требуется. Hop-by-hop заголовки (RFC 7230 §6.1: `Connection`, `Keep-Alive`, `Proxy-Authenticate`, `Proxy-Authorization`, `TE`, `Trailer`, `Transfer-Encoding`, `Upgrade`), а также заголовки, которыми управляет сам roxy (`Host`, `Content-Type`, `Content-Length`), отфильтровываются. Всё остальное — `Authorization`, `Cookie`, `X-Forwarded-For`, произвольные `X-*` заголовки, `mcp-session-id` — достигает upstream'а без изменений. Это поведение повторяет поведение nginx `fastcgi_pass` / `proxy_pass` по умолчанию и позволяет вашему upstream-бэкенду аутентифицировать конечного клиента (проверять bearer-токены, инспектировать session cookie) без необходимости понимать схему аутентификации на стороне roxy.

| Upstream | Форма передачи |
|---|---|
| HTTP | Пробрасывается как реальные HTTP-заголовки запроса. Заголовки с несколькими значениями (например, два `X-Forwarded-For`) сохраняются. |
| FastCGI | Преобразуется в CGI-параметры `HTTP_*` согласно RFC 3875 §4.1.18 — PHP-обработчики читают их через `$_SERVER['HTTP_AUTHORIZATION']`, `$_SERVER['HTTP_X_FORWARDED_FOR']` и т.д. Заголовки с несколькими значениями объединяются через `", "` в соответствии с семантикой nginx `$http_*`. |

`--upstream-header` продолжает работать как прежде для HTTP-upstream'ов — он задаёт **собственную** статическую идентификацию roxy для upstream'а (сервисный токен, фиксированный `X-Client-Id` и т.п.). Если пробрасываемый клиентский заголовок совпадает по имени со статическим заголовком из `--upstream-header`, **побеждает** пробрасываемое значение: идентификация конкретного запроса от вызывающей стороны более специфична, чем дефолт roxy, — это соответствует типичному поведению обратного прокси. Для FastCGI-upstream'ов `--upstream-header` в настоящее время является no-op — используйте вместо него автоматический проброс.

При использовании `--transport stdio` входящего HTTP-запроса нет, поэтому заголовки не пробрасываются; статические записи `--upstream-header` по-прежнему применяются к HTTP-upstream'ам как обычно.

### Переменные окружения

Все флаги CLI принимают соответствующую переменную окружения `ROXY_*` в качестве необязательного запасного значения. Порядок приоритета: **CLI > env > default**: флаг, переданный в командной строке, всегда побеждает; переменная окружения используется только при отсутствии флага; встроенное значение по умолчанию применяется, только если не задано ни то, ни другое.

| Флаг | Env variable | Пример |
|---|---|---|
| `--transport` | `ROXY_TRANSPORT` | `stdio` \| `http` |
| `--port` | `ROXY_PORT` | `8080` |
| `--upstream` | `ROXY_UPSTREAM` | `http://localhost:8000/handler` |
| `--upstream-entrypoint` | `ROXY_UPSTREAM_ENTRYPOINT` | `/srv/handler.php` |
| `--upstream-insecure` | `ROXY_UPSTREAM_INSECURE` | `true` \| `false` |
| `--upstream-timeout` | `ROXY_UPSTREAM_TIMEOUT` | `30` |
| `--upstream-header` | `ROXY_UPSTREAM_HEADER` | разделённые переносами строки, см. ниже |
| `--pool-size` | `ROXY_POOL_SIZE` | `16` |
| `--log-format` | `ROXY_LOG_FORMAT` | `pretty` \| `json` |

#### Несколько значений upstream-header

`ROXY_UPSTREAM_HEADER` принимает несколько строк заголовков, разделённых символами перевода строки. Это естественно отображается на блочный скаляр Kubernetes YAML:

```yaml
env:
  - name: ROXY_UPSTREAM_HEADER
    value: |-
      Authorization: Bearer xyz
      X-Trace-Id: abc
```

Из локального шелла используйте кавычки `$'...'`, чтобы `\n` стало настоящим переносом строки:

```bash
ROXY_UPSTREAM_HEADER=$'Authorization: Bearer xyz\nX-Trace-Id: abc' \
  roxy --upstream https://api.example.com/mcp
```

Ведущие и завершающие пустые строки отбрасываются при запуске, поэтому особенности блочного скаляра YAML `|-` не порождают некорректных заголовков. Если `--upstream-header` передан в командной строке, `ROXY_UPSTREAM_HEADER` игнорируется полностью — слияния двух источников нет.

#### Булевы значения

`ROXY_UPSTREAM_INSECURE` принимает только **точные строки в нижнем регистре** `true` или `false`. Числовые формы (`1`, `0`) и другие варианты написания (`TRUE`, `True`, `YES`, `on`) отвергаются парсером clap (`SetTrue + env`) и приводят к ошибке при запуске. Флаг CLI `--upstream-insecure` (без значения) по-прежнему работает и означает `true`.

#### `RUST_LOG`

roxy учитывает стандартную переменную окружения `RUST_LOG`, которая читается при запуске через `tracing_subscriber::EnvFilter`; она не связана с переменными `ROXY_*` выше и остаётся неизменной.

## Написание upstream-обработчика

Ваш обработчик получает простые JSON-запросы и возвращает простые JSON-ответы. **Он никогда не видит JSON-RPC, MCP framing или состояние сессий.** roxy транслирует всё.

### Для HTTP-бэкендов

Подойдёт любой HTTP-сервер, который читает JSON из тела запроса и пишет JSON в ответ. Пример на Python/Flask:

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

### Для FastCGI (PHP-FPM) бэкендов

Минимальный PHP-обработчик:

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

Полный пример с несколькими инструментами, структурированным выводом, elicitation и ссылками на ресурсы — см. [`examples/handler.php`](../examples/handler.php).

## Справочник upstream-протокола

Каждый запрос от roxy к вашему upstream — это JSON-объект с общими полями envelope:

```json
{
  "type": "...",
  "session_id": "optional-uuid-or-null",
  "request_id": "uuid-per-request",
  ...
}
```

### Типы запросов

#### `discover`

Отправляется один раз при старте roxy. Ваш обработчик должен вернуть полный каталог поддерживаемых инструментов, ресурсов и промптов. roxy кеширует результат и отдаёт его всем MCP-клиентам без повторных запросов.

```json
// Ответ
{
  "tools": [
    {
      "name": "tool_name",
      "title": "Human Name",
      "description": "Что делает",
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

Поля `title`, `description`, `mime_type`, `output_schema` необязательны.

#### `call_tool`

Выполнить инструмент по имени. Запрос:

```json
{
  "type": "call_tool",
  "name": "tool_name",
  "arguments": { "key": "value" },
  "elicitation_results": [ ... ],   // опционально: см. раздел Elicitation ниже
  "context": { ... }                 // опционально: эхом из предыдущего elicit-ответа
}
```

Успешный ответ (обычный текстовый вывод):

```json
{
  "content": [
    { "type": "text", "text": "результат" }
  ]
}
```

Успешный ответ со **структурированным содержимым** (для инструментов с `output_schema`):

```json
{
  "content": [{ "type": "text", "text": "5 + 3 = 8" }],
  "structured_content": { "sum": 8, "operands": { "a": 5, "b": 3 } }
}
```

Успешный ответ со **ссылкой на ресурс**, встроенной в вывод:

```json
{
  "content": [
    { "type": "text", "text": "Бронирование подтверждено." },
    {
      "type": "resource_link",
      "uri": "myapp://bookings/1234",
      "name": "booking-1234",
      "title": "Бронирование #1234"
    }
  ]
}
```

#### `read_resource`

Прочитать ресурс по URI. Запрос:

```json
{ "type": "read_resource", "uri": "myapp://status" }
```

Ответ: тот же формат `content`, что и у `call_tool`.

#### `get_prompt`

Сгенерировать промпт. Запрос:

```json
{ "type": "get_prompt", "name": "greet", "arguments": { "name": "Alice" } }
```

Ответ: тот же формат `content`, что и у `call_tool`.

#### `elicitation_cancelled`

Отправляется, когда MCP-клиент отменяет elicitation (см. ниже). Ваш обработчик может залогировать/почистить; ответ игнорируется.

```json
{ "type": "elicitation_cancelled", "name": "tool_name", "action": "decline", "context": {...} }
```

### Elicitation (многошаговый ввод для инструментов)

Инструмент может **запросить дополнительный ввод от пользователя** прямо во время выполнения. На первом `call_tool` верните ответ `elicit` вместо `content`:

```json
{
  "elicit": {
    "message": "Какой класс перелёта?",
    "schema": {
      "type": "object",
      "properties": {
        "class": { "type": "string", "enum": ["economy", "business", "first"] }
      },
      "required": ["class"]
    },
    "context": { "step": 1, "destination": "Токио" }
  }
}
```

roxy перешлёт elicitation MCP-клиенту. Когда пользователь заполнит данные, roxy вызовет ваш инструмент **снова**, передав собранные значения в `elicitation_results` и ваш исходный `context`:

```json
{
  "type": "call_tool",
  "name": "book_flight",
  "arguments": { "destination": "Токио" },
  "elicitation_results": [{ "class": "business" }],
  "context": { "step": 1, "destination": "Токио" }
}
```

Можно цепочкой собирать несколько раундов elicitation, возвращая очередной `elicit`, пока все данные не будут получены.

### Ответ с ошибкой

Любой тип запроса может вернуть ошибку вместо успеха:

```json
{ "error": { "code": 404, "message": "Unknown tool: foo" } }
```

## Архитектура

```
MCP-клиент (Claude, Cursor, Zed, ...)
       │
       │ JSON-RPC по stdio или Streamable HTTP
       ▼
┌──────────────┐
│    rmcp      │  Протокол MCP, транспорт, сессии
└──────────────┘
       │
       ▼
┌──────────────┐
│  RoxyServer  │  маршрутизация MCP-методов, кеш возможностей
└──────────────┘
       │
       │ упрощённый JSON (UpstreamEnvelope + UpstreamRequest)
       ▼
┌──────────────────────────┐
│    UpstreamExecutor      │  трейт с 2 реализациями
│  ┌────────┬───────────┐  │
│  │FastCgi │  Http     │  │
│  └────────┴───────────┘  │
└──────────────────────────┘
       │                │
       ▼                ▼
   PHP-FPM /        HTTP(S)
   любой FastCGI    эндпоинт
```

### Структура исходников

```
src/
  main.rs             CLI, логирование, запуск транспорта, выбор executor'а
  lib.rs              Корень библиотечного крейта (реэкспорты для бенчмарков и тестов)
  config.rs           clap Config, UpstreamKind (автоопределение), FcgiAddress
  protocol.rs         Внутренние JSON-типы (UpstreamEnvelope, UpstreamRequest, ...)
  server.rs           RoxyServer: реализация rmcp ServerHandler + кеш discover
  executor/
    mod.rs            Трейт UpstreamExecutor
    fastcgi.rs        FastCgiExecutor: deadpool + fastcgi-client
    http.rs           HttpExecutor: reqwest + rustls
examples/
  handler.php         Полный пример PHP-обработчика со всеми возможностями
  echo_upstream.rs    Минимальный HTTP echo-бэкенд для нагрузочного тестирования
  bench_client.rs     End-to-end нагрузочный клиент для профилирования
```

### Ключевые архитектурные решения

- **rmcp делает тяжёлую работу.** Официальный крейт `rmcp` берёт на себя всю сложность MCP-протокола (JSON-RPC, согласование транспорта, управление сессиями). roxy реализует только `ServerHandler`.
- **Upstream подключаемый.** Трейт `UpstreamExecutor` абстрагирует коммуникацию с бэкендом. FastCGI и HTTP — текущие реализации; добавить новый бэкенд (gRPC, stdio, WebSocket) = реализовать один трейт.
- **Возможности кешируются.** roxy вызывает `discover` один раз при старте и держит tools/resources/prompts в памяти. MCP-клиенты получают мгновенные ответы на `initialize`, не задевая upstream.
- **Пулинг соединений для FastCGI.** `deadpool` держит соединения с PHP-FPM тёплыми, избегая настройки сокета на каждый запрос.
- **Pure-Rust TLS через rustls.** Нет OpenSSL, нет системных библиотек. Полностью статические Linux-сборки, простая кросс-компиляция, переносимые бинари.
- **Upstream остаётся простым.** Ваш обработчик никогда не видит JSON-RPC, request ID (кроме как в непрозрачном поле envelope), состояние сессии или MCP framing. Простой JSON на входе, простой JSON на выходе.

## Разработка

### Сборка и тесты

```bash
cargo build           # debug
cargo build --release # оптимизированная сборка
cargo test            # прогон тестов
cargo clippy          # линтер
cargo fmt             # форматирование
```

### Локальный запуск с примером PHP-обработчика

```bash
# Терминал 1: запускаем PHP-FPM
php-fpm --nodaemonize -d "listen=127.0.0.1:9000" -d "pm=static" -d "pm.max_children=4"

# Терминал 2: запускаем roxy с примером обработчика
cargo run -- \
    --upstream 127.0.0.1:9000 \
    --upstream-entrypoint "$(pwd)/examples/handler.php"
```

Затем подключайтесь любым MCP-клиентом или отправляйте JSON-RPC руками через stdio.

### Процесс релиза

Тегированные релизы (`git tag vX.Y.Z && git push origin vX.Y.Z`) запускают `.github/workflows/release.yml`, который:

1. Собирает release-бинари для всех четырёх таргетов (macOS arm64/x86_64, Linux musl arm64/x86_64)
2. Упаковывает их в `.tar.gz` с SHA256-хешами
3. Собирает `.deb` и `.rpm` пакеты для обеих Linux-архитектур
4. Публикует GitHub Release со всеми артефактами
5. Обновляет формулу Homebrew в [`petstack/homebrew-tap`](https://github.com/petstack/homebrew-tap) (если задан секрет `HOMEBREW_TAP_TOKEN`)

Настройка tap — см. [`packaging/homebrew/README.md`](../packaging/homebrew/README.md).

## Лицензия

[AGPL-3.0-only](../LICENSE). Если вы запускаете изменённую версию roxy как сетевой сервис, вы обязаны предоставить свои изменения пользователям этого сервиса.
