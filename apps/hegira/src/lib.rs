#![recursion_limit = "256"]

#[cfg(feature = "ssr")]
pub use ::application;
pub use ::application_contracts;
#[cfg(feature = "ssr")]
pub use ::background_jobs;
#[cfg(feature = "ssr")]
pub use ::domain;
pub use ::domain_shared;
#[cfg(feature = "ssr")]
pub use ::http_support;
#[cfg(feature = "ssr")]
pub use ::identity_http;
#[cfg(feature = "ssr")]
pub use ::infrastructure;
#[cfg(feature = "ssr")]
pub use ::observability;
#[cfg(feature = "ssr")]
pub use ::presentation;
#[cfg(feature = "ssr")]
pub use ::runtime;
#[cfg(feature = "test-support")]
pub use ::test_support;
pub use ::web;

#[cfg(feature = "ssr")]
pub mod server;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(web::root::App);
}
