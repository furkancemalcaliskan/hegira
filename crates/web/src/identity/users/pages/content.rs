use leptos::prelude::*;

use crate::{
    app::{
        layout::WorkspaceRouteLayout,
        page::{PageHeaderKey, PageSection},
    },
    shared::i18n::{T, use_i18n},
};

#[component]
pub fn ContentRoute() -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <WorkspaceRouteLayout title=T::Home>
            <div class="page-stack">
                <PageHeaderKey title=T::Home description=T::HomeDescription/>

                <div class="dashboard-grid">
                    <PageSection>
                        <span class="metric-label">{move || i18n.t(T::Architecture)}</span>
                        <strong class="metric-value">{move || i18n.t(T::DddReady)}</strong>
                        <p>{move || i18n.t(T::ArchitectureDescription)}</p>
                    </PageSection>

                    <PageSection>
                        <span class="metric-label">{move || i18n.t(T::Frontend)}</span>
                        <strong class="metric-value">{move || i18n.t(T::RustUi)}</strong>
                        <p>{move || i18n.t(T::FrontendDescription)}</p>
                    </PageSection>

                    <PageSection>
                        <span class="metric-label">{move || i18n.t(T::Delivery)}</span>
                        <strong class="metric-value">{move || i18n.t(T::SingleBinary)}</strong>
                        <p>{move || i18n.t(T::DeliveryDescription)}</p>
                    </PageSection>
                </div>
            </div>
        </WorkspaceRouteLayout>
    }
}
