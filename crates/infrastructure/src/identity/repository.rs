use domain_shared::common::errors::DomainError;
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct SqlxIdentityRepository {
    pub(crate) pool: PgPool,
}

impl SqlxIdentityRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) async fn user_id_by_username(&self, username: &str) -> Result<i32, DomainError> {
        sqlx::query_scalar::<_, i32>(
            "SELECT id FROM users WHERE username = $1 AND deleted_at IS NULL",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await
        .map_err(|err| DomainError::Validation(err.to_string()))?
        .ok_or_else(|| DomainError::NotFound("User not found".to_string()))
    }
}
