use crate::composition::services::AppServices;
use infrastructure::{
    cache::CacheAdapter, config::AppConfig, search::SearchAdapter, settings::SettingsAdapter,
    storage::StorageAdapter,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub db: infrastructure::db::DatabasePool,
    pub cache: Arc<CacheAdapter>,
    pub settings: Arc<SettingsAdapter>,
    pub storage: Arc<StorageAdapter>,
    pub search: Arc<SearchAdapter>,
    pub services: Arc<AppServices>,
}

impl AppState {
    pub fn new(
        config: AppConfig,
        db: infrastructure::db::DatabasePool,
        cache: CacheAdapter,
        storage: StorageAdapter,
        search: SearchAdapter,
        settings: SettingsAdapter,
    ) -> Self {
        let services = Arc::new(AppServices::new(
            db.clone(),
            &config,
            cache.clone(),
            search.clone(),
        ));
        Self {
            config: Arc::new(config),
            db,
            cache: Arc::new(cache),
            settings: Arc::new(settings),
            storage: Arc::new(storage),
            search: Arc::new(search),
            services,
        }
    }
}
