use presentation::composition::services::AppServices;
use std::sync::Arc;

/// The transport-facing Identity state. It intentionally exposes application
/// services only, keeping host configuration and persistence out of
/// controllers.
#[derive(Clone)]
pub struct IdentityHttpState {
    pub(crate) services: Arc<AppServices>,
}

impl IdentityHttpState {
    pub fn new(services: Arc<AppServices>) -> Self {
        Self { services }
    }
}
