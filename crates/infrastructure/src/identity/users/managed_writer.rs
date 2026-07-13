use application::{
    identity::users::writer::{
        CreateManagedUser, ManagedUserWriter, RegisterManagedUser, UpdateManagedUser,
    },
    shared::{
        errors::{ApplicationError, ApplicationResult},
        jobs::DurableJobOptions,
        search::SearchDocument,
    },
};
use domain::identity::users::User;

use crate::{
    identity::{
        repository::SqlxIdentityRepository,
        users::{mapper::map_user, queries::user_select},
    },
    jobs::durable::SqlxDurableJobQueue,
    search::jobs::{SEARCH_INDEX_JOB, SearchIndexCommand},
};

const USER_INDEX: &str = "identity_users";

impl ManagedUserWriter for SqlxIdentityRepository {
    async fn register_managed_user(&self, command: RegisterManagedUser) -> ApplicationResult<User> {
        let mut transaction = self.pool.begin().await.map_err(db_error)?;
        let revision = sqlx::query_scalar::<_, i64>(
            "INSERT INTO users (
                 username, password_hash, email_verification_token,
                 email_verification_sent_at
             )
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (username) DO NOTHING
             RETURNING search_revision",
        )
        .bind(&command.username)
        .bind(&command.password_hash)
        .bind(&command.verification_token)
        .bind(command.verification_sent_at)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApplicationError::Conflict("user already exists".to_string()))?;
        let user = fetch_user(&mut transaction, &command.username)
            .await?
            .ok_or_else(|| {
                ApplicationError::Unexpected("registered user was not found".to_string())
            })?;
        if command.publish_search {
            enqueue_upsert(&mut transaction, &user, &[], revision).await?;
        }
        if let Some(mail) = command.mail {
            enqueue_mail(
                &mut transaction,
                mail,
                "verify_email",
                &command.username,
                command.verification_sent_at,
            )
            .await?;
        }
        transaction.commit().await.map_err(db_error)?;
        Ok(user)
    }

    async fn verify_managed_email(
        &self,
        token: &str,
        verified_at: chrono::DateTime<chrono::Utc>,
        publish_search: bool,
    ) -> ApplicationResult<bool> {
        let mut transaction = self.pool.begin().await.map_err(db_error)?;
        let verified = sqlx::query_as::<_, (String, i64)>(
            "UPDATE users
             SET email_verified_at = $1,
                 email_verification_token = NULL,
                 email_verification_sent_at = NULL,
                 search_revision = search_revision + 1
             WHERE email_verification_token = $2
               AND email_verification_sent_at > $1 - INTERVAL '24 hours'
               AND deleted_at IS NULL
             RETURNING username, search_revision",
        )
        .bind(verified_at)
        .bind(token)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(db_error)?;
        let Some((username, revision)) = verified else {
            transaction.rollback().await.map_err(db_error)?;
            return Ok(false);
        };
        if publish_search {
            let user = fetch_user(&mut transaction, &username)
                .await?
                .ok_or_else(|| {
                    ApplicationError::Unexpected("verified user was not found".to_string())
                })?;
            let roles = fetch_roles(&mut transaction, user.id).await?;
            enqueue_upsert(&mut transaction, &user, &roles, revision).await?;
        }
        transaction.commit().await.map_err(db_error)?;
        Ok(true)
    }

    async fn set_reset_token_with_mail(
        &self,
        username: &str,
        token: &str,
        sent_at: chrono::DateTime<chrono::Utc>,
        mail: application::shared::mail::TransactionalMail,
    ) -> ApplicationResult<bool> {
        let mut transaction = self.pool.begin().await.map_err(db_error)?;
        let updated = sqlx::query(
            "UPDATE users SET reset_token = $1, reset_sent_at = $2
             WHERE username = $3 AND deleted_at IS NULL",
        )
        .bind(token)
        .bind(sent_at)
        .bind(username)
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?
        .rows_affected()
            == 1;
        if updated {
            enqueue_mail(&mut transaction, mail, "reset_password", username, sent_at).await?;
        }
        transaction.commit().await.map_err(db_error)?;
        Ok(updated)
    }

    async fn set_verification_with_mail(
        &self,
        username: &str,
        token: &str,
        sent_at: chrono::DateTime<chrono::Utc>,
        mail: application::shared::mail::TransactionalMail,
    ) -> ApplicationResult<bool> {
        let mut transaction = self.pool.begin().await.map_err(db_error)?;
        let updated = sqlx::query(
            "UPDATE users SET email_verification_token = $1, email_verification_sent_at = $2
             WHERE username = $3 AND deleted_at IS NULL AND email_verified_at IS NULL",
        )
        .bind(token)
        .bind(sent_at)
        .bind(username)
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?
        .rows_affected()
            == 1;
        if updated {
            enqueue_mail(&mut transaction, mail, "verify_email", username, sent_at).await?;
        }
        transaction.commit().await.map_err(db_error)?;
        Ok(updated)
    }

    async fn set_magic_link_with_mail(
        &self,
        username: &str,
        token: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
        mail: application::shared::mail::TransactionalMail,
    ) -> ApplicationResult<bool> {
        let mut transaction = self.pool.begin().await.map_err(db_error)?;
        let updated = sqlx::query(
            "UPDATE users SET magic_link_token = $1, magic_link_expires_at = $2
             WHERE username = $3 AND deleted_at IS NULL",
        )
        .bind(token)
        .bind(expires_at)
        .bind(username)
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?
        .rows_affected()
            == 1;
        if updated {
            enqueue_mail(&mut transaction, mail, "magic_link", username, expires_at).await?;
        }
        transaction.commit().await.map_err(db_error)?;
        Ok(updated)
    }

    async fn request_email_change(
        &self,
        username: &str,
        new_email: &str,
        token: &str,
        sent_at: chrono::DateTime<chrono::Utc>,
        mail: Option<application::shared::mail::TransactionalMail>,
    ) -> ApplicationResult<bool> {
        let mut transaction = self.pool.begin().await.map_err(db_error)?;
        let updated = sqlx::query(
            "UPDATE users SET pending_email = $1, email_change_token = $2,
                              email_change_sent_at = $3
             WHERE username = $4 AND deleted_at IS NULL
               AND NOT EXISTS (SELECT 1 FROM users other
                               WHERE other.username = $1 AND other.deleted_at IS NULL)",
        )
        .bind(new_email)
        .bind(token)
        .bind(sent_at)
        .bind(username)
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?
        .rows_affected()
            == 1;
        if updated && let Some(mail) = mail {
            enqueue_mail(&mut transaction, mail, "change_email", username, sent_at).await?;
        }
        transaction.commit().await.map_err(db_error)?;
        Ok(updated)
    }

    async fn confirm_email_change(
        &self,
        token: &str,
        confirmed_at: chrono::DateTime<chrono::Utc>,
        publish_search: bool,
    ) -> ApplicationResult<bool> {
        let mut transaction = self.pool.begin().await.map_err(db_error)?;
        let changed = sqlx::query_as::<_, (String, i64)>(
            "UPDATE users SET username = pending_email, pending_email = NULL,
                              email_change_token = NULL, email_change_sent_at = NULL,
                              email_verified_at = $1, search_revision = search_revision + 1
             WHERE email_change_token = $2
               AND email_change_sent_at > $1 - INTERVAL '1 hour'
               AND pending_email IS NOT NULL AND deleted_at IS NULL
             RETURNING username, search_revision",
        )
        .bind(confirmed_at)
        .bind(token)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(db_error)?;
        let Some((username, revision)) = changed else {
            transaction.rollback().await.map_err(db_error)?;
            return Ok(false);
        };
        if publish_search {
            let user = fetch_user(&mut transaction, &username)
                .await?
                .ok_or_else(|| {
                    ApplicationError::Unexpected("changed user was not found".to_string())
                })?;
            let roles = fetch_roles(&mut transaction, user.id).await?;
            enqueue_upsert(&mut transaction, &user, &roles, revision).await?;
        }
        transaction.commit().await.map_err(db_error)?;
        Ok(true)
    }

    async fn create_managed_user(&self, command: CreateManagedUser) -> ApplicationResult<User> {
        let mut transaction = self.pool.begin().await.map_err(db_error)?;
        let revision = sqlx::query_scalar::<_, i64>(
            "INSERT INTO users (username, password_hash, email_verified_at, deleted_at)
             VALUES ($1, $2, $3, NULL)
             ON CONFLICT (username) DO UPDATE
             SET password_hash = EXCLUDED.password_hash,
                 email_verified_at = EXCLUDED.email_verified_at,
                 reset_token = NULL,
                 reset_sent_at = NULL,
                 email_verification_token = NULL,
                 email_verification_sent_at = NULL,
                 magic_link_token = NULL,
                 magic_link_expires_at = NULL,
                 totp_secret = NULL,
                 totp_enabled_at = NULL,
                 totp_backup_code_hashes = '{}',
                 totp_login_token = NULL,
                 totp_login_expires_at = NULL,
                 deleted_at = NULL,
                 search_revision = users.search_revision + 1
             WHERE users.deleted_at IS NOT NULL
             RETURNING search_revision",
        )
        .bind(&command.username)
        .bind(&command.password_hash)
        .bind(command.email_verified_at)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(db_error)?
        .ok_or_else(|| ApplicationError::Conflict("user already exists".to_string()))?;

        set_roles(&mut transaction, &command.username, &command.roles).await?;
        let user = fetch_user(&mut transaction, &command.username)
            .await?
            .ok_or_else(|| {
                ApplicationError::Unexpected("created user was not found".to_string())
            })?;
        if command.publish_search {
            enqueue_upsert(&mut transaction, &user, &command.roles, revision).await?;
        }
        transaction.commit().await.map_err(db_error)?;
        Ok(user)
    }

    async fn update_managed_user(
        &self,
        command: UpdateManagedUser,
    ) -> ApplicationResult<Option<User>> {
        let mut transaction = self.pool.begin().await.map_err(db_error)?;
        let revision = if let Some(password_hash) = command.password_hash.as_deref() {
            sqlx::query_scalar::<_, i64>(
                "UPDATE users
                 SET password_hash = $1, email_verified_at = $2,
                     search_revision = search_revision + 1
                 WHERE username = $3 AND deleted_at IS NULL
                 RETURNING search_revision",
            )
            .bind(password_hash)
            .bind(command.email_verified_at)
            .bind(&command.username)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(db_error)?
        } else {
            sqlx::query_scalar::<_, i64>(
                "UPDATE users
                 SET email_verified_at = $1, search_revision = search_revision + 1
                 WHERE username = $2 AND deleted_at IS NULL
                 RETURNING search_revision",
            )
            .bind(command.email_verified_at)
            .bind(&command.username)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(db_error)?
        };
        let Some(revision) = revision else {
            transaction.rollback().await.map_err(db_error)?;
            return Ok(None);
        };

        set_roles(&mut transaction, &command.username, &command.roles).await?;
        let user = fetch_user(&mut transaction, &command.username)
            .await?
            .ok_or_else(|| {
                ApplicationError::Unexpected("updated user was not found".to_string())
            })?;
        if command.publish_search {
            enqueue_upsert(&mut transaction, &user, &command.roles, revision).await?;
        }
        transaction.commit().await.map_err(db_error)?;
        Ok(Some(user))
    }

    async fn delete_managed_user(
        &self,
        username: &str,
        publish_search: bool,
    ) -> ApplicationResult<bool> {
        let mut transaction = self.pool.begin().await.map_err(db_error)?;
        let deleted = sqlx::query_as::<_, (uuid::Uuid, i64)>(
            "UPDATE users
             SET deleted_at = NOW(), search_revision = search_revision + 1
             WHERE username = $1 AND deleted_at IS NULL
             RETURNING pid, search_revision",
        )
        .bind(username)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(db_error)?;
        let Some((pid, revision)) = deleted else {
            transaction.rollback().await.map_err(db_error)?;
            return Ok(false);
        };
        if publish_search {
            enqueue_command(
                &mut transaction,
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
        transaction.commit().await.map_err(db_error)?;
        Ok(true)
    }
}

async fn enqueue_mail(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    mail: application::shared::mail::TransactionalMail,
    purpose: &str,
    username: &str,
    timestamp: chrono::DateTime<chrono::Utc>,
) -> ApplicationResult<()> {
    crate::mail::jobs::enqueue_in(
        transaction,
        mail,
        format!("mail:{purpose}:{username}:{}", timestamp.timestamp_micros()),
    )
    .await
    .map(|_| ())
    .map_err(ApplicationError::Infrastructure)
}

async fn fetch_roles(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: i32,
) -> ApplicationResult<Vec<String>> {
    sqlx::query_scalar("SELECT role_name FROM user_roles WHERE user_id = $1 ORDER BY role_name")
        .bind(user_id)
        .fetch_all(&mut **transaction)
        .await
        .map_err(db_error)
}

async fn set_roles(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    username: &str,
    roles: &[String],
) -> ApplicationResult<()> {
    let user_id = sqlx::query_scalar::<_, i32>("SELECT id FROM users WHERE username = $1")
        .bind(username)
        .fetch_one(&mut **transaction)
        .await
        .map_err(db_error)?;
    sqlx::query("DELETE FROM user_roles WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut **transaction)
        .await
        .map_err(db_error)?;
    for role in roles {
        sqlx::query(
            "INSERT INTO user_roles (user_id, role_name)
             VALUES ($1, $2)
             ON CONFLICT (user_id, role_name) DO NOTHING",
        )
        .bind(user_id)
        .bind(role)
        .execute(&mut **transaction)
        .await
        .map_err(db_error)?;
    }
    Ok(())
}

async fn fetch_user(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    username: &str,
) -> ApplicationResult<Option<User>> {
    let sql = user_select("username = $1 AND deleted_at IS NULL");
    sqlx::query(&sql)
        .bind(username)
        .fetch_optional(&mut **transaction)
        .await
        .map(|row| row.map(map_user))
        .map_err(db_error)
}

async fn enqueue_upsert(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user: &User,
    roles: &[String],
    revision: i64,
) -> ApplicationResult<()> {
    enqueue_user_upsert(
        transaction,
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
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    pid: uuid::Uuid,
    username: &str,
    created_at: chrono::DateTime<chrono::Utc>,
    is_verified: bool,
    roles: &[String],
    revision: i64,
) -> ApplicationResult<()> {
    let mut fields = serde_json::Map::new();
    fields.insert("username".to_string(), serde_json::json!(username));
    fields.insert("created_at".to_string(), serde_json::json!(created_at));
    fields.insert("is_verified".to_string(), serde_json::json!(is_verified));
    fields.insert("roles".to_string(), serde_json::json!(roles));
    enqueue_command(
        transaction,
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

async fn enqueue_command(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    command: SearchIndexCommand,
    pid: uuid::Uuid,
    revision: i64,
) -> ApplicationResult<()> {
    let payload = serde_json::to_value(command)
        .map_err(|err| ApplicationError::Unexpected(err.to_string()))?;
    SqlxDurableJobQueue::enqueue_in(
        transaction,
        SEARCH_INDEX_JOB,
        payload,
        DurableJobOptions {
            idempotency_key: Some(format!("{USER_INDEX}:{pid}:{revision}")),
            max_attempts: 5,
        },
    )
    .await
    .map(|_| ())
    .map_err(ApplicationError::Infrastructure)
}

fn db_error(error: sqlx::Error) -> ApplicationError {
    ApplicationError::Infrastructure(error.to_string())
}
