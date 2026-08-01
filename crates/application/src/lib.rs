pub use domain_shared::{common as identity_common, identity as identity_shared};

#[path = "../../../modules/identity/application/src/identity/mod.rs"]
pub mod identity;
pub mod shared;
