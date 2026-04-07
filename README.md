# Roxy

High-performance MCP (Model Context Protocol) server written in Rust with PHP backend.

Rust handles everything performance-critical: connections, concurrency, protocol parsing, connection pooling. PHP handles everything business-critical: tool logic, resource data, prompt generation. The two communicate through a simplified internal JSON protocol via FastCGI.

## How It Works

```
MCP Client (Claude, Cursor, etc.)
       |
       | JSON-RPC (stdio or HTTP/SSE)
       v
  +-----------+
  |   rmcp    |  Rust: MCP protocol, transport, sessions
  +-----------+
       |
  +-----------+
  |  roxy    |  Rust: routes MCP methods, caches capabilities
  +-----------+
       |
       | Internal JSON (simplified, no JSON-RPC)
       | via FastCGI
       v
  +-----------+
  |  PHP-FPM  |  PHP: business logic
  +-----------+
```

### Request Flow

1. MCP client sends a JSON-RPC request (e.g., `tools/call`) via stdio or HTTP/SSE
2. `rmcp` crate handles transport and protocol parsing
3. `RoxyServer` receives the typed MCP call and builds a simplified JSON request
4. `FastCgiExecutor` sends the request to PHP-FPM through a pooled FastCGI connection
5. PHP reads the JSON from `php://input`, processes it, and writes a JSON response
6. Rust parses the response and maps it back to MCP protocol types
7. `rmcp` serializes and sends back to the client

### Internal Protocol

PHP never sees JSON-RPC, request IDs, or MCP framing. It receives simple JSON:

```json
{"type": "discover"}
{"type": "call_tool", "name": "add", "arguments": {"a": 2, "b": 3}}
{"type": "read_resource", "uri": "roxy://status"}
{"type": "get_prompt", "name": "greet", "arguments": {"name": "World"}}
```

And responds with:

```json
{"content": [{"type": "text", "text": "5"}]}
{"error": {"code": 404, "message": "Unknown tool: foo"}}
```

### Capability Discovery

At startup, roxy sends a `{"type": "discover"}` request to PHP. PHP returns the full list of tools, resources, and prompts it supports. roxy caches this and serves it to all MCP clients without hitting PHP again.

## Requirements

- Rust (edition 2024)
- PHP-FPM (PHP 8.1+)

## Installation

```bash
cargo build --release
```

## Usage

### 1. Start PHP-FPM

```bash
# TCP (default port 9000)
php-fpm --nodaemonize -d "listen=127.0.0.1:9000" -d "pm=static" -d "pm.max_children=4"

# Or via Unix socket
php-fpm --nodaemonize -d "listen=/tmp/php-fpm.sock" -d "pm=static" -d "pm.max_children=4"
```

### 2. Run roxy

**stdio transport** (for clients that spawn the server as a child process):

```bash
roxy --transport stdio \
      --php-fpm 127.0.0.1:9000 \
      --php-entrypoint /path/to/handler.php
```

**HTTP/SSE transport** (for network clients):

```bash
roxy --transport http \
      --port 8080 \
      --php-fpm 127.0.0.1:9000 \
      --php-entrypoint /path/to/handler.php
```

**Unix socket to PHP-FPM:**

```bash
roxy --php-fpm /tmp/php-fpm.sock --php-entrypoint /path/to/handler.php
```

### CLI Options

| Flag | Default | Description |
|---|---|---|
| `--transport` | `stdio` | Transport mode: `stdio` or `http` |
| `--port` | `8080` | HTTP listen port (only for `http` transport) |
| `--php-fpm` | `127.0.0.1:9000` | PHP-FPM address (TCP `host:port` or Unix socket path) |
| `--php-entrypoint` | required | Absolute path to PHP handler script |
| `--pool-size` | `16` | FastCGI connection pool size |
| `--log-format` | `pretty` | Log format: `pretty` or `json` |

Log level is controlled via the `RUST_LOG` environment variable:

```bash
RUST_LOG=debug roxy --php-entrypoint handler.php
```

## Writing a PHP Handler

A PHP handler is a single file that receives JSON from `php://input` and writes JSON to stdout. It routes by the `type` field.

Minimal example:

```php
<?php
$request = json_decode(file_get_contents('php://input'), true);
header('Content-Type: application/json');

echo match ($request['type']) {
    'discover' => json_encode([
        'tools' => [
            [
                'name' => 'hello',
                'description' => 'Says hello',
                'input_schema' => [
                    'type' => 'object',
                    'properties' => [
                        'name' => ['type' => 'string', 'description' => 'Your name'],
                    ],
                    'required' => ['name'],
                ],
            ],
        ],
        'resources' => [],
        'prompts' => [],
    ]),

    'call_tool' => json_encode([
        'content' => [['type' => 'text', 'text' => "Hello, {$request['arguments']['name']}!"]],
    ]),

    default => json_encode([
        'error' => ['code' => 400, 'message' => "Unknown type: {$request['type']}"],
    ]),
};
```

See `examples/handler.php` for a complete example with multiple tools, resources, and prompts.

### Request Types

**`discover`** -- called at server startup to learn what PHP offers.

Response must include `tools`, `resources`, and `prompts` arrays (can be empty):

```json
{
  "tools": [
    {
      "name": "tool_name",
      "description": "What it does",
      "input_schema": { "type": "object", "properties": {...}, "required": [...] }
    }
  ],
  "resources": [
    {
      "uri": "myapp://resource-id",
      "name": "display-name",
      "description": "What this resource is",
      "mime_type": "application/json"
    }
  ],
  "prompts": [
    {
      "name": "prompt_name",
      "description": "What it generates",
      "arguments": [
        { "name": "arg_name", "description": "What it is", "required": true }
      ]
    }
  ]
}
```

**`call_tool`** -- execute a tool.

Request: `{"type": "call_tool", "name": "...", "arguments": {...}}`

**`read_resource`** -- read a resource.

Request: `{"type": "read_resource", "uri": "..."}`

**`get_prompt`** -- generate a prompt.

Request: `{"type": "get_prompt", "name": "...", "arguments": {...}}`

### Response Format

**Success:**

```json
{
  "content": [
    {"type": "text", "text": "result goes here"}
  ]
}
```

**Error:**

```json
{
  "error": {"code": 400, "message": "Something went wrong"}
}
```

## Architecture

```
src/
  main.rs             -- CLI, logging, transport startup
  config.rs           -- clap config, FcgiAddress (TCP/Unix)
  protocol.rs         -- internal JSON types (PhpRequest, PhpCallResult, etc.)
  server.rs           -- RoxyServer: rmcp ServerHandler impl, discover caching
  executor/
    mod.rs            -- PhpExecutor trait
    fastcgi.rs        -- FastCgiExecutor: deadpool + fastcgi-client
examples/
  handler.php         -- example PHP handler
```

### Key Design Decisions

- **`rmcp` crate** handles all MCP protocol complexity (JSON-RPC, sessions, capabilities negotiation). roxy only implements the `ServerHandler` trait.
- **Connection pooling** via `deadpool` keeps FastCGI connections alive to PHP-FPM, avoiding per-request connection overhead.
- **`PhpExecutor` trait** abstracts the PHP communication layer. Currently implemented with FastCGI (`FastCgiExecutor`), designed to support worker pool and HTTP backends in the future.
- **Discover caching** -- PHP capabilities are fetched once at startup and cached. MCP clients receive cached data without triggering PHP on each `initialize`.
- **PHP isolation** -- PHP knows nothing about MCP, JSON-RPC, or transport details. It receives and returns simple JSON. This means any PHP framework or vanilla PHP works as a handler.

## License

AGPL-3.0
