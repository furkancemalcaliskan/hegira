use serde::Serialize;
use std::{future::Future, time::Duration};
use tokio::time::{Instant, timeout};

#[derive(Debug, Serialize)]
pub struct LivenessResponse {
    pub status: &'static str,
    pub service: String,
    pub version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<&'static str>,
}

impl LivenessResponse {
    pub fn new(service: impl Into<String>, version: &'static str) -> Self {
        Self {
            status: "ok",
            service: service.into(),
            version,
            role: None,
        }
    }

    pub fn with_role(mut self, role: &'static str) -> Self {
        self.role = Some(role);
        self
    }
}

#[derive(Debug, Serialize)]
pub struct ReadinessResponse<T = ()>
where
    T: Serialize,
{
    pub status: &'static str,
    pub service: String,
    pub version: &'static str,
    pub checks: Vec<ReadinessCheck>,
    #[serde(flatten)]
    pub extension: T,
}

impl ReadinessResponse<()> {
    pub fn new(
        service: impl Into<String>,
        version: &'static str,
        checks: Vec<ReadinessCheck>,
    ) -> Self {
        Self {
            status: readiness_status(&checks),
            service: service.into(),
            version,
            checks,
            extension: (),
        }
    }
}

impl<T> ReadinessResponse<T>
where
    T: Serialize,
{
    pub fn with_extension<U>(self, extension: U) -> ReadinessResponse<U>
    where
        U: Serialize,
    {
        ReadinessResponse {
            status: self.status,
            service: self.service,
            version: self.version,
            checks: self.checks,
            extension,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.status == "ok"
    }
}

#[derive(Debug, Serialize)]
pub struct ReadinessCheck {
    pub name: &'static str,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
}

impl ReadinessCheck {
    pub fn available(name: &'static str, latency: Duration) -> Self {
        Self {
            name,
            status: "ok",
            latency_ms: Some(millis(latency)),
        }
    }

    pub fn unavailable(name: &'static str, latency: Duration) -> Self {
        Self {
            name,
            status: "unavailable",
            latency_ms: Some(millis(latency)),
        }
    }

    pub fn skipped(name: &'static str) -> Self {
        Self {
            name,
            status: "skipped",
            latency_ms: None,
        }
    }
}

pub async fn check<F>(
    name: &'static str,
    enabled: bool,
    probe_timeout: Duration,
    probe: F,
) -> ReadinessCheck
where
    F: Future<Output = Result<(), String>>,
{
    if !enabled {
        return ReadinessCheck::skipped(name);
    }

    let started_at = Instant::now();
    match timeout(probe_timeout, probe).await {
        Ok(Ok(())) => ReadinessCheck::available(name, started_at.elapsed()),
        Ok(Err(error)) => {
            tracing::warn!(dependency = name, error = %error, "readiness probe failed");
            ReadinessCheck::unavailable(name, started_at.elapsed())
        }
        Err(_) => {
            tracing::warn!(
                dependency = name,
                timeout_ms = probe_timeout.as_millis(),
                "readiness probe timed out"
            );
            ReadinessCheck::unavailable(name, started_at.elapsed())
        }
    }
}

fn readiness_status(checks: &[ReadinessCheck]) -> &'static str {
    if checks.iter().all(|check| check.status != "unavailable") {
        "ok"
    } else {
        "unavailable"
    }
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabled_probe_is_skipped_without_becoming_unready() {
        let check = check("cache", false, Duration::from_millis(1), async {
            Err("must not affect readiness".to_string())
        })
        .await;
        let response = ReadinessResponse::new("test", "1.0.0", vec![check]);

        assert!(response.is_ready());
        assert_eq!(response.checks[0].status, "skipped");
    }

    #[tokio::test]
    async fn failed_probe_makes_the_response_unready() {
        let check = check("database", true, Duration::from_millis(10), async {
            Err("offline".to_string())
        })
        .await;
        let response = ReadinessResponse::new("test", "1.0.0", vec![check]);

        assert!(!response.is_ready());
        assert_eq!(response.checks[0].status, "unavailable");
    }

    #[test]
    fn base_operational_responses_keep_the_existing_json_contract() {
        let liveness = LivenessResponse::new("test", "1.0.0");
        let readiness = ReadinessResponse::new(
            "test",
            "1.0.0",
            vec![ReadinessCheck::available("database", Duration::ZERO)],
        );

        assert_eq!(
            serde_json::to_value(liveness).unwrap(),
            serde_json::json!({
                "status": "ok",
                "service": "test",
                "version": "1.0.0"
            })
        );
        assert_eq!(
            serde_json::to_value(readiness).unwrap(),
            serde_json::json!({
                "status": "ok",
                "service": "test",
                "version": "1.0.0",
                "checks": [{
                    "name": "database",
                    "status": "ok",
                    "latency_ms": 0
                }]
            })
        );
    }
}
