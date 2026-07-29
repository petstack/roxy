//! End-to-end coverage of the MCP revisions roxy serves on one endpoint.
//!
//! roxy is a gateway: a `2025-11-25` client and a `2026-07-28` client must both
//! work against the same process, against the same unchanged upstream. These
//! tests drive [`roxy::transport::http_service`] — the exact service the binary
//! serves — over loopback HTTP with hand-written JSON-RPC, because the
//! assertions are about wire details an SDK client would hide: whether
//! `resultType` appears, whether a session id is issued, whether the standalone
//! `GET` stream still exists, and whether SEP-2243 headers are enforced.

use std::sync::Arc;
use std::time::Duration;

use roxy::executor::{ExecuteContext, UpstreamExecutor};
use roxy::protocol::{
    UpstreamCallResult, UpstreamContent, UpstreamContentResponse, UpstreamDiscoverResponse,
    UpstreamEnvelope, UpstreamRequest, UpstreamToolDef,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

/// Newest revision that still uses `initialize`, sessions and server-initiated
/// elicitation.
const LEGACY: &str = "2025-11-25";
/// The stateless revision added by issue 0021.
const MODERN: &str = "2026-07-28";

const TOOL: &str = "echo";

/// Bound on every read from the server. A POST answered over SSE closes its
/// stream once the reply is delivered, but roxy keeps rmcp's default 15-second
/// keep-alive, so an unbounded read would hang instead of failing if that ever
/// changed.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

// --- stub upstream -------------------------------------------------------

/// Minimal upstream: one tool, which echoes its own name back. Stands in for
/// the FastCGI/HTTP backend so these tests exercise only roxy's protocol
/// surface.
struct StubUpstream;

impl UpstreamExecutor for StubUpstream {
    async fn execute(
        &self,
        request: &UpstreamEnvelope<'_>,
        _ctx: ExecuteContext<'_>,
    ) -> anyhow::Result<UpstreamCallResult> {
        let text = match &request.request {
            UpstreamRequest::CallTool { name, .. } => format!("called {name}"),
            other => anyhow::bail!("unexpected upstream request: {other:?}"),
        };
        Ok(UpstreamCallResult::Content(UpstreamContentResponse {
            content: vec![UpstreamContent::Text { text }],
            structured_content: None,
        }))
    }

    async fn discover(&self) -> anyhow::Result<UpstreamDiscoverResponse> {
        Ok(UpstreamDiscoverResponse {
            tools: vec![UpstreamToolDef {
                name: TOOL.to_string(),
                title: None,
                description: Some("echo the tool name".to_string()),
                input_schema: None,
                output_schema: None,
            }],
            resources: vec![],
            prompts: vec![],
        })
    }
}

// --- harness -------------------------------------------------------------

/// A roxy HTTP endpoint on an ephemeral loopback port. Dropping the returned
/// guard cancels the server.
struct Roxy {
    client: reqwest::Client,
    url: String,
    cancel: CancellationToken,
}

impl Drop for Roxy {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

async fn spawn_roxy() -> Roxy {
    let cancel = CancellationToken::new();
    let service = roxy::transport::http_service(Arc::new(StubUpstream), cancel.child_token());
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback listener");
    let addr = listener.local_addr().expect("listener address");

    tokio::spawn({
        let cancel = cancel.clone();
        async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move { cancel.cancelled_owned().await })
                .await;
        }
    });

    Roxy {
        client: reqwest::Client::new(),
        url: format!("http://{addr}/mcp"),
        cancel,
    }
}

impl Roxy {
    /// POST a JSON-RPC message with the given extra headers.
    async fn post(&self, body: &Value, headers: &[(&str, &str)]) -> reqwest::Response {
        let mut request = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .body(body.to_string());
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        request.send().await.expect("send request")
    }

    /// POST and return the JSON-RPC reply, asserting a 2xx status first.
    async fn call(&self, body: &Value, headers: &[(&str, &str)]) -> Value {
        let response = self.post(body, headers).await;
        assert!(
            response.status().is_success(),
            "HTTP {} for {body}",
            response.status()
        );
        read_message(response).await
    }
}

