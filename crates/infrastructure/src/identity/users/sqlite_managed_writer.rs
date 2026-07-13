use application::{
    identity::users::writer::{
        CreateManagedUser, ManagedUserWriter, RegisterManagedUser, UpdateManagedUser,
    },
    shared::{
        errors::{ApplicationError, ApplicationResult},
        mail::TransactionalMail,
        search::SearchDocument,
    },
};
use chrono::{DateTime, Duration, Utc};
use domain::identity::users::User;
use sqlx::{Row, SqliteConnection};

use crate::{
    identity::users::SqliteUserRepository,
    mail::jobs::{SEND_MAIL_JOB, SendMailJob},
    search::jobs::{SEARCH_INDEX_JOB, SearchIndexCommand},
};

const USER_INDEX: &str = "identity_users";

impl ManagedUserWriter for SqliteUserRepository {
    async fn register_managed_user(&self, command: RegisterManagedUser) -> ApplicationResult<User> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let pid = uuid::Uuid::new_v4();
        let created_at = Utc::now();
        let revision = sqlx::query_scalar::<_, i64>(
            "INSERT INTO users
                (pid, username, password_hash, created_at, email_verification_token,
                 email_verification_sent_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (username) DO NOTHING RETURNING search_revision",
        )
        .bind(pid)
        .bind(&command.username)
        .bind(&command.password_hash)
        .bind(created_at)
        .bind(&command.verification_token)
        .bind(command.verification_sent_at)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApplicationError::Conflict("user already exists".to_string()))?;
        let user = fetch_user(&mut tx, &command.username)
            .await?
            .ok_or_else(|| {
                ApplicationError::Unexpected("registered user was not found".to_string())
            })?;
        if command.publish_search {
            enqueue_upsert(&mut tx, &user, &[], revision).await?;
        }
        if let Some(mail) = command.mail {
            enqueue_mail(
                &mut tx,
                mail,
                "verify_email",
                &command.username,
                command.verification_sent_at,
            )
            .await?;
        }
        tx.commit().await.map_err(db_error)?;
        Ok(user)
    }

    async fn verify_managed_email(
        &self,
        token: &str,
        verified_at: DateTime<Utc>,
        publish_search: bool,
    ) -> ApplicationResult<bool> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let verified = sqlx::query_as::<_, (String, i64)>(
            "UPDATE users SET email_verified_at = ?1, email_verification_token = NULL,
                 email_verification_sent_at = NULL, search_revision = search_revision + 1
             WHERE email_verification_token = ?2 AND email_verification_sent_at > ?3
               AND deleted_at IS NULL RETURNING username, search_revision",
        )
        .bind(verified_at)
        .bind(token)
        .bind(verified_at - Duration::hours(24))
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?;
        let Some((username, revision)) = verified else {
            return Ok(false);
        };
        if publish_search {
            let user = fetch_user(&mut tx, &username).await?.ok_or_else(|| {
                ApplicationError::Unexpected("verified user was not found".to_string())
            })?;
            let roles = fetch_roles(&mut tx, user.id).await?;
            enqueue_upsert(&mut tx, &user, &roles, revision).await?;
        }
        tx.commit().await.map_err(db_error)?;
        Ok(true)
    }

    async fn set_reset_token_with_mail(
        &self,
        username: &str,
        token: &str,
        sent_at: DateTime<Utc>,
        mail: TransactionalMail,
    ) -> ApplicationResult<bool> {
        update_token_with_mail(
            self,
            TokenMailUpdate {
                token_column: "reset_token",
                time_column: "reset_sent_at",
                username,
                token,
                time: sent_at,
                mail,
                purpose: "reset_password",
                extra_predicate: "",
            },
        )
        .await
    }

    async fn set_verification_with_mail(
        &self,
        username: &str,
        token: &str,
        sent_at: DateTime<Utc>,
        mail: TransactionalMail,
    ) -> ApplicationResult<bool> {
        update_token_with_mail(
            self,
            TokenMailUpdate {
                token_column: "email_verification_token",
                time_column: "email_verification_sent_at",
                username,
                token,
                time: sent_at,
                mail,
                purpose: "verify_email",
                extra_predicate: " AND email_verified_at IS NULL",
            },
        )
        .await
    }

    async fn set_magic_link_with_mail(
        &self,
        username: &str,
        token: &str,
        expires_at: DateTime<Utc>,
        mail: TransactionalMail,
    ) -> ApplicationResult<bool> {
        update_token_with_mail(
            self,
            TokenMailUpdate {
                token_column: "magic_link_token",
                time_column: "magic_link_expires_at",
                username,
                token,
                time: expires_at,
                mail,
                purpose: "magic_link",
                extra_predicate: "",
            },
        )
        .await
    }

    async fn request_email_change(
        &self,
        username: &str,
        new_email: &str,
        token: &str,
        sent_at: DateTime<Utc>,
        mail: Option<TransactionalMail>,
    ) -> ApplicationResult<bool> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let updated = sqlx::query(
            "UPDATE users SET pending_email = ?1, email_change_token = ?2, email_change_sent_at = ?3
             WHERE username = ?4 AND deleted_at IS NULL AND NOT EXISTS (
                 SELECT 1 FROM users other WHERE other.username = ?1 AND other.deleted_at IS NULL
             )",
        ).bind(new_email).bind(token).bind(sent_at).bind(username)
            .execute(&mut *tx).await.map_err(db_error)?.rows_affected() == 1;
        if updated && let Some(mail) = mail {
            enqueue_mail(&mut tx, mail, "change_email", username, sent_at).await?;
        }
        tx.commit().await.map_err(db_error)?;
        Ok(updated)
    }

    async fn confirm_email_change(
        &self,
        token: &str,
        confirmed_at: DateTime<Utc>,
        publish_search: bool,
    ) -> ApplicationResult<bool> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let changed = sqlx::query_as::<_, (String, i64)>(
            "UPDATE users SET username = pending_email, pending_email = NULL,
                 email_change_token = NULL, email_change_sent_at = NULL,
                 email_verified_at = ?1, search_revision = search_revision + 1
             WHERE email_change_token = ?2 AND email_change_sent_at > ?3
               AND pending_email IS NOT NULL AND deleted_at IS NULL
             RETURNING username, search_revision",
        )
        .bind(confirmed_at)
        .bind(token)
        .bind(confirmed_at - Duration::hours(1))
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?;
        let Some((username, revision)) = changed else {
            return Ok(false);
        };
        if publish_search {
            let user = fetch_user(&mut tx, &username).await?.ok_or_else(|| {
                ApplicationError::Unexpected("changed user was not found".to_string())
            })?;
            let roles = fetch_roles(&mut tx, user.id).await?;
            enqueue_upsert(&mut tx, &user, &roles, revision).await?;
        }
        tx.commit().await.map_err(db_error)?;
        Ok(true)
    }

    async fn create_managed_user(&self, command: CreateManagedUser) -> ApplicationResult<User> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let revision = sqlx::query_scalar::<_, i64>(
            "INSERT INTO users (pid, username, password_hash, created_at, email_verified_at, deleted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL)
             ON CONFLICT (username) DO UPDATE SET password_hash = excluded.password_hash,
                 email_verified_at = excluded.email_verified_at, reset_token = NULL,
                 reset_sent_at = NULL, email_verification_token = NULL,
                 email_verification_sent_at = NULL, magic_link_token = NULL,
                 magic_link_expires_at = NULL, totp_secret = NULL, totp_enabled_at = NULL,
                 totp_backup_code_hashes = '[]', totp_login_token = NULL,
                 totp_login_expires_at = NULL, deleted_at = NULL,
                 search_revision = users.search_revision + 1
             WHERE users.deleted_at IS NOT NULL RETURNING search_revision",
        ).bind(uuid::Uuid::new_v4()).bind(&command.username).bind(&command.password_hash)
            .bind(Utc::now()).bind(command.email_verified_at)
            .fetch_optional(&mut *tx).await.map_err(db_error)?
            .ok_or_else(|| ApplicationError::Conflict("user already exists".to_string()))?;
        set_roles(&mut tx, &command.username, &command.roles).await?;
        let user = fetch_user(&mut tx, &command.username)
            .await?
            .ok_or_else(|| {
                ApplicationError::Unexpected("created user was not found".to_string())
            })?;
        if command.publish_search {
            enqueue_upsert(&mut tx, &user, &command.roles, revision).await?;
        }
        tx.commit().await.map_err(db_error)?;
        Ok(user)
    }

    async fn update_managed_user(
        &self,
        command: UpdateManagedUser,
    ) -> ApplicationResult<Option<User>> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let revision = if let Some(hash) = command.password_hash.as_deref() {
            sqlx::query_scalar(
                "UPDATE users SET password_hash = ?1, email_verified_at = ?2,
                search_revision = search_revision + 1 WHERE username = ?3 AND deleted_at IS NULL
                RETURNING search_revision",
            )
            .bind(hash)
            .bind(command.email_verified_at)
            .bind(&command.username)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?
        } else {
            sqlx::query_scalar(
                "UPDATE users SET email_verified_at = ?1,
                search_revision = search_revision + 1 WHERE username = ?2 AND deleted_at IS NULL
                RETURNING search_revision",
            )
            .bind(command.email_verified_at)
            .bind(&command.username)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?
        };
        let Some(revision) = revision else {
            return Ok(None);
        };
        set_roles(&mut tx, &command.username, &command.roles).await?;
        let user = fetch_user(&mut tx, &command.username)
            .await?
            .ok_or_else(|| {
                ApplicationError::Unexpected("updated user was not found".to_string())
            })?;
        if command.publish_search {
            enqueue_upsert(&mut tx, &user, &command.roles, revision).await?;
        }
        tx.commit().await.map_err(db_error)?;
        Ok(Some(user))
    }

    async fn delete_managed_user(
        &self,
        username: &str,
        publish_search: bool,
    ) -> ApplicationResult<bool> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let deleted = sqlx::query_as::<_, (uuid::Uuid, i64)>(
            "UPDATE users SET deleted_at = ?1, search_revision = search_revision + 1
             WHERE username = ?2 AND deleted_at IS NULL RETURNING pid, search_revision",
        )
        .bind(Utc::now())
        .bind(username)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?;
        let Some((pid, revision)) = deleted else {
            return Ok(false);
        };
        if publish_search {
            enqueue_search(
                &mut tx,
                SearchIndexCommand::Delete {
                    index: USER_INDEX.to_string(),
                    document_id: pid.to_string(),
                    revision: Some(revision),
                },
                pid,
                revision,
            )
            .await?;
        }
        tx.commit().await.map_err(db_error)?;
        Ok(true)
    }
}

