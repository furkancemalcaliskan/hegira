use application_contracts::permissions::PermissionName;
use serde_json::Value;

use crate::{
    identity::authorization::{AuthorizationService, CurrentUser, CurrentUserProvider},
    shared::audit::{AuditLogEntry, AuditLogger},
};

#[derive(Debug, Clone, Copy)]
pub struct CrudPermissions {
    pub read: PermissionName,
    pub create: PermissionName,
    pub update: PermissionName,
    pub delete: PermissionName,
}

#[derive(Debug, Clone, Copy)]
pub struct CrudPolicy {
    pub entity_type: &'static str,
    pub permissions: CrudPermissions,
    pub create_action: &'static str,
    pub update_action: &'static str,
    pub delete_action: &'static str,
}

#[derive(Debug, Clone)]
pub struct CrudAuditContext {
    pub actor: String,
    pub action: &'static str,
    pub entity_type: &'static str,
    pub entity_id: String,
    pub details: Value,
}

impl CrudAuditContext {
    pub fn into_entry(self) -> AuditLogEntry {
        AuditLogEntry::new(
            self.actor,
            self.action,
            self.entity_type,
            Some(self.entity_id),
            self.details,
        )
    }
}

#[derive(Debug, Clone)]
pub struct CrudExecution<CurrentUsers, Authorization> {
    current_users: CurrentUsers,
    authorization: Authorization,
}

impl<CurrentUsers, Authorization> CrudExecution<CurrentUsers, Authorization>
where
    CurrentUsers: CurrentUserProvider,
    Authorization: AuthorizationService,
{
    pub fn new(current_users: CurrentUsers, authorization: Authorization) -> Self {
        Self {
            current_users,
            authorization,
        }
    }

    pub async fn authorize(
        &self,
        actor_token: &str,
        permission: PermissionName,
    ) -> crate::shared::errors::ApplicationResult<CurrentUser> {
        let actor = self.current_users.current_user(actor_token).await?;
        self.authorization.require(&actor, permission).await?;
        Ok(actor)
    }
}

pub async fn record_standard_audit(
    audit: &impl AuditLogger,
    actor: String,
    action: &str,
    entity_type: &str,
    entity_id: Option<String>,
    details: Value,
) {
    let entry = AuditLogEntry::new(actor, action, entity_type, entity_id, details);
    if let Err(error) = audit.record(entry).await {
        tracing::debug!(%error, action, "audit log write skipped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        identity::authorization::{AuthorizationService, CurrentUser, CurrentUserProvider},
        shared::{errors::ApplicationResult, testing::RecordingAuditLogger},
    };

    #[derive(Clone)]
    struct TestCurrentUser;

    impl CurrentUserProvider for TestCurrentUser {
        async fn current_user(&self, _token: &str) -> ApplicationResult<CurrentUser> {
            Ok(CurrentUser {
                username: "operator@example.com".to_string(),
                is_authenticated: true,
            })
        }
    }

    #[derive(Clone)]
    struct ExpectedPermission(PermissionName);

    impl AuthorizationService for ExpectedPermission {
        async fn require(
            &self,
            _user: &CurrentUser,
            permission: PermissionName,
        ) -> ApplicationResult<()> {
            assert_eq!(permission, self.0);
            Ok(())
        }
    }

    #[tokio::test]
    async fn execution_authorizes_and_records_standard_audit_shape() {
        const CREATE_RECORD: PermissionName = PermissionName("Test.Records.Create");
        let audit = RecordingAuditLogger::default();
        let execution = CrudExecution::new(TestCurrentUser, ExpectedPermission(CREATE_RECORD));
        let actor = execution.authorize("token", CREATE_RECORD).await.unwrap();
        record_standard_audit(
            &audit,
            actor.username,
            "test.records.create",
            "test.record",
            Some("record-1".to_string()),
            serde_json::json!({ "source": "test" }),
        )
        .await;

        let entries = audit.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].actor, "operator@example.com");
        assert_eq!(entries[0].action, "test.records.create");
        assert_eq!(entries[0].entity_type, "test.record");
    }
}
