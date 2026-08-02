use crate::{
    app::auth_state::AuthState,
    identity_application_contracts::identity::permissions::PermissionName,
};
use leptos::prelude::*;

pub fn can_access(permission: Option<PermissionName>) -> bool {
    permission.is_none_or(can)
}

pub fn can_access_untracked(permission: Option<PermissionName>) -> bool {
    permission.is_none_or(can_untracked)
}

pub fn can(permission: PermissionName) -> bool {
    use_context::<AuthState>().is_some_and(|auth| auth.has_permission(permission.0))
}

pub fn can_untracked(permission: PermissionName) -> bool {
    use_context::<AuthState>().is_some_and(|auth| auth.has_permission_untracked(permission.0))
}

#[component]
pub fn PermissionGate(permission: PermissionName, children: ChildrenFn) -> impl IntoView {
    let auth = use_context::<AuthState>().unwrap_or_default();

    view! {
        <Show when=move || auth.has_permission(permission.0)>
            {children()}
        </Show>
    }
}
