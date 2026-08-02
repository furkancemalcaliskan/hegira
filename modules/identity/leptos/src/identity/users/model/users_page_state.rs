use leptos::prelude::*;

use crate::{
    identity_application_contracts::identity::{
        authorization::RoleDto,
        users::{CreateUserInput, ListUsersInput, UpdateUserInput, UserDto},
    },
    shared::i18n::{I18n, T},
};
use leptos_support::mutation::MutationStatus;

pub const USERS_PAGE_SIZE: u32 = 20;

#[derive(Clone, Copy)]
pub struct UsersPageState {
    pub users: RwSignal<Vec<UserDto>>,
    pub roles: RwSignal<Vec<RoleDto>>,
    pub total_count: RwSignal<i64>,
    pub page: RwSignal<u32>,
    pub search: RwSignal<String>,
    pub verification: RwSignal<String>,
    pub filters_open: RwSignal<bool>,
    pub loading: RwSignal<bool>,
    pub error: RwSignal<Option<String>>,
    pub form_error: RwSignal<Option<String>>,
    pub form_open: RwSignal<bool>,
    pub editing_username: RwSignal<Option<String>>,
    pub username: RwSignal<String>,
    pub password: RwSignal<String>,
    pub is_verified: RwSignal<bool>,
    pub selected_roles: RwSignal<Vec<String>>,
    pub save_status: RwSignal<MutationStatus>,
    pub deleting_username: RwSignal<Option<String>>,
    pub delete_status: RwSignal<MutationStatus>,
}

pub enum UserSaveInput {
    Create(CreateUserInput),
    Update(UpdateUserInput),
}

impl Default for UsersPageState {
    fn default() -> Self {
        Self::new()
    }
}

impl UsersPageState {
    pub fn new() -> Self {
        Self {
            users: RwSignal::new(Vec::new()),
            roles: RwSignal::new(Vec::new()),
            total_count: RwSignal::new(0),
            page: RwSignal::new(1),
            search: RwSignal::new(String::new()),
            verification: RwSignal::new("all".to_string()),
            filters_open: RwSignal::new(false),
            loading: RwSignal::new(false),
            error: RwSignal::new(None),
            form_error: RwSignal::new(None),
            form_open: RwSignal::new(false),
            editing_username: RwSignal::new(None),
            username: RwSignal::new(String::new()),
            password: RwSignal::new(String::new()),
            is_verified: RwSignal::new(false),
            selected_roles: RwSignal::new(Vec::new()),
            save_status: RwSignal::new(MutationStatus::Idle),
            deleting_username: RwSignal::new(None),
            delete_status: RwSignal::new(MutationStatus::Idle),
        }
    }

    pub fn list_input(&self) -> ListUsersInput {
        let trimmed_search = self.search.get_untracked().trim().to_string();

        ListUsersInput {
            page: self.page.get_untracked(),
            page_size: USERS_PAGE_SIZE,
            search: (!trimmed_search.is_empty()).then_some(trimmed_search),
            sorting: Some("created_at desc".to_string()),
        }
    }

    pub fn total_pages(&self) -> u32 {
        let total = self.total_count.get();
        if total <= 0 {
            1
        } else {
            (total as u32).div_ceil(USERS_PAGE_SIZE).max(1)
        }
    }

    pub fn can_go_previous(&self) -> bool {
        self.page.get() > 1 && !self.loading.get()
    }

    pub fn can_go_next(&self) -> bool {
        self.page.get() < self.total_pages() && !self.loading.get()
    }

    pub fn open_create_form(&self) {
        self.editing_username.set(None);
        self.username.set(String::new());
        self.password.set(String::new());
        self.is_verified.set(false);
        self.selected_roles.set(Vec::new());
        self.form_error.set(None);
        self.form_open.set(true);
    }

    pub fn open_edit_form(&self, user: UserDto) {
        self.editing_username.set(Some(user.username.clone()));
        self.username.set(user.username);
        self.password.set(String::new());
        self.is_verified.set(user.is_verified);
        self.selected_roles.set(user.roles);
        self.form_error.set(None);
        self.form_open.set(true);
    }

    pub fn close_form(&self) {
        self.form_open.set(false);
        self.form_error.set(None);
        self.save_status.set(MutationStatus::Idle);
    }

    pub fn save_input(&self, i18n: I18n) -> Result<UserSaveInput, String> {
        let username = self.username.get_untracked().trim().to_string();
        let password = self.password.get_untracked();
        let is_verified = self.is_verified.get_untracked();
        let roles = self.selected_roles.get_untracked();

        if username.is_empty() {
            return Err(i18n.t_untracked(T::UsernameRequired).to_string());
        }

        if self.editing_username.get_untracked().is_none() && password.is_empty() {
            return Err(i18n.t_untracked(T::PasswordRequiredForNewUsers).to_string());
        }

        if self.editing_username.get_untracked().is_some() {
            Ok(UserSaveInput::Update(UpdateUserInput {
                username,
                password: (!password.is_empty()).then_some(password),
                is_verified,
                roles,
            }))
        } else {
            Ok(UserSaveInput::Create(CreateUserInput {
                username,
                password,
                is_verified,
                roles,
            }))
        }
    }

    pub fn toggle_role(&self, role: String, checked: bool) {
        let mut selected = self.selected_roles.get_untracked();
        if checked && !selected.iter().any(|item| item == &role) {
            selected.push(role);
        } else if !checked {
            selected.retain(|item| item != &role);
        }
        self.selected_roles.set(selected);
    }

    pub fn reset_filters(&self) {
        self.search.set(String::new());
        self.verification.set("all".to_string());
        self.filters_open.set(false);
        self.page.set(1);
    }
}
