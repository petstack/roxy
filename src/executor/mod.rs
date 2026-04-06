pub mod fastcgi;

use crate::protocol::{PhpCallResult, PhpDiscoverResponse, PhpRequest};

pub trait PhpExecutor: Send + Sync {
    fn execute(
        &self,
        request: &PhpRequest<'_>,
    ) -> impl std::future::Future<Output = anyhow::Result<PhpCallResult>> + Send;

    fn discover(
        &self,
    ) -> impl std::future::Future<Output = anyhow::Result<PhpDiscoverResponse>> + Send;
}
