#![recursion_limit = "256"]

pub mod identity;

// The application shell and shared UI primitives remain framework-owned.
// These re-exports let the canonical adapter sources compile independently
// while the current `web` package consumes the same sources as a compatibility
// view.
#[cfg(feature = "ssr")]
pub use application;
pub use application_contracts;
pub use domain_shared;
#[cfg(feature = "ssr")]
pub use presentation;
pub use web::{app, shared};

pub mod web {
    pub use crate::identity;
    pub use ::web::{app, shared};
}
