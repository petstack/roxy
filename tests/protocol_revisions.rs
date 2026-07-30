//! End-to-end coverage of the MCP revisions roxy serves on one endpoint, and of
//! the inbound HTTP policies it enforces in front of them.
//!
//! roxy is a gateway: a `2025-11-25` client and a `2026-07-28` client must both
//! work against the same process, against the same unchanged upstream. These
//! tests drive [`roxy::transport::http_service`] — the exact service the binary
//! serves, configured from the exact defaults the CLI parses — over loopback
//! HTTP with hand-written JSON-RPC, because the assertions are about wire
//! details an SDK client would hide: whether `resultType` appears, whether a
//! session id is issued, whether the standalone `GET` stream still exists,
//! which error code a missing resource gets, and whether SEP-2243 headers are
//! enforced.

use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use roxy::config::Config;
use roxy::executor::{ExecuteContext, UpstreamExecutor};
use roxy::protocol::{
    UpstreamCallResult, UpstreamContent, UpstreamContentResponse, UpstreamDiscoverResponse,
    UpstreamElicitResponse, UpstreamEnvelope, UpstreamError, UpstreamErrorResponse,
    UpstreamPromptDef, UpstreamRequest, UpstreamResourceDef, UpstreamToolDef,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

/// Newest revision that still uses `initialize`, sessions and server-initiated
/// elicitation.
const LEGACY: &str = "2025-11-25";
/// The stateless revision added by issue 0021.
const MODERN: &str = "2026-07-28";

const TOOL: &str = "echo";
/// Tool whose upstream asks for more input, which only some revisions can
/// deliver.
const ELICIT_TOOL: &str = "ask";
const RESOURCE: &str = "mem://note";
/// Resource the upstream reports as missing, to pin the not-found error code.
const MISSING_RESOURCE: &str = "mem://gone";
const PROMPT: &str = "greet";

/// Bound on every read from the server. A POST answered over SSE closes its
/// stream once the reply is delivered, but roxy keeps rmcp's default 15-second
/// keep-alive, so an unbounded read would hang instead of failing if that ever
/// changed.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

// --- stub upstream -------------------------------------------------------

/// Minimal upstream: one tool, one resource, one prompt, plus an eliciting tool
/// and a missing resource. Stands in for the FastCGI/HTTP backend so these tests
/// exercise only roxy's protocol surface.
struct StubUpstream;

impl UpstreamExecutor for StubUpstream {
    async fn execute(
        &self,
        request: &UpstreamEnvelope<'_>,
        _ctx: ExecuteContext<'_>,
    ) -> anyhow::Result<UpstreamCallResult> {
        let text = match &request.request {
            UpstreamRequest::CallTool { name, .. } if *name == ELICIT_TOOL => {
                return Ok(UpstreamCallResult::Elicit(UpstreamElicitResponse {
                    message: "which region?".to_string(),
                    schema: json!({"type": "object", "properties": {}}),
                    context: Some(json!({"step": 1})),
                }));
            }
            UpstreamRequest::CallTool { name, .. } => format!("called {name}"),
            UpstreamRequest::ReadResource { uri } if *uri == MISSING_RESOURCE => {
                return Ok(UpstreamCallResult::Error(UpstreamErrorResponse {
                    error: UpstreamError {
                        code: 404,
                        message: "no such resource".to_string(),
                    },
                }));
            }
            UpstreamRequest::ReadResource { uri } => format!("contents of {uri}"),
            UpstreamRequest::GetPrompt { name, .. } => format!("prompt {name}"),
            // The cancellation notification roxy sends when an elicitation
            // ends without an answer. Acked with no content.
            UpstreamRequest::ElicitationCancelled { .. } => {
                return Ok(UpstreamCallResult::Content(UpstreamContentResponse {
                    content: vec![],
                    structured_content: None,
                }));
            }
            other => anyhow::bail!("unexpected upstream request: {other:?}"),
        };
        Ok(UpstreamCallResult::Content(UpstreamContentResponse {
            content: vec![UpstreamContent::Text { text }],
            structured_content: None,
        }))
    }

    async fn discover(&self) -> anyhow::Result<UpstreamDiscoverResponse> {
        Ok(UpstreamDiscoverResponse {
            tools: vec![
                UpstreamToolDef {
                    name: TOOL.to_string(),
                    title: None,
                    description: Some("echo the tool name".to_string()),
                    input_schema: None,
                    output_schema: None,
                },
                UpstreamToolDef {
                    name: ELICIT_TOOL.to_string(),
                    title: None,
                    description: Some("ask for more input".to_string()),
                    input_schema: None,
                    output_schema: None,
                },
            ],
            resources: vec![UpstreamResourceDef {
                uri: RESOURCE.to_string(),
                name: "note".to_string(),
                title: None,
                description: None,
                mime_type: Some("text/plain".to_string()),
            }],
            prompts: vec![UpstreamPromptDef {
                name: PROMPT.to_string(),
                title: None,
                description: Some("greet someone".to_string()),
                arguments: vec![],
            }],
        })
    }
}

// --- harness -------------------------------------------------------------

/// roxy's own configuration defaults, read from the CLI parser rather than
/// restated here, so these tests pin what a plain `roxy --transport http`
/// actually serves. The policy env vars are cleared for the parse, so a
/// developer who exports them does not see confusing failures.
fn default_config() -> Config {
    temp_env::with_vars_unset(["ROXY_ALLOWED_HOST", "ROXY_MAX_BODY_SIZE"], || {
        Config::parse_from(["roxy", "--upstream", "http://127.0.0.1:1/mcp"])
    })
}

/// A roxy HTTP endpoint on an ephemeral loopback port. Dropping it cancels the
/// server.
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
    spawn_roxy_with(default_config()).await
}

