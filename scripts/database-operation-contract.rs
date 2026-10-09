//! Repository-only public CLI database contract, copied into disposable staging.
//! Pools provision only test-owned files/schemas; CLI operations must never do so.
use std::{
    env,
    io::Read,
    path::PathBuf,
    process::{Command, Output, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::Value;
use sqlx::ConnectOptions;
#[cfg(feature = "db-postgres")]
use sqlx::Connection;
#[cfg(feature = "db-sqlite")]
use std::fs;
#[cfg(feature = "db-postgres")]
use std::hash::{DefaultHasher, Hash, Hasher};

#[cfg(feature = "db-sqlite")]
type Pool = sqlx::SqlitePool;
#[cfg(feature = "db-postgres")]
type Pool = sqlx::PgPool;

struct Target {
    pool: Pool,
    url: String,
    identity: String,
}
impl Target {
    async fn new() -> Self {
        let identity = format!(
            "hegira_db_op_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        #[cfg(feature = "db-sqlite")]
        {
            let path = PathBuf::from(env::var("DATABASE_OPERATION_DIRECTORY").unwrap())
                .join(format!("{identity}.sqlite3"));
            assert!(!path.exists());
            let url = format!("sqlite://{}", path.display());
            let options = url
                .parse::<sqlx::sqlite::SqliteConnectOptions>()
                .unwrap()
                .create_if_missing(true)
                .disable_statement_logging();
            let pool = Pool::connect_with(options)
                .await
                .expect("open owned SQLite fixture");
            Self {
                pool,
                url,
                identity,
            }
        }
        #[cfg(feature = "db-postgres")]
        {
            assert_eq!(
                env::var("ALLOW_DATABASE_OPERATION_DISPOSABLE_TARGETS").as_deref(),
                Ok("true")
            );
            let base =
                env::var("DATABASE_OPERATION_POSTGRES_URL").expect("explicit disposable URL");
            assert!(
                !base.contains('?'),
                "fixture URL must not carry ambiguous options"
            );
            let options = base
                .parse::<sqlx::postgres::PgConnectOptions>()
                .expect("valid disposable URL")
                .disable_statement_logging();
            let mut admin = options
                .connect()
                .await
                .expect("connect disposable PostgreSQL");
            sqlx::query(&format!("CREATE SCHEMA {identity}"))
                .execute(&mut admin)
                .await
                .unwrap();
            admin.close().await.unwrap();
            let url = format!("{base}?options=-csearch_path%3D{identity}");
            let pool = Pool::connect(&url)
                .await
                .expect("open owned PostgreSQL schema");
            Self {
                pool,
                url,
                identity,
            }
        }
    }
    async fn cleanup(self) {
        self.pool.close().await;
        #[cfg(feature = "db-sqlite")]
        {
            let root = PathBuf::from(env::var("DATABASE_OPERATION_DIRECTORY").unwrap());
            for suffix in [".sqlite3", ".sqlite3-wal", ".sqlite3-shm"] {
                let path = root.join(format!("{}{suffix}", self.identity));
                if path.exists() {
                    fs::remove_file(path).unwrap();
                }
            }
        }
        #[cfg(feature = "db-postgres")]
        {
            let options = env::var("DATABASE_OPERATION_POSTGRES_URL")
                .unwrap()
                .parse::<sqlx::postgres::PgConnectOptions>()
                .unwrap()
                .disable_statement_logging();
            let mut admin = options
                .connect()
                .await
                .expect("reconnect for owned schema cleanup");
            sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.identity))
                .execute(&mut admin)
                .await
                .unwrap();
            admin.close().await.unwrap();
        }
    }
}

fn provider() -> &'static str {
    if cfg!(feature = "db-sqlite") {
        "sqlite"
    } else {
        "postgres"
    }
}
fn profile() -> &'static str {
    if cfg!(feature = "db-sqlite") {
        "sqlite"
    } else {
        "development"
    }
}

