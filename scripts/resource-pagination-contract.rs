//! Repository-only runtime tests against the public CLI's ValidationRecord output.
use app_application::validation_record::{
    ValidationRecordAppService, ValidationRecordAuthorization, ValidationRecordIdGenerator,
    ValidationRecordRepository,
};
use app_application_contracts::validation_record::{
    LIST_PERMISSION, ListValidationRecordsQuery, ValidationRecordService,
    ValidationRecordServiceError,
};
use app_domain::validation_record::{ValidationRecord, ValidationRecordId};
use app_infrastructure::validation_record::SqlxValidationRecordRepository;
use persistence::DatabasePool;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Debug, PartialEq, Eq)]
enum Access {
    Authorize(String, &'static str),
    List(ListValidationRecordsQuery),
}

#[derive(Clone, Default)]
struct Probe {
    calls: Arc<Mutex<Vec<Access>>>,
    denial: Arc<Mutex<Option<ValidationRecordServiceError>>>,
    failure: Arc<Mutex<Option<ValidationRecordServiceError>>>,
}

impl ValidationRecordAuthorization for Probe {
    async fn require(
        &self,
        actor: &str,
        permission: &'static str,
    ) -> Result<(), ValidationRecordServiceError> {
        self.calls
            .lock()
            .unwrap()
            .push(Access::Authorize(actor.to_owned(), permission));
        self.denial.lock().unwrap().map_or(Ok(()), Err)
    }
}

impl ValidationRecordIdGenerator for Probe {
    fn next_id(&self) -> ValidationRecordId {
        panic!("list must not allocate identifiers")
    }
}

impl ValidationRecordRepository for Probe {
    async fn list(
        &self,
        query: &ListValidationRecordsQuery,
    ) -> Result<(Vec<ValidationRecord>, u64), ValidationRecordServiceError> {
        self.calls.lock().unwrap().push(Access::List(*query));
        if let Some(error) = *self.failure.lock().unwrap() {
            return Err(error);
        }
        Ok((vec![record(51)], 101))
    }

    async fn find(
        &self,
        _: ValidationRecordId,
    ) -> Result<Option<ValidationRecord>, ValidationRecordServiceError> {
        panic!("list must not call find")
    }

    async fn insert(
        &self,
        _: ValidationRecord,
    ) -> Result<ValidationRecord, ValidationRecordServiceError> {
        panic!("list must not insert")
    }

    async fn update(
        &self,
        _: ValidationRecord,
    ) -> Result<Option<ValidationRecord>, ValidationRecordServiceError> {
        panic!("list must not update")
    }

    async fn delete(&self, _: ValidationRecordId) -> Result<bool, ValidationRecordServiceError> {
        panic!("list must not delete")
    }
}

fn record(index: u128) -> ValidationRecord {
    ValidationRecord {
        id: ValidationRecordId::new(Uuid::from_u128(index)),
        name: "value'); DROP TABLE validation_records; --".into(),
        active: true,
        sequence: index as i64,
        external_id: None,
        observed_at: None,
    }
}

#[tokio::test]
async fn every_list_request_authorizes_before_repository_rows_or_totals() {
    let probe = Probe::default();
    let service = ValidationRecordAppService::new(probe.clone(), probe.clone(), probe.clone());
    for denial in [
        ValidationRecordServiceError::Unauthorized,
        ValidationRecordServiceError::Forbidden,
        ValidationRecordServiceError::Persistence,
    ] {
        *probe.denial.lock().unwrap() = Some(denial);
        for offset in [0, 50, 100, i64::MAX as u64, u64::MAX] {
            let query = ListValidationRecordsQuery { offset, limit: 50 };
            assert_eq!(service.list("denied", query).await, Err(denial));
            // The repository port owns BOTH rows and COUNT(*). No call means
            // neither data source can be observed, even for an invalid offset.
            assert_eq!(
                std::mem::take(&mut *probe.calls.lock().unwrap()),
                [Access::Authorize("denied".into(), LIST_PERMISSION)]
            );
        }
    }
    *probe.denial.lock().unwrap() = None;
    for offset in [0, 50, 100] {
        let query = ListValidationRecordsQuery { offset, limit: 50 };
        let result = service.list("allowed", query).await.unwrap();
        assert_eq!(result.total, 101);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].id, record(51).id.into_inner());
        assert_eq!(result.items[0].name, record(51).name);
        assert_eq!(
            std::mem::take(&mut *probe.calls.lock().unwrap()),
            [
                Access::Authorize("allowed".into(), LIST_PERMISSION),
                Access::List(query)
            ]
        );
    }
    let query = ListValidationRecordsQuery {
        offset: 50,
        limit: 50,
    };
    // Revoking permission between pages must recheck authorization, including
    // when an earlier page already returned rows and a total.
    *probe.denial.lock().unwrap() = Some(ValidationRecordServiceError::Forbidden);
    assert_eq!(
        service.list("allowed", query).await,
        Err(ValidationRecordServiceError::Forbidden)
    );
    assert_eq!(
        std::mem::take(&mut *probe.calls.lock().unwrap()),
        [Access::Authorize("allowed".into(), LIST_PERMISSION)]
    );
    *probe.denial.lock().unwrap() = None;
    *probe.failure.lock().unwrap() = Some(ValidationRecordServiceError::Persistence);
    assert_eq!(
        service.list("allowed", query).await,
        Err(ValidationRecordServiceError::Persistence)
    );
    *probe.failure.lock().unwrap() = None;
    assert_eq!(service.list("allowed", query).await.unwrap().total, 101);
    assert_eq!(
        std::mem::take(&mut *probe.calls.lock().unwrap()),
        [
            Access::Authorize("allowed".into(), LIST_PERMISSION),
            Access::List(query),
            Access::Authorize("allowed".into(), LIST_PERMISSION),
            Access::List(query),
        ]
    );
}

