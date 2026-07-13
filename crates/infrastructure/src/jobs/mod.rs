pub mod cleanup;
#[cfg(feature = "db-postgres")]
pub mod durable;
#[cfg(feature = "db-sqlite")]
pub mod durable_sqlite;
pub mod in_process;
pub mod recurring;

use application::shared::jobs::DurableJobHandler;
use std::{collections::HashMap, sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct ClaimedMessage {
    pub id: Uuid,
    pub name: String,
    pub payload: serde_json::Value,
    pub attempts: i32,
    pub max_attempts: i32,
}

#[derive(Default)]
pub struct DurableJobRegistry {
    handlers: HashMap<&'static str, Arc<dyn DurableJobHandler>>,
}

impl DurableJobRegistry {
    pub fn register<H: DurableJobHandler>(&mut self, handler: H) -> Result<(), String> {
        let name = handler.name();
        if self.handlers.insert(name, Arc::new(handler)).is_some() {
            return Err(format!(
                "durable job handler `{name}` is already registered"
            ));
        }
        Ok(())
    }
    pub(crate) fn get(&self, name: &str) -> Option<Arc<dyn DurableJobHandler>> {
        self.handlers.get(name).cloned()
    }
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DurableQueueStats {
    pub pending: i64,
    pub retry: i64,
    pub dead_letter: i64,
    pub oldest_pending_seconds: f64,
}

pub trait JobObserver: Send + Sync + 'static {
    fn wants_queue_stats(&self) -> bool {
        false
    }

    fn worker_heartbeat(&self, _worker: &'static str) {}

    fn worker_iteration(&self, _worker: &'static str, _outcome: &'static str) {}

    fn durable_claimed(&self, _count: usize) {}

    fn job_finished(
        &self,
        _kind: &'static str,
        _name: &str,
        _outcome: &'static str,
        _duration: Duration,
    ) {
    }

    fn durable_queue_stats(&self, _stats: DurableQueueStats) {}

    fn search_projection(&self, _outcome: &'static str) {}
}

#[derive(Debug, Default)]
pub struct NoopJobObserver;

impl JobObserver for NoopJobObserver {}
