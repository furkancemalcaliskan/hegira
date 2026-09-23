use std::{collections::BTreeMap, fs};

use application_manifest::{ClientAdapter, DatabaseAdapter};
use application_mutator::{
    ApplicationFileOwner, CargoDependency, CargoDependencySection, ComponentArtifact,
    ComponentCargoManifest, ComponentConfigurationTarget, ComponentContribution,
    ComponentEditOutcome, ComponentInstallationPlan, ComponentIntegration,
    ComponentManagedRustTarget, ComponentRustModuleTarget, StructuredFileEdit,
    compose_component_contributions, plan_component_cargo_dependency, plan_component_cargo_feature,
    plan_component_composition_identity, plan_component_configuration_entry,
    plan_component_installation, plan_component_managed_rust_entry, plan_component_rust_module,
};
use template_renderer::ResolvedComposition;

use crate::{ApplicationContext, CliDiagnostic};

const IDENTITY: &str = "identity";

pub(crate) fn plan(
    context: &ApplicationContext,
    resolved: &ResolvedComposition,
    component: &str,
) -> Result<ComponentInstallationPlan, CliDiagnostic> {
    if component != IDENTITY {
        return Err(CliDiagnostic::validation(format!(
            "bundled component `{component}` does not declare additive installation contributions"
        )));
    }
    let requested = resolved
        .components
        .iter()
        .find(|candidate| candidate.id == component)
        .and_then(|candidate| candidate.installation.as_ref())
        .ok_or_else(|| {
            CliDiagnostic::validation("Identity installation metadata is unavailable")
        })?;
    let manifest = context
        .manifest
        .as_ref()
        .expect("validated application manifest");
    let database = manifest
        .selection
        .databases
        .iter()
        .next()
        .copied()
        .ok_or_else(|| {
            CliDiagnostic::validation(
                "Identity installation requires one selected database adapter",
            )
        })?;
    let client = manifest
        .selection
        .clients
        .iter()
        .next()
        .copied()
        .ok_or_else(|| {
            CliDiagnostic::validation("Identity installation requires one selected client adapter")
        })?;
    if !requested.databases.contains(&database) || !requested.clients.contains(&client) {
        return Err(CliDiagnostic::validation(
            "Identity is not compatible with the selected database and client adapters",
        ));
    }

    let mut sources = SourceSet::load(context)?;
    let mut contributions = Vec::new();
    add_manifest_state(&mut sources, resolved, &mut contributions)?;
    add_dependencies(&mut sources, resolved, requested, &mut contributions)?;
    add_infrastructure(&mut sources, database, &mut contributions)?;
    add_server(&mut sources, &mut contributions)?;
    add_web(&mut sources, client, &mut contributions)?;
    add_artifacts(&mut contributions)?;

    let contributions = compose_component_contributions(contributions)
        .map_err(|error| CliDiagnostic::conflict(error.to_string()))?;
    plan_component_installation(component, manifest.installed_component_ids(), contributions)
        .map_err(|error| CliDiagnostic::conflict(error.to_string()))
}

struct SourceSet {
    values: BTreeMap<&'static str, Vec<u8>>,
}

