---
title: Configuration
description: Transport modes, backend auto-detection from the upstream URL, the full CLI flag reference, and the matching ROXY_* environment variables.
---

# Configuration

## Transport modes — stdio vs HTTP

roxy offers two ways for the AI client to reach it. Pick the one that fits your situation.

```mermaid
flowchart TB
    subgraph STDIO [" stdio (default) "]
        direction LR
        C1[Claude Desktop] -. pipe .-> R1[roxy subprocess]
        R1 --> B1[Your backend]
    end

    subgraph HTTP [" HTTP "]
        direction LR
        C2[Remote AI client] -- HTTP + SSE --> R2[roxy server<br/>:8080]
        R2 --> B2[Your backend]
    end
```

### stdio (default)

The AI client **starts roxy as a child process** and communicates through standard input/output. Nothing is exposed on the network. This is what you want for a desktop AI app like Claude Desktop or Cursor.

- Started by: the MCP client itself, via the `command` field in its config.
- Exposes: nothing.
- Good for: local desktop use.

### HTTP

roxy runs as a standalone server and listens on a port (default `:8080`). Clients connect over HTTP with Server-Sent Events (SSE) for streaming. This lets remote users or multiple clients share one roxy instance.

- Started by: you, manually or from systemd/docker/k8s.
- Endpoint: `http://<host>:<port>/mcp`
- Good for: team deployments, containerized setups, remote access.

```
roxy --transport http --port 8080 --upstream http://your-backend/
```

**Who may connect.** roxy accepts only requests whose `Host` header is
`localhost`, `127.0.0.1` or `::1`. That default is a guard, not an oversight: it
stops a page open in someone's browser from reaching a roxy running on their
machine by pointing a hostname at `127.0.0.1` (DNS rebinding). Put roxy behind
nginx, Caddy or Traefik and the client's own hostname usually arrives unchanged,
so list it — otherwise every request comes back `403 Forbidden`:

```
roxy --transport http --allowed-host mcp.example.com --upstream http://your-backend/
```

Use `--allowed-host '*'` to turn the check off entirely, which is reasonable
only when whatever sits in front of roxy already validates the host.

---

## MCP protocol revisions

roxy is a gateway, so it serves **every** MCP revision a client may speak, and
picks the behaviour per request from the revision that client negotiated. One
process answers all of them on the same endpoint:

| Revisions | Lifecycle | Result shape |
|---|---|---|
| `2024-11-05`, `2025-03-26`, `2025-06-18`, `2025-11-25` | `initialize` handshake, plus `Mcp-Session-Id` and the standalone `GET` SSE stream where the revision defines them | no `resultType` |
| `2026-07-28` | no handshake — every request carries its own `_meta` (protocol version, client capabilities, client info) and no session id | `resultType` on every result; `Mcp-Method` / `Mcp-Name` request headers are required and validated against the body |

Nothing in your backend has to change for this — roxy absorbs the difference, so
one handler serves clients from both eras. A client can also ask what roxy
speaks, without committing to a revision, by calling `server/discover`.

Two things `2026-07-28` does not have yet in roxy: multi round-trip elicitation
(MRTR) and cache hints (`ttlMs` / `cacheScope`) on list results. Elicitation
still uses the server-initiated flow that `2026-07-28` removed, so a multi-step
"ask the user" tool needs a client on `2025-06-18` … `2025-11-25`; from a
`2026-07-28` client the call returns an error explaining that instead of
hanging.

---

## Connecting to your backend

roxy figures out what kind of backend you have by looking at the `--upstream` value.

```mermaid
flowchart LR
    U[--upstream value] --> Q{What does it look like?}
    Q -->|starts with http:// or https://| H[HTTP backend]
    Q -->|host:port| F1[FastCGI over TCP]
    Q -->|/path/to/socket| F2[FastCGI over Unix socket]
```

| Example `--upstream` | Detected as |
|---|---|
| `http://localhost:8000/mcp` | HTTP |
| `https://api.example.com/mcp` | HTTPS |
| `127.0.0.1:9000` | FastCGI, TCP |
| `/var/run/php-fpm.sock` | FastCGI, Unix socket |

For **FastCGI** backends you also need to tell roxy *which script* to run, via `--upstream-entrypoint`:

```
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /srv/app/handler.php
```

For **HTTP** backends you can add headers, timeouts, and skip TLS verification if needed (see the flags below).

---

## CLI flags reference

Every flag has a matching environment variable. Precedence: **CLI > environment > default**.

| Flag | Env variable | Default | What it does |
|---|---|---|---|
| `--upstream <URL>` | `ROXY_UPSTREAM` | — (**required**) | Where your backend lives. Auto-detects HTTP / FastCGI-TCP / FastCGI-Unix. |
| `--transport <mode>` | `ROXY_TRANSPORT` | `stdio` | How the AI client reaches roxy. Values: `stdio`, `http`. |
| `--port <N>` | `ROXY_PORT` | `8080` | TCP port to listen on, when transport is `http`. |
| `--upstream-entrypoint <path>` | `ROXY_UPSTREAM_ENTRYPOINT` | — | For FastCGI only. The absolute path of the handler file (sent as `SCRIPT_FILENAME`). |
| `--upstream-timeout <secs>` | `ROXY_UPSTREAM_TIMEOUT` | `30` | How long roxy waits for your backend before giving up. |
| `--upstream-insecure` | `ROXY_UPSTREAM_INSECURE` | `false` | Skip TLS certificate verification. Only use in development. Env accepts only literal `true` or `false`. |
| `--upstream-header "Name: Value"` | `ROXY_UPSTREAM_HEADER` | — | Add a static header to every upstream HTTP request. Repeatable on CLI. Ignored for FastCGI. |
| `--allowed-host <host>` | `ROXY_ALLOWED_HOST` | `localhost`, `127.0.0.1`, `::1` | `Host` header values accepted from clients, when transport is `http`. Repeatable on CLI, newline-separated in env. Entries may include a port. `*` accepts any host. |
| `--max-body-size <bytes>` | `ROXY_MAX_BODY_SIZE` | `4194304` (4 MiB) | Largest inbound request body. Bigger ones get `413 Payload Too Large`. Raise it if clients send large tool arguments. |
| `--pool-size <N>` | `ROXY_POOL_SIZE` | `16` | Number of reusable connections to a FastCGI backend. |
| `--log-format <fmt>` | `ROXY_LOG_FORMAT` | `pretty` | `pretty` for humans, `json` for log aggregators. |

Ask roxy to describe itself anytime:

```
roxy --help
roxy --version
```

---

## Environment variables

Every flag above can be set via its `ROXY_*` equivalent. A few notes:

- **`ROXY_UPSTREAM_INSECURE`** accepts only the exact strings `true` or `false`. `TRUE`, `1`, `yes` will be rejected. This is deliberate — it prevents accidental security holes from typos.
- **`ROXY_UPSTREAM_HEADER`** is newline-separated for multiple headers. It works naturally with Kubernetes YAML:
  ```yaml
  env:
    - name: ROXY_UPSTREAM_HEADER
      value: |-
        Authorization: Bearer abcdef
        X-Tenant: acme
  ```
  From a shell:
  ```
  ROXY_UPSTREAM_HEADER=$'Authorization: Bearer abcdef\nX-Tenant: acme' roxy --upstream ...
  ```
  If you pass `--upstream-header` on the command line *at all*, the environment value is ignored entirely — no merging.
- **`RUST_LOG`** controls log verbosity (see [Logging](operations.md#logging-and-observability)). It's not a `ROXY_*` variable because it's handled by the standard Rust logging stack.
