use application::{
    identity::permissions::service::AuditedRoleWriter,
    shared::{
        crud::CrudAuditContext,
        errors::{ApplicationError, ApplicationResult},
    },
};
use application_contracts::permissions::{self, PermissionName};
use chrono::{DateTime, Utc};
use domain::identity::authorization::{AuthorizationRepository, Role};
use domain_shared::common::errors::DomainError;
use sqlx::{QueryBuilder, Sqlite, SqlitePool};

#[derive(Debug, Clone)]
pub struct SqliteAuthorizationRepository {
    pool: SqlitePool,
}

impl SqliteAuthorizationRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn user_id_by_username(&self, username: &str) -> Result<i32, DomainError> {
        sqlx::query_scalar("SELECT id FROM users WHERE username = ?1 AND deleted_at IS NULL")
            .bind(username)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?
            .ok_or_else(|| DomainError::NotFound("User not found".to_string()))
    }
}

impl AuditedRoleWriter for SqliteAuthorizationRepository {
    async fn create_role_with_audit(
        &self,
        role_name: &str,
        audit: CrudAuditContext,
    ) -> ApplicationResult<()> {
        let mut transaction = self.pool.begin().await.map_err(app_error)?;
        sqlx::query("INSERT INTO roles (name, created_at, deleted_at) VALUES (?1, ?2, NULL) ON CONFLICT (name) DO UPDATE SET deleted_at = NULL")
            .bind(role_name).bind(Utc::now()).execute(&mut *transaction).await.map_err(app_error)?;
        crate::identity::audit::insert_sqlite_transaction(&mut transaction, audit.into_entry())
            .await?;
        transaction.commit().await.map_err(app_error)
    }
}

fn app_error(error: sqlx::Error) -> ApplicationError {
    ApplicationError::Infrastructure(error.to_string())
}

fn db_error(error: sqlx::Error) -> DomainError {
    DomainError::Validation(error.to_string())
}

fn roles(rows: Vec<(String, DateTime<Utc>)>) -> Vec<Role> {
    rows.into_iter()
        .map(|(name, created_at)| Role { name, created_at })
        .collect()
}

