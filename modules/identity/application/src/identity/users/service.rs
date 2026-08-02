use chrono::Utc;

use crate::{
    identity::authorization::{AuthorizationService, CurrentUserProvider, require_permission},
    identity::permissions::cache as permission_cache,
    identity::users::mapper::user_dto,
    identity::users::writer::{CreateManagedUser, ManagedUserWriter, UpdateManagedUser},
    identity::validation,
    identity_shared::{self as identity, DEFAULT_ADMIN_ROLE_NAME},
    shared::{
        audit::AuditLogger,
        cache::Cache,
        errors::{ApplicationError, ApplicationResult},
        search::{SearchIndex, SearchQuery},
        security::PasswordHasher,
    },
};
use identity_application_contracts::{
    identity::permissions,
    identity::users::{
        CreateUserInput, ListUsersInput, PagedUserResultDto, UpdateUserInput, UserDto,
    },
    localization::IdentityMessage,
};
use identity_domain::identity::users::UserRepository;

#[derive(Debug, Clone)]
pub struct UserAppService<Users, Hasher, CurrentUsers, Authorization, CacheAdapter, Audit, Search> {
    users: Users,
    password_hasher: Hasher,
    current_users: CurrentUsers,
    authorization: Authorization,
    cache: CacheAdapter,
    audit: Audit,
    search: UserSearch<Search>,
}

#[derive(Debug, Clone)]
pub struct UserSearch<Search> {
    pub adapter: Search,
    pub enabled: bool,
    pub publish_mutations: bool,
}

impl<Users, Hasher, CurrentUsers, Authorization, CacheAdapter, Audit, Search>
    UserAppService<Users, Hasher, CurrentUsers, Authorization, CacheAdapter, Audit, Search>
