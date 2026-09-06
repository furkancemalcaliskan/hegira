use serde::{Deserialize, Serialize};
use std::{fmt, future::Future};

pub mod jobs;
#[cfg(feature = "meilisearch")]
mod meilisearch;
mod null;
#[cfg(feature = "db-sqlite")]
pub mod projection_sqlite;

#[cfg(feature = "meilisearch")]
pub use meilisearch::MeilisearchAdapter;
pub use null::NullSearch;

pub const SEARCH_REBUILD_LOCK: &str = "search:rebuild";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchBackend {
    Null,
    Meilisearch,
}

#[derive(Clone, PartialEq, Eq)]
pub struct MeilisearchSettings {
    pub url: String,
    pub api_key: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SearchSettings {
    pub enabled: bool,
    pub backend: SearchBackend,
    pub index_prefix: String,
    pub task_timeout_milliseconds: u64,
    pub meilisearch: MeilisearchSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchIndexSettings {
    pub searchable_attributes: Vec<String>,
    pub filterable_attributes: Vec<String>,
    pub sortable_attributes: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum SearchAdapter {
    Null(NullSearch),
    #[cfg(feature = "meilisearch")]
    Meilisearch(MeilisearchAdapter),
}

impl SearchAdapter {
    pub fn from_settings(settings: &SearchSettings) -> Result<Self, SearchError> {
        if !settings.enabled {
            return Ok(Self::Null(NullSearch));
        }

        match settings.backend {
            SearchBackend::Null => Ok(Self::Null(NullSearch)),
            SearchBackend::Meilisearch => build_meilisearch(settings),
        }
    }

    pub async fn health_check(&self) -> Result<(), SearchError> {
        match self {
            Self::Null(_) => Ok(()),
            #[cfg(feature = "meilisearch")]
            Self::Meilisearch(search) => search.health_check().await,
        }
    }

    pub async fn initialize_index(
        &self,
        index: &str,
        settings: &SearchIndexSettings,
    ) -> Result<(), SearchError> {
        let _ = (index, settings);
        match self {
            Self::Null(_) => Ok(()),
            #[cfg(feature = "meilisearch")]
            Self::Meilisearch(search) => search.ensure_index(index, settings).await,
        }
    }

    #[cfg(feature = "db-postgres")]
    pub async fn prepare_rebuild(
        &self,
        live: &str,
        temporary: &str,
        settings: &SearchIndexSettings,
    ) -> Result<(), SearchError> {
        let _ = (live, temporary, settings);
        match self {
            Self::Null(_) => Ok(()),
            #[cfg(feature = "meilisearch")]
            Self::Meilisearch(search) => search.prepare_rebuild(live, temporary, settings).await,
        }
    }

    #[cfg(feature = "db-postgres")]
    pub async fn promote_rebuild(&self, live: &str, temporary: &str) -> Result<bool, SearchError> {
        let _ = (live, temporary);
        match self {
            Self::Null(_) => Ok(true),
            #[cfg(feature = "meilisearch")]
            Self::Meilisearch(search) => search.promote_rebuild(live, temporary).await,
        }
    }
}

impl SearchIndex for SearchAdapter {
    type Error = SearchError;

    async fn upsert(&self, index: &str, documents: Vec<SearchDocument>) -> Result<(), SearchError> {
        match self {
            Self::Null(search) => search.upsert(index, documents).await,
            #[cfg(feature = "meilisearch")]
            Self::Meilisearch(search) => search.upsert(index, documents).await,
        }
    }

    async fn delete(&self, index: &str, document_id: &str) -> Result<(), SearchError> {
        match self {
            Self::Null(search) => search.delete(index, document_id).await,
            #[cfg(feature = "meilisearch")]
            Self::Meilisearch(search) => search.delete(index, document_id).await,
        }
    }

    async fn clear(&self, index: &str) -> Result<(), SearchError> {
        match self {
            Self::Null(search) => search.clear(index).await,
            #[cfg(feature = "meilisearch")]
            Self::Meilisearch(search) => search.clear(index).await,
        }
    }

    async fn search(&self, index: &str, query: SearchQuery) -> Result<SearchResults, SearchError> {
        match self {
            Self::Null(search) => search.search(index, query).await,
            #[cfg(feature = "meilisearch")]
            Self::Meilisearch(search) => search.search(index, query).await,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchError(String);

impl SearchError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for SearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SearchError {}

#[cfg(feature = "meilisearch")]
fn build_meilisearch(settings: &SearchSettings) -> Result<SearchAdapter, SearchError> {
    MeilisearchAdapter::new(settings).map(SearchAdapter::Meilisearch)
}

#[cfg(not(feature = "meilisearch"))]
fn build_meilisearch(_settings: &SearchSettings) -> Result<SearchAdapter, SearchError> {
    Err(SearchError::new(
        "Meilisearch support is not compiled into this binary",
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchDocument {
    pub id: String,
    #[serde(flatten)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub text: String,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchResults {
    pub hits: Vec<serde_json::Value>,
    pub estimated_total_hits: usize,
}

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

pub trait SearchIndex: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn upsert(
        &self,
        index: &str,
        documents: Vec<SearchDocument>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn delete(
        &self,
        index: &str,
        document_id: &str,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn clear(&self, index: &str) -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn search(
        &self,
        index: &str,
        query: SearchQuery,
    ) -> impl Future<Output = Result<SearchResults, Self::Error>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_search_does_not_require_the_selected_provider() {
        let adapter = SearchAdapter::from_settings(&SearchSettings {
            enabled: false,
            backend: SearchBackend::Meilisearch,
            index_prefix: "test".to_string(),
            task_timeout_milliseconds: 1_000,
            meilisearch: MeilisearchSettings {
                url: "http://127.0.0.1:7700".to_string(),
                api_key: None,
            },
        })
        .unwrap();

        assert!(matches!(adapter, SearchAdapter::Null(_)));
    }

    #[cfg(not(feature = "meilisearch"))]
    #[test]
    fn enabled_uncompiled_meilisearch_fails_before_initialization() {
        let error = SearchAdapter::from_settings(&SearchSettings {
            enabled: true,
            backend: SearchBackend::Meilisearch,
            index_prefix: "test".to_string(),
            task_timeout_milliseconds: 1_000,
            meilisearch: MeilisearchSettings {
                url: "http://unreachable.invalid:7700".to_string(),
                api_key: None,
            },
        })
        .err()
        .expect("an unavailable compiled capability should fail");

        assert_eq!(
            error.to_string(),
            "Meilisearch support is not compiled into this binary"
        );
    }
}
