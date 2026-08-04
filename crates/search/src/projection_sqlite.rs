use std::sync::Arc;

use crate::SearchIndex;
use background_jobs::{DurableJobFuture, DurableJobHandler};
use sqlx::SqlitePool;

use super::{
    SearchAdapter,
    jobs::{SEARCH_INDEX_JOB, SearchIndexCommand},
};

pub struct SqliteSearchIndexJobHandler {
    search: Arc<SearchAdapter>,
    pool: SqlitePool,
}

impl SqliteSearchIndexJobHandler {
    pub fn new(search: Arc<SearchAdapter>, pool: SqlitePool) -> Self {
        Self { search, pool }
    }

    async fn apply_revisioned(
        &self,
        index: &str,
        document_id: &str,
        revision: i64,
        operation: SearchIndexCommand,
    ) -> Result<bool, String> {
        let mut tx = self.pool.begin().await.map_err(|error| error.to_string())?;
        let applied: Option<i64> = sqlx::query_scalar(
            "SELECT revision FROM search_projection_versions WHERE index_name = ?1 AND document_id = ?2",
        ).bind(index).bind(document_id).fetch_optional(&mut *tx).await.map_err(|error| error.to_string())?;
        if applied.is_some_and(|applied| applied >= revision) {
            tx.commit().await.map_err(|error| error.to_string())?;
            return Ok(false);
        }
        match operation {
            SearchIndexCommand::Upsert { documents, .. } => self
                .search
                .upsert(index, documents)
                .await
                .map_err(|error| error.to_string())?,
            SearchIndexCommand::Delete { document_id, .. } => self
                .search
                .delete(index, &document_id)
                .await
                .map_err(|error| error.to_string())?,
        }
        sqlx::query(
            "INSERT INTO search_projection_versions (index_name, document_id, revision, updated_at)
             VALUES (?1, ?2, ?3, ?4) ON CONFLICT (index_name, document_id) DO UPDATE SET
             revision = excluded.revision, updated_at = excluded.updated_at",
        )
        .bind(index)
        .bind(document_id)
        .bind(revision)
        .bind(chrono::Utc::now())
        .execute(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
        tx.commit().await.map_err(|error| error.to_string())?;
        Ok(true)
    }
}

impl DurableJobHandler for SqliteSearchIndexJobHandler {
    fn name(&self) -> &'static str {
        SEARCH_INDEX_JOB
    }

    fn handle(&self, payload: serde_json::Value) -> DurableJobFuture<'_> {
        Box::pin(async move {
            let command: SearchIndexCommand = serde_json::from_value(payload)
                .map_err(|error| format!("invalid search index command: {error}"))?;
            match command {
                SearchIndexCommand::Upsert {
                    index,
                    documents,
                    revision: Some(revision),
                } if documents.len() == 1 => {
                    let document_id = documents[0].id.clone();
                    self.apply_revisioned(
                        &index,
                        &document_id,
                        revision,
                        SearchIndexCommand::Upsert {
                            index: index.clone(),
                            documents,
                            revision: None,
                        },
                    )
                    .await
                    .map(|_| ())
                }
                SearchIndexCommand::Delete {
                    index,
                    document_id,
                    revision: Some(revision),
                } => self
                    .apply_revisioned(
                        &index,
                        &document_id,
                        revision,
                        SearchIndexCommand::Delete {
                            index: index.clone(),
                            document_id: document_id.clone(),
                            revision: None,
                        },
                    )
                    .await
                    .map(|_| ()),
                SearchIndexCommand::Upsert {
                    index, documents, ..
                } => self
                    .search
                    .upsert(&index, documents)
                    .await
                    .map_err(|error| error.to_string()),
                SearchIndexCommand::Delete {
                    index, document_id, ..
                } => self
                    .search
                    .delete(&index, &document_id)
                    .await
                    .map_err(|error| error.to_string()),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NullSearch;

    #[tokio::test]
    async fn sqlite_projection_rejects_stale_revisions_and_advances_monotonically() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE search_projection_versions (
                index_name TEXT NOT NULL,
                document_id TEXT NOT NULL,
                revision INTEGER NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (index_name, document_id)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        let handler = SqliteSearchIndexJobHandler::new(
            Arc::new(SearchAdapter::Null(NullSearch)),
            pool.clone(),
        );
        for revision in [3, 2, 4] {
            handler
                .handle(
                    serde_json::to_value(SearchIndexCommand::Delete {
                        index: "identity_users".to_string(),
                        document_id: "user-1".to_string(),
                        revision: Some(revision),
                    })
                    .unwrap(),
                )
                .await
                .unwrap();
        }
        let revision: i64 = sqlx::query_scalar("SELECT revision FROM search_projection_versions WHERE index_name = 'identity_users' AND document_id = 'user-1'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(revision, 4);
    }
}
