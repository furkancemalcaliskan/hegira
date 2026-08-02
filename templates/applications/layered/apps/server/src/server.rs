#[path = "worker_operations.rs"]
mod worker_operations;

#[derive(Debug, Default)]
struct ActiveWorkers {
    scheduler: bool,
    durable_jobs: bool,
}

impl ActiveWorkers {
    fn any(&self) -> bool {
        self.scheduler || self.durable_jobs
    }
}

pub fn run() -> std::process::ExitCode {
    runtime::run(serve)
}

async fn serve() -> Result<(), String> {
    let app_config = infrastructure::config::AppConfig::load()
        .map_err(|err| format!("failed to load application configuration: {err}"))?;
    configuration::validate(&app_config, compiled_capabilities()).map_err(|err| err.to_string())?;

    let telemetry = observability::telemetry::init(&telemetry_settings(&app_config))?;
    let result = serve_configured(app_config).await;
    let shutdown_result = telemetry.shutdown();

    match (result, shutdown_result) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn telemetry_settings(
    config: &infrastructure::config::AppConfig,
) -> observability::telemetry::TelemetrySettings {
    let exporter = config.telemetry.enabled.then(|| {
        let protocol = match config.telemetry.protocol {
            infrastructure::config::OtlpProtocol::Grpc => {
                observability::telemetry::OtlpProtocol::Grpc
            }
            infrastructure::config::OtlpProtocol::HttpProtobuf => {
                observability::telemetry::OtlpProtocol::HttpProtobuf
            }
        };
        observability::telemetry::OtlpExporterSettings {
            protocol,
            endpoint: config.telemetry.endpoint.clone(),
            timeout: std::time::Duration::from_millis(config.telemetry.timeout_milliseconds),
            sample_ratio: config.telemetry.sample_ratio,
        }
    });

    observability::telemetry::TelemetrySettings {
        service_name: config.application.name.clone(),
        service_version: env!("CARGO_PKG_VERSION"),
        environment: config.environment.clone(),
        role: format!("{:?}", config.runtime.role).to_lowercase(),
        logging_filter: config.logging.filter.clone(),
        exporter,
    }
}

fn compiled_capabilities() -> platform_core::CompiledCapabilities {
    platform_core::CompiledCapabilities {
        db_postgres: cfg!(feature = "db-postgres"),
        db_sqlite: cfg!(feature = "db-sqlite"),
        cache_redis: cfg!(feature = "cache-redis"),
        mailer_smtp: cfg!(feature = "mailer-smtp"),
        storage_s3: cfg!(feature = "storage-s3"),
        search_meilisearch: cfg!(feature = "search-meilisearch"),
        metrics_prometheus: cfg!(feature = "metrics-prometheus"),
        otel_otlp: cfg!(feature = "otel-otlp"),
        openapi: cfg!(feature = "openapi"),
    }
}

async fn serve_configured(app_config: infrastructure::config::AppConfig) -> Result<(), String> {
    tracing::info!(
        environment = %app_config.environment,
        role = ?app_config.runtime.role,
        "runtime starting"
    );

    #[cfg(feature = "db-postgres")]
    {
        if app_config.startup.ensure_database
            && !app_config.is_production()
            && app_config.database.backend == infrastructure::config::DatabaseBackend::Postgres
        {
            tracing::info!("ensuring development database exists");
            infrastructure::db::ensure_database(&app_config.database)
                .await
                .map_err(|err| {
                    format!(
                        "failed to ensure development database at {}: {err}",
                        app_config.database.safe_url()
                    )
                })?;
        }
    }

    let migration_plan = app_config
        .database
        .auto_migrate
        .then(|| {
            let migration_sources =
                infrastructure::db::application_migration_sources(&app_config.database.backend)
                    .map_err(|error| format!("failed to select application migrations: {error}"))?;
            persistence::migrations::MigrationPlan::new(migration_sources)
                .map_err(|error| format!("invalid application migration plan: {error}"))
        })
        .transpose()?;

    let db = persistence::connect_database(&app_config.database)
        .await
        .map_err(|err| {
            format!(
                "failed to initialize database at {}: {err}",
                app_config.database.safe_url()
            )
        })?;
    if let Some(migration_plan) = migration_plan {
        migration_plan
            .run(&db)
            .await
            .map_err(|error| format!("failed to run application migrations: {error}"))?;
    }
    infrastructure::identity::sessions::SessionRepositoryAdapter::from_database(
        &app_config,
        db.clone(),
    )
    .map_err(|err| format!("invalid session store configuration: {err}"))?;
    let search = infrastructure::search::SearchAdapter::from_config(&app_config)
        .map_err(|err| format!("invalid search configuration: {err}"))?;
    if app_config.search.enabled {
        search
            .health_check()
            .await
            .map_err(|err| format!("search startup probe failed: {err}"))?;
        search
            .initialize_indexes()
            .await
            .map_err(|err| format!("search index initialization failed: {err}"))?;
    }
    let search = std::sync::Arc::new(search);

    if app_config.startup.seed_identity {
        tracing::info!("running identity seed at startup");
        let seed_repository = infrastructure::identity::IdentityRepositoryAdapter::new(db.clone());
        infrastructure::identity::seed::seed_identity(
            &seed_repository,
            &infrastructure::security::password_hasher::Argon2PasswordHasher,
            &app_config.seed,
        )
        .await
        .map_err(|err| format!("failed to seed identity data: {err}"))?;
    }

    let worker_health = std::sync::Arc::new(worker_operations::WorkerHealth::default());
    let job_observer = std::sync::Arc::new(worker_operations::RuntimeJobObserver::new(
        worker_health.clone(),
        metrics_job_observer(&app_config),
    ));
    let active_workers = if app_config.runtime.role.runs_workers() {
        start_workers(
            db.clone(),
            &app_config,
            job_observer,
            worker_health.clone(),
            search.clone(),
        )?
    } else {
        tracing::info!("worker loops skipped for web-only runtime role");
        ActiveWorkers::default()
    };

    if app_config.runtime.role.runs_web() {
        tracing::info!(
            scheduler = active_workers.scheduler,
            durable_jobs = active_workers.durable_jobs,
            "runtime web role active"
        );
        serve_http(app_config, db, (*search).clone()).await
    } else {
        if active_workers.any() {
            tracing::info!(
                scheduler = active_workers.scheduler,
                durable_jobs = active_workers.durable_jobs,
                "runtime worker role active"
            );
        } else {
            tracing::warn!("runtime worker role has no active worker loops");
        }
        if app_config.worker_operations.enabled {
            serve_worker_operations(app_config, db, worker_health).await
        } else {
            runtime::shutdown_signal().await;
            Ok(())
        }
    }
}

fn start_workers(
    db: infrastructure::db::DatabasePool,
    app_config: &infrastructure::config::AppConfig,
    observer: std::sync::Arc<dyn background_jobs::JobObserver>,
    health: std::sync::Arc<worker_operations::WorkerHealth>,
    search: std::sync::Arc<infrastructure::search::SearchAdapter>,
) -> Result<ActiveWorkers, String> {
    let mut active = ActiveWorkers::default();
    let heartbeat_grace =
        std::time::Duration::from_secs(app_config.worker_operations.heartbeat_grace_seconds);

    if app_config.startup.scheduler {
        if app_config.scheduler.enabled {
            health.activate(
                "scheduler",
                std::time::Duration::from_secs(
                    app_config
                        .scheduler
                        .cleanup_expired_sessions_interval_seconds,
                ),
                heartbeat_grace,
            );
        }
        match &db {
            #[cfg(feature = "db-postgres")]
            infrastructure::db::DatabasePool::Postgres(pool) => {
                infrastructure::jobs::cleanup::start_recurring_jobs_with_observer(
                    pool.clone(),
                    &app_config.scheduler,
                    observer.clone(),
                )
            }
            #[cfg(feature = "db-sqlite")]
            infrastructure::db::DatabasePool::Sqlite(pool) => {
                infrastructure::jobs::cleanup::start_sqlite_recurring_jobs_with_observer(
                    pool.clone(),
                    &app_config.scheduler,
                    observer.clone(),
                )
            }
        }
        active.scheduler = app_config.scheduler.enabled;
    } else {
        tracing::info!("scheduler startup disabled");
    }
    if app_config.startup.durable_jobs && app_config.jobs.durable.enabled {
        health.activate(
            "durable",
            std::time::Duration::from_millis(app_config.jobs.durable.poll_interval_milliseconds),
            heartbeat_grace,
        );
        let mut registry = background_jobs::DurableJobRegistry::default();
        if app_config.mailer.enabled {
            registry.register(infrastructure::mail::jobs::SendMailJobHandler::new(
                infrastructure::mail::MailerAdapter::from_config(app_config)?,
            ))?;
        }
        match &db {
            #[cfg(feature = "db-postgres")]
            infrastructure::db::DatabasePool::Postgres(pool) => {
                if app_config.search.enabled {
                    registry.register(
                        infrastructure::search::jobs::SearchIndexJobHandler::new(
                            search,
                            pool.clone(),
                        )
                        .with_observer(observer.clone()),
                    )?;
                }
                infrastructure::jobs::durable::DurableJobWorker::new(
                    pool.clone(),
                    registry,
                    app_config.jobs.durable.clone(),
                )
                .with_observer(observer)
                .start();
            }
            #[cfg(feature = "db-sqlite")]
            infrastructure::db::DatabasePool::Sqlite(pool) => {
                if app_config.search.enabled {
                    registry.register(
                        infrastructure::search::projection_sqlite::SqliteSearchIndexJobHandler::new(
                            search,
                            pool.clone(),
                        ),
                    )?;
                }
                infrastructure::jobs::durable_sqlite::SqliteDurableJobWorker::new(
                    pool.clone(),
                    registry,
                    app_config.jobs.durable.clone(),
                )
                .with_observer(observer)
                .start();
            }
        }
        active.durable_jobs = true;
    } else if app_config.startup.durable_jobs {
        tracing::info!("durable job worker startup requested but jobs.durable.enabled=false");
    } else {
        tracing::info!("durable job worker startup disabled");
    }

    Ok(active)
}

fn metrics_job_observer(
    app_config: &infrastructure::config::AppConfig,
) -> std::sync::Arc<dyn background_jobs::JobObserver> {
    #[cfg(feature = "metrics-prometheus")]
    if app_config.metrics.enabled {
        return std::sync::Arc::new(observability::job_metrics::PrometheusJobObserver::new());
    }

    let _ = app_config;
    std::sync::Arc::new(background_jobs::NoopJobObserver)
}

async fn serve_worker_operations(
    app_config: infrastructure::config::AppConfig,
    db: infrastructure::db::DatabasePool,
    health: std::sync::Arc<worker_operations::WorkerHealth>,
) -> Result<(), String> {
    let addr = app_config.worker_operations.addr;
    let state = worker_operations::OperationsState::new(app_config, db, health);
    let app = worker_operations::routes(state);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|err| format!("failed to bind worker operations address {addr}: {err}"))?;

    tracing::info!(%addr, "worker operations server listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(runtime::shutdown_signal())
        .await
        .map_err(|err| format!("worker operations server error: {err}"))
}

async fn serve_http(
    app_config: infrastructure::config::AppConfig,
    db: infrastructure::db::DatabasePool,
    search: infrastructure::search::SearchAdapter,
) -> Result<(), String> {
    use app_web::root::{App, shell};
    use axum::{Router, middleware};
    use http_support as app_middleware;
    use leptos::prelude::*;
    use leptos_axum::{LeptosRoutes, generate_route_list};
    use tower_http::compression::CompressionLayer;
    use tower_http::{limit::RequestBodyLimitLayer, timeout::TimeoutLayer};

    infrastructure::mail::MailerAdapter::from_config(&app_config)
        .map_err(|err| format!("invalid mailer configuration: {err}"))?;
    infrastructure::cache::validate_config(&app_config)
        .await
        .map_err(|err| format!("invalid cache configuration: {err}"))?;
    let cache = infrastructure::cache::CacheAdapter::from_config(&app_config)
        .map_err(|err| format!("invalid cache configuration: {err}"))?;
    let storage = infrastructure::storage::StorageAdapter::from_config(&app_config)
        .await
        .map_err(|err| format!("invalid storage configuration: {err}"))?;
    let rate_limit_backend = match app_config.security.rate_limit.backend {
        infrastructure::config::RateLimitBackend::Memory => {
            app_middleware::rate_limit::RateLimitBackend::Memory
        }
        infrastructure::config::RateLimitBackend::Redis => {
            app_middleware::rate_limit::RateLimitBackend::Redis {
                url: app_config.security.rate_limit.redis.url.clone(),
            }
        }
    };
    let rate_limiter = app_middleware::rate_limit::RateLimiter::from_options(
        &app_middleware::rate_limit::RateLimitOptions {
            enabled: app_config.security.rate_limit.enabled,
            max_requests: app_config.security.rate_limit.max_requests,
            window: std::time::Duration::from_secs(app_config.security.rate_limit.window_seconds),
            backend: rate_limit_backend,
        },
        &app_config.security.trusted_proxies,
    )
    .map_err(|err| format!("invalid rate limiter configuration: {err}"))?;

    let settings = infrastructure::settings::SettingsAdapter::from_database(
        &app_config,
        db.clone(),
        std::sync::Arc::new(cache.clone()),
    );
    let app_state = presentation::http::state::AppState::new(
        app_config.clone(),
        db,
        cache,
        storage,
        search,
        settings,
    );
    let web_services = app_state.services.clone();
    let web_config = app_state.config.clone();
    let identity_cookie_settings = identity_http::cookie::IdentityCookieSettings {
        secure: app_config.is_production(),
        max_lifetime_seconds: app_config.sessions.max_lifetime_seconds as i64,
    };
    let cors_layer = cors_layer(&app_config.security.cors)?;
    let identity_transport_policies =
        identity_http::policy::IdentityTransportPolicies::from_public_url(
            &app_config.application.public_url,
        )
        .map_err(|err| format!("invalid CSRF configuration: {err}"))?;

    let mut conf = get_configuration(None)
        .map_err(|err| format!("failed to load leptos configuration: {err}"))?;
    conf.leptos_options.site_addr = app_config.server.addr;
    let addr = conf.leptos_options.site_addr;
    let leptos_options = conf.leptos_options;
    let routes = generate_route_list(App);

    let operational_routes =
        presentation::http::routes::operational_routes(app_state.clone()).with_state(());
    let bearer_api_routes = identity_api_routes(app_state);
    let cookie_bff_routes = Router::<LeptosOptions>::new()
        .leptos_routes_with_context(
            &leptos_options,
            routes,
            {
                let services = web_services.clone();
                let config = web_config.clone();
                move || {
                    provide_context(services.clone());
                    provide_context(config.clone());
                    provide_context(identity_cookie_settings);
                }
            },
            {
                let leptos_options = leptos_options.clone();
                move || shell(leptos_options.clone())
            },
        )
        .fallback(leptos_axum::file_and_error_handler(shell));

    let app = compose_transport_routes(
        operational_routes,
        bearer_api_routes,
        cookie_bff_routes,
        identity_transport_policies.cookie_bff,
        identity_transport_policies.bearer_api,
    );

    #[cfg(feature = "metrics-prometheus")]
    let app = if app_config.metrics.enabled {
        tracing::info!(path = %app_config.metrics.path, "prometheus metrics enabled");
        app.layer(middleware::from_fn(observability::metrics::record))
    } else {
        app
    };

    let app = app
        .layer(middleware::from_fn(app_middleware::trace::log))
        .layer(middleware::from_fn(app_middleware::request_id::set))
        .layer(middleware::from_fn_with_state(
            app_config.is_production(),
            app_middleware::security_headers::set,
        ))
        .layer(middleware::from_fn_with_state(
            rate_limiter,
            app_middleware::rate_limit::check_configured,
        ))
        .layer(cors_layer)
        .layer(CompressionLayer::new())
        .layer(RequestBodyLimitLayer::new(
            app_config.server.body_limit_bytes,
        ))
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            std::time::Duration::from_secs(app_config.server.request_timeout_seconds),
        ))
        .with_state(leptos_options);

    tracing::info!("http server listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|err| format!("failed to bind to address {addr}: {err}"))?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(runtime::shutdown_signal())
    .await
    .map_err(|err| format!("server error: {err}"))?;

    Ok(())
}

