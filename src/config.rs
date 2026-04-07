use clap::Parser;

/// roxy — high-performance MCP proxy server
///
/// Bridges MCP clients to any backend via FastCGI or HTTP.
/// The upstream type is auto-detected from the URL format:
///   http(s)://...   → HTTP executor
///   host:port       → FastCGI TCP
///   /path/to/socket → FastCGI Unix socket
#[derive(Parser, Debug)]
#[command(name = "roxy", version)]
pub struct Config {
    /// MCP transport mode for client connections
    #[arg(long, default_value = "stdio")]
    pub transport: Transport,

    /// MCP HTTP listen port (only used with --transport http)
    #[arg(long, default_value = "8080")]
    pub port: u16,

    /// Backend URL. Auto-detects executor type:
    ///   http(s)://...   → HTTP
    ///   host:port       → FastCGI TCP
    ///   /path/to/socket → FastCGI Unix
    #[arg(long)]
    pub upstream: String,

    /// Script path sent as SCRIPT_FILENAME to FastCGI backend
    #[arg(long)]
    pub upstream_entrypoint: Option<String>,

    /// Skip TLS certificate verification for HTTPS upstreams
    #[arg(long, default_value = "false")]
    pub upstream_insecure: bool,

    /// Upstream request timeout in seconds
    #[arg(long, default_value = "30")]
    pub upstream_timeout: u64,

    /// Custom HTTP header for upstream requests (repeatable).
    /// Format: "Name: Value", e.g. "Authorization: Bearer token"
    #[arg(long)]
    pub upstream_header: Vec<String>,

    /// Connection pool size (FastCGI) or max idle connections (HTTP)
    #[arg(long, default_value = "16")]
    pub pool_size: usize,

    /// Log output format
    #[arg(long, default_value = "pretty")]
    pub log_format: LogFormat,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum Transport {
    Stdio,
    Http,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum LogFormat {
    Pretty,
    Json,
}

/// Upstream backend type, determined by URL format.
#[derive(Debug, Clone)]
pub enum UpstreamKind {
    Http { url: String },
    FastCgi { address: FcgiAddress },
}

impl UpstreamKind {
    pub fn parse(upstream: &str) -> Self {
        if upstream.starts_with("http://") || upstream.starts_with("https://") {
            Self::Http {
                url: upstream.to_string(),
            }
        } else {
            Self::FastCgi {
                address: FcgiAddress::parse(upstream),
            }
        }
    }
}

/// FastCGI connection address.
/// Contains `:` → TCP, otherwise → Unix socket path.
#[derive(Debug, Clone)]
pub enum FcgiAddress {
    Tcp(String),
    Unix(String),
}

impl FcgiAddress {
    pub fn parse(addr: &str) -> Self {
        if addr.contains(':') {
            Self::Tcp(addr.to_string())
        } else {
            Self::Unix(addr.to_string())
        }
    }
}

/// Parse a "Name: Value" header string into (name, value) tuple.
pub fn parse_header(s: &str) -> anyhow::Result<(String, String)> {
    let pos = s
        .find(':')
        .ok_or_else(|| anyhow::anyhow!("invalid header format: expected 'Name: Value', got '{s}'"))?;
    let name = s[..pos].trim().to_string();
    let value = s[pos + 1..].trim().to_string();
    Ok((name, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upstream_kind_http() {
        let kind = UpstreamKind::parse("http://localhost:8000/handler");
        assert!(matches!(kind, UpstreamKind::Http { .. }));
        if let UpstreamKind::Http { url } = kind {
            assert_eq!(url, "http://localhost:8000/handler");
        }
    }

    #[test]
    fn test_upstream_kind_https() {
        let kind = UpstreamKind::parse("https://api.example.com/mcp");
        assert!(matches!(kind, UpstreamKind::Http { .. }));
    }

    #[test]
    fn test_upstream_kind_fastcgi_tcp() {
        let kind = UpstreamKind::parse("127.0.0.1:9000");
        assert!(matches!(kind, UpstreamKind::FastCgi { .. }));
        if let UpstreamKind::FastCgi { address } = kind {
            assert!(matches!(address, FcgiAddress::Tcp(_)));
        }
    }

    #[test]
    fn test_upstream_kind_fastcgi_unix() {
        let kind = UpstreamKind::parse("/var/run/php-fpm.sock");
        assert!(matches!(kind, UpstreamKind::FastCgi { .. }));
        if let UpstreamKind::FastCgi { address } = kind {
            assert!(matches!(address, FcgiAddress::Unix(_)));
        }
    }

    #[test]
    fn test_parse_header_with_space() {
        let (name, value) = parse_header("Authorization: Bearer token123").unwrap();
        assert_eq!(name, "Authorization");
        assert_eq!(value, "Bearer token123");
    }

    #[test]
    fn test_parse_header_without_space() {
        let (name, value) = parse_header("X-Key:value").unwrap();
        assert_eq!(name, "X-Key");
        assert_eq!(value, "value");
    }

    #[test]
    fn test_parse_header_multiple_colons() {
        let (name, value) = parse_header("X-Data: a:b:c").unwrap();
        assert_eq!(name, "X-Data");
        assert_eq!(value, "a:b:c");
    }

    #[test]
    fn test_parse_header_invalid() {
        assert!(parse_header("no-colon-here").is_err());
    }
}
