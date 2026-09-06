use leptos::prelude::*;
#[cfg(feature = "wasm-split")]
use leptos_router::{Lazy, LazyRoute, lazy_route};
use leptos_router::{MatchNestedRoutes, NestedRoute, ParamSegment, StaticSegment};

use crate::{
    app::protected::RequirePermission,
    identity::{
        auth::pages::{
            login::LoginRoute, oauth_callback::OAuthCallbackRoute, register::RegisterRoute,
        },
        roles::pages::roles_index::RolesIndexRoute,
        users::pages::{profile::ProfileRoute, users_index::UsersIndexRoute},
    },
    identity_application_contracts::identity::permissions,
};

pub const ROUTE_PATHS: &[&str] = &[
    "/",
    "/login",
    "/register",
    "/oauth/:provider/callback",
    "/admin/roles",
    "/admin/users",
    "/profile",
];

#[component(transparent)]
#[allow(non_snake_case)]
pub fn IdentityRoutes() -> impl MatchNestedRoutes + Clone + Send + 'static {
    routes()
}

#[cfg(not(feature = "wasm-split"))]
pub fn routes() -> impl MatchNestedRoutes + Clone + Send + 'static {
    (
        NestedRoute::new(StaticSegment(""), LoginRoute),
        NestedRoute::new(StaticSegment("login"), LoginRoute),
        NestedRoute::new(StaticSegment("register"), RegisterRoute),
        NestedRoute::new(
            (
                StaticSegment("oauth"),
                ParamSegment("provider"),
                StaticSegment("callback"),
            ),
            OAuthCallbackRoute,
        ),
        NestedRoute::new((StaticSegment("admin"), StaticSegment("roles")), || {
            view! {
                <RequirePermission permission=permissions::AUTHORIZATION>
                    <RolesIndexRoute/>
                </RequirePermission>
            }
        }),
        NestedRoute::new((StaticSegment("admin"), StaticSegment("users")), || {
            view! {
                <RequirePermission permission=permissions::USERS>
                    <UsersIndexRoute/>
                </RequirePermission>
            }
        }),
        NestedRoute::new(StaticSegment("profile"), ProfileRoute),
    )
}

#[cfg(feature = "wasm-split")]
pub fn routes() -> impl MatchNestedRoutes + Clone + Send + 'static {
    (
        NestedRoute::new(StaticSegment(""), LoginRoute),
        NestedRoute::new(StaticSegment("login"), LoginRoute),
        NestedRoute::new(StaticSegment("register"), RegisterRoute),
        NestedRoute::new(
            (
                StaticSegment("oauth"),
                ParamSegment("provider"),
                StaticSegment("callback"),
            ),
            OAuthCallbackRoute,
        ),
        NestedRoute::new(
            (StaticSegment("admin"), StaticSegment("roles")),
            Lazy::<RolesLazyRoute>::new(),
        ),
        NestedRoute::new(
            (StaticSegment("admin"), StaticSegment("users")),
            Lazy::<UsersLazyRoute>::new(),
        ),
        NestedRoute::new(StaticSegment("profile"), Lazy::<ProfileLazyRoute>::new()),
    )
}

#[derive(Debug)]
#[cfg(feature = "wasm-split")]
struct RolesLazyRoute;

#[derive(Debug)]
#[cfg(feature = "wasm-split")]
struct UsersLazyRoute;

#[derive(Debug)]
#[cfg(feature = "wasm-split")]
struct ProfileLazyRoute;

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

#[cfg(test)]
mod tests {
    use super::{ROUTE_PATHS, routes};
    use leptos_router::{MatchNestedRoutes, PathSegment};

    #[test]
    fn contribution_registers_only_the_declared_identity_routes() {
        let generated = routes()
            .generate_routes()
            .into_iter()
            .map(|route| {
                let path = route
                    .segments
                    .iter()
                    .map(|segment| match segment {
                        PathSegment::Unit => String::new(),
                        PathSegment::Static(value) => value.to_string(),
                        PathSegment::Param(value) => format!(":{value}"),
                        PathSegment::OptionalParam(value) => format!(":{value}?"),
                        PathSegment::Splat(value) => format!("*{value}"),
                    })
                    .collect::<Vec<_>>()
                    .join("/");
                format!("/{path}")
            })
            .collect::<Vec<_>>();

        assert_eq!(generated, ROUTE_PATHS);
        assert!(!generated.iter().any(|path| path == "/dashboard"));
    }
}