impl AuthorizationRepository for SqliteAuthorizationRepository {
    async fn user_has_permission(
        &self,
        username: &str,
        permission: PermissionName,
    ) -> Result<bool, DomainError> {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1 FROM user_roles ur
                INNER JOIN role_permissions rp ON rp.role_name = ur.role_name
                INNER JOIN users u ON u.id = ur.user_id
                INNER JOIN roles r ON r.name = ur.role_name
                WHERE u.username = ?1 AND u.deleted_at IS NULL AND r.deleted_at IS NULL
                  AND rp.permission_name = ?2
            )",
        )
        .bind(username)
        .bind(permission.0)
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)
    }

    async fn user_permissions(&self, username: &str) -> Result<Vec<PermissionName>, DomainError> {
        let names = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT rp.permission_name FROM user_roles ur
             INNER JOIN role_permissions rp ON rp.role_name = ur.role_name
             INNER JOIN users u ON u.id = ur.user_id
             INNER JOIN roles r ON r.name = ur.role_name
             WHERE u.username = ?1 AND u.deleted_at IS NULL AND r.deleted_at IS NULL
             ORDER BY rp.permission_name",
        )
        .bind(username)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(names
            .into_iter()
            .filter_map(|name| permissions::from_name(&name))
            .collect())
    }

    async fn assign_role(&self, username: &str, role_name: &str) -> Result<(), DomainError> {
        let user_id = self.user_id_by_username(username).await?;
        sqlx::query(
            "INSERT INTO user_roles (user_id, role_name) VALUES (?1, ?2)
             ON CONFLICT (user_id, role_name) DO NOTHING",
        )
        .bind(user_id)
        .bind(role_name)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(db_error)
    }

    async fn list_roles(&self) -> Result<Vec<Role>, DomainError> {
        sqlx::query_as("SELECT name, created_at FROM roles WHERE deleted_at IS NULL ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map(roles)
            .map_err(db_error)
    }

    async fn list_roles_page(
        &self,
        page: u32,
        page_size: u32,
        search: Option<String>,
        permission_status: Option<String>,
        sorting: Option<String>,
    ) -> Result<(Vec<Role>, i64), DomainError> {
        let search = search.unwrap_or_default().to_lowercase();
        let permission_status = permission_status.unwrap_or_else(|| "all".to_string());
        let order_by = match sorting.as_deref().unwrap_or("name asc") {
            "name desc" => "r.name DESC",
            "created_at asc" => "r.created_at ASC, r.name ASC",
            "created_at desc" => "r.created_at DESC, r.name ASC",
            _ => "r.name ASC",
        };
        let offset = i64::from(page.saturating_sub(1)) * i64::from(page_size);
        let limit = i64::from(page_size);
        let predicate = "r.deleted_at IS NULL
             AND (?1 = '' OR LOWER(r.name) LIKE '%' || ?1 || '%')
             AND (
                 ?2 = 'all'
                 OR (?2 = 'with-permissions' AND EXISTS (
                     SELECT 1 FROM role_permissions rp WHERE rp.role_name = r.name
                 ))
                 OR (?2 = 'without-permissions' AND NOT EXISTS (
                     SELECT 1 FROM role_permissions rp WHERE rp.role_name = r.name
                 ))
             )";
        let total_count =
            sqlx::query_scalar(&format!("SELECT COUNT(*) FROM roles r WHERE {predicate}"))
                .bind(&search)
                .bind(&permission_status)
                .fetch_one(&self.pool)
                .await
                .map_err(db_error)?;
        let rows = sqlx::query_as(&format!(
            "SELECT r.name, r.created_at FROM roles r WHERE {predicate}
             ORDER BY {order_by} LIMIT ?3 OFFSET ?4"
        ))
        .bind(search)
        .bind(permission_status)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        Ok((roles(rows), total_count))
    }

    async fn find_role(&self, role_name: &str) -> Result<Option<Role>, DomainError> {
        sqlx::query_as::<_, (String, DateTime<Utc>)>(
            "SELECT name, created_at FROM roles WHERE name = ?1 AND deleted_at IS NULL",
        )
        .bind(role_name)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(|(name, created_at)| Role { name, created_at }))
        .map_err(db_error)
    }

    async fn create_role(&self, role_name: &str) -> Result<(), DomainError> {
        sqlx::query(
            "INSERT INTO roles (name, created_at, deleted_at) VALUES (?1, ?2, NULL)
             ON CONFLICT (name) DO UPDATE SET deleted_at = NULL",
        )
        .bind(role_name)
        .bind(Utc::now())
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(db_error)
    }

    async fn update_role(&self, role_name: &str, new_role_name: &str) -> Result<bool, DomainError> {
        let mut transaction = self.pool.begin().await.map_err(db_error)?;
        let Some(created_at) = sqlx::query_scalar::<_, DateTime<Utc>>(
            "SELECT created_at FROM roles WHERE name = ?1 AND deleted_at IS NULL",
        )
        .bind(role_name)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(db_error)?
        else {
            return Ok(false);
        };

        sqlx::query(
            "INSERT INTO roles (name, created_at, deleted_at) VALUES (?1, ?2, NULL)
             ON CONFLICT (name) DO UPDATE SET deleted_at = NULL",
        )
        .bind(new_role_name)
        .bind(created_at)
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?;
        sqlx::query(
            "INSERT INTO role_permissions (role_name, permission_name, created_at)
             SELECT ?1, permission_name, created_at FROM role_permissions WHERE role_name = ?2
             ON CONFLICT (role_name, permission_name) DO NOTHING",
        )
        .bind(new_role_name)
        .bind(role_name)
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?;
        sqlx::query(
            "INSERT INTO user_roles (user_id, role_name, created_at)
             SELECT user_id, ?1, created_at FROM user_roles WHERE role_name = ?2
             ON CONFLICT (user_id, role_name) DO NOTHING",
        )
        .bind(new_role_name)
        .bind(role_name)
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?;
        sqlx::query("UPDATE roles SET deleted_at = ?1 WHERE name = ?2 AND deleted_at IS NULL")
            .bind(Utc::now())
            .bind(role_name)
            .execute(&mut *transaction)
            .await
            .map_err(db_error)?;
        transaction.commit().await.map_err(db_error)?;
        Ok(true)
    }

    async fn delete_role(&self, role_name: &str) -> Result<bool, DomainError> {
        sqlx::query("UPDATE roles SET deleted_at = ?1 WHERE name = ?2 AND deleted_at IS NULL")
            .bind(Utc::now())
            .bind(role_name)
            .execute(&self.pool)
            .await
            .map(|result| result.rows_affected() > 0)
            .map_err(db_error)
    }

    async fn role_permissions(&self, role_name: &str) -> Result<Vec<PermissionName>, DomainError> {
        let names = sqlx::query_scalar::<_, String>(
            "SELECT permission_name FROM role_permissions
             INNER JOIN roles r ON r.name = role_permissions.role_name
             WHERE role_name = ?1 AND r.deleted_at IS NULL ORDER BY permission_name",
        )
        .bind(role_name)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
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
        let mut transaction = self.pool.begin().await.map_err(db_error)?;
        sqlx::query(
            "INSERT INTO roles (name, created_at, deleted_at) VALUES (?1, ?2, NULL)
             ON CONFLICT (name) DO UPDATE SET deleted_at = NULL",
        )
        .bind(role_name)
        .bind(Utc::now())
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?;
        sqlx::query("DELETE FROM role_permissions WHERE role_name = ?1")
            .bind(role_name)
            .execute(&mut *transaction)
            .await
            .map_err(db_error)?;
        for permission in role_permissions {
            sqlx::query("INSERT INTO permissions (name) VALUES (?1) ON CONFLICT (name) DO NOTHING")
                .bind(permission.0)
                .execute(&mut *transaction)
                .await
                .map_err(db_error)?;
            sqlx::query(
                "INSERT INTO role_permissions (role_name, permission_name) VALUES (?1, ?2)
                 ON CONFLICT (role_name, permission_name) DO NOTHING",
            )
            .bind(role_name)
            .bind(permission.0)
            .execute(&mut *transaction)
            .await
            .map_err(db_error)?;
        }
        transaction.commit().await.map_err(db_error)
    }

    async fn ensure_identity_seed_data(&self) -> Result<(), DomainError> {
        let mut transaction = self.pool.begin().await.map_err(db_error)?;
        sqlx::query(
            "INSERT INTO roles (name, created_at, deleted_at) VALUES ('admin', ?1, NULL)
             ON CONFLICT (name) DO UPDATE SET deleted_at = NULL",
        )
        .bind(Utc::now())
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?;

        let registry = permissions::all_names().collect::<Vec<_>>();
        for permission in &registry {
            sqlx::query("INSERT INTO permissions (name) VALUES (?1) ON CONFLICT (name) DO NOTHING")
                .bind(permission.0)
                .execute(&mut *transaction)
                .await
                .map_err(db_error)?;
            sqlx::query(
                "INSERT INTO role_permissions (role_name, permission_name) VALUES ('admin', ?1)
                 ON CONFLICT (role_name, permission_name) DO NOTHING",
            )
            .bind(permission.0)
            .execute(&mut *transaction)
            .await
            .map_err(db_error)?;
        }

        if registry.is_empty() {
            sqlx::query("DELETE FROM role_permissions WHERE role_name = 'admin'")
                .execute(&mut *transaction)
                .await
                .map_err(db_error)?;
        } else {
            let mut query = QueryBuilder::<Sqlite>::new(
                "DELETE FROM role_permissions WHERE role_name = 'admin' AND permission_name NOT IN (",
            );
            let mut separated = query.separated(", ");
            for permission in &registry {
                separated.push_bind(permission.0);
            }
            separated.push_unseparated(")");
            query
                .build()
                .execute(&mut *transaction)
                .await
                .map_err(db_error)?;
        }

        transaction.commit().await.map_err(db_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{DatabaseBackend, DatabaseConfig},
        db,
        identity::users::SqliteUserRepository,
    };
    use application::shared::crud::CrudAuditContext;
    use application_contracts::identity::permissions as identity_permissions;
    use domain::identity::users::UserRepository;

    async fn repository() -> (SqlitePool, SqliteAuthorizationRepository) {
        let pool = db::connect_sqlite_with_application_migrations(&DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            url: "sqlite::memory:".to_string(),
            max_connections: 4,
            auto_migrate: true,
        })
        .await
        .unwrap();
        SqliteUserRepository::new(pool.clone())
            .insert("alice@example.com", "hash")
            .await
            .unwrap();
        let repository = SqliteAuthorizationRepository::new(pool.clone());
        (pool, repository)
    }

    fn role_audit(role_name: &str) -> CrudAuditContext {
        CrudAuditContext {
            actor: "admin@example.com".to_string(),
            action: "identity.roles.create",
            entity_type: "identity.role",
            entity_id: role_name.to_string(),
            details: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn sqlite_role_creation_and_audit_are_atomic() {
        let (pool, repository) = repository().await;
        repository
            .create_role_with_audit("auditor", role_audit("auditor"))
            .await
            .unwrap();
        let row: (String, String, Option<String>) =
            sqlx::query_as("SELECT actor, action, entity_id FROM audit_logs")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            row,
            (
                "admin@example.com".to_string(),
                "identity.roles.create".to_string(),
                Some("auditor".to_string())
            )
        );

        sqlx::query("DROP TABLE audit_logs")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            repository
                .create_role_with_audit("rolled-back", role_audit("rolled-back"))
                .await
                .is_err()
        );
        assert!(repository.find_role("rolled-back").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn sqlite_authorization_computes_effective_registered_permissions() {
        let (pool, repository) = repository().await;
        repository
            .set_role_permissions(
                "editor",
                vec![
                    identity_permissions::USERS,
                    identity_permissions::USERS_UPDATE,
                ],
            )
            .await
            .unwrap();
        repository
            .assign_role("alice@example.com", "editor")
            .await
            .unwrap();
        repository
            .assign_role("alice@example.com", "editor")
            .await
            .unwrap();

        assert!(
            repository
                .user_has_permission("alice@example.com", identity_permissions::USERS_UPDATE)
                .await
                .unwrap()
        );
        assert!(
            !repository
                .user_has_permission("alice@example.com", identity_permissions::USERS_DELETE)
                .await
                .unwrap()
        );
        assert_eq!(
            repository
                .user_permissions("alice@example.com")
                .await
                .unwrap(),
            vec![
                identity_permissions::USERS,
                identity_permissions::USERS_UPDATE
            ]
        );

        sqlx::query("INSERT INTO permissions (name) VALUES ('Unknown.Permission')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO role_permissions (role_name, permission_name)
             VALUES ('editor', 'Unknown.Permission')",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            repository
                .user_permissions("alice@example.com")
                .await
                .unwrap()
                .len(),
            2
        );

        repository.delete_role("editor").await.unwrap();
        assert!(
            repository
                .user_permissions("alice@example.com")
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            !repository
                .user_has_permission("alice@example.com", identity_permissions::USERS_UPDATE)
                .await
                .unwrap()
        );

        repository.create_role("editor").await.unwrap();
        assert!(
            repository
                .user_has_permission("alice@example.com", identity_permissions::USERS_UPDATE)
                .await
                .unwrap()
        );
        sqlx::query("UPDATE users SET deleted_at = ?1 WHERE username = 'alice@example.com'")
            .bind(Utc::now())
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            repository
                .user_permissions("alice@example.com")
                .await
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            repository.assign_role("missing@example.com", "admin").await,
            Err(DomainError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn sqlite_role_rename_preserves_permissions_assignments_and_creation_time() {
        let (_, repository) = repository().await;
        repository
            .set_role_permissions("editor", vec![identity_permissions::USERS_UPDATE])
            .await
            .unwrap();
        repository
            .assign_role("alice@example.com", "editor")
            .await
            .unwrap();
        let original = repository.find_role("editor").await.unwrap().unwrap();

        assert!(repository.update_role("editor", "publisher").await.unwrap());
        assert!(!repository.update_role("missing", "other").await.unwrap());
        assert!(repository.find_role("editor").await.unwrap().is_none());
        let renamed = repository.find_role("publisher").await.unwrap().unwrap();
        assert_eq!(renamed.created_at, original.created_at);
        assert_eq!(
            repository.role_permissions("publisher").await.unwrap(),
            vec![identity_permissions::USERS_UPDATE]
        );
        assert!(
            repository
                .user_has_permission("alice@example.com", identity_permissions::USERS_UPDATE)
                .await
                .unwrap()
        );

        assert!(repository.delete_role("publisher").await.unwrap());
        assert!(!repository.delete_role("publisher").await.unwrap());
        assert!(
            repository
                .role_permissions("publisher")
                .await
                .unwrap()
                .is_empty()
        );
        repository.create_role("publisher").await.unwrap();
        assert_eq!(repository.list_roles().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn sqlite_role_pages_and_admin_registry_reconciliation_match_contract() {
        let (pool, repository) = repository().await;
        repository.create_role("auditor").await.unwrap();
        repository
            .set_role_permissions("editor", vec![identity_permissions::USERS])
            .await
            .unwrap();
        repository.create_role("publisher").await.unwrap();

        let (with_permissions, count) = repository
            .list_roles_page(
                1,
                10,
                Some("IT".to_string()),
                Some("with-permissions".to_string()),
                Some("name desc".to_string()),
            )
            .await
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(with_permissions[0].name, "editor");

        let (without_permissions, count) = repository
            .list_roles_page(
                1,
                1,
                None,
                Some("without-permissions".to_string()),
                Some("name asc".to_string()),
            )
            .await
            .unwrap();
        assert_eq!(count, 2);
        assert_eq!(without_permissions[0].name, "auditor");

        sqlx::query("INSERT INTO permissions (name) VALUES ('Legacy.Permission')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO role_permissions (role_name, permission_name)
             VALUES ('admin', 'Legacy.Permission')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("UPDATE roles SET deleted_at = ?1 WHERE name = 'admin'")
            .bind(Utc::now())
            .execute(&pool)
            .await
            .unwrap();

        repository.ensure_identity_seed_data().await.unwrap();
        repository.ensure_identity_seed_data().await.unwrap();
        let admin_permissions = repository.role_permissions("admin").await.unwrap();
        assert_eq!(admin_permissions.len(), permissions::all_names().count());
        assert!(repository.find_role("admin").await.unwrap().is_some());
        let stale_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM role_permissions
             WHERE role_name = 'admin' AND permission_name = 'Legacy.Permission'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stale_count, 0);
    }
}
