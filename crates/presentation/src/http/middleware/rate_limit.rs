use crate::http::error_response::ErrorBody;
use axum::{
    Json,
    body::Body,
    extract::{ConnectInfo, State},
    http::{HeaderValue, Method, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use infrastructure::config::{RateLimitBackend, RateLimitConfig};
use ipnet::IpNet;
#[cfg(feature = "cache-redis")]
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const MAX_FORWARDED_HOPS: usize = 32;

#[cfg(test)]
const MAX_REQUESTS: usize = 20;
#[cfg(test)]
const WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct RateLimiter {
    enabled: bool,
    max_requests: usize,
    window: Duration,
    backend: RateLimiterBackend,
    client_ip_resolver: ClientIpResolver,
}

#[derive(Debug, Clone, Default)]
struct ClientIpResolver {
    trusted_proxies: Vec<IpNet>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClientIpError {
    MissingPeerAddress,
    InvalidForwardedFor,
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
    pub fn from_config(
        config: &RateLimitConfig,
        trusted_proxies: &[String],
    ) -> Result<Self, String> {
        let client_ip_resolver = ClientIpResolver::from_config(trusted_proxies)?;

        if !config.enabled {
            return Ok(Self {
                enabled: false,
                max_requests: config.max_requests.max(1),
                window: Duration::from_secs(config.window_seconds.max(1)),
                backend: RateLimiterBackend::Memory(MemoryRateLimiter::default()),
                client_ip_resolver,
            });
        }

        let backend = match config.backend {
            RateLimitBackend::Memory => RateLimiterBackend::Memory(MemoryRateLimiter::default()),
            RateLimitBackend::Redis => build_redis(&config.redis.url)?,
        };

        Ok(Self {
            enabled: config.enabled,
            max_requests: config.max_requests.max(1),
            window: Duration::from_secs(config.window_seconds.max(1)),
            backend,
            client_ip_resolver,
        })
    }

    #[cfg(test)]
    fn memory(max_requests: usize, window: Duration) -> Self {
        Self {
            enabled: true,
            max_requests,
            window,
            backend: RateLimiterBackend::Memory(MemoryRateLimiter::default()),
            client_ip_resolver: ClientIpResolver::default(),
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

impl ClientIpResolver {
    fn from_config(networks: &[String]) -> Result<Self, String> {
        let trusted_proxies = networks
            .iter()
            .map(|network| {
                let parsed = network.parse::<IpNet>().map_err(|_| {
                    format!(
                        "security.trusted_proxies entry `{network}` must be a valid IPv4 or IPv6 CIDR"
                    )
                })?;
                if parsed.prefix_len() == 0 {
                    return Err(format!(
                        "security.trusted_proxies entry `{network}` must not trust every address"
                    ));
                }
                Ok(parsed)
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { trusted_proxies })
    }

    fn resolve(&self, req: &Request<Body>) -> Result<IpAddr, ClientIpError> {
        let peer = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|connect_info| normalize_ip(connect_info.0.ip()))
            .ok_or(ClientIpError::MissingPeerAddress)?;

        if !self.is_trusted(peer) {
            return Ok(peer);
        }

        let forwarded = parse_x_forwarded_for(req)?;
        let mut resolved = peer;
        for candidate in forwarded.into_iter().rev() {
            if !self.is_trusted(resolved) {
                break;
            }
            resolved = candidate;
        }

        Ok(resolved)
    }

    fn is_trusted(&self, address: IpAddr) -> bool {
        self.trusted_proxies
            .iter()
            .any(|network| network.contains(&address))
    }
}

fn parse_x_forwarded_for(req: &Request<Body>) -> Result<Vec<IpAddr>, ClientIpError> {
    let mut addresses = Vec::new();
    for value in req.headers().get_all("x-forwarded-for") {
        let value = value
            .to_str()
            .map_err(|_| ClientIpError::InvalidForwardedFor)?;
        for item in value.split(',') {
            if addresses.len() >= MAX_FORWARDED_HOPS {
                return Err(ClientIpError::InvalidForwardedFor);
            }
            let address = item
                .trim()
                .parse::<IpAddr>()
                .map(normalize_ip)
                .map_err(|_| ClientIpError::InvalidForwardedFor)?;
            addresses.push(address);
        }
    }

    Ok(addresses)
}

fn normalize_ip(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V6(address) => address
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(address)),
        address => address,
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

fn rate_limit_key(req: &Request<Body>, client_ip: IpAddr) -> String {
    format!(
        "rate_limit:{}:{}:{}",
        req.method(),
        req.uri().path(),
        client_ip
    )
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

    let client_ip = match limiter.client_ip_resolver.resolve(&req) {
        Ok(client_ip) => client_ip,
        Err(ClientIpError::InvalidForwardedFor) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    code: "request:invalid_forwarded_for",
                    message: "invalid forwarded client address".to_string(),
                }),
            )
                .into_response();
        }
        Err(ClientIpError::MissingPeerAddress) => {
            tracing::error!("rate limiter request is missing its TCP peer address");
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

    let decision = match limiter.check(&rate_limit_key(&req, client_ip)).await {
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
    use infrastructure::config::RedisRateLimitConfig;
    use tower::ServiceExt;

    fn app_with_limiter(limiter: RateLimiter) -> Router {
        Router::new()
            .route("/test", post(|| async { "ok" }))
            .route("/get", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(limiter, check_configured))
    }

    fn app() -> Router {
        app_with_limiter(RateLimiter::memory(MAX_REQUESTS, WINDOW))
    }

    fn request(
        method: Method,
        path: &str,
        peer: IpAddr,
        forwarded_for: Option<&str>,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .extension(ConnectInfo(SocketAddr::new(peer, 54321)));
        if let Some(forwarded_for) = forwarded_for {
            builder = builder.header("x-forwarded-for", forwarded_for);
        }
        builder.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn get_requests_bypass_rate_limit() {
        let resp = app()
            .oneshot(request(
                Method::GET,
                "/get",
                IpAddr::from([203, 0, 113, 10]),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn post_requests_are_rate_limited() {
        let peer = IpAddr::from([203, 0, 113, 10]);
        let app = app();
        for i in 0..MAX_REQUESTS {
            let resp = app
                .clone()
                .oneshot(request(Method::POST, "/test", peer, None))
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "request {i} should succeed");
        }

        let resp = app
            .oneshot(request(Method::POST, "/test", peer, None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            resp.headers().get("x-ratelimit-remaining").unwrap(),
            HeaderValue::from_static("0")
        );
    }

    #[test]
    fn direct_clients_cannot_spoof_forwarded_addresses() {
        let resolver = ClientIpResolver::default();
        let peer = IpAddr::from([203, 0, 113, 10]);
        let req = request(Method::POST, "/test", peer, Some("198.51.100.99"));

        assert_eq!(resolver.resolve(&req), Ok(peer));
    }

    #[test]
    fn malformed_forwarding_header_from_an_untrusted_peer_is_ignored() {
        let resolver = ClientIpResolver::default();
        let peer = IpAddr::from([203, 0, 113, 10]);
        let req = request(Method::POST, "/test", peer, Some("not-an-ip"));

        assert_eq!(resolver.resolve(&req), Ok(peer));
    }

    #[test]
    fn trusted_proxy_chain_stops_at_the_nearest_untrusted_address() {
        let resolver = ClientIpResolver::from_config(&[
            "10.0.0.0/8".to_string(),
            "192.168.0.0/16".to_string(),
        ])
        .unwrap();
        let req = request(
            Method::POST,
            "/test",
            IpAddr::from([10, 0, 0, 2]),
            Some("203.0.113.99, 198.51.100.7, 192.168.1.10"),
        );

        assert_eq!(resolver.resolve(&req), Ok(IpAddr::from([198, 51, 100, 7])));
    }

    #[test]
    fn trusted_proxy_resolution_supports_ipv6() {
        let resolver = ClientIpResolver::from_config(&["2001:db8:ffff::/48".to_string()]).unwrap();
        let peer = "2001:db8:ffff::10".parse().unwrap();
        let client = "2001:db8:1::42".parse().unwrap();
        let req = request(Method::POST, "/test", peer, Some("2001:db8:1::42"));

        assert_eq!(resolver.resolve(&req), Ok(client));
    }

    #[test]
    fn malformed_forwarding_chain_from_a_trusted_proxy_fails_closed() {
        let resolver = ClientIpResolver::from_config(&["10.0.0.0/8".to_string()]).unwrap();
        let req = request(
            Method::POST,
            "/test",
            IpAddr::from([10, 0, 0, 2]),
            Some("not-an-ip"),
        );

        assert_eq!(
            resolver.resolve(&req),
            Err(ClientIpError::InvalidForwardedFor)
        );
    }

    #[test]
    fn universal_trusted_proxy_networks_are_rejected() {
        assert!(ClientIpResolver::from_config(&["0.0.0.0/0".to_string()]).is_err());
        assert!(ClientIpResolver::from_config(&["::/0".to_string()]).is_err());
    }

    #[test]
    fn missing_peer_address_fails_closed() {
        let resolver = ClientIpResolver::default();
        let req = Request::post("/test").body(Body::empty()).unwrap();

        assert_eq!(
            resolver.resolve(&req),
            Err(ClientIpError::MissingPeerAddress)
        );
    }

    #[tokio::test]
    async fn malformed_trusted_forwarding_chain_is_rejected_by_middleware() {
        let limiter = RateLimiter {
            client_ip_resolver: ClientIpResolver::from_config(&["10.0.0.0/8".to_string()]).unwrap(),
            ..RateLimiter::memory(1, WINDOW)
        };
        let response = app_with_limiter(limiter)
            .oneshot(request(
                Method::POST,
                "/test",
                IpAddr::from([10, 0, 0, 2]),
                Some("not-an-ip"),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn spoofed_headers_do_not_create_new_memory_quotas() {
        let peer = IpAddr::from([203, 0, 113, 10]);
        let app = app_with_limiter(RateLimiter::memory(1, WINDOW));

        let first = app
            .clone()
            .oneshot(request(Method::POST, "/test", peer, Some("198.51.100.1")))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);

        let second = app
            .oneshot(request(Method::POST, "/test", peer, Some("198.51.100.2")))
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn trusted_proxy_clients_receive_independent_memory_quotas() {
        let limiter = RateLimiter {
            client_ip_resolver: ClientIpResolver::from_config(&["10.0.0.0/8".to_string()]).unwrap(),
            ..RateLimiter::memory(1, WINDOW)
        };
        let app = app_with_limiter(limiter);
        let peer = IpAddr::from([10, 0, 0, 2]);

        for client in ["198.51.100.1", "198.51.100.2"] {
            let response = app
                .clone()
                .oneshot(request(Method::POST, "/test", peer, Some(client)))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{client}");
        }
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

    #[test]
    fn disabled_limiter_does_not_initialize_selected_redis_backend() {
        let limiter = RateLimiter::from_config(
            &RateLimitConfig {
                enabled: false,
                backend: RateLimitBackend::Redis,
                max_requests: 20,
                window_seconds: 60,
                redis: RedisRateLimitConfig {
                    url: "not a redis URL".to_string(),
                },
            },
            &[],
        )
        .expect("a disabled limiter should not initialize its selected backend");

        assert!(!limiter.enabled);
        assert!(matches!(limiter.backend, RateLimiterBackend::Memory(_)));
    }
}
