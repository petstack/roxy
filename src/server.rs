use rmcp::{ServerHandler, model::*};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::executor::{ExecuteContext, UpstreamExecutor};
use crate::protocol::{UpstreamCallResult, UpstreamContent, UpstreamEnvelope, UpstreamRequest};

type McpError = rmcp::ErrorData;

/// Hard cap on how many elicitation rounds a single `call_tool` may drive.
///
/// Each round re-invokes the upstream with the accumulated answers. A backend
/// that returns `Elicit` unconditionally — through a bug or malice — would
/// otherwise spin the loop forever: unbounded client prompts, unbounded growth
/// of the answer accumulator, and a request that never completes. When the cap
/// is exceeded `call_tool` aborts with an error. 32 sits comfortably above any
/// realistic multi-step form while still bounding the blast radius; revisit (or
/// make configurable) if a legitimate workflow ever needs more.
const MAX_ELICITATION_ROUNDS: usize = 32;

/// Fill a 36-byte stack buffer with a fresh UUID v4 hyphenated ascii string.
/// Returns a `&str` borrowed from the caller-supplied buffer so callers can
/// embed a per-request correlation id in an envelope without allocating.
#[doc(hidden)]
pub fn fresh_request_id(buf: &mut [u8; uuid::fmt::Hyphenated::LENGTH]) -> &str {
    Uuid::new_v4().hyphenated().encode_lower(buf)
}

pub struct RoxyServer<E: UpstreamExecutor> {
    executor: E,
}

/// The one client-facing side effect of the `call_tool` elicitation loop:
/// prompt the MCP client and wait for its answer. Abstracting it behind a
/// trait lets the bounded loop (`run_tool_loop`) be unit-tested with a stub
/// client and no live rmcp peer. Production uses [`PeerPrompter`].
///
/// Follows the `UpstreamExecutor` convention: the trait *declaration* spells
/// out an explicit `+ Send` future (a bare `async fn` in a trait cannot express
/// the `Send` bound that `call_tool`'s returned future requires), while
/// implementors may still write the method as an ordinary `async fn` — the
/// compiler verifies each impl's future is `Send` against this bound.
trait ElicitationPrompter {
    fn prompt(
        &self,
        params: CreateElicitationRequestParams,
    ) -> impl std::future::Future<Output = Result<CreateElicitationResult, McpError>> + Send;
}

/// Production [`ElicitationPrompter`] backed by the live MCP client peer.
struct PeerPrompter<'p> {
    peer: &'p rmcp::service::Peer<rmcp::RoleServer>,
}

impl ElicitationPrompter for PeerPrompter<'_> {
    async fn prompt(
        &self,
        params: CreateElicitationRequestParams,
    ) -> Result<CreateElicitationResult, McpError> {
        self.peer.create_elicitation(params).await.map_err(|e| {
            error!("elicitation request failed: {e}");
            McpError::internal_error(format!("elicitation failed: {e}"), None)
        })
    }
}