impl SourceSet {
    fn load(context: &ApplicationContext) -> Result<Self, CliDiagnostic> {
        let paths = [
            "hegira.toml",
            "Cargo.toml",
            "crates/infrastructure/Cargo.toml",
            "crates/infrastructure/src/lib.rs",
            "crates/infrastructure/src/operations.rs",
            "apps/server/Cargo.toml",
            "apps/server/src/lib.rs",
            "apps/server/src/server.rs",
            "apps/web/Cargo.toml",
            "apps/web/src/lib.rs",
            "apps/web/src/root.rs",
            "apps/web/src/routes.rs",
            "apps/web/src/dashboard.rs",
        ];
        let mut values = BTreeMap::new();
        for path in paths {
            let absolute = context.root.join(path);
            let metadata = fs::symlink_metadata(&absolute).map_err(|_| {
                CliDiagnostic::conflict(format!("required application source `{path}` is missing"))
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(CliDiagnostic::conflict(format!(
                    "required application source `{path}` must be a regular file"
                )));
            }
            values.insert(
                path,
                fs::read(absolute).map_err(|_| {
                    CliDiagnostic::conflict(format!(
                        "required application source `{path}` is unreadable"
                    ))
                })?,
            );
        }
        Ok(Self { values })
    }

    fn get(&self, path: &'static str) -> &[u8] {
        self.values
            .get(path)
            .expect("installation source is loaded")
    }

    fn accept(
        &mut self,
        path: &'static str,
        outcome: ComponentEditOutcome,
        contributions: &mut Vec<ComponentContribution>,
    ) {
        if let ComponentEditOutcome::Planned(contribution) = outcome {
            self.values
                .insert(path, contribution.resulting_content().to_vec());
            contributions.push(contribution);
        }
    }
}

fn edit_error(error: impl std::fmt::Display) -> CliDiagnostic {
    CliDiagnostic::conflict(error.to_string())
}

fn add_manifest_state(
    sources: &mut SourceSet,
    resolved: &ResolvedComposition,
    contributions: &mut Vec<ComponentContribution>,
) -> Result<(), CliDiagnostic> {
    let identity = resolved
        .components
        .iter()
        .find(|component| component.id == IDENTITY)
        .expect("resolved Identity component exists");
    for (target, id, version) in [
        (
            ComponentConfigurationTarget::Components,
            identity.id.as_str(),
            identity.version.as_str(),
        ),
        (
            ComponentConfigurationTarget::Modules,
            IDENTITY,
            identity.version.as_str(),
        ),
    ] {
        let outcome =
            plan_component_composition_identity(target, sources.get("hegira.toml"), id, version)
                .map_err(edit_error)?;
        sources.accept("hegira.toml", outcome, contributions);
    }
    for capability in ["authentication", "authorization"] {
        let outcome = plan_component_configuration_entry(
            ComponentConfigurationTarget::Capabilities,
            sources.get("hegira.toml"),
            capability,
        )
        .map_err(edit_error)?;
        sources.accept("hegira.toml", outcome, contributions);
    }
    Ok(())
}

fn add_dependencies(
    sources: &mut SourceSet,
    resolved: &ResolvedComposition,
    installation: &template_renderer::ComponentInstallationManifest,
    contributions: &mut Vec<ComponentContribution>,
) -> Result<(), CliDiagnostic> {
    for dependency in &installation.framework_dependencies {
        let dependency = CargoDependency::framework_release(
            dependency.name.clone(),
            resolved.framework.repository.clone(),
            resolved.framework.version.clone(),
            dependency.default_features.unwrap_or(true),
            false,
            std::iter::empty::<String>(),
        );
        cargo_dependency(
            sources,
            ComponentCargoManifest::Workspace,
            CargoDependencySection::WorkspaceDependencies,
            dependency,
            contributions,
        )?;
    }
    for (name, version, features) in [
        ("argon2", "0.5.3", vec![]),
        ("base64", "0.23.0", vec![]),
        ("hmac", "0.13.0", vec![]),
        ("ipnet", "2.12.0", vec![]),
        ("rand_core", "0.6.4", vec!["getrandom"]),
        ("reqwest", "0.12.28", vec!["json", "rustls-tls"]),
        ("serde_json", "1.0.145", vec![]),
        ("sha2", "0.11.0", vec![]),
    ] {
        cargo_dependency(
            sources,
            ComponentCargoManifest::Workspace,
            CargoDependencySection::WorkspaceDependencies,
            CargoDependency::registry(name, version, false, features),
            contributions,
        )?;
    }

    for name in [
        "audit",
        "background_jobs",
        "cache",
        "identity_application",
        "identity_application_contracts",
        "identity_domain",
        "identity_domain_shared",
        "identity_sqlx",
        "mail",
        "observability",
        "runtime",
        "search",
    ] {
        cargo_dependency(
            sources,
            ComponentCargoManifest::Infrastructure,
            CargoDependencySection::Dependencies,
            CargoDependency::workspace(name, false, std::iter::empty::<String>()),
            contributions,
        )?;
    }
    for name in [
        "argon2",
        "base64",
        "chrono",
        "hmac",
        "ipnet",
        "rand_core",
        "reqwest",
        "serde_json",
        "sha2",
        "uuid",
    ] {
        cargo_dependency(
            sources,
            ComponentCargoManifest::Infrastructure,
            CargoDependencySection::Dependencies,
            CargoDependency::workspace(name, false, std::iter::empty::<String>()),
            contributions,
        )?;
    }
    for name in [
        "cache",
        "identity_http",
        "identity_leptos",
        "identity_sqlx",
        "mail",
        "platform_core",
        "search",
    ] {
        cargo_dependency(
            sources,
            ComponentCargoManifest::Server,
            CargoDependencySection::Dependencies,
            CargoDependency::workspace(name, true, std::iter::empty::<String>()),
            contributions,
        )?;
    }
    cargo_dependency(
        sources,
        ComponentCargoManifest::Web,
        CargoDependencySection::Dependencies,
        CargoDependency::workspace("identity_leptos", false, std::iter::empty::<String>()),
        contributions,
    )?;

    for (manifest, feature, selection) in [
        (
            ComponentCargoManifest::Infrastructure,
            "db-postgres",
            "identity_sqlx/db-postgres",
        ),
        (
            ComponentCargoManifest::Infrastructure,
            "db-sqlite",
            "identity_sqlx/db-sqlite",
        ),
        (
            ComponentCargoManifest::Server,
            "db-postgres",
            "identity_http/db-postgres",
        ),
        (
            ComponentCargoManifest::Server,
            "db-postgres",
            "identity_sqlx/db-postgres",
        ),
        (
            ComponentCargoManifest::Server,
            "db-sqlite",
            "identity_http/db-sqlite",
        ),
        (
            ComponentCargoManifest::Server,
            "db-sqlite",
            "identity_sqlx/db-sqlite",
        ),
        (ComponentCargoManifest::Server, "ssr", "dep:cache"),
        (ComponentCargoManifest::Server, "ssr", "dep:identity_http"),
        (ComponentCargoManifest::Server, "ssr", "dep:identity_leptos"),
        (ComponentCargoManifest::Server, "ssr", "dep:mail"),
        (ComponentCargoManifest::Server, "ssr", "dep:platform_core"),
        (ComponentCargoManifest::Server, "ssr", "dep:search"),
        (ComponentCargoManifest::Server, "ssr", "identity_leptos/ssr"),
        (
            ComponentCargoManifest::Server,
            "openapi",
            "identity_http/openapi",
        ),
        (
            ComponentCargoManifest::Web,
            "db-postgres",
            "identity_leptos/db-postgres",
        ),
        (
            ComponentCargoManifest::Web,
            "db-sqlite",
            "identity_leptos/db-sqlite",
        ),
        (
            ComponentCargoManifest::Web,
            "hydrate",
            "identity_leptos/hydrate",
        ),
        (ComponentCargoManifest::Web, "ssr", "identity_leptos/ssr"),
        (
            ComponentCargoManifest::Web,
            "wasm-split",
            "identity_leptos/wasm-split",
        ),
    ] {
        let path = cargo_path(manifest);
        let outcome = plan_component_cargo_feature(manifest, sources.get(path), feature, selection)
            .map_err(edit_error)?;
        sources.accept(path, outcome, contributions);
    }
    Ok(())
}

fn cargo_dependency(
    sources: &mut SourceSet,
    manifest: ComponentCargoManifest,
    section: CargoDependencySection,
    dependency: CargoDependency,
    contributions: &mut Vec<ComponentContribution>,
) -> Result<(), CliDiagnostic> {
    let path = cargo_path(manifest);
    let outcome =
        plan_component_cargo_dependency(manifest, sources.get(path), section, &dependency)
            .map_err(edit_error)?;
    sources.accept(path, outcome, contributions);
    Ok(())
}

fn cargo_path(manifest: ComponentCargoManifest) -> &'static str {
    match manifest {
        ComponentCargoManifest::Workspace => "Cargo.toml",
        ComponentCargoManifest::Infrastructure => "crates/infrastructure/Cargo.toml",
        ComponentCargoManifest::Server => "apps/server/Cargo.toml",
        ComponentCargoManifest::Web => "apps/web/Cargo.toml",
        _ => unreachable!("Identity installer uses only outward Cargo manifests"),
    }
}

