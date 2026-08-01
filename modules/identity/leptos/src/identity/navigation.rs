use crate::{
    application_contracts::identity::permissions,
    web::{
        app::navigation::{NavIcon, NavItem},
        shared::i18n::T,
    },
};

pub const NAVIGATION_ITEMS: &[NavItem] = &[
    NavItem {
        key: "roles",
        label: T::Roles,
        href: "/admin/roles",
        icon: NavIcon::Roles,
        permission: Some(permissions::AUTHORIZATION),
    },
    NavItem {
        key: "users",
        label: T::Users,
        href: "/admin/users",
        icon: NavIcon::Users,
        permission: Some(permissions::USERS),
    },
    NavItem {
        key: "profile",
        label: T::Profile,
        href: "/profile",
        icon: NavIcon::Profile,
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
