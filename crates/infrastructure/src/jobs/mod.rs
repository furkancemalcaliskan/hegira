pub mod cleanup;
#[cfg(feature = "db-postgres")]
pub mod durable;
#[cfg(feature = "db-sqlite")]
pub mod durable_sqlite;

use uuid::Uuid;

pub use background_jobs::{DurableJobRegistry, DurableQueueStats, JobObserver, NoopJobObserver};

pub mod in_process {
    pub use background_jobs::InProcessQueue;
}

pub mod recurring {
    pub use background_jobs::{spawn_recurring, spawn_recurring_with_observer};
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaimedMessage {
    pub id: Uuid,
    pub name: String,
    pub payload: serde_json::Value,
    pub attempts: i32,
    pub max_attempts: i32,
}
