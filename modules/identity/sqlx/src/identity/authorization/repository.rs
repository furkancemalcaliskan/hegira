use crate::identity::repository::SqlxIdentityRepository;
use application::{
    identity::permissions::service::AuditedRoleWriter,
    shared::{
        crud::CrudAuditContext,
        errors::{ApplicationError, ApplicationResult},
    },
};
use application_contracts::permissions::{self, PermissionName};
use domain::identity::authorization::{AuthorizationRepository, Role};
use domain_shared::common::errors::DomainError;

impl AuditedRoleWriter for SqlxIdentityRepository {
    async fn create_role_with_audit(
        &self,
        role_name: &str,
        audit: CrudAuditContext,
    ) -> ApplicationResult<()> {
        let mut transaction = self.pool.begin().await.map_err(app_error)?;
        sqlx::query("INSERT INTO roles (name, deleted_at) VALUES ($1, NULL) ON CONFLICT (name) DO UPDATE SET deleted_at = NULL")
            .bind(role_name).execute(&mut *transaction).await.map_err(app_error)?;
        crate::identity::audit::insert_postgres_transaction(&mut transaction, audit.into_entry())
            .await?;
        transaction.commit().await.map_err(app_error)
    }
}

fn app_error(error: sqlx::Error) -> ApplicationError {
    ApplicationError::Infrastructure(error.to_string())
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod audited_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires DATABASE_URL and resets the test database"]
    async fn postgres_role_creation_and_audit_are_atomic() {
        let pool = crate::testing::reset_database_from_env().await.unwrap();
        let repository = SqlxIdentityRepository::new(pool.clone());
        repository
            .create_role_with_audit(
                "auditor",
                CrudAuditContext {
                    actor: "admin@example.com".to_string(),
                    action: "identity.roles.create",
                    entity_type: "identity.role",
                    entity_id: "auditor".to_string(),
                    details: serde_json::json!({}),
                },
            )
            .await
            .unwrap();
        let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM audit_logs WHERE action = 'identity.roles.create' AND entity_id = 'auditor'").fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1);
    }
}

