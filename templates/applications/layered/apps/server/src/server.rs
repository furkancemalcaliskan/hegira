use app_infrastructure::config::ApplicationConfig;

pub async fn serve() -> Result<(), String> {
    let config = ApplicationConfig::load()
        .map_err(|error| format!("failed to load application configuration: {error}"))?;
    configuration::validate(&config, compiled_capabilities()).map_err(|error| error.to_string())?;

    let telemetry = observability::telemetry::init(&config.telemetry_settings())?;
    let result = serve_configured(config).await;
    let shutdown_result = telemetry.shutdown();

    match (result, shutdown_result) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

async fn serve_configured(config: ApplicationConfig) -> Result<(), String> {
    app_infrastructure::database::ensure_development_database(&config).await?;

    let migration_plan = config
        .database
        .auto_migrate
        .then(|| {
            persistence::migrations::MigrationPlan::new(std::iter::empty::<
                persistence::migrations::ModuleMigrationSource,
            >())
            .map_err(|error| format!("invalid application migration plan: {error}"))
        })
        .transpose()?;
    let database = persistence::connect_database(&config.database)
        .await
        .map_err(|error| {
            format!(
                "failed to initialize database at {}: {error}",
                config.database.safe_url()
            )
        })?;
    if let Some(migration_plan) = migration_plan {
        migration_plan
            .run(&database)
            .await
            .map_err(|error| format!("failed to run application migrations: {error}"))?;
    }

    let listener = tokio::net::TcpListener::bind(config.server.addr)
        .await
        .map_err(|error| {
            format!(
                "failed to bind application server to {}: {error}",
                config.server.addr
            )
        })?;
    let routes = app_presentation::http::routes(
        app_presentation::http::ApplicationState::new(
            config.application.name.clone(),
            database,
            config.health.readiness_timeout(),
        ),
        config.is_production(),
        config.server.body_limit_bytes,
        config.server.request_timeout(),
    );

    tracing::info!(
        address = %config.server.addr,
        environment = %config.environment,
        "application server listening"
    );
    axum::serve(listener, routes)
        .with_graceful_shutdown(runtime::shutdown_signal())
        .await
        .map_err(|error| format!("application server failed: {error}"))
}

fn compiled_capabilities() -> platform_core::CompiledCapabilities {
    platform_core::CompiledCapabilities {
        db_postgres: cfg!(feature = "db-postgres"),
        db_sqlite: cfg!(feature = "db-sqlite"),
        otel_otlp: cfg!(feature = "otel-otlp"),
        ..Default::default()
    }
}
