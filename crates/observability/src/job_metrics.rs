use background_jobs::{DurableQueueStats, JobObserver};
use prometheus::{
    Encoder, Gauge, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGaugeVec, Opts,
    TextEncoder, default_registry,
};
use std::{
    sync::LazyLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static WORKER_HEARTBEAT_SECONDS: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register(IntGaugeVec::new(
        Opts::new(
            "worker_last_heartbeat_unixtime_seconds",
            "Unix timestamp of the most recent worker loop heartbeat.",
        ),
        &["worker"],
    ))
});

static WORKER_ITERATIONS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register(IntCounterVec::new(
        Opts::new(
            "worker_iterations_total",
            "Total number of worker loop iterations by outcome.",
        ),
        &["worker", "outcome"],
    ))
});

static JOB_EXECUTIONS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register(IntCounterVec::new(
        Opts::new(
            "worker_job_executions_total",
            "Total number of background job executions by kind, job, and outcome.",
        ),
        &["kind", "job", "outcome"],
    ))
});

static JOB_DURATION_SECONDS: LazyLock<HistogramVec> = LazyLock::new(|| {
    register(HistogramVec::new(
        HistogramOpts::new(
            "worker_job_duration_seconds",
            "Background job execution duration in seconds.",
        )
        .buckets(vec![
            0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 15.0, 30.0,
        ]),
        &["kind", "job", "outcome"],
    ))
});

static DURABLE_CLAIMED_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
    register(IntCounter::new(
        "durable_jobs_claimed_total",
        "Total number of durable jobs claimed by this process.",
    ))
});

static DURABLE_QUEUE_MESSAGES: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register(IntGaugeVec::new(
        Opts::new(
            "durable_jobs_queue_messages",
            "Current durable queue message count by state.",
        ),
        &["state"],
    ))
});

static DURABLE_OLDEST_PENDING_SECONDS: LazyLock<Gauge> = LazyLock::new(|| {
    register(Gauge::new(
        "durable_jobs_oldest_pending_age_seconds",
        "Age of the oldest pending durable job in seconds.",
    ))
});

static SEARCH_PROJECTIONS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register(IntCounterVec::new(
        Opts::new(
            "search_projection_operations_total",
            "Total number of search projection operations by outcome.",
        ),
        &["outcome"],
    ))
});

#[derive(Debug, Default)]
pub struct PrometheusJobObserver;

impl PrometheusJobObserver {
    pub fn new() -> Self {
        LazyLock::force(&WORKER_HEARTBEAT_SECONDS);
        LazyLock::force(&WORKER_ITERATIONS_TOTAL);
        LazyLock::force(&JOB_EXECUTIONS_TOTAL);
        LazyLock::force(&JOB_DURATION_SECONDS);
        LazyLock::force(&DURABLE_CLAIMED_TOTAL);
        LazyLock::force(&DURABLE_QUEUE_MESSAGES);
        LazyLock::force(&DURABLE_OLDEST_PENDING_SECONDS);
        LazyLock::force(&SEARCH_PROJECTIONS_TOTAL);
        Self
    }
}

impl JobObserver for PrometheusJobObserver {
    fn wants_queue_stats(&self) -> bool {
        true
    }

    fn worker_heartbeat(&self, worker: &'static str) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .min(i64::MAX as u64) as i64;
        WORKER_HEARTBEAT_SECONDS
            .with_label_values(&[worker])
            .set(timestamp);
    }

    fn worker_iteration(&self, worker: &'static str, outcome: &'static str) {
        WORKER_ITERATIONS_TOTAL
            .with_label_values(&[worker, outcome])
            .inc();
    }

    fn durable_claimed(&self, count: usize) {
        DURABLE_CLAIMED_TOTAL.inc_by(count.min(u64::MAX as usize) as u64);
    }

    fn job_finished(
        &self,
        kind: &'static str,
        name: &str,
        outcome: &'static str,
        duration: Duration,
    ) {
        let labels = [kind, name, outcome];
        JOB_EXECUTIONS_TOTAL.with_label_values(&labels).inc();
        JOB_DURATION_SECONDS
            .with_label_values(&labels)
            .observe(duration.as_secs_f64());
    }

    fn durable_queue_stats(&self, stats: DurableQueueStats) {
        for (state, value) in [
            ("pending", stats.pending),
            ("retry", stats.retry),
            ("dead_letter", stats.dead_letter),
        ] {
            DURABLE_QUEUE_MESSAGES
                .with_label_values(&[state])
                .set(value);
        }
        DURABLE_OLDEST_PENDING_SECONDS.set(stats.oldest_pending_seconds);
    }

    fn search_projection(&self, outcome: &'static str) {
        SEARCH_PROJECTIONS_TOTAL.with_label_values(&[outcome]).inc();
    }
}

pub fn scrape() -> Result<String, String> {
    PrometheusJobObserver::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    TextEncoder::new()
        .encode(&metric_families, &mut buffer)
        .map_err(|err| format!("failed to encode metrics: {err}"))?;
    String::from_utf8(buffer).map_err(|err| format!("failed to encode metrics as utf-8: {err}"))
}

fn register<M>(metric: Result<M, prometheus::Error>) -> M
where
    M: prometheus::core::Collector + Clone + 'static,
{
    let metric = metric.expect("worker metric should be valid");
    default_registry()
        .register(Box::new(metric.clone()))
        .expect("worker metric should register once");
    metric
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observer_records_worker_and_queue_metrics() {
        let observer = PrometheusJobObserver::new();
        observer.worker_heartbeat("durable");
        observer.worker_iteration("durable", "ok");
        observer.job_finished("durable", "test.job", "completed", Duration::from_millis(5));
        observer.search_projection("applied");
        observer.durable_queue_stats(DurableQueueStats {
            pending: 3,
            retry: 1,
            dead_letter: 2,
            oldest_pending_seconds: 12.0,
        });

        let names = prometheus::gather()
            .into_iter()
            .map(|family| family.name().to_string())
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name == "worker_iterations_total"));
        assert!(
            names
                .iter()
                .any(|name| name == "durable_jobs_queue_messages")
        );
        assert!(
            names
                .iter()
                .any(|name| name == "search_projection_operations_total")
        );
    }
}
