use crate::{
    SearchDocument, SearchError, SearchIndex, SearchIndexSettings, SearchQuery, SearchResults,
    SearchSettings,
};
use reqwest::{Client, Method, Url};
use serde::Deserialize;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct MeilisearchAdapter {
    client: Client,
    base_url: Url,
    api_key: Option<String>,
    index_prefix: String,
    task_timeout: Duration,
}

impl std::fmt::Debug for MeilisearchAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MeilisearchAdapter")
            .field("base_url", &self.base_url)
            .field("index_prefix", &self.index_prefix)
            .field("task_timeout", &self.task_timeout)
            .finish_non_exhaustive()
    }
}

impl MeilisearchAdapter {
    pub fn new(config: &SearchSettings) -> Result<Self, SearchError> {
        let base_url = Url::parse(&config.meilisearch.url)
            .map_err(|err| SearchError::new(format!("invalid Meilisearch URL: {err}")))?;
        Ok(Self {
            client: Client::new(),
            base_url,
            api_key: config.meilisearch.api_key.clone(),
            index_prefix: config.index_prefix.clone(),
            task_timeout: Duration::from_millis(config.task_timeout_milliseconds),
        })
    }

    pub async fn health_check(&self) -> Result<(), SearchError> {
        self.request(Method::GET, &["health"])
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map(|_| ())
            .map_err(|err| SearchError::new(format!("Meilisearch health probe failed: {err}")))
    }

    pub async fn prepare_rebuild(
        &self,
        live: &str,
        temporary: &str,
        settings: &SearchIndexSettings,
    ) -> Result<(), SearchError> {
        let live = self.physical_index(live)?;
        let temporary = self.physical_index(temporary)?;
        self.ensure_physical_index(&live).await?;
        self.delete_index_if_exists(&temporary).await?;
        self.create_index(&temporary).await?;
        self.apply_settings(&temporary, settings).await
    }

    pub async fn ensure_index(
        &self,
        index: &str,
        settings: &SearchIndexSettings,
    ) -> Result<(), SearchError> {
        let index = self.physical_index(index)?;
        self.ensure_physical_index(&index).await?;
        self.apply_settings(&index, settings).await
    }

    pub async fn promote_rebuild(&self, live: &str, temporary: &str) -> Result<bool, SearchError> {
        let live = self.physical_index(live)?;
        let temporary = self.physical_index(temporary)?;
        let response = self
            .request(Method::POST, &["swap-indexes"])
            .json(&serde_json::json!([{ "indexes": [live, temporary] }]))
            .send()
            .await
            .map_err(search_error)?;
        let task_uid = self.task_uid(response).await?;
        match self.wait_for_swap(task_uid).await? {
            true => {
                self.delete_index_if_exists(&temporary).await?;
                Ok(true)
            }
            false => Ok(false),
        }
    }

    async fn ensure_physical_index(&self, index: &str) -> Result<(), SearchError> {
        if self.index_exists(index).await? {
            return Ok(());
        }
        self.create_index(index).await
    }

