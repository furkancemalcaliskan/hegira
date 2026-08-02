use crate::localization::IdentityMessage;
pub use crate::permissions::{PermissionDefinition, PermissionName};

pub const USERS: PermissionName = PermissionName("Identity.Users");
pub const USERS_CREATE: PermissionName = PermissionName("Identity.Users.Create");
pub const USERS_UPDATE: PermissionName = PermissionName("Identity.Users.Update");
pub const USERS_DELETE: PermissionName = PermissionName("Identity.Users.Delete");
pub const AUTHORIZATION: PermissionName = PermissionName("Identity.Authorization");

pub const ALL: &[PermissionDefinition] = &[
    PermissionDefinition {
        name: USERS,
        display_name: IdentityMessage::PermissionUsers,
    },
    PermissionDefinition {
        name: USERS_CREATE,
        display_name: IdentityMessage::PermissionUsersCreate,
    },
    PermissionDefinition {
        name: USERS_UPDATE,
        display_name: IdentityMessage::PermissionUsersUpdate,
    },
    PermissionDefinition {
        name: USERS_DELETE,
        display_name: IdentityMessage::PermissionUsersDelete,
    },
    PermissionDefinition {
        name: AUTHORIZATION,
        display_name: IdentityMessage::PermissionAuthorization,
    },
];
