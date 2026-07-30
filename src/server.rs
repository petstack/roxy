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

/// Upstream `action` token for an elicitation roxy abandoned itself, as
/// opposed to `decline`/`cancel`, which report a user's decision. Part of the
/// backend contract — see `docs/en/backend-api.md`.
const ACTION_UNSUPPORTED: &str = "unsupported";

/// Reason text for a `2026-07-28`-style request. Reads as the tail of
/// "This tool needs more information, but …".
const REASON_NO_MRTR: &str = "this client's MCP revision replaced server-initiated elicitation \
     with multi round-trip requests, which roxy does not implement yet (call the tool from a \
     client on MCP 2025-06-18 … 2025-11-25)";

/// Reason text for a client that never declared the capability, or declared it
/// without form support.
const REASON_NO_CAPABILITY: &str = "the client did not declare support for form elicitation, so \
     the MCP specification forbids prompting it";

/// Reason text for a revision that predates elicitation entirely.
const REASON_NO_ELICITATION: &str =
    "this client's MCP revision predates elicitation, which arrived in 2025-06-18";

/// Whether roxy may prompt this client in the middle of a tool call.
///
/// Deliverability is a property of the individual request, not of roxy's
/// configuration, so it is classified per call by [`Elicitation::for_request`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Elicitation {
    /// The client can receive `elicitation/create` and answer it.
    ServerInitiated,
    /// It cannot. Prompting anyway would emit a request that never gets a
    /// reply, and since the prompt has no timeout the tool call would never
    /// complete — so the prompt must not be sent at all. The payload is the
    /// reason, for the log and for the client-facing error.
    Blocked(&'static str),
}

impl Elicitation {
    /// Classify one request.
    ///
    /// Two things independently rule a prompt out:
    ///
    /// - **No round trip.** `2026-07-28` removed server-initiated requests in
    ///   favour of MRTR. Version alone is not the test, though: over HTTP, rmcp
    ///   serves any request carrying the full inline-lifecycle `_meta`
    ///   statelessly — *whatever* revision that `_meta` names — and a stateless
    ///   request has no channel to route a client response back through. So a
    ///   request that declares `2025-11-25` inline is just as undeliverable as a
    ///   modern one, which is why `inline_lifecycle` is a separate input rather
    ///   than being inferred from the revision. The revision arm still matters on
    ///   its own: a client may declare `2026-07-28` in the
    ///   `MCP-Protocol-Version` header with no `_meta` at all.
    ///
    ///   Under stdio an inline-lifecycle opener keeps a bidirectional peer, so a
    ///   prompt would in fact be deliverable there. roxy cannot tell the two
    ///   apart — the flag that would say so is `pub(crate)` in rmcp — so it
    ///   declines to prompt in both cases. That costs a stdio client on an old
    ///   revision the ability to elicit through the inline lifecycle, and buys
    ///   never hanging a call; the alternative errs the other way.
    /// - **No elicitation at all.** It only exists from `2025-06-18` onward.
    /// - **No form support.** From `2025-06-18` the spec makes prompting a
    ///   client that did not declare `elicitation` a MUST NOT. roxy only sends
    ///   *form* prompts, so a client declaring URL mode alone is no more
    ///   promptable than one declaring nothing; per rmcp, an
    ///   `ElicitationCapability` with neither mode set means form.
    ///
    /// A missing revision is treated as legacy: it means the peer never
    /// announced one, which only happens on the pre-`2026-07-28` handshake
    /// path, where preserving the old behaviour is the safe direction.
    fn for_request(
        revision: Option<&ProtocolVersion>,
        inline_lifecycle: bool,
        capabilities: Option<&ClientCapabilities>,
    ) -> Self {
        let no_round_trip = inline_lifecycle
            || revision.is_some_and(|revision| *revision >= ProtocolVersion::V_2026_07_28);
        if no_round_trip {
            return Self::Blocked(REASON_NO_MRTR);
        }
        if revision.is_some_and(|revision| *revision < ProtocolVersion::V_2025_06_18) {
            return Self::Blocked(REASON_NO_ELICITATION);
        }
        let form_capable = capabilities
            .and_then(|capabilities| capabilities.elicitation.as_ref())
            .is_some_and(|elicitation| elicitation.form.is_some() || elicitation.url.is_none());
        if !form_capable {
            return Self::Blocked(REASON_NO_CAPABILITY);
        }
        Self::ServerInitiated
    }
}

