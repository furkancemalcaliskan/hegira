#![recursion_limit = "256"]

pub mod app;
#[path = "../../../modules/identity/leptos/src/identity/mod.rs"]
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
    pub use crate::{app, identity, root, routes, shared};
}
