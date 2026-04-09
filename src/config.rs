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
    #[arg(long, env = "ROXY_TRANSPORT", default_value = "stdio")]
    pub transport: Transport,

    /// MCP HTTP listen port (only used with --transport http)
    #[arg(long, env = "ROXY_PORT", default_value = "8080")]
    pub port: u16,

    /// Backend URL. Auto-detects executor type:
    ///   http(s)://...   → HTTP
    ///   host:port       → FastCGI TCP
    ///   /path/to/socket → FastCGI Unix
    #[arg(long, env = "ROXY_UPSTREAM")]
    pub upstream: String,

    /// Script path sent as SCRIPT_FILENAME to FastCGI backend
    #[arg(long, env = "ROXY_UPSTREAM_ENTRYPOINT")]
    pub upstream_entrypoint: Option<String>,

    /// Skip TLS certificate verification for HTTPS upstreams
    #[arg(long, default_value = "false")]
    pub upstream_insecure: bool,

    /// Upstream request timeout in seconds
    #[arg(long, env = "ROXY_UPSTREAM_TIMEOUT", default_value = "30")]
    pub upstream_timeout: u64,

    /// Custom HTTP header for upstream requests (repeatable).
    /// Format: "Name: Value", e.g. "Authorization: Bearer token"
    #[arg(long)]
    pub upstream_header: Vec<String>,

    /// FastCGI connection pool size
    #[arg(long, env = "ROXY_POOL_SIZE", default_value = "16")]
    pub pool_size: usize,

    /// Log output format
    #[arg(long, env = "ROXY_LOG_FORMAT", default_value = "pretty")]
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

/// Drop whitespace-only or empty entries from a header list.
///
/// The main input to this helper is a `Vec<String>` that came from clap,
/// where a trailing/leading `\n` in `ROXY_UPSTREAM_HEADER` (e.g. from a
/// Kubernetes YAML `|-` block scalar) or an empty env var can produce
/// spurious empty slots. Filtering them here keeps the rest of the
/// pipeline simple.
pub fn normalize_header_list(raw: Vec<String>) -> Vec<String> {
    raw.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

/// Parse a "Name: Value" header string into (name, value) tuple.
pub fn parse_header(s: &str) -> anyhow::Result<(String, String)> {
    let pos = s.find(':').ok_or_else(|| {
        anyhow::anyhow!("invalid header format: expected 'Name: Value', got '{s}'")
    })?;
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

    #[test]
    fn normalize_header_list_empty() {
        let out = normalize_header_list(Vec::<String>::new());
        assert!(out.is_empty());
    }

    #[test]
    fn normalize_header_list_drops_empty_strings() {
        let out =
            normalize_header_list(vec!["A: 1".to_string(), "".to_string(), "B: 2".to_string()]);
        assert_eq!(out, vec!["A: 1".to_string(), "B: 2".to_string()]);
    }

    #[test]
    fn normalize_header_list_drops_whitespace_only() {
        let out =
            normalize_header_list(vec!["   ".to_string(), "\t".to_string(), "\n".to_string()]);
        assert!(out.is_empty());
    }

    #[test]
    fn normalize_header_list_preserves_order_in_mixed_input() {
        let out = normalize_header_list(vec![
            "".to_string(),
            "A: 1".to_string(),
            "   ".to_string(),
            "B: 2".to_string(),
            "\n".to_string(),
            "C: 3".to_string(),
        ]);
        assert_eq!(
            out,
            vec!["A: 1".to_string(), "B: 2".to_string(), "C: 3".to_string()]
        );
    }

    #[test]
    fn env_transport_http() {
        temp_env::with_var("ROXY_TRANSPORT", Some("http"), || {
            let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x"]).unwrap();
            assert!(matches!(cfg.transport, Transport::Http));
        });
    }

    #[test]
    fn env_port_parsed_as_u16() {
        temp_env::with_var("ROXY_PORT", Some("9999"), || {
            let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x"]).unwrap();
            assert_eq!(cfg.port, 9999);
        });
    }

    #[test]
    fn env_port_invalid_fails() {
        temp_env::with_var("ROXY_PORT", Some("not-a-number"), || {
            assert!(Config::try_parse_from(["roxy", "--upstream", "http://x"]).is_err());
        });
    }

    #[test]
    fn cli_overrides_env_port() {
        temp_env::with_var("ROXY_PORT", Some("9999"), || {
            let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x", "--port", "7777"])
                .unwrap();
            assert_eq!(cfg.port, 7777);
        });
    }

    #[test]
    fn env_upstream_required_can_come_from_env() {
        temp_env::with_var("ROXY_UPSTREAM", Some("http://env-only"), || {
            let cfg = Config::try_parse_from(["roxy"]).unwrap();
            assert_eq!(cfg.upstream, "http://env-only");
        });
    }

    #[test]
    fn env_upstream_entrypoint() {
        temp_env::with_var("ROXY_UPSTREAM_ENTRYPOINT", Some("/srv/handler.php"), || {
            let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x"]).unwrap();
            assert_eq!(cfg.upstream_entrypoint.as_deref(), Some("/srv/handler.php"));
        });
    }

    #[test]
    fn env_upstream_timeout() {
        temp_env::with_var("ROXY_UPSTREAM_TIMEOUT", Some("45"), || {
            let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x"]).unwrap();
            assert_eq!(cfg.upstream_timeout, 45);
        });
    }

    #[test]
    fn env_pool_size_parsed() {
        temp_env::with_var("ROXY_POOL_SIZE", Some("64"), || {
            let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x"]).unwrap();
            assert_eq!(cfg.pool_size, 64);
        });
    }

    #[test]
    fn env_log_format_json() {
        temp_env::with_var("ROXY_LOG_FORMAT", Some("json"), || {
            let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x"]).unwrap();
            assert!(matches!(cfg.log_format, LogFormat::Json));
        });
    }

    #[test]
    fn defaults_when_no_cli_no_env() {
        let vars: Vec<(&str, Option<&str>)> = vec![
            ("ROXY_TRANSPORT", None),
            ("ROXY_PORT", None),
            ("ROXY_UPSTREAM", None),
            ("ROXY_UPSTREAM_ENTRYPOINT", None),
            ("ROXY_UPSTREAM_INSECURE", None),
            ("ROXY_UPSTREAM_TIMEOUT", None),
            ("ROXY_UPSTREAM_HEADER", None),
            ("ROXY_POOL_SIZE", None),
            ("ROXY_LOG_FORMAT", None),
        ];
        temp_env::with_vars(vars, || {
            let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x"]).unwrap();
            assert!(matches!(cfg.transport, Transport::Stdio));
            assert_eq!(cfg.port, 8080);
            assert!(cfg.upstream_entrypoint.is_none());
            assert!(!cfg.upstream_insecure);
            assert_eq!(cfg.upstream_timeout, 30);
            assert!(cfg.upstream_header.is_empty());
            assert_eq!(cfg.pool_size, 16);
            assert!(matches!(cfg.log_format, LogFormat::Pretty));
        });
    }
}
