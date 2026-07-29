use leptos::prelude::*;
#[cfg(feature = "wasm-split")]
use leptos_router::{Lazy, LazyRoute, lazy_route};
use leptos_router::{
    StaticSegment,
    components::{Route, Router, Routes},
};

use crate::web::{
    app::{dashboard::DashboardRoute, shell::AppShell},
    identity::routes::IdentityRoutes,
    shared::feedback::not_found::NotFound,
};

// hegira:route-imports
// hegira:route-imports:end

#[component]
#[cfg(not(feature = "wasm-split"))]
pub fn WebRoutes() -> impl IntoView {
    view! {
        <Router>
            <AppShell>
                <Routes fallback=|| view! { <NotFound/> }.into_view()>
                    <IdentityRoutes/>
                    <Route path=StaticSegment("dashboard") view=DashboardRoute/>

                    // hegira:routes
                    // hegira:routes:end
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

                    // hegira:lazy-routes
                    // hegira:lazy-routes:end
                </Routes>
            </AppShell>
        </Router>
    }
}

#[derive(Debug)]
#[cfg(feature = "wasm-split")]
struct DashboardLazyRoute;

// hegira:lazy-route-structs
// hegira:lazy-route-structs:end

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

// hegira:lazy-route-impls
// hegira:lazy-route-impls:end