/// Read one JSON-RPC message from a response, whether it arrived as plain JSON
/// or inside an SSE stream.
///
/// Parses incrementally rather than buffering the whole body: in legacy session
/// mode rmcp emits a priming event before the reply and may hold the stream open
/// afterwards, so the read stops at the first frame that actually carries a
/// JSON-RPC message.
async fn read_message(mut response: reqwest::Response) -> Value {
    let is_json = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));

    let mut body = String::new();
    loop {
        if !is_json && let Some(message) = jsonrpc_frame(&body) {
            return message;
        }
        let chunk = tokio::time::timeout(READ_TIMEOUT, response.chunk())
            .await
            .expect("reply arrives within the read timeout")
            .expect("read response body");
        match chunk {
            Some(bytes) => body.push_str(&String::from_utf8_lossy(&bytes)),
            None => break,
        }
    }

    if is_json {
        serde_json::from_str(&body).expect("parse JSON body")
    } else {
        jsonrpc_frame(&body).expect("SSE body carries a JSON-RPC frame")
    }
}

/// First complete SSE event in `body` whose `data:` payload is a JSON-RPC
/// message. Skips rmcp's priming event, which carries an empty `data:` line.
fn jsonrpc_frame(body: &str) -> Option<Value> {
    body.split("\n\n")
        .filter_map(|event| event.lines().find_map(|line| line.strip_prefix("data:")))
        .map(str::trim)
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .find(|message| message.get("jsonrpc").is_some())
}

// --- request builders ----------------------------------------------------

fn initialize_body(version: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": version,
            "capabilities": {},
            "clientInfo": {"name": "roxy-tests", "version": "0.0.0"}
        }
    })
}

/// The `_meta` block a `2026-07-28` client sends on every request in place of
/// the removed `initialize` handshake (SEP-2575).
fn stateless_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": MODERN,
        "io.modelcontextprotocol/clientInfo": {"name": "roxy-tests", "version": "0.0.0"},
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

/// Complete a legacy handshake and return the session id rmcp issued.
async fn legacy_session(roxy: &Roxy) -> String {
    let response = roxy.post(&initialize_body(LEGACY), &[]).await;
    assert!(response.status().is_success(), "HTTP {}", response.status());
    let session = response
        .headers()
        .get("mcp-session-id")
        .expect("legacy initialize issues Mcp-Session-Id")
        .to_str()
        .expect("session id is ASCII")
        .to_owned();

    let reply = read_message(response).await;
    assert_eq!(
        reply["result"]["protocolVersion"], LEGACY,
        "roxy must negotiate the revision the client asked for, got: {reply}"
    );

    roxy.post(
        &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        &[("mcp-session-id", session.as_str())],
    )
    .await;

    session
}

// --- legacy revision -----------------------------------------------------

/// A `2025-11-25` client keeps everything the revision gives it: a negotiated
/// `initialize`, a session id, and results with none of the `2026-07-28`
/// fields.
#[tokio::test]
async fn legacy_client_lists_and_calls_tools_without_2026_fields() {
    let roxy = spawn_roxy().await;
    let session = legacy_session(&roxy).await;
    let headers = [
        ("mcp-session-id", session.as_str()),
        ("MCP-Protocol-Version", LEGACY),
    ];

    let list = roxy
        .call(
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
            &headers,
        )
        .await;
    assert_eq!(
        list["result"]["tools"][0]["name"], TOOL,
        "upstream tool must reach a legacy client, got: {list}"
    );
    for field in ["resultType", "ttlMs", "cacheScope"] {
        assert!(
            list["result"].get(field).is_none(),
            "{field} is a 2026-07-28 field and must not appear for {LEGACY}, got: {list}"
        );
    }

    let call = roxy
        .call(
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {"name": TOOL, "arguments": {}}
            }),
            &headers,
        )
        .await;
    assert_eq!(
        call["result"]["content"][0]["text"],
        format!("called {TOOL}"),
        "tool call must reach the upstream, got: {call}"
    );
    assert!(
        call["result"].get("resultType").is_none(),
        "resultType must not appear for {LEGACY}, got: {call}"
    );
}

