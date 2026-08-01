use icons::{Filter, RotateCcw, Search};
use leptos::prelude::*;

use crate::{
    identity::users::model::users_page_state::UsersPageState,
    shared::{
        i18n::{T, use_i18n},
        rust_ui::ui::{
            button::{Button, ButtonVariant},
            input::{Input, InputType},
            label::Label,
            select_native::SelectNative,
        },
    },
};

#[component]
pub fn UsersFilters(
    state: UsersPageState,
    on_search: Callback<()>,
    on_reset: Callback<()>,
) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <div class="grid grid-cols-1 items-start gap-3 md:grid-cols-[minmax(18rem,1fr)_auto]">
            <div class="relative flex items-center">
                <Search class="absolute left-3 z-10 size-4 text-muted-foreground".to_string()/>
                <Input
                    class="pl-9".to_string()
                    r#type=InputType::Search
                    placeholder=i18n.t_untracked(T::Username).to_string()
                    bind_value=state.search
                />
            </div>

            <div class="flex flex-wrap items-center gap-2">
                <Button
                    variant=ButtonVariant::Default
                    on:click=move |_| {
                        if !state.loading.get_untracked() {
                            on_search.run(());
                        }
                    }
                >
                    <Search class="size-4".to_string()/>
                    {move || i18n.t(T::Search)}
                </Button>
                <Button
                    variant=ButtonVariant::Outline
                    on:click=move |_| state.filters_open.set(!state.filters_open.get())
                >
                    <Filter class="size-4".to_string()/>
                    {move || i18n.t(T::Filter)}
                </Button>
                <Button
                    variant=ButtonVariant::Ghost
                    on:click=move |_| on_reset.run(())
                >
                    <RotateCcw class="size-4".to_string()/>
                    {move || i18n.t(T::Reset)}
                </Button>
            </div>

            <div class=move || {
                if state.filters_open.get() {
                    "filter-panel is-open md:col-span-2"
                } else {
                    "filter-panel md:col-span-2"
                }
            }>
                <div>
                    <div class="grid gap-4 rounded-md border bg-muted/30 p-4 md:grid-cols-[minmax(12rem,18rem)]">
                                <div class="grid gap-2">
                                    <Label html_for="users-verification">{move || i18n.t(T::Verification)}</Label>
                                    <SelectNative
                                        id="users-verification"
                                        value=state.verification.read_only()
                                        on_change=Callback::new(move |ev| {
                                            state.verification.set(event_target_value(&ev));
                                        })
                                    >
                                        <option value="all">{move || i18n.t(T::AllUsers)}</option>
                                        <option value="verified">{move || i18n.t(T::Verified)}</option>
                                        <option value="unverified">{move || i18n.t(T::Pending)}</option>
                                    </SelectNative>
                                </div>
                    </div>
                </div>
            </div>
        </div>
    }
}