impl<E: UpstreamExecutor + 'static> RoxyServer<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    /// Discover capabilities from the upstream backend and convert to MCP types.
    async fn discover(&self) -> Result<(Vec<Tool>, Vec<Resource>, Vec<Prompt>), McpError> {
        let discover = self.executor.discover().await.map_err(|e| {
            error!("upstream discover error: {e}");
            McpError::internal_error(format!("upstream discover error: {e}"), None)
        })?;

        let tools = discover
            .tools
            .into_iter()
            .map(|t| {
                let schema = t.input_schema.unwrap_or_default();
                let mut tool = Tool::new(t.name, t.description.unwrap_or_default(), schema);
                if let Some(title) = t.title {
                    tool = tool.with_title(title);
                }
                if let Some(output) = t.output_schema {
                    tool = tool.with_raw_output_schema(std::sync::Arc::new(output));
                }
                tool
            })
            .collect();

        let resources = discover
            .resources
            .into_iter()
            .map(|r| {
                build_raw_resource(r.uri, r.name, r.title, r.description, r.mime_type)
                    .no_annotation()
            })
            .collect();

        let prompts = discover
            .prompts
            .into_iter()
            .map(|p| {
                let mut prompt = Prompt::new(
                    p.name,
                    p.description,
                    Some(
                        p.arguments
                            .into_iter()
                            .map(|a| {
                                let mut arg = PromptArgument::new(a.name);
                                if let Some(title) = a.title {
                                    arg = arg.with_title(title);
                                }
                                if let Some(desc) = a.description {
                                    arg = arg.with_description(desc);
                                }
                                arg = arg.with_required(a.required);
                                arg
                            })
                            .collect(),
                    ),
                );
                if let Some(title) = p.title {
                    prompt = prompt.with_title(title);
                }
                prompt
            })
            .collect();

        Ok((tools, resources, prompts))
    }

    /// Drive a tool call to completion, bounding the elicitation loop.
    ///
    /// Repeatedly invokes the upstream; on each `Elicit` response it prompts
    /// the client (via `prompter`) and feeds the answer back into the next
    /// upstream call. The loop is capped at [`MAX_ELICITATION_ROUNDS`], so a
    /// backend that returns `Elicit` forever cannot spin indefinitely or grow
    /// `elicitation_results` without bound — on exceed it aborts with an
    /// internal error. Split out from `call_tool` (which supplies a live
    /// [`PeerPrompter`]) so the cap can be unit-tested with a stub prompter.
    async fn run_tool_loop<P: ElicitationPrompter>(
        &self,
        request: &CallToolRequestParams,
        session_id: Option<&str>,
        request_id: &str,
        exec_ctx: ExecuteContext<'_>,
        prompter: &P,
    ) -> Result<CallToolResult, McpError> {
        let mut elicitation_results: Vec<serde_json::Value> = Vec::new();
        let mut elicit_context: Option<serde_json::Value> = None;
        let mut elicitation_rounds: usize = 0;

        loop {
            let upstream_request = UpstreamRequest::CallTool {
                name: &request.name,
                arguments: request.arguments.as_ref(),
                elicitation_results: if elicitation_results.is_empty() {
                    None
                } else {
                    Some(&elicitation_results)
                },
                context: elicit_context.as_ref(),
            };
            let envelope = UpstreamEnvelope {
                session_id,
                request_id,
                request: upstream_request,
            };

            let response = self
                .executor
                .execute(&envelope, exec_ctx)
                .await
                .map_err(|e| {
                    error!("upstream executor error: {e}");
                    McpError::internal_error(format!("upstream error: {e}"), None)
                })?;

            match response {
                UpstreamCallResult::Content(c) => {
                    let content: Vec<Content> =
                        c.content.into_iter().map(map_upstream_content).collect();

                    let mut result = CallToolResult::success(content);
                    if c.structured_content.is_some() {
                        result.structured_content = c.structured_content;
                    }

                    return Ok(result);
                }
                UpstreamCallResult::Error(e) => {
                    return Ok(CallToolResult::error(vec![Content::text(e.error.message)]));
                }
                UpstreamCallResult::Elicit(elicit) => {
                    // Bound the loop *before* prompting the client again: a
                    // backend stuck returning `Elicit` must not drive unbounded
                    // prompts or unbounded `elicitation_results` growth. On
                    // exceed we abandon the upstream's elicitation context (the
                    // same as the invalid-schema error path below) rather than
                    // sending a cancellation notification — that path is for a
                    // user decision, and mislabeling a roxy policy abort as one
                    // would be wrong.
                    elicitation_rounds += 1;
                    if elicitation_rounds > MAX_ELICITATION_ROUNDS {
                        error!(
                            "elicitation exceeded {MAX_ELICITATION_ROUNDS} rounds for tool {}",
                            request.name
                        );
                        return Err(McpError::internal_error(
                            format!("elicitation exceeded {MAX_ELICITATION_ROUNDS} rounds"),
                            None,
                        ));
                    }

                    let crate::protocol::UpstreamElicitResponse {
                        message,
                        schema,
                        context: elicit_ctx,
                    } = elicit;
                    let schema: ElicitationSchema =
                        serde_json::from_value(schema).map_err(|e| {
                            error!("invalid elicitation schema from PHP: {e}");
                            McpError::internal_error(
                                format!("invalid elicitation schema: {e}"),
                                None,
                            )
                        })?;

                    let params = CreateElicitationRequestParams::FormElicitationParams {
                        meta: None,
                        message,
                        requested_schema: schema,
                    };

                    let elicit_result = prompter.prompt(params).await?;

                    match elicit_result.action {
                        ElicitationAction::Accept => {
                            if let Some(content) = elicit_result.content {
                                elicitation_results.push(content);
                            }
                            elicit_context = elicit_ctx;
                            // continue loop — re-invoke upstream with results
                        }
                        action @ (ElicitationAction::Decline | ElicitationAction::Cancel) => {
                            // Single match over the two terminal actions yields
                            // both the upstream wire token and the client-facing
                            // message — no `unreachable!()` arms to drift out of
                            // sync if `ElicitationAction` ever grows a variant.
                            let (action_str, msg) = match action {
                                ElicitationAction::Decline => {
                                    ("decline", "User declined to provide information")
                                }
                                ElicitationAction::Cancel => {
                                    ("cancel", "User cancelled the operation")
                                }
                                ElicitationAction::Accept => unreachable!("guarded by outer arm"),
                            };

                            // Notify upstream about cancellation
                            let cancel_request = UpstreamRequest::ElicitationCancelled {
                                name: &request.name,
                                action: action_str,
                                context: elicit_ctx.as_ref(),
                            };
                            let cancel_envelope = UpstreamEnvelope {
                                session_id,
                                request_id,
                                request: cancel_request,
                            };
                            if let Err(e) = self.executor.execute(&cancel_envelope, exec_ctx).await
                            {
                                warn!(
                                    "failed to notify upstream about elicitation cancellation: {e}"
                                );
                            }

                            return Ok(CallToolResult::error(vec![Content::text(msg)]));
                        }
                    }
                }
            }
        }
    }
}

