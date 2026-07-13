use crate::shared::errors::ApplicationResult;
use std::{future::Future, time::Duration};

pub trait Cache: Send + Sync {
    fn get_string(
        &self,
        key: &str,
    ) -> impl Future<Output = ApplicationResult<Option<String>>> + Send;

    fn set_string(
        &self,
        key: &str,
        value: String,
        ttl: Option<Duration>,
    ) -> impl Future<Output = ApplicationResult<()>> + Send;

    fn remove(&self, key: &str) -> impl Future<Output = ApplicationResult<()>> + Send;
}
