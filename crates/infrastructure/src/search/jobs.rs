#[cfg(feature = "db-postgres")]
use super::{SEARCH_REBUILD_LOCK, SearchAdapter};
#[cfg(feature = "db-postgres")]
use crate::jobs::{JobObserver, NoopJobObserver};
#[cfg(feature = "db-postgres")]
use application::shared::{
    jobs::{DurableJobFuture, DurableJobHandler},
    search::SearchIndex,
};
use application::shared::{
    jobs::{DurableJobOptions, DurableJobQueue},
    search::SearchDocument,
};
use serde::{Deserialize, Serialize};
#[cfg(feature = "db-postgres")]
use sqlx::PgPool;
#[cfg(feature = "db-postgres")]
use std::sync::Arc;

pub const SEARCH_INDEX_JOB: &str = "search.index.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum SearchIndexCommand {
    Upsert {
        index: String,
        documents: Vec<SearchDocument>,
        #[serde(default)]
        revision: Option<i64>,
    },
    Delete {
        index: String,
        document_id: String,
        #[serde(default)]
        revision: Option<i64>,
    },
}

pub async fn enqueue<Q>(
    queue: &Q,
    command: SearchIndexCommand,
    idempotency_key: Option<String>,
) -> Result<uuid::Uuid, String>
where
    Q: DurableJobQueue,
{
    let payload = serde_json::to_value(command)
        .map_err(|err| format!("failed to serialize search index command: {err}"))?;
    queue
        .enqueue(
            SEARCH_INDEX_JOB,
            payload,
            DurableJobOptions {
                idempotency_key,
                max_attempts: 5,
            },
        )
        .await
}

#[cfg(feature = "db-postgres")]
pub struct SearchIndexJobHandler {
    search: Arc<SearchAdapter>,
    pool: PgPool,
    observer: Arc<dyn JobObserver>,
}

#[cfg(feature = "db-postgres")]
impl SearchIndexJobHandler {
    pub fn new(search: Arc<SearchAdapter>, pool: PgPool) -> Self {
        Self {
            search,
            pool,
            observer: Arc::new(NoopJobObserver),
        }
    }

    pub fn with_observer(mut self, observer: Arc<dyn JobObserver>) -> Self {
        self.observer = observer;
        self
    }

    async fn apply_revisioned(
        &self,
        index: &str,
        document_id: &str,
        revision: i64,
        operation: SearchIndexCommand,
    ) -> Result<bool, String> {
        let mut transaction = self.pool.begin().await.map_err(|err| err.to_string())?;
        sqlx::query("SELECT pg_advisory_xact_lock_shared(hashtextextended($1, 0))")
            .bind(SEARCH_REBUILD_LOCK)
            .execute(&mut *transaction)
            .await
            .map_err(|err| err.to_string())?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("{index}:{document_id}"))
            .execute(&mut *transaction)
            .await
            .map_err(|err| err.to_string())?;
        let applied = sqlx::query_scalar::<_, i64>(
            "SELECT revision FROM search_projection_versions
             WHERE index_name = $1 AND document_id = $2",
        )
        .bind(index)
        .bind(document_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|err| err.to_string())?;
        if applied.is_some_and(|applied| applied >= revision) {
            transaction.commit().await.map_err(|err| err.to_string())?;
            return Ok(false);
        }

        match operation {
            SearchIndexCommand::Upsert { documents, .. } => self
                .search
                .upsert(index, documents)
                .await
                .map_err(|err| err.to_string())?,
            SearchIndexCommand::Delete { document_id, .. } => self
                .search
                .delete(index, &document_id)
                .await
                .map_err(|err| err.to_string())?,
        }
        sqlx::query(
            "INSERT INTO search_projection_versions (index_name, document_id, revision)
             VALUES ($1, $2, $3)
             ON CONFLICT (index_name, document_id) DO UPDATE
             SET revision = EXCLUDED.revision, updated_at = NOW()",
        )
        .bind(index)
        .bind(document_id)
        .bind(revision)
        .execute(&mut *transaction)
        .await
        .map_err(|err| err.to_string())?;
        transaction
            .commit()
            .await
            .map(|_| true)
            .map_err(|err| err.to_string())
    }

    fn observe_revisioned(&self, result: &Result<bool, String>) {
        self.observer.search_projection(match result {
            Ok(true) => "applied",
            Ok(false) => "stale",
            Err(_) => "failed",
        });
    }
}

#[cfg(feature = "db-postgres")]
impl DurableJobHandler for SearchIndexJobHandler {
    fn name(&self) -> &'static str {
        SEARCH_INDEX_JOB
    }

    fn handle(&self, payload: serde_json::Value) -> DurableJobFuture<'_> {
        Box::pin(async move {
            let command = serde_json::from_value::<SearchIndexCommand>(payload)
                .map_err(|err| format!("invalid search index command: {err}"))?;
            match command {
                SearchIndexCommand::Upsert {
                    index,
                    documents,
                    revision,
                } => {
                    if let Some(revision) = revision
                        && documents.len() == 1
                    {
                        let document_id = documents[0].id.clone();
                        let result = self
                            .apply_revisioned(
                                &index,
                                &document_id,
                                revision,
                                SearchIndexCommand::Upsert {
                                    index: index.clone(),
                                    documents,
                                    revision: None,
                                },
                            )
                            .await;
                        self.observe_revisioned(&result);
                        return result.map(|_| ());
                    }
                    self.search
                        .upsert(&index, documents)
                        .await
                        .map_err(|err| err.to_string())
                }
                SearchIndexCommand::Delete {
                    index,
                    document_id,
                    revision,
                } => {
                    if let Some(revision) = revision {
                        let result = self
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
                            .await;
                        self.observe_revisioned(&result);
                        return result.map(|_| ());
                    }
                    self.search
                        .delete(&index, &document_id)
                        .await
                        .map_err(|err| err.to_string())
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_payload_has_stable_revisioned_contract() {
        let payload = serde_json::to_value(SearchIndexCommand::Delete {
            index: "records".to_string(),
            document_id: "record-1".to_string(),
            revision: Some(3),
        })
        .unwrap();

        assert_eq!(payload["operation"], "delete");
        assert_eq!(payload["index"], "records");
        assert_eq!(payload["document_id"], "record-1");
        assert_eq!(payload["revision"], 3);
    }
}
