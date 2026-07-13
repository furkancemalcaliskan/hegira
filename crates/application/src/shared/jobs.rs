use std::{future::Future, pin::Pin};
use uuid::Uuid;

pub trait Job<Args>: Send + Sync + Clone + 'static {
    fn name(&self) -> &'static str;
    fn perform(&self, args: Args) -> impl Future<Output = Result<(), String>> + Send;
}

pub trait JobDispatcher: Send + Sync {
    fn dispatch<J, Args>(&self, job: J, args: Args)
    where
        J: Job<Args>,
        Args: Send + 'static;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableJobOptions {
    pub idempotency_key: Option<String>,
    pub max_attempts: u32,
}

impl Default for DurableJobOptions {
    fn default() -> Self {
        Self {
            idempotency_key: None,
            max_attempts: 5,
        }
    }
}

pub trait DurableJobQueue: Send + Sync {
    fn enqueue(
        &self,
        name: &str,
        payload: serde_json::Value,
        options: DurableJobOptions,
    ) -> impl Future<Output = Result<Uuid, String>> + Send;
}

pub type DurableJobFuture<'a> = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

pub trait DurableJobHandler: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn handle(&self, payload: serde_json::Value) -> DurableJobFuture<'_>;
}
