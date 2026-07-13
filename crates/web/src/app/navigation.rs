use crate::{
    application_contracts::{catalog::permissions as catalog_permissions, identity::permissions},
    web::shared::i18n::T,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavIcon {
    Home,
    Roles,
    Users,
    Profile,
    Products,
    // hegira:nav-icons
    // hegira:nav-icons:end
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavItem {
    pub key: &'static str,
    pub label: T,
    pub href: &'static str,
    pub icon: NavIcon,
    pub permission: Option<permissions::PermissionName>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavSection {
    pub label: T,
    pub items: &'static [NavItem],
}

pub const WORKSPACE_NAV: &[NavSection] = &[NavSection {
    label: T::Menu,
    items: &[
        NavItem {
            key: "home",
            label: T::Home,
            href: "/content",
            icon: NavIcon::Home,
            permission: None,
        },
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
            key: "products",
            label: T::Products,
            href: "/catalog/products",
            icon: NavIcon::Products,
            permission: Some(catalog_permissions::PRODUCTS),
        },
        NavItem {
            key: "profile",
            label: T::Profile,
            href: "/profile",
            icon: NavIcon::Profile,
            permission: None,
        },
        // hegira:nav-items
        // hegira:nav-items:end
    ],
}];

pub fn title_key_for_path(path: &str) -> T {
    match path {
        "/" | "/login" => T::Login,
        "/content" => T::Home,
        "/admin/roles" => T::Roles,
        "/admin/users" => T::Users,
        "/catalog/products" => T::Products,
        "/profile" => T::Profile,

        // hegira:nav-titles
        // hegira:nav-titles:end
        _ => T::Page,
    }
}

pub fn is_auth_path(path: &str) -> bool {
    matches!(path, "/" | "/login" | "/register")
}
