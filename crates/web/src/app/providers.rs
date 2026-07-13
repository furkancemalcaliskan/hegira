use leptos::prelude::*;

use crate::{
    app::auth_state::AuthState,
    shared::{
        feedback::toast::{ToastController, ToastViewport},
        i18n::I18n,
    },
};

#[component]
pub fn AppProviders(children: Children) -> impl IntoView {
    provide_context(AuthState::new());
    provide_context(ToastController::new());
    provide_context(I18n::default());

    view! {
        {children()}
        <ToastViewport/>
    }
}
