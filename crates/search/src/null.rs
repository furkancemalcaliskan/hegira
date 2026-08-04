use crate::{SearchDocument, SearchError, SearchIndex, SearchQuery, SearchResults};

#[derive(Debug, Clone, Default)]
pub struct NullSearch;

impl SearchIndex for NullSearch {
    type Error = SearchError;

    async fn upsert(
        &self,
        _index: &str,
        _documents: Vec<SearchDocument>,
    ) -> Result<(), SearchError> {
        Ok(())
    }

    async fn delete(&self, _index: &str, _document_id: &str) -> Result<(), SearchError> {
        Ok(())
    }

    async fn clear(&self, _index: &str) -> Result<(), SearchError> {
        Ok(())
    }

    async fn search(
        &self,
        _index: &str,
        _query: SearchQuery,
    ) -> Result<SearchResults, SearchError> {
        Ok(SearchResults {
            hits: Vec::new(),
            estimated_total_hits: 0,
        })
    }
}
