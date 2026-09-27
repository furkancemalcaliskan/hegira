use leptos::prelude::*;

use crate::shared::i18n::{T, use_i18n};

#[component]
pub fn PageHeader(
    title: &'static str,
    #[prop(optional)] description: &'static str,
) -> impl IntoView {
    view! {
        <header class="page-header">
            <div class="min-w-0">
                <h1>{title}</h1>
                {(!description.is_empty()).then(|| view! { <p>{description}</p> })}
            </div>
        </header>
    }
}

#[component]
pub fn PageHeaderKey(title: T, #[prop(optional)] description: Option<T>) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <header class="page-header">
            <div class="min-w-0">
                <h1>{move || i18n.t(title)}</h1>
                {description.map(|description| view! { <p>{move || i18n.t(description)}</p> })}
            </div>
        </header>
    }
}

#[component]
pub fn PageSection(children: Children, #[prop(into, optional)] class: String) -> impl IntoView {
    let class = if class.is_empty() {
        "page-section".to_string()
    } else {
        format!("page-section {class}")
    };

    view! { <section class=class>{children()}</section> }
}
