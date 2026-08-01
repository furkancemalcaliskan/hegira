#![recursion_limit = "256"]

pub mod dashboard;
pub mod root;
pub mod routes;

pub use identity_leptos::identity;
pub use web::{app, shared};
