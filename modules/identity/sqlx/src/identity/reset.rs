use persistence::DatabasePool;

pub async fn reset_identity_schema(pool: &DatabasePool) -> Result<(), sqlx::Error> {
    match pool {
        #[cfg(feature = "db-postgres")]
        DatabasePool::Postgres(pool) => reset_postgres(pool).await,
        #[cfg(feature = "db-sqlite")]
        DatabasePool::Sqlite(pool) => reset_sqlite(pool).await,
    }
}

#[cfg(feature = "db-sqlite")]
async fn reset_sqlite(pool: &sqlx::SqlitePool) -> Result<(), sqlx::Error> {
    let mut connection = pool.acquire().await?;
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *connection)
        .await?;
    for table in [
        "oauth_pending_signups",
        "user_oauth_connections",
        "oauth_states",
        "sessions",
        "role_permissions",
        "user_roles",
        "permissions",
        "roles",
        "users",
    ] {
        sqlx::query(&format!("DROP TABLE IF EXISTS {table}"))
            .execute(&mut *connection)
            .await?;
    }
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *connection)
        .await?;
    Ok(())
}

#[cfg(feature = "db-postgres")]
async fn reset_postgres(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    for table in [
        "oauth_pending_signups",
        "user_oauth_connections",
        "oauth_states",
        "role_permissions",
        "user_roles",
        "permissions",
        "roles",
        "sessions",
        "users",
    ] {
        sqlx::query(&format!("DROP TABLE IF EXISTS {table}"))
            .execute(pool)
            .await?;
    }
    Ok(())
}
