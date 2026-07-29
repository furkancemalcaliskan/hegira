use application::shared::{
    audit::AuditLogEntry,
    errors::{ApplicationError, ApplicationResult},
};

#[cfg(feature = "db-postgres")]
pub(crate) async fn insert_postgres_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    entry: AuditLogEntry,
) -> ApplicationResult<()> {
    sqlx::query(
        "INSERT INTO audit_logs (actor, action, entity_type, entity_id, details)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(entry.actor)
    .bind(entry.action)
    .bind(entry.entity_type)
    .bind(entry.entity_id)
    .bind(entry.details)
    .execute(&mut **transaction)
    .await
    .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?;
    Ok(())
}

#[cfg(feature = "db-sqlite")]
pub(crate) async fn insert_sqlite_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    entry: AuditLogEntry,
) -> ApplicationResult<()> {
    sqlx::query(
        "INSERT INTO audit_logs
         (actor, action, entity_type, entity_id, details, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(entry.actor)
    .bind(entry.action)
    .bind(entry.entity_type)
    .bind(entry.entity_id)
    .bind(entry.details.to_string())
    .bind(chrono::Utc::now())
    .execute(&mut **transaction)
    .await
    .map_err(|error| ApplicationError::Infrastructure(error.to_string()))?;
    Ok(())
}