/// What to do after the upstream asked for more input.
enum Resolution {
    /// The user answered; feed the content back into the next upstream call.
    Answered(Option<serde_json::Value>),
    /// No answer is coming. The upstream is told `action` — so it can drop the
    /// form it is holding — and then the client gets `outcome`.
    Ended {
        action: &'static str,
        outcome: Ended,
    },
}

/// How a finished-without-an-answer elicitation is reported to the client.
enum Ended {
    /// A decision was made — by the user, or by roxy on their behalf. The tool
    /// call itself succeeded in the protocol sense, carrying an error *result*.
    Result(String),
    /// roxy could not carry the exchange through at all. That is a fault, so it
    /// surfaces as a JSON-RPC error.
    Failed(McpError),
}

/// Decide what an upstream `Elicit` response resolves to, prompting the client
/// only when the request can carry one.
///
/// Split out of [`RoxyServer::run_tool_loop`] so the loop body stays a
/// dispatch: this function owns the round cap, the schema conversion, the one
/// client-facing side effect, and the mapping from the outcome to what the loop
/// has to do next.
///
/// Every way this ends without an answer resolves to [`Resolution::Ended`], so
/// the caller always gets the chance to tell the upstream — the one exception is
/// the round cap, which returns `Err` and is the *only* `Err` this function
/// produces. See the comment on that branch for why it stays silent.
async fn resolve_elicitation<P: ElicitationPrompter>(
    tool: &str,
    message: String,
    schema: serde_json::Value,
    elicitation: Elicitation,
    rounds: &mut usize,
    prompter: &P,
) -> Result<Resolution, McpError> {
    if let Elicitation::Blocked(reason) = elicitation {
        warn!("tool {tool} requested elicitation, but {reason}; aborting the call");
        return Ok(Resolution::Ended {
            action: ACTION_UNSUPPORTED,
            outcome: Ended::Result(format!("This tool needs more information, but {reason}.")),
        });
    }

    // Bound the loop *before* prompting the client again: a backend stuck
    // returning `Elicit` must not drive unbounded prompts or unbounded
    // `elicitation_results` growth. This is the one exit that tells the upstream
    // nothing, deliberately: the backend being aborted is the one misbehaving,
    // and it is already ignoring the answers it gets, so there is nothing a
    // notification would let it do. Every other exit reports.
    *rounds += 1;
    if *rounds > MAX_ELICITATION_ROUNDS {
        error!("elicitation exceeded {MAX_ELICITATION_ROUNDS} rounds for tool {tool}");
        return Err(McpError::internal_error(
            format!("elicitation exceeded {MAX_ELICITATION_ROUNDS} rounds"),
            None,
        ));
    }

    let requested_schema: ElicitationSchema = match serde_json::from_value(schema) {
        Ok(schema) => schema,
        Err(e) => {
            // The upstream's own schema is unusable, so it never gets an answer
            // — but it is still holding the form, so it is still told.
            error!("invalid elicitation schema from upstream: {e}");
            return Ok(Resolution::Ended {
                action: ACTION_UNSUPPORTED,
                outcome: Ended::Failed(McpError::internal_error(
                    format!("invalid elicitation schema: {e}"),
                    None,
                )),
            });
        }
    };

    let result = match prompter
        .prompt(ElicitRequestParams::FormElicitationParams {
            meta: None,
            message,
            requested_schema,
        })
        .await
    {
        Ok(result) => result,
        Err(e) => {
            // The prompt went out but produced no answer: the client rejected
            // it, errored, or went away. Same obligation to the upstream.
            return Ok(Resolution::Ended {
                action: ACTION_UNSUPPORTED,
                outcome: Ended::Failed(e),
            });
        }
    };

    // `ElicitationAction` is `#[non_exhaustive]` in rmcp 3.x, so a future
    // revision may add actions. Anything that is not `Accept` carries no answer
    // to feed back, which makes "stop and tell the upstream" the only safe
    // default.
    let (action, message) = match &result.action {
        ElicitationAction::Accept => return Ok(Resolution::Answered(result.content)),
        ElicitationAction::Decline => ("decline", "User declined to provide information"),
        ElicitationAction::Cancel => ("cancel", "User cancelled the operation"),
        unhandled => {
            // Not a misbehaving client: an unrecognised wire value fails
            // deserialization inside rmcp and never reaches here. This is a
            // newer rmcp with an action this roxy build predates.
            warn!(
                "elicitation action {unhandled:?} is not handled by this roxy build; treating as cancel"
            );
            ("cancel", "Elicitation ended without an answer")
        }
    };
    Ok(Resolution::Ended {
        action,
        outcome: Ended::Result(message.to_string()),
    })
}

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
        params: ElicitRequestParams,
    ) -> impl std::future::Future<Output = Result<ElicitResult, McpError>> + Send;
}