fn invoke(operation: &str, profile: &str, url: &str, execute: bool, approval: bool) -> Output {
    let mut command = Command::new(env::var("DATABASE_OPERATION_CLI").unwrap());
    command
        .args([
            "db",
            operation,
            "--profile",
            profile,
            "--json",
            "--application-root",
        ])
        .arg(env::var("DATABASE_OPERATION_APPLICATION").unwrap())
        .env("APP_ENV", "unselected-profile")
        .env("APP__DATABASE__BACKEND", "unselected-provider")
        .env("APP__DATABASE__URL", url)
        .env("APP__SERVER__ADDR", "invalid-unrelated-http-setting")
        .env("APP__MAILER__BACKEND", "invalid-unrelated-provider")
        .env("APP__STARTUP__SEED_IDENTITY", "true")
        .env("APP__STARTUP__SCHEDULER", "true")
        .env("APP__STARTUP__ENSURE_DATABASE", "false")
        .env("APP__DATABASE__AUTO_MIGRATE", "false")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if execute {
        command
            .args(["--execute", "--trust-application", "--cargo"])
            .arg(env::var("DATABASE_OPERATION_CARGO").unwrap())
            .arg("--tool-directory")
            .arg(env::var("DATABASE_OPERATION_TOOL_DIRECTORY").unwrap())
            .args(["--tool-directory", "/usr/bin"]);
    } else {
        command.arg("--dry-run");
    }
    if approval {
        command.arg("--approve-production-migration");
    }
    let mut child = command.spawn().unwrap();
    // Drain both pipes while polling; a filled pipe must not look like a DB hang.
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let read = |pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            pipe.take(2 * 1024 * 1024).read_to_end(&mut bytes).unwrap();
            bytes
        })
    };
    let stdout = read(Box::new(stdout));
    let stderr = read(Box::new(stderr));
    let deadline = Instant::now() + Duration::from_secs(120);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            // Let the public CLI forward termination and reap its owned Cargo group.
            Command::new("/bin/kill")
                .args(["-TERM", &child.id().to_string()])
                .status()
                .unwrap();
            let grace = Instant::now() + Duration::from_secs(5);
            while child.try_wait().unwrap().is_none() && Instant::now() < grace {
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = child.kill();
            let _ = child.wait();
            panic!("public database operation exceeded its fixture deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Output {
        status: child.wait().unwrap(),
        stdout: stdout.join().unwrap(),
        stderr: stderr.join().unwrap(),
    }
}
fn report(output: Output, exit: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(exit),
        "unexpected public CLI outcome"
    );
    assert!(
        output.stderr.is_empty(),
        "raw child/driver output must not escape"
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("closed CLI JSON report");
    assert_eq!(value["output_schema"], 1);
    let text = value.to_string();
    assert!(!text.contains("search_path"));
    assert!(!text.contains("invalid-unrelated"));
    value
}
fn success(value: &Value, operation: &str, history: &str) {
    assert_eq!(value["execution"]["outcome"]["status"], "succeeded");
    let database = &value["execution"]["database"];
    assert_eq!(database["provider"], provider());
    assert_eq!(database["operation"], operation);
    assert_eq!(database["status"]["history"], history);
    let entries = database["status"]["migrations"].as_array().unwrap();
    assert!(entries.windows(2).all(|pair| pair[0]["version"].as_i64().unwrap() < pair[1]["version"].as_i64().unwrap()));
    let identity = env::var("DATABASE_OPERATION_IDENTITY").unwrap() == "true";
    assert_eq!(
        entries.iter().any(|e| e["module_id"] == "identity"),
        identity
    );
}
fn child_failure(value: &Value, code: i32) {
    assert_eq!(value["execution"]["outcome"]["status"], "child-failed");
    assert_eq!(value["execution"]["outcome"]["exit_code"], code);
    assert!(value["execution"].get("database").is_none());
}

