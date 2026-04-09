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

/// Convert an HTTP header name to its CGI `HTTP_*` parameter name per
/// RFC 3875 §4.1.18: upper-case and replace `-` with `_`, then prefix
/// with `HTTP_`. Input is assumed ASCII — the `http` crate enforces
/// ASCII on `HeaderName` construction, so callers always pass ASCII.
#[doc(hidden)]
pub fn cgi_header_param(name: &str) -> String {
    let mut out = String::with_capacity(5 + name.len());
    out.push_str("HTTP_");
    for b in name.bytes() {
        let mapped = if b == b'-' {
            b'_'
        } else {
            b.to_ascii_uppercase()
        };
        out.push(mapped as char);
    }
    out
}

/// Construct the full `Params` map for a FastCGI request. Pure function
/// (no I/O, no pool access) so the CGI-param shape can be unit-tested
/// without a live PHP-FPM. Takes the body slice directly so
/// `CONTENT_LENGTH` is computed from the same bytes PHP-FPM will see —
/// no opportunity for a caller to pass a mismatched length.
///
/// Forward headers — when present — are added as `HTTP_*` CGI variables
/// per RFC 3875 §4.1.18. Non-UTF-8 header values are silently dropped,
/// since CGI params must be ASCII and MCP clients in practice only send
/// text headers. Multi-valued headers (e.g. two `X-Forwarded-For`
/// entries from an upstream proxy chain) are joined with `", "` to
/// match nginx's `$http_*` variable semantics; CGI has no native
/// multi-value representation and last-write-wins would silently drop
/// proxy-chain history.
fn build_fcgi_params<'a>(
    script_filename: &'a str,
    script_name: &'a str,
    request_uri: &'a str,
    body: &[u8],
    forward_headers: Option<&http::HeaderMap>,
) -> Params<'a> {
    let mut params = Params::default()
        .request_method("POST")
        .script_filename(script_filename)
        .script_name(script_name)
        .request_uri(request_uri)
        .content_type("application/json")
        .content_length(body.len())
        .server_name("localhost")
        .server_port(0);

    if let Some(headers) = forward_headers {
        // Iterate unique header names via `keys()`, then collect every
        // UTF-8-valid value via `get_all()`. This preserves multi-value
        // semantics through the CGI boundary.
        for name in headers.keys() {
            let values: Vec<&str> = headers
                .get_all(name)
                .iter()
                .filter_map(|v| v.to_str().ok())
                .collect();
            if values.is_empty() {
                continue;
            }
            params = params.custom(cgi_header_param(name.as_str()), values.join(", "));
        }
    }

    params
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
    async fn send_request(
        &self,
        body: &[u8],
        ctx: ExecuteContext<'_>,
    ) -> anyhow::Result<(Vec<u8>, usize)> {
        let params = build_fcgi_params(
            &self.script_filename,
            &self.script_name,
            &self.request_uri,
            body,
            ctx.forward_headers,
        );

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
        ctx: ExecuteContext<'_>,
    ) -> anyhow::Result<UpstreamCallResult> {
        let body = serde_json::to_vec(request)?;
        let (raw, body_start) = self.send_request(&body, ctx).await?;
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
        // Discovery runs once at startup — no client, no forward headers.
        let (raw, body_start) = self.send_request(&body, ExecuteContext::default()).await?;
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

    /// Regression guard for the `SCRIPT_FILENAME` byte-for-byte invariant
    /// and for the `new()` field-plumbing: constructing an executor with a
    /// non-`handler.php` entrypoint must store `script_filename` unchanged
    /// and derive `script_name` / `request_uri` from its basename. Uses a
    /// non-listening TCP address — `deadpool` pool construction is lazy
    /// (no I/O) so this succeeds without PHP-FPM.
    #[test]
    fn executor_new_derives_cgi_fields_from_entrypoint() {
        let addr = FcgiAddress::Tcp("127.0.0.1:0".to_string());
        let entrypoint = "/var/www/bookings/api.php".to_string();
        let ex = FastCgiExecutor::new(&addr, entrypoint.clone(), 1).unwrap();
        assert_eq!(
            ex.script_filename, entrypoint,
            "SCRIPT_FILENAME must be byte-for-byte"
        );
        assert_eq!(ex.script_name, "/api.php");
        assert_eq!(ex.request_uri, "/api.php");
    }

    #[test]
    fn cgi_header_param_standard_names() {
        assert_eq!(cgi_header_param("Authorization"), "HTTP_AUTHORIZATION");
        assert_eq!(cgi_header_param("X-My-Thing"), "HTTP_X_MY_THING");
        assert_eq!(cgi_header_param("Accept-Language"), "HTTP_ACCEPT_LANGUAGE");
    }

    #[test]
    fn cgi_header_param_lowercase_input() {
        // http::HeaderName normalizes to lowercase on the wire, so this is
        // the realistic shape of the input we receive.
        assert_eq!(cgi_header_param("authorization"), "HTTP_AUTHORIZATION");
        assert_eq!(cgi_header_param("x-my-thing"), "HTTP_X_MY_THING");
        assert_eq!(cgi_header_param("mcp-session-id"), "HTTP_MCP_SESSION_ID");
    }

    #[test]
    fn cgi_header_param_empty_input() {
        assert_eq!(cgi_header_param(""), "HTTP_");
    }

    #[test]
    fn build_fcgi_params_includes_script_fields_and_defaults() {
        let script_filename = "/var/www/app/handler.php";
        let script_name = "/handler.php";
        let request_uri = "/handler.php";
        let body = [0u8; 42];
        let params = build_fcgi_params(script_filename, script_name, request_uri, &body, None);

        assert_eq!(
            params.get("SCRIPT_FILENAME").unwrap().as_ref(),
            script_filename
        );
        assert_eq!(params.get("SCRIPT_NAME").unwrap().as_ref(), script_name);
        assert_eq!(params.get("REQUEST_URI").unwrap().as_ref(), request_uri);
        assert_eq!(params.get("REQUEST_METHOD").unwrap().as_ref(), "POST");
        assert_eq!(
            params.get("CONTENT_TYPE").unwrap().as_ref(),
            "application/json"
        );
        assert_eq!(params.get("CONTENT_LENGTH").unwrap().as_ref(), "42");
    }

    #[test]
    fn build_fcgi_params_applies_forward_headers_as_http_star() {
        use http::header::{HeaderMap, HeaderName, HeaderValue};

        let mut forward = HeaderMap::new();
        forward.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer xyz"),
        );
        forward.insert(
            HeaderName::from_static("x-my-custom"),
            HeaderValue::from_static("value"),
        );
        forward.insert(
            HeaderName::from_static("mcp-session-id"),
            HeaderValue::from_static("sess-1"),
        );

        let params = build_fcgi_params(
            "/var/www/handler.php",
            "/handler.php",
            "/handler.php",
            b"",
            Some(&forward),
        );

        assert_eq!(
            params.get("HTTP_AUTHORIZATION").unwrap().as_ref(),
            "Bearer xyz"
        );
        assert_eq!(params.get("HTTP_X_MY_CUSTOM").unwrap().as_ref(), "value");
        assert_eq!(
            params.get("HTTP_MCP_SESSION_ID").unwrap().as_ref(),
            "sess-1"
        );
    }

    #[test]
    fn build_fcgi_params_skips_non_utf8_header_values() {
        use http::header::{HeaderMap, HeaderName, HeaderValue};

        let mut forward = HeaderMap::new();
        forward.insert(
            HeaderName::from_static("x-binary"),
            HeaderValue::from_bytes(&[0xff, 0xfe]).unwrap(),
        );
        forward.insert(
            HeaderName::from_static("x-text"),
            HeaderValue::from_static("ok"),
        );

        let params = build_fcgi_params(
            "/var/www/handler.php",
            "/handler.php",
            "/handler.php",
            b"",
            Some(&forward),
        );

        assert!(params.get("HTTP_X_BINARY").is_none());
        assert_eq!(params.get("HTTP_X_TEXT").unwrap().as_ref(), "ok");
    }

    #[test]
    fn build_fcgi_params_coalesces_multi_value_headers() {
        // filter_forward_headers in server.rs preserves multi-valued
        // headers via append; build_fcgi_params must coalesce them into
        // a single HTTP_* variable because CGI has no multi-value
        // representation. Join with ", " matches nginx's $http_*
        // semantics and preserves proxy-chain history for headers like
        // X-Forwarded-For.
        use http::header::{HeaderMap, HeaderName, HeaderValue};

        let mut forward = HeaderMap::new();
        forward.append(
            HeaderName::from_static("x-forwarded-for"),
            HeaderValue::from_static("10.0.0.1"),
        );
        forward.append(
            HeaderName::from_static("x-forwarded-for"),
            HeaderValue::from_static("10.0.0.2"),
        );

        let params = build_fcgi_params(
            "/var/www/handler.php",
            "/handler.php",
            "/handler.php",
            b"",
            Some(&forward),
        );

        assert_eq!(
            params.get("HTTP_X_FORWARDED_FOR").unwrap().as_ref(),
            "10.0.0.1, 10.0.0.2"
        );
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
