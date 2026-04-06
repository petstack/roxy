mod config;
mod executor;
mod protocol;
mod server;

use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use rmcp::ServiceExt;
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, StreamableHttpServerConfig,
    session::local::LocalSessionManager,
};
use tracing::info;

use config::{Config, FcgiAddress, LogFormat, Transport};
use executor::fastcgi::FastCgiExecutor;
use server::RoxyServer;

fn init_logging(format: &LogFormat) {
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(
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
    let config = Config::parse();
    init_logging(&config.log_format);

    info!("roxy starting");
    info!("transport: {:?}", config.transport);
    info!("php-fpm: {}", config.php_fpm);
    info!("entrypoint: {}", config.php_entrypoint);

    let address = FcgiAddress::parse(&config.php_fpm);
    let executor = Arc::new(FastCgiExecutor::new(
        &address,
        config.php_entrypoint.clone(),
        config.pool_size,
    )?);

    match config.transport {
        Transport::Stdio => run_stdio(executor).await,
        Transport::Http => run_http(executor, config.port).await,
    }
}

async fn run_stdio(executor: Arc<FastCgiExecutor>) -> anyhow::Result<()> {
    info!("starting stdio transport");
    let server = RoxyServer::new(executor).await?;
    let service = server
        .serve(rmcp::transport::io::stdio())
        .await
        .context("failed to start stdio server")?;
    service.waiting().await?;
    Ok(())
}

async fn run_http(executor: Arc<FastCgiExecutor>, port: u16) -> anyhow::Result<()> {
    let addr = format!("127.0.0.1:{port}");
    info!("starting HTTP/SSE transport on {addr}");

    // Pre-discover capabilities so the factory closure can be synchronous.
    let discover_result =
        RoxyServer::discover_from(&*executor).await
            .context("failed to discover PHP capabilities")?;

    let ct = tokio_util::sync::CancellationToken::new();

    let service = StreamableHttpService::new(
        move || {
            Ok(RoxyServer::from_cached(
                executor.clone(),
                discover_result.clone(),
            ))
        },
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default()
            .with_cancellation_token(ct.child_token()),
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
