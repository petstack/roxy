use anyhow::Context;
use reqwest::Client;
use tracing::debug;

use crate::config::parse_header;
use crate::protocol::{
    UpstreamCallResult, UpstreamDiscoverResponse, UpstreamEnvelope, UpstreamRequest,
};

use super::{ExecuteContext, UpstreamExecutor};

/// Merge the static `--upstream-header` set with the per-request
/// forward-header set. Forward headers **replace** static headers on
/// name collisions: the client's per-request identity is more specific
/// than roxy's default service identity.
///
/// `reqwest::header::HeaderMap` is a re-export of `http::HeaderMap`
/// (same type underneath), so the argument types are interchangeable —
/// the differentiated names here make the cross-crate boundary explicit.
fn merge_forward_headers(
    static_headers: &reqwest::header::HeaderMap,
    forward: Option<&http::HeaderMap>,
) -> reqwest::header::HeaderMap {
    let mut out = static_headers.clone();
    if let Some(extra) = forward {
        for (name, value) in extra {
            // `insert` removes every existing entry for this name and
            // replaces it with the new value. `append` would keep the
            // static entry, which is the wrong semantic here.
            out.insert(name.clone(), value.clone());
        }
    }
    out
}

pub struct HttpExecutor {
    client: Client,
    url: String,
    static_headers: reqwest::header::HeaderMap,
}

impl HttpExecutor {
    pub fn new(
        url: String,
        timeout_secs: u64,
        insecure: bool,
        raw_headers: &[String],
    ) -> anyhow::Result<Self> {
        let mut client_default_headers = reqwest::header::HeaderMap::new();
        client_default_headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );

        let mut static_headers = reqwest::header::HeaderMap::new();
        for raw in raw_headers {
            let (name, value) = parse_header(raw)?;
            let header_name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .context(format!("invalid header name: {name}"))?;
            let header_value = reqwest::header::HeaderValue::from_str(&value)
                .context(format!("invalid header value for {name}"))?;
            static_headers.insert(header_name, header_value);
        }

        let client = Client::builder()
            .default_headers(client_default_headers)
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .danger_accept_invalid_certs(insecure)
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self {
            client,
            url,
            static_headers,
        })
    }
}

impl UpstreamExecutor for HttpExecutor {
    async fn execute(
        &self,
        request: &UpstreamEnvelope<'_>,
        ctx: ExecuteContext<'_>,
    ) -> anyhow::Result<UpstreamCallResult> {
        let body = serde_json::to_vec(request)?;
        debug!("sending HTTP request to {}", self.url);

        let headers = merge_forward_headers(&self.static_headers, ctx.forward_headers);

        let response = self
            .client
            .post(&self.url)
            .headers(headers)
            .body(body)
            .send()
            .await
            .context("HTTP request to upstream failed")?;

        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("upstream returned HTTP {status}");
        }

        let bytes = response
            .bytes()
            .await
            .context("failed to read upstream response body")?;
        debug!("HTTP response: {}", String::from_utf8_lossy(&bytes));
        UpstreamCallResult::parse(&bytes).context("failed to parse upstream response")
    }

    async fn discover(&self) -> anyhow::Result<UpstreamDiscoverResponse> {
        // Startup handshake — no incoming client, static headers only.
        let request_id = uuid::Uuid::new_v4().to_string();
        let envelope = UpstreamEnvelope {
            session_id: None,
            request_id: &request_id,
            request: UpstreamRequest::Discover,
        };

        let body = serde_json::to_vec(&envelope)?;
        debug!("sending HTTP discover request to {}", self.url);

        let response = self
            .client
            .post(&self.url)
            .headers(self.static_headers.clone())
            .body(body)
            .send()
            .await
            .context("HTTP discover request failed")?;

        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("upstream returned HTTP {status} during discover");
        }

        let bytes = response
            .bytes()
            .await
            .context("failed to read discover response body")?;
        debug!(
            "HTTP discover response: {}",
            String::from_utf8_lossy(&bytes)
        );

        let response: UpstreamDiscoverResponse =
            serde_json::from_slice(&bytes).context("failed to parse upstream discover response")?;
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_executor_new_valid() {
        let executor =
            HttpExecutor::new("http://localhost:8000/handler".to_string(), 30, false, &[]);
        assert!(executor.is_ok());
    }

    #[test]
    fn test_http_executor_new_with_headers() {
        let executor = HttpExecutor::new(
            "http://localhost:8000/handler".to_string(),
            30,
            false,
            &[
                "Authorization: Bearer token".to_string(),
                "X-Custom: value".to_string(),
            ],
        );
        assert!(executor.is_ok());
    }

    #[test]
    fn test_http_executor_new_insecure() {
        let executor =
            HttpExecutor::new("https://localhost:8443/handler".to_string(), 10, true, &[]);
        assert!(executor.is_ok());
    }

    #[test]
    fn test_http_executor_new_invalid_header() {
        let executor = HttpExecutor::new(
            "http://localhost:8000".to_string(),
            30,
            false,
            &["no-colon-here".to_string()],
        );
        assert!(executor.is_err());
    }

    #[test]
    fn merge_forward_headers_returns_static_when_forward_is_none() {
        use http::header::{HeaderMap, HeaderName, HeaderValue};

        let mut static_headers = HeaderMap::new();
        static_headers.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer static"),
        );
        static_headers.insert(
            HeaderName::from_static("x-service"),
            HeaderValue::from_static("roxy"),
        );

        let merged = merge_forward_headers(&static_headers, None);

        assert_eq!(merged.len(), 2);
        assert_eq!(merged.get("authorization").unwrap(), "Bearer static");
        assert_eq!(merged.get("x-service").unwrap(), "roxy");
    }

    #[test]
    fn merge_forward_headers_adds_forward_headers_without_collision() {
        use http::header::{HeaderMap, HeaderName, HeaderValue};

        let mut static_headers = HeaderMap::new();
        static_headers.insert(
            HeaderName::from_static("x-service"),
            HeaderValue::from_static("roxy"),
        );

        let mut forward = HeaderMap::new();
        forward.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer client"),
        );

        let merged = merge_forward_headers(&static_headers, Some(&forward));

        assert_eq!(merged.get("x-service").unwrap(), "roxy");
        assert_eq!(merged.get("authorization").unwrap(), "Bearer client");
    }

    #[test]
    fn merge_forward_headers_forward_overrides_static_on_collision() {
        use http::header::{HeaderMap, HeaderName, HeaderValue};

        let mut static_headers = HeaderMap::new();
        static_headers.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer static"),
        );

        let mut forward = HeaderMap::new();
        forward.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer client"),
        );

        let merged = merge_forward_headers(&static_headers, Some(&forward));

        // Exactly one Authorization header, and it's the client's.
        assert_eq!(merged.get_all("authorization").iter().count(), 1);
        assert_eq!(merged.get("authorization").unwrap(), "Bearer client");
    }
}
