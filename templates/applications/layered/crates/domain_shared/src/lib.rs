//! Cross-cutting domain values shared by the generated application layers.
//!
//! Keep this package independent from transports, persistence providers, and
//! framework adapters.

/// The locale used when an application has not selected another locale.
pub const DEFAULT_LOCALE: &str = "en";
