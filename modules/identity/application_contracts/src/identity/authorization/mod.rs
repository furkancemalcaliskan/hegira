pub mod dto;
pub mod inputs;

pub use dto::{PagedRoleResultDto, PermissionDto, RoleDto};
pub use inputs::{
    AssignUserRoleInput, CreateRoleInput, ListRolesInput, SetRolePermissionsInput, UpdateRoleInput,
};
