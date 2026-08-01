#[path = "../../../../modules/identity/sqlx/src/identity/audit.rs"]
mod audit;
pub mod authorization;
#[path = "../../../../modules/identity/sqlx/src/identity/cleanup.rs"]
pub mod cleanup;
pub mod migrations;
pub mod oauth;
#[path = "../../../../modules/identity/sqlx/src/identity/provider.rs"]
pub mod provider;
#[cfg(feature = "db-postgres")]
#[path = "../../../../modules/identity/sqlx/src/identity/repository.rs"]
pub mod repository;
#[path = "../../../../modules/identity/sqlx/src/identity/reset.rs"]
pub mod reset;
#[cfg(test)]
#[path = "../../../../modules/identity/sqlx/src/identity/retirement_tests.rs"]
mod retirement_tests;
#[path = "../../../../modules/identity/sqlx/src/identity/search.rs"]
pub mod search;
#[path = "../../../../modules/identity/sqlx/src/identity/seed.rs"]
pub mod seed;
pub mod sessions;
pub mod two_factor;
pub mod users;

pub use provider::IdentityRepositoryAdapter;
#[cfg(feature = "db-postgres")]
pub use repository::SqlxIdentityRepository;
