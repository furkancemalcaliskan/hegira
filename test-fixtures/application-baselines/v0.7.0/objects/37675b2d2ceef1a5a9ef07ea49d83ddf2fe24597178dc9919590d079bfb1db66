use leptos::prelude::*;
use leptos_router::{
    StaticSegment,
    components::{Route, Router, Routes},
};

use crate::dashboard::DashboardRoute;

#[component]
pub fn WebRoutes() -> impl IntoView {
    view! {
        <Router>
            <main class="minimal-shell">
                <Routes fallback=|| view! { <p>"Page not found."</p> }.into_view()>
                    <Route path=StaticSegment("") view=DashboardRoute/>
                    <Route path=StaticSegment("dashboard") view=DashboardRoute/>
                    // hegira:resource-routes-native
                    // hegira:resource-routes-native:end
                </Routes>
            </main>
        </Router>
    }
}
