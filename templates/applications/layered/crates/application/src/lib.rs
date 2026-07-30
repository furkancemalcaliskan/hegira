//! Application services and use-case orchestration.
//!
//! Authorization, validation, and transaction intent belong at this boundary.

use app_application_contracts::ApplicationSummary;

#[derive(Debug, Clone)]
pub struct ApplicationInformation {
    name: String,
}

impl ApplicationInformation {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn summary(&self) -> ApplicationSummary {
        ApplicationSummary {
            name: self.name.clone(),
            locale: app_domain_shared::DEFAULT_LOCALE,
        }
    }
}