fn add_infrastructure(
    sources: &mut SourceSet,
    database: DatabaseAdapter,
    contributions: &mut Vec<ComponentContribution>,
) -> Result<(), CliDiagnostic> {
    for module in ["audit", "identity", "identity_config", "security"] {
        let outcome = plan_component_rust_module(
            ComponentRustModuleTarget::Infrastructure,
            sources.get("crates/infrastructure/src/lib.rs"),
            module,
        )
        .map_err(edit_error)?;
        sources.accept("crates/infrastructure/src/lib.rs", outcome, contributions);
    }
    let target = match database {
        DatabaseAdapter::Postgres => {
            ComponentManagedRustTarget::InfrastructurePostgresMigrationSources
        }
        DatabaseAdapter::Sqlite => ComponentManagedRustTarget::InfrastructureSqliteMigrationSources,
    };
    let entry = match database {
        DatabaseAdapter::Postgres => {
            "identity_sqlx::identity::migrations::postgres_migration_source(),"
        }
        DatabaseAdapter::Sqlite => {
            "identity_sqlx::identity::migrations::sqlite_migration_source(),"
        }
    };
    let outcome = plan_component_managed_rust_entry(
        target,
        sources.get("crates/infrastructure/src/operations.rs"),
        IDENTITY,
        entry,
    )
    .map_err(edit_error)?;
    sources.accept(
        "crates/infrastructure/src/operations.rs",
        outcome,
        contributions,
    );
    Ok(())
}

