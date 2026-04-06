use rmcp::{
    ServerHandler,
    model::*,
};
use tracing::{error, info};

use crate::executor::PhpExecutor;
use crate::protocol::{PhpCallResult, PhpContent, PhpRequest};

type McpError = rmcp::ErrorData;

pub struct RoxyServer<E: PhpExecutor> {
    executor: E,
    tools: Vec<Tool>,
    resources: Vec<Resource>,
    prompts: Vec<Prompt>,
}

impl<E: PhpExecutor + 'static> RoxyServer<E> {
    /// Create server and discover capabilities from PHP.
    pub async fn new(executor: E) -> anyhow::Result<Self> {
        info!("discovering capabilities from PHP...");
        let discover = executor.discover().await?;

        let tools: Vec<Tool> = discover
            .tools
            .into_iter()
            .map(|t| {
                let schema = t.input_schema.unwrap_or_default();
                Tool::new(t.name, t.description.unwrap_or_default(), schema)
            })
            .collect();

        let resources: Vec<Resource> = discover
            .resources
            .into_iter()
            .map(|r| {
                let mut raw = RawResource::new(r.uri, r.name);
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
            .map(|p| Prompt::new(
                p.name,
                p.description,
                Some(
                    p.arguments
                        .into_iter()
                        .map(|a| {
                            let mut arg = PromptArgument::new(a.name);
                            if let Some(desc) = a.description {
                                arg = arg.with_description(desc);
                            }
                            arg = arg.with_required(a.required);
                            arg
                        })
                        .collect(),
                ),
            ))
            .collect();

        info!(
            "discovered {} tools, {} resources, {} prompts",
            tools.len(),
            resources.len(),
            prompts.len()
        );

        Ok(Self {
            executor,
            tools,
            resources,
            prompts,
        })
    }
}

impl<E: PhpExecutor + 'static> ServerHandler for RoxyServer<E> {
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
            tools: self.tools.clone(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        info!("call_tool: {}", request.name);
        let php_request = PhpRequest::CallTool {
            name: &request.name,
            arguments: request.arguments.as_ref(),
        };

        let response = self
            .executor
            .execute(&php_request)
            .await
            .map_err(|e| {
                error!("PHP executor error: {e}");
                McpError::internal_error(format!("PHP error: {e}"), None)
            })?;

        match response {
            PhpCallResult::Content(c) => {
                let content: Vec<Content> = c
                    .content
                    .into_iter()
                    .map(|item| match item {
                        PhpContent::Text { text } => Content::text(text),
                    })
                    .collect();
                Ok(CallToolResult::success(content))
            }
            PhpCallResult::Error(e) => {
                Ok(CallToolResult::error(vec![Content::text(e.error.message)]))
            }
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: self.resources.clone(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        info!("read_resource: {}", request.uri);
        let php_request = PhpRequest::ReadResource {
            uri: &request.uri,
        };

        let response = self
            .executor
            .execute(&php_request)
            .await
            .map_err(|e| {
                error!("PHP executor error: {e}");
                McpError::internal_error(format!("PHP error: {e}"), None)
            })?;

        match response {
            PhpCallResult::Content(c) => {
                let contents: Vec<ResourceContents> = c
                    .content
                    .into_iter()
                    .map(|item| match item {
                        PhpContent::Text { text } => {
                            ResourceContents::text(text, request.uri.clone())
                        }
                    })
                    .collect();
                Ok(ReadResourceResult::new(contents))
            }
            PhpCallResult::Error(e) => {
                Err(McpError::resource_not_found(e.error.message, None))
            }
        }
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult {
            prompts: self.prompts.clone(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        info!("get_prompt: {}", request.name);
        let php_request = PhpRequest::GetPrompt {
            name: &request.name,
            arguments: request.arguments.as_ref(),
        };

        let response = self
            .executor
            .execute(&php_request)
            .await
            .map_err(|e| {
                error!("PHP executor error: {e}");
                McpError::internal_error(format!("PHP error: {e}"), None)
            })?;

        match response {
            PhpCallResult::Content(c) => {
                let messages: Vec<PromptMessage> = c
                    .content
                    .into_iter()
                    .map(|item| match item {
                        PhpContent::Text { text } => {
                            PromptMessage::new_text(PromptMessageRole::Assistant, text)
                        }
                    })
                    .collect();
                Ok(GetPromptResult::new(messages))
            }
            PhpCallResult::Error(e) => {
                Err(McpError::invalid_params(e.error.message, None))
            }
        }
    }
}
