use application::shared::{
    errors::ApplicationResult,
    search::{SearchDocument, SearchIndex, SearchQuery, SearchResults},
};

#[derive(Debug, Clone, Default)]
pub struct NullSearch;

impl SearchIndex for NullSearch {
    async fn upsert(&self, _index: &str, _documents: Vec<SearchDocument>) -> ApplicationResult<()> {
        Ok(())
    }

    async fn delete(&self, _index: &str, _document_id: &str) -> ApplicationResult<()> {
        Ok(())
    }

    async fn clear(&self, _index: &str) -> ApplicationResult<()> {
        Ok(())
    }

    async fn search(&self, _index: &str, _query: SearchQuery) -> ApplicationResult<SearchResults> {
        Ok(SearchResults {
            hits: Vec::new(),
            estimated_total_hits: 0,
        })
    }
}
