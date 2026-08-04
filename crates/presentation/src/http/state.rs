use crate::composition::services::AppServices;
use cache::CacheAdapter;
use infrastructure::{config::AppConfig, settings::SettingsAdapter};
use mail::MailerAdapter;
use search::SearchAdapter;
use std::sync::Arc;
use storage::StorageAdapter;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub db: persistence::DatabasePool,
    pub cache: Arc<CacheAdapter>,
    pub settings: Arc<SettingsAdapter>,
    pub storage: Arc<StorageAdapter>,
    pub search: Arc<SearchAdapter>,
    pub services: Arc<AppServices>,
}

impl AppState {
    pub fn new(
        config: AppConfig,
        db: persistence::DatabasePool,
        cache: CacheAdapter,
        storage: StorageAdapter,
        search: SearchAdapter,
        mailer: MailerAdapter,
        settings: SettingsAdapter,
    ) -> Self {
        let services = Arc::new(AppServices::new(
            db.clone(),
            &config,
            cache.clone(),
            search.clone(),
            mailer,
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