struct TokenMailUpdate<'a> {
    token_column: &'static str,
    time_column: &'static str,
    username: &'a str,
    token: &'a str,
    time: DateTime<Utc>,
    mail: TransactionalMail,
    purpose: &'static str,
    extra_predicate: &'static str,
}

async fn update_token_with_mail(
    repository: &SqliteUserRepository,
    update: TokenMailUpdate<'_>,
) -> ApplicationResult<bool> {
    let mut tx = repository.pool.begin().await.map_err(db_error)?;
    let sql = format!(
        "UPDATE users SET {} = ?1, {} = ?2 WHERE username = ?3 AND deleted_at IS NULL{}",
        update.token_column, update.time_column, update.extra_predicate
    );
    let updated = sqlx::query(&sql)
        .bind(update.token)
        .bind(update.time)
        .bind(update.username)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?
        .rows_affected()
        == 1;
    if updated {
        enqueue_mail(
            &mut tx,
            update.mail,
            update.purpose,
            update.username,
            update.time,
        )
        .await?;
    }
    tx.commit().await.map_err(db_error)?;
    Ok(updated)
}

async fn set_roles(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    username: &str,
    roles: &[String],
) -> ApplicationResult<()> {
    let id: i32 = sqlx::query_scalar("SELECT id FROM users WHERE username = ?1")
        .bind(username)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_error)?;
    sqlx::query("DELETE FROM user_roles WHERE user_id = ?1")
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    for role in roles {
        sqlx::query("INSERT INTO user_roles (user_id, role_name) VALUES (?1, ?2) ON CONFLICT (user_id, role_name) DO NOTHING")
            .bind(id).bind(role).execute(&mut **tx).await.map_err(db_error)?;
    }
    Ok(())
}

