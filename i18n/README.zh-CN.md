# roxy

---

[English](../README.md) · [Русский](README.ru.md) · [Українська](README.uk.md) · [Беларуская](README.be.md) · [Polski](README.pl.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · [Español](README.es.md) · **中文** · [日本語](README.ja.md)

---

**用 Rust 编写的高性能 MCP (Model Context Protocol) 代理服务器。**

roxy 将 MCP 客户端(Claude Desktop、Cursor、Zed 等)桥接到任何上游处理器,该处理器可以作为 **FastCGI** 后端(例如 PHP-FPM)或 **HTTP(S)** 端点运行。Rust 通过官方 [`rmcp`](https://crates.io/crates/rmcp) crate 处理所有对性能敏感的部分——传输、协议解析、连接池、并发。你的处理器只需处理一个小而简化的 JSON 协议,并返回结果。

这让你可以用**任何语言**编写 MCP 服务器——PHP、Python、Node、Go、Ruby——而无需每次都重新实现 JSON-RPC 封帧、传输、会话管理和能力协商。

## 目录

- [特性](#特性)
- [安装](#安装)
  - [Homebrew (macOS 和 Linux)](#homebrew-macos-和-linux)
  - [安装脚本(任何 Unix)](#安装脚本任何-unix)
  - [Debian / Ubuntu (.deb)](#debian--ubuntu-deb)
  - [Fedora / RHEL / openSUSE (.rpm)](#fedora--rhel--opensuse-rpm)
  - [Alpine / 任何 Linux(静态 tarball)](#alpine--任何-linux静态-tarball)
  - [从源码构建](#从源码构建)
  - [验证安装](#验证安装)
- [快速开始](#快速开始)
- [CLI 参考](#cli-参考)
  - [环境变量](#环境变量)
- [编写上游处理器](#编写上游处理器)
- [上游协议参考](#上游协议参考)
  - [请求类型](#请求类型)
  - [Elicitation(多轮工具输入)](#elicitation多轮工具输入)
  - [错误响应](#错误响应)
- [架构](#架构)
- [开发](#开发)
- [许可证](#许可证)

## 特性

- **多后端**:FastCGI(TCP 或 Unix socket)和 HTTP(S) 上游,根据 URL 格式自动检测
- **传输**:stdio 和 Streamable HTTP,都通过 `rmcp` 原生支持
- **MCP 2025-06-18 特性**:elicitation(多轮工具输入)、结构化工具输出、工具响应中的资源链接
- **连接池**(FastCGI,通过 `deadpool`)
- **通过 rustls 的 TLS** — 无 OpenSSL 依赖,完全静态的 musl 构建
- **能力缓存** — tools/resources/prompts 在启动时一次性发现
- **自定义 HTTP 头**、可配置的超时、向上游传递请求/会话 ID

## 安装

每次打标签的发布都会为 **macOS(arm64、x86_64)**和 **Linux(arm64、x86_64、musl 静态)**发布预编译的二进制文件。根据你的平台选择合适的方式。

### Homebrew (macOS 和 Linux)

```bash
brew tap petstack/tap
brew install roxy
```

适用于安装了 Homebrew 的 macOS(Intel 和 Apple Silicon)和 Linux(x86_64 和 arm64)。

### 安装脚本(任何 Unix)

```bash
curl -sSfL https://raw.githubusercontent.com/petstack/roxy/main/install.sh | sh
```

脚本会自动检测你的操作系统和架构,从 GitHub Releases 下载正确的 tarball,验证 SHA256 校验和,并安装到 `/usr/local/bin/roxy`(必要时使用 `sudo`)。

选项:

```bash
# 安装特定版本
curl -sSfL .../install.sh | sh -s -- --version v0.1.0

# 安装到自定义目录(不需要 sudo)
curl -sSfL .../install.sh | sh -s -- --bin-dir $HOME/.local/bin
```

环境变量 `ROXY_REPO`、`ROXY_VERSION`、`ROXY_BIN_DIR` 也可用。

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

### Alpine / 任何 Linux(静态 tarball)

Linux 二进制文件与 musl libc 静态链接,因此可以在**任何** Linux 发行版上运行,无需依赖:

```bash
# 选择你的架构
ARCH=x86_64   # 或 aarch64
curl -sSfL https://github.com/petstack/roxy/releases/latest/download/roxy-v0.1.0-${ARCH}-unknown-linux-musl.tar.gz | tar -xz
sudo install -m 755 roxy-v0.1.0-${ARCH}-unknown-linux-musl/roxy /usr/local/bin/
```

适用于 Alpine、Debian、Ubuntu、RHEL、Arch、Amazon Linux、Void、NixOS 以及任何其他 Linux 内核系统。

### 从源码构建

需要 [Rust](https://rustup.rs/)(edition 2024,stable toolchain):

```bash
git clone https://github.com/petstack/roxy
cd roxy
cargo build --release
# 二进制文件在 ./target/release/roxy
```

或通过 `cargo install`:

```bash
cargo install --git https://github.com/petstack/roxy
```

### 验证安装

```bash
roxy --version
roxy --help
```

## 快速开始

roxy 只需**一个参数**:`--upstream`,指向你的处理器。上游类型根据 URL 格式**自动检测**:

| URL 格式 | 后端类型 |
|---|---|
| `http://...` 或 `https://...` | HTTP(S) |
| `host:port` | FastCGI TCP |
| `/path/to/socket` | FastCGI Unix socket |

### 示例:HTTP 后端

```bash
# 在 8000 端口启动你的 HTTP 处理器(任何语言、任何框架)
# 然后让 roxy 指向它:
roxy --upstream http://localhost:8000/mcp
```

### 示例:PHP-FPM 后端

```bash
# 启动 PHP-FPM
php-fpm --nodaemonize \
    -d "listen=127.0.0.1:9000" \
    -d "pm=static" \
    -d "pm.max_children=4"

# 让 roxy 指向它
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /absolute/path/to/handler.php
```

### 从 MCP 客户端连接

对于 Claude Desktop 或任何以子进程方式启动 MCP 服务器的客户端(stdio 传输 —— 默认):

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

对于通过 Streamable HTTP 连接的网络客户端:

```bash
roxy --transport http --port 8080 --upstream http://localhost:8000/mcp
# 客户端连接到 http://localhost:8080/mcp
```

## CLI 参考

```
roxy [OPTIONS] --upstream <UPSTREAM>
```

| 标志 | 默认值 | 描述 |
|---|---|---|
| `--upstream <URL>` | **必需** | 后端 URL。自动检测类型(见上表) |
| `--transport <MODE>` | `stdio` | MCP 客户端传输:`stdio` 或 `http` |
| `--port <PORT>` | `8080` | HTTP 监听端口(仅与 `--transport http` 一起使用) |
| `--upstream-entrypoint <PATH>` | — | 发送给 FastCGI 后端的 `SCRIPT_FILENAME`(PHP-FPM 必需) |
| `--upstream-insecure` | `false` | 跳过 HTTPS 上游的 TLS 证书验证 |
| `--upstream-timeout <SECS>` | `30` | HTTP 上游请求超时(秒) |
| `--upstream-header <HEADER>` | — | 附加到每个 HTTP 上游请求的静态 HTTP 头,`Name: Value`。可重复。仅适用于 HTTP 上游——对 FastCGI 无效 |
| `--pool-size <N>` | `16` | FastCGI 连接池大小 |
| `--log-format <FORMAT>` | `pretty` | 日志格式:`pretty` 或 `json` |

日志**级别**通过 `RUST_LOG` 环境变量控制:

```bash
RUST_LOG=debug roxy --upstream http://localhost:8000/mcp
RUST_LOG=roxy=debug,rmcp=info roxy --upstream ...  # 按模块过滤
```

### 完整 HTTP 后端示例

```bash
roxy --upstream https://api.example.com/mcp \
     --upstream-header "Authorization: Bearer $TOKEN" \
     --upstream-header "X-Tenant: acme" \
     --upstream-timeout 60
```

### 完整 FastCGI (PHP-FPM) 示例

```bash
# TCP
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /srv/app/handler.php

# Unix socket
roxy --upstream /var/run/php-fpm.sock --upstream-entrypoint /srv/app/handler.php

# HTTP 传输配合 FastCGI 上游
roxy --transport http --port 8080 \
     --upstream 127.0.0.1:9000 \
     --upstream-entrypoint /srv/app/handler.php
```

### 转发客户端请求头

在 `--transport http` 模式下，每个来自 MCP 客户端的请求头都会自动转发到上游后端——无需任何配置。逐跳头部（RFC 7230 §6.1：`Connection`、`Keep-Alive`、`Proxy-Authenticate`、`Proxy-Authorization`、`TE`、`Trailer`、`Transfer-Encoding`、`Upgrade`）以及 roxy 自身管理的头部（`Host`、`Content-Type`、`Content-Length`）会被过滤掉。其余所有头部——`Authorization`、`Cookie`、`X-Forwarded-For`、自定义 `X-*` 头部、`mcp-session-id`——均原样传达上游。这与 nginx `fastcgi_pass` / `proxy_pass` 的默认行为一致，目的是让你的上游后端能够对终端客户端进行认证（校验 bearer token、检查 session cookie），而无需 roxy 理解具体的认证方案。

| 上游类型 | 转发形式 |
|---|---|
| HTTP | 作为真实 HTTP 请求头转发。多值头部（例如两个 `X-Forwarded-For` 条目）会被完整保留。 |
| FastCGI | 按照 RFC 3875 §4.1.18 转换为 CGI `HTTP_*` 参数——PHP 处理器从 `$_SERVER['HTTP_AUTHORIZATION']`、`$_SERVER['HTTP_X_FORWARDED_FOR']` 等读取。多值头部以 `", "` 连接，与 nginx `$http_*` 语义保持一致。 |

`--upstream-header` 对 HTTP 上游的工作方式与以前相同——它为 roxy **自身**向上游提供静态身份标识（服务 token、固定的 `X-Client-Id` 等）。当客户端转发的头部与某个静态 `--upstream-header` 的名称冲突时，转发值**优先**：调用方的每请求身份比 roxy 的默认值更具体，与反向代理的惯常行为一致。`--upstream-header` 对 FastCGI 上游目前不生效——请改用自动转发。

在 `--transport stdio` 模式下，不存在传入的 HTTP 请求，因此不会转发任何头部；静态 `--upstream-header` 条目仍照常作用于 HTTP 上游。

### 环境变量

所有 CLI 标志都接受对应的 `ROXY_*` 环境变量作为可选的后备值。解析顺序为 **CLI > env > default**：命令行上传入的标志始终优先，仅在标志缺失时才查询环境变量，只有两者均未设置时才使用内置默认值。

| 标志 | Env variable | 示例 |
|---|---|---|
| `--transport` | `ROXY_TRANSPORT` | `stdio` \| `http` |
| `--port` | `ROXY_PORT` | `8080` |
| `--upstream` | `ROXY_UPSTREAM` | `http://localhost:8000/handler` |
| `--upstream-entrypoint` | `ROXY_UPSTREAM_ENTRYPOINT` | `/srv/handler.php` |
| `--upstream-insecure` | `ROXY_UPSTREAM_INSECURE` | `true` \| `false` |
| `--upstream-timeout` | `ROXY_UPSTREAM_TIMEOUT` | `30` |
| `--upstream-header` | `ROXY_UPSTREAM_HEADER` | 以换行符分隔，见下文 |
| `--pool-size` | `ROXY_POOL_SIZE` | `16` |
| `--log-format` | `ROXY_LOG_FORMAT` | `pretty` \| `json` |

#### 多个 upstream-header 值

`ROXY_UPSTREAM_HEADER` 接受以字面换行符分隔的多行头部值。这可以自然地映射到 Kubernetes YAML 块标量：

```yaml
env:
  - name: ROXY_UPSTREAM_HEADER
    value: |-
      Authorization: Bearer xyz
      X-Trace-Id: abc
```

在本地 shell 中，使用 `$'...'` 引号使 `\n` 成为真正的换行符：

```bash
ROXY_UPSTREAM_HEADER=$'Authorization: Bearer xyz\nX-Trace-Id: abc' \
  roxy --upstream https://api.example.com/mcp
```

启动时会丢弃首尾的空行，因此 YAML `|-` 块标量的特殊性不会产生格式错误的头部。如果在 CLI 上传入了 `--upstream-header`，`ROXY_UPSTREAM_HEADER` 将被完全忽略——两个来源不会合并。

#### 布尔值

`ROXY_UPSTREAM_INSECURE` 只接受**完全小写的字符串** `true` 或 `false`。数字形式（`1`、`0`）和其他大小写形式（`TRUE`、`True`、`YES`、`on`）会被 clap 的 `SetTrue + env` 解析器拒绝，并在启动时报错。CLI 标志 `--upstream-insecure`（无值）继续像以前一样工作，简单地表示 `true`。

#### `RUST_LOG`

roxy 遵守标准的 `RUST_LOG` 环境变量，由 `tracing_subscriber::EnvFilter` 在启动时读取；这与上述 `ROXY_*` 变量相互独立，保持不变。

## 编写上游处理器

你的处理器接收简单的 JSON 请求并返回简单的 JSON 响应。**它永远看不到 JSON-RPC、MCP 封帧或会话状态。**roxy 会翻译一切。

### 对于 HTTP 后端

任何从请求体读取 JSON 并将 JSON 写入响应的 HTTP 服务器都可以工作。Python/Flask 示例:

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

### 对于 FastCGI (PHP-FPM) 后端

最小 PHP 处理器:

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

有关包含多个工具、结构化输出、elicitation 和资源链接的完整示例,请参阅 [`examples/handler.php`](../examples/handler.php)。

## 上游协议参考

roxy 发给你的上游的每个请求都是带有以下公共信封字段的 JSON 对象:

```json
{
  "type": "...",
  "session_id": "optional-uuid-or-null",
  "request_id": "uuid-per-request",
  ...
}
```

### 请求类型

#### `discover`

在 roxy 启动时发送一次。你的处理器必须返回它支持的所有工具、资源和提示的完整目录。roxy 会缓存结果,并在不再次查询的情况下将其提供给所有 MCP 客户端。

```json
// 响应
{
  "tools": [
    {
      "name": "tool_name",
      "title": "Human Name",
      "description": "它做什么",
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

`title`、`description`、`mime_type`、`output_schema` 字段都是可选的。

#### `call_tool`

按名称执行工具。请求:

```json
{
  "type": "call_tool",
  "name": "tool_name",
  "arguments": { "key": "value" },
  "elicitation_results": [ ... ],   // 可选:见下方 Elicitation 部分
  "context": { ... }                 // 可选:从之前的 elicit 响应中回显
}
```

成功响应(常规文本输出):

```json
{
  "content": [
    { "type": "text", "text": "结果" }
  ]
}
```

带有**结构化内容**的成功响应(适用于定义了 `output_schema` 的工具):

```json
{
  "content": [{ "type": "text", "text": "5 + 3 = 8" }],
  "structured_content": { "sum": 8, "operands": { "a": 5, "b": 3 } }
}
```

带有嵌入在输出中的**资源链接**的成功响应:

```json
{
  "content": [
    { "type": "text", "text": "预订已确认。" },
    {
      "type": "resource_link",
      "uri": "myapp://bookings/1234",
      "name": "booking-1234",
      "title": "预订 #1234"
    }
  ]
}
```

#### `read_resource`

按 URI 读取资源。请求:

```json
{ "type": "read_resource", "uri": "myapp://status" }
```

响应:与 `call_tool` 相同的 `content` 格式。

#### `get_prompt`

生成提示。请求:

```json
{ "type": "get_prompt", "name": "greet", "arguments": { "name": "Alice" } }
```

响应:与 `call_tool` 相同的 `content` 格式。

#### `elicitation_cancelled`

当 MCP 客户端取消 elicitation 时发送(见下文)。你的处理器可以记录日志/清理;响应会被忽略。

```json
{ "type": "elicitation_cancelled", "name": "tool_name", "action": "decline", "context": {...} }
```

### Elicitation(多轮工具输入)

工具可以在执行过程中**向用户请求更多输入**。在第一次 `call_tool` 时,返回 `elicit` 响应而不是 `content`:

```json
{
  "elicit": {
    "message": "选择哪个航班舱位?",
    "schema": {
      "type": "object",
      "properties": {
        "class": { "type": "string", "enum": ["economy", "business", "first"] }
      },
      "required": ["class"]
    },
    "context": { "step": 1, "destination": "东京" }
  }
}
```

roxy 将 elicitation 转发给 MCP 客户端。当用户填写后,roxy **再次**调用你的工具,在 `elicitation_results` 中传递收集的值,并回显你原来的 `context`:

```json
{
  "type": "call_tool",
  "name": "book_flight",
  "arguments": { "destination": "东京" },
  "elicitation_results": [{ "class": "business" }],
  "context": { "step": 1, "destination": "东京" }
}
```

你可以通过返回另一个 `elicit` 来串联多轮 elicitation,直到收集所有数据。

### 错误响应

任何请求类型都可以返回错误而不是成功:

```json
{ "error": { "code": 404, "message": "Unknown tool: foo" } }
```

## 架构

```
MCP 客户端 (Claude, Cursor, Zed, ...)
       │
       │ 通过 stdio 或 Streamable HTTP 的 JSON-RPC
       ▼
┌──────────────┐
│    rmcp      │  MCP 协议、传输、会话
└──────────────┘
       │
       ▼
┌──────────────┐
│  RoxyServer  │  路由 MCP 方法、缓存能力
└──────────────┘
       │
       │ 简化的 JSON (UpstreamEnvelope + UpstreamRequest)
       ▼
┌──────────────────────────┐
│    UpstreamExecutor      │  trait,有 2 个实现
│  ┌────────┬───────────┐  │
│  │FastCgi │  Http     │  │
│  └────────┴───────────┘  │
└──────────────────────────┘
       │                │
       ▼                ▼
   PHP-FPM /        HTTP(S)
   任何 FastCGI      端点
```

### 源代码布局

```
src/
  main.rs             CLI、日志、传输启动、executor 选择
  lib.rs              库 crate 根（为基准测试和测试重新导出）
  config.rs           clap Config、UpstreamKind（自动检测）、FcgiAddress
  protocol.rs         内部 JSON 类型 (UpstreamEnvelope, UpstreamRequest, ...)
  server.rs           RoxyServer:rmcp ServerHandler 实现 + discover 缓存
  executor/
    mod.rs            UpstreamExecutor trait
    fastcgi.rs        FastCgiExecutor:deadpool + fastcgi-client
    http.rs           HttpExecutor:reqwest + rustls
examples/
  handler.php         包含所有功能的完整 PHP 处理器示例
  echo_upstream.rs    用于负载测试的最小 HTTP echo 后端
  bench_client.rs     用于性能分析的端到端负载客户端
```

### 关键设计决策

- **rmcp 承担重任。**官方 `rmcp` crate 处理所有 MCP 协议复杂性(JSON-RPC、传输协商、会话管理)。roxy 只实现 `ServerHandler`。
- **上游可插拔。** `UpstreamExecutor` trait 抽象了后端通信。FastCGI 和 HTTP 是当前的实现;添加新后端(gRPC、stdio、WebSocket)只需实现一个 trait。
- **能力被缓存。** roxy 在启动时调用一次 `discover` 并将 tools/resources/prompts 保存在内存中。MCP 客户端获得对 `initialize` 的即时响应,而无需触及上游。
- **FastCGI 连接池。** `deadpool` 保持与 PHP-FPM 的连接处于温暖状态,避免每次请求都设置套接字。
- **通过 rustls 的纯 Rust TLS。**无 OpenSSL,无系统库。完全静态的 Linux 构建、简单的交叉编译、可移植的二进制文件。
- **上游保持简单。**你的处理器永远看不到 JSON-RPC、请求 ID(除非作为不透明的信封字段)、会话状态或 MCP 封帧。简单 JSON 进,简单 JSON 出。

## 开发

### 构建和测试

```bash
cargo build           # debug
cargo build --release # 优化
cargo test            # 运行测试
cargo clippy          # linter
cargo fmt             # 格式化
```

### 使用示例 PHP 处理器本地运行

```bash
# 终端 1:启动 PHP-FPM
php-fpm --nodaemonize -d "listen=127.0.0.1:9000" -d "pm=static" -d "pm.max_children=4"

# 终端 2:使用示例处理器运行 roxy
cargo run -- \
    --upstream 127.0.0.1:9000 \
    --upstream-entrypoint "$(pwd)/examples/handler.php"
```

然后用任何 MCP 客户端连接,或通过 stdio 手动发送 JSON-RPC。

### 发布工作流

标签发布(`git tag vX.Y.Z && git push origin vX.Y.Z`)会触发 `.github/workflows/release.yml`,它会:

1. 为所有四个目标构建 release 二进制(macOS arm64/x86_64、Linux musl arm64/x86_64)
2. 将它们打包为 `.tar.gz`,并带有 SHA256 校验和
3. 为两种 Linux 架构构建 `.deb` 和 `.rpm` 软件包
4. 发布带有所有工件的 GitHub Release
5. 更新 [`petstack/homebrew-tap`](https://github.com/petstack/homebrew-tap) 中的 Homebrew 公式(如果设置了 `HOMEBREW_TAP_TOKEN` 密钥)

有关 tap 设置,请参阅 [`packaging/homebrew/README.md`](../packaging/homebrew/README.md)。

## 许可证

[AGPL-3.0-only](../LICENSE)。如果你将修改后的 roxy 版本作为网络服务运行,你必须向该服务的用户提供你的修改内容。
