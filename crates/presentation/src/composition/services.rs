use application::{
    catalog::products::ProductAppService,
    identity::{
        auth::{AuthAppService, SessionPolicy},
        authorization::TokenCurrentUserProvider,
        oauth::service::{OAuthAppService, OAuthProviderSettings, OAuthSettings},
        permissions::service::PermissionAppService,
        users::{UserAppService, UserSearch},
    },
};
use infrastructure::config::{AppConfig, OAuthProviderConfig};
use infrastructure::{
    audit::AuditLoggerAdapter,
    cache::CacheAdapter,
    catalog::ProductRepositoryAdapter,
    db::DatabasePool,
    identity::authorization::CachedAuthorization,
    identity::oauth::provider_client::ReqwestOAuthProviderClient,
    identity::{IdentityRepositoryAdapter, sessions::SessionRepositoryAdapter},
    mail::MailerAdapter,
    search::SearchAdapter,
    security::{password_hasher::Argon2PasswordHasher, token_service::JwtTokenService},
};

// hegira:service-imports
// hegira:service-imports:end

pub type IdentityAuthService = AuthAppService<
    IdentityRepositoryAdapter,
    SessionRepositoryAdapter,
    IdentityRepositoryAdapter,
    IdentityRepositoryAdapter,
    Argon2PasswordHasher,
    JwtTokenService,
    MailerAdapter,
>;

pub type IdentityUserService = UserAppService<
    IdentityRepositoryAdapter,
    Argon2PasswordHasher,
    TokenCurrentUserProvider<SessionRepositoryAdapter, IdentityRepositoryAdapter, JwtTokenService>,
    CachedAuthorization<IdentityRepositoryAdapter, CacheAdapter>,
    CacheAdapter,
    AuditLoggerAdapter,
    SearchAdapter,
>;

pub type IdentityPermissionService = PermissionAppService<
    IdentityRepositoryAdapter,
    TokenCurrentUserProvider<SessionRepositoryAdapter, IdentityRepositoryAdapter, JwtTokenService>,
    CachedAuthorization<IdentityRepositoryAdapter, CacheAdapter>,
    CacheAdapter,
    AuditLoggerAdapter,
>;

pub type IdentityOAuthService = OAuthAppService<
    IdentityRepositoryAdapter,
    TokenCurrentUserProvider<SessionRepositoryAdapter, IdentityRepositoryAdapter, JwtTokenService>,
    SessionRepositoryAdapter,
    IdentityRepositoryAdapter,
    JwtTokenService,
    Argon2PasswordHasher,
    AuditLoggerAdapter,
    ReqwestOAuthProviderClient,
>;

pub type CatalogProductService = ProductAppService<
    ProductRepositoryAdapter,
    TokenCurrentUserProvider<SessionRepositoryAdapter, IdentityRepositoryAdapter, JwtTokenService>,
    CachedAuthorization<IdentityRepositoryAdapter, CacheAdapter>,
>;

// hegira:service-type-aliases
// hegira:service-type-aliases:end

#[derive(Clone)]
pub struct AppServices {
    pub auth: IdentityAuthService,
    pub oauth: IdentityOAuthService,
    pub users: IdentityUserService,
    pub permissions: IdentityPermissionService,
    pub products: CatalogProductService,
    // hegira:service-fields
    // hegira:service-fields:end
}

impl AppServices {
    pub fn new(
        pool: DatabasePool,
        config: &AppConfig,
        cache: CacheAdapter,
        search: SearchAdapter,
    ) -> Self {
        Self {
            auth: auth_service(pool.clone(), config),
            oauth: oauth_service(pool.clone(), config),
            users: user_service(pool.clone(), config, cache.clone(), search),
            permissions: permission_service(pool.clone(), config, cache.clone()),
            products: product_service(pool, config, cache),
            // hegira:service-init
            // hegira:service-init:end
        }
    }
}

pub fn product_service(
    pool: DatabasePool,
    config: &AppConfig,
    cache: CacheAdapter,
) -> CatalogProductService {
    let products = ProductRepositoryAdapter::new(pool.clone());
    let identity = IdentityRepositoryAdapter::new(pool.clone());
    let sessions = SessionRepositoryAdapter::from_database(config, pool)
        .expect("failed to initialize session store");
    let max_lifetime = chrono::Duration::seconds(config.sessions.max_lifetime_seconds as i64);
    let current_users = TokenCurrentUserProvider::new(
        sessions,
        identity.clone(),
        JwtTokenService::new_with_lifetime(config.security.jwt_secret.clone(), max_lifetime),
    );
    ProductAppService::new(
        products,
        current_users,
        CachedAuthorization::new(
            identity,
            cache,
            std::time::Duration::from_secs(config.cache.authorization_ttl_seconds),
        ),
    )
}

