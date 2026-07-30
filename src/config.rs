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

    /// Skip TLS certificate verification for HTTPS upstreams.
    ///
    /// When set via env (`ROXY_UPSTREAM_INSECURE`) it accepts only the
    /// exact lowercase strings `true` or `false`. Numeric forms such as
    /// `1` / `0`, and other casings such as `TRUE` / `True`, are not
    /// accepted by clap's `SetTrue + env` parser and will fail at
    /// startup. The CLI `--upstream-insecure` flag continues to work
    /// without a value (presence means `true`).
    #[arg(long, env = "ROXY_UPSTREAM_INSECURE")]
    pub upstream_insecure: bool,

    /// Upstream request timeout in seconds
    #[arg(long, env = "ROXY_UPSTREAM_TIMEOUT", default_value = "30")]
    pub upstream_timeout: u64,

    /// Custom HTTP header for upstream requests (repeatable).
    ///
    /// Format: "Name: Value", e.g. "Authorization: Bearer token".
    ///
    /// When set via env (`ROXY_UPSTREAM_HEADER`) multiple headers are
    /// separated by `\n`, which maps naturally onto a Kubernetes YAML
    /// `|-` block scalar. Leading/trailing blank lines are discarded
    /// during startup. Passing the CLI flag at all causes the env
    /// value to be ignored entirely — there is no merging.
    #[arg(
        long,
        env = "ROXY_UPSTREAM_HEADER",
        value_delimiter = '\n',
        num_args = 0..,
    )]
    pub upstream_header: Vec<String>,

    /// Hostname accepted in the `Host` header of inbound MCP requests
    /// (repeatable, only used with `--transport http`).
    ///
    /// The default is loopback-only, which is what stops a malicious web page
    /// from reaching a roxy running on a developer's machine through DNS
    /// rebinding. A roxy behind a reverse proxy usually receives the public
    /// name the client typed, so that name has to be listed here (e.g.
    /// `--allowed-host mcp.example.com`) or those requests are answered with
    /// `403 Forbidden`. Entries may carry a port (`example.com:8080`), which
    /// then has to match too.
    ///
    /// The special value `*` accepts any `Host`, turning the check off. Only do
    /// that when something in front of roxy already validates it. An empty or
    /// blank value is *not* a way to disable it — see [`Config::allowed_hosts`].
    ///
    /// When set via env (`ROXY_ALLOWED_HOST`) entries are separated by `\n`,
    /// like `ROXY_UPSTREAM_HEADER`.
    #[arg(
        long,
        env = "ROXY_ALLOWED_HOST",
        value_delimiter = '\n',
        num_args = 0..,
        default_values = DEFAULT_ALLOWED_HOSTS,
    )]
    pub allowed_host: Vec<String>,

    /// Maximum inbound MCP request body size in bytes (only used with
    /// `--transport http`).
    ///
    /// Bodies over the limit are rejected with `413 Payload Too Large`,
    /// enforced while streaming, so a lying `Content-Length` does not get
    /// around it. Raise it if clients send large tool arguments — an embedded
    /// document or image, say — through to the backend.
    #[arg(
        long,
        env = "ROXY_MAX_BODY_SIZE",
        default_value = "4194304",
        value_parser = parse_body_size,
    )]
    pub max_body_size: usize,

    /// FastCGI connection pool size
    #[arg(long, env = "ROXY_POOL_SIZE", default_value = "16")]
    pub pool_size: usize,

    /// Log output format
    #[arg(long, env = "ROXY_LOG_FORMAT", default_value = "pretty")]
    pub log_format: LogFormat,
}

/// `Host` values accepted when `--allowed-host` is not configured: loopback
/// only, so a page in someone's browser cannot reach a roxy on their machine by
/// pointing a hostname at `127.0.0.1`.
pub const DEFAULT_ALLOWED_HOSTS: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

