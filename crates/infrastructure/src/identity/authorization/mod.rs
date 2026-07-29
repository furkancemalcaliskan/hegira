#[cfg(feature = "db-postgres")]
#[path = "../../../../../modules/identity/sqlx/src/identity/authorization/repository.rs"]
pub mod repository;
pub mod service;
#[cfg(feature = "db-sqlite")]
#[path = "../../../../../modules/identity/sqlx/src/identity/authorization/sqlite_repository.rs"]
pub mod sqlite_repository;

pub use service::{AllowAuthenticatedIdentityUsers, CachedAuthorization, RepositoryAuthorization};
#[cfg(feature = "db-sqlite")]
pub use sqlite_repository::SqliteAuthorizationRepository;
