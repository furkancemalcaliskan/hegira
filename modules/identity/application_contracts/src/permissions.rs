use crate::features;
use crate::localization::IdentityMessage;
pub use identity_domain::identity::authorization::PermissionName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionDefinition {
    pub name: PermissionName,
    pub display_name: IdentityMessage,
}

/// Returns every permission contributed by the application's bounded contexts.
/// Add a context's `permissions::ALL` iterator here when introducing a module.
pub fn all() -> impl Iterator<Item = &'static PermissionDefinition> {
    features::FEATURES
        .iter()
        .flat_map(|feature| feature.permissions)
    // hegira:permission-registry
    // hegira:permission-registry:end
}

pub fn all_names() -> impl Iterator<Item = PermissionName> {
    all().map(|definition| definition.name)
}

pub fn from_name(name: &str) -> Option<PermissionName> {
    all()
        .find(|definition| definition.name.0 == name)
        .map(|definition| definition.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_context_permissions() {
        assert_eq!(
            from_name(crate::identity::permissions::USERS_CREATE.0),
            Some(crate::identity::permissions::USERS_CREATE)
        );

        // hegira:permission-tests
        // hegira:permission-tests:end

        assert_eq!(from_name("Unknown.Permission"), None);
    }

    #[test]
    fn feature_registry_contains_only_active_contexts() {
        let keys = features::FEATURES
            .iter()
            .map(|feature| feature.key)
            .collect::<Vec<_>>();
        assert_eq!(keys, ["identity"]);
    }
}
