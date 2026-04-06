pub mod fastcgi;

use std::sync::Arc;

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

impl<T: PhpExecutor> PhpExecutor for Arc<T> {
    fn execute(
        &self,
        request: &PhpRequest<'_>,
    ) -> impl std::future::Future<Output = anyhow::Result<PhpCallResult>> + Send {
        (**self).execute(request)
    }

    fn discover(
        &self,
    ) -> impl std::future::Future<Output = anyhow::Result<PhpDiscoverResponse>> + Send {
        (**self).discover()
    }
}