async fn spawn_roxy_with(config: Config) -> Roxy {
    // `allowed_hosts()`, not the raw field: the normalization and the
    // loopback fallback are part of the policy, so the tests have to go
    // through the same accessor the binary does.
    spawn_roxy_raw(config.allowed_hosts(), config.max_body_size).await
}

/// Hand `http_service` a host list verbatim, as an external caller of the
/// library would — without `Config::allowed_hosts()` in between.
async fn spawn_roxy_raw(allowed_hosts: Vec<String>, max_body_size: usize) -> Roxy {
    let cancel = CancellationToken::new();
    let service = roxy::transport::http_service(
        Arc::new(StubUpstream),
        &allowed_hosts,
        max_body_size,
        cancel.child_token(),
    );
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
        tokio::time::timeout(READ_TIMEOUT, request.send())
            .await
            .expect("server responds within the read timeout")
            .expect("send request")
    }

    /// POST and return the JSON-RPC reply to it, asserting a 2xx status first.
    /// A non-2xx carries the server's own explanation, so surface it rather
    /// than just the code.
    async fn call(&self, body: &Value, headers: &[(&str, &str)]) -> Value {
        let response = self.post(body, headers).await;
        let status = response.status();
        if !status.is_success() {
            let detail = response.text().await.unwrap_or_default();
            panic!("HTTP {status} for {body}: {detail}");
        }
        read_reply(response, &body["id"]).await
    }
}

/// Read the JSON-RPC reply carrying `id` from a response, whether it arrived as
/// plain JSON or inside an SSE stream.
///
/// Parses incrementally rather than buffering the whole body: in legacy session
/// mode rmcp emits a priming event before the reply and may hold the stream open
/// afterwards. Selecting by `id` (rather than taking the first frame) means an
/// interleaved notification cannot be mistaken for the reply.
async fn read_reply(mut response: reqwest::Response, id: &Value) -> Value {
    let is_json = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));

    let mut body = String::new();
    loop {
        if !is_json && let Some(reply) = jsonrpc_reply(&body, id) {
            return reply;
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
        jsonrpc_reply(&body, id)
            .unwrap_or_else(|| panic!("no reply for id {id} in SSE body: {body}"))
    }
}

