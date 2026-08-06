use leptos::prelude::*;
#[cfg(feature = "wasm-split")]
use leptos_router::{Lazy, LazyRoute, lazy_route};
use leptos_router::{
    StaticSegment,
    components::{Route, Router, Routes},
};

use identity_leptos::identity::routes::IdentityRoutes;

use crate::{
    app::shell::AppShell, dashboard::DashboardRoute, shared::feedback::not_found::NotFound,
};

#[component]
#[cfg(not(feature = "wasm-split"))]
pub fn WebRoutes() -> impl IntoView {
    view! {
        <Router>
            <AppShell>
                <Routes fallback=|| view! { <NotFound/> }.into_view()>
                    <IdentityRoutes/>
                    <Route path=StaticSegment("dashboard") view=DashboardRoute/>
                </Routes>
            </AppShell>
        </Router>
    }
}

#[component]
#[cfg(feature = "wasm-split")]
pub fn WebRoutes() -> impl IntoView {
    view! {
        <Router>
            <AppShell>
                <Routes fallback=|| view! { <NotFound/> }.into_view()>
                    <IdentityRoutes/>
                    <Route path=StaticSegment("dashboard") view={Lazy::<DashboardLazyRoute>::new()}/>
                </Routes>
            </AppShell>
        </Router>
    }
}

#[derive(Debug)]
#[cfg(feature = "wasm-split")]
struct DashboardLazyRoute;

#[lazy_route]
#[cfg(feature = "wasm-split")]
impl LazyRoute for DashboardLazyRoute {
    fn data() -> Self {
        Self
    }

    fn view(_this: Self) -> AnyView {
        view! { <DashboardRoute/> }.into_any()
    }
}
