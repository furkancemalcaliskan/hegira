#![recursion_limit = "256"]

pub mod app;
pub mod identity;
pub mod shared;

#[cfg(feature = "ssr")]
pub use identity_application;
pub use identity_application_contracts;
pub use identity_domain_shared;
