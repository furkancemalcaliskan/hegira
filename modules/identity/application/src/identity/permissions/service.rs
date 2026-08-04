use crate::{
    identity::{
        authorization::{AuthorizationService, CurrentUser, CurrentUserProvider},
        http_contracts::PermissionServiceContract,
        permissions::cache as permission_cache,
        validation,
    },
    identity_shared::is_protected_admin_role,
    shared::{
        audit::{AuditLogEntry, AuditLogger},
        cache::Cache,
        crud::CrudAuditContext,
        errors::{ApplicationError, ApplicationResult},
    },
};
use async_trait::async_trait;
use identity_application_contracts::{
    identity::{
        authorization::{
            AssignUserRoleInput, CreateRoleInput, ListRolesInput, PagedRoleResultDto,
            PermissionDto, RoleDto, SetRolePermissionsInput, UpdateRoleInput,
        },
        permissions as identity_permissions,
    },
    localization::IdentityMessage,
    permissions::{self, PermissionName},
};
use identity_domain::identity::authorization::AuthorizationRepository;

pub trait AuditedRoleWriter: Send + Sync {
    fn create_role_with_audit(
        &self,
        role_name: &str,
        audit: CrudAuditContext,
    ) -> impl Future<Output = ApplicationResult<()>> + Send;
}

#[derive(Debug, Clone)]
pub struct PermissionAppService<Repository, CurrentUsers, Authorization, CacheAdapter, Audit> {
    repository: Repository,
    current_users: CurrentUsers,
    authorization: Authorization,
    cache: CacheAdapter,
    audit: Audit,
}

impl<Repository, CurrentUsers, Authorization, CacheAdapter, Audit>
    PermissionAppService<Repository, CurrentUsers, Authorization, CacheAdapter, Audit>
