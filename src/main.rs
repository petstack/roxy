use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use rmcp::ServiceExt;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tracing::{info, warn};

use roxy::config::{Config, LogFormat, Transport, UpstreamKind, normalize_header_list};
use roxy::executor::UpstreamExecutor;
use roxy::executor::fastcgi::FastCgiExecutor;
use roxy::executor::http::HttpExecutor;
use roxy::server::RoxyServer;


fn init_logging(format: &LogFormat) {
    let subscriber = tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
    );

    match format {
        LogFormat::Pretty => subscriber.pretty().init(),
        LogFormat::Json => subscriber.json().init(),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut config = Config::parse();
    init_logging(&config.log_format);

    // Without this, a blank line at the start/end of ROXY_UPSTREAM_HEADER
    // (e.g. from a Kubernetes YAML `|-` block scalar) would reach
    // parse_header() as an empty string and fail with "invalid header format".
    config.upstream_header = normalize_header_list(std::mem::take(&mut config.upstream_header));

    info!("roxy starting");
    info!("transport: {:?}", config.transport);
    info!("upstream: {}", config.upstream);

    let upstream_kind = UpstreamKind::parse(&config.upstream);

    match upstream_kind {
        UpstreamKind::Http { url } => {
            if config.upstream_entrypoint.is_some() {
                warn!("--upstream-entrypoint is ignored for HTTP upstream");
            }
            if config.upstream_insecure && url.starts_with("http://") {
                warn!("--upstream-insecure has no effect with plain HTTP upstream");
            }

            info!("using HTTP executor → {url}");
            let executor = Arc::new(HttpExecutor::new(
                url,
                config.upstream_timeout,
                config.upstream_insecure,
                &config.upstream_header,
            )?);
            run(executor, &config).await
        }
        UpstreamKind::FastCgi { address } => {
            let entrypoint = config.upstream_entrypoint.clone().ok_or_else(|| {
                anyhow::anyhow!("--upstream-entrypoint is required for FastCGI upstream")
            })?;

            if config.upstream_insecure {
                warn!("--upstream-insecure is ignored for FastCGI upstream");
            }
            if !config.upstream_header.is_empty() {
                warn!("--upstream-header is ignored for FastCGI upstream");
            }

            info!("using FastCGI executor → {}", config.upstream);
            let executor = Arc::new(FastCgiExecutor::new(
                &address,
                entrypoint,
                config.pool_size,
            )?);
            run(executor, &config).await
        }
    }
}

async fn run<E: UpstreamExecutor + 'static>(
    executor: Arc<E>,
    config: &Config,
) -> anyhow::Result<()> {
    match config.transport {
        Transport::Stdio => run_stdio(executor).await,
        Transport::Http => run_http(executor, config.port).await,
    }
}

async fn run_stdio<E: UpstreamExecutor + 'static>(executor: Arc<E>) -> anyhow::Result<()> {
    info!("starting stdio transport");
    let server = RoxyServer::new(executor);
    let service = server
        .serve(rmcp::transport::io::stdio())
        .await
        .context("failed to start stdio server")?;
    service.waiting().await?;
    Ok(())
}

async fn run_http<E: UpstreamExecutor + 'static>(
    executor: Arc<E>,
    port: u16,
) -> anyhow::Result<()> {
    let addr = format!("127.0.0.1:{port}");
    info!("starting HTTP/SSE transport on {addr}");

    let ct = tokio_util::sync::CancellationToken::new();

    let service = StreamableHttpService::new(
        move || Ok(RoxyServer::new(executor.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token()),
    );

    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .context("failed to bind HTTP listener")?;

    info!("listening on {addr}/mcp");

    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c().await.ok();
            info!("shutting down...");
            ct.cancel();
        })
        .await
        .context("HTTP server error")?;

    Ok(())
}
