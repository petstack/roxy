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
