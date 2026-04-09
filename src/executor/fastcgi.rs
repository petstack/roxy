use anyhow::Context;
use deadpool::managed;
use fastcgi_client::{Client, Params, Request, conn::KeepAlive, io};
use tokio::net::{TcpStream, UnixStream};
use tokio_util::compat::Compat;
use tracing::{debug, warn};

use crate::config::FcgiAddress;
use crate::protocol::{
    UpstreamCallResult, UpstreamDiscoverResponse, UpstreamEnvelope, UpstreamRequest,
};

use super::{ExecuteContext, UpstreamExecutor};

/// Compute a CGI `SCRIPT_NAME` value from a `--upstream-entrypoint` path.
/// Returns `/` + the basename of the entrypoint, or `/handler.php` when
/// the entrypoint has no file component (empty string, bare `/`, etc.).
/// The entrypoint string itself is **not** returned — callers still use
/// it unchanged as `SCRIPT_FILENAME`.
#[doc(hidden)]
pub fn derive_script_name(entrypoint: &str) -> String {
    let base = std::path::Path::new(entrypoint)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("handler.php");
    format!("/{base}")
}

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
    script_name: String,
    request_uri: String,
}

impl FastCgiExecutor {
    pub fn new(
        address: &FcgiAddress,
        script_filename: String,
        pool_size: usize,
    ) -> anyhow::Result<Self> {
        let script_name = derive_script_name(&script_filename);
        let request_uri = script_name.clone();

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
            script_name,
            request_uri,
        })
    }

    /// Send a FastCGI request and return `(stdout, body_offset)` where the
    /// JSON body starts at `stdout[body_offset..]`. Avoids copying the body
    /// out of the full CGI payload.
    async fn send_request(&self, body: &[u8]) -> anyhow::Result<(Vec<u8>, usize)> {
        let params = Params::default()
            .request_method("POST")
            .script_filename(&self.script_filename)
            .script_name(&self.script_name)
            .request_uri(&self.request_uri)
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
                let res = conn
                    .execute(Request::new(params, io::Cursor::new(body)))
                    .await
                    .map_err(anyhow::Error::from);
                detach_on_error(conn, res)?
            }
            FcgiPool::Unix(pool) => {
                let mut conn = pool
                    .get()
                    .await
                    .map_err(|e| anyhow::anyhow!("pool error: {e}"))?;
                debug!("sending FastCGI request via Unix socket");
                let res = conn
                    .execute(Request::new(params, io::Cursor::new(body)))
                    .await
                    .map_err(anyhow::Error::from);
                detach_on_error(conn, res)?
            }
        };

        if let Some(stderr) = &response.stderr
            && !stderr.is_empty()
        {
            warn!("PHP stderr: {}", String::from_utf8_lossy(stderr));
        }

        let stdout = response.stdout.unwrap_or_default();
        let offset = body_start_offset(&stdout);
        Ok((stdout, offset))
    }
}

/// Return the offset at which the JSON body starts inside raw CGI output.
/// CGI prefixes the response with HTTP-style headers terminated by `\r\n\r\n`;
/// if that separator is absent the whole payload is treated as body.
#[doc(hidden)]
pub fn body_start_offset(raw: &[u8]) -> usize {
    const SEP: &[u8] = b"\r\n\r\n";
    match raw.windows(SEP.len()).position(|w| w == SEP) {
        Some(pos) => pos + SEP.len(),
        None => 0,
    }
}

/// Consume `obj` when `result` is `Err` so that `deadpool` does NOT return
/// a potentially-broken FastCGI keep-alive connection to the pool. On `Ok`
/// the object is dropped normally and goes back into the pool.
///
/// This is necessary because `recycle()` cannot detect a stale keep-alive
/// socket without actually issuing a request — the only reliable signal
/// of brokenness is a failing `execute()` call.
fn detach_on_error<M, T, E>(obj: managed::Object<M>, result: Result<T, E>) -> Result<T, E>
where
    M: managed::Manager,
{
    if result.is_err() {
        let _ = managed::Object::take(obj);
    }
    result
}

impl UpstreamExecutor for FastCgiExecutor {
    async fn execute(
        &self,
        request: &UpstreamEnvelope<'_>,
        _ctx: ExecuteContext<'_>,
    ) -> anyhow::Result<UpstreamCallResult> {
        let body = serde_json::to_vec(request)?;
        let (raw, body_start) = self.send_request(&body).await?;
        let body_bytes = &raw[body_start..];
        debug!("FastCGI response: {}", String::from_utf8_lossy(body_bytes));
        UpstreamCallResult::parse(body_bytes).context("failed to parse FastCGI response")
    }