/// Returns `true` for header names that must not be forwarded to the
/// upstream backend: hop-by-hop headers (RFC 7230 §6.1) and headers that
/// roxy itself manages on the outgoing request (Host, Content-Type,
/// Content-Length).
fn is_dropped_header(name: &str) -> bool {
    // `eq_ignore_ascii_case` avoids allocating a lowercase copy on the
    // hot path of every incoming header.
    const DROPPED: &[&str] = &[
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "host",
        "content-type",
        "content-length",
        // `Proxy` is never a legitimate request header; forwarding it to
        // a CGI backend turns into `HTTP_PROXY` and triggers httpoxy
        // (CVE-2016-5385), letting a client redirect the backend's
        // outbound HTTP traffic.
        "proxy",
    ];
    DROPPED
        .iter()
        .any(|dropped| name.eq_ignore_ascii_case(dropped))
}

/// Build the forward-header set by copying every entry from `incoming`
/// that is not filtered by [`is_dropped_header`]. Header names are
/// preserved exactly as received so the upstream sees the canonical
/// casing it expects.
fn filter_forward_headers(incoming: &http::HeaderMap) -> http::HeaderMap {
    let mut out = http::HeaderMap::with_capacity(incoming.len());
    for (name, value) in incoming {
        if !is_dropped_header(name.as_str()) {
            // `append` (not `insert`) is intentional — a client may
            // legitimately send the same header name twice (e.g. a
            // multi-valued `X-Forwarded-For`) and we want to preserve
            // every entry.
            out.append(name.clone(), value.clone());
        }
    }
    out
}

