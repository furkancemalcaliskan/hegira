use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_navigate;

use crate::{
    application_contracts::identity::permissions::PermissionName,
    web::{
        app::auth_state::AuthState, identity::auth::server_fns::current_user,
        shared::feedback::unauthorized::Unauthorized,
    },
};

pub fn is_authenticated() -> bool {
    use_context::<AuthState>().is_some_and(|state| state.is_authenticated())
}

pub fn redirect_to_login_if_unauthenticated() {
    let navigate = use_navigate();

    Effect::new(move |_| {
        if !is_authenticated() {
            navigate("/", Default::default());
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthGateStatus {
    Checking,
    Authenticated,
}

pub fn use_protected_auth_gate() -> RwSignal<AuthGateStatus> {
    let navigate = use_navigate();
    let auth = use_context::<AuthState>().unwrap_or_default();
    let status = RwSignal::new(AuthGateStatus::Checking);

    Effect::new(move |_| {
        if auth.is_authenticated_untracked() {
            status.set(AuthGateStatus::Authenticated);
            return;
        }

        let auth = auth.clone();
        let navigate = navigate.clone();
        spawn_local(async move {
            match current_user().await {
                Ok(user) => {
                    auth.set_authenticated(Some(user.username), user.permissions);
                    status.set(AuthGateStatus::Authenticated);
                }
                Err(_) => {
                    auth.clear();
                    status.set(AuthGateStatus::Checking);
                    navigate("/", Default::default());
                }
            }
        });
    });

    status
}

#[component]
pub fn RequirePermission(permission: PermissionName, children: ChildrenFn) -> impl IntoView {
    let auth = use_context::<AuthState>().unwrap_or_default();
    let status = use_protected_auth_gate();

    view! {
        {move || match status.get() {
            AuthGateStatus::Checking => view! {
                <main class="min-h-screen bg-background" aria-busy="true"></main>
            }
                .into_any(),
            AuthGateStatus::Authenticated if auth.has_permission(permission.0) => children().into_any(),
            AuthGateStatus::Authenticated => view! { <Unauthorized/> }.into_any(),
        }}
    }
}
