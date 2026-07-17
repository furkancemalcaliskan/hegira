use std::process::ExitCode;

mod telemetry;
#[cfg(feature = "metrics-prometheus")]
mod worker_metrics;
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

pub fn run() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("failed to initialize Tokio runtime: {err}");
            return ExitCode::FAILURE;
        }
    };

    match runtime.block_on(serve()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

async fn serve() -> Result<(), String> {
    let app_config = infrastructure::config::AppConfig::load()
        .map_err(|err| format!("failed to load application configuration: {err}"))?;
    app_config
        .validate_for_boot()
        .map_err(|err| format!("invalid application configuration: {err}"))?;
    app_config
        .validate_capabilities(compiled_capabilities())
        .map_err(|err| format!("invalid application capabilities: {err}"))?;

    let telemetry = telemetry::init(&app_config)?;
    let result = serve_configured(app_config).await;
    let shutdown_result = telemetry.shutdown();

    match (result, shutdown_result) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn compiled_capabilities() -> infrastructure::config::CompiledCapabilities {
    infrastructure::config::CompiledCapabilities {
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

    let db = infrastructure::db::connect_database(&app_config.database)
        .await
        .map_err(|err| {
            format!(
                "failed to initialize database at {}: {err}",
                app_config.database.safe_url()
            )
        })?;
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
        infrastructure::identity::seed::seed_identity(&seed_repository, &app_config.seed)
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
            shutdown_signal().await;
            Ok(())
        }
    }
}

fn start_workers(
    db: infrastructure::db::DatabasePool,
    app_config: &infrastructure::config::AppConfig,
    observer: std::sync::Arc<dyn infrastructure::jobs::JobObserver>,
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
        let mut registry = infrastructure::jobs::DurableJobRegistry::default();
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
) -> std::sync::Arc<dyn infrastructure::jobs::JobObserver> {
    #[cfg(feature = "metrics-prometheus")]
    if app_config.metrics.enabled {
        return std::sync::Arc::new(worker_metrics::PrometheusJobObserver::new());
    }

    let _ = app_config;
    std::sync::Arc::new(infrastructure::jobs::NoopJobObserver)
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
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|err| format!("worker operations server error: {err}"))
}

async fn serve_http(
    app_config: infrastructure::config::AppConfig,
    db: infrastructure::db::DatabasePool,
    search: infrastructure::search::SearchAdapter,
) -> Result<(), String> {
    use axum::{Router, middleware};
    use leptos::prelude::*;
    use leptos_axum::{LeptosRoutes, generate_route_list};
    use presentation::http::middleware as app_middleware;
    use tower_http::compression::CompressionLayer;
    use tower_http::{limit::RequestBodyLimitLayer, timeout::TimeoutLayer};
    use web::root::{App, shell};

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
    let rate_limiter = presentation::http::middleware::rate_limit::RateLimiter::from_config(
        &app_config.security.rate_limit,
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
    let cors_layer = cors_layer(&app_config.security.cors)?;

    let mut conf = get_configuration(Some("Cargo.toml"))
        .map_err(|err| format!("failed to load leptos configuration: {err}"))?;
    conf.leptos_options.site_addr = app_config.server.addr;
    let addr = conf.leptos_options.site_addr;
    let leptos_options = conf.leptos_options;
    let routes = generate_route_list(App);

    let app = Router::<LeptosOptions>::new()
        .merge(presentation::http::routes::routes(app_state).with_state(()))
        .leptos_routes_with_context(
            &leptos_options,
            routes,
            {
                let services = web_services.clone();
                let config = web_config.clone();
                move || {
                    provide_context(services.clone());
                    provide_context(config.clone());
                }
            },
            {
                let leptos_options = leptos_options.clone();
                move || shell(leptos_options.clone())
            },
        )
        .fallback(leptos_axum::file_and_error_handler(shell));

    #[cfg(feature = "metrics-prometheus")]
    let app = if app_config.metrics.enabled {
        tracing::info!(path = %app_config.metrics.path, "prometheus metrics enabled");
        app.layer(middleware::from_fn(app_middleware::metrics::record))
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
        .layer(middleware::from_fn(app_middleware::csrf::validate))
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
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|err| format!("server error: {err}"))?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received; draining connections");
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
