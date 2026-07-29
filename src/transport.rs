//! Transport wiring shared by the binary and the integration tests.

use std::sync::Arc;

use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio_util::sync::CancellationToken;

use crate::executor::UpstreamExecutor;
use crate::server::RoxyServer;

/// The Streamable-HTTP service roxy serves under `/mcp`.
pub type RoxyHttpService<E> = StreamableHttpService<RoxyServer<Arc<E>>, LocalSessionManager>;

/// Build the Streamable-HTTP tower service for `executor`.
///
/// `legacy_session_mode` is left at rmcp's default (`true`) deliberately. It
/// governs only revisions **older** than `2026-07-28`, which keep
/// `Mcp-Session-Id` and the standalone `GET` SSE stream; `2026-07-28` requests
/// are served statelessly regardless of the flag. One service therefore covers
/// every revision roxy speaks, which is the whole point of a gateway — see
/// `README.md` → "MCP protocol revisions".
///
/// Lives in the library rather than in `main.rs` so `tests/` exercises the
/// exact configuration the binary runs.
pub fn http_service<E: UpstreamExecutor + 'static>(
    executor: Arc<E>,
    cancellation_token: CancellationToken,
) -> RoxyHttpService<E> {
    StreamableHttpService::new(
        move || Ok(RoxyServer::new(executor.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default().with_cancellation_token(cancellation_token),
    )
}
