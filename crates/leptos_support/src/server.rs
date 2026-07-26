use leptos::prelude::{ServerFnError, expect_context};
use std::fmt::Display;

pub fn context<T>() -> T
where
    T: Clone + 'static,
{
    expect_context::<T>()
}

pub fn public_error(message: impl Display) -> ServerFnError {
    ServerFnError::new(message.to_string())
}

pub fn internal_error(error: impl Display) -> ServerFnError {
    tracing::error!(error = %error, "server function failed");
    ServerFnError::new("internal server error")
}

pub use leptos::prelude::ServerFnError as Error;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_errors_do_not_expose_the_original_message() {
        let error = internal_error("database credentials leaked");

        assert!(error.to_string().contains("internal server error"));
        assert!(!error.to_string().contains("credentials"));
    }
}