/// The complete SSE event in `body` whose `data:` payload is the JSON-RPC reply
/// to `id`. rmcp's priming event carries an empty `data:` line, so it never
/// parses and is skipped.
fn jsonrpc_reply(body: &str, id: &Value) -> Option<Value> {
    body.split("\n\n")
        .filter_map(|event| event.lines().find_map(|line| line.strip_prefix("data:")))
        .map(str::trim)
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .find(|message| message.get("id") == Some(id))
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

/// A request body as a `2026-07-28` client sends it: the params plus the
/// per-request `_meta` that replaces the handshake.
fn modern_body(id: u32, method: &str, mut params: Value) -> Value {
    params["_meta"] = stateless_meta();
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

/// The headers a `2026-07-28` client sends: the SEP-2243 pair mirroring
/// `method` and, where the method has one, `params.name` / `params.uri`.
fn modern_headers<'a>(method: &'a str, name: Option<&'a str>) -> Vec<(&'a str, &'a str)> {
    let mut headers = vec![("MCP-Protocol-Version", MODERN), ("Mcp-Method", method)];
    if let Some(name) = name {
        headers.push(("Mcp-Name", name));
    }
    headers
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

    let reply = read_reply(response, &json!(1)).await;
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

/// Headers a legacy client sends on every request after the handshake.
fn legacy_headers(session: &str) -> [(&str, &str); 2] {
    [
        ("mcp-session-id", session),
        ("MCP-Protocol-Version", LEGACY),
    ]
}

/// Assert that `result` carries none of the fields `2026-07-28` added.
fn assert_no_modern_fields(result: &Value, what: &str) {
    for field in ["resultType", "ttlMs", "cacheScope"] {
        assert!(
            result.get(field).is_none(),
            "{field} is a 2026-07-28 field and must not appear on {what} for {LEGACY}, got: {result}"
        );
    }
}

// --- legacy revision -----------------------------------------------------

/// A `2025-11-25` client keeps everything the revision gives it: a negotiated
/// `initialize`, a session id, and results with none of the `2026-07-28`
/// fields.
#[tokio::test]
async fn legacy_client_lists_and_calls_tools_without_2026_fields() {
    let roxy = spawn_roxy().await;
    let session = legacy_session(&roxy).await;
    let headers = legacy_headers(&session);

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
    assert_no_modern_fields(&list["result"], "tools/list");

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
    assert_no_modern_fields(&call["result"], "tools/call");
}

/// The resource and prompt surfaces travel the same two-era path as tools, so
/// they need the same check: content from the upstream, and no `2026-07-28`
/// fields on the way back.
#[tokio::test]
async fn legacy_client_reads_resources_and_prompts() {
    let roxy = spawn_roxy().await;
    let session = legacy_session(&roxy).await;
    let headers = legacy_headers(&session);

    let resources = roxy
        .call(
            &json!({"jsonrpc": "2.0", "id": 2, "method": "resources/list", "params": {}}),
            &headers,
        )
        .await;
    assert_eq!(
        resources["result"]["resources"][0]["uri"], RESOURCE,
        "upstream resource must reach a legacy client, got: {resources}"
    );
    assert_no_modern_fields(&resources["result"], "resources/list");

    let read = roxy
        .call(
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "resources/read",
                "params": {"uri": RESOURCE}
            }),
            &headers,
        )
        .await;
    assert_eq!(
        read["result"]["contents"][0]["text"],
        format!("contents of {RESOURCE}"),
        "resource read must reach the upstream, got: {read}"
    );
    assert_no_modern_fields(&read["result"], "resources/read");

    let prompts = roxy
        .call(
            &json!({"jsonrpc": "2.0", "id": 4, "method": "prompts/list", "params": {}}),
            &headers,
        )
        .await;
    assert_eq!(
        prompts["result"]["prompts"][0]["name"], PROMPT,
        "upstream prompt must reach a legacy client, got: {prompts}"
    );
    assert_no_modern_fields(&prompts["result"], "prompts/list");

    let prompt = roxy
        .call(
            &json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "prompts/get",
                "params": {"name": PROMPT}
            }),
            &headers,
        )
        .await;
    assert_eq!(
        prompt["result"]["messages"][0]["content"]["text"],
        format!("prompt {PROMPT}"),
        "prompt must reach the upstream, got: {prompt}"
    );
    assert_no_modern_fields(&prompt["result"], "prompts/get");
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

