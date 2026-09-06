#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdentityMessage {
    UsernameRequired,
    PasswordRequiredForNewUsers,
    PasswordRequired,
    UserNotFound,
    UserAlreadyExists,
    RoleNameRequired,
    RoleNotFound,
    SessionExpired,
    ProtectedAdminCannotBeDeleted,
    ProtectedAdminRoleCannotBeDeleted,
    InvalidCredentials,
    PermissionUsers,
    PermissionUsersCreate,
    PermissionUsersUpdate,
    PermissionUsersDelete,
    PermissionAuthorization,
}

impl IdentityMessage {
    pub const fn default_text(self) -> &'static str {
        match self {
            Self::UsernameRequired => "Username is required",
            Self::PasswordRequiredForNewUsers => "Password is required for new users.",
            Self::PasswordRequired => "Password is required.",
            Self::UserNotFound => "User not found",
            Self::UserAlreadyExists => "User already exists",
            Self::RoleNameRequired => "Role name is required",
            Self::RoleNotFound => "Role not found",
            Self::SessionExpired => "Session expired. Please log in again.",
            Self::ProtectedAdminCannotBeDeleted => "Admin user cannot be deleted",
            Self::ProtectedAdminRoleCannotBeDeleted => "Admin role cannot be deleted.",
            Self::InvalidCredentials => "Invalid username or password.",
            Self::PermissionUsers => "View identity users",
            Self::PermissionUsersCreate => "Create identity users",
            Self::PermissionUsersUpdate => "Update identity users",
            Self::PermissionUsersDelete => "Delete identity users",
            Self::PermissionAuthorization => "Manage identity roles and permissions",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_stable_default_identity_text() {
        assert_eq!(
            IdentityMessage::InvalidCredentials.default_text(),
            "Invalid username or password."
        );
        assert_eq!(
            IdentityMessage::PermissionAuthorization.default_text(),
            "Manage identity roles and permissions"
        );

        let permission_text = crate::permissions::all()
            .map(|permission| permission.display_name.default_text())
            .collect::<Vec<_>>();
        assert_eq!(
            permission_text,
            [
                "View identity users",
                "Create identity users",
                "Update identity users",
                "Delete identity users",
                "Manage identity roles and permissions",
            ]
        );
    }
}