/// Adding `2026-07-28` must not disturb the handshake of any revision that
/// still has one: each legacy client gets back the revision it asked for.
#[tokio::test]
async fn every_legacy_revision_negotiates_itself() {
    let roxy = spawn_roxy().await;

    for version in ["2024-11-05", "2025-03-26", "2025-06-18", LEGACY] {
        let reply = roxy.call(&initialize_body(version), &[]).await;
        assert_eq!(
            reply["result"]["protocolVersion"], version,
            "initialize must echo the negotiated revision, got: {reply}"
        );
    }
}

/// The standalone `GET` SSE stream is part of every revision from `2025-03-26`
/// to `2025-11-25`. The `2026-07-28` guidance to answer `405` addresses servers
/// that support *only* that revision, so a gateway must not adopt it.
#[tokio::test]
async fn legacy_get_stream_is_still_served() {
    let roxy = spawn_roxy().await;
    let session = legacy_session(&roxy).await;

    let response = tokio::time::timeout(
        READ_TIMEOUT,
        roxy.client
            .get(&roxy.url)
            .header("Accept", "text/event-stream")
            .header("mcp-session-id", &session)
            .header("MCP-Protocol-Version", LEGACY)
            .send(),
    )
    .await
    .expect("GET responds within the read timeout")
    .expect("send GET request");

    assert_eq!(
        response.status(),
        200,
        "roxy must keep the legacy GET stream, not answer 405"
    );
    // The stream stays open by design; dropping the response closes it.
}

// --- 2026-07-28 ----------------------------------------------------------

/// A `2026-07-28` client sends no `initialize` and no session id: the
/// revision, its capabilities and its identity all travel in per-request
/// `_meta`, and every result is tagged with `resultType`.
#[tokio::test]
async fn modern_client_works_without_initialize_or_session() {
    let roxy = spawn_roxy().await;

    let list = roxy
        .call(
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/list",
                "params": {"_meta": stateless_meta()}
            }),
            &[
                ("MCP-Protocol-Version", MODERN),
                ("Mcp-Method", "tools/list"),
            ],
        )
        .await;
    assert_eq!(
        list["result"]["tools"][0]["name"], TOOL,
        "upstream tool must reach a modern client, got: {list}"
    );
    assert_eq!(
        list["result"]["resultType"], "complete",
        "every {MODERN} result carries resultType, got: {list}"
    );

    let call = roxy
        .call(
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {"name": TOOL, "arguments": {}, "_meta": stateless_meta()}
            }),
            &[
                ("MCP-Protocol-Version", MODERN),
                ("Mcp-Method", "tools/call"),
                ("Mcp-Name", TOOL),
            ],
        )
        .await;
    assert_eq!(
        call["result"]["content"][0]["text"],
        format!("called {TOOL}"),
        "tool call must reach the upstream, got: {call}"
    );
    assert_eq!(
        call["result"]["resultType"], "complete",
        "a completed tool call reports resultType complete, got: {call}"
    );
}

/// SEP-2243 promotes `method` and `params.name` into headers so an intermediary
/// can route without parsing the body, and requires the server to reject any
/// disagreement between the two copies with `-32020`. roxy inherits that check
/// from rmcp; this pins it, since a gateway that forwards those headers must
/// never pass an unvalidated pair through.
#[tokio::test]
async fn modern_header_body_mismatch_is_rejected() {
    let roxy = spawn_roxy().await;

    let response = roxy
        .post(
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {"name": TOOL, "arguments": {}, "_meta": stateless_meta()}
            }),
            &[
                ("MCP-Protocol-Version", MODERN),
                ("Mcp-Method", "tools/call"),
                ("Mcp-Name", "not-the-tool"),
            ],
        )
        .await;

    assert_eq!(response.status(), 400, "a header mismatch is a bad request");
    let reply = read_message(response).await;
    assert_eq!(
        reply["error"]["code"], -32020,
        "mismatch must be reported as HeaderMismatch, got: {reply}"
    );
}

// --- discovery -----------------------------------------------------------