where
    Repository: AuthorizationRepository + AuditedRoleWriter,
    CurrentUsers: CurrentUserProvider,
    Authorization: AuthorizationService,
    CacheAdapter: Cache,
    Audit: AuditLogger<Error = ApplicationError>,
{
    pub fn new(
        repository: Repository,
        current_users: CurrentUsers,
        authorization: Authorization,
        cache: CacheAdapter,
        audit: Audit,
    ) -> Self {
        Self {
            repository,
            current_users,
            authorization,
            cache,
            audit,
        }
    }

    pub async fn list_permissions(
        &self,
        actor_token: String,
    ) -> ApplicationResult<Vec<PermissionDto>> {
        self.require(&actor_token).await?;

        Ok(permissions::all()
            .map(|definition| PermissionDto {
                name: definition.name.0.to_string(),
                display_name: definition.display_name.default_text().to_string(),
            })
            .collect())
    }

    pub async fn list_roles(&self, actor_token: String) -> ApplicationResult<Vec<RoleDto>> {
        self.require(&actor_token).await?;
        self.roles_dto(self.repository.list_roles().await?).await
    }

    pub async fn list_roles_page(
        &self,
        actor_token: String,
        input: ListRolesInput,
    ) -> ApplicationResult<PagedRoleResultDto> {
        self.require(&actor_token).await?;

        let page = input.page.max(1);
        let page_size = input.page_size.clamp(1, 100);
        let (roles, total_count) = self
            .repository
            .list_roles_page(
                page,
                page_size,
                input.search,
                input.permission_status,
                input.sorting,
            )
            .await?;

        Ok(PagedRoleResultDto {
            items: self.roles_dto(roles).await?,
            total_count,
            page,
            page_size,
        })
    }

    pub async fn get_role(
        &self,
        actor_token: String,
        role_name: String,
    ) -> ApplicationResult<RoleDto> {
        self.require(&actor_token).await?;
        validate_role_name(&role_name)?;
        let role = self
            .repository
            .find_role(&role_name)
            .await?
            .ok_or_else(|| ApplicationError::localized_not_found(IdentityMessage::RoleNotFound))?;

        self.role_dto(role).await
    }

    pub async fn create_role(
        &self,
        actor_token: String,
        input: CreateRoleInput,
    ) -> ApplicationResult<()> {
        let actor = self.require(&actor_token).await?;
        validate_role_name(&input.name)?;
        self.repository
            .create_role_with_audit(
                &input.name,
                CrudAuditContext {
                    actor: actor.username,
                    action: "identity.roles.create",
                    entity_type: "identity.role",
                    entity_id: input.name.clone(),
                    details: serde_json::json!({}),
                },
            )
            .await?;
        self.invalidate_authorization_cache().await;
        Ok(())
    }

    pub async fn update_role(
        &self,
        actor_token: String,
        input: UpdateRoleInput,
    ) -> ApplicationResult<()> {
        self.require(&actor_token).await?;
        validate_role_name(&input.name)?;
        validate_role_name(&input.new_name)?;

        if is_protected_admin_role(&input.name) {
            return Err(ApplicationError::localized_forbidden(
                IdentityMessage::ProtectedAdminRoleCannotBeDeleted,
            ));
        }

        if !self
            .repository
            .update_role(&input.name, &input.new_name)
            .await?
        {
            return Err(ApplicationError::localized_not_found(
                IdentityMessage::RoleNotFound,
            ));
        }

        self.invalidate_authorization_cache().await;
        self.record_audit(
            actor_token.as_str(),
            "identity.roles.update",
            "identity.role",
            Some(input.new_name),
            serde_json::json!({ "previous_name": input.name }),
        )
        .await;
        Ok(())
    }

    pub async fn delete_role(
        &self,
        actor_token: String,
        role_name: String,
    ) -> ApplicationResult<()> {
        self.require(&actor_token).await?;
        validate_role_name(&role_name)?;

        if is_protected_admin_role(&role_name) {
            return Err(ApplicationError::localized_forbidden(
                IdentityMessage::ProtectedAdminRoleCannotBeDeleted,
            ));
        }

        if !self.repository.delete_role(&role_name).await? {
            return Err(ApplicationError::localized_not_found(
                IdentityMessage::RoleNotFound,
            ));
        }

        self.invalidate_authorization_cache().await;
        self.record_audit(
            actor_token.as_str(),
            "identity.roles.delete",
            "identity.role",
            Some(role_name),
            serde_json::json!({}),
        )
        .await;
        Ok(())
    }

    pub async fn role_permissions(
        &self,
        actor_token: String,
        role_name: String,
    ) -> ApplicationResult<Vec<String>> {
        self.require(&actor_token).await?;
        validate_role_name(&role_name)?;

        Ok(self
            .repository
            .role_permissions(&role_name)
            .await?
            .into_iter()
            .map(|permission| permission.0.to_string())
            .collect())
    }

    pub async fn set_role_permissions(
        &self,
        actor_token: String,
        input: SetRolePermissionsInput,
    ) -> ApplicationResult<()> {
        self.require(&actor_token).await?;
        validate_role_name(&input.role_name)?;

        let permissions = input
            .permissions
            .iter()
            .map(|permission| parse_permission(permission))
            .collect::<ApplicationResult<Vec<_>>>()?;

        self.repository
            .set_role_permissions(&input.role_name, permissions)
            .await?;
        self.invalidate_authorization_cache().await;
        self.record_audit(
            actor_token.as_str(),
            "identity.roles.set_permissions",
            "identity.role",
            Some(input.role_name),
            serde_json::json!({ "permissions": input.permissions }),
        )
        .await;
        Ok(())
    }

    pub async fn assign_user_role(
        &self,
        actor_token: String,
        input: AssignUserRoleInput,
    ) -> ApplicationResult<()> {
        self.require(&actor_token).await?;
        validation::required_username(&input.username)?;
        validate_role_name(&input.role_name)?;
        self.repository
            .assign_role(&input.username, &input.role_name)
            .await?;
        self.invalidate_authorization_cache().await;
        self.record_audit(
            actor_token.as_str(),
            "identity.users.assign_role",
            "identity.user",
            Some(input.username),
            serde_json::json!({ "role": input.role_name }),
        )
        .await;
        Ok(())
    }

    async fn require(&self, actor_token: &str) -> ApplicationResult<CurrentUser> {
        let current_user = self.current_users.current_user(actor_token).await?;
        self.authorization
            .require(&current_user, identity_permissions::AUTHORIZATION)
            .await?;
        Ok(current_user)
    }

    async fn roles_dto(
        &self,
        roles: Vec<identity_domain::identity::authorization::Role>,
    ) -> ApplicationResult<Vec<RoleDto>> {
        let mut result = Vec::with_capacity(roles.len());
        for role in roles {
            result.push(self.role_dto(role).await?);
        }
        Ok(result)
    }

    async fn role_dto(
        &self,
        role: identity_domain::identity::authorization::Role,
    ) -> ApplicationResult<RoleDto> {
        let permissions = self
            .repository
            .role_permissions(&role.name)
            .await?
            .into_iter()
            .map(|permission| permission.0.to_string())
            .collect();

        Ok(RoleDto {
            name: role.name,
            permissions,
        })
    }

    async fn invalidate_authorization_cache(&self) {
        permission_cache::invalidate(&self.cache).await;
    }

    async fn record_audit(
        &self,
        actor_token: &str,
        action: &str,
        entity_type: &str,
        entity_id: Option<String>,
        details: serde_json::Value,
    ) {
        let actor = self
            .current_users
            .current_user(actor_token)
            .await
            .map(|user| user.username)
            .unwrap_or_else(|_| "unknown".to_string());

        let entry = AuditLogEntry::new(actor, action, entity_type, entity_id, details);
        if let Err(error) = self.audit.record(entry).await {
            tracing::debug!(%error, action, "audit log write skipped");
        }
    }
}