async fn assert_page(
    repository: &impl ValidationRecordRepository,
    total: u64,
    offset: u64,
    limit: u32,
) {
    let (items, actual_total) = repository
        .list(&ListValidationRecordsQuery { offset, limit })
        .await
        .unwrap();
    assert_eq!(actual_total, total, "COUNT must not be limited by the page");
    let expected: Vec<_> = (1..=u128::from(total))
        .skip(usize::try_from(offset).unwrap())
        .take(limit as usize)
        .map(|index| record(index).id)
        .collect();
    assert_eq!(
        items.iter().map(|item| item.id).collect::<Vec<_>>(),
        expected
    );
    for item in items {
        assert_eq!(item, record(item.sequence as u128));
    }
}

async fn verify_provider(repository: &impl ValidationRecordRepository) {
    let mut previous = 0;
    for total in [0, 1, 50, 51, 100, 101] {
        // Reverse insertion order ensures ORDER BY, rather than physical order,
        // determines traversal on both providers.
        for index in ((previous + 1)..=total).rev() {
            repository.insert(record(index as u128)).await.unwrap();
        }
        previous = total;
        for offset in [0, 50, 100, 150, i64::MAX as u64] {
            assert_page(repository, total, offset, 50).await;
        }
        assert_page(repository, total, 0, 0).await;
        assert_page(repository, total, 0, u32::MAX).await;
        let mut visited = Vec::new();
        for offset in (0..total).step_by(50) {
            let (items, count) = repository
                .list(&ListValidationRecordsQuery { offset, limit: 50 })
                .await
                .unwrap();
            assert_eq!(count, total);
            visited.extend(items.into_iter().map(|item| item.id));
        }
        assert_eq!(
            visited,
            (1..=u128::from(total))
                .map(|index| record(index).id)
                .collect::<Vec<_>>()
        );
        for offset in [i64::MAX as u64 + 1, u64::MAX] {
            assert_eq!(
                repository
                    .list(&ListValidationRecordsQuery { offset, limit: 50 })
                    .await,
                Err(ValidationRecordServiceError::InvalidInput)
            );
        }
    }
    assert!(repository.delete(record(101).id).await.unwrap());
    assert_page(repository, 100, 100, 50).await;
    assert_page(repository, 100, 50, 50).await;
    for index in (1..=100).rev() {
        assert!(repository.delete(record(index).id).await.unwrap());
    }
    assert_page(repository, 0, 50, 50).await;
    assert_page(repository, 0, 0, 50).await;
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_bound_paging_order_totals_deletion_and_failures() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let repository =
        SqlxValidationRecordRepository::from_database(&DatabasePool::Sqlite(pool.clone())).unwrap();
    // Only the generated table DDL is needed here; Identity permission seeding
    // and full migration history retain their separate lifecycle coverage.
    let migration = include_str!("resource_pagination.sql");
    let (table_ddl, _) = migration.split_once("INSERT INTO permissions").unwrap();
    sqlx::raw_sql(table_ddl).execute(&pool).await.unwrap();
    verify_provider(&repository).await;
    sqlx::query("DROP TABLE validation_records")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        repository
            .list(&ListValidationRecordsQuery {
                offset: 0,
                limit: 50
            })
            .await,
        Err(ValidationRecordServiceError::Persistence)
    );
    sqlx::raw_sql(table_ddl).execute(&pool).await.unwrap();
    assert_page(&repository, 0, 0, 50).await;
    pool.close().await;
}

#[cfg(feature = "db-postgres")]
#[tokio::test]
#[ignore = "requires explicitly authorized disposable PostgreSQL"]
async fn postgres_bound_paging_order_totals_deletion_and_failures() {
    assert_eq!(
        std::env::var("ALLOW_GENERATED_APP_DB_RESET").as_deref(),
        Ok("true")
    );
    let url =
        std::env::var("GENERATED_APP_DATABASE_URL").expect("explicit disposable PostgreSQL URL");
    let schema = format!("pagination_{}", Uuid::new_v4().simple());
    let admin = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let search_path = format!("SET search_path TO {schema}");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .after_connect(move |connection, _| {
            let search_path = search_path.clone();
            Box::pin(async move {
                sqlx::query(&search_path).execute(connection).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    let repository =
        SqlxValidationRecordRepository::from_database(&DatabasePool::Postgres(pool.clone()))
            .unwrap();
    let migration = include_str!("resource_pagination.sql");
    let (table_ddl, _) = migration.split_once("INSERT INTO permissions").unwrap();
    sqlx::raw_sql(table_ddl).execute(&pool).await.unwrap();
    verify_provider(&repository).await;
    sqlx::query("DROP TABLE validation_records")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        repository
            .list(&ListValidationRecordsQuery {
                offset: 0,
                limit: 50
            })
            .await,
        Err(ValidationRecordServiceError::Persistence)
    );
    sqlx::raw_sql(table_ddl).execute(&pool).await.unwrap();
    assert_page(&repository, 0, 0, 50).await;
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
