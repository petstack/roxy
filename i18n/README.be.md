# roxy

---

[English](../README.md) · [Русский](README.ru.md) · [Українська](README.uk.md) · **Беларуская** · [Polski](README.pl.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · [Español](README.es.md) · [中文](README.zh-CN.md) · [日本語](README.ja.md)

---

**Высокапрадукцыйны проксі-сервер MCP (Model Context Protocol) на Rust.**

roxy злучае MCP-кліентаў (Claude Desktop, Cursor, Zed і іншыя) з любым upstream-апрацоўшчыкам, які працуе як **FastCGI**-бэкэнд (напрыклад, PHP-FPM) або **HTTP(S)**-эндпоінт. Rust бярэ на сябе ўсё, што крытычна для прадукцыйнасці — транспарт, парсінг пратакола, пулінг злучэнняў, канкурэнтнасць — праз афіцыйны крэйт [`rmcp`](https://crates.io/crates/rmcp). Ваш апрацоўшчык працуе з простым, спрошчаным JSON-пратаколам і вяртае вынікі.

Гэта дазваляе пісаць MCP-серверы на **любой мове** — PHP, Python, Node, Go, Ruby — не пераадкрываючы кожны раз JSON-RPC framing, транспарт, кіраванне сесіямі і ўзгадненне магчымасцяў.

## Змест

- [Магчымасці](#магчымасці)
- [Усталяванне](#усталяванне)
  - [Homebrew (macOS і Linux)](#homebrew-macos-і-linux)
  - [Скрыпт усталявання (любы Unix)](#скрыпт-усталявання-любы-unix)
  - [Debian / Ubuntu (.deb)](#debian--ubuntu-deb)
  - [Fedora / RHEL / openSUSE (.rpm)](#fedora--rhel--opensuse-rpm)
  - [Alpine / любы Linux (статычны tarball)](#alpine--любы-linux-статычны-tarball)
  - [З зыходнікаў](#з-зыходнікаў)
  - [Праверка ўсталявання](#праверка-ўсталявання)
- [Хуткі старт](#хуткі-старт)
- [Даведнік CLI](#даведнік-cli)
  - [Пераменныя асяроддзя](#пераменныя-асяроддзя)
- [Напісанне upstream-апрацоўшчыка](#напісанне-upstream-апрацоўшчыка)
- [Даведнік upstream-пратакола](#даведнік-upstream-пратакола)
  - [Тыпы запытаў](#тыпы-запытаў)
  - [Elicitation (шматкрокавы ўвод для інструментаў)](#elicitation-шматкрокавы-ўвод-для-інструментаў)
  - [Адказ з памылкай](#адказ-з-памылкай)
- [Архітэктура](#архітэктура)
- [Распрацоўка](#распрацоўка)
- [Ліцэнзія](#ліцэнзія)

## Магчымасці

- **Мульці-бэкэнд**: FastCGI (TCP або Unix socket) і HTTP(S) upstream’ы, аўтавызначэнне па фармаце URL
- **Транспарты**: stdio і Streamable HTTP, абодва падтрымліваюцца нативна праз `rmcp`
- **Магчымасці MCP 2025-06-18**: elicitation (шматкрокавы ўвод), структураваны вывад інструментаў, спасылкі на рэсурсы ў адказах
- **Пулінг злучэнняў** для FastCGI (праз `deadpool`)
- **TLS праз rustls** — без залежнасці ад OpenSSL, цалкам статычныя musl-зборкі
- **Кешаванне магчымасцяў** — tools/resources/prompts апытваюцца адзін раз пры старце
- **Адвольныя HTTP-загалоўкі**, настройвальныя таймаўты, перадача request/session ID у upstream

## Усталяванне

Папярэдне сабраныя бінарныя файлы публікуюцца на кожны тэгаваны рэліз для **macOS (arm64, x86_64)** і **Linux (arm64, x86_64, статыка musl)**. Выбірайце спосаб пад сваю платформу.

### Homebrew (macOS і Linux)

```bash
brew tap petstack/tap
brew install roxy
```

Працуе на macOS (Intel і Apple Silicon) і Linux (x86_64 і arm64) з усталяваным Homebrew.

### Скрыпт усталявання (любы Unix)

```bash
curl -sSfL https://raw.githubusercontent.com/petstack/roxy/main/install.sh | sh
```

Скрыпт аўтаматычна вызначае вашу АС і архітэктуру, сцягвае патрэбны tarball з GitHub Releases, правярае кантрольную суму SHA256 і ўсталёўвае бінар у `/usr/local/bin/roxy` (пры неабходнасці праз `sudo`).

Опцыі:

```bash
# Усталяваць канкрэтную версію
curl -sSfL .../install.sh | sh -s -- --version v0.1.0

# Усталяваць у сваю дырэкторыю (sudo не патрабуецца)
curl -sSfL .../install.sh | sh -s -- --bin-dir $HOME/.local/bin
```

Таксама працуюць зменныя асяроддзя `ROXY_REPO`, `ROXY_VERSION`, `ROXY_BIN_DIR`.

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

### Alpine / любы Linux (статычны tarball)

Linux-бінары статычна злінкаваныя з musl libc, таму працуюць на **любым** Linux-дыстрыбутыве без залежнасцяў:

```bash
# Выберыце сваю архітэктуру
ARCH=x86_64   # або aarch64
curl -sSfL https://github.com/petstack/roxy/releases/latest/download/roxy-v0.1.0-${ARCH}-unknown-linux-musl.tar.gz | tar -xz
sudo install -m 755 roxy-v0.1.0-${ARCH}-unknown-linux-musl/roxy /usr/local/bin/
```

Працуе на Alpine, Debian, Ubuntu, RHEL, Arch, Amazon Linux, Void, NixOS і ўсім астатнім з Linux-ядром.

### З зыходнікаў

Патрабуецца [Rust](https://rustup.rs/) (edition 2024, stable toolchain):

```bash
git clone https://github.com/petstack/roxy
cd roxy
cargo build --release
# Бінар будзе ў ./target/release/roxy
```

Або праз `cargo install`:

```bash
cargo install --git https://github.com/petstack/roxy
```

### Праверка ўсталявання

```bash
roxy --version
roxy --help
```

## Хуткі старт

roxy патрабуе **адзін аргумент**: `--upstream`, які ўказвае на ваш апрацоўшчык. Тып upstream **вызначаецца аўтаматычна** па фармаце URL:

| Фармат URL | Тып бэкэнда |
|---|---|
| `http://...` або `https://...` | HTTP(S) |
| `host:port` | FastCGI TCP |
| `/шлях/да/сокета` | FastCGI Unix socket |

### Прыклад: HTTP-бэкэнд

```bash
# Запусціце свой HTTP-апрацоўшчык на порце 8000 (любая мова, любы фрэймворк)
# Затым накіруйце на яго roxy:
roxy --upstream http://localhost:8000/mcp
```

### Прыклад: PHP-FPM бэкэнд

```bash
# Запуск PHP-FPM
php-fpm --nodaemonize \
    -d "listen=127.0.0.1:9000" \
    -d "pm=static" \
    -d "pm.max_children=4"

# Накіроўваем roxy
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /absolute/path/to/handler.php
```

### Падключэнне з MCP-кліента

Для Claude Desktop або любога кліента, які запускае MCP-серверы як падпрацэсы (транспарт stdio — па змаўчанні):

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

Для сеткавых кліентаў, якія падключаюцца па Streamable HTTP:

```bash
roxy --transport http --port 8080 --upstream http://localhost:8000/mcp
# Кліент падключаецца да http://localhost:8080/mcp
```

## Даведнік CLI

```
roxy [OPTIONS] --upstream <UPSTREAM>
```

| Сцяг | Па змаўчанні | Апісанне |
|---|---|---|
| `--upstream <URL>` | **абавязковы** | URL бэкэнда. Тып вызначаецца аўтаматычна (гл. табліцу вышэй) |
| `--transport <MODE>` | `stdio` | Транспарт MCP-кліента: `stdio` або `http` |
| `--port <PORT>` | `8080` | Порт HTTP-сервера (толькі пры `--transport http`) |
| `--upstream-entrypoint <PATH>` | — | `SCRIPT_FILENAME`, адпраўляемы ў FastCGI-бэкэнд (абавязковы для PHP-FPM) |
| `--upstream-insecure` | `false` | Прапускаць праверку TLS-сертыфікатаў для HTTPS-upstream’аў |
| `--upstream-timeout <SECS>` | `30` | Таймаўт HTTP-upstream запыту ў секундах |
| `--upstream-header <HEADER>` | — | Статычны HTTP-загаловак, які дадаецца да кожнага запыту да HTTP-upstream, `Name: Value`. Можна ўказваць некалькі разоў. Толькі для HTTP-upstream — ігнаруецца для FastCGI |
| `--pool-size <N>` | `16` | Памер пула злучэнняў FastCGI |
| `--log-format <FORMAT>` | `pretty` | Фармат логаў: `pretty` або `json` |

**Узровень** лагіравання кіруецца праз зменную асяроддзя `RUST_LOG`:

```bash
RUST_LOG=debug roxy --upstream http://localhost:8000/mcp
RUST_LOG=roxy=debug,rmcp=info roxy --upstream ...  # фільтры па модулях
```

### Поўны прыклад HTTP-бэкэнда

```bash
roxy --upstream https://api.example.com/mcp \
     --upstream-header "Authorization: Bearer $TOKEN" \
     --upstream-header "X-Tenant: acme" \
     --upstream-timeout 60
```

### Поўны прыклад FastCGI (PHP-FPM)

```bash
# TCP
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /srv/app/handler.php

# Unix socket
roxy --upstream /var/run/php-fpm.sock --upstream-entrypoint /srv/app/handler.php

# Транспарт HTTP з FastCGI-upstream
roxy --transport http --port 8080 \
     --upstream 127.0.0.1:9000 \
     --upstream-entrypoint /srv/app/handler.php
```

### Перасылка загалоўкаў кліента

Пры `--transport http` кожны ўваходны загаловак MCP-кліента аўтаматычна перасылаецца да upstream-бэкэнда — ніякай дадатковай налады не патрабуецца. Агульныя загалоўкі (RFC 7230 §6.1: `Connection`, `Keep-Alive`, `Proxy-Authenticate`, `Proxy-Authorization`, `TE`, `Trailer`, `Transfer-Encoding`, `Upgrade`) і загалоўкі, якімі кіруе сам roxy (`Host`, `Content-Type`, `Content-Length`), адфільтроўваюцца. Усё астатняе — `Authorization`, `Cookie`, `X-Forwarded-For`, карыстальніцкія загалоўкі `X-*`, `mcp-session-id` — трапляе да upstream без змяненняў. Гэта адпавядае стандартным паводзінам nginx `fastcgi_pass` / `proxy_pass` і дазваляе вашаму upstream-бэкэнду аўтэнтыфікаваць канчатковага кліента (правяраць bearer-токены, аналізаваць сесійныя кукі) без таго, каб roxy трэба было разбірацца ў схеме аўтэнтыфікацыі.

| Upstream | Форма перадачы |
|---|---|
| HTTP | Перасылаецца ў выглядзе сапраўдных HTTP-загалоўкаў запыту. Загалоўкі з некалькімі значэннямі (напрыклад, два запісы `X-Forwarded-For`) захоўваюцца. |
| FastCGI | Пераўтвараецца ў CGI-параметры `HTTP_*` згодна з RFC 3875 §4.1.18 — PHP-апрацоўшчыкі чытаюць іх праз `$_SERVER['HTTP_AUTHORIZATION']`, `$_SERVER['HTTP_X_FORWARDED_FOR']` і г.д. Загалоўкі з некалькімі значэннямі аб'ядноўваюцца праз `", "`, каб адпавядаць семантыцы `$http_*` у nginx. |

`--upstream-header` працуе, як і раней, для HTTP-upstream — ён забяспечвае roxy ўласнымі статычнымі ідэнтыфікатарамі для upstream (сэрвісны токен, фіксаваны `X-Client-Id` і г.д.). Калі перасланы ад кліента загаловак супадае па назве са статычным загалоўкам `--upstream-header`, перасланае значэнне **перамагае**: ідэнтыфікатар кліента на ўзроўні запыту больш канкрэтны, чым стандартнае значэнне roxy — гэта адпавядае тыповым паводзінам зваротнага проксі. `--upstream-header` у цяперашні час не дзейнічае для FastCGI-upstream — выкарыстоўвайце аўтаматычную перасылку.

Пры `--transport stdio` ўваходнага HTTP-запыту няма, таму ніякія загалоўкі не перасылаюцца; статычныя запісы `--upstream-header` па-ранейшаму прымяняюцца да HTTP-upstream у звычайным рэжыме.

### Пераменныя асяроддзя

Усе сцягі CLI прымаюць адпаведную зменную асяроддзя `ROXY_*` як неабавязковае запаснае значэнне. Парадак прыярытэту: **CLI > env > default**: сцяг, перададзены ў камандным радку, заўжды перамагае; зменная асяроддзя выкарыстоўваецца толькі пры адсутнасці сцяга; убудаванае значэнне па змаўчанні ўжываецца, толькі калі не зададзена ні тое, ні другое.

| Сцяг | Env variable | Прыклад |
|---|---|---|
| `--transport` | `ROXY_TRANSPORT` | `stdio` \| `http` |
| `--port` | `ROXY_PORT` | `8080` |
| `--upstream` | `ROXY_UPSTREAM` | `http://localhost:8000/handler` |
| `--upstream-entrypoint` | `ROXY_UPSTREAM_ENTRYPOINT` | `/srv/handler.php` |
| `--upstream-insecure` | `ROXY_UPSTREAM_INSECURE` | `true` \| `false` |
| `--upstream-timeout` | `ROXY_UPSTREAM_TIMEOUT` | `30` |
| `--upstream-header` | `ROXY_UPSTREAM_HEADER` | падзеленыя пераносамі радкоў, гл. ніжэй |
| `--pool-size` | `ROXY_POOL_SIZE` | `16` |
| `--log-format` | `ROXY_LOG_FORMAT` | `pretty` \| `json` |

#### Некалькі значэнняў upstream-header

`ROXY_UPSTREAM_HEADER` прымае некалькі радкоў загалоўкаў, падзеленых сімваламі пераносу радка. Гэта натуральна адлюстроўваецца на блочны скаляр Kubernetes YAML:

```yaml
env:
  - name: ROXY_UPSTREAM_HEADER
    value: |-
      Authorization: Bearer xyz
      X-Trace-Id: abc
```

З лакальнага шэла выкарыстоўвайце двукоссе `$'...'`, каб `\n` стала сапраўдным пераносам радка:

```bash
ROXY_UPSTREAM_HEADER=$'Authorization: Bearer xyz\nX-Trace-Id: abc' \
  roxy --upstream https://api.example.com/mcp
```

Пачатковыя і завяршальныя пустыя радкі адкідаюцца пры запуску, таму асаблівасці блочнага скаляра YAML `|-` не ствараюць некарэктных загалоўкаў. Калі `--upstream-header` перададзены ў камандным радку, `ROXY_UPSTREAM_HEADER` ігнаруецца цалкам — зліцця двух крыніц няма.

#### Булевы значэнні

`ROXY_UPSTREAM_INSECURE` прымае толькі **дакладныя радкі ў ніжнім рэгістры** `true` або `false`. Лічбавыя формы (`1`, `0`) і іншыя варыянты напісання (`TRUE`, `True`, `YES`, `on`) адхіляюцца парсерам clap (`SetTrue + env`) і прыводзяць да памылкі пры запуску. Сцяг CLI `--upstream-insecure` (без значэння) як і раней працуе і азначае `true`.

#### `RUST_LOG`

roxy падтрымлівае стандартную зменную асяроддзя `RUST_LOG`, якая счытваецца пры запуску праз `tracing_subscriber::EnvFilter`; яна не звязана са зменнымі `ROXY_*` вышэй і застаецца нязменнай.

## Напісанне upstream-апрацоўшчыка

Ваш апрацоўшчык атрымлівае простыя JSON-запыты і вяртае простыя JSON-адказы. **Ён ніколі не бачыць JSON-RPC, MCP framing або стан сесій.** roxy транслюе ўсё.

### Для HTTP-бэкэндаў

Падыдзе любы HTTP-сервер, які чытае JSON з цела запыту і піша JSON у адказ. Прыклад на Python/Flask:

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

### Для FastCGI (PHP-FPM) бэкэндаў

Мінімальны PHP-апрацоўшчык:

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

Поўны прыклад з некалькімі інструментамі, структураваным вывадам, elicitation і спасылкамі на рэсурсы — гл. [`examples/handler.php`](../examples/handler.php).

## Даведнік upstream-пратакола

Кожны запыт ад roxy да вашага upstream — гэта JSON-аб’ект з агульнымі палямі envelope:

```json
{
  "type": "...",
  "session_id": "optional-uuid-or-null",
  "request_id": "uuid-per-request",
  ...
}
```

### Тыпы запытаў

#### `discover`

Адпраўляецца адзін раз пры старце roxy. Ваш апрацоўшчык павінен вярнуць поўны каталог падтрымліваных інструментаў, рэсурсаў і промптаў. roxy кешуе вынік і аддае яго ўсім MCP-кліентам без паўторных запытаў.

```json
// Адказ
{
  "tools": [
    {
      "name": "tool_name",
      "title": "Human Name",
      "description": "Што робіць",
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

Палі `title`, `description`, `mime_type`, `output_schema` неабавязковыя.

#### `call_tool`

Выканаць інструмент па імені. Запыт:

```json
{
  "type": "call_tool",
  "name": "tool_name",
  "arguments": { "key": "value" },
  "elicitation_results": [ ... ],   // неабавязкова: гл. раздзел Elicitation ніжэй
  "context": { ... }                 // неабавязкова: рэхам з папярэдняга elicit-адказу
}
```

Паспяховы адказ (звычайны тэкставы вывад):

```json
{
  "content": [
    { "type": "text", "text": "вынік" }
  ]
}
```

Паспяховы адказ са **структураваным змесцівам** (для інструментаў з `output_schema`):

```json
{
  "content": [{ "type": "text", "text": "5 + 3 = 8" }],
  "structured_content": { "sum": 8, "operands": { "a": 5, "b": 3 } }
}
```

Паспяховы адказ са **спасылкай на рэсурс**, убудаванай у вывад:

```json
{
  "content": [
    { "type": "text", "text": "Браніраванне пацверджана." },
    {
      "type": "resource_link",
      "uri": "myapp://bookings/1234",
      "name": "booking-1234",
      "title": "Браніраванне #1234"
    }
  ]
}
```

#### `read_resource`

Прачытаць рэсурс па URI. Запыт:

```json
{ "type": "read_resource", "uri": "myapp://status" }
```

Адказ: той жа фармат `content`, што і ў `call_tool`.

#### `get_prompt`

Згенераваць промпт. Запыт:

```json
{ "type": "get_prompt", "name": "greet", "arguments": { "name": "Alice" } }
```

Адказ: той жа фармат `content`, што і ў `call_tool`.

#### `elicitation_cancelled`

Адпраўляецца, калі MCP-кліент адмяняе elicitation (гл. ніжэй). Ваш апрацоўшчык можа залагаваць/прыбраць; адказ ігнаруецца.

```json
{ "type": "elicitation_cancelled", "name": "tool_name", "action": "decline", "context": {...} }
```

### Elicitation (шматкрокавы ўвод для інструментаў)

Інструмент можа **запытаць дадатковы ўвод у карыстальніка** прама падчас выканання. На першым `call_tool` вярніце адказ `elicit` замест `content`:

```json
{
  "elicit": {
    "message": "Які клас палёту?",
    "schema": {
      "type": "object",
      "properties": {
        "class": { "type": "string", "enum": ["economy", "business", "first"] }
      },
      "required": ["class"]
    },
    "context": { "step": 1, "destination": "Токіа" }
  }
}
```

roxy перашле elicitation MCP-кліенту. Калі карыстальнік запоўніць дадзеныя, roxy выкліча ваш інструмент **зноў**, перадаўшы сабраныя значэнні ў `elicitation_results` і ваш пачатковы `context`:

```json
{
  "type": "call_tool",
  "name": "book_flight",
  "arguments": { "destination": "Токіа" },
  "elicitation_results": [{ "class": "business" }],
  "context": { "step": 1, "destination": "Токіа" }
}
```

Можна ланцужком збіраць некалькі раундаў elicitation, вяртаючы чарговы `elicit`, пакуль усе дадзеныя не будуць сабраныя.

### Адказ з памылкай

Любы тып запыту можа вярнуць памылку замест поспеху:

```json
{ "error": { "code": 404, "message": "Unknown tool: foo" } }
```

## Архітэктура

```
MCP-кліент (Claude, Cursor, Zed, ...)
       │
       │ JSON-RPC па stdio або Streamable HTTP
       ▼
┌──────────────┐
│    rmcp      │  Пратакол MCP, транспарт, сесіі
└──────────────┘
       │
       ▼
┌──────────────┐
│  RoxyServer  │  маршрутызацыя MCP-метадаў, кэш магчымасцяў
└──────────────┘
       │
       │ спрошчаны JSON (UpstreamEnvelope + UpstreamRequest)
       ▼
┌──────────────────────────┐
│    UpstreamExecutor      │  трэйт з 2 рэалізацыямі
│  ┌────────┬───────────┐  │
│  │FastCgi │  Http     │  │
│  └────────┴───────────┘  │
└──────────────────────────┘
       │                │
       ▼                ▼
   PHP-FPM /        HTTP(S)
   любы FastCGI     эндпоінт
```

### Структура зыходнікаў

```
src/
  main.rs             CLI, лагіраванне, запуск транспарту, выбар executor’а
  lib.rs              Корань бібліятэчнага крэйту (рээкспарты для бенчмаркаў і тэстаў)
  config.rs           clap Config, UpstreamKind (аўтавызначэнне), FcgiAddress
  protocol.rs         Унутраныя JSON-тыпы (UpstreamEnvelope, UpstreamRequest, ...)
  server.rs           RoxyServer: рэалізацыя rmcp ServerHandler + кэш discover
  executor/
    mod.rs            Трэйт UpstreamExecutor
    fastcgi.rs        FastCgiExecutor: deadpool + fastcgi-client
    http.rs           HttpExecutor: reqwest + rustls
examples/
  handler.php         Поўны прыклад PHP-апрацоўшчыка з усімі магчымасцямі
  echo_upstream.rs    Мінімальны HTTP echo-бэкенд для нагрузачнага тэсціравання
  bench_client.rs     End-to-end нагрузачны кліент для прафіліравання
```

### Ключавыя архітэктурныя рашэнні

- **rmcp робіць цяжкую працу.** Афіцыйны крэйт `rmcp` бярэ на сябе ўсю складанасць MCP-пратакола (JSON-RPC, узгадненне транспарту, кіраванне сесіямі). roxy рэалізуе толькі `ServerHandler`.
- **Upstream падключальны.** Трэйт `UpstreamExecutor` абстрагуе камунікацыю з бэкэндам. FastCGI і HTTP — бягучыя рэалізацыі; дадаць новы бэкэнд (gRPC, stdio, WebSocket) = рэалізаваць адзін трэйт.
- **Магчымасці кешуюцца.** roxy выклікае `discover` адзін раз пры старце і трымае tools/resources/prompts у памяці. MCP-кліенты атрымліваюць імгненныя адказы на `initialize`, не крануўшы upstream.
- **Пулінг злучэнняў для FastCGI.** `deadpool` трымае злучэнні з PHP-FPM цёплымі, пазбягаючы настройкі сокета на кожны запыт.
- **Pure-Rust TLS праз rustls.** Без OpenSSL, без сістэмных бібліятэк. Цалкам статычныя Linux-зборкі, простая крос-кампіляцыя, пераносныя бінары.
- **Upstream застаецца простым.** Ваш апрацоўшчык ніколі не бачыць JSON-RPC, request ID (акрамя як у непразрыстым полі envelope), стан сесіі або MCP framing. Просты JSON на ўваходзе, просты JSON на выхадзе.

## Распрацоўка

### Зборка і тэсты

```bash
cargo build           # debug
cargo build --release # аптымізаваная зборка
cargo test            # прагон тэстаў
cargo clippy          # лінтар
cargo fmt             # фарматаванне
```

### Лакальны запуск з прыкладам PHP-апрацоўшчыка

```bash
# Тэрмінал 1: запускаем PHP-FPM
php-fpm --nodaemonize -d "listen=127.0.0.1:9000" -d "pm=static" -d "pm.max_children=4"

# Тэрмінал 2: запускаем roxy з прыкладам апрацоўшчыка
cargo run -- \
    --upstream 127.0.0.1:9000 \
    --upstream-entrypoint "$(pwd)/examples/handler.php"
```

Затым падключайцеся любым MCP-кліентам або адпраўляйце JSON-RPC уручную праз stdio.

### Працэс рэлізу

Тэгаваныя рэлізы (`git tag vX.Y.Z && git push origin vX.Y.Z`) запускаюць `.github/workflows/release.yml`, які:

1. Збірае release-бінары для ўсіх чатырох таргетаў (macOS arm64/x86_64, Linux musl arm64/x86_64)
2. Пакуе іх у `.tar.gz` з SHA256-хэшамі
3. Збірае `.deb` і `.rpm` пакеты для абедзвюх Linux-архітэктур
4. Публікуе GitHub Release з усімі артэфактамі
5. Абнаўляе формулу Homebrew у [`petstack/homebrew-tap`](https://github.com/petstack/homebrew-tap) (калі зададзены сакрэт `HOMEBREW_TAP_TOKEN`)

Наладка tap — гл. [`packaging/homebrew/README.md`](../packaging/homebrew/README.md).

## Ліцэнзія

[AGPL-3.0-only](../LICENSE). Калі вы запускаеце змененую версію roxy як сеткавы сэрвіс, вы абавязаны даць свае змены карыстальнікам гэтага сэрвісу.