pub fn auth_service(pool: DatabasePool, config: &AppConfig) -> IdentityAuthService {
    let repository = IdentityRepositoryAdapter::new(pool.clone());
    let sessions = SessionRepositoryAdapter::from_database(config, pool)
        .expect("failed to initialize session store");
    let max_lifetime = chrono::Duration::seconds(config.sessions.max_lifetime_seconds as i64);

    AuthAppService::new(
        repository.clone(),
        sessions,
        repository.clone(),
        repository,
        Argon2PasswordHasher,
        JwtTokenService::new_with_lifetime(config.security.jwt_secret.clone(), max_lifetime),
        MailerAdapter::from_config(config).expect("failed to initialize mailer"),
        config.application.name.clone(),
        config.application.public_url.clone(),
        SessionPolicy {
            sliding_ttl: chrono::Duration::seconds(config.sessions.sliding_ttl_seconds as i64),
            max_lifetime,
            refresh_threshold_percent: config.sessions.refresh_threshold_percent,
        },
        config.search.enabled && config.jobs.durable.enabled,
        config.mailer.enabled && config.jobs.durable.enabled,
    )
}

pub fn user_service(
    pool: DatabasePool,
    config: &AppConfig,
    cache: CacheAdapter,
    search: SearchAdapter,
) -> IdentityUserService {
    let audit = AuditLoggerAdapter::from_database(config, pool.clone());
    let repository = IdentityRepositoryAdapter::new(pool.clone());
    let sessions = SessionRepositoryAdapter::from_database(config, pool)
        .expect("failed to initialize session store");
    let max_lifetime = chrono::Duration::seconds(config.sessions.max_lifetime_seconds as i64);
    let current_users = TokenCurrentUserProvider::new(
        sessions,
        repository.clone(),
        JwtTokenService::new_with_lifetime(config.security.jwt_secret.clone(), max_lifetime),
    );

    UserAppService::new(
        repository.clone(),
        Argon2PasswordHasher,
        current_users,
        CachedAuthorization::new(
            repository,
            cache.clone(),
            std::time::Duration::from_secs(config.cache.authorization_ttl_seconds),
        ),
        cache,
        audit,
        UserSearch {
            adapter: search,
            enabled: config.search.enabled,
            publish_mutations: config.search.enabled && config.jobs.durable.enabled,
        },
    )
}

pub fn permission_service(
    pool: DatabasePool,
    config: &AppConfig,
    cache: CacheAdapter,
) -> IdentityPermissionService {
    let audit = AuditLoggerAdapter::from_database(config, pool.clone());
    let repository = IdentityRepositoryAdapter::new(pool.clone());
    let sessions = SessionRepositoryAdapter::from_database(config, pool)
        .expect("failed to initialize session store");
    let max_lifetime = chrono::Duration::seconds(config.sessions.max_lifetime_seconds as i64);
    let current_users = TokenCurrentUserProvider::new(
        sessions,
        repository.clone(),
        JwtTokenService::new_with_lifetime(config.security.jwt_secret.clone(), max_lifetime),
    );

    PermissionAppService::new(
        repository.clone(),
        current_users,
        CachedAuthorization::new(
            repository,
            cache.clone(),
            std::time::Duration::from_secs(config.cache.authorization_ttl_seconds),
        ),
        cache,
        audit,
    )
}

pub fn oauth_service(pool: DatabasePool, config: &AppConfig) -> IdentityOAuthService {
    let repository = IdentityRepositoryAdapter::new(pool.clone());
    let current_user_sessions = SessionRepositoryAdapter::from_database(config, pool.clone())
        .expect("failed to initialize session store");
    let oauth_sessions = SessionRepositoryAdapter::from_database(config, pool.clone())
        .expect("failed to initialize session store");
    let max_lifetime = chrono::Duration::seconds(config.sessions.max_lifetime_seconds as i64);
    let token_service =
        JwtTokenService::new_with_lifetime(config.security.jwt_secret.clone(), max_lifetime);
    let current_users = TokenCurrentUserProvider::new(
        current_user_sessions,
        repository.clone(),
        token_service.clone(),
    );
    let audit = AuditLoggerAdapter::from_database(config, pool.clone());

    OAuthAppService::new(
        repository.clone(),
        current_users,
        oauth_sessions,
        repository,
        token_service,
        Argon2PasswordHasher,
        audit,
        ReqwestOAuthProviderClient::default(),
        OAuthSettings {
            enabled: config.oauth.enabled,
            state_ttl: chrono::Duration::seconds(config.oauth.state_ttl_seconds),
            google: oauth_provider_settings(&config.oauth.providers.google),
            github: oauth_provider_settings(&config.oauth.providers.github),
        },
        chrono::Duration::seconds(config.sessions.sliding_ttl_seconds as i64),
        config.search.enabled && config.jobs.durable.enabled,
    )
}

// hegira:service-factories
// hegira:service-factories:end

fn oauth_provider_settings(config: &OAuthProviderConfig) -> OAuthProviderSettings {
    OAuthProviderSettings {
        enabled: config.enabled,
        client_id: config.client_id.clone(),
        client_secret: config.client_secret.clone(),
        redirect_uri: config.redirect_uri.clone(),
        authorization_url: config.authorization_url.clone(),
        token_url: config.token_url.clone(),
        userinfo_url: config.userinfo_url.clone(),
        scopes: config.scopes.clone(),
    }
}