fn compiled_provider_mismatch_is_rejected() {
    let (backend, url) = if cfg!(feature = "db-sqlite") {
        ("postgres", "postgres://owner@127.0.0.1:1/absent")
    } else {
        ("sqlite", "sqlite://absent-unsupported-provider.sqlite3")
    };
    let output = Command::new(env!("CARGO_BIN_EXE_app_database"))
        .current_dir(env::var("DATABASE_OPERATION_APPLICATION").unwrap())
        .env_clear()
        .env("APP_ENV", "development")
        .env("APP__DATABASE__BACKEND", backend)
        .env("APP__DATABASE__URL", url)
        .args(["migrate", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"], "configuration-capabilities");
    assert!(!value.to_string().contains(url));
    assert!(
        !PathBuf::from(env::var("DATABASE_OPERATION_APPLICATION").unwrap())
            .join("absent-unsupported-provider.sqlite3")
            .exists()
    );
}

type History = Vec<(i64, String, bool, Vec<u8>, String, i64)>;
#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    schema: Vec<(String, String, String)>,
    history: Option<History>,
    product: Vec<i64>,
    row_fingerprints: Vec<(String, usize, u64)>,
    #[cfg(feature = "db-sqlite")]
    bytes: Vec<u8>,
}
async fn table_exists(pool: &Pool, name: &str) -> bool {
    assert!(name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'));
    #[cfg(feature = "db-sqlite")]
    let query =
        format!("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='{name}')");
    #[cfg(feature = "db-postgres")]
    let query = format!(
        "SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_schema=current_schema() AND table_name='{name}')"
    );
    sqlx::query_scalar(&query).fetch_one(pool).await.unwrap()
}
async fn snapshot(pool: &Pool) -> Snapshot {
    #[cfg(feature = "db-sqlite")]
    let schema = "SELECT type, name, COALESCE(sql,'') FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name";
    #[cfg(feature = "db-postgres")]
    let schema = "SELECT table_name,column_name,data_type FROM information_schema.columns WHERE table_schema=current_schema() ORDER BY table_name,ordinal_position";
    let history = if table_exists(pool, "_sqlx_migrations").await {
        Some(sqlx::query_as("SELECT version,description,success,checksum,CAST(installed_on AS TEXT),execution_time FROM _sqlx_migrations ORDER BY version").fetch_all(pool).await.unwrap())
    } else {
        None
    };
    let product = if table_exists(pool, "operation_existing").await {
        sqlx::query_scalar("SELECT id FROM operation_existing ORDER BY id")
            .fetch_all(pool)
            .await
            .unwrap()
    } else {
        vec![]
    };
    #[cfg(feature = "db-postgres")]
    let tables: Vec<String> = sqlx::query_scalar("SELECT table_name FROM information_schema.tables WHERE table_schema=current_schema() AND table_type='BASE TABLE' ORDER BY table_name")
        .fetch_all(pool).await.unwrap();
    #[cfg(feature = "db-postgres")]
    let row_fingerprints = {
        let mut rows = Vec::new();
        for table in tables {
            let quoted = table.replace('"', "\"\"");
            let values: Vec<String> = sqlx::query_scalar(&format!(
                "SELECT row_to_json(t)::text FROM \"{quoted}\" t ORDER BY row_to_json(t)::text"
            ))
            .fetch_all(pool)
            .await
            .unwrap();
            // In-process equality evidence only, not a cryptographic/authentication
            // digest. Assertions never print source rows or unexpected seed data.
            let mut hasher = DefaultHasher::new();
            values.hash(&mut hasher);
            rows.push((table, values.len(), hasher.finish()));
        }
        rows
    };
    #[cfg(feature = "db-sqlite")]
    let row_fingerprints = Vec::new();
    Snapshot {
        schema: sqlx::query_as(schema).fetch_all(pool).await.unwrap(),
        history,
        product,
        row_fingerprints,
        #[cfg(feature = "db-sqlite")]
        bytes: fs::read(pool.connect_options().get_filename()).unwrap(),
    }
}
async fn prepare(pool: &Pool, failing: bool) {
    sqlx::query("CREATE TABLE operation_existing (id BIGINT PRIMARY KEY)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO operation_existing VALUES (42)")
        .execute(pool)
        .await
        .unwrap();
    let allowed = if failing { 0 } else { 1 };
    sqlx::query(&format!(
        "CREATE TABLE operation_control (id INTEGER CHECK (id={allowed}))"
    ))
    .execute(pool)
    .await
    .unwrap();
}

async fn contract(pool: Pool, url: String, case: &'static str) {
    let before = snapshot(&pool).await;
    if case == "composition" {
        for operation in ["status", "migrate"] {
            child_failure(
                &report(invoke(operation, profile(), &url, true, false), 1),
                3,
            );
            assert_eq!(snapshot(&pool).await, before);
        }
        return;
    }
    success(
        &report(invoke("status", profile(), &url, true, false), 0),
        "status",
        "metadata-missing",
    );
    assert_eq!(snapshot(&pool).await, before);
    prepare(&pool, case == "failure").await;
    let before = snapshot(&pool).await;
    for operation in ["status", "migrate"] {
        let preview = report(invoke(operation, profile(), &url, false, false), 0);
        assert!(preview["execution"].is_null());
        assert_eq!(snapshot(&pool).await, before);
    }
    let mismatch = if cfg!(feature = "db-sqlite") {
        "development"
    } else {
        "sqlite"
    };
    let denied = report(invoke("migrate", mismatch, &url, true, false), 3);
    assert!(denied["execution"].is_null());
    assert_eq!(snapshot(&pool).await, before);
    #[cfg(feature = "db-postgres")]
    {
        let denied = report(invoke("migrate", "production", &url, true, false), 3);
        assert_eq!(
            denied["diagnostics"][0]["code"],
            "production-migration-approval"
        );
        assert_eq!(snapshot(&pool).await, before);
    }
    let migration_profile = if case == "production" {
        "production"
    } else {
        profile()
    };
    let migrated = report(
        invoke(
            "migrate",
            migration_profile,
            &url,
            true,
            case == "production",
        ),
        if case == "failure" { 1 } else { 0 },
    );
    if case == "failure" {
        child_failure(&migrated, 4);
        let after = snapshot(&pool).await;
        assert_eq!(after.product, vec![42]);
        assert!(
            after
                .history
                .as_ref()
                .unwrap()
                .iter()
                .all(|r| r.2 && r.0 != 1000002)
        );
        assert!(
            after
                .history
                .as_ref()
                .unwrap()
                .iter()
                .any(|r| r.0 == 1000001)
        );
        assert!(table_exists(&pool, "operation_completed").await);
        assert!(!table_exists(&pool, "operation_failed").await);
        let status = report(invoke("status", profile(), &url, true, false), 0);
        success(&status, "status", "present");
        assert_eq!(
            status["execution"]["database"]["status"]["migrations"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["state"] == "pending")
                .count(),
            1
        );
        child_failure(
            &report(invoke("migrate", profile(), &url, true, false), 1),
            4,
        );
        assert_eq!(
            snapshot(&pool).await,
            after,
            "failed migration and its advisory lock must be recoverable"
        );
        return;
    }
    success(&migrated, "migrate", "present");
    if table_exists(&pool, "users").await {
        let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(users, 0, "normal Identity seed must not run");
    }
    match case {
        "checksum" => {
            #[cfg(feature = "db-sqlite")]
            let query = "UPDATE _sqlx_migrations SET checksum=X'00' WHERE version=1000001";
            #[cfg(feature = "db-postgres")]
            let query =
                "UPDATE _sqlx_migrations SET checksum=decode('00','hex') WHERE version=1000001";
            sqlx::query(query).execute(&pool).await.unwrap();
        }
        "unknown" => {
            sqlx::query("UPDATE _sqlx_migrations SET version=2000000 WHERE version=1000001")
                .execute(&pool)
                .await
                .unwrap();
        }
        "failed-history" => {
            sqlx::query("UPDATE _sqlx_migrations SET success=FALSE WHERE version=1000001")
                .execute(&pool)
                .await
                .unwrap();
        }
        _ => {}
    }
    let before = snapshot(&pool).await;
    for operation in ["status", "migrate"] {
        let result = report(
            invoke(operation, profile(), &url, true, false),
            if matches!(case, "success" | "production") {
                0
            } else {
                1
            },
        );
        if matches!(case, "success" | "production") {
            success(&result, operation, "present");
        } else {
            child_failure(&result, 4);
        }
        assert_eq!(
            snapshot(&pool).await,
            before,
            "status/rejected/repeated migration must preserve history, schema, and product data"
        );
    }
    #[cfg(feature = "db-postgres")]
    if case == "success" {
        success(
            &report(invoke("migrate", "production", &url, true, true), 0),
            "migrate",
            "present",
        );
        assert_eq!(snapshot(&pool).await, before);
    }
}

#[tokio::test]
async fn public_database_security_contract() {
    if env::var("DATABASE_OPERATION_COMPOSITION_CONFLICT").as_deref() == Ok("true") {
        let target = Target::new().await;
        let result = tokio::spawn(contract(
            target.pool.clone(),
            target.url.clone(),
            "composition",
        ))
        .await;
        target.cleanup().await;
        result.expect("composition must fail without DB access");
        return;
    }
    compiled_provider_mismatch_is_rejected();
    #[cfg(feature = "db-sqlite")]
    let missing = format!(
        "sqlite://{}/absent-operation.sqlite3",
        env::var("DATABASE_OPERATION_DIRECTORY").unwrap()
    );
    #[cfg(feature = "db-postgres")]
    let missing = env::var("DATABASE_OPERATION_POSTGRES_URL")
        .unwrap()
        .parse::<sqlx::postgres::PgConnectOptions>()
        .unwrap()
        .database(&format!("absent_operation_{}", std::process::id()))
        .to_url_lossy()
        .to_string();
    success(
        &report(invoke("status", profile(), &missing, true, false), 0),
        "status",
        "database-missing",
    );
    child_failure(
        &report(invoke("migrate", profile(), &missing, true, false), 1),
        4,
    );
    #[cfg(feature = "db-sqlite")]
    assert!(
        !PathBuf::from(env::var("DATABASE_OPERATION_DIRECTORY").unwrap())
            .join("absent-operation.sqlite3")
            .exists()
    );
    #[cfg(feature = "db-postgres")]
    let cases = [
        "success",
        "production",
        "checksum",
        "unknown",
        "failed-history",
        "failure",
    ]
    .as_slice();
    #[cfg(feature = "db-sqlite")]
    let cases = [
        "success",
        "checksum",
        "unknown",
        "failed-history",
        "failure",
    ]
    .as_slice();
    for &case in cases {
        let target = Target::new().await;
        let result = tokio::spawn(contract(target.pool.clone(), target.url.clone(), case)).await;
        // Cleanup also runs when an assertion panics in the joined task.
        target.cleanup().await;
        result.expect("public database authority/history contract failed");
    }
}