    async fn index_exists(&self, index: &str) -> Result<bool, SearchError> {
        let response = self
            .request(Method::GET, &["indexes", index])
            .send()
            .await
            .map_err(search_error)?;
        if response.status().is_success() {
            return Ok(true);
        }
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }
        Err(search_error(response.error_for_status().unwrap_err()))
    }

    async fn create_index(&self, index: &str) -> Result<(), SearchError> {
        let response = self
            .request(Method::POST, &["indexes"])
            .json(&serde_json::json!({ "uid": index, "primaryKey": "id" }))
            .send()
            .await
            .map_err(search_error)?;
        let task_uid = self.task_uid(response).await?;
        match self.wait(task_uid).await {
            Ok(()) => Ok(()),
            Err(_) if self.index_exists(index).await? => Ok(()),
            Err(error) => Err(error),
        }
    }

    async fn delete_index_if_exists(&self, index: &str) -> Result<(), SearchError> {
        let response = self
            .request(Method::DELETE, &["indexes", index])
            .send()
            .await
            .map_err(search_error)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        let task_uid = self.task_uid(response).await?;
        self.wait(task_uid).await
    }

    async fn apply_settings(
        &self,
        index: &str,
        settings: &SearchIndexSettings,
    ) -> Result<(), SearchError> {
        let response = self
            .request(Method::PATCH, &["indexes", index, "settings"])
            .json(&serde_json::json!({
                "searchableAttributes": settings.searchable_attributes,
                "filterableAttributes": settings.filterable_attributes,
                "sortableAttributes": settings.sortable_attributes
            }))
            .send()
            .await
            .map_err(search_error)?;
        let task_uid = self.task_uid(response).await?;
        self.wait(task_uid).await
    }

    async fn wait_for_swap(&self, task_uid: u64) -> Result<bool, SearchError> {
        let started_at = Instant::now();
        loop {
            let task = self.task_status(task_uid).await?;
            match task.status.as_str() {
                "succeeded" => return Ok(true),
                "failed" | "canceled" => return Err(task_failure(task_uid, task)),
                _ if started_at.elapsed() >= self.task_timeout => return Ok(false),
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    }

    fn physical_index(&self, logical_name: &str) -> Result<String, SearchError> {
        if logical_name.is_empty()
            || !logical_name.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
        {
            return Err(SearchError::new(
                "search index name contains unsupported characters",
            ));
        }
        Ok(format!("{}_{}", self.index_prefix, logical_name))
    }

    fn request(&self, method: Method, segments: &[&str]) -> reqwest::RequestBuilder {
        let mut url = self.base_url.clone();
        {
            let mut path = url
                .path_segments_mut()
                .expect("validated HTTP Meilisearch URL should support path segments");
            path.pop_if_empty();
            path.extend(segments);
        }
        let request = self.client.request(method, url);
        if let Some(api_key) = self.api_key.as_deref() {
            request.bearer_auth(api_key)
        } else {
            request
        }
    }

    async fn task_uid(&self, response: reqwest::Response) -> Result<u64, SearchError> {
        response
            .error_for_status()
            .map_err(search_error)?
            .json::<TaskAccepted>()
            .await
            .map(|task| task.task_uid)
            .map_err(search_error)
    }

    async fn wait(&self, task_uid: u64) -> Result<(), SearchError> {
        let started_at = Instant::now();
        loop {
            let task = self.task_status(task_uid).await?;
            match task.status.as_str() {
                "succeeded" => return Ok(()),
                "failed" | "canceled" => return Err(task_failure(task_uid, task)),
                _ if started_at.elapsed() >= self.task_timeout => {
                    return Err(SearchError::new(format!(
                        "Meilisearch task {task_uid} timed out"
                    )));
                }
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    }

    async fn task_status(&self, task_uid: u64) -> Result<TaskStatus, SearchError> {
        self.request(Method::GET, &["tasks", &task_uid.to_string()])
            .send()
            .await
            .map_err(search_error)?
            .error_for_status()
            .map_err(search_error)?
            .json::<TaskStatus>()
            .await
            .map_err(search_error)
    }
}

impl SearchIndex for MeilisearchAdapter {
    type Error = SearchError;

    async fn upsert(&self, index: &str, documents: Vec<SearchDocument>) -> Result<(), SearchError> {
        if documents.is_empty() {
            return Ok(());
        }
        let physical_index = self.physical_index(index)?;
        let response = self
            .request(Method::POST, &["indexes", &physical_index, "documents"])
            .query(&[("primaryKey", "id")])
            .json(&documents)
            .send()
            .await
            .map_err(search_error)?;
        let task_uid = self.task_uid(response).await?;
        self.wait(task_uid).await
    }

    async fn delete(&self, index: &str, document_id: &str) -> Result<(), SearchError> {
        let physical_index = self.physical_index(index)?;
        let response = self
            .request(
                Method::DELETE,
                &["indexes", &physical_index, "documents", document_id],
            )
            .send()
            .await
            .map_err(search_error)?;
        let task_uid = self.task_uid(response).await?;
        self.wait(task_uid).await
    }

    async fn clear(&self, index: &str) -> Result<(), SearchError> {
        let physical_index = self.physical_index(index)?;
        let response = self
            .request(Method::DELETE, &["indexes", &physical_index, "documents"])
            .send()
            .await
            .map_err(search_error)?;
        let task_uid = self.task_uid(response).await?;
        self.wait(task_uid).await
    }

    async fn search(&self, index: &str, query: SearchQuery) -> Result<SearchResults, SearchError> {
        let physical_index = self.physical_index(index)?;
        let response = self
            .request(Method::POST, &["indexes", &physical_index, "search"])
            .json(&serde_json::json!({
                "q": query.text,
                "offset": query.offset,
                "limit": query.limit.clamp(1, 100),
            }))
            .send()
            .await
            .map_err(search_error)?
            .error_for_status()
            .map_err(search_error)?
            .json::<SearchResponse>()
            .await
            .map_err(search_error)?;
        Ok(SearchResults {
            hits: response.hits,
            estimated_total_hits: response.estimated_total_hits.unwrap_or(0),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskAccepted {
    task_uid: u64,
}

#[derive(Debug, Deserialize)]
struct TaskStatus {
    status: String,
    error: Option<TaskError>,
}

#[derive(Debug, Deserialize)]
struct TaskError {
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchResponse {
    hits: Vec<serde_json::Value>,
    estimated_total_hits: Option<usize>,
}

fn search_error(error: reqwest::Error) -> SearchError {
    SearchError::new(error.to_string())
}

fn task_failure(task_uid: u64, task: TaskStatus) -> SearchError {
    SearchError::new(
        task.error
            .and_then(|error| error.message)
            .unwrap_or_else(|| format!("Meilisearch task {task_uid} {}", task.status)),
    )
}
