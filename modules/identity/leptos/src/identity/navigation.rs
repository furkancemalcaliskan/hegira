use crate::{
    identity_application_contracts::identity::permissions::{self, PermissionName},
    shared::i18n::T,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityNavIcon {
    Roles,
    Users,
    Profile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdentityNavItem {
    pub key: &'static str,
    pub label: T,
    pub href: &'static str,
    pub icon: IdentityNavIcon,
    pub permission: Option<PermissionName>,
}

pub const NAVIGATION_ITEMS: &[IdentityNavItem] = &[
    IdentityNavItem {
        key: "roles",
        label: T::Roles,
        href: "/admin/roles",
        icon: IdentityNavIcon::Roles,
        permission: Some(permissions::AUTHORIZATION),
    },
    IdentityNavItem {
        key: "users",
        label: T::Users,
        href: "/admin/users",
        icon: IdentityNavIcon::Users,
        permission: Some(permissions::USERS),
    },
    IdentityNavItem {
        key: "profile",
        label: T::Profile,
        href: "/profile",
        icon: IdentityNavIcon::Profile,
        permission: None,
    },
];

pub fn title_key_for_path(path: &str) -> Option<T> {
    match path {
        "/" | "/login" | "/register" => Some(T::Login),
        "/admin/roles" => Some(T::Roles),
        "/admin/users" => Some(T::Users),
        "/profile" => Some(T::Profile),
        _ => None,
    }
}

pub fn is_public_auth_path(path: &str) -> bool {
    matches!(path, "/" | "/login" | "/register")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contribution_exposes_identity_navigation_without_host_items() {
        assert_eq!(
            NAVIGATION_ITEMS
                .iter()
                .map(|item| item.key)
                .collect::<Vec<_>>(),
            ["roles", "users", "profile"]
        );
        assert!(
            NAVIGATION_ITEMS
                .iter()
                .all(|item| item.key != "home" && item.href != "/dashboard")
        );
    }

    #[test]
    fn contribution_owns_identity_route_metadata() {
        assert_eq!(title_key_for_path("/admin/users"), Some(T::Users));
        assert_eq!(title_key_for_path("/dashboard"), None);
        assert!(is_public_auth_path("/register"));
        assert!(!is_public_auth_path("/dashboard"));
    }
}