fn add_server(
    sources: &mut SourceSet,
    contributions: &mut Vec<ComponentContribution>,
) -> Result<(), CliDiagnostic> {
    let outcome = plan_component_managed_rust_entry(
        ComponentManagedRustTarget::ServerModules,
        sources.get("apps/server/src/lib.rs"),
        IDENTITY,
        "#[cfg(feature = \"ssr\")]\nmod identity_runtime;",
    )
    .map_err(edit_error)?;
    sources.accept("apps/server/src/lib.rs", outcome, contributions);
    for (target, key, entry) in [
        (
            ComponentManagedRustTarget::ServerPreflight,
            "identity",
            "let identity_config = crate::identity_runtime::IdentityRuntime::preflight()?;",
        ),
        (
            ComponentManagedRustTarget::ServerInitialization,
            "identity",
            "let identity_runtime = crate::identity_runtime::IdentityRuntime::initialize(database.clone(), identity_config).await?;\nlet identity_web = identity_runtime.clone();",
        ),
        (
            ComponentManagedRustTarget::ServerLeptosContexts,
            "identity",
            "identity_web.provide_leptos_context();",
        ),
        (
            ComponentManagedRustTarget::ServerBearerRoutes,
            "identity",
            ".merge(identity_runtime.bearer_routes())",
        ),
        (
            ComponentManagedRustTarget::ServerCookieBffPolicy,
            "identity",
            ".layer(axum::middleware::from_fn_with_state(identity_runtime.cookie_policy(), http_support::csrf::validate))",
        ),
        (
            ComponentManagedRustTarget::ServerRateLimit,
            "identity",
            ".layer(axum::middleware::from_fn_with_state(identity_runtime.rate_limiter(), http_support::rate_limit::check_configured))",
        ),
    ] {
        let outcome = plan_component_managed_rust_entry(
            target,
            sources.get("apps/server/src/server.rs"),
            key,
            entry,
        )
        .map_err(edit_error)?;
        sources.accept("apps/server/src/server.rs", outcome, contributions);
    }
    Ok(())
}

