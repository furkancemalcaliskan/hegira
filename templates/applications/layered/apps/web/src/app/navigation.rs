use crate::shared::i18n::T;
use identity_leptos::identity_application_contracts::identity::permissions;
use identity_leptos::{
    identity::navigation::{self, IdentityNavIcon},
    shared::i18n::T as IdentityText,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavIcon {
    Home,
    Roles,
    Users,
    Profile,
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
    items: &[NavItem {
        key: "home",
        label: T::Home,
        href: "/dashboard",
        icon: NavIcon::Home,
        permission: None,
        // hegira:nav-items
        // hegira:nav-items:end
    }],
}];

const IDENTITY_NAV_ITEMS: &[NavItem] = &[
    identity_nav_item(navigation::NAVIGATION_ITEMS[0]),
    identity_nav_item(navigation::NAVIGATION_ITEMS[1]),
    identity_nav_item(navigation::NAVIGATION_ITEMS[2]),
];

const fn identity_nav_item(item: navigation::IdentityNavItem) -> NavItem {
    NavItem {
        key: item.key,
        label: match item.label {
            IdentityText::Roles => T::Roles,
            IdentityText::Users => T::Users,
            IdentityText::Profile => T::Profile,
            _ => T::Page,
        },
        href: item.href,
        icon: match item.icon {
            IdentityNavIcon::Roles => NavIcon::Roles,
            IdentityNavIcon::Users => NavIcon::Users,
            IdentityNavIcon::Profile => NavIcon::Profile,
        },
        permission: item.permission,
    }
}

pub fn workspace_nav_items(section: &NavSection) -> impl Iterator<Item = &'static NavItem> {
    section.items.iter().chain(IDENTITY_NAV_ITEMS.iter())
}

pub fn title_key_for_path(path: &str) -> T {
    match path {
        "/dashboard" => T::Home,
        // hegira:nav-titles
        // hegira:nav-titles:end
        _ => match navigation::title_key_for_path(path) {
            Some(IdentityText::Login) => T::Login,
            Some(IdentityText::Roles) => T::Roles,
            Some(IdentityText::Users) => T::Users,
            Some(IdentityText::Profile) => T::Profile,
            _ => T::Page,
        },
    }
}

pub fn is_auth_path(path: &str) -> bool {
    navigation::is_public_auth_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_shell_explicitly_selects_identity_navigation() {
        assert_eq!(
            IDENTITY_NAV_ITEMS
                .iter()
                .map(|item| item.key)
                .collect::<Vec<_>>(),
            ["roles", "users", "profile"]
        );
        assert_eq!(title_key_for_path("/admin/users"), T::Users);
        assert!(is_auth_path("/login"));
    }
}
