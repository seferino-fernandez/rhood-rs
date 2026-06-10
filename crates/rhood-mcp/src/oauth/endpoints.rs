use axum::{
    Form, Json,
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};
use std::fmt::Write as _;
use std::sync::Arc;

use subtle::ConstantTimeEq;

use crate::oauth::model::{
    ApprovalForm, AuthorizationServerMetadata, AuthorizeQuery, OAuthEndpointState,
    ProtectedResourceMetadata, RegistrationRequest, TokenRequest,
};

use super::pkce;

const CONSENT_HTML: &str = include_str!("consent.html");

/// `GET /.well-known/oauth-protected-resource` — returns resource metadata
/// including supported scopes and authorization server references.
pub async fn protected_resource_metadata(
    State(state): State<Arc<OAuthEndpointState>>,
) -> impl IntoResponse {
    tracing::debug!("GET /.well-known/oauth-protected-resource");
    let mut scopes = vec!["read".to_string()];
    if !state.server_read_only {
        scopes.push("write".to_string());
    }
    Json(ProtectedResourceMetadata {
        resource: state.base_url.clone(),
        authorization_servers: vec![state.base_url.clone()],
        scopes_supported: scopes,
        bearer_methods_supported: vec!["header".to_string()],
    })
}

/// `GET /.well-known/oauth-authorization-server` — returns authorization server
/// metadata including endpoints, supported grant types, and PKCE methods.
pub async fn authorization_server_metadata(
    State(state): State<Arc<OAuthEndpointState>>,
) -> impl IntoResponse {
    tracing::debug!("GET /.well-known/oauth-authorization-server");
    let mut scopes = vec!["read".to_string()];
    if !state.server_read_only {
        scopes.push("write".to_string());
    }
    Json(AuthorizationServerMetadata {
        issuer: state.base_url.clone(),
        authorization_endpoint: format!("{}/oauth/authorize", state.base_url),
        token_endpoint: format!("{}/oauth/token", state.base_url),
        registration_endpoint: format!("{}/oauth/register", state.base_url),
        scopes_supported: scopes,
        response_types_supported: vec!["code".to_string()],
        grant_types_supported: vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ],
        code_challenge_methods_supported: vec!["S256".to_string()],
        token_endpoint_auth_methods_supported: vec!["none".to_string()],
    })
}

/// `GET /oauth/authorize` — validates the authorization request and renders
/// the consent form HTML. Requires PKCE `code_challenge` with method `S256`.
pub async fn authorize(
    State(state): State<Arc<OAuthEndpointState>>,
    Query(params): Query<AuthorizeQuery>,
) -> Response {
    tracing::debug!(
        client_id = %params.client_id,
        redirect_uri = %params.redirect_uri,
        response_type = %params.response_type,
        scope = ?params.scope,
        "GET /oauth/authorize"
    );
    if params.response_type != "code" {
        return error_redirect_or_json(
            &params.redirect_uri,
            params.state.as_deref(),
            "unsupported_response_type",
            "Only response_type=code is supported",
        );
    }

    match params.code_challenge_method.as_deref() {
        Some("S256") => {}
        Some(other) => {
            return error_redirect_or_json(
                &params.redirect_uri,
                params.state.as_deref(),
                "invalid_request",
                &format!("Unsupported code_challenge_method: {other}. Only S256 is supported."),
            );
        }
        None => {
            return error_redirect_or_json(
                &params.redirect_uri,
                params.state.as_deref(),
                "invalid_request",
                "code_challenge and code_challenge_method=S256 are required",
            );
        }
    }

    if params.code_challenge.is_none() {
        return error_redirect_or_json(
            &params.redirect_uri,
            params.state.as_deref(),
            "invalid_request",
            "code_challenge is required",
        );
    }

    if !state
        .store
        .validate_redirect_uri(&params.client_id, &params.redirect_uri)
        .await
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_request",
                "error_description": "Unknown client_id or redirect_uri mismatch"
            })),
        )
            .into_response();
    }

    let csrf_nonce = state.store.create_csrf_nonce().await;
    let pin_required = state.oauth_pin.is_some();
    let html = CONSENT_HTML.replace(
        "<body>",
        &format!("<body data-pin-required=\"{pin_required}\" data-csrf-nonce=\"{csrf_nonce}\">"),
    );
    (
        [
            (axum::http::header::X_FRAME_OPTIONS, "DENY"),
            (
                axum::http::header::CONTENT_SECURITY_POLICY,
                "frame-ancestors 'none'",
            ),
        ],
        Html(html),
    )
        .into_response()
}