impl Config {
    /// The `Host` allow-list to enforce, ready to hand to the transport.
    ///
    /// Drops blank entries — a `ROXY_ALLOWED_HOST` that exists but is empty (a
    /// Kubernetes ConfigMap key with no value), a bare `--allowed-host` — and
    /// falls back to [`DEFAULT_ALLOWED_HOSTS`] when nothing usable is left. That
    /// fallback is the point: an empty list tells rmcp to accept *every* host, so
    /// a blank value would silently disable the only DNS-rebinding defence roxy
    /// has, in exactly the case where an operator believes they configured it.
    /// `*` stays the one explicit, greppable way to turn the check off.
    ///
    /// Both the binary and the integration tests go through here, so the policy
    /// under test is the policy that ships.
    pub fn allowed_hosts(&self) -> Vec<String> {
        let hosts = normalize_list(self.allowed_host.clone());
        if hosts.is_empty() {
            return DEFAULT_ALLOWED_HOSTS
                .iter()
                .map(|host| (*host).to_string())
                .collect();
        }
        hosts
    }
}

/// Whether `hosts` turns `Host` validation off entirely. Shared by the startup
/// log and the transport builder so the two cannot disagree about what the
/// configuration means.
pub fn host_validation_disabled(hosts: &[String]) -> bool {
    hosts.iter().any(|host| host == "*")
}

