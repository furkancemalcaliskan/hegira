use leptos::prelude::*;

#[component]
pub fn AppShell(children: Children) -> impl IntoView {
    view! {
        <div class="app-root">
            {children()}
        </div>
    }
}
