use crate::shared::errors::ApplicationResult;
use serde::{Deserialize, Serialize};
use std::future::Future;

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

pub trait SearchIndex: Send + Sync {
    fn upsert(
        &self,
        index: &str,
        documents: Vec<SearchDocument>,
    ) -> impl Future<Output = ApplicationResult<()>> + Send;

    fn delete(
        &self,
        index: &str,
        document_id: &str,
    ) -> impl Future<Output = ApplicationResult<()>> + Send;

    fn clear(&self, index: &str) -> impl Future<Output = ApplicationResult<()>> + Send;

    fn search(
        &self,
        index: &str,
        query: SearchQuery,
    ) -> impl Future<Output = ApplicationResult<SearchResults>> + Send;
}