#[async_trait]
impl<Repository, CurrentUsers, Authorization, CacheAdapter, Audit> PermissionServiceContract
    for PermissionAppService<Repository, CurrentUsers, Authorization, CacheAdapter, Audit>
where
    Repository: AuthorizationRepository + AuditedRoleWriter,
    CurrentUsers: CurrentUserProvider,
    Authorization: AuthorizationService,
    CacheAdapter: Cache,
    Audit: AuditLogger<Error = ApplicationError>,
{
    async fn list_permissions(&self, actor_token: String) -> ApplicationResult<Vec<PermissionDto>> {
        PermissionAppService::list_permissions(self, actor_token).await
    }

    async fn list_roles(&self, actor_token: String) -> ApplicationResult<Vec<RoleDto>> {
        PermissionAppService::list_roles(self, actor_token).await
    }

    async fn list_roles_page(
        &self,
        actor_token: String,
        input: ListRolesInput,
    ) -> ApplicationResult<PagedRoleResultDto> {
        PermissionAppService::list_roles_page(self, actor_token, input).await
    }

    async fn get_role(&self, actor_token: String, role_name: String) -> ApplicationResult<RoleDto> {
        PermissionAppService::get_role(self, actor_token, role_name).await
    }

    async fn create_role(
        &self,
        actor_token: String,
        input: CreateRoleInput,
    ) -> ApplicationResult<()> {
        PermissionAppService::create_role(self, actor_token, input).await
    }

    async fn update_role(
        &self,
        actor_token: String,
        input: UpdateRoleInput,
    ) -> ApplicationResult<()> {
        PermissionAppService::update_role(self, actor_token, input).await
    }

    async fn delete_role(&self, actor_token: String, role_name: String) -> ApplicationResult<()> {
        PermissionAppService::delete_role(self, actor_token, role_name).await
    }

    async fn set_role_permissions(
        &self,
        actor_token: String,
        input: SetRolePermissionsInput,
    ) -> ApplicationResult<()> {
        PermissionAppService::set_role_permissions(self, actor_token, input).await
    }

    async fn assign_user_role(
        &self,
        actor_token: String,
        input: AssignUserRoleInput,
    ) -> ApplicationResult<()> {
        PermissionAppService::assign_user_role(self, actor_token, input).await
    }
}

fn validate_role_name(role_name: &str) -> ApplicationResult<()> {
    let role_name = role_name.trim();

    if role_name.is_empty() {
        return Err(ApplicationError::localized_validation(
            IdentityMessage::RoleNameRequired,
        ));
    }

    Ok(())
}

fn parse_permission(permission: &str) -> ApplicationResult<PermissionName> {
    permissions::from_name(permission)
        .ok_or_else(|| ApplicationError::Validation(format!("Unknown permission: {permission}")))
}
