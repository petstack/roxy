//! Transport wiring shared by the binary and the integration tests.

use std::sync::Arc;

use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio_util::sync::CancellationToken;

use crate::executor::UpstreamExecutor;
use crate::server::RoxyServer;

/// The Streamable-HTTP service roxy serves to MCP clients.
pub type RoxyHttpService<E> = StreamableHttpService<RoxyServer<Arc<E>>, LocalSessionManager>;

/// Build the Streamable-HTTP tower service for `executor`.
///
/// Every policy roxy exposes to the network is set here explicitly rather than
/// inherited from the SDK's defaults, because rmcp 3.x added inbound checks
/// (`Host` validation, a request-body cap) that 1.x did not have and that
/// change who can reach roxy:
///
/// - **`allowed_hosts`** — `Host` values to accept, per `--allowed-host`. An
///   empty slice, or one containing `*`, accepts any host. Loopback-only is the
///   default in [`crate::config`]; see the flag's docs for why.
/// - **`max_body_bytes`** — inbound POST body cap, per `--max-body-size`.
///   Larger bodies get `413`.
/// - **`legacy_session_mode`** is left at rmcp's default (`true`) deliberately.
///   It governs only revisions **older** than `2026-07-28`, which keep
///   `Mcp-Session-Id` and the standalone `GET` SSE stream; `2026-07-28`
///   requests are served statelessly regardless of the flag. One service
///   therefore covers every revision roxy speaks, which is the point of a
///   gateway — see `README.md` → "MCP protocol revisions".
///
/// Lives in the library rather than in `main.rs` so `tests/` exercises the
/// exact configuration the binary runs.
pub fn http_service<E: UpstreamExecutor + 'static>(
    executor: Arc<E>,
    allowed_hosts: &[String],
    max_body_bytes: usize,
    cancellation_token: CancellationToken,
) -> RoxyHttpService<E> {
    let config = StreamableHttpServerConfig::default()
        .with_max_request_body_bytes(max_body_bytes)
        .with_cancellation_token(cancellation_token);
    let config = if allowed_hosts.iter().any(|host| host == "*") {
        config.disable_allowed_hosts()
    } else {
        config.with_allowed_hosts(allowed_hosts.iter().map(String::as_str))
    };

    StreamableHttpService::new(
        move || Ok(RoxyServer::new(executor.clone())),
        Arc::new(LocalSessionManager::default()),
        config,
    )
}
