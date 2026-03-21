use axum::http::{HeaderName, HeaderValue, Method};
use tower_http::cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer};

use lattice_config::CorsConfig;

/// Build a [`CorsLayer`] from the gateway's CORS configuration.
///
/// When origins is empty, all origins are allowed (permissive default for
/// development). In production, callers should always specify explicit origins.
pub fn build_cors_layer(config: &CorsConfig) -> CorsLayer {
    let origins = if config.origins.is_empty() {
        AllowOrigin::any()
    } else {
        let values: Vec<HeaderValue> = config.origins.iter().filter_map(|o| o.parse().ok()).collect();
        AllowOrigin::list(values)
    };

    let methods: Vec<Method> = config.methods.iter().filter_map(|m| m.parse().ok()).collect();

    let headers: Vec<HeaderName> = config.allow_headers.iter().filter_map(|h| h.parse().ok()).collect();

    let mut layer = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods(AllowMethods::list(methods))
        .max_age(std::time::Duration::from_secs(config.max_age));

    if headers.is_empty() {
        layer = layer.allow_headers(AllowHeaders::any());
    } else {
        layer = layer.allow_headers(AllowHeaders::list(headers));
    }

    if config.allow_credentials {
        layer = layer.allow_credentials(true);
    }

    layer
}
