use leptos::prelude::*;
use leptos_router::components::A;

use crate::shared::i18n::{T, use_i18n};
use identity_leptos::app::auth_state::AuthState;
use leptos_support::rust_ui::ui::{
    button::{Button, ButtonVariant},
    card::{Card, CardContent, CardDescription, CardHeader, CardTitle},
};

#[component]
pub fn NotFound() -> impl IntoView {
    let i18n = use_i18n();
    let auth = use_context::<AuthState>().unwrap_or_default();
    let is_authorized = Signal::derive(move || auth.is_authenticated());
    let target_href = move || {
        if is_authorized.get() {
            "/dashboard".to_string()
        } else {
            "/".to_string()
        }
    };
    let target_label = move || {
        if is_authorized.get() {
            i18n.t(T::GoHome)
        } else {
            i18n.t(T::GoLogin)
        }
    };

    view! {
        <section class="route-fade grid min-h-screen place-items-center bg-background p-6 text-foreground">
            <Card class="w-full max-w-md text-center".to_string()>
                <CardHeader>
                    <CardTitle class="text-3xl".to_string()>{move || i18n.t(T::PageNotFound)}</CardTitle>
                    <CardDescription>{move || i18n.t(T::PageNotFoundDescription)}</CardDescription>
                </CardHeader>
                <CardContent>
                    <Button variant=ButtonVariant::Default class="w-full".to_string()>
                        <A href=target_href attr:class="inline-flex w-full items-center justify-center">
                            {target_label}
                        </A>
                    </Button>
                </CardContent>
            </Card>
        </section>
    }
}
