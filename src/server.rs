use std::sync::Arc;

use rmcp::{ServerHandler, model::*};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::executor::{ExecuteContext, UpstreamExecutor};
use crate::protocol::{UpstreamCallResult, UpstreamContent, UpstreamEnvelope, UpstreamRequest};

type McpError = rmcp::ErrorData;

/// Fill a 36-byte stack buffer with a fresh UUID v4 hyphenated ascii string.
/// Returns a `&str` borrowed from the caller-supplied buffer so callers can
/// embed a per-request correlation id in an envelope without allocating.
#[doc(hidden)]
pub fn fresh_request_id(buf: &mut [u8; uuid::fmt::Hyphenated::LENGTH]) -> &str {
    Uuid::new_v4().hyphenated().encode_lower(buf)
}

pub struct RoxyServer<E: UpstreamExecutor> {
    executor: E,
    capabilities: Arc<DiscoverResult>,
}

/// Pre-discovered capabilities, shared across all server instances spawned
/// by the HTTP transport (one per MCP session). Stored behind an `Arc` so
/// session creation is O(1) instead of cloning the full tool/resource/prompt
/// definitions per session.
pub struct DiscoverResult {
    pub tools: Vec<Tool>,
    pub resources: Vec<Resource>,
    pub prompts: Vec<Prompt>,
}

impl<E: UpstreamExecutor + 'static> RoxyServer<E> {
    /// Create server and discover capabilities from PHP.
    pub async fn new(executor: E) -> anyhow::Result<Self> {
        info!("discovering capabilities from PHP...");
        let discover = Self::discover_from(&executor).await?;
        Ok(Self::from_cached(executor, Arc::new(discover)))
    }

    /// Create server from pre-discovered capabilities (synchronous).
    /// Used by the HTTP transport factory closure which cannot be async.
    pub fn from_cached(executor: E, capabilities: Arc<DiscoverResult>) -> Self {
        Self {
            executor,
            capabilities,
        }
    }

    /// Discover capabilities from the PHP backend and convert to MCP types.
    pub async fn discover_from(executor: &E) -> anyhow::Result<DiscoverResult> {
        let discover = executor.discover().await?;

        let tools: Vec<Tool> = discover
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

        let resources: Vec<Resource> = discover
            .resources
            .into_iter()
            .map(|r| {
                let mut raw = RawResource::new(r.uri, r.name);
                if let Some(title) = r.title {
                    raw = raw.with_title(title);
                }
                if let Some(desc) = r.description {
                    raw.description = Some(desc);
                }
                if let Some(mime) = r.mime_type {
                    raw.mime_type = Some(mime);
                }
                raw.no_annotation()
            })
            .collect();

        let prompts: Vec<Prompt> = discover
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

        info!(
            "discovered {} tools, {} resources, {} prompts",
            tools.len(),
            resources.len(),
            prompts.len()
        );

        Ok(DiscoverResult {
            tools,
            resources,
            prompts,
        })
    }
}

fn extract_session_id(context: &rmcp::service::RequestContext<rmcp::RoleServer>) -> Option<String> {
    context
        .extensions
        .get::<http::request::Parts>()
        .and_then(|parts| parts.headers.get("mcp-session-id"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned())
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
        } => {
            let mut raw = RawResource::new(uri, name);
            if let Some(t) = title {
                raw = raw.with_title(t);
            }
            if let Some(d) = description {
                raw.description = Some(d);
            }
            if let Some(m) = mime_type {
                raw.mime_type = Some(m);
            }
            Content::resource_link(raw)
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
        Ok(ListToolsResult {
            tools: self.capabilities.tools.clone(),
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

        let mut elicitation_results: Vec<serde_json::Value> = Vec::new();
        let mut elicit_context: Option<serde_json::Value> = None;

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
                session_id: session_id_ref,
                request_id,
                request: upstream_request,
            };

            let response = self
                .executor
                .execute(&envelope, ExecuteContext::default())
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

                    let elicit_result =
                        context.peer.create_elicitation(params).await.map_err(|e| {
                            error!("elicitation request failed: {e}");
                            McpError::internal_error(format!("elicitation failed: {e}"), None)
                        })?;

                    match elicit_result.action {
                        ElicitationAction::Accept => {
                            if let Some(content) = elicit_result.content {
                                elicitation_results.push(content);
                            }
                            elicit_context = elicit_ctx;
                            // continue loop — re-invoke PHP with results
                        }
                        action @ (ElicitationAction::Decline | ElicitationAction::Cancel) => {
                            let action_str = match action {
                                ElicitationAction::Decline => "decline",
                                ElicitationAction::Cancel => "cancel",
                                _ => unreachable!(),
                            };

                            // Notify PHP about cancellation
                            let cancel_request = UpstreamRequest::ElicitationCancelled {
                                name: &request.name,
                                action: action_str,
                                context: elicit_ctx.as_ref(),
                            };
                            let cancel_envelope = UpstreamEnvelope {
                                session_id: session_id_ref,
                                request_id,
                                request: cancel_request,
                            };
                            if let Err(e) = self
                                .executor
                                .execute(&cancel_envelope, ExecuteContext::default())
                                .await
                            {
                                warn!(
                                    "failed to notify upstream about elicitation cancellation: {e}"
                                );
                            }

                            let msg = match action {
                                ElicitationAction::Decline => {
                                    "User declined to provide information"
                                }
                                ElicitationAction::Cancel => "User cancelled the operation",
                                _ => unreachable!(),
                            };
                            return Ok(CallToolResult::error(vec![Content::text(msg)]));
                        }
                    }
                }
            }
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: self.capabilities.resources.clone(),
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
        let upstream_request = UpstreamRequest::ReadResource { uri: &request.uri };
        let envelope = UpstreamEnvelope {
            session_id: session_id.as_deref(),
            request_id,
            request: upstream_request,
        };

        let response = self
            .executor
            .execute(&envelope, ExecuteContext::default())
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
        Ok(ListPromptsResult {
            prompts: self.capabilities.prompts.clone(),
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
            .execute(&envelope, ExecuteContext::default())
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
                        } => {
                            let mut raw = RawResource::new(uri, name);
                            if let Some(t) = title {
                                raw = raw.with_title(t);
                            }
                            if let Some(d) = description {
                                raw.description = Some(d);
                            }
                            if let Some(m) = mime_type {
                                raw.mime_type = Some(m);
                            }
                            PromptMessage::new_resource_link(
                                PromptMessageRole::Assistant,
                                raw.no_annotation(),
                            )
                        }
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
