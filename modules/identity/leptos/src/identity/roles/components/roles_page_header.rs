use icons::Plus;
use leptos::prelude::*;

use crate::{
    application_contracts::identity::permissions,
    web::shared::{
        authorization::PermissionGate,
        i18n::{T, use_i18n},
        rust_ui::ui::button::{Button, ButtonVariant},
    },
};

#[component]
pub fn RolesPageHeader(on_new: Callback<()>) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <header class="page-header items-center">
            <div class="min-w-0">
                <h1>{move || i18n.t(T::Roles)}</h1>
                <p>{move || i18n.t(T::RolesDescription)}</p>
            </div>
            <div class="page-header-actions">
                <PermissionGate permission=permissions::AUTHORIZATION>
                    <Button
                        variant=ButtonVariant::Default
                        on:click=move |_| on_new.run(())
                    >
                        <Plus class="size-4".to_string()/>
                        {move || i18n.t(T::AddRole)}
                    </Button>
                </PermissionGate>
            </div>
        </header>
    }
}
