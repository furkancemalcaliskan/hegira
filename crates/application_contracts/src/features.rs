use crate::permissions::PermissionDefinition;

#[derive(Debug, Clone, Copy)]
pub struct FeatureDescriptor {
    pub key: &'static str,
    pub permissions: &'static [PermissionDefinition],
}

pub const FEATURES: &[FeatureDescriptor] = &[
    FeatureDescriptor {
        key: "identity",
        permissions: crate::identity::permissions::ALL,
    },
    FeatureDescriptor {
        key: "catalog.products",
        permissions: crate::catalog::permissions::ALL,
    },
];

pub fn descriptor(key: &str) -> Option<&'static FeatureDescriptor> {
    FEATURES.iter().find(|feature| feature.key == key)
}
