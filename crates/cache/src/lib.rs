use std::{future::Future, time::Duration};

pub trait Cache: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn get_string(
        &self,
        key: &str,
    ) -> impl Future<Output = Result<Option<String>, Self::Error>> + Send;

    fn set_string(
        &self,
        key: &str,
        value: String,
        ttl: Option<Duration>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn remove(&self, key: &str) -> impl Future<Output = Result<(), Self::Error>> + Send;
}
