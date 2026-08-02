use leptos::prelude::*;

use crate::{
    identity_application_contracts::identity::authorization::{
        CreateRoleInput, ListRolesInput, PermissionDto, RoleDto, UpdateRoleInput,
    },
    shared::i18n::{I18n, T},
};
use leptos_support::mutation::MutationStatus;

pub const ROLES_PAGE_SIZE: u32 = 20;

#[derive(Clone, Copy)]
pub struct RolesPageState {
    pub roles: RwSignal<Vec<RoleDto>>,
    pub permissions: RwSignal<Vec<PermissionDto>>,
    pub total_count: RwSignal<i64>,
    pub page: RwSignal<u32>,
    pub search: RwSignal<String>,
    pub filters_open: RwSignal<bool>,
    pub permission_filter: RwSignal<String>,
    pub loading: RwSignal<bool>,
    pub error: RwSignal<Option<String>>,
    pub form_error: RwSignal<Option<String>>,
    pub form_open: RwSignal<bool>,
    pub editing_role: RwSignal<Option<String>>,
    pub role_name: RwSignal<String>,
    pub deleting_role: RwSignal<Option<String>>,
    pub permissions_open: RwSignal<bool>,
    pub permissions_role: RwSignal<Option<String>>,
    pub selected_permissions: RwSignal<Vec<String>>,
    pub mutation_status: RwSignal<MutationStatus>,
}

pub enum RoleSaveInput {
    Create(CreateRoleInput),
    Update(UpdateRoleInput),
}

impl Default for RolesPageState {
    fn default() -> Self {
        Self::new()
    }
}

impl RolesPageState {
    pub fn new() -> Self {
        Self {
            roles: RwSignal::new(Vec::new()),
            permissions: RwSignal::new(Vec::new()),
            total_count: RwSignal::new(0),
            page: RwSignal::new(1),
            search: RwSignal::new(String::new()),
            filters_open: RwSignal::new(false),
            permission_filter: RwSignal::new("all".to_string()),
            loading: RwSignal::new(false),
            error: RwSignal::new(None),
            form_error: RwSignal::new(None),
            form_open: RwSignal::new(false),
            editing_role: RwSignal::new(None),
            role_name: RwSignal::new(String::new()),
            deleting_role: RwSignal::new(None),
            permissions_open: RwSignal::new(false),
            permissions_role: RwSignal::new(None),
            selected_permissions: RwSignal::new(Vec::new()),
            mutation_status: RwSignal::new(MutationStatus::Idle),
        }
    }

    pub fn open_create_form(&self) {
        self.editing_role.set(None);
        self.role_name.set(String::new());
        self.form_error.set(None);
        self.form_open.set(true);
    }

    pub fn open_edit_form(&self, role: RoleDto) {
        self.editing_role.set(Some(role.name.clone()));
        self.role_name.set(role.name);
        self.form_error.set(None);
        self.form_open.set(true);
    }

    pub fn close_form(&self) {
        self.form_open.set(false);
        self.form_error.set(None);
        self.mutation_status.set(MutationStatus::Idle);
    }

    pub fn open_permissions(&self, role: RoleDto) {
        self.permissions_role.set(Some(role.name));
        self.selected_permissions.set(role.permissions);
        self.permissions_open.set(true);
    }

    pub fn close_permissions(&self) {
        self.permissions_open.set(false);
        self.permissions_role.set(None);
        self.selected_permissions.set(Vec::new());
        self.mutation_status.set(MutationStatus::Idle);
    }

    pub fn toggle_permission(&self, permission: String, checked: bool) {
        let mut selected = self.selected_permissions.get_untracked();
        if checked && !selected.iter().any(|item| item == &permission) {
            selected.push(permission);
        } else if !checked {
            selected.retain(|item| item != &permission);
        }
        self.selected_permissions.set(selected);
    }

    pub fn reset_filters(&self) {
        self.search.set(String::new());
        self.permission_filter.set("all".to_string());
        self.filters_open.set(false);
        self.page.set(1);
    }

    pub fn list_input(&self) -> ListRolesInput {
        let search = self.search.get_untracked().trim().to_string();
        let permission_status = self.permission_filter.get_untracked();

        ListRolesInput {
            page: self.page.get_untracked(),
            page_size: ROLES_PAGE_SIZE,
            search: (!search.is_empty()).then_some(search),
            permission_status: (permission_status != "all").then_some(permission_status),
            sorting: Some("name asc".to_string()),
        }
    }

    pub fn total_pages(&self) -> u32 {
        let total = self.total_count.get();
        if total <= 0 {
            1
        } else {
            (total as u32).div_ceil(ROLES_PAGE_SIZE).max(1)
        }
    }

    pub fn can_go_previous(&self) -> bool {
        self.page.get() > 1 && !self.loading.get()
    }

    pub fn can_go_next(&self) -> bool {
        self.page.get() < self.total_pages() && !self.loading.get()
    }

    pub fn save_input(&self, i18n: I18n) -> Result<RoleSaveInput, String> {
        let role_name = self.role_name.get_untracked().trim().to_string();
        if role_name.is_empty() {
            return Err(i18n.t_untracked(T::RoleNameRequired).to_string());
        }

        if let Some(current_name) = self.editing_role.get_untracked() {
            Ok(RoleSaveInput::Update(UpdateRoleInput {
                name: current_name,
                new_name: role_name,
            }))
        } else {
            Ok(RoleSaveInput::Create(CreateRoleInput { name: role_name }))
        }
    }
}
