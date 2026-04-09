//! Internal modules of the `roxy` MCP proxy.
//!
//! `roxy` is primarily a binary, but the crate is exposed as a library so
//! integration tests, benchmarks and downstream tooling can import the same
//! hot-path helpers the server uses.

pub mod config;
pub mod executor;
pub mod protocol;
pub mod server;

/// Internal re-exports used by `benches/`. Not part of the stable public API;
/// the contents can change or disappear between versions.
#[doc(hidden)]
pub mod __bench {
    pub use crate::executor::fastcgi::body_start_offset;
    pub use crate::server::fresh_request_id;
}
