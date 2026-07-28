#![cfg(feature = "ssr")]

use axum::{Router, http::StatusCode, middleware};
use hegira::{
    application_contracts::identity::users::{PagedUserResultDto, UserDto},
    infrastructure::{
        cache::CacheAdapter,
        config::{
            AppConfig, ApplicationConfig, AuditConfig, CacheBackend, CacheConfig, CorsConfig,
            DatabaseConfig, DurableJobsConfig, HealthConfig, JobsConfig, LoggingConfig,
            MailerBackend, MailerConfig, MetricsConfig, OAuthConfig, OAuthProviderConfig,
            OAuthProvidersConfig, OpenApiConfig, OtlpProtocol, RateLimitBackend, RateLimitConfig,
            RedisCacheConfig, RedisRateLimitConfig, RedisSessionConfig, RuntimeConfig, RuntimeRole,
            SchedulerConfig, SearchBackend, SearchConfig, SecurityConfig, SeedConfig,
            SessionBackend, SessionsConfig, SettingsConfig, SmtpMailerConfig, StartupConfig,
            StorageBackend, StorageConfig, TelemetryConfig, WorkerOperationsConfig,
        },
        identity::{
            SqlxIdentityRepository,
            seed::{seed_identity, seed_sqlite_identity},
        },
        search::SearchAdapter,
        settings::SettingsAdapter,
        storage::StorageAdapter,
    },
    presentation::http::state::AppState,
};
use serde_json::{Value, json};
use std::env;
use test_support::http::{
    request_empty, request_empty_with_authorization, request_json, response_json,
};

static DB_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn test_config() -> AppConfig {
    AppConfig {
        environment: "test".to_string(),
        application: ApplicationConfig {
            name: "Hegira".to_string(),
            public_url: "http://127.0.0.1:3000".to_string(),
        },
        server: hegira::infrastructure::config::ServerConfig {
            addr: "127.0.0.1:3000".parse().unwrap(),
            request_timeout_seconds: 30,
            body_limit_bytes: 2_097_152,
        },
        runtime: RuntimeConfig {
            role: RuntimeRole::All,
        },
        worker_operations: WorkerOperationsConfig {
            enabled: false,
            addr: "127.0.0.1:9091".parse().unwrap(),
            heartbeat_grace_seconds: 30,
        },
        startup: StartupConfig {
            ensure_database: false,
            seed_identity: false,
            scheduler: false,
            durable_jobs: false,
        },
        database: DatabaseConfig {
            backend: infrastructure::config::DatabaseBackend::Postgres,
            url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://localhost/hegira_test".to_string()),
            max_connections: 5,
            auto_migrate: true,
        },
        security: SecurityConfig {
            jwt_secret: "test-secret".to_string(),
            trusted_proxies: Vec::new(),
            cors: CorsConfig {
                enabled: true,
                allowed_origins: vec!["http://127.0.0.1:3000".to_string()],
                allow_credentials: false,
            },
            rate_limit: RateLimitConfig {
                enabled: true,
                backend: RateLimitBackend::Memory,
                max_requests: 1000,
                window_seconds: 60,
                redis: RedisRateLimitConfig {
                    url: "redis://127.0.0.1:6379".to_string(),
                },
            },
        },
        sessions: SessionsConfig {
            backend: SessionBackend::Database,
            sliding_ttl_seconds: 1800,
            max_lifetime_seconds: 3600,
            refresh_threshold_percent: 25,
            redis: RedisSessionConfig {
                url: "redis://127.0.0.1:6379".to_string(),
            },
        },
        oauth: OAuthConfig {
            enabled: false,
            state_ttl_seconds: 600,
            providers: OAuthProvidersConfig {
                google: test_oauth_provider("google"),
                github: test_oauth_provider("github"),
            },
        },
        mailer: MailerConfig {
            enabled: false,
            backend: MailerBackend::Null,
            from: "no-reply@example.com".to_string(),
            smtp: SmtpMailerConfig {
                host: "localhost".to_string(),
                port: 1025,
                username: None,
                password: None,
                starttls: false,
            },
        },
        cache: CacheConfig {
            enabled: false,
            backend: CacheBackend::Null,
            authorization_ttl_seconds: 60,
            redis: RedisCacheConfig {
                url: "redis://127.0.0.1:6379".to_string(),
            },
        },
        storage: StorageConfig {
            enabled: false,
            backend: StorageBackend::Null,
            local: hegira::infrastructure::config::LocalStorageConfig {
                root: "storage/test".to_string(),
            },
            s3: hegira::infrastructure::config::S3StorageConfig {
                bucket: "hegira-test".to_string(),
                region: "us-east-1".to_string(),
                endpoint_url: None,
                force_path_style: true,
            },
        },
        search: SearchConfig {
            enabled: false,
            backend: SearchBackend::Null,
            index_prefix: "test".to_string(),
            task_timeout_milliseconds: 10_000,
            meilisearch: hegira::infrastructure::config::MeilisearchConfig {
                url: "http://127.0.0.1:7700".to_string(),
                api_key: None,
            },
        },
        scheduler: SchedulerConfig {
            enabled: false,
            run_on_startup: false,
            cleanup_expired_sessions_interval_seconds: 300,
        },
        jobs: JobsConfig {
            durable: DurableJobsConfig {
                enabled: false,
                poll_interval_milliseconds: 10,
                batch_size: 20,
                lock_timeout_seconds: 30,
            },
        },
        audit: AuditConfig { enabled: false },
        settings: SettingsConfig {
            enabled: false,
            cache_ttl_seconds: 60,
        },
        openapi: OpenApiConfig { enabled: true },
        metrics: MetricsConfig {
            enabled: false,
            path: "/metrics".to_string(),
        },
        telemetry: TelemetryConfig {
            enabled: false,
            protocol: OtlpProtocol::Grpc,
            endpoint: "http://127.0.0.1:4317".to_string(),
            timeout_milliseconds: 5_000,
            sample_ratio: 1.0,
        },
        health: HealthConfig {
            readiness_timeout_milliseconds: 100,
        },
        seed: SeedConfig {
            seed_admin: true,
            admin_username: "admin@example.com".to_string(),
            admin_password: "admin12345".to_string(),
        },
        logging: LoggingConfig {
            filter: "warn".to_string(),
        },
    }
}