/// Builds the Identity Bearer API selected by this application host.
///
/// Kept public so integration tests exercise the same explicit composition
/// used by the production server rather than reconstructing module routes.
#[doc(hidden)]
pub fn identity_api_routes<S>(state: presentation::http::state::AppState) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let router = identity_http::bearer_api_routes(identity_http::state::IdentityHttpState::new(
        state.services.auth.clone(),
        state.services.oauth.clone(),
        state.services.users.clone(),
        state.services.permissions.clone(),
    ))
    .with_state(());

    #[cfg(feature = "openapi")]
    let router = if state.config.openapi.enabled && !state.config.is_production() {
        router.merge(identity_http::openapi::routes())
    } else {
        router
    };

    router
}

fn compose_transport_routes<S>(
    operational_routes: axum::Router<S>,
    bearer_api_routes: axum::Router<S>,
    cookie_bff_routes: axum::Router<S>,
    cookie_policy: http_support::policy::CookieBffPolicy,
    _bearer_policy: http_support::policy::BearerApiPolicy,
) -> axum::Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    use axum::middleware;

    axum::Router::new()
        .merge(operational_routes)
        .merge(bearer_api_routes)
        .merge(cookie_bff_routes.layer(middleware::from_fn_with_state(
            cookie_policy.csrf(),
            http_support::csrf::validate,
        )))
}

