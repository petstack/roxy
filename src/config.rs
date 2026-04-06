use clap::Parser;

/// roxy -- high-performance MCP server with PHP backend
#[derive(Parser, Debug)]
#[command(name = "roxy", version)]
pub struct Config {
    /// Transport mode
    #[arg(long, default_value = "stdio")]
    pub transport: Transport,

    /// HTTP listen port (for http transport)
    #[arg(long, default_value = "8080")]
    pub port: u16,

    /// PHP-FPM address. TCP: "127.0.0.1:9000", Unix: "/var/run/php-fpm.sock"
    #[arg(long, default_value = "127.0.0.1:9000")]
    pub php_fpm: String,

    /// Path to PHP entrypoint script
    #[arg(long)]
    pub php_entrypoint: String,

    /// FastCGI connection pool size
    #[arg(long, default_value = "16")]
    pub pool_size: usize,

    /// Log format
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

/// PHP-FPM connection address.
/// Contains `:` -> TCP, otherwise -> Unix socket path.
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
