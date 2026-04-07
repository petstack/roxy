use anyhow::Context;
use deadpool::managed;
use fastcgi_client::{Client, Params, Request, conn::KeepAlive, io};
use tokio::net::{TcpStream, UnixStream};
use tokio_util::compat::Compat;
use tracing::{debug, warn};

use crate::config::FcgiAddress;
use crate::protocol::{UpstreamCallResult, UpstreamDiscoverResponse, UpstreamEnvelope, UpstreamRequest};

use super::UpstreamExecutor;

// --- Pool manager types ---

type TcpFcgiClient = Client<Compat<TcpStream>, KeepAlive>;
type UnixFcgiClient = Client<Compat<UnixStream>, KeepAlive>;

struct TcpFcgiManager {
    addr: String,
}

impl managed::Manager for TcpFcgiManager {
    type Type = TcpFcgiClient;
    type Error = std::io::Error;

    async fn create(&self) -> Result<TcpFcgiClient, std::io::Error> {
        let stream = TcpStream::connect(&self.addr).await?;
        Ok(Client::new_keep_alive_tokio(stream))
    }

    async fn recycle(
        &self,
        _obj: &mut TcpFcgiClient,
        _metrics: &managed::Metrics,
    ) -> managed::RecycleResult<std::io::Error> {
        Ok(())
    }
}

struct UnixFcgiManager {
    path: String,
}

impl managed::Manager for UnixFcgiManager {
    type Type = UnixFcgiClient;
    type Error = std::io::Error;

    async fn create(&self) -> Result<UnixFcgiClient, std::io::Error> {
        let stream = UnixStream::connect(&self.path).await?;
        Ok(Client::new_keep_alive_tokio(stream))
    }

    async fn recycle(
        &self,
        _obj: &mut UnixFcgiClient,
        _metrics: &managed::Metrics,
    ) -> managed::RecycleResult<std::io::Error> {
        Ok(())
    }
}

enum FcgiPool {
    Tcp(managed::Pool<TcpFcgiManager>),
    Unix(managed::Pool<UnixFcgiManager>),
}

// --- FastCgiExecutor ---

pub struct FastCgiExecutor {
    pool: FcgiPool,
    script_filename: String,
}

impl FastCgiExecutor {
    pub fn new(
        address: &FcgiAddress,
        script_filename: String,
        pool_size: usize,
    ) -> anyhow::Result<Self> {
        let pool = match address {
            FcgiAddress::Tcp(addr) => {
                let mgr = TcpFcgiManager { addr: addr.clone() };
                let pool = managed::Pool::builder(mgr)
                    .max_size(pool_size)
                    .build()
                    .context("failed to build TCP FastCGI pool")?;
                FcgiPool::Tcp(pool)
            }
            FcgiAddress::Unix(path) => {
                let mgr = UnixFcgiManager { path: path.clone() };
                let pool = managed::Pool::builder(mgr)
                    .max_size(pool_size)
                    .build()
                    .context("failed to build Unix FastCGI pool")?;
                FcgiPool::Unix(pool)
            }
        };

        Ok(Self {
            pool,
            script_filename,
        })
    }

    async fn send_request(&self, body: &[u8]) -> anyhow::Result<Vec<u8>> {
        let params = Params::default()
            .request_method("POST")
            .script_filename(&self.script_filename)
            .script_name("/handler.php")
            .request_uri("/handler.php")
            .content_type("application/json")
            .content_length(body.len())
            .server_name("localhost")
            .server_port(0);

        let response = match &self.pool {
            FcgiPool::Tcp(pool) => {
                let mut conn = pool
                    .get()
                    .await
                    .map_err(|e| anyhow::anyhow!("pool error: {e}"))?;
                debug!("sending FastCGI request via TCP");
                conn.execute(Request::new(params, io::Cursor::new(body)))
                    .await?
            }
            FcgiPool::Unix(pool) => {
                let mut conn = pool
                    .get()
                    .await
                    .map_err(|e| anyhow::anyhow!("pool error: {e}"))?;
                debug!("sending FastCGI request via Unix socket");
                conn.execute(Request::new(params, io::Cursor::new(body)))
                    .await?
            }
        };

        if let Some(stderr) = &response.stderr
            && !stderr.is_empty()
        {
            warn!("PHP stderr: {}", String::from_utf8_lossy(stderr));
        }

        let stdout = response.stdout.unwrap_or_default();
        extract_body(&stdout)
    }
}

/// Extract body from raw CGI output (skip HTTP headers before `\r\n\r\n`).
fn extract_body(raw: &[u8]) -> anyhow::Result<Vec<u8>> {
    let separator = b"\r\n\r\n";
    if let Some(pos) = raw.windows(separator.len()).position(|w| w == separator) {
        Ok(raw[pos + separator.len()..].to_vec())
    } else {
        Ok(raw.to_vec())
    }
}

impl UpstreamExecutor for FastCgiExecutor {
    async fn execute(&self, request: &UpstreamEnvelope<'_>) -> anyhow::Result<UpstreamCallResult> {
        let body = serde_json::to_vec(request)?;
        let response_bytes = self.send_request(&body).await?;
        debug!("PHP response: {}", String::from_utf8_lossy(&response_bytes));
        UpstreamCallResult::parse(&response_bytes).context("failed to parse PHP response")
    }

    async fn discover(&self) -> anyhow::Result<UpstreamDiscoverResponse> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let envelope = UpstreamEnvelope {
            session_id: None,
            request_id: &request_id,
            request: UpstreamRequest::Discover,
        };
        let body = serde_json::to_vec(&envelope)?;
        let response_bytes = self.send_request(&body).await?;
        debug!(
            "PHP discover response: {}",
            String::from_utf8_lossy(&response_bytes)
        );
        let response: UpstreamDiscoverResponse = serde_json::from_slice(&response_bytes)
            .context("failed to parse PHP discover response")?;
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_body_with_headers() {
        let raw = b"Content-Type: application/json\r\n\r\n{\"ok\":true}";
        let body = extract_body(raw).unwrap();
        assert_eq!(body, b"{\"ok\":true}");
    }

    #[test]
    fn test_extract_body_no_headers() {
        let raw = b"{\"ok\":true}";
        let body = extract_body(raw).unwrap();
        assert_eq!(body, b"{\"ok\":true}");
    }

    #[test]
    fn test_extract_body_multiple_headers() {
        let raw = b"Status: 200 OK\r\nContent-Type: application/json\r\n\r\n{\"data\":1}";
        let body = extract_body(raw).unwrap();
        assert_eq!(body, b"{\"data\":1}");
    }
}
