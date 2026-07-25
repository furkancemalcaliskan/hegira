use leptos::prelude::*;
#[cfg(feature = "wasm-split")]
use leptos_router::{Lazy, LazyRoute, lazy_route};
use leptos_router::{
    ParamSegment, StaticSegment,
    components::{Route, Router, Routes},
};

use crate::{
    application_contracts::identity::permissions,
    web::{
        app::{dashboard::DashboardRoute, protected::RequirePermission, shell::AppShell},
        identity::{
            auth::pages::{login::LoginRoute, oauth_callback::OAuthCallbackRoute},
            roles::pages::roles_index::RolesIndexRoute,
            users::pages::{profile::ProfileRoute, users_index::UsersIndexRoute},
        },
        shared::feedback::not_found::NotFound,
    },
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
                    <Route path=StaticSegment("") view=LoginRoute/>
                    <Route path=StaticSegment("login") view=LoginRoute/>
                    <Route
                        path=(StaticSegment("oauth"), ParamSegment("provider"), StaticSegment("callback"))
                        view=OAuthCallbackRoute
                    />
                    <Route path=StaticSegment("dashboard") view=DashboardRoute/>
                    <Route
                        path=(StaticSegment("admin"), StaticSegment("roles"))
                        view=|| view! {
                            <RequirePermission permission=permissions::AUTHORIZATION>
                                <RolesIndexRoute/>
                            </RequirePermission>
                        }
                    />
                    <Route
                        path=(StaticSegment("admin"), StaticSegment("users"))
                        view=|| view! {
                            <RequirePermission permission=permissions::USERS>
                                <UsersIndexRoute/>
                            </RequirePermission>
                        }
                    />
                    <Route path=StaticSegment("profile") view=ProfileRoute/>

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
                    <Route path=StaticSegment("") view=LoginRoute/>
                    <Route path=StaticSegment("login") view=LoginRoute/>
                    <Route
                        path=(StaticSegment("oauth"), ParamSegment("provider"), StaticSegment("callback"))
                        view=OAuthCallbackRoute
                    />
                    <Route path=StaticSegment("dashboard") view={Lazy::<DashboardLazyRoute>::new()}/>
                    <Route
                        path=(StaticSegment("admin"), StaticSegment("roles"))
                        view={Lazy::<RolesLazyRoute>::new()}
                    />
                    <Route
                        path=(StaticSegment("admin"), StaticSegment("users"))
                        view={Lazy::<UsersLazyRoute>::new()}
                    />
                    <Route path=StaticSegment("profile") view={Lazy::<ProfileLazyRoute>::new()}/>

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

#[derive(Debug)]
#[cfg(feature = "wasm-split")]
struct RolesLazyRoute;

#[derive(Debug)]
#[cfg(feature = "wasm-split")]
struct UsersLazyRoute;

#[derive(Debug)]
#[cfg(feature = "wasm-split")]
struct ProfileLazyRoute;

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

#[lazy_route]
#[cfg(feature = "wasm-split")]
impl LazyRoute for RolesLazyRoute {
    fn data() -> Self {
        Self
    }

    fn view(_this: Self) -> AnyView {
        view! {
            <RequirePermission permission=permissions::AUTHORIZATION>
                <RolesIndexRoute/>
            </RequirePermission>
        }
        .into_any()
    }
}

#[lazy_route]
#[cfg(feature = "wasm-split")]
impl LazyRoute for UsersLazyRoute {
    fn data() -> Self {
        Self
    }

    fn view(_this: Self) -> AnyView {
        view! {
            <RequirePermission permission=permissions::USERS>
                <UsersIndexRoute/>
            </RequirePermission>
        }
        .into_any()
    }
}

#[lazy_route]
#[cfg(feature = "wasm-split")]
impl LazyRoute for ProfileLazyRoute {
    fn data() -> Self {
        Self
    }

    fn view(_this: Self) -> AnyView {
        view! { <ProfileRoute/> }.into_any()
    }
}

// hegira:lazy-route-impls
// hegira:lazy-route-impls:end
