#[cfg(feature = "db-postgres")]
#[path = "../../../../../modules/identity/sqlx/src/identity/two_factor/repository.rs"]
pub mod repository;
#[cfg(feature = "db-sqlite")]
#[path = "../../../../../modules/identity/sqlx/src/identity/two_factor/sqlite_repository.rs"]
pub mod sqlite_repository;

#[cfg(feature = "db-sqlite")]
pub use sqlite_repository::SqliteTwoFactorRepository;
