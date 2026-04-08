# roxy

---

[English](../README.md) · [Русский](README.ru.md) · [Українська](README.uk.md) · [Беларуская](README.be.md) · [Polski](README.pl.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · [Español](README.es.md) · [中文](README.zh-CN.md) · **日本語**

---

**Rust で書かれた高性能な MCP (Model Context Protocol) プロキシサーバー。**

roxy は、MCP クライアント (Claude Desktop、Cursor、Zed など) を、**FastCGI** バックエンド (例: PHP-FPM) または **HTTP(S)** エンドポイントとして動作する任意の upstream ハンドラーに橋渡しします。Rust は、公式の [`rmcp`](https://crates.io/crates/rmcp) クレートを通じて、パフォーマンスに関わるすべて — トランスポート、プロトコル解析、コネクションプーリング、並行性 — を処理します。あなたのハンドラーは、小さな簡略化された JSON プロトコルを扱い、結果を返すだけです。

これにより、**どんな言語** — PHP、Python、Node、Go、Ruby — でも MCP サーバーを書くことができ、毎回 JSON-RPC フレーミング、トランスポート、セッション管理、機能ネゴシエーションを再実装する必要がありません。

## 目次

- [特徴](#特徴)
- [インストール](#インストール)
  - [Homebrew (macOS と Linux)](#homebrew-macos-と-linux)
  - [インストールスクリプト (任意の Unix)](#インストールスクリプト-任意の-unix)
  - [Debian / Ubuntu (.deb)](#debian--ubuntu-deb)
  - [Fedora / RHEL / openSUSE (.rpm)](#fedora--rhel--opensuse-rpm)
  - [Alpine / 任意の Linux (静的 tarball)](#alpine--任意の-linux-静的-tarball)
  - [ソースから](#ソースから)
  - [インストールの確認](#インストールの確認)
- [クイックスタート](#クイックスタート)
- [CLI リファレンス](#cli-リファレンス)
- [upstream ハンドラーの書き方](#upstream-ハンドラーの書き方)
- [upstream プロトコルリファレンス](#upstream-プロトコルリファレンス)
  - [リクエストタイプ](#リクエストタイプ)
  - [Elicitation (マルチターンのツール入力)](#elicitation-マルチターンのツール入力)
  - [エラーレスポンス](#エラーレスポンス)
- [アーキテクチャ](#アーキテクチャ)
- [開発](#開発)
- [ライセンス](#ライセンス)

## 特徴

- **マルチバックエンド**: FastCGI (TCP または Unix ソケット) と HTTP(S) upstream を URL 形式から自動検出
- **トランスポート**: stdio と HTTP/SSE、どちらも `rmcp` でネイティブサポート
- **MCP 2025-06-18 の機能**: elicitation (マルチターン入力)、構造化ツール出力、ツールレスポンス内のリソースリンク
- **コネクションプーリング** (FastCGI、`deadpool` 経由)
- **rustls による TLS** — OpenSSL 依存なし、完全に静的な musl ビルド
- **機能のキャッシング** — tools/resources/prompts は起動時に一度だけ発見される
- **カスタム HTTP ヘッダー**、設定可能なタイムアウト、リクエスト/セッション ID を upstream に伝搬

## インストール

タグ付けされたリリースごとに、**macOS (arm64、x86_64)** と **Linux (arm64、x86_64、musl 静的)** 向けの事前ビルドされたバイナリが公開されます。お使いのプラットフォームに合った方法を選んでください。

### Homebrew (macOS と Linux)

```bash
brew tap petstack/tap
brew install roxy
```

Homebrew がインストールされた macOS (Intel および Apple Silicon) と Linux (x86_64 および arm64) で動作します。

### インストールスクリプト (任意の Unix)

```bash
curl -sSfL https://raw.githubusercontent.com/petstack/roxy/main/install.sh | sh
```

スクリプトは OS とアーキテクチャを自動検出し、GitHub Releases から正しい tarball をダウンロードし、SHA256 チェックサムを検証して、`/usr/local/bin/roxy` にインストールします (必要に応じて `sudo` を使用)。

オプション:

```bash
# 特定のバージョンをインストール
curl -sSfL .../install.sh | sh -s -- --version v0.1.0

# カスタムディレクトリにインストール (sudo 不要)
curl -sSfL .../install.sh | sh -s -- --bin-dir $HOME/.local/bin
```

環境変数 `ROXY_REPO`、`ROXY_VERSION`、`ROXY_BIN_DIR` も使用できます。

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

### Alpine / 任意の Linux (静的 tarball)

Linux バイナリは musl libc に対して静的リンクされているため、**どんな** Linux ディストリビューションでも依存関係なしで動作します:

```bash
# アーキテクチャを選択
ARCH=x86_64   # または aarch64
curl -sSfL https://github.com/petstack/roxy/releases/latest/download/roxy-v0.1.0-${ARCH}-unknown-linux-musl.tar.gz | tar -xz
sudo install -m 755 roxy-v0.1.0-${ARCH}-unknown-linux-musl/roxy /usr/local/bin/
```

Alpine、Debian、Ubuntu、RHEL、Arch、Amazon Linux、Void、NixOS、その他 Linux カーネルを持つあらゆるシステムで動作します。

### ソースから

[Rust](https://rustup.rs/) (edition 2024、stable toolchain) が必要です:

```bash
git clone https://github.com/petstack/roxy
cd roxy
cargo build --release
# バイナリは ./target/release/roxy にあります
```

または `cargo install` で:

```bash
cargo install --git https://github.com/petstack/roxy
```

### インストールの確認

```bash
roxy --version
roxy --help
```

## クイックスタート

roxy には **1 つの引数** だけ必要です: `--upstream`、ハンドラーを指します。upstream のタイプは URL 形式から**自動検出**されます:

| URL 形式 | バックエンドタイプ |
|---|---|
| `http://...` または `https://...` | HTTP(S) |
| `host:port` | FastCGI TCP |
| `/path/to/socket` | FastCGI Unix ソケット |

### 例: HTTP バックエンド

```bash
# ポート 8000 で HTTP ハンドラーを起動 (どんな言語・フレームワークでも)
# それから roxy をそこに向ける:
roxy --upstream http://localhost:8000/mcp
```

### 例: PHP-FPM バックエンド

```bash
# PHP-FPM を起動
php-fpm --nodaemonize \
    -d "listen=127.0.0.1:9000" \
    -d "pm=static" \
    -d "pm.max_children=4"

# roxy をそれに向ける
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /absolute/path/to/handler.php
```

### MCP クライアントから接続

Claude Desktop や、MCP サーバーをサブプロセスとして起動するクライアント (stdio トランスポート — デフォルト) の場合:

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

HTTP/SSE で接続するネットワーククライアントの場合:

```bash
roxy --transport http --port 8080 --upstream http://localhost:8000/mcp
# クライアントは http://localhost:8080/sse に接続
```

## CLI リファレンス

```
roxy [OPTIONS] --upstream <UPSTREAM>
```

| フラグ | デフォルト | 説明 |
|---|---|---|
| `--upstream <URL>` | **必須** | バックエンド URL。タイプは自動検出 (上記の表を参照) |
| `--transport <MODE>` | `stdio` | MCP クライアントのトランスポート: `stdio` または `http` |
| `--port <PORT>` | `8080` | HTTP リッスンポート (`--transport http` の場合のみ) |
| `--upstream-entrypoint <PATH>` | — | FastCGI バックエンドに送信される `SCRIPT_FILENAME` (PHP-FPM には必須) |
| `--upstream-insecure` | `false` | HTTPS upstream の TLS 証明書検証をスキップ |
| `--upstream-timeout <SECS>` | `30` | HTTP upstream リクエストのタイムアウト (秒) |
| `--upstream-header <HEADER>` | — | カスタム HTTP ヘッダー、`Name: Value`。繰り返し可能 |
| `--pool-size <N>` | `16` | FastCGI コネクションプールのサイズ |
| `--log-format <FORMAT>` | `pretty` | ログ形式: `pretty` または `json` |

ログの**レベル**は環境変数 `RUST_LOG` で制御します:

```bash
RUST_LOG=debug roxy --upstream http://localhost:8000/mcp
RUST_LOG=roxy=debug,rmcp=info roxy --upstream ...  # モジュールごとのフィルター
```

### HTTP バックエンドの完全な例

```bash
roxy --upstream https://api.example.com/mcp \
     --upstream-header "Authorization: Bearer $TOKEN" \
     --upstream-header "X-Tenant: acme" \
     --upstream-timeout 60
```

### FastCGI (PHP-FPM) の完全な例

```bash
# TCP
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /srv/app/handler.php

# Unix ソケット
roxy --upstream /var/run/php-fpm.sock --upstream-entrypoint /srv/app/handler.php

# FastCGI upstream での HTTP トランスポート
roxy --transport http --port 8080 \
     --upstream 127.0.0.1:9000 \
     --upstream-entrypoint /srv/app/handler.php
```

## upstream ハンドラーの書き方

あなたのハンドラーはシンプルな JSON リクエストを受け取り、シンプルな JSON レスポンスを返します。**JSON-RPC、MCP フレーミング、セッション状態は一切見えません。** roxy がすべて翻訳します。

### HTTP バックエンド向け

リクエストボディから JSON を読み取り、レスポンスに JSON を書き込む HTTP サーバーであれば何でも動作します。Python/Flask の例:

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

### FastCGI (PHP-FPM) バックエンド向け

最小限の PHP ハンドラー:

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

複数のツール、構造化出力、elicitation、リソースリンクを含む完全な例については [`examples/handler.php`](../examples/handler.php) を参照してください。

## upstream プロトコルリファレンス

roxy から upstream へのすべてのリクエストは、以下の共通のエンベロープフィールドを持つ JSON オブジェクトです:

```json
{
  "type": "...",
  "session_id": "optional-uuid-or-null",
  "request_id": "uuid-per-request",
  ...
}
```

### リクエストタイプ

#### `discover`

roxy 起動時に一度だけ送信されます。あなたのハンドラーは、サポートするすべてのツール、リソース、プロンプトの完全なカタログを返す必要があります。roxy は結果をキャッシュし、再度問い合わせることなくすべての MCP クライアントに提供します。

```json
// レスポンス
{
  "tools": [
    {
      "name": "tool_name",
      "title": "Human Name",
      "description": "何をするか",
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

`title`、`description`、`mime_type`、`output_schema` フィールドはすべてオプションです。

#### `call_tool`

名前でツールを実行します。リクエスト:

```json
{
  "type": "call_tool",
  "name": "tool_name",
  "arguments": { "key": "value" },
  "elicitation_results": [ ... ],   // オプション: 下記の Elicitation セクションを参照
  "context": { ... }                 // オプション: 前回の elicit レスポンスからのエコー
}
```

成功レスポンス (通常のテキスト出力):

```json
{
  "content": [
    { "type": "text", "text": "結果" }
  ]
}
```

**構造化コンテンツ** を含む成功レスポンス (`output_schema` が定義されたツール向け):

```json
{
  "content": [{ "type": "text", "text": "5 + 3 = 8" }],
  "structured_content": { "sum": 8, "operands": { "a": 5, "b": 3 } }
}
```

出力に **リソースリンク** を埋め込んだ成功レスポンス:

```json
{
  "content": [
    { "type": "text", "text": "予約が確定しました。" },
    {
      "type": "resource_link",
      "uri": "myapp://bookings/1234",
      "name": "booking-1234",
      "title": "予約 #1234"
    }
  ]
}
```

#### `read_resource`

URI でリソースを読み取ります。リクエスト:

```json
{ "type": "read_resource", "uri": "myapp://status" }
```

レスポンス: `call_tool` と同じ `content` 形式。

#### `get_prompt`

プロンプトを生成します。リクエスト:

```json
{ "type": "get_prompt", "name": "greet", "arguments": { "name": "Alice" } }
```

レスポンス: `call_tool` と同じ `content` 形式。

#### `elicitation_cancelled`

MCP クライアントが elicitation をキャンセルしたときに送信されます (下記参照)。ハンドラーはログ出力/クリーンアップ可能; レスポンスは無視されます。

```json
{ "type": "elicitation_cancelled", "name": "tool_name", "action": "decline", "context": {...} }
```

### Elicitation (マルチターンのツール入力)

ツールは実行の途中で**ユーザーに追加の入力を要求**できます。最初の `call_tool` で、`content` の代わりに `elicit` レスポンスを返します:

```json
{
  "elicit": {
    "message": "フライトクラスを選択してください",
    "schema": {
      "type": "object",
      "properties": {
        "class": { "type": "string", "enum": ["economy", "business", "first"] }
      },
      "required": ["class"]
    },
    "context": { "step": 1, "destination": "東京" }
  }
}
```

roxy は elicitation を MCP クライアントに転送します。ユーザーが入力を完了すると、roxy はツールを**再度**呼び出し、収集された値を `elicitation_results` に、元の `context` をエコーとして渡します:

```json
{
  "type": "call_tool",
  "name": "book_flight",
  "arguments": { "destination": "東京" },
  "elicitation_results": [{ "class": "business" }],
  "context": { "step": 1, "destination": "東京" }
}
```

すべてのデータが収集されるまで、別の `elicit` を返すことで複数ラウンドの elicitation をチェーンすることができます。

### エラーレスポンス

どのリクエストタイプも、成功の代わりにエラーを返すことができます:

```json
{ "error": { "code": 404, "message": "Unknown tool: foo" } }
```

## アーキテクチャ

```
MCP クライアント (Claude, Cursor, Zed, ...)
       │
       │ stdio または HTTP/SSE 経由の JSON-RPC
       ▼
┌──────────────┐
│    rmcp      │  MCP プロトコル、トランスポート、セッション
└──────────────┘
       │
       ▼
┌──────────────┐
│  RoxyServer  │  MCP メソッドのルーティング、機能のキャッシュ
└──────────────┘
       │
       │ 簡略化された JSON (UpstreamEnvelope + UpstreamRequest)
       ▼
┌──────────────────────────┐
│    UpstreamExecutor      │  2 つの実装を持つ trait
│  ┌────────┬───────────┐  │
│  │FastCgi │  Http     │  │
│  └────────┴───────────┘  │
└──────────────────────────┘
       │                │
       ▼                ▼
   PHP-FPM /        HTTP(S)
   任意の FastCGI    エンドポイント
```

### ソースレイアウト

```
src/
  main.rs             CLI、ログ、トランスポート起動、executor 選択
  config.rs           clap Config、UpstreamKind (自動検出)、FcgiAddress
  protocol.rs         内部 JSON 型 (UpstreamEnvelope, UpstreamRequest, ...)
  server.rs           RoxyServer: rmcp ServerHandler 実装 + discover キャッシュ
  executor/
    mod.rs            UpstreamExecutor trait
    fastcgi.rs        FastCgiExecutor: deadpool + fastcgi-client
    http.rs           HttpExecutor: reqwest + rustls
examples/
  handler.php         すべての機能を含む完全な PHP ハンドラーの例
```

### 主要な設計判断

- **rmcp が重労働を担当。** 公式の `rmcp` クレートが MCP プロトコルのすべての複雑さ (JSON-RPC、トランスポートネゴシエーション、セッション管理) を処理します。roxy は `ServerHandler` だけを実装します。
- **upstream はプラガブル。** `UpstreamExecutor` trait がバックエンド通信を抽象化しています。FastCGI と HTTP が現在の実装で、新しいバックエンド (gRPC、stdio、WebSocket) を追加するには 1 つの trait を実装するだけです。
- **機能はキャッシュされます。** roxy は起動時に一度 `discover` を呼び出し、tools/resources/prompts をメモリに保持します。MCP クライアントは upstream に触れることなく `initialize` に対して瞬時にレスポンスを受け取ります。
- **FastCGI 向けのコネクションプーリング。** `deadpool` が PHP-FPM への接続を温かく保ち、リクエストごとのソケットセットアップを回避します。
- **rustls による純粋 Rust の TLS。** OpenSSL なし、システムライブラリなし。完全に静的な Linux ビルド、簡単なクロスコンパイル、移植可能なバイナリ。
- **upstream はシンプルなまま。** あなたのハンドラーは JSON-RPC、リクエスト ID (エンベロープの不透明なフィールドとしてを除き)、セッション状態、MCP フレーミングを一切見ません。シンプルな JSON が入り、シンプルな JSON が出ていきます。

## 開発

### ビルドとテスト

```bash
cargo build           # debug
cargo build --release # 最適化
cargo test            # テストを実行
cargo clippy          # リンター
cargo fmt             # フォーマット
```

### サンプル PHP ハンドラーでのローカル実行

```bash
# ターミナル 1: PHP-FPM を起動
php-fpm --nodaemonize -d "listen=127.0.0.1:9000" -d "pm=static" -d "pm.max_children=4"

# ターミナル 2: サンプルハンドラーで roxy を起動
cargo run -- \
    --upstream 127.0.0.1:9000 \
    --upstream-entrypoint "$(pwd)/examples/handler.php"
```

その後、任意の MCP クライアントで接続するか、stdio を通じて手動で JSON-RPC を送信します。

### リリースワークフロー

タグ付きリリース (`git tag vX.Y.Z && git push origin vX.Y.Z`) は `.github/workflows/release.yml` をトリガーし、以下を行います:

1. 4 つすべてのターゲット (macOS arm64/x86_64、Linux musl arm64/x86_64) の release バイナリをビルド
2. SHA256 チェックサム付きで `.tar.gz` としてパッケージ化
3. 両方の Linux アーキテクチャ向けの `.deb` と `.rpm` パッケージをビルド
4. すべてのアーティファクトを含む GitHub Release を公開
5. [`petstack/homebrew-tap`](https://github.com/petstack/homebrew-tap) の Homebrew formula を更新 (`HOMEBREW_TAP_TOKEN` シークレットが設定されている場合)

tap のセットアップについては [`packaging/homebrew/README.md`](../packaging/homebrew/README.md) を参照してください。

## ライセンス

[AGPL-3.0-only](../LICENSE). 変更した roxy をネットワークサービスとして実行する場合、そのサービスのユーザーに対して変更内容を提供する必要があります。