/// Parse `--max-body-size`, rejecting `0` — which would reject every request,
/// never what an operator means, and a confusing way to find out.
fn parse_body_size(raw: &str) -> Result<usize, String> {
    match raw.parse::<usize>() {
        Ok(0) => Err("must be at least 1 byte".to_string()),
        Ok(bytes) => Ok(bytes),
        Err(e) => Err(format!("not a byte count: {e}")),
    }
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
        // URL schemes are case-insensitive (RFC 3986 §3.1), so compare the
        // scheme — the part before "://" — without regard to case. The
        // original `upstream` string is kept verbatim for the URL value.
        let is_http = upstream.split_once("://").is_some_and(|(scheme, _)| {
            scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
        });
        if is_http {
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
///
/// Classified as TCP only when the text after the **last** `:` parses as a
/// `u16` port; everything else is a Unix socket path. See [`FcgiAddress::parse`].
#[derive(Debug, Clone)]
pub enum FcgiAddress {
    Tcp(String),
    Unix(String),
}

impl FcgiAddress {
    /// Classify a FastCGI upstream address as TCP or a Unix socket path.
    ///
    /// The rule is exactly one test, applied to nothing but the text after the
    /// **last** `:`: it is **TCP** if and only if that trailing segment parses
    /// as a `u16` port (`127.0.0.1:9000`, `[::1]:9000`, `:9000`); otherwise it
    /// is a **Unix socket path**. Nothing else is inspected — the host half is
    /// never validated and brackets are not parsed specially. Cases that fall
    /// through to Unix:
    ///
    /// - a bare host with no port (`localhost`) — a TCP upstream must always
    ///   be written `host:port`, so a port-less value is taken literally as a
    ///   socket path;
    /// - a Windows drive-letter path (`C:\fpm.sock`), whose `:` is not a port
    ///   separator;
    /// - a Unix socket path whose last `:`-segment is non-numeric
    ///   (`/run/with:colon.sock`).
    ///
    /// The one residual false positive of the heuristic: a Unix socket path
    /// whose final `:`-segment *is* a valid `u16` (e.g. `/run/sock:1234`) is
    /// classified as **TCP**. This is vanishingly rare for real `.sock` paths;
    /// if you hit it, rename the socket. There is no such ambiguity for a path
    /// ending in a non-numeric component.
    ///
    /// For IPv6 over TCP, use the bracketed `[host]:port` form
    /// (`[::1]:9000` → TCP because the last segment `9000` parses). The
    /// brackets are not parsed; they matter only because they push the port
    /// into the final segment. A bare IPv6 address with no port is resolved by
    /// the same rule: `::1` ends in `:1` (a valid `u16`) → TCP, whereas
    /// `[::1]` ends in `1]` → Unix. Always include the port for an IPv6 TCP
    /// upstream.
    ///
    /// This is deliberately a heuristic: parsing is platform-independent (both
    /// variants exist on every platform); only the connection is
    /// `#[cfg(unix)]`-gated.
    pub fn parse(addr: &str) -> Self {
        let is_tcp = addr
            .rsplit_once(':')
            .is_some_and(|(_, port)| port.parse::<u16>().is_ok());
        if is_tcp {
            Self::Tcp(addr.to_string())
        } else {
            Self::Unix(addr.to_string())
        }
    }
}

/// Drop whitespace-only or empty entries from a repeatable list flag.
///
/// The main input to this helper is a `Vec<String>` that came from clap,
/// where a trailing/leading `\n` in a `\n`-delimited env var (e.g. from a
/// Kubernetes YAML `|-` block scalar) or an empty env var can produce
/// spurious empty slots. Filtering them here keeps the rest of the
/// pipeline simple — and for `ROXY_ALLOWED_HOST` it is load-bearing: a
/// blank entry would otherwise reach rmcp as a host pattern that matches
/// nothing, turning `ROXY_ALLOWED_HOST=` into "reject every request"
/// rather than "no restriction".
pub fn normalize_list(raw: Vec<String>) -> Vec<String> {
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
    fn test_upstream_kind_http_mixed_case_scheme() {
        // URL schemes are case-insensitive (RFC 3986 §3.1): mixed-case
        // schemes must still route to the HTTP executor, and the original
        // URL string must be preserved verbatim.
        for upstream in [
            "HTTP://host/x",
            "HtTpS://host/x",
            "Http://localhost:8000/handler",
            "HTTPS://api.example.com/mcp",
        ] {
            let kind = UpstreamKind::parse(upstream);
            assert!(
                matches!(kind, UpstreamKind::Http { .. }),
                "{upstream} should route to HTTP"
            );
            if let UpstreamKind::Http { url } = kind {
                assert_eq!(url, upstream, "URL value must be kept verbatim");
            }
        }
    }

    #[test]
    fn test_upstream_kind_scheme_must_be_full_prefix() {
        // The scheme must be the whole segment before "://", not merely a
        // substring of it. A non-HTTP scheme that happens to contain "http"
        // must still route to FastCGI — this pins the equivalence to an
        // anchored, case-insensitive `starts_with`.
        for upstream in ["xhttp://host", "ftp://host", "shttps://host"] {
            assert!(
                matches!(UpstreamKind::parse(upstream), UpstreamKind::FastCgi { .. }),
                "{upstream} should route to FastCGI"
            );
        }
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
    fn test_fcgi_address_tcp() {
        // A trailing ":<u16 port>" marks a TCP address, including bracketed
        // IPv6, a port-only bind, and the u16 boundary values.
        for addr in [
            "127.0.0.1:9000",
            "[::1]:9000",
            "localhost:8080",
            ":9000",
            "host:0",     // port 0 is a valid u16
            "host:65535", // u16::MAX — upper boundary
        ] {
            assert!(
                matches!(FcgiAddress::parse(addr), FcgiAddress::Tcp(_)),
                "{addr} should parse as TCP"
            );
        }
    }

    #[test]
    fn test_fcgi_address_unix() {
        // No colon, or a colon whose suffix is not a valid u16 port, is a Unix
        // socket path. This deliberately covers the cases the old
        // `contains(':')` heuristic misclassified.
        for addr in [
            "/var/run/php-fpm.sock", // plain path, no colon
            "localhost",             // bare host, no port → taken as a path
            r"C:\fpm.sock",          // Windows drive path, ':' is not a port
            "/run/with:colon.sock",  // legal Unix path containing ':'
            "host:",                 // empty port
            "host:notaport",         // non-numeric port
            "host:65536",            // just past u16::MAX — exact overflow edge
            "host:99999",            // out-of-range port (> u16::MAX)
            "",                      // empty input → empty Unix path
        ] {
            assert!(
                matches!(FcgiAddress::parse(addr), FcgiAddress::Unix(_)),
                "{addr} should parse as Unix"
            );
        }
    }

    #[test]
    fn test_fcgi_address_numeric_suffix_path_is_documented_false_positive() {
        // Documented limitation: a Unix path whose final `:`-segment is a valid
        // u16 is read as TCP. Pinned so the known sharp edge stays visible and
        // intentional. Real `.sock` paths end in a non-numeric component and
        // are unaffected.
        assert!(
            matches!(FcgiAddress::parse("/run/sock:1234"), FcgiAddress::Tcp(_)),
            "a path ending in :<u16> is classified TCP by the last-segment rule"
        );
        assert!(
            matches!(
                FcgiAddress::parse("/run/php-fpm.sock"),
                FcgiAddress::Unix(_)
            ),
            "a normal .sock path stays Unix"
        );
    }

    #[test]
    fn test_fcgi_address_bare_ipv6_is_documented() {
        // A bare IPv6 address without a port is ambiguous and resolved by the
        // same last-segment rule. These assertions pin the documented
        // behaviour so it stays a deliberate decision, not an accident:
        // `::1` ends in a numeric segment → TCP; `[::1]` ends in `1]` → Unix.
        // The canonical, unambiguous form is bracketed-with-port (`[::1]:9000`).
        assert!(
            matches!(FcgiAddress::parse("::1"), FcgiAddress::Tcp(_)),
            "bare ::1 is classified TCP by the last-segment rule"
        );
        assert!(
            matches!(FcgiAddress::parse("[::1]"), FcgiAddress::Unix(_)),
            "[::1] without a port falls through to Unix"
        );
    }

    #[test]
    fn test_fcgi_address_preserves_value_verbatim() {
        match FcgiAddress::parse("127.0.0.1:9000") {
            FcgiAddress::Tcp(a) => assert_eq!(a, "127.0.0.1:9000"),
            other => panic!("expected Tcp, got {other:?}"),
        }
        match FcgiAddress::parse("/run/with:colon.sock") {
            FcgiAddress::Unix(a) => assert_eq!(a, "/run/with:colon.sock"),
            other => panic!("expected Unix, got {other:?}"),
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
    fn normalize_list_empty() {
        let out = normalize_list(Vec::<String>::new());
        assert!(out.is_empty());
    }

    #[test]
    fn normalize_list_drops_empty_strings() {
        let out = normalize_list(vec!["A: 1".to_string(), "".to_string(), "B: 2".to_string()]);
        assert_eq!(out, vec!["A: 1".to_string(), "B: 2".to_string()]);
    }

    #[test]
    fn normalize_list_drops_whitespace_only() {
        let out = normalize_list(vec!["   ".to_string(), "\t".to_string(), "\n".to_string()]);
        assert!(out.is_empty());
    }

    #[test]
    fn normalize_list_preserves_order_in_mixed_input() {
        let out = normalize_list(vec![
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

    #[test]
    fn env_upstream_insecure_true() {
        temp_env::with_var("ROXY_UPSTREAM_INSECURE", Some("true"), || {
            let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x"]).unwrap();
            assert!(cfg.upstream_insecure);
        });
    }

    #[test]
    fn env_upstream_insecure_false() {
        temp_env::with_var("ROXY_UPSTREAM_INSECURE", Some("false"), || {
            let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x"]).unwrap();
            assert!(!cfg.upstream_insecure);
        });
    }

    #[test]
    fn env_upstream_insecure_rejects_numeric() {
        // clap's SetTrue + env only accepts the strings "true" and "false".
        // Numeric forms like "1" and "0" are deliberately not supported; see
        // the field doc-comment for the exact accepted value set.
        temp_env::with_var("ROXY_UPSTREAM_INSECURE", Some("1"), || {
            assert!(Config::try_parse_from(["roxy", "--upstream", "http://x"]).is_err());
        });
    }

    #[test]
    fn env_upstream_insecure_rejects_zero() {
        // Sibling of rejects_numeric: both "1" and "0" are numeric
        // forms that clap's SetTrue + env parser rejects.
        temp_env::with_var("ROXY_UPSTREAM_INSECURE", Some("0"), || {
            assert!(Config::try_parse_from(["roxy", "--upstream", "http://x"]).is_err());
        });
    }

    #[test]
    fn cli_overrides_env_upstream_insecure() {
        // ROXY_UPSTREAM_INSECURE=false, but CLI passes --upstream-insecure
        // as a flag. CLI should win.
        temp_env::with_var("ROXY_UPSTREAM_INSECURE", Some("false"), || {
            let cfg =
                Config::try_parse_from(["roxy", "--upstream", "http://x", "--upstream-insecure"])
                    .unwrap();
            assert!(cfg.upstream_insecure);
        });
    }

    #[test]
    fn env_upstream_header_single() {
        temp_env::with_var(
            "ROXY_UPSTREAM_HEADER",
            Some("Authorization: Bearer t"),
            || {
                let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x"]).unwrap();
                assert_eq!(
                    cfg.upstream_header,
                    vec!["Authorization: Bearer t".to_string()]
                );
            },
        );
    }

    #[test]
    fn env_upstream_header_newline_split() {
        temp_env::with_var("ROXY_UPSTREAM_HEADER", Some("A: 1\nB: 2"), || {
            let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x"]).unwrap();
            assert_eq!(
                cfg.upstream_header,
                vec!["A: 1".to_string(), "B: 2".to_string()]
            );
        });
    }

    #[test]
    fn env_upstream_header_trailing_newline_trimmed() {
        temp_env::with_var("ROXY_UPSTREAM_HEADER", Some("A: 1\n"), || {
            let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x"]).unwrap();
            let normalized = normalize_list(cfg.upstream_header);
            assert_eq!(normalized, vec!["A: 1".to_string()]);
        });
    }

    #[test]
    fn env_upstream_header_yaml_block_scalar() {
        temp_env::with_var("ROXY_UPSTREAM_HEADER", Some("\nA: 1\nB: 2\n"), || {
            let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x"]).unwrap();
            let normalized = normalize_list(cfg.upstream_header);
            assert_eq!(normalized, vec!["A: 1".to_string(), "B: 2".to_string()]);
        });
    }

    #[test]
    fn cli_header_overrides_env_header() {
        temp_env::with_var("ROXY_UPSTREAM_HEADER", Some("Env: fromenv"), || {
            let cfg = Config::try_parse_from([
                "roxy",
                "--upstream",
                "http://x",
                "--upstream-header",
                "Cli: fromcli",
            ])
            .unwrap();
            assert_eq!(cfg.upstream_header, vec!["Cli: fromcli".to_string()]);
        });
    }

    #[test]
    fn env_upstream_header_empty_string_normalized() {
        // Pin the pipeline contract: when the env var is set to "" (which
        // happens in Kubernetes when a ConfigMap key exists but has no
        // content), clap produces a one-element vec containing the empty
        // string. Task 6's normalize_list call in main.rs must
        // collapse that to an empty vec before any downstream consumer
        // tries to parse it as a "Name: Value" header.
        temp_env::with_var("ROXY_UPSTREAM_HEADER", Some(""), || {
            let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x"]).unwrap();
            assert_eq!(cfg.upstream_header, vec!["".to_string()]);
            assert!(normalize_list(cfg.upstream_header).is_empty());
        });
    }

    // --- inbound HTTP policy ---

    /// Parse with the two policy env vars cleared, so a developer who exports
    /// them does not get confusing failures.
    fn parse_clean(args: &[&str]) -> Config {
        temp_env::with_vars_unset(["ROXY_ALLOWED_HOST", "ROXY_MAX_BODY_SIZE"], || {
            Config::try_parse_from(args).unwrap()
        })
    }

    #[test]
    fn allowed_hosts_defaults_to_loopback() {
        let cfg = parse_clean(&["roxy", "--upstream", "http://x"]);
        assert_eq!(cfg.allowed_hosts(), DEFAULT_ALLOWED_HOSTS.to_vec());
        assert!(!host_validation_disabled(&cfg.allowed_hosts()));
    }

    #[test]
    fn allowed_hosts_takes_repeated_cli_values() {
        let cfg = parse_clean(&[
            "roxy",
            "--upstream",
            "http://x",
            "--allowed-host",
            "a.example.com",
            "--allowed-host",
            "b.example.com:8443",
        ]);
        assert_eq!(
            cfg.allowed_hosts(),
            vec![
                "a.example.com".to_string(),
                "b.example.com:8443".to_string()
            ]
        );
    }

    #[test]
    fn env_allowed_host_newline_split() {
        temp_env::with_var(
            "ROXY_ALLOWED_HOST",
            Some("a.example.com\nb.example.com"),
            || {
                let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x"]).unwrap();
                assert_eq!(
                    cfg.allowed_hosts(),
                    vec!["a.example.com".to_string(), "b.example.com".to_string()]
                );
            },
        );
    }

    /// The security-relevant case. rmcp reads an empty allow-list as "accept
    /// every host", so a blank `ROXY_ALLOWED_HOST` — a ConfigMap key that exists
    /// with no content — must fall back to loopback rather than fail open.
    #[test]
    fn env_allowed_host_empty_falls_back_to_loopback() {
        temp_env::with_var("ROXY_ALLOWED_HOST", Some(""), || {
            let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x"]).unwrap();
            assert_eq!(cfg.allowed_host, vec!["".to_string()]);
            assert_eq!(cfg.allowed_hosts(), DEFAULT_ALLOWED_HOSTS.to_vec());
            assert!(!host_validation_disabled(&cfg.allowed_hosts()));
        });
    }

    /// …and the only way to actually turn the check off stays explicit.
    #[test]
    fn wildcard_allowed_host_disables_validation() {
        let cfg = parse_clean(&["roxy", "--upstream", "http://x", "--allowed-host", "*"]);
        assert!(host_validation_disabled(&cfg.allowed_hosts()));
    }

    #[test]
    fn cli_allowed_host_overrides_env() {
        temp_env::with_var("ROXY_ALLOWED_HOST", Some("fromenv.example.com"), || {
            let cfg = Config::try_parse_from([
                "roxy",
                "--upstream",
                "http://x",
                "--allowed-host",
                "fromcli.example.com",
            ])
            .unwrap();
            assert_eq!(cfg.allowed_hosts(), vec!["fromcli.example.com".to_string()]);
        });
    }

    #[test]
    fn max_body_size_defaults_to_4_mib() {
        let cfg = parse_clean(&["roxy", "--upstream", "http://x"]);
        assert_eq!(cfg.max_body_size, 4 * 1024 * 1024);
    }

    #[test]
    fn env_max_body_size_parsed() {
        temp_env::with_var("ROXY_MAX_BODY_SIZE", Some("1048576"), || {
            let cfg = Config::try_parse_from(["roxy", "--upstream", "http://x"]).unwrap();
            assert_eq!(cfg.max_body_size, 1048576);
        });
    }

    /// Zero would reject every request; failing at startup beats debugging a
    /// 413 on an empty body.
    #[test]
    fn max_body_size_rejects_zero() {
        temp_env::with_vars_unset(["ROXY_MAX_BODY_SIZE"], || {
            let err =
                Config::try_parse_from(["roxy", "--upstream", "http://x", "--max-body-size", "0"])
                    .expect_err("zero must not parse");
            assert!(
                err.to_string().contains("at least 1 byte"),
                "error must say why, got: {err}"
            );
        });
    }
}
