pub mod entity;
pub mod repository;

pub use entity::{Permission, PermissionName, Role};
pub use repository::AuthorizationRepository;
