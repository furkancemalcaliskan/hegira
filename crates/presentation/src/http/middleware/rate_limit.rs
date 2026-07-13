use crate::http::error_response::ErrorBody;
use axum::{
    Json,
    body::Body,
    extract::State,
    http::{HeaderValue, Method, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use infrastructure::config::{RateLimitBackend, RateLimitConfig};
#[cfg(feature = "cache-redis")]
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const MAX_REQUESTS: usize = 20;
const WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct RateLimiter {
    enabled: bool,
    max_requests: usize,
    window: Duration,
    backend: RateLimiterBackend,
}

#[derive(Debug, Clone)]
enum RateLimiterBackend {
    Memory(MemoryRateLimiter),
    #[cfg(feature = "cache-redis")]
    Redis(RedisSlidingWindowRateLimiter),
}

#[derive(Debug, Clone, Default)]
struct MemoryRateLimiter {
    clients: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
}

#[cfg(feature = "cache-redis")]
#[derive(Debug, Clone)]
struct RedisSlidingWindowRateLimiter {
    client: redis::Client,
}

#[derive(Debug, Clone, Copy)]
struct RateLimitDecision {
    allowed: bool,
    limit: usize,
    remaining: usize,
    reset_seconds: u64,
    retry_after_seconds: Option<u64>,
}

impl RateLimiter {
    pub fn from_config(config: &RateLimitConfig) -> Result<Self, String> {
        let backend = match config.backend {
            RateLimitBackend::Memory => RateLimiterBackend::Memory(MemoryRateLimiter::default()),
            RateLimitBackend::Redis => build_redis(&config.redis.url)?,
        };

        Ok(Self {
            enabled: config.enabled,
            max_requests: config.max_requests.max(1),
            window: Duration::from_secs(config.window_seconds.max(1)),
            backend,
        })
    }

    fn memory(max_requests: usize, window: Duration) -> Self {
        Self {
            enabled: true,
            max_requests,
            window,
            backend: RateLimiterBackend::Memory(MemoryRateLimiter::default()),
        }
    }

    async fn check(&self, key: &str) -> Result<RateLimitDecision, String> {
        match &self.backend {
            RateLimiterBackend::Memory(limiter) => {
                Ok(limiter.check(key, self.max_requests, self.window))
            }
            #[cfg(feature = "cache-redis")]
            RateLimiterBackend::Redis(limiter) => {
                limiter.check(key, self.max_requests, self.window).await
            }
        }
    }
}

impl MemoryRateLimiter {
    fn check(&self, key: &str, max_requests: usize, window: Duration) -> RateLimitDecision {
        let mut clients = self.clients.lock().unwrap_or_else(|err| err.into_inner());
        let now = Instant::now();
        clients.retain(|_, timestamps| {
            timestamps.retain(|timestamp| now.duration_since(*timestamp) < window);
            !timestamps.is_empty()
        });

        let timestamps = clients.entry(key.to_string()).or_default();
        timestamps.retain(|timestamp| now.duration_since(*timestamp) < window);

        if timestamps.len() >= max_requests {
            let retry_after_seconds = timestamps
                .first()
                .map(|oldest| {
                    window
                        .saturating_sub(now.duration_since(*oldest))
                        .as_secs()
                        .max(1)
                })
                .unwrap_or_else(|| window.as_secs().max(1));

            return RateLimitDecision {
                allowed: false,
                limit: max_requests,
                remaining: 0,
                reset_seconds: retry_after_seconds,
                retry_after_seconds: Some(retry_after_seconds),
            };
        }

        timestamps.push(now);
        RateLimitDecision {
            allowed: true,
            limit: max_requests,
            remaining: max_requests.saturating_sub(timestamps.len()),
            reset_seconds: window.as_secs().max(1),
            retry_after_seconds: None,
        }
    }
}

#[cfg(feature = "cache-redis")]
impl RedisSlidingWindowRateLimiter {
    fn new(url: &str) -> redis::RedisResult<Self> {
        redis::Client::open(url).map(|client| Self { client })
    }

    async fn check(
        &self,
        key: &str,
        max_requests: usize,
        window: Duration,
    ) -> Result<RateLimitDecision, String> {
        let mut connection = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|err| err.to_string())?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| err.to_string())?
            .as_millis() as i64;
        let window_ms = window.as_millis() as i64;
        let member = uuid::Uuid::new_v4().to_string();
        let values: Vec<i64> = redis::cmd("EVAL")
            .arg(SLIDING_WINDOW_LUA)
            .arg(1)
            .arg(key)
            .arg(now)
            .arg(window_ms)
            .arg(max_requests as i64)
            .arg(member)
            .query_async(&mut connection)
            .await
            .map_err(|err| err.to_string())?;

        let allowed = values.first().copied().unwrap_or_default() == 1;
        let count = values.get(1).copied().unwrap_or_default().max(0) as usize;
        let retry_after_seconds = values.get(2).copied().unwrap_or_default().max(0) as u64;

        Ok(RateLimitDecision {
            allowed,
            limit: max_requests,
            remaining: max_requests.saturating_sub(count),
            reset_seconds: if retry_after_seconds == 0 {
                window.as_secs().max(1)
            } else {
                retry_after_seconds
            },
            retry_after_seconds: (!allowed).then_some(retry_after_seconds.max(1)),
        })
    }
}

