use leptos::prelude::*;

#[derive(Clone, Debug)]
pub struct AuthState {
    authenticated: RwSignal<bool>,
    username: RwSignal<Option<String>>,
    permissions: RwSignal<Vec<String>>,
}

impl AuthState {
    pub fn new() -> Self {
        Self {
            authenticated: RwSignal::new(false),
            username: RwSignal::new(None),
            permissions: RwSignal::new(Vec::new()),
        }
    }

    pub fn username(&self) -> Option<String> {
        self.username.get()
    }

    pub fn permissions(&self) -> Vec<String> {
        self.permissions.get()
    }

    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions
            .get()
            .iter()
            .any(|current| current == permission)
    }

    pub fn has_permission_untracked(&self, permission: &str) -> bool {
        self.permissions
            .get_untracked()
            .iter()
            .any(|current| current == permission)
    }

    pub fn set_authenticated(&self, username: Option<String>, permissions: Vec<String>) {
        self.authenticated.set(true);
        self.username.set(username);
        self.permissions.set(permissions);
    }

    pub fn clear(&self) {
        self.authenticated.set(false);
        self.username.set(None);
        self.permissions.set(Vec::new());
    }

    pub fn is_authenticated(&self) -> bool {
        self.authenticated.get()
    }

    pub fn is_authenticated_untracked(&self) -> bool {
        self.authenticated.get_untracked()
    }
}

impl Default for AuthState {
    fn default() -> Self {
        Self::new()
    }
}
