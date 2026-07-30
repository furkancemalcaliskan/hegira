//! Transport-neutral inputs, outputs, permissions, and application contracts.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ApplicationSummary {
    pub name: String,
    pub locale: &'static str,
}
