use leptos::prelude::*;

use crate::{
    app::auth_state::AuthState,
    shared::{
        feedback::toast::{ToastController, ToastViewport},
        i18n::I18n,
    },
};

fn identity_route_layout(title: identity_leptos::shared::i18n::T, children: ChildrenFn) -> AnyView {
    let title = match title {
        identity_leptos::shared::i18n::T::Roles => crate::shared::i18n::T::Roles,
        identity_leptos::shared::i18n::T::Users => crate::shared::i18n::T::Users,
        identity_leptos::shared::i18n::T::Profile => crate::shared::i18n::T::Profile,
        _ => crate::shared::i18n::T::Page,
    };

    view! {
        <crate::app::layout::WorkspaceRouteLayout title=title>
            {children()}
        </crate::app::layout::WorkspaceRouteLayout>
    }
    .into_any()
}

#[component]
pub fn AppProviders(children: Children) -> impl IntoView {
    let locale = leptos_support::i18n::LocaleContext::new("hegira-locale");
    provide_context(locale);
    provide_context(AuthState::new());
    provide_context(ToastController::new());
    provide_context(I18n::new(locale));
    provide_context(identity_leptos::shared::i18n::I18n::new(locale));
    provide_context(identity_leptos::app::layout::IdentityRouteLayout::new(
        identity_route_layout,
    ));

    view! {
        {children()}
        <ToastViewport/>
    }
}
