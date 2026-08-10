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

/// OAuth authorization bound to a request after its Bearer token is validated.
///
/// The principal is the registered OAuth client's `client_id`; it is not the
/// account owner or the access token itself.
#[derive(Clone, Debug)]
pub struct OAuthPrincipal {
    pub principal: String,
    pub scopes: Vec<String>,
}

/// Marks HTTP request parts that passed through the OAuth middleware.
///
/// This lets the tool dispatcher distinguish an OAuth request with a missing
/// authorization carrier (which must fail closed) from stdio and the other
/// HTTP authentication modes, which do not carry OAuth scopes.
#[derive(Clone, Debug)]
pub(crate) struct OAuthAuthenticated;

/// Axum middleware that validates an OAuth 2.1 Bearer token from the
/// `Authorization` header.
pub async fn oauth_bearer_auth(
    State(state): State<OAuthMiddlewareState>,
    mut req: Request<Body>,
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
        req.extensions_mut().insert(OAuthAuthenticated);
        req.extensions_mut().insert(OAuthPrincipal {
            principal: access_token.client_id,
            scopes: access_token.scope,
        });
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

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::Body,
        extract::Extension,
        http::{Request, StatusCode},
        middleware,
        routing::get,
    };
    use tower::ServiceExt;

    use super::{OAuthMiddlewareState, OAuthPrincipal, oauth_bearer_auth};
    use crate::oauth::store::{OAuthStore, OAuthStoreConfig};

    async fn principal_handler(Extension(authorization): Extension<OAuthPrincipal>) -> String {
        format!(
            "{}:{}",
            authorization.principal,
            authorization.scopes.join(",")
        )
    }

    #[tokio::test]
    async fn validated_token_attaches_principal_and_scopes() {
        let store = OAuthStore::new(OAuthStoreConfig::default());
        let access_token = store
            .create_access_token(
                "rhood_cid_test_client".to_string(),
                vec!["read".to_string(), "write".to_string()],
            )
            .await;
        let state = OAuthMiddlewareState {
            store,
            resource_metadata_url: "http://127.0.0.1/oauth-resource".to_string(),
        };
        let app = Router::new()
            .route("/protected", get(principal_handler))
            .layer(middleware::from_fn_with_state(state, oauth_bearer_auth));

        let response = app
            .oneshot(
                Request::get("/protected")
                    .header("authorization", format!("Bearer {}", access_token.token))
                    .body(Body::empty())
                    .expect("request builder uses a valid static URI"),
            )
            .await
            .expect("router is infallible");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body is readable");
        assert_eq!(body.as_ref(), b"rhood_cid_test_client:read,write");
    }
}
