use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::oauth::store::OAuthStore;

/// State shared with the OAuth bearer-token authentication middleware layer.
#[derive(Clone)]
pub struct OAuthMiddlewareState {
    pub store: OAuthStore,
    pub resource_metadata_url: String,
}

/// Axum middleware that validates an OAuth 2.1 Bearer token from the
/// `Authorization` header.
pub async fn oauth_bearer_auth(
    State(state): State<OAuthMiddlewareState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    tracing::debug!(%method, %uri, "oauth_bearer_auth: incoming request");

    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|header_value| header_value.to_str().ok())
        .and_then(|auth_header| auth_header.strip_prefix("Bearer "));

    let token = match token {
        Some(value) if !value.is_empty() => value,
        _ => {
            tracing::debug!(%method, %uri, "oauth_bearer_auth: no Bearer token present");
            return unauthorized_response(&state.resource_metadata_url, None);
        }
    };

    if let Some(access_token) = state.store.validate_access_token(token).await {
        tracing::debug!(
            %method, %uri,
            scope = ?access_token.scope,
            "oauth_bearer_auth: token validated"
        );
        next.run(req).await
    } else {
        tracing::warn!(%method, %uri, "oauth_bearer_auth: invalid or expired token");
        unauthorized_response(&state.resource_metadata_url, Some("invalid_token"))
    }
}

fn unauthorized_response(resource_metadata_url: &str, error: Option<&str>) -> Response {
    let www_auth = match error {
        Some(err) => {
            format!("Bearer resource_metadata=\"{resource_metadata_url}\", error=\"{err}\"")
        }
        None => format!("Bearer resource_metadata=\"{resource_metadata_url}\""),
    };

    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, www_auth)],
    )
        .into_response()
}