/// The standalone `GET` SSE stream and `DELETE` teardown are part of every
/// revision from `2025-03-26` to `2025-11-25`. The `2026-07-28` guidance to
/// answer `405` addresses servers that support *only* that revision, so a
/// gateway must not adopt it.
#[tokio::test]
async fn legacy_get_and_delete_are_still_served() {
    let roxy = spawn_roxy().await;
    let session = legacy_session(&roxy).await;

    let stream = tokio::time::timeout(
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
        stream.status(),
        200,
        "roxy must keep the legacy GET stream, not answer 405"
    );
    // The stream stays open by design; dropping the response closes it.
    drop(stream);

    let deleted = tokio::time::timeout(
        READ_TIMEOUT,
        roxy.client
            .delete(&roxy.url)
            .header("mcp-session-id", &session)
            .header("MCP-Protocol-Version", LEGACY)
            .send(),
    )
    .await
    .expect("DELETE responds within the read timeout")
    .expect("send DELETE request");
    assert!(
        deleted.status().is_success(),
        "a legacy client must be able to end its session, got HTTP {}",
        deleted.status()
    );
}

// --- 2026-07-28 ----------------------------------------------------------

/// A `2026-07-28` client sends no `initialize` and no session id: the
/// revision, its capabilities and its identity all travel in per-request
/// `_meta`, and every result is tagged with `resultType`.
#[tokio::test]
async fn modern_client_works_without_initialize_or_session() {
    let roxy = spawn_roxy().await;

    let response = roxy
        .post(
            &modern_body(1, "tools/list", json!({})),
            &modern_headers("tools/list", None),
        )
        .await;
    assert!(
        response.headers().get("mcp-session-id").is_none(),
        "sessions are removed from {MODERN}; roxy must not issue one"
    );
    let list = read_reply(response, &json!(1)).await;
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
            &modern_body(2, "tools/call", json!({"name": TOOL, "arguments": {}})),
            &modern_headers("tools/call", Some(TOOL)),
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

/// Same surfaces as the legacy pair, from the other era.
#[tokio::test]
async fn modern_client_reads_resources_and_prompts() {
    let roxy = spawn_roxy().await;

    let read = roxy
        .call(
            &modern_body(1, "resources/read", json!({"uri": RESOURCE})),
            &modern_headers("resources/read", Some(RESOURCE)),
        )
        .await;
    assert_eq!(
        read["result"]["contents"][0]["text"],
        format!("contents of {RESOURCE}"),
        "resource read must reach the upstream, got: {read}"
    );
    assert_eq!(
        read["result"]["resultType"], "complete",
        "every {MODERN} result carries resultType, got: {read}"
    );

    let prompt = roxy
        .call(
            &modern_body(2, "prompts/get", json!({"name": PROMPT})),
            &modern_headers("prompts/get", Some(PROMPT)),
        )
        .await;
    assert_eq!(
        prompt["result"]["messages"][0]["content"]["text"],
        format!("prompt {PROMPT}"),
        "prompt must reach the upstream, got: {prompt}"
    );
    assert_eq!(
        prompt["result"]["resultType"], "complete",
        "every {MODERN} result carries resultType, got: {prompt}"
    );
}

/// SEP-2243 promotes `method` and `params.name` into headers so an intermediary
/// can route without parsing the body, and requires the server to reject any
/// disagreement between the two copies with `-32020`. roxy inherits that check
/// from rmcp; this pins it, since roxy forwards client headers to the backend and
/// a mismatched pair must not be among them.
///
/// Scope note: rmcp validates `Mcp-Param-*` only when it can resolve a tool
/// schema through `ServerHandler::get_tool`, which roxy does not implement (it
/// discovers tools per request, and that hook is synchronous). Those headers
/// therefore still reach the backend unvalidated — see issue 0025.
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
    let reply = read_reply(response, &json!(1)).await;
    assert_eq!(
        reply["error"]["code"], -32020,
        "mismatch must be reported as HeaderMismatch, got: {reply}"
    );
}

