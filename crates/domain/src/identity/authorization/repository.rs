use crate::identity::authorization::{PermissionName, Role};
use domain_shared::common::errors::DomainError;

pub trait AuthorizationRepository: Send + Sync {
    fn user_has_permission(
        &self,
        username: &str,
        permission: PermissionName,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;

    fn user_permissions(
        &self,
        username: &str,
    ) -> impl std::future::Future<Output = Result<Vec<PermissionName>, DomainError>> + Send;

    fn assign_role(
        &self,
        username: &str,
        role_name: &str,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn list_roles(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<Role>, DomainError>> + Send;

    fn list_roles_page(
        &self,
        page: u32,
        page_size: u32,
        search: Option<String>,
        permission_status: Option<String>,
        sorting: Option<String>,
    ) -> impl std::future::Future<Output = Result<(Vec<Role>, i64), DomainError>> + Send;

    fn find_role(
        &self,
        role_name: &str,
    ) -> impl std::future::Future<Output = Result<Option<Role>, DomainError>> + Send;

    fn create_role(
        &self,
        role_name: &str,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn update_role(
        &self,
        role_name: &str,
        new_role_name: &str,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;

    fn delete_role(
        &self,
        role_name: &str,
    ) -> impl std::future::Future<Output = Result<bool, DomainError>> + Send;

    fn role_permissions(
        &self,
        role_name: &str,
    ) -> impl std::future::Future<Output = Result<Vec<PermissionName>, DomainError>> + Send;

    fn set_role_permissions(
        &self,
        role_name: &str,
        permissions: Vec<PermissionName>,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn ensure_identity_seed_data(
        &self,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;
}
