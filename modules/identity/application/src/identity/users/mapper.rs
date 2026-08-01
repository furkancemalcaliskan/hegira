use identity_application_contracts::identity::users::UserDto;
use identity_domain::identity::users::User;

pub fn user_dto(user: User, roles: Vec<String>) -> UserDto {
    UserDto {
        id: user.id,
        pid: user.pid,
        username: user.username,
        created_at: user.created_at,
        is_verified: user.email_verified_at.is_some(),
        roles,
    }
}
