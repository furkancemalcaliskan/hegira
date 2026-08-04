#![cfg(feature = "ssr")]

use chrono::Utc;
use hegira::{
    application::{
        identity::{
            oauth::signup_writer::{CompleteOAuthSignup, OAuthSignupWriter},
            users::writer::{
                CreateManagedUser, ManagedUserWriter, RegisterManagedUser, UpdateManagedUser,
            },
        },
        shared::{jobs::DurableJobHandler, mail::TransactionalMail},
    },
    infrastructure::{identity::SqlxIdentityRepository, testing::reset_database_from_env},
    search::{
        NullSearch, SearchAdapter,
        jobs::{SearchIndexCommand, SearchIndexJobHandler},
    },
};
use std::sync::Arc;

#[tokio::test]
#[ignore = "requires DATABASE_URL and resets the test database"]
async fn identity_mutations_and_search_outbox_are_atomic_and_revisioned() {
    let pool = reset_database_from_env()
        .await
        .expect("test database should reset and migrate");
    sqlx::query("INSERT INTO roles (name) VALUES ('member')")
        .execute(&pool)
        .await
        .unwrap();
    let repository = SqlxIdentityRepository::new(pool.clone());

    let stale_document_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO search_projection_versions (index_name, document_id, revision)
         VALUES ('identity_users', $1, 3)",
    )
    .bind(&stale_document_id)
    .execute(&pool)
    .await
    .unwrap();
    let handler =
        SearchIndexJobHandler::new(Arc::new(SearchAdapter::Null(NullSearch)), pool.clone());
    handler
        .handle(
            serde_json::to_value(SearchIndexCommand::Delete {
                index: "identity_users".to_string(),
                document_id: stale_document_id.clone(),
                revision: Some(2),
            })
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT revision FROM search_projection_versions
             WHERE index_name = 'identity_users' AND document_id = $1",
        )
        .bind(stale_document_id)
        .fetch_one(&pool)
        .await
        .unwrap(),
        3
    );

    let registered = repository
        .register_managed_user(RegisterManagedUser {
            username: "registered@example.com".to_string(),
            password_hash: "hash".to_string(),
            verification_token: "verification-token".to_string(),
            verification_sent_at: Utc::now(),
            publish_search: true,
            mail: Some(TransactionalMail::VerifyEmail {
                to: "registered@example.com".to_string(),
                app_name: "Test".to_string(),
                verification_url: "https://example.test/verify/token".to_string(),
            }),
        })
        .await
        .unwrap();
    assert!(
        repository
            .verify_managed_email("verification-token", Utc::now(), true)
            .await
            .unwrap()
    );

    let registration_messages = sqlx::query_as::<_, (serde_json::Value, String)>(
        "SELECT payload, idempotency_key
         FROM outbox_messages
         WHERE name = 'search.index.v1'
           AND payload->'documents'->0->>'username' = 'registered@example.com'
         ORDER BY created_at, id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(registration_messages.len(), 2);
    assert_eq!(registration_messages[0].0["revision"], 0);
    assert_eq!(registration_messages[1].0["revision"], 1);
    assert_eq!(
        registration_messages[0].0["documents"][0]["id"],
        registered.pid.to_string()
    );
    assert_eq!(
        registration_messages[0].0["documents"][0]["is_verified"],
        false
    );
    assert_eq!(
        registration_messages[1].0["documents"][0]["is_verified"],
        true
    );
    assert_ne!(registration_messages[0].1, registration_messages[1].1);
    let mail_payload = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload FROM outbox_messages
         WHERE name = 'mail.send.v1'
           AND payload->'mail'->>'to' = 'registered@example.com'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(mail_payload["mail"]["template"], "verify_email");

    assert!(
        repository
            .set_reset_token_with_mail(
                "registered@example.com",
                "reset-token",
                Utc::now(),
                TransactionalMail::ResetPassword {
                    to: "registered@example.com".to_string(),
                    app_name: "Test".to_string(),
                    reset_url: "https://example.test/reset/token".to_string(),
                },
            )
            .await
            .unwrap()
    );
    assert!(
        repository
            .set_magic_link_with_mail(
                "registered@example.com",
                "magic-token",
                Utc::now() + chrono::Duration::minutes(5),
                TransactionalMail::MagicLink {
                    to: "registered@example.com".to_string(),
                    app_name: "Test".to_string(),
                    login_url: "https://example.test/magic/token".to_string(),
                },
            )
            .await
            .unwrap()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM outbox_messages
             WHERE name = 'mail.send.v1'
               AND payload->'mail'->>'to' = 'registered@example.com'",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        3
    );

    sqlx::query(
        "INSERT INTO oauth_pending_signups
             (token, provider, provider_user_id, email, created_at, expires_at)
         VALUES ('oauth-token', 'github', 'provider-user', 'oauth@example.com',
                 NOW(), NOW() + INTERVAL '10 minutes')",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        repository
            .complete_oauth_signup(CompleteOAuthSignup {
                token: "oauth-token".to_string(),
                now: Utc::now(),
                username: "oauth@example.com".to_string(),
                password_hash: "random-hash".to_string(),
                publish_search: true,
            })
            .await
            .unwrap()
    );
    let oauth_projection = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload FROM outbox_messages
         WHERE name = 'search.index.v1'
           AND payload->'documents'->0->>'username' = 'oauth@example.com'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(oauth_projection["operation"], "upsert");
    assert_eq!(oauth_projection["revision"], 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM oauth_pending_signups WHERE token = 'oauth-token'",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)
             FROM user_oauth_connections c
             JOIN users u ON u.id = c.user_id
             WHERE u.username = 'oauth@example.com' AND c.provider = 'github'",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );

    let created = repository
        .create_managed_user(CreateManagedUser {
            username: "indexed@example.com".to_string(),
            password_hash: "hash".to_string(),
            email_verified_at: None,
            roles: vec!["member".to_string()],
            publish_search: true,
        })
        .await
        .unwrap();

    repository
        .update_managed_user(UpdateManagedUser {
            username: created.username.clone(),
            password_hash: None,
            email_verified_at: Some(Utc::now()),
            roles: vec!["member".to_string()],
            publish_search: true,
        })
        .await
        .unwrap()
        .expect("user should update");
    assert!(
        repository
            .delete_managed_user(&created.username, true)
            .await
            .unwrap()
    );

    let messages = sqlx::query_as::<_, (serde_json::Value, String)>(
        "SELECT payload, idempotency_key
         FROM outbox_messages
         WHERE name = 'search.index.v1'
           AND (
               (payload->>'operation' = 'upsert'
                AND payload->'documents'->0->>'id' = $1)
               OR
               (payload->>'operation' = 'delete'
                AND payload->>'document_id' = $1)
           )
         ORDER BY created_at, id",
    )
    .bind(created.pid.to_string())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].0["operation"], "upsert");
    assert_eq!(messages[1].0["operation"], "upsert");
    assert_eq!(messages[2].0["operation"], "delete");
    assert_eq!(messages[0].0["revision"], 0);
    assert_eq!(messages[1].0["revision"], 1);
    assert_eq!(messages[2].0["revision"], 2);
    assert_ne!(messages[0].1, messages[1].1);
    assert_ne!(messages[1].1, messages[2].1);

    let failed = repository
        .create_managed_user(CreateManagedUser {
            username: "rollback@example.com".to_string(),
            password_hash: "hash".to_string(),
            email_verified_at: None,
            roles: vec!["missing-role".to_string()],
            publish_search: true,
        })
        .await;
    assert!(failed.is_err());
    let user_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE username = 'rollback@example.com'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let message_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM outbox_messages
         WHERE payload->'documents'->0->>'username' = 'rollback@example.com'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(user_count, 0);
    assert_eq!(message_count, 0);
}