where
    Users: UserRepository + ManagedUserWriter,
    Hasher: PasswordHasher<Error = ApplicationError>,
    CurrentUsers: CurrentUserProvider,
    Authorization: AuthorizationService,
    CacheAdapter: Cache<Error = ApplicationError>,
    Audit: AuditLogger<Error = ApplicationError>,
    Search: SearchIndex<Error = ApplicationError>,
{
    pub fn new(
        users: Users,
        password_hasher: Hasher,
        current_users: CurrentUsers,
        authorization: Authorization,
        cache: CacheAdapter,
        audit: Audit,
        search: UserSearch<Search>,
    ) -> Self {
        Self {
            users,
            password_hasher,
            current_users,
            authorization,
            cache,
            audit,
            search,
        }
    }

    pub async fn list(
        &self,
        actor_token: String,
        input: ListUsersInput,
    ) -> ApplicationResult<PagedUserResultDto> {
        require_permission(
            &self.current_users,
            &self.authorization,
            &actor_token,
            permissions::USERS,
        )
        .await?;

        let page = input.page.max(1);
        let page_size = input.page_size.clamp(1, 100);
        let (items, total_count) = if self.search.enabled
            && input.sorting.is_none()
            && input
                .search
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
        {
            match self
                .search_users(page, page_size, input.search.as_deref().unwrap())
                .await
            {
                Ok(result) => result,
                Err(error) => {
                    tracing::warn!(%error, "search query failed; using PostgreSQL fallback");
                    self.users
                        .list(page, page_size, input.search, input.sorting)
                        .await?
                }
            }
        } else {
            self.users
                .list(page, page_size, input.search, input.sorting)
                .await?
        };

        Ok(PagedUserResultDto {
            items: self.users_dto(items).await?,
            total_count,
            page,
            page_size,
        })
    }

    pub async fn get(&self, actor_token: String, username: String) -> ApplicationResult<UserDto> {
        require_permission(
            &self.current_users,
            &self.authorization,
            &actor_token,
            permissions::USERS,
        )
        .await?;

        let user = self
            .users
            .find_by_username(&username)
            .await?
            .ok_or_else(|| ApplicationError::localized_not_found(IdentityMessage::UserNotFound))?;

        let roles = self.users.user_roles(&user.username).await?;
        Ok(user_dto(user, roles))
    }

    pub async fn create(
        &self,
        actor_token: String,
        input: CreateUserInput,
    ) -> ApplicationResult<UserDto> {
        require_permission(
            &self.current_users,
            &self.authorization,
            &actor_token,
            permissions::USERS_CREATE,
        )
        .await?;

        validation::required_username_password(&input.username, &input.password)?;

        if self.users.exists(&input.username).await? {
            return Err(ApplicationError::localized_conflict(
                IdentityMessage::UserAlreadyExists,
            ));
        }

        let roles = normalized_roles(&input.username, input.roles);
        self.users
            .create_managed_user(CreateManagedUser {
                username: input.username.clone(),
                password_hash: self.password_hasher.hash(&input.password)?,
                email_verified_at: input.is_verified.then(Utc::now),
                roles: roles.clone(),
                publish_search: self.search.publish_mutations,
            })
            .await
            .map_err(localize_user_conflict)?;
        self.invalidate_authorization_cache().await;
        self.record_audit(
            actor_token.as_str(),
            "identity.users.create",
            Some(input.username.clone()),
            serde_json::json!({
                "is_verified": input.is_verified,
                "roles": roles,
            }),
        )
        .await;

        self.get(actor_token, input.username).await
    }

    pub async fn update(
        &self,
        actor_token: String,
        input: UpdateUserInput,
    ) -> ApplicationResult<UserDto> {
        require_permission(
            &self.current_users,
            &self.authorization,
            &actor_token,
            permissions::USERS_UPDATE,
        )
        .await?;

        validation::required_username(&input.username)?;

        if !self.users.exists(&input.username).await? {
            return Err(ApplicationError::localized_not_found(
                IdentityMessage::UserNotFound,
            ));
        }

        let password_changed = input
            .password
            .as_deref()
            .is_some_and(|password| !password.is_empty());
        let password_hash = match validation::optional_password(input.password.as_deref())? {
            Some(password) => Some(self.password_hasher.hash(password)?),
            None => None,
        };
        let verified_at = input.is_verified.then(Utc::now);

        let roles = normalized_roles(&input.username, input.roles);
        if self
            .users
            .update_managed_user(UpdateManagedUser {
                username: input.username.clone(),
                password_hash,
                email_verified_at: verified_at,
                roles: roles.clone(),
                publish_search: self.search.publish_mutations,
            })
            .await?
            .is_none()
        {
            return Err(ApplicationError::localized_not_found(
                IdentityMessage::UserNotFound,
            ));
        }
        self.invalidate_authorization_cache().await;
        self.record_audit(
            actor_token.as_str(),
            "identity.users.update",
            Some(input.username.clone()),
            serde_json::json!({
                "is_verified": input.is_verified,
                "roles": roles,
                "password_changed": password_changed,
            }),
        )
        .await;

        self.get(actor_token, input.username).await
    }

    pub async fn delete(&self, actor_token: String, username: String) -> ApplicationResult<()> {
        require_permission(
            &self.current_users,
            &self.authorization,
            &actor_token,
            permissions::USERS_DELETE,
        )
        .await?;

        if identity::is_protected_admin_username(&username) {
            return Err(ApplicationError::localized_forbidden(
                IdentityMessage::ProtectedAdminCannotBeDeleted,
            ));
        }

        if !self
            .users
            .delete_managed_user(&username, self.search.publish_mutations)
            .await?
        {
            return Err(ApplicationError::localized_not_found(
                IdentityMessage::UserNotFound,
            ));
        }

        self.invalidate_authorization_cache().await;
        self.record_audit(
            actor_token.as_str(),
            "identity.users.delete",
            Some(username),
            serde_json::json!({}),
        )
        .await;
        Ok(())
    }

    async fn users_dto(
        &self,
        users: Vec<identity_domain::identity::users::User>,
    ) -> ApplicationResult<Vec<UserDto>> {
        let mut items = Vec::with_capacity(users.len());

        for user in users {
            let roles = self.users.user_roles(&user.username).await?;
            items.push(user_dto(user, roles));
        }

        Ok(items)
    }

    async fn search_users(
        &self,
        page: u32,
        page_size: u32,
        text: &str,
    ) -> ApplicationResult<(Vec<identity_domain::identity::users::User>, i64)> {
        let result = self
            .search
            .adapter
            .search(
                "identity_users",
                SearchQuery {
                    text: text.trim().to_string(),
                    offset: (page.saturating_sub(1) as usize).saturating_mul(page_size as usize),
                    limit: page_size as usize,
                },
            )
            .await?;
        let pids = result
            .hits
            .iter()
            .filter_map(|hit| hit.get("id").and_then(serde_json::Value::as_str))
            .filter_map(|pid| uuid::Uuid::parse_str(pid).ok())
            .collect::<Vec<_>>();
        let users = self.users.find_by_pids(&pids).await?;
        Ok((
            users,
            result.estimated_total_hits.min(i64::MAX as usize) as i64,
        ))
    }

    async fn invalidate_authorization_cache(&self) {
        permission_cache::invalidate(&self.cache).await;
    }

    async fn record_audit(
        &self,
        actor_token: &str,
        action: &str,
        entity_id: Option<String>,
        details: serde_json::Value,
    ) {
        let actor = self
            .current_users
            .current_user(actor_token)
            .await
            .map(|user| user.username)
            .unwrap_or_else(|_| "unknown".to_string());

        crate::shared::crud::record_standard_audit(
            &self.audit,
            actor,
            action,
            "identity.user",
            entity_id,
            details,
        )
        .await;
    }
}

fn normalized_roles(username: &str, roles: Vec<String>) -> Vec<String> {
    let mut roles = roles
        .into_iter()
        .map(|role| role.trim().to_string())
        .filter(|role| !role.is_empty())
        .fold(Vec::<String>::new(), |mut roles, role| {
            if !roles.iter().any(|current| current == &role) {
                roles.push(role);
            }
            roles
        });

    if identity::is_protected_admin_username(username)
        && !roles
            .iter()
            .any(|role| role.eq_ignore_ascii_case(DEFAULT_ADMIN_ROLE_NAME))
    {
        roles.push(DEFAULT_ADMIN_ROLE_NAME.to_string());
    }

    roles
}

fn localize_user_conflict(error: ApplicationError) -> ApplicationError {
    if matches!(error, ApplicationError::Conflict(_)) {
        ApplicationError::localized_conflict(IdentityMessage::UserAlreadyExists)
    } else {
        error
    }
}
