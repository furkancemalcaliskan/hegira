use serde::{Deserialize, Serialize};
use std::future::Future;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub actor: String,
    pub action: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub details: serde_json::Value,
}

impl AuditLogEntry {
    pub fn new(
        actor: impl Into<String>,
        action: impl Into<String>,
        entity_type: impl Into<String>,
        entity_id: Option<String>,
        details: serde_json::Value,
    ) -> Self {
        Self {
            actor: actor.into(),
            action: action.into(),
            entity_type: entity_type.into(),
            entity_id,
            details,
        }
    }
}

pub trait AuditLogger: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn record(&self, entry: AuditLogEntry) -> impl Future<Output = Result<(), Self::Error>> + Send;
}
