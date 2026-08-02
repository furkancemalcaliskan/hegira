use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use audit::{AuditLogEntry, AuditLogger};
use background_jobs::{Job, JobDispatcher};
use cache::Cache;
use mail::{MailMessage, Mailer};
use settings::{SettingKey, SettingsProvider};
use storage::{Storage, StoragePath, StoredObject};

#[derive(Debug)]
pub struct TestSupportError(String);

impl std::fmt::Display for TestSupportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for TestSupportError {}

impl From<serde_json::Error> for TestSupportError {
    fn from(error: serde_json::Error) -> Self {
        Self(error.to_string())
    }
}

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
    type Error = TestSupportError;

    async fn send(&self, message: MailMessage) -> Result<(), Self::Error> {
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
    type Error = TestSupportError;

    async fn record(&self, entry: AuditLogEntry) -> Result<(), Self::Error> {
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
    type Error = TestSupportError;

    async fn get_string(&self, key: &str) -> Result<Option<String>, Self::Error> {
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
    ) -> Result<(), Self::Error> {
        self.values.lock().expect("cache mutex poisoned").insert(
            key.to_string(),
            CacheValue {
                value,
                expires_at: ttl.map(|ttl| Instant::now() + ttl),
            },
        );
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<(), Self::Error> {
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
    type Error = TestSupportError;

    async fn put(
        &self,
        path: &StoragePath,
        bytes: Vec<u8>,
        content_type: Option<String>,
    ) -> Result<(), Self::Error> {
        self.objects.lock().expect("storage mutex poisoned").insert(
            path.as_str().to_string(),
            StoredObject {
                bytes,
                content_type,
            },
        );
        Ok(())
    }

    async fn get(&self, path: &StoragePath) -> Result<Option<StoredObject>, Self::Error> {
        Ok(self
            .objects
            .lock()
            .expect("storage mutex poisoned")
            .get(path.as_str())
            .cloned())
    }

    async fn delete(&self, path: &StoragePath) -> Result<(), Self::Error> {
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
    type Error = TestSupportError;

    async fn get_json(&self, key: &SettingKey) -> Result<Option<serde_json::Value>, Self::Error> {
        Ok(self
            .values
            .lock()
            .expect("settings mutex poisoned")
            .get(key.as_str())
            .cloned())
    }

    async fn set_json(
        &self,
        key: &SettingKey,
        value: serde_json::Value,
    ) -> Result<(), Self::Error> {
        self.values
            .lock()
            .expect("settings mutex poisoned")
            .insert(key.as_str().to_string(), value);
        Ok(())
    }

    async fn remove(&self, key: &SettingKey) -> Result<(), Self::Error> {
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
