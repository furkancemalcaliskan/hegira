use leptos::prelude::*;
use leptos_ui::clx;

use crate::shared::rust_ui::ui::button::{Button, ButtonSize, ButtonVariant};

clx! {Pagination, nav, "mx-auto flex w-full justify-center"}
clx! {PaginationList, ul, "flex flex-row items-center gap-1"}
clx! {PaginationItem, li, ""}

#[component]
pub fn PaginationLink(
    children: Children,
    #[prop(optional)] active: bool,
    #[prop(optional, into)] class: String,
    #[prop(optional)] on_click: Option<Callback<()>>,
    #[prop(default = Signal::derive(|| false), into)] disabled: Signal<bool>,
) -> impl IntoView {
    let variant = if active {
        ButtonVariant::Outline
    } else {
        ButtonVariant::Ghost
    };

    view! {
        <Button
            variant=variant
            size=ButtonSize::Icon
            class=class
            attr:aria-current=active.then_some("page")
            attr:disabled=move || disabled.get().then_some("true")
            on:click=move |ev| {
                ev.prevent_default();
                if !disabled.get_untracked() && let Some(on_click) = on_click {
                    on_click.run(());
                }
            }
        >
            {children()}
        </Button>
    }
}

#[component]
pub fn PaginationNavButton(
    children: Children,
    on_click: Callback<()>,
    #[prop(default = Signal::derive(|| false), into)] disabled: Signal<bool>,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    view! {
        <Button
            variant=ButtonVariant::Ghost
            size=ButtonSize::Default
            class=class
            attr:disabled=move || disabled.get().then_some("true")
            on:click=move |ev| {
                ev.prevent_default();
                if !disabled.get_untracked() {
                    on_click.run(());
                }
            }
        >
            {children()}
        </Button>
    }
}