#[cfg(feature = "cache-redis")]
const SLIDING_WINDOW_LUA: &str = r#"
local key = KEYS[1]
local now = tonumber(ARGV[1])
local window = tonumber(ARGV[2])
local max_requests = tonumber(ARGV[3])
local member = ARGV[4]

redis.call('ZREMRANGEBYSCORE', key, 0, now - window)
local count = redis.call('ZCARD', key)

if count >= max_requests then
    local oldest = redis.call('ZRANGE', key, 0, 0, 'WITHSCORES')
    local retry_after = math.ceil((tonumber(oldest[2]) + window - now) / 1000)
    return {0, count, retry_after}
end

redis.call('ZADD', key, now, member)
redis.call('EXPIRE', key, math.ceil(window / 1000) + 1)
return {1, count + 1, 0}
"#;

#[cfg(feature = "cache-redis")]
fn build_redis(url: &str) -> Result<RateLimiterBackend, String> {
    RedisSlidingWindowRateLimiter::new(url)
        .map(RateLimiterBackend::Redis)
        .map_err(|err| format!("failed to initialize Redis rate limiter: {err}"))
}

#[cfg(not(feature = "cache-redis"))]
fn build_redis(_url: &str) -> Result<RateLimiterBackend, String> {
    Err(
        "security.rate_limit.backend=redis requires building with --features cache-redis"
            .to_string(),
    )
}

fn rate_limit_key(req: &Request<Body>) -> String {
    format!(
        "rate_limit:{}:{}:{}",
        req.method(),
        req.uri().path(),
        extract_ip(req)
    )
}

fn extract_ip(req: &Request<Body>) -> IpAddr {
    req.headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .and_then(|value| value.trim().parse::<IpAddr>().ok())
        .unwrap_or(IpAddr::from([127, 0, 0, 1]))
}

pub async fn check(req: Request<Body>, next: Next) -> Response {
    check_with_limiter(State(RateLimiter::memory(MAX_REQUESTS, WINDOW)), req, next).await
}

pub async fn check_configured(
    State(limiter): State<RateLimiter>,
    req: Request<Body>,
    next: Next,
) -> Response {
    check_with_limiter(State(limiter), req, next).await
}

async fn check_with_limiter(
    State(limiter): State<RateLimiter>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if !limiter.enabled || req.method() != Method::POST {
        return next.run(req).await;
    }

    let decision = match limiter.check(&rate_limit_key(&req)).await {
        Ok(decision) => decision,
        Err(error) => {
            tracing::error!(%error, "rate limiter failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    code: "system:rate_limiter_error",
                    message: "internal server error".to_string(),
                }),
            )
                .into_response();
        }
    };

    if decision.allowed {
        let mut response = next.run(req).await;
        write_rate_limit_headers(response.headers_mut(), decision);
        return response;
    }

    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(ErrorBody {
            code: "rate_limit:exceeded",
            message: "too many requests".to_string(),
        }),
    )
        .into_response();
    write_rate_limit_headers(response.headers_mut(), decision);
    response
}

fn write_rate_limit_headers(headers: &mut header::HeaderMap, decision: RateLimitDecision) {
    insert_header(headers, "x-ratelimit-limit", decision.limit.to_string());
    insert_header(
        headers,
        "x-ratelimit-remaining",
        decision.remaining.to_string(),
    );
    insert_header(
        headers,
        "x-ratelimit-reset",
        decision.reset_seconds.to_string(),
    );

    if let Some(retry_after) = decision.retry_after_seconds {
        insert_header(headers, "retry-after", retry_after.to_string());
    }
}

fn insert_header(headers: &mut header::HeaderMap, name: &'static str, value: String) {
    if let Ok(value) = HeaderValue::from_str(&value) {
        headers.insert(name, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, middleware, routing::get, routing::post};
    use tower::ServiceExt;

    fn app() -> Router {
        Router::new()
            .route("/test", post(|| async { "ok" }))
            .route("/get", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(
                RateLimiter::memory(MAX_REQUESTS, WINDOW),
                check_configured,
            ))
    }

    #[tokio::test]
    async fn get_requests_bypass_rate_limit() {
        let resp = app()
            .oneshot(Request::get("/get").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn post_requests_are_rate_limited() {
        let ip = "10.99.99.99";
        let app = app();
        for i in 0..MAX_REQUESTS {
            let resp = app
                .clone()
                .oneshot(
                    Request::post("/test")
                        .header("x-forwarded-for", ip)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "request {i} should succeed");
        }

        let resp = app
            .oneshot(
                Request::post("/test")
                    .header("x-forwarded-for", ip)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            resp.headers().get("x-ratelimit-remaining").unwrap(),
            HeaderValue::from_static("0")
        );
    }

    #[test]
    fn extract_ip_from_forwarded_header() {
        let req = Request::post("/test")
            .header("x-forwarded-for", "192.168.1.1, 10.0.0.1")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_ip(&req), IpAddr::from([192, 168, 1, 1]));
    }

    #[test]
    fn extract_ip_default_without_header() {
        let req = Request::post("/test").body(Body::empty()).unwrap();
        assert_eq!(extract_ip(&req), IpAddr::from([127, 0, 0, 1]));
    }

    #[test]
    fn memory_limiter_enforces_limit() {
        let limiter = MemoryRateLimiter::default();
        let key = "test-key";
        for _ in 0..MAX_REQUESTS {
            assert!(limiter.check(key, MAX_REQUESTS, WINDOW).allowed);
        }
        assert!(!limiter.check(key, MAX_REQUESTS, WINDOW).allowed);
    }
}
