# roxy

---

[English](../README.md) · [Русский](README.ru.md) · **Українська** · [Беларуская](README.be.md) · [Polski](README.pl.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · [Español](README.es.md) · [中文](README.zh-CN.md) · [日本語](README.ja.md)

---

**Високопродуктивний проксі-сервер MCP (Model Context Protocol) на Rust.**

roxy з’єднує MCP-клієнтів (Claude Desktop, Cursor, Zed тощо) з будь-яким upstream-обробником, що працює як **FastCGI**-бекенд (наприклад, PHP-FPM) або **HTTP(S)**-ендпоінт. Rust бере на себе все, що критично для продуктивності — транспорт, парсинг протоколу, пулінг з’єднань, конкурентність — через офіційний крейт [`rmcp`](https://crates.io/crates/rmcp). Ваш обробник працює з простим, спрощеним JSON-протоколом і повертає результати.

Це дозволяє писати MCP-сервери **будь-якою мовою** — PHP, Python, Node, Go, Ruby — без необхідності щоразу переписувати JSON-RPC framing, транспорт, керування сесіями та узгодження можливостей.

## Зміст

- [Можливості](#можливості)
- [Встановлення](#встановлення)
  - [Homebrew (macOS та Linux)](#homebrew-macos-та-linux)
  - [Скрипт встановлення (будь-який Unix)](#скрипт-встановлення-будь-який-unix)
  - [Debian / Ubuntu (.deb)](#debian--ubuntu-deb)
  - [Fedora / RHEL / openSUSE (.rpm)](#fedora--rhel--opensuse-rpm)
  - [Alpine / будь-який Linux (статичний tarball)](#alpine--будь-який-linux-статичний-tarball)
  - [З вихідного коду](#з-вихідного-коду)
  - [Перевірка встановлення](#перевірка-встановлення)
- [Швидкий старт](#швидкий-старт)
- [Довідник CLI](#довідник-cli)
  - [Змінні середовища](#змінні-середовища)
- [Написання upstream-обробника](#написання-upstream-обробника)
- [Довідник upstream-протоколу](#довідник-upstream-протоколу)
  - [Типи запитів](#типи-запитів)
  - [Elicitation (багатокроковий ввід для інструментів)](#elicitation-багатокроковий-ввід-для-інструментів)
  - [Відповідь з помилкою](#відповідь-з-помилкою)
- [Архітектура](#архітектура)
- [Розробка](#розробка)
- [Ліцензія](#ліцензія)

## Можливості

- **Мульти-бекенд**: FastCGI (TCP або Unix socket) та HTTP(S) upstream’и, автовизначення за форматом URL
- **Транспорти**: stdio та Streamable HTTP, обидва підтримуються нативно через `rmcp`
- **Можливості MCP 2025-06-18**: elicitation (багатокроковий ввід), структурований вивід інструментів, посилання на ресурси у відповідях
- **Пулінг з’єднань** для FastCGI (через `deadpool`)
- **TLS через rustls** — без залежності від OpenSSL, повністю статичні musl-збірки
- **Кешування можливостей** — tools/resources/prompts опитуються один раз при старті
- **Довільні HTTP-заголовки**, налаштовувані таймаути, передача request/session ID в upstream

## Встановлення

Передзібрані бінарні файли публікуються на кожен тегований реліз для **macOS (arm64, x86_64)** і **Linux (arm64, x86_64, статика musl)**. Обирайте спосіб під свою платформу.

### Homebrew (macOS та Linux)

```bash
brew tap petstack/tap
brew install roxy
```

Працює на macOS (Intel та Apple Silicon) і Linux (x86_64 та arm64) зі встановленим Homebrew.

### Скрипт встановлення (будь-який Unix)

```bash
curl -sSfL https://raw.githubusercontent.com/petstack/roxy/main/install.sh | sh
```

Скрипт автоматично визначає вашу ОС та архітектуру, завантажує потрібний tarball з GitHub Releases, перевіряє контрольну суму SHA256 і встановлює бінар у `/usr/local/bin/roxy` (за потреби через `sudo`).

Опції:

```bash
# Встановити конкретну версію
curl -sSfL .../install.sh | sh -s -- --version v0.1.0

# Встановити у власну директорію (sudo не потрібен)
curl -sSfL .../install.sh | sh -s -- --bin-dir $HOME/.local/bin
```

Також працюють змінні середовища `ROXY_REPO`, `ROXY_VERSION`, `ROXY_BIN_DIR`.

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

### Alpine / будь-який Linux (статичний tarball)

Linux-бінарники статично злінковані з musl libc, тому працюють на **будь-якому** Linux-дистрибутиві без залежностей:

```bash
# Оберіть свою архітектуру
ARCH=x86_64   # або aarch64
curl -sSfL https://github.com/petstack/roxy/releases/latest/download/roxy-v0.1.0-${ARCH}-unknown-linux-musl.tar.gz | tar -xz
sudo install -m 755 roxy-v0.1.0-${ARCH}-unknown-linux-musl/roxy /usr/local/bin/
```

Працює на Alpine, Debian, Ubuntu, RHEL, Arch, Amazon Linux, Void, NixOS та всьому іншому з Linux-ядром.

### З вихідного коду

Потрібен [Rust](https://rustup.rs/) (edition 2024, stable toolchain):

```bash
git clone https://github.com/petstack/roxy
cd roxy
cargo build --release
# Бінар буде у ./target/release/roxy
```

Або через `cargo install`:

```bash
cargo install --git https://github.com/petstack/roxy
```

### Перевірка встановлення

```bash
roxy --version
roxy --help
```

## Швидкий старт

roxy потребує **один аргумент**: `--upstream`, що вказує на ваш обробник. Тип upstream **визначається автоматично** за форматом URL:

| Формат URL | Тип бекенда |
|---|---|
| `http://...` або `https://...` | HTTP(S) |
| `host:port` | FastCGI TCP |
| `/шлях/до/сокета` | FastCGI Unix socket |

### Приклад: HTTP-бекенд

```bash
# Запустіть свій HTTP-обробник на порту 8000 (будь-яка мова, будь-який фреймворк)
# Потім направте на нього roxy:
roxy --upstream http://localhost:8000/mcp
```

### Приклад: PHP-FPM бекенд

```bash
# Запуск PHP-FPM
php-fpm --nodaemonize \
    -d "listen=127.0.0.1:9000" \
    -d "pm=static" \
    -d "pm.max_children=4"

# Направляємо roxy
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /absolute/path/to/handler.php
```

### Підключення з MCP-клієнта

Для Claude Desktop або будь-якого клієнта, що запускає MCP-сервери як підпроцеси (транспорт stdio — за замовчуванням):

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

Для мережевих клієнтів, що підключаються по Streamable HTTP:

```bash
roxy --transport http --port 8080 --upstream http://localhost:8000/mcp
# Клієнт підключається до http://localhost:8080/mcp
```

## Довідник CLI

```
roxy [OPTIONS] --upstream <UPSTREAM>
```

| Прапорець | За замовчуванням | Опис |
|---|---|---|
| `--upstream <URL>` | **обов’язковий** | URL бекенда. Тип визначається автоматично (див. таблицю вище) |
| `--transport <MODE>` | `stdio` | Транспорт MCP-клієнта: `stdio` або `http` |
| `--port <PORT>` | `8080` | Порт HTTP-сервера (тільки при `--transport http`) |
| `--upstream-entrypoint <PATH>` | — | `SCRIPT_FILENAME`, що надсилається у FastCGI-бекенд (обов’язковий для PHP-FPM) |
| `--upstream-insecure` | `false` | Пропускати перевірку TLS-сертифікатів для HTTPS-upstream’ів |
| `--upstream-timeout <SECS>` | `30` | Таймаут HTTP-upstream запиту в секундах |
| `--upstream-header <HEADER>` | — | Статичний HTTP-заголовок, що додається до кожного запиту до HTTP-upstream, `Name: Value`. Можна вказувати кілька разів. Тільки для HTTP-upstream — ігнорується для FastCGI |
| `--pool-size <N>` | `16` | Розмір пулу з’єднань FastCGI |
| `--log-format <FORMAT>` | `pretty` | Формат логів: `pretty` або `json` |

**Рівень** логування контролюється через змінну середовища `RUST_LOG`:

```bash
RUST_LOG=debug roxy --upstream http://localhost:8000/mcp
RUST_LOG=roxy=debug,rmcp=info roxy --upstream ...  # фільтри за модулями
```

### Повний приклад HTTP-бекенда

```bash
roxy --upstream https://api.example.com/mcp \
     --upstream-header "Authorization: Bearer $TOKEN" \
     --upstream-header "X-Tenant: acme" \
     --upstream-timeout 60
```

### Повний приклад FastCGI (PHP-FPM)

```bash
# TCP
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /srv/app/handler.php

# Unix socket
roxy --upstream /var/run/php-fpm.sock --upstream-entrypoint /srv/app/handler.php

# Транспорт HTTP з FastCGI-upstream
roxy --transport http --port 8080 \
     --upstream 127.0.0.1:9000 \
     --upstream-entrypoint /srv/app/handler.php
```

### Перенаправлення заголовків клієнта

При використанні `--transport http` кожен вхідний заголовок MCP-клієнта автоматично пересилається до upstream-бекенда — жодного додаткового налаштування не потрібно. Hop-by-hop заголовки (RFC 7230 §6.1: `Connection`, `Keep-Alive`, `Proxy-Authenticate`, `Proxy-Authorization`, `TE`, `Trailer`, `Transfer-Encoding`, `Upgrade`) та заголовки, якими керує сам roxy (`Host`, `Content-Type`, `Content-Length`), відфільтровуються. Усе інше — `Authorization`, `Cookie`, `X-Forwarded-For`, власні `X-*`-заголовки, `mcp-session-id` — досягає upstream'а у незмінному вигляді. Це відповідає поведінці nginx `fastcgi_pass` / `proxy_pass` за замовчуванням і дозволяє upstream-бекенду автентифікувати кінцевого клієнта (перевіряти bearer-токени, аналізувати сесійні cookie) без необхідності для roxy розуміти схему автентифікації.

| Upstream | Форма транспорту |
|---|---|
| HTTP | Пересилається як справжні HTTP-заголовки запиту. Заголовки з кількома значеннями (наприклад, два записи `X-Forwarded-For`) зберігаються. |
| FastCGI | Перетворюється на CGI-параметри `HTTP_*` згідно з RFC 3875 §4.1.18 — PHP-обробники читають їх з `$_SERVER['HTTP_AUTHORIZATION']`, `$_SERVER['HTTP_X_FORWARDED_FOR']` тощо. Заголовки з кількома значеннями об'єднуються через `", "`, що відповідає семантиці nginx `$http_*`. |

`--upstream-header` продовжує працювати як раніше для HTTP-upstream'ів — він визначає **власну** статичну ідентичність roxy щодо upstream'а (service-токен, фіксований `X-Client-Id` тощо). Якщо пересланий заголовок клієнта збігається за іменем зі статичним заголовком `--upstream-header`, перемагає пересланий заголовок: ідентичність виклику на рівні запиту є конкретнішою, ніж значення roxy за замовчуванням — це відповідає типовій поведінці зворотного проксі. `--upstream-header` наразі не має ефекту для FastCGI-upstream'ів — використовуйте натомість автоматичне перенаправлення.

При використанні `--transport stdio` вхідного HTTP-запиту немає, тому заголовки не пересилаються; статичні записи `--upstream-header` як і раніше застосовуються до HTTP-upstream'ів.

### Змінні середовища

Усі прапорці CLI приймають відповідну змінну середовища `ROXY_*` як необов'язкове запасне значення. Порядок пріоритету: **CLI > env > default**: прапорець, переданий у командному рядку, завжди перемагає; змінна середовища використовується лише за відсутності прапорця; вбудоване значення за замовчуванням застосовується, тільки якщо не задано ні те, ні інше.

| Прапорець | Env variable | Приклад |
|---|---|---|
| `--transport` | `ROXY_TRANSPORT` | `stdio` \| `http` |
| `--port` | `ROXY_PORT` | `8080` |
| `--upstream` | `ROXY_UPSTREAM` | `http://localhost:8000/handler` |
| `--upstream-entrypoint` | `ROXY_UPSTREAM_ENTRYPOINT` | `/srv/handler.php` |
| `--upstream-insecure` | `ROXY_UPSTREAM_INSECURE` | `true` \| `false` |
| `--upstream-timeout` | `ROXY_UPSTREAM_TIMEOUT` | `30` |
| `--upstream-header` | `ROXY_UPSTREAM_HEADER` | розділені переносами рядків, див. нижче |
| `--pool-size` | `ROXY_POOL_SIZE` | `16` |
| `--log-format` | `ROXY_LOG_FORMAT` | `pretty` \| `json` |

#### Кілька значень upstream-header

`ROXY_UPSTREAM_HEADER` приймає кілька рядків заголовків, розділених символами переносу рядка. Це природно відображається на блочний скаляр Kubernetes YAML:

```yaml
env:
  - name: ROXY_UPSTREAM_HEADER
    value: |-
      Authorization: Bearer xyz
      X-Trace-Id: abc
```

З локального шелу використовуйте лапки `$'...'`, щоб `\n` стало справжнім переносом рядка:

```bash
ROXY_UPSTREAM_HEADER=$'Authorization: Bearer xyz\nX-Trace-Id: abc' \
  roxy --upstream https://api.example.com/mcp
```

Початкові та завершальні порожні рядки відкидаються при запуску, тому особливості блочного скаляра YAML `|-` не породжують некоректних заголовків. Якщо `--upstream-header` переданий у командному рядку, `ROXY_UPSTREAM_HEADER` ігнорується повністю — злиття двох джерел немає.

#### Булеві значення

`ROXY_UPSTREAM_INSECURE` приймає лише **точні рядки в нижньому регістрі** `true` або `false`. Числові форми (`1`, `0`) та інші варіанти написання (`TRUE`, `True`, `YES`, `on`) відхиляються парсером clap (`SetTrue + env`) і призводять до помилки при запуску. Прапорець CLI `--upstream-insecure` (без значення) як і раніше працює і означає `true`.

#### `RUST_LOG`

roxy підтримує стандартну змінну середовища `RUST_LOG`, яка зчитується при запуску через `tracing_subscriber::EnvFilter`; вона не пов'язана зі змінними `ROXY_*` вище і залишається незмінною.

## Написання upstream-обробника

Ваш обробник отримує прості JSON-запити і повертає прості JSON-відповіді. **Він ніколи не бачить JSON-RPC, MCP framing чи стану сесій.** roxy транслює все.

### Для HTTP-бекендів

Підійде будь-який HTTP-сервер, що читає JSON з тіла запиту і пише JSON у відповідь. Приклад на Python/Flask:

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

### Для FastCGI (PHP-FPM) бекендів

Мінімальний PHP-обробник:

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

Повний приклад з кількома інструментами, структурованим виводом, elicitation та посиланнями на ресурси — див. [`examples/handler.php`](../examples/handler.php).

## Довідник upstream-протоколу

Кожен запит від roxy до вашого upstream — це JSON-об’єкт із спільними полями envelope:

```json
{
  "type": "...",
  "session_id": "optional-uuid-or-null",
  "request_id": "uuid-per-request",
  ...
}
```

### Типи запитів

#### `discover`

Надсилається один раз при старті roxy. Ваш обробник має повернути повний каталог підтримуваних інструментів, ресурсів і промптів. roxy кешує результат і віддає його всім MCP-клієнтам без повторних запитів.

```json
// Відповідь
{
  "tools": [
    {
      "name": "tool_name",
      "title": "Human Name",
      "description": "Що робить",
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

Поля `title`, `description`, `mime_type`, `output_schema` необов’язкові.

#### `call_tool`

Виконати інструмент за іменем. Запит:

```json
{
  "type": "call_tool",
  "name": "tool_name",
  "arguments": { "key": "value" },
  "elicitation_results": [ ... ],   // опціонально: див. розділ Elicitation нижче
  "context": { ... }                 // опціонально: ехом з попередньої elicit-відповіді
}
```

Успішна відповідь (звичайний текстовий вивід):

```json
{
  "content": [
    { "type": "text", "text": "результат" }
  ]
}
```

Успішна відповідь зі **структурованим вмістом** (для інструментів із `output_schema`):

```json
{
  "content": [{ "type": "text", "text": "5 + 3 = 8" }],
  "structured_content": { "sum": 8, "operands": { "a": 5, "b": 3 } }
}
```

Успішна відповідь із **посиланням на ресурс**, вбудованим у вивід:

```json
{
  "content": [
    { "type": "text", "text": "Бронювання підтверджено." },
    {
      "type": "resource_link",
      "uri": "myapp://bookings/1234",
      "name": "booking-1234",
      "title": "Бронювання #1234"
    }
  ]
}
```

#### `read_resource`

Прочитати ресурс за URI. Запит:

```json
{ "type": "read_resource", "uri": "myapp://status" }
```

Відповідь: той самий формат `content`, що й у `call_tool`.

#### `get_prompt`

Згенерувати промпт. Запит:

```json
{ "type": "get_prompt", "name": "greet", "arguments": { "name": "Alice" } }
```

Відповідь: той самий формат `content`, що й у `call_tool`.

#### `elicitation_cancelled`

Надсилається, коли MCP-клієнт скасовує elicitation (див. нижче). Ваш обробник може залогувати/прибрати за собою; відповідь ігнорується.

```json
{ "type": "elicitation_cancelled", "name": "tool_name", "action": "decline", "context": {...} }
```

### Elicitation (багатокроковий ввід для інструментів)

Інструмент може **запитати додатковий ввід у користувача** прямо під час виконання. На першому `call_tool` поверніть відповідь `elicit` замість `content`:

```json
{
  "elicit": {
    "message": "Який клас польоту?",
    "schema": {
      "type": "object",
      "properties": {
        "class": { "type": "string", "enum": ["economy", "business", "first"] }
      },
      "required": ["class"]
    },
    "context": { "step": 1, "destination": "Токіо" }
  }
}
```

roxy перешле elicitation MCP-клієнту. Коли користувач заповнить дані, roxy викличе ваш інструмент **знову**, передавши зібрані значення в `elicitation_results` і ваш початковий `context`:

```json
{
  "type": "call_tool",
  "name": "book_flight",
  "arguments": { "destination": "Токіо" },
  "elicitation_results": [{ "class": "business" }],
  "context": { "step": 1, "destination": "Токіо" }
}
```

Можна ланцюжком збирати кілька раундів elicitation, повертаючи черговий `elicit`, поки всі дані не будуть зібрані.

### Відповідь з помилкою

Будь-який тип запиту може повернути помилку замість успіху:

```json
{ "error": { "code": 404, "message": "Unknown tool: foo" } }
```

## Архітектура

```
MCP-клієнт (Claude, Cursor, Zed, ...)
       │
       │ JSON-RPC по stdio або Streamable HTTP
       ▼
┌──────────────┐
│    rmcp      │  Протокол MCP, транспорт, сесії
└──────────────┘
       │
       ▼
┌──────────────┐
│  RoxyServer  │  маршрутизація MCP-методів, кеш можливостей
└──────────────┘
       │
       │ спрощений JSON (UpstreamEnvelope + UpstreamRequest)
       ▼
┌──────────────────────────┐
│    UpstreamExecutor      │  трейт з 2 реалізаціями
│  ┌────────┬───────────┐  │
│  │FastCgi │  Http     │  │
│  └────────┴───────────┘  │
└──────────────────────────┘
       │                │
       ▼                ▼
   PHP-FPM /        HTTP(S)
   будь-який        ендпоінт
   FastCGI
```

### Структура вихідників

```
src/
  main.rs             CLI, логування, запуск транспорту, вибір executor’а
  lib.rs              Корінь бібліотечного крейту (реекспорти для бенчмарків і тестів)
  config.rs           clap Config, UpstreamKind (автовизначення), FcgiAddress
  protocol.rs         Внутрішні JSON-типи (UpstreamEnvelope, UpstreamRequest, ...)
  server.rs           RoxyServer: реалізація rmcp ServerHandler + кеш discover
  executor/
    mod.rs            Трейт UpstreamExecutor
    fastcgi.rs        FastCgiExecutor: deadpool + fastcgi-client
    http.rs           HttpExecutor: reqwest + rustls
examples/
  handler.php         Повний приклад PHP-обробника з усіма можливостями
  echo_upstream.rs    Мінімальний HTTP echo-бекенд для навантажувального тестування
  bench_client.rs     End-to-end навантажувальний клієнт для профілювання
```

### Ключові архітектурні рішення

- **rmcp робить важку роботу.** Офіційний крейт `rmcp` бере на себе всю складність MCP-протоколу (JSON-RPC, узгодження транспорту, керування сесіями). roxy реалізує лише `ServerHandler`.
- **Upstream підключаємий.** Трейт `UpstreamExecutor` абстрагує комунікацію з бекендом. FastCGI та HTTP — поточні реалізації; додати новий бекенд (gRPC, stdio, WebSocket) = реалізувати один трейт.
- **Можливості кешуються.** roxy викликає `discover` один раз при старті та тримає tools/resources/prompts у пам’яті. MCP-клієнти отримують миттєві відповіді на `initialize`, не чіпаючи upstream.
- **Пулінг з’єднань для FastCGI.** `deadpool` тримає з’єднання з PHP-FPM теплими, уникаючи налаштування сокета на кожен запит.
- **Pure-Rust TLS через rustls.** Без OpenSSL, без системних бібліотек. Повністю статичні Linux-збірки, проста крос-компіляція, переносимі бінари.
- **Upstream лишається простим.** Ваш обробник ніколи не бачить JSON-RPC, request ID (окрім як в непрозорому полі envelope), стану сесії чи MCP framing. Простий JSON на вході, простий JSON на виході.

## Розробка

### Збірка та тести

```bash
cargo build           # debug
cargo build --release # оптимізована збірка
cargo test            # прогін тестів
cargo clippy          # лінтер
cargo fmt             # форматування
```

### Локальний запуск з прикладом PHP-обробника

```bash
# Термінал 1: запускаємо PHP-FPM
php-fpm --nodaemonize -d "listen=127.0.0.1:9000" -d "pm=static" -d "pm.max_children=4"

# Термінал 2: запускаємо roxy з прикладом обробника
cargo run -- \
    --upstream 127.0.0.1:9000 \
    --upstream-entrypoint "$(pwd)/examples/handler.php"
```

Потім підключайтеся будь-яким MCP-клієнтом або надсилайте JSON-RPC вручну через stdio.

### Процес релізу

Теговані релізи (`git tag vX.Y.Z && git push origin vX.Y.Z`) запускають `.github/workflows/release.yml`, який:

1. Збирає release-бінари для всіх чотирьох таргетів (macOS arm64/x86_64, Linux musl arm64/x86_64)
2. Пакує їх у `.tar.gz` з SHA256-хешами
3. Збирає `.deb` і `.rpm` пакети для обох Linux-архітектур
4. Публікує GitHub Release з усіма артефактами
5. Оновлює формулу Homebrew у [`petstack/homebrew-tap`](https://github.com/petstack/homebrew-tap) (якщо задано секрет `HOMEBREW_TAP_TOKEN`)

Налаштування tap — див. [`packaging/homebrew/README.md`](../packaging/homebrew/README.md).

## Ліцензія

[AGPL-3.0-only](../LICENSE). Якщо ви запускаєте змінену версію roxy як мережевий сервіс, ви зобов’язані надати свої зміни користувачам цього сервісу.
