#![cfg(all(feature = "ssr", feature = "test-support"))]

use std::time::Duration;

use hegira::application::shared::{
    audit::{AuditLogEntry, AuditLogger},
    cache::Cache,
    jobs::{Job, JobDispatcher},
    mail::{MailAddress, MailMessage, Mailer},
    settings::{SettingKey, get_setting, set_setting},
    storage::{Storage, StoragePath},
};
use hegira::test_support::{
    InMemoryCache, InMemorySettings, InMemoryStorage, RecordingAuditLogger, RecordingJobDispatcher,
    RecordingMailer,
};

#[derive(Clone)]
struct ExampleJob;

impl Job<u32> for ExampleJob {
    fn name(&self) -> &'static str {
        "example_job"
    }

    async fn perform(&self, value: u32) -> Result<(), String> {
        assert_eq!(value, 42);
        Ok(())
    }
}

#[tokio::test]
async fn shared_test_doubles_cover_optional_capability_ports() {
    let mailer = RecordingMailer::default();
    mailer
        .send(MailMessage {
            to: MailAddress::new("customer@example.com"),
            subject: "Created".to_string(),
            text_body: "Created".to_string(),
            html_body: None,
        })
        .await
        .unwrap();
    assert_eq!(mailer.messages()[0].subject, "Created");

    let audit = RecordingAuditLogger::default();
    audit
        .record(AuditLogEntry::new(
            "admin@example.com",
            "test.records.create",
            "test.record",
            Some("record-id".to_string()),
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(audit.entries()[0].action, "test.records.create");

    let cache = InMemoryCache::default();
    cache
        .set_string("test:record:1", "value".to_string(), None)
        .await
        .unwrap();
    assert_eq!(
        cache.get_string("test:record:1").await.unwrap(),
        Some("value".to_string())
    );
    cache
        .set_string("test:expired", "value".to_string(), Some(Duration::ZERO))
        .await
        .unwrap();
    assert_eq!(cache.get_string("test:expired").await.unwrap(), None);

    let storage = InMemoryStorage::default();
    let path = StoragePath::from_segments(["test", "records", "attachment.bin"]).unwrap();
    storage
        .put(
            &path,
            vec![1, 2, 3],
            Some("application/octet-stream".to_string()),
        )
        .await
        .unwrap();
    assert_eq!(storage.paths(), vec![path.to_string()]);
    assert_eq!(
        storage.get(&path).await.unwrap().unwrap().bytes,
        vec![1, 2, 3]
    );

    let settings = InMemorySettings::default();
    let key = SettingKey::new("test.records.page_size").unwrap();
    set_setting(&settings, &key, &25_u32).await.unwrap();
    assert_eq!(
        get_setting::<u32, _>(&settings, &key).await.unwrap(),
        Some(25)
    );

    let jobs = RecordingJobDispatcher::default();
    jobs.dispatch(ExampleJob, 42);
    assert_eq!(jobs.jobs(), vec!["example_job"]);
}