async fn fetch_roles(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: i32,
) -> ApplicationResult<Vec<String>> {
    sqlx::query_scalar("SELECT role_name FROM user_roles WHERE user_id = ?1 ORDER BY role_name")
        .bind(id)
        .fetch_all(&mut **tx)
        .await
        .map_err(db_error)
}

async fn fetch_user(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    username: &str,
) -> ApplicationResult<Option<User>> {
    let row = sqlx::query("SELECT id, pid, username, password_hash, created_at, reset_token, reset_sent_at,
        email_verification_token, email_verification_sent_at, email_verified_at,
        magic_link_token, magic_link_expires_at FROM users WHERE username = ?1 AND deleted_at IS NULL")
        .bind(username).fetch_optional(&mut **tx).await.map_err(db_error)?;
    Ok(row.map(|row| User {
        id: row.get("id"),
        pid: row.get("pid"),
        username: row.get("username"),
        password_hash: row.get("password_hash"),
        created_at: row.get("created_at"),
        reset_token: row.get("reset_token"),
        reset_sent_at: row.get("reset_sent_at"),
        email_verification_token: row.get("email_verification_token"),
        email_verification_sent_at: row.get("email_verification_sent_at"),
        email_verified_at: row.get("email_verified_at"),
        magic_link_token: row.get("magic_link_token"),
        magic_link_expires_at: row.get("magic_link_expires_at"),
    }))
}

async fn enqueue_mail(
    connection: &mut SqliteConnection,
    mail: TransactionalMail,
    purpose: &str,
    username: &str,
    timestamp: DateTime<Utc>,
) -> ApplicationResult<()> {
    let payload = serde_json::to_value(SendMailJob { mail })
        .map_err(|error| ApplicationError::Unexpected(error.to_string()))?;
    enqueue_outbox(
        connection,
        SEND_MAIL_JOB,
        payload,
        Some(format!(
            "mail:{purpose}:{username}:{}",
            timestamp.timestamp_micros()
        )),
    )
    .await
}

async fn enqueue_upsert(
    connection: &mut SqliteConnection,
    user: &User,
    roles: &[String],
    revision: i64,
) -> ApplicationResult<()> {
    enqueue_user_upsert(
        connection,
        user.pid,
        &user.username,
        user.created_at,
        user.email_verified_at.is_some(),
        roles,
        revision,
    )
    .await
}

pub(crate) async fn enqueue_user_upsert(
    connection: &mut SqliteConnection,
    pid: uuid::Uuid,
    username: &str,
    created_at: DateTime<Utc>,
    is_verified: bool,
    roles: &[String],
    revision: i64,
) -> ApplicationResult<()> {
    let mut fields = serde_json::Map::new();
    fields.insert("username".to_string(), serde_json::json!(username));
    fields.insert("created_at".to_string(), serde_json::json!(created_at));
    fields.insert("is_verified".to_string(), serde_json::json!(is_verified));
    fields.insert("roles".to_string(), serde_json::json!(roles));
    enqueue_search(
        connection,
        SearchIndexCommand::Upsert {
            index: USER_INDEX.to_string(),
            documents: vec![SearchDocument {
                id: pid.to_string(),
                fields,
            }],
            revision: Some(revision),
        },
        pid,
        revision,
    )
    .await
}

async fn enqueue_search(
    connection: &mut SqliteConnection,
    command: SearchIndexCommand,
    pid: uuid::Uuid,
    revision: i64,
) -> ApplicationResult<()> {
    let payload = serde_json::to_value(command)
        .map_err(|error| ApplicationError::Unexpected(error.to_string()))?;
    enqueue_outbox(
        connection,
        SEARCH_INDEX_JOB,
        payload,
        Some(format!("{USER_INDEX}:{pid}:{revision}")),
    )
    .await
}

async fn enqueue_outbox(
    connection: &mut SqliteConnection,
    name: &str,
    payload: serde_json::Value,
    idempotency_key: Option<String>,
) -> ApplicationResult<()> {
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO outbox_messages
        (id, name, payload, idempotency_key, max_attempts, available_at, created_at)
        VALUES (?1, ?2, ?3, ?4, 5, ?5, ?5)
        ON CONFLICT (name, idempotency_key) WHERE idempotency_key IS NOT NULL
        DO UPDATE SET name = excluded.name",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(name)
    .bind(payload.to_string())
    .bind(idempotency_key)
    .bind(now)
    .execute(connection)
    .await
    .map(|_| ())
    .map_err(db_error)
}

fn db_error(error: sqlx::Error) -> ApplicationError {
    ApplicationError::Infrastructure(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{DatabaseBackend, DatabaseConfig},
        db,
    };

    async fn repository() -> (sqlx::SqlitePool, SqliteUserRepository) {
        let pool = db::connect_sqlite(&DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            url: "sqlite::memory:".to_string(),
            max_connections: 4,
            auto_migrate: true,
        })
        .await
        .unwrap();
        (pool.clone(), SqliteUserRepository::new(pool))
    }

    fn mail(to: &str) -> TransactionalMail {
        TransactionalMail::VerifyEmail {
            to: to.to_string(),
            app_name: "Test".to_string(),
            verification_url: "https://example.test/verify".to_string(),
        }
    }

    async fn outbox_count(pool: &sqlx::SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM outbox_messages")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn sqlite_managed_user_mutation_revision_and_outbox_are_atomic() {
        let (pool, repository) = repository().await;
        let user = repository
            .create_managed_user(CreateManagedUser {
                username: "alice@example.com".to_string(),
                password_hash: "hash-1".to_string(),
                email_verified_at: None,
                roles: vec!["admin".to_string()],
                publish_search: true,
            })
            .await
            .unwrap();
        assert_eq!(outbox_count(&pool).await, 1);
        let revision: i64 = sqlx::query_scalar("SELECT search_revision FROM users WHERE id = ?1")
            .bind(user.id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(revision, 0);

        let updated = repository
            .update_managed_user(UpdateManagedUser {
                username: "alice@example.com".to_string(),
                password_hash: Some("hash-2".to_string()),
                email_verified_at: Some(Utc::now()),
                roles: vec!["admin".to_string()],
                publish_search: true,
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.password_hash, "hash-2");
        assert_eq!(outbox_count(&pool).await, 2);

        let failed = repository
            .update_managed_user(UpdateManagedUser {
                username: "alice@example.com".to_string(),
                password_hash: Some("must-rollback".to_string()),
                email_verified_at: None,
                roles: vec!["missing-role".to_string()],
                publish_search: true,
            })
            .await;
        assert!(failed.is_err());
        let state: (String, i64) =
            sqlx::query_as("SELECT password_hash, search_revision FROM users WHERE id = ?1")
                .bind(user.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state, ("hash-2".to_string(), 1));
        assert_eq!(outbox_count(&pool).await, 2);

        assert!(
            repository
                .delete_managed_user("alice@example.com", true)
                .await
                .unwrap()
        );
        assert_eq!(outbox_count(&pool).await, 3);
        let payload: String = sqlx::query_scalar(
            "SELECT payload FROM outbox_messages ORDER BY created_at DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(payload["operation"], "delete");
        assert_eq!(payload["revision"], 2);
    }

    #[tokio::test]
    async fn sqlite_registration_and_mail_outbox_commit_together() {
        let (pool, repository) = repository().await;
        let sent_at = Utc::now();
        repository
            .register_managed_user(RegisterManagedUser {
                username: "alice@example.com".to_string(),
                password_hash: "hash".to_string(),
                verification_token: "verify".to_string(),
                verification_sent_at: sent_at,
                publish_search: true,
                mail: Some(mail("alice@example.com")),
            })
            .await
            .unwrap();
        assert_eq!(outbox_count(&pool).await, 2);
        assert!(
            repository
                .register_managed_user(RegisterManagedUser {
                    username: "alice@example.com".to_string(),
                    password_hash: "other".to_string(),
                    verification_token: "other".to_string(),
                    verification_sent_at: sent_at,
                    publish_search: true,
                    mail: Some(mail("alice@example.com")),
                })
                .await
                .is_err()
        );
        assert_eq!(outbox_count(&pool).await, 2);

        assert!(
            repository
                .verify_managed_email("verify", sent_at + Duration::minutes(1), true)
                .await
                .unwrap()
        );
        assert!(
            !repository
                .verify_managed_email("verify", sent_at + Duration::minutes(2), true)
                .await
                .unwrap()
        );
        assert_eq!(outbox_count(&pool).await, 3);
        assert!(
            repository
                .set_reset_token_with_mail(
                    "alice@example.com",
                    "reset",
                    sent_at,
                    mail("alice@example.com")
                )
                .await
                .unwrap()
        );
        assert!(
            repository
                .set_magic_link_with_mail(
                    "alice@example.com",
                    "magic",
                    sent_at + Duration::minutes(5),
                    mail("alice@example.com")
                )
                .await
                .unwrap()
        );
        assert!(
            !repository
                .set_verification_with_mail(
                    "alice@example.com",
                    "again",
                    sent_at,
                    mail("alice@example.com")
                )
                .await
                .unwrap()
        );
        assert_eq!(outbox_count(&pool).await, 5);
    }

    #[tokio::test]
    async fn sqlite_email_change_enforces_uniqueness_expiry_and_search_publication() {
        let (pool, repository) = repository().await;
        for username in ["alice@example.com", "taken@example.com"] {
            repository
                .create_managed_user(CreateManagedUser {
                    username: username.to_string(),
                    password_hash: "hash".to_string(),
                    email_verified_at: None,
                    roles: vec![],
                    publish_search: false,
                })
                .await
                .unwrap();
        }
        let now = Utc::now();
        assert!(
            !repository
                .request_email_change(
                    "alice@example.com",
                    "taken@example.com",
                    "collision",
                    now,
                    None
                )
                .await
                .unwrap()
        );
        assert!(
            repository
                .request_email_change(
                    "alice@example.com",
                    "new@example.com",
                    "change",
                    now,
                    Some(mail("new@example.com"))
                )
                .await
                .unwrap()
        );
        assert!(
            !repository
                .confirm_email_change("change", now + Duration::hours(2), true)
                .await
                .unwrap()
        );
        assert!(
            repository
                .confirm_email_change("change", now + Duration::minutes(30), true)
                .await
                .unwrap()
        );
        assert_eq!(outbox_count(&pool).await, 2);
        let revision: i64 = sqlx::query_scalar(
            "SELECT search_revision FROM users WHERE username = 'new@example.com'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(revision, 1);
    }
}
