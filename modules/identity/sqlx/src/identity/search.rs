#[cfg(feature = "db-postgres")]
use application::shared::{
    errors::{ApplicationError, ApplicationResult},
    search::SearchDocument,
};

#[cfg(feature = "db-postgres")]
pub struct IdentitySearchSnapshot<'pool> {
    transaction: sqlx::Transaction<'pool, sqlx::Postgres>,
    last_id: i32,
}

#[cfg(feature = "db-postgres")]
impl<'pool> IdentitySearchSnapshot<'pool> {
    pub async fn begin(pool: &'pool sqlx::PgPool, rebuild_lock: &str) -> ApplicationResult<Self> {
        let mut transaction = pool.begin().await.map_err(db_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *transaction)
            .await
            .map_err(db_error)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(rebuild_lock)
            .execute(&mut *transaction)
            .await
            .map_err(db_error)?;
        Ok(Self {
            transaction,
            last_id: 0,
        })
    }

    pub async fn next_page(&mut self, batch_size: i64) -> ApplicationResult<Vec<SearchDocument>> {
        let rows = sqlx::query_as::<
            _,
            (
                i32,
                uuid::Uuid,
                String,
                chrono::DateTime<chrono::Utc>,
                bool,
                Vec<String>,
            ),
        >(
            "SELECT id, pid, username, created_at, email_verified_at IS NOT NULL,
                    ARRAY(SELECT role_name FROM user_roles WHERE user_id = users.id ORDER BY role_name)
             FROM users
             WHERE deleted_at IS NULL AND id > $1
             ORDER BY id
             LIMIT $2",
        )
        .bind(self.last_id)
        .bind(batch_size)
        .fetch_all(&mut *self.transaction)
        .await
        .map_err(db_error)?;

        self.last_id = rows.last().map(|row| row.0).unwrap_or(self.last_id);
        Ok(rows
            .into_iter()
            .map(|(_, pid, username, created_at, is_verified, roles)| {
                let mut fields = serde_json::Map::new();
                fields.insert("username".to_string(), serde_json::json!(username));
                fields.insert("created_at".to_string(), serde_json::json!(created_at));
                fields.insert("is_verified".to_string(), serde_json::json!(is_verified));
                fields.insert("roles".to_string(), serde_json::json!(roles));
                SearchDocument {
                    id: pid.to_string(),
                    fields,
                }
            })
            .collect())
    }

    pub async fn commit(self) -> ApplicationResult<()> {
        self.transaction.commit().await.map_err(db_error)
    }
}

#[cfg(feature = "db-postgres")]
fn db_error(error: sqlx::Error) -> ApplicationError {
    ApplicationError::Infrastructure(error.to_string())
}
