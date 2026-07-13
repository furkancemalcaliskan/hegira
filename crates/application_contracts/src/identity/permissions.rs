pub use crate::permissions::{PermissionDefinition, PermissionName};
use domain_shared::localization::T;

pub const USERS: PermissionName = PermissionName("Identity.Users");
pub const USERS_CREATE: PermissionName = PermissionName("Identity.Users.Create");
pub const USERS_UPDATE: PermissionName = PermissionName("Identity.Users.Update");
pub const USERS_DELETE: PermissionName = PermissionName("Identity.Users.Delete");
pub const AUTHORIZATION: PermissionName = PermissionName("Identity.Authorization");

pub const ALL: &[PermissionDefinition] = &[
    PermissionDefinition {
        name: USERS,
        display_name: T::PermissionIdentityUsers,
    },
    PermissionDefinition {
        name: USERS_CREATE,
        display_name: T::PermissionIdentityUsersCreate,
    },
    PermissionDefinition {
        name: USERS_UPDATE,
        display_name: T::PermissionIdentityUsersUpdate,
    },
    PermissionDefinition {
        name: USERS_DELETE,
        display_name: T::PermissionIdentityUsersDelete,
    },
    PermissionDefinition {
        name: AUTHORIZATION,
        display_name: T::PermissionIdentityAuthorization,
    },
];
