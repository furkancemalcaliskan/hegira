#![cfg(feature = "ssr")]

use std::collections::BTreeSet;

#[test]
fn host_leptos_composition_preserves_identity_and_dashboard_routes() {
    let actual = leptos_axum::generate_route_list(hegira::web::root::App)
        .into_iter()
        .map(|route| route.path().to_string())
        .collect::<BTreeSet<_>>();
    let expected = [
        "/",
        "/admin/roles",
        "/admin/users",
        "/dashboard",
        "/login",
        "/oauth/{provider}/callback",
        "/profile",
        "/register",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();

    assert_eq!(actual, expected);
}
