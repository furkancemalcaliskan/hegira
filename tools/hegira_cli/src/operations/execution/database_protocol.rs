//! Closed data-only projection of the application-owned database protocol.
//! This tool neither interprets SQL nor authenticates database history itself.
use serde::{Deserialize, Serialize};

use super::{OperationError, OperationErrorKind, failure};
use crate::operations::{DatabaseOperation, OperationPlan};

pub(super) const OUTPUT_LIMIT: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseResult {
    pub output_schema: u32,
    pub outcome: DatabaseSuccess,
    pub operation: DatabaseOperation,
    pub provider: application_manifest::DatabaseAdapter,
    pub status: DatabaseStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DatabaseSuccess {
    Success,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseStatus {
    pub output_schema: u32,
    pub history: DatabaseHistory,
    pub migrations: Vec<DatabaseMigration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DatabaseHistory {
    DatabaseMissing,
    MetadataMissing,
    Present,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseMigration {
    pub module_id: String,
    pub version: i64,
    pub state: DatabaseMigrationState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DatabaseMigrationState {
    Applied,
    Pending,
}

impl DatabaseResult {
    pub(super) fn parse(bytes: &[u8], plan: &OperationPlan) -> Result<Self, OperationError> {
        if bytes.len() > OUTPUT_LIMIT {
            return Err(protocol_error());
        }
        let result: Self = serde_json::from_slice(bytes).map_err(|_| protocol_error())?;
        let operation = match plan.summary().intent {
            crate::operations::OperationIntent::DatabaseStatus { .. } => DatabaseOperation::Status,
            crate::operations::OperationIntent::DatabaseMigrate { .. } => {
                DatabaseOperation::Migrate
            }
            _ => return Err(protocol_error()),
        };
        if result.output_schema != 1
            || result.status.output_schema != 1
            || result.operation != operation
            || result.provider != plan.summary().database
            || result.status.migrations.len() > 4096
            || !result
                .status
                .migrations
                .windows(2)
                .all(|pair| pair[0].version < pair[1].version)
            || result.status.migrations.iter().any(|entry| {
                entry.module_id != "application"
                    && !plan
                        .summary()
                        .modules
                        .iter()
                        .any(|module| module.id == entry.module_id)
            })
            || (result.status.history != DatabaseHistory::Present
                && result
                    .status
                    .migrations
                    .iter()
                    .any(|entry| entry.state != DatabaseMigrationState::Pending))
            || (operation == DatabaseOperation::Migrate
                && (result.status.history != DatabaseHistory::Present
                    || result
                        .status
                        .migrations
                        .iter()
                        .any(|entry| entry.state != DatabaseMigrationState::Applied)))
        {
            return Err(protocol_error());
        }
        Ok(result)
    }

    pub fn render_human(&self) -> String {
        let mut lines = vec![format!(
            "Database {:?} {:?}: {:?}",
            self.provider, self.operation, self.status.history
        )];
        for entry in &self.status.migrations {
            lines.push(format!(
                "{} {} {:?}",
                entry.version, entry.module_id, entry.state
            ));
        }
        lines.join("\n")
    }
}

pub(super) fn protocol_error() -> OperationError {
    failure(
        OperationErrorKind::Internal,
        "database-protocol",
        "The application database command did not return a valid bounded schema-1 result; raw output is discarded.",
    )
}
