use crate::permissions::{PermissionDefinition, PermissionName};
use domain_shared::localization::T;

pub const PRODUCTS: PermissionName = PermissionName("Catalog.Products");
pub const PRODUCTS_CREATE: PermissionName = PermissionName("Catalog.Products.Create");
pub const PRODUCTS_UPDATE: PermissionName = PermissionName("Catalog.Products.Update");
pub const PRODUCTS_DELETE: PermissionName = PermissionName("Catalog.Products.Delete");

pub const ALL: &[PermissionDefinition] = &[
    PermissionDefinition {
        name: PRODUCTS,
        display_name: T::PermissionCatalogProducts,
    },
    PermissionDefinition {
        name: PRODUCTS_CREATE,
        display_name: T::PermissionCatalogProductsCreate,
    },
    PermissionDefinition {
        name: PRODUCTS_UPDATE,
        display_name: T::PermissionCatalogProductsUpdate,
    },
    PermissionDefinition {
        name: PRODUCTS_DELETE,
        display_name: T::PermissionCatalogProductsDelete,
    },
];
