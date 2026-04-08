pub mod fastcgi;
pub mod http;

use std::sync::Arc;

use crate::protocol::{UpstreamCallResult, UpstreamDiscoverResponse, UpstreamEnvelope};

pub trait UpstreamExecutor: Send + Sync {
    fn execute(
        &self,
        request: &UpstreamEnvelope<'_>,
    ) -> impl std::future::Future<Output = anyhow::Result<UpstreamCallResult>> + Send;

    fn discover(
        &self,
    ) -> impl std::future::Future<Output = anyhow::Result<UpstreamDiscoverResponse>> + Send;
}

impl<T: UpstreamExecutor> UpstreamExecutor for Arc<T> {
    fn execute(
        &self,
        request: &UpstreamEnvelope<'_>,
    ) -> impl std::future::Future<Output = anyhow::Result<UpstreamCallResult>> + Send {
        (**self).execute(request)
    }

    fn discover(
        &self,
    ) -> impl std::future::Future<Output = anyhow::Result<UpstreamDiscoverResponse>> + Send {
        (**self).discover()
    }
}