/// `POST /oauth/approve` — processes the consent form submission. On approval,
/// issues an authorization code and redirects to the client's `redirect_uri`.
pub async fn approve(
    State(state): State<Arc<OAuthEndpointState>>,
    Form(form): Form<ApprovalForm>,
) -> Response {
    tracing::debug!(
        client_id = %form.client_id,
        approved = %form.approved,
        scope = %form.scope,
        "POST /oauth/approve"
    );
    let state_param = form.state.as_deref().unwrap_or("");

    if !state.store.consume_csrf_nonce(&form.csrf_nonce).await {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "invalid_request",
                "error_description": "Invalid or expired CSRF nonce"
            })),
        )
            .into_response();
    }

    if !state
        .store
        .validate_redirect_uri(&form.client_id, &form.redirect_uri)
        .await
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_request",
                "error_description": "Unknown client_id or redirect_uri mismatch"
            })),
        )
            .into_response();
    }

    if form.approved != "true" {
        return redirect_with_error(
            &form.redirect_uri,
            state_param,
            "access_denied",
            "User denied the authorization request",
        );
    }

    if let Some(ref expected_pin) = state.oauth_pin {
        let provided_pin = form.pin.as_deref().unwrap_or("");
        let expected_bytes = expected_pin.as_bytes();
        let provided_bytes = provided_pin.as_bytes();
        if expected_bytes.len() != provided_bytes.len()
            || expected_bytes.ct_eq(provided_bytes).unwrap_u8() != 1
        {
            return redirect_with_error(
                &form.redirect_uri,
                state_param,
                "access_denied",
                "Invalid PIN",
            );
        }
    }

    let has_write_request = form.scope.split_whitespace().any(|scope| scope == "write");

    if has_write_request && state.server_read_only {
        return redirect_with_error(
            &form.redirect_uri,
            state_param,
            "access_denied",
            "Write scope is not available — server is in read-only mode",
        );
    }

    let requested_scopes: Vec<String> = form
        .scope
        .split_whitespace()
        .filter(|scope| *scope == "read" || *scope == "write")
        .map(std::string::ToString::to_string)
        .collect();
    let scope = if requested_scopes.is_empty() {
        vec!["read".to_string()]
    } else {
        requested_scopes
    };

    let code = state
        .store
        .create_auth_code(
            form.client_id,
            form.redirect_uri.clone(),
            scope.clone(),
            form.code_challenge,
        )
        .await;
    tracing::info!(scope = ?scope, "Authorization code issued");

    let mut redirect_url = format!("{}?code={}", form.redirect_uri, code);
    if !state_param.is_empty() {
        let _ = write!(redirect_url, "&state={}", urlencoding::encode(state_param));
    }
    Redirect::to(&redirect_url).into_response()
}

/// `POST /oauth/token` — handles token exchange (`authorization_code`) and
/// token refresh (`refresh_token`) grant types.
pub async fn token(
    State(state): State<Arc<OAuthEndpointState>>,
    Form(req): Form<TokenRequest>,
) -> Response {
    tracing::debug!(
        grant_type = %req.grant_type,
        client_id = %req.client_id,
        "POST /oauth/token"
    );
    match req.grant_type.as_str() {
        "authorization_code" => handle_code_exchange(state, req).await,
        "refresh_token" => handle_refresh(state, req).await,
        _ => token_error(
            "unsupported_grant_type",
            "Supported: authorization_code, refresh_token",
        ),
    }
}

