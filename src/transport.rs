//! Transport wiring shared by the binary and the integration tests.

use std::sync::Arc;

use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio_util::sync::CancellationToken;

use crate::config::{DEFAULT_ALLOWED_HOSTS, host_validation_disabled, normalize_list};
use crate::executor::UpstreamExecutor;
use crate::server::RoxyServer;

/// The Streamable-HTTP service roxy serves to MCP clients.
pub type RoxyHttpService<E> = StreamableHttpService<RoxyServer<Arc<E>>, LocalSessionManager>;

/// Build the Streamable-HTTP tower service for `executor`.
///
/// The two inbound checks rmcp 3.x added — `Host` validation and a request-body
/// cap — are roxy's own, set from configuration here, because they decide who
/// can reach roxy and rmcp 1.x had neither. Everything else is
/// `StreamableHttpServerConfig::default()`, deliberately:
///
/// - **`allowed_hosts`** — `Host` values to accept, normally from
///   [`crate::config::Config::allowed_hosts`]. A list containing `*` disables
///   the check; one that is empty (or only blanks) falls back to
///   [`crate::config::DEFAULT_ALLOWED_HOSTS`] rather than passing through to
///   rmcp, where an empty list means "accept every host". This function is
///   public API, so it enforces that itself instead of trusting its caller to.
/// - **`max_body_bytes`** — inbound POST body cap, per `--max-body-size`.
///   Larger bodies get `413`.
/// - **`legacy_session_mode`** stays at rmcp's default (`true`). It governs only
///   revisions **older** than `2026-07-28`, which keep `Mcp-Session-Id` and the
///   standalone `GET` SSE stream; `2026-07-28` requests are served statelessly
///   regardless of the flag. One service therefore covers every revision roxy
///   speaks, which is the point of a gateway — see `README.md` → "MCP protocol
///   revisions".
/// - **`allowed_origins`** stays empty, i.e. `Origin` is not validated. roxy is
///   a gateway with no browser surface of its own; a deployment that needs it
///   should get a flag of its own rather than a default nobody chose.
///
/// `StreamableHttpServerConfig` is `#[non_exhaustive]`, so a future rmcp can add
/// another inbound policy without breaking this build. There is no compile-time
/// canary for that: read its changelog on every bump.
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
    // Blank entries are dropped here too, not just in `Config::allowed_hosts`:
    // rmcp reads a blank as a host pattern that matches nothing, so a caller
    // handing us `[""]` would reject every request.
    let hosts = normalize_list(allowed_hosts.to_vec());
    let config = if host_validation_disabled(&hosts) {
        config.disable_allowed_hosts()
    } else if hosts.is_empty() {
        config.with_allowed_hosts(DEFAULT_ALLOWED_HOSTS)
    } else {
        config.with_allowed_hosts(hosts)
    };

    StreamableHttpService::new(
        move || Ok(RoxyServer::new(executor.clone())),
        Arc::new(LocalSessionManager::default()),
        config,
    )
}