    async fn discover(&self) -> anyhow::Result<UpstreamDiscoverResponse> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let envelope = UpstreamEnvelope {
            session_id: None,
            request_id: &request_id,
            request: UpstreamRequest::Discover,
        };
        let body = serde_json::to_vec(&envelope)?;
        let (raw, body_start) = self.send_request(&body).await?;
        let body_bytes = &raw[body_start..];
        debug!(
            "FastCGI discover response: {}",
            String::from_utf8_lossy(body_bytes)
        );
        let response: UpstreamDiscoverResponse = serde_json::from_slice(body_bytes)
            .context("failed to parse FastCGI discover response")?;
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_body_start_offset_with_headers() {
        let raw = b"Content-Type: application/json\r\n\r\n{\"ok\":true}";
        let offset = body_start_offset(raw);
        assert_eq!(&raw[offset..], b"{\"ok\":true}");
    }

    #[test]
    fn test_body_start_offset_no_headers() {
        let raw = b"{\"ok\":true}";
        let offset = body_start_offset(raw);
        assert_eq!(offset, 0);
        assert_eq!(&raw[offset..], b"{\"ok\":true}");
    }

    #[test]
    fn test_body_start_offset_multiple_headers() {
        let raw = b"Status: 200 OK\r\nContent-Type: application/json\r\n\r\n{\"data\":1}";
        let offset = body_start_offset(raw);
        assert_eq!(&raw[offset..], b"{\"data\":1}");
    }

    #[test]
    fn test_body_start_offset_empty_body() {
        let raw = b"Content-Type: application/json\r\n\r\n";
        let offset = body_start_offset(raw);
        assert_eq!(&raw[offset..], b"");
    }

    #[test]
    fn derive_script_name_from_absolute_path() {
        assert_eq!(
            derive_script_name("/var/www/app/handler.php"),
            "/handler.php"
        );
    }

    #[test]
    fn derive_script_name_from_relative_path() {
        assert_eq!(derive_script_name("examples/handler.php"), "/handler.php");
    }

    #[test]
    fn derive_script_name_from_bare_basename() {
        assert_eq!(derive_script_name("handler.php"), "/handler.php");
    }

    #[test]
    fn derive_script_name_uses_real_basename_not_hardcoded() {
        assert_eq!(derive_script_name("/var/www/bookings/api.php"), "/api.php");
    }

    #[test]
    fn derive_script_name_falls_back_for_degenerate_input() {
        // Trailing slash -> no file name -> fallback.
        assert_eq!(derive_script_name("/"), "/handler.php");
        assert_eq!(derive_script_name(""), "/handler.php");
    }

    /// Exercises the stale-connection-recovery contract that real FastCGI
    /// cannot test without a live PHP-FPM: a pool whose recycle() always
    /// succeeds will happily return a broken keep-alive socket forever
    /// unless the caller explicitly detaches the object on error. We
    /// emulate the pool with a counting mock manager and prove that
    /// `detach_on_error` forces a fresh connection to be created on the
    /// next get().
    #[tokio::test]
    async fn test_detach_on_error_forces_new_connection() {
        use deadpool::managed::{Manager, Metrics, Pool, RecycleResult};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct MockManager(Arc<AtomicUsize>);
        impl Manager for MockManager {
            type Type = usize;
            type Error = std::io::Error;

            async fn create(&self) -> Result<usize, std::io::Error> {
                let id = self.0.fetch_add(1, Ordering::SeqCst);
                Ok(id)
            }
            async fn recycle(
                &self,
                _obj: &mut usize,
                _metrics: &Metrics,
            ) -> RecycleResult<std::io::Error> {
                Ok(())
            }
        }

        let created = Arc::new(AtomicUsize::new(0));
        let pool: Pool<MockManager> = Pool::builder(MockManager(Arc::clone(&created)))
            .max_size(1)
            .build()
            .unwrap();

        // Baseline: dropping the object returns it to the pool.
        drop(pool.get().await.unwrap());
        drop(pool.get().await.unwrap());
        assert_eq!(
            created.load(Ordering::SeqCst),
            1,
            "healthy drop should reuse the existing connection"
        );

        // Simulated failure: the helper consumes the object and therefore
        // prevents it from being recycled back into the pool.
        let err: anyhow::Result<()> = Err(anyhow::anyhow!("boom"));
        let obj = pool.get().await.unwrap();
        let result = detach_on_error(obj, err);
        assert!(result.is_err());

        // Next acquisition must build a brand new connection.
        drop(pool.get().await.unwrap());
        assert_eq!(
            created.load(Ordering::SeqCst),
            2,
            "detach_on_error must prevent a broken connection from being reused"
        );
    }
}