/// Production [`ElicitationPrompter`] backed by the live MCP client peer.
struct PeerPrompter<'p> {
    peer: &'p rmcp::service::Peer<rmcp::RoleServer>,
}

impl ElicitationPrompter for PeerPrompter<'_> {
    async fn prompt(&self, params: ElicitRequestParams) -> Result<ElicitResult, McpError> {
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
                let mut raw = Resource::new(r.uri, r.name);
                if let Some(title) = r.title {
                    raw = raw.with_title(title);
                }
                if let Some(desc) = r.description {
                    raw.description = Some(desc);
                }
                if let Some(mime) = r.mime_type {
                    raw.mime_type = Some(mime);
                }
                raw
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
    ///
    /// `elicitation` says whether the client can be prompted at all; see
    /// [`Elicitation`].
    async fn run_tool_loop<P: ElicitationPrompter>(
        &self,
        request: &CallToolRequestParams,
        session_id: Option<&str>,
        request_id: &str,
        exec_ctx: ExecuteContext<'_>,
        prompter: &P,
        elicitation: Elicitation,
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
                    let content: Vec<ContentBlock> =
                        c.content.into_iter().map(map_upstream_content).collect();

                    let mut result = CallToolResult::success(content);
                    if c.structured_content.is_some() {
                        result.structured_content = c.structured_content;
                    }

                    return Ok(result);
                }
                UpstreamCallResult::Error(e) => {
                    return Ok(CallToolResult::error(vec![ContentBlock::text(
                        e.error.message,
                    )]));
                }
                UpstreamCallResult::Elicit(elicit) => {
                    let crate::protocol::UpstreamElicitResponse {
                        message,
                        schema,
                        context: elicit_ctx,
                    } = elicit;

                    match resolve_elicitation(
                        &request.name,
                        message,
                        schema,
                        elicitation,
                        &mut elicitation_rounds,
                        prompter,
                    )
                    .await?
                    {
                        Resolution::Answered(content) => {
                            if let Some(content) = content {
                                elicitation_results.push(content);
                            }
                            elicit_context = elicit_ctx;
                            // re-invoke the upstream with the accumulated results
                        }
                        Resolution::Ended { action, outcome } => {
                            // No answer is coming, so tell the upstream: it is
                            // holding an elicitation context that would
                            // otherwise leak until the client disconnects. This
                            // runs for every such ending — the user's decision,
                            // a client roxy must not prompt, a prompt that
                            // failed, a schema roxy could not read.
                            let cancel_request = UpstreamRequest::ElicitationCancelled {
                                name: &request.name,
                                action,
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

                            return match outcome {
                                Ended::Result(message) => {
                                    Ok(CallToolResult::error(vec![ContentBlock::text(message)]))
                                }
                                Ended::Failed(e) => Err(e),
                            };
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

fn map_upstream_content(item: UpstreamContent) -> ContentBlock {
    match item {
        UpstreamContent::Text { text } => ContentBlock::text(text),
        UpstreamContent::ResourceLink {
            uri,
            name,
            title,
            description,
            mime_type,
        } => {
            let mut raw = Resource::new(uri, name);
            if let Some(t) = title {
                raw = raw.with_title(t);
            }
            if let Some(d) = description {
                raw.description = Some(d);
            }
            if let Some(m) = mime_type {
                raw.mime_type = Some(m);
            }
            ContentBlock::resource_link(raw)
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
        // `with_all_items` stamps `resultType: "complete"` (SEP-2322). rmcp
        // strips the field again for peers that negotiated a pre-`2026-07-28`
        // revision, so one construction serves every client. Cache hints
        // (`ttlMs`/`cacheScope`) are left unset — see issue 0024.
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
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
        // `2026-07-28` replaces server-initiated elicitation with multi
        // round-trip requests, where this arm becomes
        // `CallToolResponse::InputRequired`. Until issue 0022 lands roxy always
        // completes in one response, and elicitation only works where a prompt
        // can actually be delivered — decided per request.
        //
        // `missing_required_keys(V_2026_07_28).is_empty()` is rmcp's own
        // inline-lifecycle predicate — the same check its transport uses to
        // decide a request is served statelessly, which is what leaves no
        // channel for a prompt's answer. `context.protocol_version()` cannot
        // stand in for it: it falls back to the session's negotiated revision,
        // so it reads the same for a legacy session and for an inline request
        // that names a legacy revision.
        let elicitation = Elicitation::for_request(
            context.protocol_version().as_ref(),
            context
                .meta
                .missing_required_keys(&ProtocolVersion::V_2026_07_28)
                .is_empty(),
            context.client_capabilities().as_ref(),
        );
        let result = self
            .run_tool_loop(
                &request,
                session_id_ref,
                request_id,
                exec_ctx,
                &prompter,
                elicitation,
            )
            .await?;
        Ok(CallToolResponse::Complete(result))
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let (_, resources, _) = self.discover().await?;
        Ok(ListResourcesResult::with_all_items(resources))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
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
                    .map(|item| match item {
                        UpstreamContent::Text { text } => {
                            ResourceContents::text(text, request.uri.clone())
                        }
                        UpstreamContent::ResourceLink { .. } => ResourceContents::text(
                            "[resource link]".to_string(),
                            request.uri.clone(),
                        ),
                    })
                    .collect();
                Ok(ReadResourceResponse::Complete(ReadResourceResult::new(
                    contents,
                )))
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
        Ok(ListPromptsResult::with_all_items(prompts))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
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
                            PromptMessage::new_text(Role::Assistant, text)
                        }
                        UpstreamContent::ResourceLink {
                            uri,
                            name,
                            title,
                            description,
                            mime_type,
                        } => {
                            let mut raw = Resource::new(uri, name);
                            if let Some(t) = title {
                                raw = raw.with_title(t);
                            }
                            if let Some(d) = description {
                                raw.description = Some(d);
                            }
                            if let Some(m) = mime_type {
                                raw.mime_type = Some(m);
                            }
                            PromptMessage::new_resource_link(Role::Assistant, raw)
                        }
                    })
                    .collect();
                Ok(GetPromptResponse::Complete(GetPromptResult::new(messages)))
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

    // --- Bounded elicitation loop (issue 0002) ---

    /// The `action` of every `elicitation_cancelled` notification an upstream
    /// received, in order. Shared with the test so the notification — the thing
    /// that lets a backend release its elicitation context — can be asserted
    /// rather than assumed.
    type Cancellations = Arc<std::sync::Mutex<Vec<String>>>;

    /// Scripted upstream for `run_tool_loop` tests: returns `Elicit` for the
    /// first `elicit_rounds` `CallTool` invocations, then `Content`. Pass
    /// `usize::MAX` to model a backend that elicits forever. The
    /// `ElicitationCancelled` notification (not a `CallTool`) is recorded and
    /// acked with empty content. `calls` counts every `CallTool` invocation.
    struct StubExecutor {
        calls: Arc<AtomicUsize>,
        cancellations: Cancellations,
        elicit_rounds: usize,
        valid_schema: bool,
    }

    impl StubExecutor {
        /// Returns the executor, a handle to its `CallTool` counter, and a
        /// handle to the cancellations it was told about.
        fn new(elicit_rounds: usize) -> (Self, Arc<AtomicUsize>, Cancellations) {
            Self::with_schema(elicit_rounds, true)
        }

        /// Same, but the `Elicit` response carries a schema that is not an
        /// elicitation schema at all — a backend bug roxy cannot ask around.
        fn with_invalid_schema() -> (Self, Arc<AtomicUsize>, Cancellations) {
            Self::with_schema(usize::MAX, false)
        }

        fn with_schema(
            elicit_rounds: usize,
            valid_schema: bool,
        ) -> (Self, Arc<AtomicUsize>, Cancellations) {
            let calls = Arc::new(AtomicUsize::new(0));
            let cancellations: Cancellations = Arc::default();
            (
                Self {
                    calls: Arc::clone(&calls),
                    cancellations: Arc::clone(&cancellations),
                    elicit_rounds,
                    valid_schema,
                },
                calls,
                cancellations,
            )
        }
    }

    impl UpstreamExecutor for StubExecutor {
        async fn execute(
            &self,
            request: &UpstreamEnvelope<'_>,
            _ctx: ExecuteContext<'_>,
        ) -> anyhow::Result<UpstreamCallResult> {
            if let UpstreamRequest::ElicitationCancelled { action, .. } = &request.request {
                self.cancellations
                    .lock()
                    .expect("cancellation log is not poisoned")
                    .push((*action).to_string());
                return Ok(UpstreamCallResult::Content(UpstreamContentResponse {
                    content: vec![],
                    structured_content: None,
                }));
            }
            if !matches!(request.request, UpstreamRequest::CallTool { .. }) {
                return Ok(UpstreamCallResult::Content(UpstreamContentResponse {
                    content: vec![],
                    structured_content: None,
                }));
            }
            let prior = self.calls.fetch_add(1, Ordering::SeqCst);
            if prior < self.elicit_rounds {
                Ok(UpstreamCallResult::Elicit(UpstreamElicitResponse {
                    message: "need more input".to_string(),
                    schema: if self.valid_schema {
                        // Minimal schema that deserializes into
                        // ElicitationSchema so the loop reaches the prompt.
                        serde_json::json!({"type": "object", "properties": {}})
                    } else {
                        serde_json::json!("not-an-object")
                    },
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
    /// returns a fixed action (with dummy content for `Accept`), or fails the
    /// prompt outright, as a client that rejects `elicitation/create` or goes
    /// away mid-prompt does.
    struct StubPrompter {
        prompts: Arc<AtomicUsize>,
        action: Option<ElicitationAction>,
    }

    impl StubPrompter {
        /// Returns the prompter and a shared handle to its prompt counter.
        fn new(action: ElicitationAction) -> (Self, Arc<AtomicUsize>) {
            let prompts = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    prompts: Arc::clone(&prompts),
                    action: Some(action),
                },
                prompts,
            )
        }

        /// A prompter whose request never yields an answer.
        fn failing() -> (Self, Arc<AtomicUsize>) {
            let prompts = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    prompts: Arc::clone(&prompts),
                    action: None,
                },
                prompts,
            )
        }
    }

    impl ElicitationPrompter for StubPrompter {
        async fn prompt(&self, _params: ElicitRequestParams) -> Result<ElicitResult, McpError> {
            self.prompts.fetch_add(1, Ordering::SeqCst);
            let Some(action) = self.action.clone() else {
                return Err(McpError::internal_error(
                    "elicitation failed: no answer",
                    None,
                ));
            };
            let mut result = ElicitResult::new(action.clone());
            if matches!(action, ElicitationAction::Accept) {
                result = result.with_content(serde_json::json!({"answer": "yes"}));
            }
            Ok(result)
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
        let (executor, calls, cancellations) = StubExecutor::new(usize::MAX);
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
                Elicitation::ServerInitiated,
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
        // Deliberately asymmetric with the other terminal paths: the cap is a
        // roxy policy abort against a *misbehaving* backend, so there is nothing
        // to tell it that it would act on, and the abort is surfaced as a
        // transport error rather than a result.
        assert!(
            cancellations
                .lock()
                .expect("cancellation log is not poisoned")
                .is_empty(),
            "the cap must not report a cancellation to the upstream"
        );
    }

    /// A legitimate multi-step elicitation under the cap completes normally and
    /// returns the upstream's content — the cap does not interfere.
    #[tokio::test]
    async fn run_tool_loop_completes_within_cap() {
        let (executor, _calls, _cancellations) = StubExecutor::new(3);
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
                Elicitation::ServerInitiated,
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
        let (executor, _calls, _cancellations) = StubExecutor::new(0);
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
                Elicitation::ServerInitiated,
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
    async fn assert_terminal_action(
        action: ElicitationAction,
        expected_msg_fragment: &str,
        expected_action: &str,
    ) {
        let (executor, calls, cancellations) = StubExecutor::new(usize::MAX);
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
                Elicitation::ServerInitiated,
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
        // The backend is holding an elicitation context for this call; it only
        // gets to release it if roxy actually sends the notification.
        assert_eq!(
            *cancellations
                .lock()
                .expect("cancellation log is not poisoned"),
            vec![expected_action.to_string()],
            "the upstream must be told the elicitation ended, with the user's action"
        );
    }

    #[tokio::test]
    async fn run_tool_loop_stops_on_decline() {
        assert_terminal_action(ElicitationAction::Decline, "declined", "decline").await;
    }

    #[tokio::test]
    async fn run_tool_loop_stops_on_cancel() {
        assert_terminal_action(ElicitationAction::Cancel, "cancelled", "cancel").await;
    }

    // --- elicitation the client cannot receive ---

    /// A client that cannot answer a server-initiated prompt must never be sent
    /// one: the prompt has no timeout, so emitting it parks the tool call until
    /// the client gives up. The call ends immediately with an error result naming
    /// the reason, the upstream is told so it can release its context, and the
    /// tool is not re-invoked.
    #[tokio::test]
    async fn run_tool_loop_refuses_to_prompt_when_blocked() {
        let (executor, calls, cancellations) = StubExecutor::new(usize::MAX);
        let server = RoxyServer::new(executor);
        let (prompter, prompts) = StubPrompter::new(ElicitationAction::Accept);

        let request = tool_request("wants_input");
        let result = server
            .run_tool_loop(
                &request,
                None,
                "req-blocked",
                ExecuteContext::default(),
                &prompter,
                Elicitation::Blocked(REASON_NO_MRTR),
            )
            .await
            .expect("a blocked elicitation yields an error result, not a transport error");

        assert_eq!(result.is_error, Some(true), "must map to an error result");
        assert!(
            result_text(&result).contains("multi round-trip"),
            "message must explain why the tool cannot run, got: {}",
            result_text(&result)
        );
        assert_eq!(
            prompts.load(Ordering::SeqCst),
            0,
            "the client must not be prompted at all"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "no further CallTool after refusing to prompt"
        );
        assert_eq!(
            *cancellations
                .lock()
                .expect("cancellation log is not poisoned"),
            vec![ACTION_UNSUPPORTED.to_string()],
            "roxy's own abort must reach the upstream under its own action token, \
             not as a user decision"
        );
    }

    /// A prompt that goes out but yields no answer still leaves the upstream
    /// holding the form, so it is still told — with roxy's own action token,
    /// since no user decided anything. The tool call itself fails, because this
    /// is a fault rather than a decision.
    #[tokio::test]
    async fn run_tool_loop_tells_the_upstream_when_the_prompt_fails() {
        let (executor, calls, cancellations) = StubExecutor::new(usize::MAX);
        let server = RoxyServer::new(executor);
        let (prompter, prompts) = StubPrompter::failing();

        let request = tool_request("prompt_fails");
        let result = server
            .run_tool_loop(
                &request,
                None,
                "req-prompt-failed",
                ExecuteContext::default(),
                &prompter,
                Elicitation::ServerInitiated,
            )
            .await;

        let err = result.expect_err("a failed prompt is a fault, not an error result");
        assert!(
            err.message.contains("elicitation failed"),
            "the client must learn the prompt failed, got: {}",
            err.message
        );
        assert_eq!(
            prompts.load(Ordering::SeqCst),
            1,
            "the prompt was attempted exactly once"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "no further CallTool after a failed prompt"
        );
        assert_eq!(
            *cancellations
                .lock()
                .expect("cancellation log is not poisoned"),
            vec![ACTION_UNSUPPORTED.to_string()],
            "the upstream must be told so it can drop the form it is holding"
        );
    }

    /// The other half of the same obligation: when the upstream's own schema is
    /// unusable, roxy never gets to ask — but the upstream is still holding the
    /// form, so it is still told. The client is never prompted.
    #[tokio::test]
    async fn run_tool_loop_tells_the_upstream_when_its_schema_is_invalid() {
        let (executor, calls, cancellations) = StubExecutor::with_invalid_schema();
        let server = RoxyServer::new(executor);
        let (prompter, prompts) = StubPrompter::new(ElicitationAction::Accept);

        let request = tool_request("bad_schema");
        let result = server
            .run_tool_loop(
                &request,
                None,
                "req-bad-schema",
                ExecuteContext::default(),
                &prompter,
                Elicitation::ServerInitiated,
            )
            .await;

        let err = result.expect_err("an unusable schema is a fault, not an error result");
        assert!(
            err.message.contains("invalid elicitation schema"),
            "the error must name the cause, got: {}",
            err.message
        );
        assert_eq!(
            prompts.load(Ordering::SeqCst),
            0,
            "there is nothing to prompt with, so the client is not prompted"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "no further CallTool after an unusable schema"
        );
        assert_eq!(
            *cancellations
                .lock()
                .expect("cancellation log is not poisoned"),
            vec![ACTION_UNSUPPORTED.to_string()],
            "the upstream must be told so it can drop the form it is holding"
        );
    }

    /// Client capabilities that declare form elicitation, as a legacy client
    /// does.
    fn elicitation_capable() -> ClientCapabilities {
        ClientCapabilities::builder().enable_elicitation().build()
    }

    /// The classification that decides the above. Deliberately covers the
    /// combination that a revision check alone gets wrong: an *inline-lifecycle*
    /// request that declares a legacy revision. rmcp serves those statelessly,
    /// so there is no channel for a client response, whatever the version says.
    #[test]
    fn elicitation_is_blocked_unless_the_request_can_carry_a_prompt() {
        let capable = elicitation_capable();

        for legacy in [ProtocolVersion::V_2025_06_18, ProtocolVersion::V_2025_11_25] {
            assert_eq!(
                Elicitation::for_request(Some(&legacy), false, Some(&capable)),
                Elicitation::ServerInitiated,
                "a {legacy} session can carry a server-initiated prompt"
            );
        }
        for legacy in [
            ProtocolVersion::V_2024_11_05,
            ProtocolVersion::V_2025_03_26,
            ProtocolVersion::V_2025_06_18,
            ProtocolVersion::V_2025_11_25,
        ] {
            assert_eq!(
                Elicitation::for_request(Some(&legacy), true, Some(&capable)),
                Elicitation::Blocked(REASON_NO_MRTR),
                "an inline-lifecycle request is served statelessly even when it \
                 declares {legacy}, so no prompt can be delivered"
            );
        }

        for inline in [false, true] {
            assert_eq!(
                Elicitation::for_request(
                    Some(&ProtocolVersion::V_2026_07_28),
                    inline,
                    Some(&capable)
                ),
                Elicitation::Blocked(REASON_NO_MRTR),
                "2026-07-28 replaced server-initiated requests with MRTR — including when the \
                 revision arrives in the header alone, with no `_meta` (inline = {inline})"
            );
        }
        assert_eq!(
            Elicitation::for_request(None, true, Some(&capable)),
            Elicitation::Blocked(REASON_NO_MRTR),
            "an inline-lifecycle request is stateless even with no revision to compare"
        );
        assert_eq!(
            Elicitation::for_request(None, false, Some(&capable)),
            Elicitation::ServerInitiated,
            "a request with no revision attached is a legacy session"
        );
        assert_eq!(
            Elicitation::for_request(Some(&ProtocolVersion::V_2025_11_25), false, None),
            Elicitation::Blocked(REASON_NO_CAPABILITY),
            "a client that declared nothing must not be prompted"
        );
        assert_eq!(
            Elicitation::for_request(
                Some(&ProtocolVersion::V_2025_11_25),
                false,
                Some(&ClientCapabilities::default()),
            ),
            Elicitation::Blocked(REASON_NO_CAPABILITY),
            "capabilities without `elicitation` are a MUST NOT, not a maybe"
        );
    }

    /// roxy only sends *form* prompts, so URL mode alone is not promptable.
    /// (The "neither mode set means form" case is `elicitation_capable()`, which
    /// the matrix above already covers.)
    #[test]
    fn elicitation_requires_form_mode_specifically() {
        let mut url_only = elicitation_capable();
        url_only.elicitation =
            Some(ElicitationCapability::new().with_url(UrlElicitationCapability::new()));

        assert_eq!(
            Elicitation::for_request(Some(&ProtocolVersion::V_2025_11_25), false, Some(&url_only)),
            Elicitation::Blocked(REASON_NO_CAPABILITY),
            "a url-only client cannot render the form roxy would send"
        );
    }

    /// Elicitation arrived in `2025-06-18`; before that there is no
    /// `elicitation/create` to send, whatever a client claims.
    #[test]
    fn elicitation_is_blocked_before_it_existed() {
        let capable = elicitation_capable();
        for ancient in [ProtocolVersion::V_2024_11_05, ProtocolVersion::V_2025_03_26] {
            assert_eq!(
                Elicitation::for_request(Some(&ancient), false, Some(&capable)),
                Elicitation::Blocked(REASON_NO_ELICITATION),
                "{ancient} predates elicitation"
            );
        }
    }
}