fn extract_session_id(context: &rmcp::service::RequestContext<rmcp::RoleServer>) -> Option<String> {
    context
        .extensions
        .get::<http::request::Parts>()
        .and_then(|parts| parts.headers.get("mcp-session-id"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned())
}

/// Pull incoming HTTP headers from the rmcp request context (populated
/// by the streamable-HTTP transport) and return the forward-header set.
/// Returns `None` under `--transport stdio`, where no `http::request::Parts`
/// is attached to the context extensions.
fn extract_forward_headers(
    context: &rmcp::service::RequestContext<rmcp::RoleServer>,
) -> Option<http::HeaderMap> {
    let parts = context.extensions.get::<http::request::Parts>()?;
    Some(filter_forward_headers(&parts.headers))
}

/// Build a `RawResource` from upstream resource-link fields, applying the
/// optional `title` / `description` / `mime_type` uniformly. Centralizes the
/// construction that was otherwise duplicated across `discover`,
/// `map_upstream_content`, and `get_prompt` (issue 0014). Returns the
/// un-annotated `RawResource`; callers that need an annotated `Resource`
/// finish with `.no_annotation()`.
fn build_raw_resource(
    uri: String,
    name: String,
    title: Option<String>,
    description: Option<String>,
    mime_type: Option<String>,
) -> RawResource {
    let mut raw = RawResource::new(uri, name);
    if let Some(title) = title {
        raw = raw.with_title(title);
    }
    if let Some(description) = description {
        raw.description = Some(description);
    }
    if let Some(mime_type) = mime_type {
        raw.mime_type = Some(mime_type);
    }
    raw
}

fn map_upstream_content(item: UpstreamContent) -> Content {
    match item {
        UpstreamContent::Text { text } => Content::text(text),
        UpstreamContent::ResourceLink {
            uri,
            name,
            title,
            description,
            mime_type,
        } => Content::resource_link(build_raw_resource(uri, name, title, description, mime_type)),
    }
}

/// Map upstream content to MCP [`ResourceContents`] for a resource read.
///
/// `ResourceContents` has only text and blob variants — there is no link
/// shape — so a [`UpstreamContent::ResourceLink`] is surfaced as text content
/// that drops nothing:
///
/// - the contents carry the link's **own** `uri` (so a client can follow it);
/// - the link's `mime_type`, when present, is set on the typed `mime_type`
///   field where a client expects it (not only in the body);
/// - the body is a human-readable summary listing every populated field
///   (`name`, `title`, `description`, `mime_type`).
///
/// The body is presentational, not a stable parse target — clients should read
/// the URI and MIME type from the typed fields. Plain text content keeps
/// `fallback_uri` (the requested resource URI) as before.
fn map_upstream_resource_content(item: UpstreamContent, fallback_uri: &str) -> ResourceContents {
    match item {
        UpstreamContent::Text { text } => ResourceContents::text(text, fallback_uri.to_string()),
        UpstreamContent::ResourceLink {
            uri,
            name,
            title,
            description,
            mime_type,
        } => {
            let mut body = format!("Resource link: {name}\nURI: {uri}");
            if let Some(t) = &title {
                body.push_str("\nTitle: ");
                body.push_str(t);
            }
            if let Some(d) = &description {
                body.push_str("\nDescription: ");
                body.push_str(d);
            }
            if let Some(m) = &mime_type {
                body.push_str("\nMIME type: ");
                body.push_str(m);
            }
            // The contents URI is the link target itself, so a client can
            // follow it — not the placeholder string this used to emit. Carry
            // the link's real MIME type on the typed field too, rather than
            // leaving the `text` default that `ResourceContents::text` sets.
            let contents = ResourceContents::text(body, uri);
            match mime_type {
                Some(m) => contents.with_mime_type(m),
                None => contents,
            }
        }
    }
}

impl<E: UpstreamExecutor + 'static> ServerHandler for RoxyServer<E> {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_server_info(Implementation::new("roxy", env!("CARGO_PKG_VERSION")))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let (tools, _, _) = self.discover().await?;
        Ok(ListToolsResult {
            tools,
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        info!("call_tool: {}", request.name);

        let session_id = extract_session_id(&context);
        let session_id_ref = session_id.as_deref();
        let mut request_id_buf = [0u8; uuid::fmt::Hyphenated::LENGTH];
        let request_id = fresh_request_id(&mut request_id_buf);
        let forward_headers = extract_forward_headers(&context);
        let exec_ctx = ExecuteContext {
            forward_headers: forward_headers.as_ref(),
        };

        let prompter = PeerPrompter {
            peer: &context.peer,
        };
        self.run_tool_loop(&request, session_id_ref, request_id, exec_ctx, &prompter)
            .await
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let (_, resources, _) = self.discover().await?;
        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        info!("read_resource: {}", request.uri);

        let session_id = extract_session_id(&context);
        let mut request_id_buf = [0u8; uuid::fmt::Hyphenated::LENGTH];
        let request_id = fresh_request_id(&mut request_id_buf);
        let forward_headers = extract_forward_headers(&context);
        let exec_ctx = ExecuteContext {
            forward_headers: forward_headers.as_ref(),
        };
        let upstream_request = UpstreamRequest::ReadResource { uri: &request.uri };
        let envelope = UpstreamEnvelope {
            session_id: session_id.as_deref(),
            request_id,
            request: upstream_request,
        };

        let response = self
            .executor
            .execute(&envelope, exec_ctx)
            .await
            .map_err(|e| {
                error!("upstream executor error: {e}");
                McpError::internal_error(format!("upstream error: {e}"), None)
            })?;

        match response {
            UpstreamCallResult::Content(c) => {
                let contents: Vec<ResourceContents> = c
                    .content
                    .into_iter()
                    .map(|item| map_upstream_resource_content(item, &request.uri))
                    .collect();
                Ok(ReadResourceResult::new(contents))
            }
            UpstreamCallResult::Error(e) => {
                Err(McpError::resource_not_found(e.error.message, None))
            }
            UpstreamCallResult::Elicit(_) => Err(McpError::internal_error(
                "elicitation not supported in read_resource",
                None,
            )),
        }
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        let (_, _, prompts) = self.discover().await?;
        Ok(ListPromptsResult {
            prompts,
            next_cursor: None,
            meta: None,
        })
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        info!("get_prompt: {}", request.name);

        let session_id = extract_session_id(&context);
        let mut request_id_buf = [0u8; uuid::fmt::Hyphenated::LENGTH];
        let request_id = fresh_request_id(&mut request_id_buf);
        let forward_headers = extract_forward_headers(&context);
        let exec_ctx = ExecuteContext {
            forward_headers: forward_headers.as_ref(),
        };
        let upstream_request = UpstreamRequest::GetPrompt {
            name: &request.name,
            arguments: request.arguments.as_ref(),
        };
        let envelope = UpstreamEnvelope {
            session_id: session_id.as_deref(),
            request_id,
            request: upstream_request,
        };

        let response = self
            .executor
            .execute(&envelope, exec_ctx)
            .await
            .map_err(|e| {
                error!("upstream executor error: {e}");
                McpError::internal_error(format!("upstream error: {e}"), None)
            })?;

        match response {
            UpstreamCallResult::Content(c) => {
                let messages: Vec<PromptMessage> = c
                    .content
                    .into_iter()
                    .map(|item| match item {
                        UpstreamContent::Text { text } => {
                            PromptMessage::new_text(PromptMessageRole::Assistant, text)
                        }
                        UpstreamContent::ResourceLink {
                            uri,
                            name,
                            title,
                            description,
                            mime_type,
                        } => PromptMessage::new_resource_link(
                            PromptMessageRole::Assistant,
                            build_raw_resource(uri, name, title, description, mime_type)
                                .no_annotation(),
                        ),
                    })
                    .collect();
                Ok(GetPromptResult::new(messages))
            }
            UpstreamCallResult::Error(e) => Err(McpError::invalid_params(e.error.message, None)),
            UpstreamCallResult::Elicit(_) => Err(McpError::internal_error(
                "elicitation not supported in get_prompt",
                None,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{
        UpstreamContentResponse, UpstreamDiscoverResponse, UpstreamElicitResponse,
    };
    use http::header::{HeaderMap, HeaderName, HeaderValue};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn is_dropped_header_drops_hop_by_hop() {
        for name in [
            "connection",
            "Keep-Alive",
            "Proxy-Authenticate",
            "proxy-authorization",
            "te",
            "trailer",
            "Transfer-Encoding",
            "upgrade",
        ] {
            assert!(is_dropped_header(name), "expected {name} to be dropped");
        }
    }

    #[test]
    fn is_dropped_header_drops_roxy_managed() {
        for name in ["Host", "content-type", "Content-Length"] {
            assert!(is_dropped_header(name), "expected {name} to be dropped");
        }
    }

    #[test]
    fn is_dropped_header_drops_proxy_httpoxy() {
        // CVE-2016-5385: a `Proxy` request header forwarded to a CGI
        // backend becomes `HTTP_PROXY` and hijacks outbound traffic.
        for name in ["proxy", "Proxy", "PROXY"] {
            assert!(is_dropped_header(name), "expected {name} to be dropped");
        }
    }

    #[test]
    fn is_dropped_header_keeps_pass_through_headers() {
        for name in [
            "Authorization",
            "Cookie",
            "X-My-Custom",
            "Accept-Language",
            "User-Agent",
            "mcp-session-id",
        ] {
            assert!(!is_dropped_header(name), "expected {name} to be kept");
        }
    }

    #[test]
    fn filter_forward_headers_drops_hop_by_hop_and_keeps_the_rest() {
        let mut incoming = HeaderMap::new();
        incoming.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer xyz"),
        );
        incoming.insert(
            HeaderName::from_static("x-my-custom"),
            HeaderValue::from_static("value"),
        );
        incoming.insert(
            HeaderName::from_static("host"),
            HeaderValue::from_static("mcp.example.com"),
        );
        incoming.insert(
            HeaderName::from_static("connection"),
            HeaderValue::from_static("keep-alive"),
        );
        incoming.insert(
            HeaderName::from_static("content-length"),
            HeaderValue::from_static("123"),
        );
        incoming.insert(
            HeaderName::from_static("mcp-session-id"),
            HeaderValue::from_static("sess-1"),
        );

        let filtered = filter_forward_headers(&incoming);

        assert_eq!(filtered.len(), 3);
        assert_eq!(filtered.get("authorization").unwrap(), "Bearer xyz");
        assert_eq!(filtered.get("x-my-custom").unwrap(), "value");
        assert_eq!(filtered.get("mcp-session-id").unwrap(), "sess-1");
        assert!(filtered.get("host").is_none());
        assert!(filtered.get("connection").is_none());
        assert!(filtered.get("content-length").is_none());
    }

    #[test]
    fn filter_forward_headers_handles_empty_input() {
        let incoming = HeaderMap::new();
        let filtered = filter_forward_headers(&incoming);
        assert!(filtered.is_empty());
    }

    #[test]
    fn map_upstream_resource_content_preserves_resource_link() {
        // Regression for #0005: a resource-link read response must carry the
        // link's real URI and drop none of its metadata, not collapse to the
        // old "[resource link]" placeholder.
        let item = UpstreamContent::ResourceLink {
            uri: "roxy://docs/readme".to_string(),
            name: "readme".to_string(),
            title: Some("README".to_string()),
            description: Some("project intro".to_string()),
            mime_type: Some("text/markdown".to_string()),
        };

        let contents = map_upstream_resource_content(item, "roxy://requested");

        match contents {
            ResourceContents::TextResourceContents {
                uri,
                text,
                mime_type,
                ..
            } => {
                // The contents URI is the link target, not the requested URI
                // and not a placeholder.
                assert_eq!(uri, "roxy://docs/readme");
                assert_ne!(text, "[resource link]");
                // The link's real MIME type lives on the typed field, not just
                // in the prose body.
                assert_eq!(mime_type.as_deref(), Some("text/markdown"));
                // Every field survives in the body.
                assert!(text.contains("roxy://docs/readme"), "uri: {text}");
                assert!(text.contains("readme"), "name: {text}");
                assert!(text.contains("README"), "title: {text}");
                assert!(text.contains("project intro"), "description: {text}");
                assert!(text.contains("text/markdown"), "mime_type: {text}");
            }
            other => panic!("expected text contents, got {other:?}"),
        }
    }

    #[test]
    fn map_upstream_resource_content_omits_absent_link_fields() {
        // Only the required fields are present; optional ones must not leak
        // empty labels into the body.
        let item = UpstreamContent::ResourceLink {
            uri: "roxy://x".to_string(),
            name: "x".to_string(),
            title: None,
            description: None,
            mime_type: None,
        };

        match map_upstream_resource_content(item, "roxy://requested") {
            ResourceContents::TextResourceContents { uri, text, .. } => {
                assert_eq!(uri, "roxy://x");
                assert!(text.contains("roxy://x"));
                assert!(!text.contains("Title:"), "no empty title label: {text}");
                assert!(
                    !text.contains("Description:"),
                    "no empty description label: {text}"
                );
                assert!(!text.contains("MIME type:"), "no empty mime label: {text}");
            }
            other => panic!("expected text contents, got {other:?}"),
        }
    }

    #[test]
    fn map_upstream_resource_content_text_uses_fallback_uri() {
        // Plain text content is unchanged: it keeps the requested resource URI.
        let item = UpstreamContent::Text {
            text: "hello".to_string(),
        };

        match map_upstream_resource_content(item, "roxy://requested") {
            ResourceContents::TextResourceContents { uri, text, .. } => {
                assert_eq!(uri, "roxy://requested");
                assert_eq!(text, "hello");
            }
            other => panic!("expected text contents, got {other:?}"),
        }
    }

    #[test]
    fn filter_forward_headers_preserves_multi_value_headers() {
        // Guards the `append`-not-`insert` decision in filter_forward_headers.
        // A future refactor to `insert` (which replaces) would silently drop
        // the first value and this test would catch it.
        let mut incoming = HeaderMap::new();
        incoming.append(
            HeaderName::from_static("x-forwarded-for"),
            HeaderValue::from_static("10.0.0.1"),
        );
        incoming.append(
            HeaderName::from_static("x-forwarded-for"),
            HeaderValue::from_static("10.0.0.2"),
        );

        let filtered = filter_forward_headers(&incoming);

        let values: Vec<&str> = filtered
            .get_all("x-forwarded-for")
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect();
        assert_eq!(values, vec!["10.0.0.1", "10.0.0.2"]);
    }

    // --- RawResource construction helper (issue 0014) ---

    #[test]
    fn build_raw_resource_sets_all_optional_fields() {
        let raw = build_raw_resource(
            "file:///a.txt".to_string(),
            "a.txt".to_string(),
            Some("Title".to_string()),
            Some("Desc".to_string()),
            Some("text/plain".to_string()),
        );

        assert_eq!(raw.uri, "file:///a.txt");
        assert_eq!(raw.name, "a.txt");
        assert_eq!(raw.title.as_deref(), Some("Title"));
        assert_eq!(raw.description.as_deref(), Some("Desc"));
        assert_eq!(raw.mime_type.as_deref(), Some("text/plain"));
    }

    #[test]
    fn build_raw_resource_leaves_optional_fields_unset_when_none() {
        let raw = build_raw_resource(
            "file:///b.bin".to_string(),
            "b.bin".to_string(),
            None,
            None,
            None,
        );

        assert_eq!(raw.uri, "file:///b.bin");
        assert_eq!(raw.name, "b.bin");
        assert!(raw.title.is_none());
        assert!(raw.description.is_none());
        assert!(raw.mime_type.is_none());
    }

    // --- Bounded elicitation loop (issue 0002) ---

    /// Scripted upstream for `run_tool_loop` tests: returns `Elicit` for the
    /// first `elicit_rounds` `CallTool` invocations, then `Content`. Pass
    /// `usize::MAX` to model a backend that elicits forever. The
    /// `ElicitationCancelled` notification (not a `CallTool`) is acked with
    /// empty content. `calls` counts every `CallTool` invocation.
    struct StubExecutor {
        calls: Arc<AtomicUsize>,
        elicit_rounds: usize,
    }

    impl StubExecutor {
        /// Returns the executor and a shared handle to its `CallTool` counter.
        fn new(elicit_rounds: usize) -> (Self, Arc<AtomicUsize>) {
            let calls = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    calls: Arc::clone(&calls),
                    elicit_rounds,
                },
                calls,
            )
        }
    }

    impl UpstreamExecutor for StubExecutor {
        async fn execute(
            &self,
            request: &UpstreamEnvelope<'_>,
            _ctx: ExecuteContext<'_>,
        ) -> anyhow::Result<UpstreamCallResult> {
            if !matches!(request.request, UpstreamRequest::CallTool { .. }) {
                // ElicitationCancelled notification — ack and move on.
                return Ok(UpstreamCallResult::Content(UpstreamContentResponse {
                    content: vec![],
                    structured_content: None,
                }));
            }
            let prior = self.calls.fetch_add(1, Ordering::SeqCst);
            if prior < self.elicit_rounds {
                Ok(UpstreamCallResult::Elicit(UpstreamElicitResponse {
                    message: "need more input".to_string(),
                    // Minimal schema that deserializes into ElicitationSchema so
                    // the loop reaches the prompt instead of erroring on an
                    // invalid schema.
                    schema: serde_json::json!({"type": "object", "properties": {}}),
                    context: None,
                }))
            } else {
                Ok(UpstreamCallResult::Content(UpstreamContentResponse {
                    content: vec![UpstreamContent::Text {
                        text: "done".to_string(),
                    }],
                    structured_content: None,
                }))
            }
        }

        async fn discover(&self) -> anyhow::Result<UpstreamDiscoverResponse> {
            anyhow::bail!("discover is not exercised by run_tool_loop tests")
        }
    }

    /// Stub client prompter: records how many times the client was prompted and
    /// returns a fixed action (with dummy content for `Accept`).
    struct StubPrompter {
        prompts: Arc<AtomicUsize>,
        action: ElicitationAction,
    }

    impl StubPrompter {
        /// Returns the prompter and a shared handle to its prompt counter.
        fn new(action: ElicitationAction) -> (Self, Arc<AtomicUsize>) {
            let prompts = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    prompts: Arc::clone(&prompts),
                    action,
                },
                prompts,
            )
        }
    }

    impl ElicitationPrompter for StubPrompter {
        async fn prompt(
            &self,
            _params: CreateElicitationRequestParams,
        ) -> Result<CreateElicitationResult, McpError> {
            self.prompts.fetch_add(1, Ordering::SeqCst);
            let content = matches!(self.action, ElicitationAction::Accept)
                .then(|| serde_json::json!({"answer": "yes"}));
            Ok(CreateElicitationResult {
                action: self.action.clone(),
                content,
            })
        }
    }

    fn tool_request(name: &str) -> CallToolRequestParams {
        let mut request = CallToolRequestParams::default();
        request.name = name.to_string().into();
        request
    }

    /// The core stability guarantee: a backend that returns `Elicit`
    /// unconditionally is stopped at the cap with an error rather than spinning
    /// forever. The client is prompted exactly MAX times and the upstream is
    /// invoked exactly MAX + 1 times (the extra round trips the cap), so neither
    /// the prompts nor the answer accumulator grow without bound.
    #[tokio::test]
    async fn run_tool_loop_caps_runaway_elicitation() {
        let (executor, calls) = StubExecutor::new(usize::MAX);
        let server = RoxyServer::new(executor);
        let (prompter, prompts) = StubPrompter::new(ElicitationAction::Accept);

        let request = tool_request("loops_forever");
        let result = server
            .run_tool_loop(
                &request,
                None,
                "req-cap",
                ExecuteContext::default(),
                &prompter,
            )
            .await;

        let err = result.expect_err("a runaway elicitation must abort with an error");
        assert!(
            err.message.contains("exceeded") && err.message.contains("rounds"),
            "error must name the cap, got: {}",
            err.message
        );
        assert_eq!(
            prompts.load(Ordering::SeqCst),
            MAX_ELICITATION_ROUNDS,
            "client must be prompted exactly the cap number of times"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            MAX_ELICITATION_ROUNDS + 1,
            "upstream is invoked once past the cap, then the loop aborts"
        );
    }

    /// A legitimate multi-step elicitation under the cap completes normally and
    /// returns the upstream's content — the cap does not interfere.
    #[tokio::test]
    async fn run_tool_loop_completes_within_cap() {
        let (executor, _calls) = StubExecutor::new(3);
        let server = RoxyServer::new(executor);
        let (prompter, prompts) = StubPrompter::new(ElicitationAction::Accept);

        let request = tool_request("three_steps");
        let result = server
            .run_tool_loop(
                &request,
                None,
                "req-ok",
                ExecuteContext::default(),
                &prompter,
            )
            .await
            .expect("a bounded elicitation flow must succeed");

        assert_eq!(result.is_error, Some(false), "result must be a success");
        assert_eq!(
            prompts.load(Ordering::SeqCst),
            3,
            "client prompted once per elicitation round"
        );
    }

    /// A backend that returns content immediately never prompts the client.
    #[tokio::test]
    async fn run_tool_loop_returns_without_elicitation() {
        let (executor, _calls) = StubExecutor::new(0);
        let server = RoxyServer::new(executor);
        let (prompter, prompts) = StubPrompter::new(ElicitationAction::Accept);

        let request = tool_request("no_elicit");
        let result = server
            .run_tool_loop(
                &request,
                None,
                "req-direct",
                ExecuteContext::default(),
                &prompter,
            )
            .await
            .expect("direct content must succeed");

        assert_eq!(result.is_error, Some(false));
        assert_eq!(
            prompts.load(Ordering::SeqCst),
            0,
            "no elicitation means no client prompt"
        );
    }

    /// Concatenate the text content of a tool result, for asserting the
    /// client-facing message.
    fn result_text(result: &CallToolResult) -> String {
        result
            .content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.clone()))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Shared body for the two terminal-action tests. A backend that would
    /// elicit forever is short-circuited by the user's `action` on the very
    /// first prompt: the loop returns an error *result* (not a transport
    /// error), surfaces the action-specific message, and stops calling the
    /// upstream — proving the cap is not the loop's only exit.
    async fn assert_terminal_action(action: ElicitationAction, expected_msg_fragment: &str) {
        let (executor, calls) = StubExecutor::new(usize::MAX);
        let server = RoxyServer::new(executor);
        let (prompter, prompts) = StubPrompter::new(action);

        let request = tool_request("terminated");
        let result = server
            .run_tool_loop(
                &request,
                None,
                "req-terminal",
                ExecuteContext::default(),
                &prompter,
            )
            .await
            .expect("a terminal action yields an error result, not a transport error");

        assert_eq!(result.is_error, Some(true), "must map to an error result");
        assert!(
            result_text(&result)
                .to_lowercase()
                .contains(expected_msg_fragment),
            "message must reflect the action, got: {}",
            result_text(&result)
        );
        assert_eq!(
            prompts.load(Ordering::SeqCst),
            1,
            "user is prompted exactly once"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "no further CallTool after a terminal action"
        );
    }

    #[tokio::test]
    async fn run_tool_loop_stops_on_decline() {
        assert_terminal_action(ElicitationAction::Decline, "declined").await;
    }

    #[tokio::test]
    async fn run_tool_loop_stops_on_cancel() {
        assert_terminal_action(ElicitationAction::Cancel, "cancelled").await;
    }
}
