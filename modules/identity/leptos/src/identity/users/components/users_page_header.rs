use icons::Plus;
use leptos::prelude::*;

use crate::{
    identity_application_contracts::identity::permissions,
    shared::{
        authorization::PermissionGate,
        i18n::{T, use_i18n},
        rust_ui::ui::button::{Button, ButtonVariant},
    },
};

#[component]
pub fn UsersPageHeader(on_new: Callback<()>) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <header class="page-header items-center">
            <div class="min-w-0">
                <h1>{move || i18n.t(T::Users)}</h1>
                <p>{move || i18n.t(T::UsersDescription)}</p>
            </div>
            <div class="page-header-actions">
                <PermissionGate permission=permissions::USERS_CREATE>
                    <Button
                        variant=ButtonVariant::Default
                        on:click=move |_| on_new.run(())
                    >
                        <Plus class="size-4".to_string()/>
                        {move || i18n.t(T::AddUser)}
                    </Button>
                </PermissionGate>
            </div>
        </header>
    }
}