fn add_web(
    sources: &mut SourceSet,
    client: ClientAdapter,
    contributions: &mut Vec<ComponentContribution>,
) -> Result<(), CliDiagnostic> {
    if client != ClientAdapter::Leptos {
        return Err(CliDiagnostic::validation(
            "Identity installation currently requires the Leptos client",
        ));
    }
    remove_minimal_root_route(sources, contributions)?;
    let outcome = plan_component_managed_rust_entry(
        ComponentManagedRustTarget::WebNativeRoutes,
        sources.get("apps/web/src/routes.rs"),
        IDENTITY,
        "<identity_leptos::identity::routes::IdentityRoutes/>",
    )
    .map_err(edit_error)?;
    sources.accept("apps/web/src/routes.rs", outcome, contributions);
    let outcome = plan_component_managed_rust_entry(
        ComponentManagedRustTarget::WebDashboardNavigation,
        sources.get("apps/web/src/dashboard.rs"),
        IDENTITY,
        "<nav class=\"identity-navigation\"><a href=\"/login\">\"Sign in\"</a><a href=\"/profile\">\"Profile\"</a><a href=\"/admin/users\">\"Users\"</a></nav>",
    )
    .map_err(edit_error)?;
    sources.accept("apps/web/src/dashboard.rs", outcome, contributions);
    let outcome = plan_component_managed_rust_entry(
        ComponentManagedRustTarget::WebProviders,
        sources.get("apps/web/src/root.rs"),
        IDENTITY,
        "provide_context(identity_leptos::app::auth_state::AuthState::new());\nprovide_context(identity_leptos::shared::i18n::I18n::default());",
    )
    .map_err(edit_error)?;
    sources.accept("apps/web/src/root.rs", outcome, contributions);
    Ok(())
}

fn remove_minimal_root_route(
    sources: &mut SourceSet,
    contributions: &mut Vec<ComponentContribution>,
) -> Result<(), CliDiagnostic> {
    const PATH: &str = "apps/web/src/routes.rs";
    const ROOT: &str =
        "                    <Route path=StaticSegment(\"\") view=DashboardRoute/>\n";
    let observed = sources.get(PATH);
    let source = std::str::from_utf8(observed)
        .map_err(|_| CliDiagnostic::conflict("application routes must be UTF-8"))?;
    if source.matches(ROOT).count() != 1 {
        return Err(CliDiagnostic::conflict(
            "minimal dashboard root route differs from the supported Identity installation point",
        ));
    }
    let resulting = source.replacen(ROOT, "", 1);
    let edit =
        StructuredFileEdit::new(PATH, observed, resulting.into_bytes()).map_err(edit_error)?;
    let integration =
        ComponentIntegration::new(ApplicationFileOwner::Web, edit).map_err(edit_error)?;
    sources.accept(
        PATH,
        ComponentEditOutcome::Planned(integration.into()),
        contributions,
    );
    Ok(())
}

fn add_artifacts(contributions: &mut Vec<ComponentContribution>) -> Result<(), CliDiagnostic> {
    for (owner, path, source) in identity_artifacts() {
        let source = strip_test_modules(source);
        contributions.push(
            ComponentArtifact::new(owner, path, source.into_bytes())
                .map_err(edit_error)?
                .into(),
        );
    }
    Ok(())
}

