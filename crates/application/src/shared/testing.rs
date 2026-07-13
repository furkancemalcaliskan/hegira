use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::shared::{
    audit::{AuditLogEntry, AuditLogger},
    cache::Cache,
    errors::ApplicationResult,
    jobs::{Job, JobDispatcher},
    mail::{MailMessage, Mailer},
    settings::{SettingKey, SettingsProvider},
    storage::{Storage, StoragePath, StoredObject},
};

#[derive(Debug, Clone, Default)]
pub struct RecordingMailer {
    messages: Arc<Mutex<Vec<MailMessage>>>,
}

impl RecordingMailer {
    pub fn messages(&self) -> Vec<MailMessage> {
        self.messages.lock().expect("mailer mutex poisoned").clone()
    }
}

impl Mailer for RecordingMailer {
    async fn send(&self, message: MailMessage) -> ApplicationResult<()> {
        self.messages
            .lock()
            .expect("mailer mutex poisoned")
            .push(message);
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct RecordingAuditLogger {
    entries: Arc<Mutex<Vec<AuditLogEntry>>>,
}

impl RecordingAuditLogger {
    pub fn entries(&self) -> Vec<AuditLogEntry> {
        self.entries.lock().expect("audit mutex poisoned").clone()
    }
}

impl AuditLogger for RecordingAuditLogger {
    async fn record(&self, entry: AuditLogEntry) -> ApplicationResult<()> {
        self.entries
            .lock()
            .expect("audit mutex poisoned")
            .push(entry);
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct CacheValue {
    value: String,
    expires_at: Option<Instant>,
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryCache {
    values: Arc<Mutex<HashMap<String, CacheValue>>>,
}

impl Cache for InMemoryCache {
    async fn get_string(&self, key: &str) -> ApplicationResult<Option<String>> {
        let mut values = self.values.lock().expect("cache mutex poisoned");
        if values
            .get(key)
            .and_then(|entry| entry.expires_at)
            .is_some_and(|expires_at| expires_at <= Instant::now())
        {
            values.remove(key);
        }
        Ok(values.get(key).map(|entry| entry.value.clone()))
    }

    async fn set_string(
        &self,
        key: &str,
        value: String,
        ttl: Option<Duration>,
    ) -> ApplicationResult<()> {
        self.values.lock().expect("cache mutex poisoned").insert(
            key.to_string(),
            CacheValue {
                value,
                expires_at: ttl.map(|ttl| Instant::now() + ttl),
            },
        );
        Ok(())
    }

    async fn remove(&self, key: &str) -> ApplicationResult<()> {
        self.values
            .lock()
            .expect("cache mutex poisoned")
            .remove(key);
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryStorage {
    objects: Arc<Mutex<HashMap<String, StoredObject>>>,
}

impl InMemoryStorage {
    pub fn paths(&self) -> Vec<String> {
        let mut paths = self
            .objects
            .lock()
            .expect("storage mutex poisoned")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }
}

impl Storage for InMemoryStorage {
    async fn put(
        &self,
        path: &StoragePath,
        bytes: Vec<u8>,
        content_type: Option<String>,
    ) -> ApplicationResult<()> {
        self.objects.lock().expect("storage mutex poisoned").insert(
            path.as_str().to_string(),
            StoredObject {
                bytes,
                content_type,
            },
        );
        Ok(())
    }

    async fn get(&self, path: &StoragePath) -> ApplicationResult<Option<StoredObject>> {
        Ok(self
            .objects
            .lock()
            .expect("storage mutex poisoned")
            .get(path.as_str())
            .cloned())
    }

    async fn delete(&self, path: &StoragePath) -> ApplicationResult<()> {
        self.objects
            .lock()
            .expect("storage mutex poisoned")
            .remove(path.as_str());
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct InMemorySettings {
    values: Arc<Mutex<HashMap<String, serde_json::Value>>>,
}

impl SettingsProvider for InMemorySettings {
    async fn get_json(&self, key: &SettingKey) -> ApplicationResult<Option<serde_json::Value>> {
        Ok(self
            .values
            .lock()
            .expect("settings mutex poisoned")
            .get(key.as_str())
            .cloned())
    }

    async fn set_json(&self, key: &SettingKey, value: serde_json::Value) -> ApplicationResult<()> {
        self.values
            .lock()
            .expect("settings mutex poisoned")
            .insert(key.as_str().to_string(), value);
        Ok(())
    }

    async fn remove(&self, key: &SettingKey) -> ApplicationResult<()> {
        self.values
            .lock()
            .expect("settings mutex poisoned")
            .remove(key.as_str());
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct RecordingJobDispatcher {
    jobs: Arc<Mutex<Vec<&'static str>>>,
}

impl RecordingJobDispatcher {
    pub fn jobs(&self) -> Vec<&'static str> {
        self.jobs.lock().expect("jobs mutex poisoned").clone()
    }
}

impl JobDispatcher for RecordingJobDispatcher {
    fn dispatch<J, Args>(&self, job: J, _args: Args)
    where
        J: Job<Args>,
        Args: Send + 'static,
    {
        self.jobs
            .lock()
            .expect("jobs mutex poisoned")
            .push(job.name());
    }
}