async fn handle_code_exchange(state: Arc<OAuthEndpointState>, req: TokenRequest) -> Response {
    tracing::debug!(client_id = %req.client_id, "Exchanging authorization code for tokens");

    let code_verifier = match req.code_verifier {
        Some(ref verifier) if !verifier.is_empty() => verifier.as_str(),
        _ => {
            tracing::warn!("Token exchange failed: missing code_verifier");
            return token_error("invalid_request", "code_verifier is required");
        }
    };

    let Some(auth_code) = state.store.consume_auth_code(&req.code).await else {
        tracing::warn!("Token exchange failed: invalid or expired authorization code");
        return token_error("invalid_grant", "Invalid or expired authorization code");
    };

    if !pkce::verify_s256(code_verifier, &auth_code.code_challenge) {
        tracing::warn!(client_id = %req.client_id, "Token exchange failed: PKCE verification failed");
        return token_error("invalid_grant", "PKCE verification failed");
    }

    if auth_code.client_id != req.client_id || auth_code.redirect_uri != req.redirect_uri {
        tracing::warn!(
            expected_client = %auth_code.client_id,
            actual_client = %req.client_id,
            "Token exchange failed: client_id or redirect_uri mismatch"
        );
        return token_error("invalid_grant", "client_id or redirect_uri mismatch");
    }

    let access_token = state
        .store
        .create_access_token(auth_code.client_id, auth_code.scope.clone())
        .await;
    tracing::info!(scope = ?auth_code.scope, "Access token issued");

    token_response(
        &access_token.token,
        &access_token.refresh_token,
        access_token.expires_in,
        &auth_code.scope,
    )
}

async fn handle_refresh(state: Arc<OAuthEndpointState>, req: TokenRequest) -> Response {
    tracing::debug!("Refreshing access token");
    if req.refresh_token.is_empty() {
        tracing::warn!("Refresh failed: missing refresh_token");
        return token_error("invalid_request", "refresh_token is required");
    }

    if let Some(new_access_token) = state.store.refresh_access_token(&req.refresh_token).await {
        tracing::info!("Access token refreshed");
        token_response(
            &new_access_token.token,
            &new_access_token.refresh_token,
            new_access_token.expires_in,
            &new_access_token.scope,
        )
    } else {
        tracing::warn!("Refresh failed: invalid refresh token");
        token_error("invalid_grant", "Invalid refresh token")
    }
}

fn token_response(
    access_token: &str,
    refresh_token: &str,
    expires_in: u64,
    scope: &[String],
) -> Response {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "access_token": access_token,
            "token_type": "Bearer",
            "expires_in": expires_in,
            "refresh_token": refresh_token,
            "scope": scope.join(" ")
        })),
    )
        .into_response()
}

fn token_error(error: &str, description: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "error": error,
            "error_description": description
        })),
    )
        .into_response()
}

/// `POST /oauth/register` — dynamic client registration endpoint.
/// Returns a new `client_id` and `client_secret`.
pub async fn register(
    State(state): State<Arc<OAuthEndpointState>>,
    Json(req): Json<RegistrationRequest>,
) -> Response {
    tracing::debug!(
        client_name = ?req.client_name,
        redirect_uris = ?req.redirect_uris,
        "POST /oauth/register"
    );
    if req.redirect_uris.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_request",
                "error_description": "At least one redirect_uri is required"
            })),
        )
            .into_response();
    }

    let client = state
        .store
        .register_client(req.client_name, req.redirect_uris.clone())
        .await;
    tracing::info!(client_id = %client.client_id, "Dynamic client registered");

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "client_id": client.client_id,
            "client_secret": client.client_secret,
            "client_name": client.client_name,
            "redirect_uris": req.redirect_uris
        })),
    )
        .into_response()
}

fn error_redirect_or_json(
    redirect_uri: &str,
    state: Option<&str>,
    error: &str,
    description: &str,
) -> Response {
    redirect_with_error(redirect_uri, state.unwrap_or(""), error, description)
}

fn redirect_with_error(
    redirect_uri: &str,
    state: &str,
    error: &str,
    description: &str,
) -> Response {
    let mut url = format!(
        "{}?error={}&error_description={}",
        redirect_uri,
        error,
        urlencoding::encode(description)
    );
    if !state.is_empty() {
        let _ = write!(url, "&state={}", urlencoding::encode(state));
    }
    Redirect::to(&url).into_response()
}
