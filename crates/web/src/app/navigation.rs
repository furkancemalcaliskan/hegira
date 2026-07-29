use crate::{
    application_contracts::identity::permissions, identity::navigation, web::shared::i18n::T,
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

pub fn workspace_nav_items(section: &NavSection) -> impl Iterator<Item = &'static NavItem> {
    section
        .items
        .iter()
        .chain(navigation::NAVIGATION_ITEMS.iter())
}

pub fn title_key_for_path(path: &str) -> T {
    match path {
        "/dashboard" => T::Home,
        // hegira:nav-titles
        // hegira:nav-titles:end
        _ => navigation::title_key_for_path(path).unwrap_or(T::Page),
    }
}

pub fn is_auth_path(path: &str) -> bool {
    navigation::is_public_auth_path(path)
}
