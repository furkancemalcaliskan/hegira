use serde_json::json;

use crate::{
    identity::{
        auth::LoginResultDto,
        permissions,
        users::{CreateUserInput, UserDto},
    },
    permissions as registry,
};

#[test]
fn permission_identifiers_remain_stable() {
    let names = registry::all_names()
        .map(|permission| permission.0)
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        [
            "Identity.Users",
            "Identity.Users.Create",
            "Identity.Users.Update",
            "Identity.Users.Delete",
            "Identity.Authorization",
        ]
    );
    assert_eq!(
        registry::from_name("Identity.Users.Create"),
        Some(permissions::USERS_CREATE)
    );
}

#[test]
fn authentication_result_wire_shape_remains_stable() {
    assert_eq!(
        serde_json::to_value(LoginResultDto::totp_required("challenge".to_string())).unwrap(),
        json!({
            "token": null,
            "totp_required": true,
            "totp_token": "challenge"
        })
    );
}

#[test]
fn user_contract_wire_shapes_remain_stable() {
    let input: CreateUserInput = serde_json::from_value(json!({
        "username": "user@example.com",
        "password": "correct horse battery staple",
        "is_verified": false
    }))
    .unwrap();
    assert!(input.roles.is_empty());

    let dto = UserDto {
        id: 7,
        pid: uuid::Uuid::nil(),
        username: input.username,
        created_at: chrono::DateTime::UNIX_EPOCH,
        is_verified: input.is_verified,
        roles: vec!["member".to_string()],
    };
    let value = serde_json::to_value(dto).unwrap();
    assert_eq!(value["id"], 7);
    assert_eq!(value["pid"], "00000000-0000-0000-0000-000000000000");
    assert_eq!(value["roles"], json!(["member"]));
}
