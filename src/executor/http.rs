use anyhow::Context;
use reqwest::Client;
use tracing::debug;

use crate::config::parse_header;
use crate::protocol::{UpstreamCallResult, UpstreamDiscoverResponse, UpstreamEnvelope, UpstreamRequest};

use super::UpstreamExecutor;

pub struct HttpExecutor {
    client: Client,
    url: String,
}

impl HttpExecutor {
    pub fn new(
        url: String,
        timeout_secs: u64,
        insecure: bool,
        raw_headers: &[String],
    ) -> anyhow::Result<Self> {
        let mut default_headers = reqwest::header::HeaderMap::new();
        default_headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("application/json"),
        );

        for raw in raw_headers {
            let (name, value) = parse_header(raw)?;
            let header_name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .context(format!("invalid header name: {name}"))?;
            let header_value = reqwest::header::HeaderValue::from_str(&value)
                .context(format!("invalid header value for {name}"))?;
            default_headers.insert(header_name, header_value);
        }

        let client = Client::builder()
            .default_headers(default_headers)
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .danger_accept_invalid_certs(insecure)
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self { client, url })
    }
}

impl UpstreamExecutor for HttpExecutor {
    async fn execute(&self, request: &UpstreamEnvelope<'_>) -> anyhow::Result<UpstreamCallResult> {
        let body = serde_json::to_vec(request)?;
        debug!("sending HTTP request to {}", self.url);

        let response = self
            .client
            .post(&self.url)
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
        debug!("HTTP discover response: {}", String::from_utf8_lossy(&bytes));

        let response: UpstreamDiscoverResponse = serde_json::from_slice(&bytes)
            .context("failed to parse upstream discover response")?;
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_executor_new_valid() {
        let executor = HttpExecutor::new(
            "http://localhost:8000/handler".to_string(),
            30,
            false,
            &[],
        );
        assert!(executor.is_ok());
    }

    #[test]
    fn test_http_executor_new_with_headers() {
        let executor = HttpExecutor::new(
            "http://localhost:8000/handler".to_string(),
            30,
            false,
            &["Authorization: Bearer token".to_string(), "X-Custom: value".to_string()],
        );
        assert!(executor.is_ok());
    }

    #[test]
    fn test_http_executor_new_insecure() {
        let executor = HttpExecutor::new(
            "https://localhost:8443/handler".to_string(),
            10,
            true,
            &[],
        );
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
}
