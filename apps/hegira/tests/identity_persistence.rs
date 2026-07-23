#![cfg(feature = "ssr")]

use chrono::{Duration, Utc};
use hegira::{
    application_contracts::identity::permissions,
    domain::identity::{
        authorization::AuthorizationRepository,
        oauth::{OAuthRepository, OAuthUnlinkResult, PendingOAuthSignup},
        users::UserRepository,
    },
    infrastructure::{identity::SqlxIdentityRepository, testing::reset_and_seed_database_from_env},
};

#[tokio::test]
#[ignore = "requires DATABASE_URL and resets the test database"]
async fn migrations_rbac_and_oauth_invariants_hold() {
    let pool = reset_and_seed_database_from_env()
        .await
        .expect("test database should reset, migrate, and seed");
    let repository = SqlxIdentityRepository::new(pool.clone());

    let resolved_permissions = repository
        .user_permissions("admin@example.com")
        .await
        .expect("admin permissions should resolve");
    assert!(resolved_permissions.contains(&permissions::USERS));
    assert!(resolved_permissions.contains(&permissions::AUTHORIZATION));

    repository.create_role("auditor").await.unwrap();
    repository.create_role("empty-role").await.unwrap();
    repository
        .set_role_permissions("auditor", vec![permissions::USERS])
        .await
        .unwrap();

    let (roles, total) = repository
        .list_roles_page(
            1,
            1,
            Some("audit".to_string()),
            Some("with-permissions".to_string()),
            Some("name asc".to_string()),
        )
        .await
        .unwrap();
    assert_eq!(total, 1);
    assert_eq!(roles.len(), 1);
    assert_eq!(roles[0].name, "auditor");

    let (roles_without_permissions, total_without_permissions) = repository
        .list_roles_page(1, 20, None, Some("without-permissions".to_string()), None)
        .await
        .unwrap();
    assert_eq!(total_without_permissions, 1);
    assert_eq!(roles_without_permissions[0].name, "empty-role");
    assert!(repository.find_role("auditor").await.unwrap().is_some());

    let now = Utc::now();
    repository
        .insert_pending_signup(PendingOAuthSignup {
            token: "pending-test".to_string(),
            provider: "github".to_string(),
            provider_user_id: "github-test-user".to_string(),
            email: "oauth@example.com".to_string(),
            created_at: now,
            expires_at: now + Duration::minutes(5),
        })
        .await
        .expect("pending OAuth signup should be stored");
    assert!(
        repository
            .complete_pending_signup(
                "pending-test",
                now,
                "oauth-user",
                "unusable-test-password-hash",
            )
            .await
            .expect("OAuth signup should complete")
    );
    assert!(repository.exists("oauth-user").await.unwrap());
    assert_eq!(
        repository
            .unlink_connection("oauth-user", "github")
            .await
            .unwrap(),
        OAuthUnlinkResult::LastConnection
    );

    for (table, column) in [("sessions", "username"), ("user_roles", "username")] {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1 FROM information_schema.columns
                WHERE table_schema = 'public' AND table_name = $1 AND column_name = $2
             )",
        )
        .bind(table)
        .bind(column)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!exists, "legacy column {table}.{column} should be removed");
    }
}
