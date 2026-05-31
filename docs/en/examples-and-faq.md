---
title: Examples & FAQ
description: Runnable example backends in Python, TypeScript, and PHP, end-to-end configuration recipes, and answers to the questions people ask most.
---

# Examples & FAQ

## Example backends

Complete, runnable examples live in the `examples/` folder. They all implement the same feature set so you can compare languages side by side.

| File | Language | How to run it |
|---|---|---|
| `handler.py` | Python 3, standard library only | `python3 examples/handler.py` — listens on `:8000`. |
| `handler.ts` | TypeScript / Node.js | `npx tsx examples/handler.ts` — listens on `:8000`. |
| `handler.php` | PHP (via PHP-FPM) | Start `php-fpm` on `:9000`, then point roxy at it with `--upstream-entrypoint examples/handler.php`. |
| `echo_upstream.rs` | Rust | A minimal echo backend used for benchmarking roxy itself. |

Each example demonstrates tools, resources, prompts, elicitation, structured output, and resource links.

---

## Full configuration examples

### Claude Desktop + a Python backend

Terminal 1:

```
python3 examples/handler.py
```

Terminal 2 — no roxy needed in a terminal because Claude Desktop starts it for you. Just edit `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "my-tools": {
      "command": "roxy",
      "args": ["--upstream", "http://127.0.0.1:8000/"]
    }
  }
}
```

### Claude Desktop + PHP-FPM

Terminal 1 — start PHP-FPM:

```
php-fpm --nodaemonize -d "listen=127.0.0.1:9000" -d "pm=static" -d "pm.max_children=4"
```

Claude Desktop config:

```json
{
  "mcpServers": {
    "php-tools": {
      "command": "roxy",
      "args": [
        "--upstream", "127.0.0.1:9000",
        "--upstream-entrypoint", "/absolute/path/to/handler.php"
      ]
    }
  }
}
```

### Team server, remote access over HTTPS

Run roxy as a long-lived network server behind your own reverse proxy / TLS terminator:

```
roxy --transport http --port 8080 \
     --upstream https://internal-api.example.com/mcp \
     --upstream-header "Authorization: Bearer $SERVICE_TOKEN" \
     --upstream-header "X-Tenant: acme" \
     --upstream-timeout 60 \
     --log-format json
```

Clients connect to `https://roxy.yourcompany.com/mcp`.

### Kubernetes-style configuration via environment

```
ROXY_TRANSPORT=http \
ROXY_PORT=8080 \
ROXY_UPSTREAM=https://api.example.com/mcp \
ROXY_UPSTREAM_HEADER=$'Authorization: Bearer xyz\nX-Tenant: acme' \
ROXY_UPSTREAM_TIMEOUT=60 \
ROXY_LOG_FORMAT=json \
RUST_LOG=info \
roxy
```

---

## Frequently asked questions

**Do I need to know Rust to use roxy?**
No. roxy is written in Rust, but you only ever write your backend, in any language you like.

**Does my backend need to speak MCP?**
No. Your backend speaks the simple JSON protocol described in [The backend API](backend-api.md). roxy handles MCP for you.

**Can roxy serve multiple clients at the same time?**
Yes. Under `--transport http` many clients can connect in parallel.

**Can I put roxy behind nginx or a load balancer?**
Yes. Run roxy with `--transport http` and front it with whatever TLS / load-balancing solution you use. Forwarded headers will reach your backend.

**Is data persisted anywhere?**
No. roxy is a stateless translator. Sessions, storage, and state all live in your backend.

**What happens if my backend restarts?**
Existing in-flight requests will fail with a connection error and be reported to the client. New requests succeed as soon as the backend is back up.

**What if I change tools at runtime?**
Nothing special is needed. roxy calls `discover` on every `list_tools`/`list_resources`/`list_prompts` request, so the client sees your new catalogue the next time it lists.

**Why are some headers missing from my backend?**
They were probably hop-by-hop headers or internally managed by roxy. See [Header forwarding](operations.md#header-forwarding).

**Can I use HTTPS between roxy and my backend?**
Yes — just use an `https://` URL in `--upstream`. For development against a self-signed cert, add `--upstream-insecure`.

**Can I run roxy in Docker?**
Yes. The static Linux tarball works in any minimal container. Expose the port if using `--transport http`.

---

Happy routing. If anything here is unclear, that's a documentation bug — please let the roxy team know.