impl AuthorizationRepository for SqlxIdentityRepository {
    async fn user_has_permission(
        &self,
        username: &str,
        permission: PermissionName,
    ) -> Result<bool, DomainError> {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1
                FROM user_roles ur
                INNER JOIN role_permissions rp ON rp.role_name = ur.role_name
                INNER JOIN users u ON u.id = ur.user_id
                INNER JOIN roles r ON r.name = ur.role_name
                WHERE u.username = $1
                  AND u.deleted_at IS NULL
                  AND r.deleted_at IS NULL
                  AND rp.permission_name = $2
            )",
        )
        .bind(username)
        .bind(permission.0)
        .fetch_one(&self.pool)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn user_permissions(&self, username: &str) -> Result<Vec<PermissionName>, DomainError> {
        let names = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT rp.permission_name
             FROM user_roles ur
             INNER JOIN role_permissions rp ON rp.role_name = ur.role_name
             INNER JOIN users u ON u.id = ur.user_id
             INNER JOIN roles r ON r.name = ur.role_name
             WHERE u.username = $1
               AND u.deleted_at IS NULL
               AND r.deleted_at IS NULL
             ORDER BY rp.permission_name",
        )
        .bind(username)
        .fetch_all(&self.pool)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))?;

        Ok(names
            .into_iter()
            .filter_map(|name| permissions::from_name(&name))
            .collect())
    }

    async fn assign_role(&self, username: &str, role_name: &str) -> Result<(), DomainError> {
        let user_id = self.user_id_by_username(username).await?;

        sqlx::query(
            "INSERT INTO user_roles (user_id, role_name)
             VALUES ($1, $2)
             ON CONFLICT (user_id, role_name) DO NOTHING",
        )
        .bind(user_id)
        .bind(role_name)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn list_roles(&self) -> Result<Vec<Role>, DomainError> {
        sqlx::query_as::<_, (String, chrono::DateTime<chrono::Utc>)>(
            "SELECT name, created_at FROM roles WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|(name, created_at)| Role { name, created_at })
                .collect()
        })
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn list_roles_page(
        &self,
        page: u32,
        page_size: u32,
        search: Option<String>,
        permission_status: Option<String>,
        sorting: Option<String>,
    ) -> Result<(Vec<Role>, i64), DomainError> {
        let search = search.unwrap_or_default();
        let permission_status = permission_status.unwrap_or_else(|| "all".to_string());
        let sorting = sorting.unwrap_or_else(|| "name asc".to_string());
        let offset = i64::from(page.saturating_sub(1)) * i64::from(page_size);
        let limit = i64::from(page_size);

        let total_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)
             FROM roles r
             WHERE r.deleted_at IS NULL
               AND ($1 = '' OR r.name ILIKE '%' || $1 || '%')
               AND (
                   $2 = 'all'
                   OR ($2 = 'with-permissions' AND EXISTS (
                       SELECT 1 FROM role_permissions rp WHERE rp.role_name = r.name
                   ))
                   OR ($2 = 'without-permissions' AND NOT EXISTS (
                       SELECT 1 FROM role_permissions rp WHERE rp.role_name = r.name
                   ))
               )",
        )
        .bind(&search)
        .bind(&permission_status)
        .fetch_one(&self.pool)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))?;

        let rows = sqlx::query_as::<_, (String, chrono::DateTime<chrono::Utc>)>(
            "SELECT r.name, r.created_at
             FROM roles r
             WHERE r.deleted_at IS NULL
               AND ($1 = '' OR r.name ILIKE '%' || $1 || '%')
               AND (
                   $2 = 'all'
                   OR ($2 = 'with-permissions' AND EXISTS (
                       SELECT 1 FROM role_permissions rp WHERE rp.role_name = r.name
                   ))
                   OR ($2 = 'without-permissions' AND NOT EXISTS (
                       SELECT 1 FROM role_permissions rp WHERE rp.role_name = r.name
                   ))
               )
             ORDER BY
               CASE WHEN $3 = 'name desc' THEN r.name END DESC,
               CASE WHEN $3 = 'created_at asc' THEN r.created_at END ASC,
               CASE WHEN $3 = 'created_at desc' THEN r.created_at END DESC,
               r.name ASC
             LIMIT $4 OFFSET $5",
        )
        .bind(search)
        .bind(permission_status)
        .bind(sorting)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))?;

        Ok((
            rows.into_iter()
                .map(|(name, created_at)| Role { name, created_at })
                .collect(),
            total_count,
        ))
    }

    async fn find_role(&self, role_name: &str) -> Result<Option<Role>, DomainError> {
        sqlx::query_as::<_, (String, chrono::DateTime<chrono::Utc>)>(
            "SELECT name, created_at
             FROM roles
             WHERE name = $1 AND deleted_at IS NULL",
        )
        .bind(role_name)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(|(name, created_at)| Role { name, created_at }))
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn create_role(&self, role_name: &str) -> Result<(), DomainError> {
        sqlx::query(
            "INSERT INTO roles (name, deleted_at)
             VALUES ($1, NULL)
             ON CONFLICT (name) DO UPDATE SET deleted_at = NULL",
        )
        .bind(role_name)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn update_role(&self, role_name: &str, new_role_name: &str) -> Result<bool, DomainError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|err| DomainError::Validation(err.to_string()))?;

        let Some(created_at) = sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
            "SELECT created_at FROM roles WHERE name = $1 AND deleted_at IS NULL",
        )
        .bind(role_name)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))?
        else {
            return Ok(false);
        };

        sqlx::query(
            "INSERT INTO roles (name, created_at, deleted_at)
             VALUES ($1, $2, NULL)
             ON CONFLICT (name) DO UPDATE SET deleted_at = NULL",
        )
        .bind(new_role_name)
        .bind(created_at)
        .execute(&mut *transaction)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))?;

        sqlx::query(
            "INSERT INTO role_permissions (role_name, permission_name, created_at)
             SELECT $1, permission_name, created_at
             FROM role_permissions
             WHERE role_name = $2
             ON CONFLICT (role_name, permission_name) DO NOTHING",
        )
        .bind(new_role_name)
        .bind(role_name)
        .execute(&mut *transaction)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))?;

        sqlx::query(
            "INSERT INTO user_roles (user_id, role_name, created_at)
             SELECT user_id, $1, created_at
             FROM user_roles
             WHERE role_name = $2
             ON CONFLICT (user_id, role_name) DO NOTHING",
        )
        .bind(new_role_name)
        .bind(role_name)
        .execute(&mut *transaction)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))?;

        sqlx::query("UPDATE roles SET deleted_at = NOW() WHERE name = $1 AND deleted_at IS NULL")
            .bind(role_name)
            .execute(&mut *transaction)
            .await
            .map_err(|err| DomainError::Validation(err.to_string()))?;

        transaction
            .commit()
            .await
            .map_err(|err| DomainError::Validation(err.to_string()))?;

        Ok(true)
    }

    async fn delete_role(&self, role_name: &str) -> Result<bool, DomainError> {
        sqlx::query("UPDATE roles SET deleted_at = NOW() WHERE name = $1 AND deleted_at IS NULL")
            .bind(role_name)
            .execute(&self.pool)
            .await
            .map(|result| result.rows_affected() > 0)
            .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn role_permissions(&self, role_name: &str) -> Result<Vec<PermissionName>, DomainError> {
        let names = sqlx::query_scalar::<_, String>(
            "SELECT permission_name
             FROM role_permissions
             INNER JOIN roles r ON r.name = role_permissions.role_name
             WHERE role_name = $1 AND r.deleted_at IS NULL
             ORDER BY permission_name",
        )
        .bind(role_name)
        .fetch_all(&self.pool)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))?;

        Ok(names
            .into_iter()
            .filter_map(|name| permissions::from_name(&name))
            .collect())
    }

    async fn set_role_permissions(
        &self,
        role_name: &str,
        role_permissions: Vec<PermissionName>,
    ) -> Result<(), DomainError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|err| DomainError::Validation(err.to_string()))?;

        sqlx::query(
            "INSERT INTO roles (name, deleted_at)
             VALUES ($1, NULL)
             ON CONFLICT (name) DO UPDATE SET deleted_at = NULL",
        )
        .bind(role_name)
        .execute(&mut *transaction)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))?;

        sqlx::query("DELETE FROM role_permissions WHERE role_name = $1")
            .bind(role_name)
            .execute(&mut *transaction)
            .await
            .map_err(|err| DomainError::Validation(err.to_string()))?;

        for permission in role_permissions {
            sqlx::query("INSERT INTO permissions (name) VALUES ($1) ON CONFLICT (name) DO NOTHING")
                .bind(permission.0)
                .execute(&mut *transaction)
                .await
                .map_err(|err| DomainError::Validation(err.to_string()))?;

            sqlx::query(
                "INSERT INTO role_permissions (role_name, permission_name)
                 VALUES ($1, $2)
                 ON CONFLICT (role_name, permission_name) DO NOTHING",
            )
            .bind(role_name)
            .bind(permission.0)
            .execute(&mut *transaction)
            .await
            .map_err(|err| DomainError::Validation(err.to_string()))?;
        }

        transaction
            .commit()
            .await
            .map_err(|err| DomainError::Validation(err.to_string()))
    }

    async fn ensure_identity_seed_data(&self) -> Result<(), DomainError> {
        sqlx::query(
            "INSERT INTO roles (name, deleted_at)
             VALUES ('admin', NULL)
             ON CONFLICT (name) DO UPDATE SET deleted_at = NULL",
        )
        .execute(&self.pool)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))?;

        for permission in permissions::all_names() {
            sqlx::query("INSERT INTO permissions (name) VALUES ($1) ON CONFLICT (name) DO NOTHING")
                .bind(permission.0)
                .execute(&self.pool)
                .await
                .map_err(|err| DomainError::Validation(err.to_string()))?;

            sqlx::query(
                "INSERT INTO role_permissions (role_name, permission_name)
                 VALUES ('admin', $1)
                 ON CONFLICT (role_name, permission_name) DO NOTHING",
            )
            .bind(permission.0)
            .execute(&self.pool)
            .await
            .map_err(|err| DomainError::Validation(err.to_string()))?;
        }

        let registry_names = permissions::all_names()
            .map(|permission| permission.0.to_string())
            .collect::<Vec<_>>();

        sqlx::query(
            "DELETE FROM role_permissions
             WHERE role_name = 'admin'
             AND NOT (permission_name = ANY($1))",
        )
        .bind(registry_names)
        .execute(&self.pool)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))?;

        Ok(())
    }
}
