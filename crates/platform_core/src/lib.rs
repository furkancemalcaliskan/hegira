#[derive(Debug, Clone, Copy, Default)]
pub struct CompiledCapabilities {
    pub db_postgres: bool,
    pub db_sqlite: bool,
    pub cache_redis: bool,
    pub mailer_smtp: bool,
    pub storage_s3: bool,
    pub search_meilisearch: bool,
    pub metrics_prometheus: bool,
    pub otel_otlp: bool,
    pub openapi: bool,
}
