#![recursion_limit = "256"]

pub mod app;
pub mod catalog;
pub mod identity;
pub mod root;
pub mod routes;
pub mod shared;

#[cfg(feature = "ssr")]
pub use ::application;
pub use ::application_contracts;
pub use ::domain_shared;
#[cfg(feature = "ssr")]
pub use ::presentation;

pub mod web {
    pub use crate::{app, catalog, identity, root, routes, shared};
}