/// `2026-07-28` removed server-initiated requests, so a prompt sent to a modern
/// client is one it can never answer — and roxy's prompt has no timeout, so the
/// call would hang until the client gives up. Until MRTR lands (issue 0022) the
/// call must instead fail fast with an explanation.
#[tokio::test]
async fn modern_client_gets_an_error_instead_of_a_hanging_elicitation() {
    let roxy = spawn_roxy().await;

    let call = roxy
        .call(
            &modern_body(
                1,
                "tools/call",
                json!({"name": ELICIT_TOOL, "arguments": {}}),
            ),
            &modern_headers("tools/call", Some(ELICIT_TOOL)),
        )
        .await;

    assert_eq!(
        call["result"]["isError"],
        json!(true),
        "an undeliverable elicitation must surface as an error result, got: {call}"
    );
    let text = call["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("error result carries text, got: {call}"));
    assert!(
        text.contains("multi round-trip"),
        "the message must explain why the tool cannot run, got: {text}"
    );
}

/// The same hang is reachable without naming `2026-07-28`: a client that puts
/// its revision in `_meta` instead of running `initialize` is served statelessly
/// whatever revision it declares, so there is no channel to route a prompt's
/// answer back through. Deliverability is a property of the request, not of the
/// version string — this is the case a version-only check gets wrong.
#[tokio::test]
async fn inline_lifecycle_request_declaring_a_legacy_revision_still_gets_an_error() {
    let roxy = spawn_roxy().await;

    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": ELICIT_TOOL,
            "arguments": {},
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": LEGACY,
                "io.modelcontextprotocol/clientInfo": {"name": "roxy-tests", "version": "0.0.0"},
                "io.modelcontextprotocol/clientCapabilities": {"elicitation": {}}
            }
        }
    });

    let call = roxy.call(&body, &[("MCP-Protocol-Version", LEGACY)]).await;

    assert_eq!(
        call["result"]["isError"],
        json!(true),
        "a stateless request cannot receive a prompt, whatever revision it declares, got: {call}"
    );
    let text = call["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("error result carries text, got: {call}"));
    assert!(
        text.contains("multi round-trip"),
        "it must fail for the round-trip reason, not some other error, got: {text}"
    );
}

/// The other half of the same rule: a `2026-07-28` client that declares its
/// revision in the header only, with no `_meta` at all, is still served
/// statelessly — so the revision arm of the classifier has to stand on its own.
#[tokio::test]
async fn header_only_modern_request_gets_an_error_instead_of_a_hanging_elicitation() {
    let roxy = spawn_roxy().await;

    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": ELICIT_TOOL, "arguments": {}}
    });

    let call = roxy
        .call(&body, &modern_headers("tools/call", Some(ELICIT_TOOL)))
        .await;

    assert_eq!(
        call["result"]["isError"],
        json!(true),
        "a header-only modern request is stateless too, got: {call}"
    );
    let text = call["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("error result carries text, got: {call}"));
    assert!(
        text.contains("multi round-trip"),
        "it must fail for the round-trip reason, got: {text}"
    );
}

