pub mod authorization;
pub mod oauth;
pub mod sessions;
pub mod two_factor;
pub mod users;

#[cfg(any(feature = "db-postgres", feature = "db-sqlite"))]
pub use identity_sqlx::identity::{
    IdentityRepositoryAdapter, cleanup, migrations, provider, reset, search, seed,
};
#[cfg(feature = "db-postgres")]
pub use identity_sqlx::identity::{SqlxIdentityRepository, repository};
