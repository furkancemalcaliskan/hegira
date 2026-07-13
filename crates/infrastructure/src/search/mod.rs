pub mod jobs;
#[cfg(feature = "search-meilisearch")]
pub mod meilisearch;
pub mod null;
#[cfg(feature = "db-sqlite")]
pub mod projection_sqlite;

use crate::config::{AppConfig, SearchBackend};
use application::shared::{
    errors::ApplicationResult,
    search::{SearchDocument, SearchIndex, SearchQuery, SearchResults},
};
use null::NullSearch;

pub const SEARCH_REBUILD_LOCK: &str = "search:rebuild";

#[derive(Debug, Clone)]
pub enum SearchAdapter {
    Null(NullSearch),
    #[cfg(feature = "search-meilisearch")]
    Meilisearch(meilisearch::MeilisearchAdapter),
}

impl SearchAdapter {
    pub fn from_config(config: &AppConfig) -> Result<Self, String> {
        if !config.search.enabled {
            return Ok(Self::Null(NullSearch));
        }
        match config.search.backend {
            SearchBackend::Null => Ok(Self::Null(NullSearch)),
            SearchBackend::Meilisearch => build_meilisearch(config),
        }
    }

    pub async fn health_check(&self) -> Result<(), String> {
        match self {
            Self::Null(_) => Ok(()),
            #[cfg(feature = "search-meilisearch")]
            Self::Meilisearch(search) => search.health_check().await,
        }
    }

    pub async fn initialize_indexes(&self) -> ApplicationResult<()> {
        match self {
            Self::Null(_) => Ok(()),
            #[cfg(feature = "search-meilisearch")]
            Self::Meilisearch(search) => search.ensure_identity_user_index("identity_users").await,
        }
    }

    #[cfg(feature = "db-postgres")]
    async fn prepare_rebuild(&self, live: &str, temporary: &str) -> ApplicationResult<()> {
        let _ = (live, temporary);
        match self {
            Self::Null(_) => Ok(()),
            #[cfg(feature = "search-meilisearch")]
            Self::Meilisearch(search) => search.prepare_rebuild(live, temporary).await,
        }
    }

    #[cfg(feature = "db-postgres")]
    async fn promote_rebuild(&self, live: &str, temporary: &str) -> ApplicationResult<bool> {
        let _ = (live, temporary);
        match self {
            Self::Null(_) => Ok(true),
            #[cfg(feature = "search-meilisearch")]
            Self::Meilisearch(search) => search.promote_rebuild(live, temporary).await,
        }
    }
}

impl SearchIndex for SearchAdapter {
    async fn upsert(&self, index: &str, documents: Vec<SearchDocument>) -> ApplicationResult<()> {
        match self {
            Self::Null(search) => search.upsert(index, documents).await,
            #[cfg(feature = "search-meilisearch")]
            Self::Meilisearch(search) => search.upsert(index, documents).await,
        }
    }

    async fn delete(&self, index: &str, document_id: &str) -> ApplicationResult<()> {
        match self {
            Self::Null(search) => search.delete(index, document_id).await,
            #[cfg(feature = "search-meilisearch")]
            Self::Meilisearch(search) => search.delete(index, document_id).await,
        }
    }

    async fn clear(&self, index: &str) -> ApplicationResult<()> {
        match self {
            Self::Null(search) => search.clear(index).await,
            #[cfg(feature = "search-meilisearch")]
            Self::Meilisearch(search) => search.clear(index).await,
        }
    }

    async fn search(&self, index: &str, query: SearchQuery) -> ApplicationResult<SearchResults> {
        match self {
            Self::Null(search) => search.search(index, query).await,
            #[cfg(feature = "search-meilisearch")]
            Self::Meilisearch(search) => search.search(index, query).await,
        }
    }
}

#[cfg(feature = "search-meilisearch")]
fn build_meilisearch(config: &AppConfig) -> Result<SearchAdapter, String> {
    meilisearch::MeilisearchAdapter::new(&config.search).map(SearchAdapter::Meilisearch)
}

#[cfg(not(feature = "search-meilisearch"))]
fn build_meilisearch(_config: &AppConfig) -> Result<SearchAdapter, String> {
    Err(
        "search.backend=meilisearch requires building with --features search-meilisearch"
            .to_string(),
    )
}

#[cfg(feature = "db-postgres")]
pub async fn reindex_identity_users(
    pool: &sqlx::PgPool,
    search: &SearchAdapter,
) -> Result<u64, String> {
    use application::shared::search::SearchIndex as _;

    const INDEX: &str = "identity_users";
    const TEMPORARY_INDEX: &str = "identity_users_reindex";
    const BATCH_SIZE: i64 = 500;
    let mut transaction = pool.begin().await.map_err(|err| err.to_string())?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *transaction)
        .await
        .map_err(|err| err.to_string())?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(SEARCH_REBUILD_LOCK)
        .execute(&mut *transaction)
        .await
        .map_err(|err| err.to_string())?;
    search
        .prepare_rebuild(INDEX, TEMPORARY_INDEX)
        .await
        .map_err(|err| err.to_string())?;

    let mut last_id = 0_i32;
    let mut indexed = 0_u64;
    loop {
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
        .bind(last_id)
        .bind(BATCH_SIZE)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|err| err.to_string())?;
        if rows.is_empty() {
            break;
        }

        last_id = rows.last().map(|row| row.0).unwrap_or(last_id);
        let documents = rows
            .into_iter()
            .map(|(_, pid, username, created_at, is_verified, roles)| {
                let mut fields = serde_json::Map::new();
                fields.insert("username".to_string(), serde_json::json!(username));
                fields.insert("created_at".to_string(), serde_json::json!(created_at));
                fields.insert("is_verified".to_string(), serde_json::json!(is_verified));
                fields.insert("roles".to_string(), serde_json::json!(roles));
                application::shared::search::SearchDocument {
                    id: pid.to_string(),
                    fields,
                }
            })
            .collect::<Vec<_>>();
        indexed += documents.len() as u64;
        search
            .upsert(TEMPORARY_INDEX, documents)
            .await
            .map_err(|err| err.to_string())?;
    }
    let confirmed = search
        .promote_rebuild(INDEX, TEMPORARY_INDEX)
        .await
        .map_err(|err| err.to_string())?;
    transaction.commit().await.map_err(|err| err.to_string())?;
    if !confirmed {
        tracing::warn!(
            index = INDEX,
            temporary_index = TEMPORARY_INDEX,
            "search index swap was enqueued but could not be confirmed before timeout"
        );
    }
    Ok(indexed)
}
