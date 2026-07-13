use leptos::prelude::*;

use crate::{
    app::{
        protected::{AuthGateStatus, use_protected_auth_gate},
        sidebar::Sidebar,
        topbar::Topbar,
    },
    shared::i18n::T,
};

#[component]
pub fn WorkspaceLayout(
    page_title: Signal<T>,
    sidebar_open: RwSignal<bool>,
    sidebar_collapsed: RwSignal<bool>,
    children: ChildrenFn,
) -> impl IntoView {
    let workspace_class = move || {
        if sidebar_collapsed.get() {
            "workspace-layout is-sidebar-collapsed"
        } else {
            "workspace-layout"
        }
    };

    view! {
        <div class=workspace_class>
            <Sidebar sidebar_open=sidebar_open sidebar_collapsed=sidebar_collapsed/>
            <div class="workspace-main">
                <Topbar page_title=page_title sidebar_open=sidebar_open/>
                <main class="workspace-content">
                    <div class="route-fade">
                        {children()}
                    </div>
                </main>
            </div>
        </div>
    }
}

#[component]
pub fn WorkspaceRouteLayout(title: T, children: ChildrenFn) -> impl IntoView {
    let auth_status = use_protected_auth_gate();
    let sidebar_open = RwSignal::new(false);
    let sidebar_collapsed = RwSignal::new(false);
    let page_title = Signal::derive(move || title);

    view! {
        {move || {
            if auth_status.get() == AuthGateStatus::Authenticated {
                let children = children.clone();
                view! {
                    <WorkspaceLayout
                        page_title=page_title
                        sidebar_open=sidebar_open
                        sidebar_collapsed=sidebar_collapsed
                    >
                        {children()}
                    </WorkspaceLayout>
                }
                    .into_any()
            } else {
                view! { <div class="web-loading" /> }.into_any()
            }
        }}
    }
}
