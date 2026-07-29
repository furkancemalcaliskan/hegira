pub mod provider_client;
#[cfg(feature = "db-postgres")]
#[path = "../../../../../modules/identity/sqlx/src/identity/oauth/repository.rs"]
pub mod repository;
#[cfg(feature = "db-sqlite")]
#[path = "../../../../../modules/identity/sqlx/src/identity/oauth/sqlite_repository.rs"]
pub mod sqlite_repository;

#[cfg(feature = "db-sqlite")]
pub use sqlite_repository::SqliteOAuthRepository;
