use leptos::prelude::*;

use crate::identity::auth::pages::login::LoginRoute;

#[component]
pub fn RegisterRoute() -> impl IntoView {
    view! { <LoginRoute/> }
}
