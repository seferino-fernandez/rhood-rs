use tower_http::cors::{Any, CorsLayer};

use crate::config::OAuthCorsConfig;

/// Builds the CORS layer for OAuth routes from [`OAuthCorsConfig`].
///
/// If origins contains `"*"`, allows all origins. Otherwise, parses each
/// origin string as an allowed `HeaderValue`.
pub fn oauth_cors_layer(cors_config: OAuthCorsConfig) -> CorsLayer {
    let methods = [
        axum::http::Method::GET,
        axum::http::Method::POST,
        axum::http::Method::OPTIONS,
    ];

    if cors_config.origins.iter().any(|origin| origin == "*") {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(methods)
            .allow_headers(Any)
    } else {
        let origins: Vec<axum::http::HeaderValue> = cors_config
            .origins
            .iter()
            .filter_map(|origin| origin.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods(methods)
            .allow_headers(Any)
    }
}
