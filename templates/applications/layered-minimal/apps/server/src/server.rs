#[derive(Clone)]
struct ServerState {
    name: String,
    database: persistence::DatabasePool,
}

pub fn run() -> std::process::ExitCode {
    runtime::run(serve)
}

async fn serve() -> Result<(), String> {
    use app_web::root::{App, shell};
    use axum::{Router, routing::get};
    use leptos::prelude::*;
    use leptos_axum::{LeptosRoutes, generate_route_list};
    use tower_http::{
        compression::CompressionLayer, limit::RequestBodyLimitLayer, timeout::TimeoutLayer,
    };

    let config = app_infrastructure::config::AppConfig::load()
        .map_err(|error| format!("failed to load application configuration: {error}"))?;
    configuration::validate(
        &config,
        app_infrastructure::config::CompiledCapabilities {
            db_postgres: cfg!(feature = "db-postgres"),
            db_sqlite: cfg!(feature = "db-sqlite"),
        },
    )
    .map_err(|error| error.to_string())?;

    // hegira:module-preflight
    // hegira:module-preflight:end
    let database = app_infrastructure::operations::initialize_database(&config).await?;
    // hegira:module-initialization
    // hegira:module-initialization:end
    let state = ServerState {
        name: config.application.name.clone(),
        database,
    };

    let mut leptos_config = get_configuration(None)
        .map_err(|error| format!("failed to load Leptos configuration: {error}"))?;
    leptos_config.leptos_options.site_addr = config.server.addr;
    let addr = leptos_config.leptos_options.site_addr;
    let leptos_options = leptos_config.leptos_options;
    let routes = generate_route_list(App);

    let application_routes = Router::new().nest(
        "/api/application",
        app_presentation::http::routes(
            app_presentation::http::ApplicationState::new(config.application.name.clone()),
            config.is_production(),
            config.server.body_limit_bytes,
            std::time::Duration::from_secs(config.server.request_timeout_seconds),
        ),
    );
    let operational_routes = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .layer(axum::Extension(state.clone()))
        .merge(application_routes)
        .with_state(());
    let web_routes = Router::<LeptosOptions>::new()
        .leptos_routes_with_context(
            &leptos_options,
            routes,
            move || {
                // hegira:resource-leptos-contexts
                // hegira:resource-leptos-contexts:end
            },
            {
                let options = leptos_options.clone();
                move || shell(options.clone())
            },
        )
        .fallback(leptos_axum::file_and_error_handler(shell));
    let web_routes = web_routes
        // hegira:cookie-bff-policy
        // hegira:cookie-bff-policy:end
        ;

    let app = operational_routes
        .merge(web_routes)
        // hegira:resource-bearer-routes
        // hegira:resource-bearer-routes:end
        .layer(axum::middleware::from_fn(http_support::request_id::set))
        .layer(axum::middleware::from_fn_with_state(
            config.is_production(),
            http_support::security_headers::set,
        ))
        // hegira:module-rate-limit
        // hegira:module-rate-limit:end
        .layer(CompressionLayer::new())
        .layer(RequestBodyLimitLayer::new(config.server.body_limit_bytes))
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            std::time::Duration::from_secs(config.server.request_timeout_seconds),
        ))
        .with_state(leptos_options);

    tracing::info!(%addr, "minimal application server listening");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|error| format!("failed to bind application server: {error}"))?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(runtime::shutdown_signal())
    .await
    .map_err(|error| format!("application server error: {error}"))
}

async fn healthz(axum::Extension(state): axum::Extension<ServerState>) -> String {
    state.name
}

async fn readyz(axum::Extension(state): axum::Extension<ServerState>) -> axum::http::StatusCode {
    match state.database.health_check().await {
        Ok(()) => axum::http::StatusCode::OK,
        Err(_) => axum::http::StatusCode::SERVICE_UNAVAILABLE,
    }
}