/// SEP-2164: the resource-not-found code follows the negotiated revision —
/// `-32002` for the legacy era, the standard `-32602` from `2026-07-28` on.
/// The same upstream error produces both.
#[tokio::test]
async fn resource_not_found_code_follows_the_revision() {
    let roxy = spawn_roxy().await;

    let session = legacy_session(&roxy).await;
    let legacy = roxy
        .call(
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "resources/read",
                "params": {"uri": MISSING_RESOURCE}
            }),
            &legacy_headers(&session),
        )
        .await;
    assert_eq!(
        legacy["error"]["code"], -32002,
        "{LEGACY} keeps the legacy RESOURCE_NOT_FOUND code, got: {legacy}"
    );

    // Not `call`: a stateless reply carrying `-32602` is also mapped onto
    // HTTP 400, so the JSON-RPC error is the thing to read, not the status.
    let response = roxy
        .post(
            &modern_body(1, "resources/read", json!({"uri": MISSING_RESOURCE})),
            &modern_headers("resources/read", Some(MISSING_RESOURCE)),
        )
        .await;
    let modern = read_reply(response, &json!(1)).await;
    assert_eq!(
        modern["error"]["code"], -32602,
        "{MODERN} uses INVALID_PARAMS for a missing resource, got: {modern}"
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
            &modern_body(1, "server/discover", json!({})),
            &modern_headers("server/discover", None),
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

// --- inbound HTTP policy -------------------------------------------------

/// rmcp 3.x validates the inbound `Host` header, which 1.x did not. roxy's
/// default is loopback-only — the protection that stops a web page from
/// reaching a locally running roxy by DNS rebinding — so a proxied request
/// carrying the client's own hostname is refused until an operator lists it.
#[tokio::test]
async fn foreign_host_is_refused_by_default() {
    let roxy = spawn_roxy().await;

    let response = roxy
        .post(&initialize_body(LEGACY), &[("Host", "roxy.example.com")])
        .await;

    assert_eq!(
        response.status(),
        403,
        "an unlisted Host must be refused, not served"
    );
}

/// …and accepted once it is listed, which is what makes roxy usable behind a
/// reverse proxy that preserves the client's `Host`.
#[tokio::test]
async fn foreign_host_is_accepted_when_allowed() {
    let mut config = default_config();
    config.allowed_host = vec!["roxy.example.com".to_string()];
    let roxy = spawn_roxy_with(config).await;

    let response = roxy
        .post(&initialize_body(LEGACY), &[("Host", "roxy.example.com")])
        .await;

    assert!(
        response.status().is_success(),
        "a listed Host must be served, got HTTP {}",
        response.status()
    );
}

/// `http_service` is public API, so it must not fail open when handed a list
/// that carries no usable host — to rmcp, an empty list means "accept
/// everything" and a blank entry means "match nothing". Goes through
/// `spawn_roxy_raw` on purpose: `Config::allowed_hosts()` would fix the input
/// before the transport ever saw it, which is the layer under test here.
#[tokio::test]
async fn empty_host_list_falls_back_to_loopback_rather_than_opening_up() {
    let roxy = spawn_roxy_raw(vec![String::new(), "   ".to_string()], 4 * 1024 * 1024).await;

    let foreign = roxy
        .post(&initialize_body(LEGACY), &[("Host", "anything.invalid")])
        .await;
    assert_eq!(
        foreign.status(),
        403,
        "a blank allow-list must not accept every host"
    );

    let loopback = roxy.post(&initialize_body(LEGACY), &[]).await;
    assert!(
        loopback.status().is_success(),
        "…while loopback still works, got HTTP {}",
        loopback.status()
    );
}

/// `*` turns the check off, for deployments where something in front of roxy
/// already validates the host.
#[tokio::test]
async fn wildcard_allowed_host_accepts_anything() {
    let mut config = default_config();
    config.allowed_host = vec!["*".to_string()];
    let roxy = spawn_roxy_with(config).await;

    let response = roxy
        .post(&initialize_body(LEGACY), &[("Host", "anything.invalid")])
        .await;

    assert!(
        response.status().is_success(),
        "'*' must accept any Host, got HTTP {}",
        response.status()
    );
}

/// The body cap is roxy's, not the SDK's: `--max-body-size` has to actually
/// reach the transport, or an operator raising it for large tool arguments
/// would silently keep getting `413`.
#[tokio::test]
async fn oversized_body_is_rejected_at_the_configured_limit() {
    let mut config = default_config();
    config.max_body_size = 2048;
    let roxy = spawn_roxy_with(config).await;

    let mut body = initialize_body(LEGACY);
    body["params"]["clientInfo"]["version"] = json!("v".repeat(4096));

    let response = roxy.post(&body, &[]).await;

    assert_eq!(
        response.status(),
        413,
        "a body over --max-body-size must be rejected"
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
    assert_no_modern_fields(&replies[1]["result"], "tools/list");
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
