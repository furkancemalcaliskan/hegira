use crate::health::ReadinessCheck;
use background_jobs::{DurableQueueStats, JobObserver};
use serde::Serialize;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Debug)]
struct LoopHealth {
    name: &'static str,
    last_heartbeat: Instant,
    stale_after: Duration,
}

#[derive(Debug, Default)]
pub struct WorkerHealth {
    loops: Mutex<Vec<LoopHealth>>,
}

impl WorkerHealth {
    pub fn activate(&self, name: &'static str, expected_interval: Duration, grace: Duration) {
        let mut loops = self.loops.lock().expect("worker health lock poisoned");
        let stale_after = expected_interval.saturating_add(grace);
        if let Some(worker_loop) = loops
            .iter_mut()
            .find(|worker_loop| worker_loop.name == name)
        {
            worker_loop.last_heartbeat = Instant::now();
            worker_loop.stale_after = stale_after;
        } else {
            loops.push(LoopHealth {
                name,
                last_heartbeat: Instant::now(),
                stale_after,
            });
        }
    }

    fn heartbeat(&self, name: &'static str) {
        let mut loops = self.loops.lock().expect("worker health lock poisoned");
        if let Some(worker_loop) = loops
            .iter_mut()
            .find(|worker_loop| worker_loop.name == name)
        {
            worker_loop.last_heartbeat = Instant::now();
        }
    }

    pub fn snapshot(&self) -> Vec<WorkerCheck> {
        let loops = self.loops.lock().expect("worker health lock poisoned");
        loops
            .iter()
            .map(|worker_loop| {
                let age = worker_loop.last_heartbeat.elapsed();
                WorkerCheck {
                    name: worker_loop.name,
                    status: if age <= worker_loop.stale_after {
                        "ok"
                    } else {
                        "stale"
                    },
                    heartbeat_age_ms: millis(age),
                    stale_after_ms: millis(worker_loop.stale_after),
                }
            })
            .collect()
    }

    pub fn readiness(&self) -> (ReadinessCheck, WorkerReadinessExtension) {
        let workers = self.snapshot();
        let workers_ok = !workers.is_empty() && workers.iter().all(WorkerCheck::is_ready);
        let check = if workers_ok {
            ReadinessCheck::available("worker_loops", Duration::ZERO)
        } else {
            ReadinessCheck::unavailable("worker_loops", Duration::ZERO)
        };

        (check, WorkerReadinessExtension { workers })
    }
}

#[derive(Debug, Serialize)]
pub struct WorkerReadinessExtension {
    pub workers: Vec<WorkerCheck>,
}

#[derive(Debug, Serialize)]
pub struct WorkerCheck {
    pub name: &'static str,
    pub status: &'static str,
    pub heartbeat_age_ms: u64,
    pub stale_after_ms: u64,
}

impl WorkerCheck {
    pub fn is_ready(&self) -> bool {
        self.status == "ok"
    }
}

pub struct RuntimeJobObserver {
    health: Arc<WorkerHealth>,
    delegate: Arc<dyn JobObserver>,
}

impl RuntimeJobObserver {
    pub fn new(health: Arc<WorkerHealth>, delegate: Arc<dyn JobObserver>) -> Self {
        Self { health, delegate }
    }
}

impl JobObserver for RuntimeJobObserver {
    fn wants_queue_stats(&self) -> bool {
        self.delegate.wants_queue_stats()
    }

    fn worker_heartbeat(&self, worker: &'static str) {
        self.health.heartbeat(worker);
        self.delegate.worker_heartbeat(worker);
    }

    fn worker_iteration(&self, worker: &'static str, outcome: &'static str) {
        self.delegate.worker_iteration(worker, outcome);
    }

    fn durable_claimed(&self, count: usize) {
        self.delegate.durable_claimed(count);
    }

    fn job_finished(
        &self,
        kind: &'static str,
        name: &str,
        outcome: &'static str,
        duration: Duration,
    ) {
        self.delegate.job_finished(kind, name, outcome, duration);
    }

    fn durable_queue_stats(&self, stats: DurableQueueStats) {
        self.delegate.durable_queue_stats(stats);
    }

    fn search_projection(&self, outcome: &'static str) {
        self.delegate.search_projection(outcome);
    }
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_worker_is_ready_and_job_heartbeats_are_recorded() {
        let health = Arc::new(WorkerHealth::default());
        health.activate("durable", Duration::from_secs(1), Duration::from_secs(1));
        let observer =
            RuntimeJobObserver::new(health.clone(), Arc::new(background_jobs::NoopJobObserver));
        observer.worker_heartbeat("durable");

        let workers = health.snapshot();
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].name, "durable");
        assert_eq!(workers[0].status, "ok");
        assert_eq!(workers[0].stale_after_ms, 2_000);
    }

    #[test]
    fn stale_or_missing_workers_make_worker_readiness_unavailable() {
        let health = WorkerHealth::default();
        let (missing_check, missing_extension) = health.readiness();
        assert_eq!(missing_check.status, "unavailable");
        assert!(missing_extension.workers.is_empty());

        health.activate("scheduler", Duration::ZERO, Duration::ZERO);
        std::thread::sleep(Duration::from_millis(1));

        let (stale_check, stale_extension) = health.readiness();
        assert_eq!(stale_check.status, "unavailable");
        assert_eq!(stale_extension.workers[0].status, "stale");
    }
}