fn cors_layer(
    config: &infrastructure::config::CorsConfig,
) -> Result<tower_http::cors::CorsLayer, String> {
    use axum::http::{HeaderValue, Method, header};
    use std::time::Duration;
    use tower_http::cors::CorsLayer;

    if !config.enabled {
        return Ok(CorsLayer::new());
    }

    let origins = config
        .allowed_origins
        .iter()
        .map(|origin| {
            origin
                .parse::<HeaderValue>()
                .map_err(|err| format!("invalid CORS allowed origin `{origin}`: {err}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .allow_credentials(config.allow_credentials)
        .max_age(Duration::from_secs(600)))
}

#[cfg(test)]
mod transport_policy_tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode, header},
        middleware,
        routing::{delete, get, patch, post, put},
    };
    use http_support as app_middleware;
    use tower::ServiceExt;

    fn app() -> Router {
        let operational = Router::new().route("/healthz", get(|| async { "ok" }));
        let bearer_api = Router::new()
            .route("/api/external-post", post(|| async { "ok" }))
            .route("/api/external-put", put(|| async { "ok" }))
            .route("/api/external-patch", patch(|| async { "ok" }))
            .route("/api/external-delete", delete(|| async { "ok" }));
        let cookie_bff = Router::new()
            .route("/api/browser-post", post(|| async { "ok" }))
            .route("/api/browser-put", put(|| async { "ok" }))
            .route("/api/browser-patch", patch(|| async { "ok" }))
            .route("/api/browser-delete", delete(|| async { "ok" }));
        let cookie_policy =
            app_middleware::policy::CookieBffPolicy::from_public_url("https://application.example")
                .unwrap();

        compose_transport_routes(
            operational,
            bearer_api,
            cookie_bff,
            cookie_policy,
            app_middleware::policy::BearerApiPolicy,
        )
        .layer(middleware::from_fn(app_middleware::request_id::set))
        .layer(middleware::from_fn_with_state(
            false,
            app_middleware::security_headers::set,
        ))
    }

    #[tokio::test]
    async fn bearer_api_mutation_does_not_require_browser_origin_headers() {
        for (method, uri) in [
            (axum::http::Method::POST, "/api/external-post"),
            (axum::http::Method::PUT, "/api/external-put"),
            (axum::http::Method::PATCH, "/api/external-patch"),
            (axum::http::Method::DELETE, "/api/external-delete"),
        ] {
            let response = app()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK, "{uri}");
        }
    }

    #[tokio::test]
    async fn cookie_bff_mutation_remains_csrf_protected() {
        for (method, uri) in [
            (axum::http::Method::POST, "/api/browser-post"),
            (axum::http::Method::PUT, "/api/browser-put"),
            (axum::http::Method::PATCH, "/api/browser-patch"),
            (axum::http::Method::DELETE, "/api/browser-delete"),
        ] {
            let missing_origin = app()
                .oneshot(
                    Request::builder()
                        .method(method.clone())
                        .uri(uri)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(missing_origin.status(), StatusCode::FORBIDDEN, "{uri}");

            let same_origin = app()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header(header::ORIGIN, "https://application.example")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(same_origin.status(), StatusCode::OK, "{uri}");
        }
    }

    #[tokio::test]
    async fn common_middleware_wraps_bearer_and_cookie_transports() {
        for uri in ["/api/external-post", "/api/browser-post"] {
            let response = app()
                .oneshot(
                    Request::post(uri)
                        .header(header::ORIGIN, "https://application.example")
                        .header(
                            app_middleware::request_id::REQUEST_ID_HEADER.clone(),
                            "test-id",
                        )
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response
                    .headers()
                    .get(&app_middleware::request_id::REQUEST_ID_HEADER)
                    .unwrap(),
                "test-id"
            );
            assert_eq!(
                response.headers().get(header::X_FRAME_OPTIONS).unwrap(),
                "DENY"
            );
        }
    }
}