/// `server/discover` is how a client learns what roxy speaks before committing
/// to a revision. It must advertise every revision roxy actually serves,
/// including the four legacy ones.
#[tokio::test]
async fn discover_advertises_every_supported_revision() {
    let roxy = spawn_roxy().await;

    let reply = roxy
        .call(
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "server/discover",
                "params": {"_meta": stateless_meta()}
            }),
            &[
                ("MCP-Protocol-Version", MODERN),
                ("Mcp-Method", "server/discover"),
            ],
        )
        .await;

    let advertised = reply["result"]["supportedVersions"]
        .as_array()
        .unwrap_or_else(|| panic!("discover reports supportedVersions, got: {reply}"));
    for version in ["2024-11-05", "2025-03-26", "2025-06-18", LEGACY, MODERN] {
        assert!(
            advertised.iter().any(|v| v == version),
            "roxy serves {version} and must advertise it, got: {reply}"
        );
    }
    assert_eq!(
        reply["result"]["capabilities"]["tools"],
        json!({}),
        "discover reports the tools capability roxy advertises, got: {reply}"
    );
}

// --- stdio ---------------------------------------------------------------

/// Run `requests` against a roxy stdio server over an in-memory pipe and return
/// one reply per request that carries an `id`.
///
/// `--transport stdio` is roxy's default and the one desktop clients use, so
/// both eras need coverage there too. Framing is newline-delimited JSON, which
/// is all the stdio transport is.
async fn stdio_replies(requests: &[Value]) -> Vec<Value> {
    use rmcp::ServiceExt;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (server_io, client_io) = tokio::io::duplex(16 * 1024);
    let server = tokio::spawn(async move {
        roxy::server::RoxyServer::new(Arc::new(StubUpstream))
            .serve(server_io)
            .await
            .expect("start stdio server")
            .waiting()
            .await
    });

    let (reader, mut writer) = tokio::io::split(client_io);
    let mut lines = BufReader::new(reader).lines();

    for request in requests {
        writer
            .write_all(format!("{request}\n").as_bytes())
            .await
            .expect("write request");
    }
    writer.flush().await.expect("flush requests");

    let expected = requests.iter().filter(|r| r.get("id").is_some()).count();
    let mut replies = Vec::with_capacity(expected);
    while replies.len() < expected {
        let line = tokio::time::timeout(READ_TIMEOUT, lines.next_line())
            .await
            .expect("reply arrives within the read timeout")
            .expect("read reply line")
            .expect("server closed the stream before replying");
        replies.push(serde_json::from_str(&line).expect("parse reply"));
    }

    drop(writer);
    drop(lines);
    let _ = tokio::time::timeout(READ_TIMEOUT, server).await;
    replies
}

/// A legacy client over stdio: handshake, then a list with no `2026-07-28`
/// fields in the result.
#[tokio::test]
async fn stdio_serves_a_legacy_client() {
    let replies = stdio_replies(&[
        initialize_body(LEGACY),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
    ])
    .await;

    assert_eq!(
        replies[0]["result"]["protocolVersion"], LEGACY,
        "stdio must negotiate the legacy revision, got: {}",
        replies[0]
    );
    assert_eq!(
        replies[1]["result"]["tools"][0]["name"], TOOL,
        "upstream tool must reach a legacy stdio client, got: {}",
        replies[1]
    );
    assert!(
        replies[1]["result"].get("resultType").is_none(),
        "resultType must not appear for {LEGACY}, got: {}",
        replies[1]
    );
}

/// A `2026-07-28` client over stdio sends no handshake at all — the first
/// message is a real request carrying its `_meta`.
#[tokio::test]
async fn stdio_serves_a_stateless_client() {
    let replies = stdio_replies(&[json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": {"_meta": stateless_meta()}
    })])
    .await;

    assert_eq!(
        replies[0]["result"]["tools"][0]["name"], TOOL,
        "upstream tool must reach a stateless stdio client, got: {}",
        replies[0]
    );
    assert_eq!(
        replies[0]["result"]["resultType"], "complete",
        "every {MODERN} result carries resultType, got: {}",
        replies[0]
    );
}