fn test_oauth_provider(provider: &str) -> OAuthProviderConfig {
    OAuthProviderConfig {
        enabled: false,
        client_id: String::new(),
        client_secret: String::new(),
        redirect_uri: format!("http://127.0.0.1:3000/oauth/{provider}/callback"),
        authorization_url: format!("https://{provider}.example.com/authorize"),
        token_url: format!("https://{provider}.example.com/token"),
        userinfo_url: format!("https://{provider}.example.com/userinfo"),
        scopes: vec!["email".to_string()],
    }
}

async fn setup() -> Router {
    let config = test_config();

    let pool = hegira::infrastructure::testing::reset_database(&config.database.url)
        .await
        .expect("failed to reset api test database");
    let repository = SqlxIdentityRepository::new(pool.clone());
    seed_identity(&repository, &config.seed)
        .await
        .expect("failed to seed api test identity data");
    let state = AppState::new(
        config,
        infrastructure::db::DatabasePool::Postgres(pool),
        CacheAdapter::Null(Default::default()),
        StorageAdapter::Null(Default::default()),
        SearchAdapter::Null(Default::default()),
        SettingsAdapter::Null(Default::default()),
    );

    hegira::presentation::http::routes::routes(state)
        .layer(middleware::from_fn(hegira::http_support::request_id::set))
}

#[cfg(feature = "db-sqlite")]
async fn setup_sqlite() -> Router {
    let mut config = test_config();
    config.database.backend = infrastructure::config::DatabaseBackend::Sqlite;
    config.database.url = "sqlite::memory:".to_string();
    config.database.max_connections = 1;
    let pool =
        hegira::infrastructure::db::connect_sqlite_with_application_migrations(&config.database)
            .await
            .unwrap();
    seed_sqlite_identity(pool.clone(), &config.seed)
        .await
        .unwrap();
    let state = AppState::new(
        config,
        infrastructure::db::DatabasePool::Sqlite(pool),
        CacheAdapter::Null(Default::default()),
        StorageAdapter::Null(Default::default()),
        SearchAdapter::Null(Default::default()),
        SettingsAdapter::Null(Default::default()),
    );
    hegira::presentation::http::routes::routes(state)
        .layer(middleware::from_fn(hegira::http_support::request_id::set))
}