fn identity_artifacts() -> Vec<(ApplicationFileOwner, &'static str, String)> {
    let infrastructure = ApplicationFileOwner::Infrastructure;
    let replace_config = |source: &str| source.replace("crate::config", "crate::identity_config");
    vec![
        (infrastructure, "crates/infrastructure/src/identity_config.rs", include_str!("../../../templates/applications/layered/crates/infrastructure/src/config.rs")
            .replace(".set_default(\"startup.scheduler\", true)?", ".set_default(\"startup.scheduler\", false)?")
            .replace(".set_default(\"scheduler.enabled\", true)?", ".set_default(\"scheduler.enabled\", false)?")
            .replace(".set_default(\"mailer.enabled\", true)?", ".set_default(\"mailer.enabled\", false)?")),
        (infrastructure, "crates/infrastructure/src/audit/mod.rs", strip_between(
            replace_config(include_str!("../../../templates/applications/layered/crates/infrastructure/src/audit/mod.rs")),
            "#[cfg(all(test, feature = \"db-sqlite\"))]",
            "#[derive(Debug, Clone, Copy)]",
        )),
        (infrastructure, "crates/infrastructure/src/identity/mod.rs", include_str!("../../../templates/applications/layered/crates/infrastructure/src/identity/mod.rs").to_owned()),
        (infrastructure, "crates/infrastructure/src/identity/authorization/mod.rs", include_str!("../../../templates/applications/layered/crates/infrastructure/src/identity/authorization/mod.rs").to_owned()),
        (infrastructure, "crates/infrastructure/src/identity/authorization/service.rs", include_str!("../../../templates/applications/layered/crates/infrastructure/src/identity/authorization/service.rs").to_owned()),
        (infrastructure, "crates/infrastructure/src/identity/oauth/mod.rs", include_str!("../../../templates/applications/layered/crates/infrastructure/src/identity/oauth/mod.rs").to_owned()),
        (infrastructure, "crates/infrastructure/src/identity/oauth/provider_client.rs", include_str!("../../../templates/applications/layered/crates/infrastructure/src/identity/oauth/provider_client.rs").to_owned()),
        (infrastructure, "crates/infrastructure/src/identity/sessions/mod.rs", replace_config(include_str!("../../../templates/applications/layered/crates/infrastructure/src/identity/sessions/mod.rs")).replace("feature = \"cache-redis\"", "any()")),
        (infrastructure, "crates/infrastructure/src/identity/sessions/redis_session.rs", replace_config(include_str!("../../../templates/applications/layered/crates/infrastructure/src/identity/sessions/redis_session.rs"))),
        (infrastructure, "crates/infrastructure/src/identity/services.rs", replace_config(include_str!("../../../templates/applications/layered/crates/infrastructure/src/identity/services.rs"))),
        (infrastructure, "crates/infrastructure/src/security/mod.rs", include_str!("../../../templates/applications/layered/crates/infrastructure/src/security/mod.rs").to_owned()),
        (infrastructure, "crates/infrastructure/src/security/password_hasher.rs", include_str!("../../../templates/applications/layered/crates/infrastructure/src/security/password_hasher.rs").to_owned()),
        (infrastructure, "crates/infrastructure/src/security/token_service.rs", include_str!("../../../templates/applications/layered/crates/infrastructure/src/security/token_service.rs").to_owned()),
        (ApplicationFileOwner::Server, "apps/server/src/identity_runtime.rs", IDENTITY_RUNTIME.to_owned()),
    ]
}

fn strip_test_modules(source: String) -> String {
    let Some(position) = source.find("\n#[cfg(test)]\nmod tests") else {
        return source;
    };
    let mut output = source[..position].to_owned();
    output.push('\n');
    output
}

fn strip_between(source: String, start: &str, end: &str) -> String {
    let Some(start_index) = source.find(start) else {
        return source;
    };
    let Some(relative_end) = source[start_index..].find(end) else {
        return source;
    };
    let end_index = start_index + relative_end;
    format!("{}{}", &source[..start_index], &source[end_index..])
}

const IDENTITY_RUNTIME: &str = r#"use app_infrastructure::{identity::services::AppServices, identity_config::AppConfig};
use leptos::prelude::provide_context;

#[derive(Clone)]
pub struct IdentityRuntime {
    services: std::sync::Arc<AppServices>,
    leptos: identity_leptos::identity::server::IdentityLeptosServices,
    cookie: identity_http::cookie::IdentityCookieSettings,
    policy: http_support::policy::CookieBffPolicy,
    rate_limiter: http_support::rate_limit::RateLimiter,
    openapi: bool,
}

impl IdentityRuntime {
    pub fn preflight() -> Result<AppConfig, String> {
        let config = AppConfig::load().map_err(|error| format!("failed to load Identity configuration: {error}"))?;
        configuration::validate(&config, platform_core::CompiledCapabilities {
            db_postgres: cfg!(feature = "db-postgres"), db_sqlite: cfg!(feature = "db-sqlite"),
            cache_redis: false, mailer_smtp: false, storage_s3: false,
            search_meilisearch: false, metrics_prometheus: false, otel_otlp: false,
            openapi: cfg!(feature = "openapi"),
        }).map_err(|error| error.to_string())?;
        if config.mailer.enabled || config.cache.enabled || config.search.enabled || config.storage.enabled
            || config.telemetry.enabled || config.metrics.enabled || config.jobs.durable.enabled
            || config.worker_operations.enabled || config.startup.scheduler {
            return Err("Identity installation does not compose distributed providers or background workers".to_owned());
        }
        if !matches!(config.security.rate_limit.backend, app_infrastructure::identity_config::RateLimitBackend::Memory) {
            return Err("Identity installation supports only memory rate limiting".to_owned());
        }
        if !matches!(config.sessions.backend, app_infrastructure::identity_config::SessionBackend::Database) {
            return Err("Identity installation supports only database-backed sessions".to_owned());
        }
        Ok(config)
    }

