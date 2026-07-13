use leptos::prelude::*;
use leptos_router::hooks::use_location;

pub struct QUERY;

impl QUERY {
    pub const PAGE: &'static str = "page";
}

pub struct QueryUtils;

impl QueryUtils {
    pub fn extract(key: String) -> impl Fn() -> Option<String> + Clone + 'static {
        let location = use_location();

        move || location.query.with(|query| query.get(&key))
    }
}