async fn setup_without_database() -> Router {
    setup_without_database_with_config(test_config()).await
}

async fn setup_without_database_with_config(config: AppConfig) -> Router {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy(&config.database.url)
        .expect("failed to create lazy test database pool");
    pool.close().await;
    let state = AppState::new(
        config,
        infrastructure::db::DatabasePool::Postgres(pool),
        CacheAdapter::Null(Default::default()),
        StorageAdapter::Null(Default::default()),
        SearchAdapter::Null(Default::default()),
        SettingsAdapter::Null(Default::default()),
    );

    hegira::presentation::http::routes::routes(state)
        .layer(middleware::from_fn(hegira::http_support::request_id::set))
}

async fn login_admin(app: Router) -> String {
    let response = request_json(
        app,
        "POST",
        "/api/identity/auth/login",
        json!({
            "username": "admin@example.com",
            "password": "admin12345"
        }),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = response_json(response).await;
    body["token"]
        .as_str()
        .expect("login response must contain token")
        .to_string()
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn admin_can_manage_identity_users_on_sqlite() {
    let app = setup_sqlite().await;
    let token = login_admin(app.clone()).await;

    let response = request_json(
        app.clone(),
        "POST",
        "/api/identity/users",
        json!({
            "username": "operator@example.com",
            "password": "operator12345",
            "is_verified": false,
            "roles": []
        }),
        Some(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: UserDto = response_json(response).await;
    assert_eq!(created.username, "operator@example.com");
    assert!(created.roles.is_empty());
    assert!(!created.is_verified);

    let response = request_empty(
        app.clone(),
        "GET",
        "/api/identity/users?page=1&page_size=10&search=operator",
        Some(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let page: PagedUserResultDto = response_json(response).await;
    assert_eq!(page.total_count, 1);
    assert_eq!(page.items[0].pid, created.pid);

    let response = request_json(
        app.clone(),
        "PUT",
        "/api/identity/users/operator@example.com",
        json!({
            "password": null,
            "is_verified": true,
            "roles": []
        }),
        Some(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let updated: UserDto = response_json(response).await;
    assert!(updated.is_verified);
    assert!(updated.roles.is_empty());

    let response = request_empty(
        app.clone(),
        "DELETE",
        "/api/identity/users/operator@example.com",
        Some(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = request_empty(
        app,
        "GET",
        "/api/identity/users/operator@example.com",
        Some(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "requires a reachable PostgreSQL DATABASE_URL and resets the test database"]
async fn auth_login_me_and_logout_flow() {
    let _guard = DB_TEST_LOCK.lock().await;
    let app = setup().await;
    let token = login_admin(app.clone()).await;

    let response = request_empty(app.clone(), "GET", "/api/identity/auth/me", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response_json(response).await;
    assert_eq!(body["username"], "admin@example.com");

    let response = request_empty(
        app.clone(),
        "POST",
        "/api/identity/auth/logout",
        Some(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = request_empty(app, "GET", "/api/identity/auth/me", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn protected_users_api_requires_bearer_token() {
    let app = setup_without_database().await;

    let response = request_empty(app, "GET", "/api/identity/users", None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().contains_key("x-request-id"));

    let body: Value = response_json(response).await;
    assert_eq!(body["code"], "auth:missing_bearer_token");
    assert_eq!(body["message"], "missing bearer token");
}

#[tokio::test]
async fn protected_users_api_rejects_invalid_authorization_scheme() {
    let app = setup_without_database().await;

    let response =
        request_empty_with_authorization(app, "GET", "/api/identity/users", Some("Token abc"))
            .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let body: Value = response_json(response).await;
    assert_eq!(body["code"], "auth:invalid_bearer_token");
    assert_eq!(body["message"], "invalid bearer token");
}

#[tokio::test]
async fn register_validation_errors_use_standard_error_body() {
    let app = setup_without_database().await;

    let response = request_json(
        app,
        "POST",
        "/api/identity/auth/register",
        json!({
            "username": "",
            "password": ""
        }),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body: Value = response_json(response).await;
    assert_eq!(body["code"], "identity:username_required");
    assert_eq!(body["message"], "Username is required");
}

#[tokio::test]
async fn health_endpoint_returns_operational_metadata() {
    let app = setup_without_database().await;

    let response = request_empty(app, "GET", "/healthz", None).await;
    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = response_json(response).await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "Hegira");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn readiness_reports_failed_database_and_skips_disabled_dependencies() {
    let app = setup_without_database().await;

    let response = request_empty(app, "GET", "/readyz", None).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let body: Value = response_json(response).await;
    assert_eq!(body["status"], "unavailable");
    assert_eq!(body["service"], "Hegira");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["checks"][0]["name"], "database");
    assert_eq!(body["checks"][0]["status"], "unavailable");
    assert_eq!(body["checks"][1]["name"], "cache");
    assert_eq!(body["checks"][1]["status"], "skipped");
    assert_eq!(body["checks"][2]["name"], "storage");
    assert_eq!(body["checks"][2]["status"], "skipped");
    assert_eq!(body["checks"][3]["name"], "search");
    assert_eq!(body["checks"][3]["status"], "skipped");
    assert!(body["checks"][0].get("error").is_none());
}

#[tokio::test]
#[cfg(feature = "openapi")]
async fn openapi_is_available_outside_production() {
    let app = setup_without_database().await;

    let response = request_empty(app, "GET", "/api-docs/openapi.json", None).await;
    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = response_json(response).await;
    assert_eq!(body["openapi"], "3.1.0");
    assert!(body["paths"]["/api/identity/auth/login"].is_object());
    assert!(body["components"]["securitySchemes"]["bearer_auth"].is_object());
}

#[tokio::test]
async fn openapi_is_not_available_in_production() {
    let mut config = test_config();
    config.environment = "production".to_string();
    config.openapi.enabled = true;
    let app = setup_without_database_with_config(config).await;

    let response = request_empty(app, "GET", "/api-docs/openapi.json", None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "requires a reachable PostgreSQL DATABASE_URL and resets the test database"]
async fn admin_can_manage_identity_users() {
    let _guard = DB_TEST_LOCK.lock().await;
    let app = setup().await;
    let token = login_admin(app.clone()).await;

    let response = request_json(
        app.clone(),
        "POST",
        "/api/identity/users",
        json!({
            "username": "user1@example.com",
            "password": "secret123",
            "is_verified": true
        }),
        Some(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: UserDto = response_json(response).await;
    assert!(created.id > 0);
    assert_ne!(created.pid, uuid::Uuid::nil());
    assert_eq!(created.username, "user1@example.com");
    assert!(created.is_verified);

    let response = request_empty(
        app.clone(),
        "GET",
        "/api/identity/users?page=1&page_size=10&search=user1",
        Some(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let list: PagedUserResultDto = response_json(response).await;
    assert_eq!(list.total_count, 1);
    assert_eq!(list.items[0].username, "user1@example.com");

    let response = request_json(
        app.clone(),
        "PUT",
        "/api/identity/users/user1@example.com",
        json!({
            "password": "newsecret123",
            "is_verified": false
        }),
        Some(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let updated: UserDto = response_json(response).await;
    assert_eq!(updated.username, "user1@example.com");
    assert!(!updated.is_verified);

    let response = request_empty(
        app.clone(),
        "DELETE",
        "/api/identity/users/user1@example.com",
        Some(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = request_empty(
        app,
        "GET",
        "/api/identity/users/user1@example.com",
        Some(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "requires a reachable PostgreSQL DATABASE_URL and resets the test database"]
async fn admin_user_cannot_be_deleted() {
    let _guard = DB_TEST_LOCK.lock().await;
    let app = setup().await;
    let token = login_admin(app.clone()).await;

    let response = request_empty(
        app.clone(),
        "DELETE",
        "/api/identity/users/admin@example.com",
        Some(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let body: Value = response_json(response).await;
    assert_eq!(body["code"], "identity:protected_admin_cannot_be_deleted");
    assert_eq!(body["message"], "Admin user cannot be deleted");

    let response = request_empty(
        app,
        "GET",
        "/api/identity/users/admin@example.com",
        Some(&token),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
}