    pub async fn initialize(database: persistence::DatabasePool, config: AppConfig) -> Result<Self, String> {
        if config.startup.seed_identity {
            let repository = identity_sqlx::identity::IdentityRepositoryAdapter::new(database.clone());
            identity_sqlx::identity::seed::seed_identity(
                &repository,
                &app_infrastructure::security::password_hasher::Argon2PasswordHasher,
                &config.seed,
            ).await.map_err(|error| format!("failed to seed Identity: {error}"))?;
        }
        let cache = cache::CacheAdapter::from_settings(&cache::CacheSettings {
            enabled: false, backend: cache::CacheBackend::Null, redis_url: String::new(),
        }).map_err(|error| error.to_string())?;
        let search = search::SearchAdapter::from_settings(&search::SearchSettings {
            enabled: false, backend: search::SearchBackend::Null, index_prefix: config.search.index_prefix.clone(),
            task_timeout_milliseconds: config.search.task_timeout_milliseconds,
            meilisearch: search::MeilisearchSettings { url: String::new(), api_key: None },
        }).map_err(|error| error.to_string())?;
        let mailer = mail::MailerAdapter::from_settings(&mail::MailerSettings {
            enabled: false, backend: mail::MailerBackend::Null, from: config.mailer.from.clone(),
            smtp: mail::SmtpSettings { host: String::new(), port: 0, username: None, password: None, starttls: false },
        }).map_err(|error| error.to_string())?;
        let services = std::sync::Arc::new(AppServices::new(database, &config, cache, search, mailer));
        let leptos = identity_leptos::identity::server::IdentityLeptosServices::new(
            services.auth.clone(), services.oauth.clone(), services.users.clone(), services.permissions.clone(),
            identity_http::cookie::SESSION_COOKIE,
        );
        let cookie = identity_http::cookie::IdentityCookieSettings {
            secure: config.is_production(), max_lifetime_seconds: config.sessions.max_lifetime_seconds as i64,
        };
        let policy = identity_http::policy::IdentityTransportPolicies::from_public_url(&config.application.public_url)
            .map_err(|error| format!("invalid Identity transport policy: {error}"))?.cookie_bff;
        let rate_limiter = http_support::rate_limit::RateLimiter::from_options(
            &http_support::rate_limit::RateLimitOptions {
                enabled: config.security.rate_limit.enabled,
                max_requests: config.security.rate_limit.max_requests,
                window: std::time::Duration::from_secs(config.security.rate_limit.window_seconds),
                backend: http_support::rate_limit::RateLimitBackend::Memory,
            },
            &config.security.trusted_proxies,
        ).map_err(|error| format!("invalid Identity rate limit configuration: {error}"))?;
        Ok(Self { services, leptos, cookie, policy, rate_limiter, openapi: config.openapi.enabled && !config.is_production() })
    }

    pub fn provide_leptos_context(&self) {
        provide_context(self.leptos.clone());
        provide_context(self.cookie);
    }

    #[allow(dead_code)] // Becomes active after the first application resource is generated.
    pub fn services(&self) -> &AppServices {
        &self.services
    }

    pub fn bearer_routes(&self) -> axum::Router<leptos::prelude::LeptosOptions> {
        let router = identity_http::bearer_api_routes(identity_http::state::IdentityHttpState::new(
            self.services.auth.clone(), self.services.oauth.clone(), self.services.users.clone(), self.services.permissions.clone(),
        )).with_state(());
        #[cfg(feature = "openapi")]
        let router = if self.openapi {
            let document = identity_http::openapi::document();
            // hegira:resource-openapi-documents
            // hegira:resource-openapi-documents:end
            router.merge(identity_http::openapi::routes_with_document(document))
        } else { router };
        let _ = self.openapi;
        router
    }

    pub fn cookie_policy(&self) -> http_support::csrf::CsrfPolicy {
        self.policy.csrf()
    }

    pub fn rate_limiter(&self) -> http_support::rate_limit::RateLimiter {
        self.rate_limiter.clone()
    }
}
"#;
