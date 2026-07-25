pub mod identity;

use axum::Router;

use crate::http::feature::HttpFeatureDescriptor;

pub const FEATURES: &[HttpFeatureDescriptor] = &[identity::DESCRIPTOR];

pub fn routes() -> Router {
    FEATURES.iter().fold(Router::new(), |router, feature| {
        tracing::debug!(feature = feature.key, "registering HTTP feature");
        router.nest(feature.path, (feature.routes)())
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn http_registry_matches_declared_feature_keys() {
        let registered_keys = super::FEATURES
            .iter()
            .map(|feature| feature.key)
            .collect::<Vec<_>>();
        assert_eq!(registered_keys, ["identity"]);

        for feature in super::FEATURES {
            assert!(application_contracts::features::descriptor(feature.key).is_some());
        }
    }
}
