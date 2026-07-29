#[cfg(feature = "db-postgres")]
const DELETE_EXPIRED_IDENTITY_LOCK_ID: i64 = 9_271_001;

#[cfg(feature = "db-sqlite")]
pub async fn delete_expired_sqlite(pool: &sqlx::SqlitePool) -> Result<u64, sqlx::Error> {
    let now = chrono::Utc::now();
    let mut transaction = pool.begin().await?;
    let sessions = sqlx::query("DELETE FROM sessions WHERE expires_at < ?1 OR max_expires_at < ?1")
        .bind(now)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
    let states = sqlx::query("DELETE FROM oauth_states WHERE expires_at < ?1")
        .bind(now)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
    let signups = sqlx::query("DELETE FROM oauth_pending_signups WHERE expires_at < ?1")
        .bind(now)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
    transaction.commit().await?;
    Ok(sessions + states + signups)
}

#[cfg(feature = "db-postgres")]
pub async fn delete_expired_postgres(pool: &sqlx::PgPool) -> Result<Option<u64>, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let locked = sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_xact_lock($1)")
        .bind(DELETE_EXPIRED_IDENTITY_LOCK_ID)
        .fetch_one(&mut *transaction)
        .await?;

    if !locked {
        transaction.rollback().await?;
        return Ok(None);
    }

    let rows_affected = sqlx::query_scalar::<_, i64>(
        "WITH deleted_sessions AS (
             DELETE FROM sessions
             WHERE expires_at < NOW() OR max_expires_at < NOW()
             RETURNING 1
         ), deleted_oauth_states AS (
             DELETE FROM oauth_states WHERE expires_at < NOW() RETURNING 1
         ), deleted_oauth_signups AS (
             DELETE FROM oauth_pending_signups WHERE expires_at < NOW() RETURNING 1
         )
         SELECT
             (SELECT COUNT(*) FROM deleted_sessions)
           + (SELECT COUNT(*) FROM deleted_oauth_states)
           + (SELECT COUNT(*) FROM deleted_oauth_signups)",
    )
    .fetch_one(&mut *transaction)
    .await? as u64;

    transaction.commit().await?;
    Ok(Some(rows_affected))
}

#[cfg(all(test, feature = "db-sqlite"))]
mod tests {
    use super::*;
    use crate::{
        config::{DatabaseBackend, DatabaseConfig},
        db,
        identity::users::SqliteUserRepository,
    };
    use domain::identity::users::UserRepository;

    #[tokio::test]
    async fn sqlite_cleanup_removes_only_expired_identity_artifacts() {
        let pool = db::connect_sqlite_with_application_migrations(&DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            url: "sqlite::memory:".to_string(),
            max_connections: 1,
            auto_migrate: true,
        })
        .await
        .unwrap();
        SqliteUserRepository::new(pool.clone())
            .insert("user@example.com", "hash")
            .await
            .unwrap();
        let user_id: i32 =
            sqlx::query_scalar("SELECT id FROM users WHERE username = 'user@example.com'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let now = chrono::Utc::now();
        for (token, expiry) in [
            ("expired", now - chrono::Duration::minutes(1)),
            ("active", now + chrono::Duration::minutes(5)),
        ] {
            sqlx::query("INSERT INTO sessions (pid, token, user_id, created_at, expires_at, max_expires_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)")
                .bind(uuid::Uuid::new_v4()).bind(token).bind(user_id).bind(now).bind(expiry).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO oauth_states (state, provider, csrf_token, flow, created_at, expires_at) VALUES (?1, 'github', 'csrf', 'login', ?2, ?3)")
                .bind(token).bind(now).bind(expiry).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO oauth_pending_signups (token, provider, provider_user_id, email, created_at, expires_at) VALUES (?1, 'github', ?1, 'user@example.com', ?2, ?3)")
                .bind(token).bind(now).bind(expiry).execute(&pool).await.unwrap();
        }

        assert_eq!(delete_expired_sqlite(&pool).await.unwrap(), 3);
        for table in ["sessions", "oauth_states", "oauth_pending_signups"] {
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(count, 1, "unexpected cleanup result for {table}");
        }
    }
}
