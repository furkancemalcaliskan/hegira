pub mod authorization;
pub mod oauth;
pub mod services;
pub mod sessions;

pub use identity_sqlx::identity::{IdentityRepositoryAdapter, migrations, search, seed};

#[cfg(feature = "db-postgres")]
pub use identity_sqlx::identity::SqlxIdentityRepository;
