pub mod fastcgi;
pub mod http;

use std::sync::Arc;

use crate::protocol::{UpstreamCallResult, UpstreamDiscoverResponse, UpstreamEnvelope};

/// Per-request, transport-level context threaded into every `execute()`
/// call. Kept as a struct (rather than a bare `Option<&HeaderMap>`) so new
/// fields — remote addr, client cert, trace headers — can be added later
/// without another trait-signature break.
#[derive(Debug, Default, Clone, Copy)]
pub struct ExecuteContext<'a> {
    /// HTTP headers from the incoming MCP client request that should be
    /// forwarded to the upstream. `None` when roxy is running under
    /// `--transport stdio` (no incoming HTTP request exists).
    pub forward_headers: Option<&'a ::http::HeaderMap>,
}

pub trait UpstreamExecutor: Send + Sync {
    fn execute(
        &self,
        request: &UpstreamEnvelope<'_>,
        ctx: ExecuteContext<'_>,
    ) -> impl std::future::Future<Output = anyhow::Result<UpstreamCallResult>> + Send;

    fn discover(
        &self,
    ) -> impl std::future::Future<Output = anyhow::Result<UpstreamDiscoverResponse>> + Send;
}

impl<T: UpstreamExecutor> UpstreamExecutor for Arc<T> {
    fn execute(
        &self,
        request: &UpstreamEnvelope<'_>,
        ctx: ExecuteContext<'_>,
    ) -> impl std::future::Future<Output = anyhow::Result<UpstreamCallResult>> + Send {
        (**self).execute(request, ctx)
    }

    fn discover(
        &self,
    ) -> impl std::future::Future<Output = anyhow::Result<UpstreamDiscoverResponse>> + Send {
        (**self).discover()
    }
}
