use crate::middleware::oauth::{OAuthMiddlewareState, oauth_bearer_auth};
use crate::oauth::model::OAuthEndpointState;

use super::*;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
    middleware as axum_middleware,
    routing::get,
};
use tower::ServiceExt;

fn test_base_url() -> String {
    "http://127.0.0.1:9999".to_string()
}

async fn protected_handler() -> &'static str {
    "OK"
}

/// Build a minimal router with OAuth endpoints + a protected /test route.
/// Returns (Router, OAuthStore) so tests can create CSRF nonces directly.
fn build_test_router(
    server_read_only: bool,
    oauth_pin: Option<String>,
) -> (Router, store::OAuthStore) {
    let store_instance = store::OAuthStore::new(store::OAuthStoreConfig::default());
    let base = test_base_url();

    let endpoint_state = std::sync::Arc::new(OAuthEndpointState {
        store: store_instance.clone(),
        base_url: base.clone(),
        server_read_only,
        oauth_pin,
    });

    let middleware_state = OAuthMiddlewareState {
        store: store_instance.clone(),
        resource_metadata_url: format!("{base}/.well-known/oauth-protected-resource"),
    };

    let oauth_routes = Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            get(endpoints::protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(endpoints::protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(endpoints::authorization_server_metadata),
        )
        .route(
            "/.well-known/openid-configuration",
            get(endpoints::authorization_server_metadata),
        )
        .route("/oauth/authorize", get(endpoints::authorize))
        .route("/oauth/approve", axum::routing::post(endpoints::approve))
        .route("/oauth/token", axum::routing::post(endpoints::token))
        .route("/oauth/register", axum::routing::post(endpoints::register))
        .with_state(endpoint_state);

    let protected_routes = Router::new().route("/test", get(protected_handler)).layer(
        axum_middleware::from_fn_with_state(middleware_state, oauth_bearer_auth),
    );

    (oauth_routes.merge(protected_routes), store_instance)
}

async fn body_to_string(body: Body) -> String {
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn body_to_json(body: Body) -> serde_json::Value {
    let text = body_to_string(body).await;
    serde_json::from_str(&text).unwrap()
}

#[tokio::test]
async fn protected_route_requires_auth() {
    let (app, _store) = build_test_router(false, None);
    let response = app
        .oneshot(Request::get("/test").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let www_auth = response
        .headers()
        .get(header::WWW_AUTHENTICATE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        www_auth.contains("oauth-protected-resource"),
        "WWW-Authenticate: {www_auth}"
    );
}

#[tokio::test]
async fn metadata_endpoints_return_json() {
    let (app, _store) = build_test_router(false, None);

    let response = app
        .clone()
        .oneshot(
            Request::get("/.well-known/oauth-protected-resource")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response.into_body()).await;
    assert!(!json["authorization_servers"].as_array().unwrap().is_empty());
    assert!(
        json["scopes_supported"]
            .as_array()
            .unwrap()
            .iter()
            .any(|scope| scope == "read")
    );
    assert!(
        json["scopes_supported"]
            .as_array()
            .unwrap()
            .iter()
            .any(|scope| scope == "write")
    );

    let response = app
        .oneshot(
            Request::get("/.well-known/oauth-authorization-server")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response.into_body()).await;
    assert!(
        json["authorization_endpoint"]
            .as_str()
            .unwrap()
            .contains("/oauth/authorize")
    );
    assert!(
        json["token_endpoint"]
            .as_str()
            .unwrap()
            .contains("/oauth/token")
    );
    assert_eq!(
        json["code_challenge_methods_supported"],
        serde_json::json!(["S256"])
    );
}

#[tokio::test]
async fn metadata_at_suffixed_path() {
    let (app, _store) = build_test_router(false, None);
    let response = app
        .oneshot(
            Request::get("/.well-known/oauth-protected-resource/mcp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response.into_body()).await;
    assert!(!json["authorization_servers"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn openid_configuration_returns_metadata() {
    let (app, _store) = build_test_router(false, None);
    let response = app
        .oneshot(
            Request::get("/.well-known/openid-configuration")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_to_json(response.into_body()).await;
    assert!(
        json["authorization_endpoint"]
            .as_str()
            .unwrap()
            .contains("/oauth/authorize")
    );
}

#[tokio::test]
async fn read_only_server_hides_write_scope() {
    let (app, _store) = build_test_router(true, None);
    let response = app
        .oneshot(
            Request::get("/.well-known/oauth-protected-resource")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let json = body_to_json(response.into_body()).await;
    let scopes = json["scopes_supported"].as_array().unwrap();
    assert!(scopes.iter().any(|scope| scope == "read"));
    assert!(!scopes.iter().any(|scope| scope == "write"));
}

#[tokio::test]
async fn dynamic_client_registration() {
    let (app, _store) = build_test_router(false, None);
    let response = app
        .oneshot(
            Request::post("/oauth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "client_name": "Test Client",
                        "redirect_uris": ["http://localhost:8080/callback"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_to_json(response.into_body()).await;
    assert!(
        json["client_id"]
            .as_str()
            .unwrap()
            .starts_with("rhood_cid_")
    );
    assert!(json["client_secret"].as_str().is_some());
}

#[tokio::test]
async fn full_oauth_flow() {
    let (app, test_store) = build_test_router(false, None);

    // 1. Register client
    let response = app
        .clone()
        .oneshot(
            Request::post("/oauth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "client_name": "Test",
                        "redirect_uris": ["http://localhost/cb"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let registration = body_to_json(response.into_body()).await;
    let client_id = registration["client_id"].as_str().unwrap().to_string();

    // 2. Generate PKCE verifier and challenge
    let verifier = "test-verifier-string-that-is-long-enough-for-pkce";
    let challenge = pkce::challenge_from_verifier(verifier);

    // 3. Submit approval (simulating consent form POST)
    let csrf_nonce = test_store.create_csrf_nonce().await;
    let form_body = serde_urlencoded::to_string([
        ("client_id", client_id.as_str()),
        ("redirect_uri", "http://localhost/cb"),
        ("scope", "read write"),
        ("state", "test-state"),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("approved", "true"),
        ("pin", ""),
        ("csrf_nonce", &csrf_nonce),
    ])
    .unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::post("/oauth/approve")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(form_body))
                .unwrap(),
        )
        .await
        .unwrap();

    // Should redirect with code
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        location.starts_with("http://localhost/cb?code=rhood_ac_"),
        "location: {location}"
    );
    assert!(
        location.contains("state=test-state"),
        "location: {location}"
    );

    // Extract code from redirect URL
    let redirect_url = url::Url::parse(location).unwrap();
    let auth_code = redirect_url
        .query_pairs()
        .find(|(key, _)| key == "code")
        .unwrap()
        .1
        .to_string();

    // 4. Exchange code for token
    let token_body = serde_urlencoded::to_string([
        ("grant_type", "authorization_code"),
        ("code", auth_code.as_str()),
        ("client_id", client_id.as_str()),
        ("redirect_uri", "http://localhost/cb"),
        ("code_verifier", verifier),
    ])
    .unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::post("/oauth/token")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(token_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let token_json = body_to_json(response.into_body()).await;
    let access_token = token_json["access_token"].as_str().unwrap().to_string();
    let refresh_token = token_json["refresh_token"].as_str().unwrap().to_string();
    assert!(access_token.starts_with("rhood_at_"));
    assert!(refresh_token.starts_with("rhood_rt_"));
    assert_eq!(token_json["token_type"], "Bearer");
    assert_eq!(token_json["scope"], "read write");

    // 5. Access protected route with token
    let response = app
        .clone()
        .oneshot(
            Request::get("/test")
                .header("authorization", format!("Bearer {access_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_to_string(response.into_body()).await, "OK");

    // 6. Invalid token gets 401
    let response = app
        .clone()
        .oneshot(
            Request::get("/test")
                .header("authorization", "Bearer invalid-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // 7. Refresh token
    let refresh_body = serde_urlencoded::to_string([
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token.as_str()),
    ])
    .unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::post("/oauth/token")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(refresh_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let new_token_json = body_to_json(response.into_body()).await;
    let new_access_token = new_token_json["access_token"].as_str().unwrap().to_string();
    assert_ne!(new_access_token, access_token, "new token should differ");

    // 8. Old token is revoked
    let response = app
        .clone()
        .oneshot(
            Request::get("/test")
                .header("authorization", format!("Bearer {access_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // 9. New token works
    let response = app
        .oneshot(
            Request::get("/test")
                .header("authorization", format!("Bearer {new_access_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn pkce_failure_rejects_token_exchange() {
    let (app, test_store) = build_test_router(false, None);

    // Register
    let response = app
        .clone()
        .oneshot(
            Request::post("/oauth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "redirect_uris": ["http://localhost/cb"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let registration = body_to_json(response.into_body()).await;
    let client_id = registration["client_id"].as_str().unwrap().to_string();

    let verifier = "correct-verifier";
    let challenge = pkce::challenge_from_verifier(verifier);

    // Approve
    let csrf_nonce = test_store.create_csrf_nonce().await;
    let form_body = serde_urlencoded::to_string([
        ("client_id", client_id.as_str()),
        ("redirect_uri", "http://localhost/cb"),
        ("scope", "read"),
        ("state", ""),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("approved", "true"),
        ("pin", ""),
        ("csrf_nonce", &csrf_nonce),
    ])
    .unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::post("/oauth/approve")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(form_body))
                .unwrap(),
        )
        .await
        .unwrap();
    let location = response
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap();
    let parsed_url = url::Url::parse(location).unwrap();
    let auth_code = parsed_url
        .query_pairs()
        .find(|(key, _)| key == "code")
        .unwrap()
        .1
        .to_string();

    // Exchange with WRONG verifier
    let token_body = serde_urlencoded::to_string([
        ("grant_type", "authorization_code"),
        ("code", auth_code.as_str()),
        ("client_id", client_id.as_str()),
        ("redirect_uri", "http://localhost/cb"),
        ("code_verifier", "wrong-verifier"),
    ])
    .unwrap();

    let response = app
        .oneshot(
            Request::post("/oauth/token")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(token_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = body_to_json(response.into_body()).await;
    assert_eq!(json["error"], "invalid_grant");
    assert!(json["error_description"].as_str().unwrap().contains("PKCE"));
}

#[tokio::test]
async fn consent_page_has_anti_clickjacking_headers() {
    let (app, _store) = build_test_router(false, None);

    // Register a client first
    let response = app
        .clone()
        .oneshot(
            Request::post("/oauth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "redirect_uris": ["http://localhost/cb"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let registration = body_to_json(response.into_body()).await;
    let client_id = registration["client_id"].as_str().unwrap();

    let challenge = pkce::challenge_from_verifier("test-verifier");

    let response = app
        .oneshot(
            Request::get(format!(
                "/oauth/authorize?response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256",
                client_id,
                urlencoding::encode("http://localhost/cb"),
                challenge
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let x_frame = response
        .headers()
        .get("x-frame-options")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(x_frame, "DENY");

    let csp = response
        .headers()
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(csp.contains("frame-ancestors 'none'"));
}

#[tokio::test]
async fn pin_required_rejects_wrong_pin() {
    let (app, test_store) = build_test_router(false, Some("secret-pin".to_string()));

    // Register
    let response = app
        .clone()
        .oneshot(
            Request::post("/oauth/register")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "redirect_uris": ["http://localhost/cb"]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let registration = body_to_json(response.into_body()).await;
    let client_id = registration["client_id"].as_str().unwrap().to_string();

    let challenge = pkce::challenge_from_verifier("verifier");

    // Approve with wrong pin
    let csrf_nonce = test_store.create_csrf_nonce().await;
    let form_body = serde_urlencoded::to_string([
        ("client_id", client_id.as_str()),
        ("redirect_uri", "http://localhost/cb"),
        ("scope", "read"),
        ("state", "s"),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("approved", "true"),
        ("pin", "wrong-pin"),
        ("csrf_nonce", &csrf_nonce),
    ])
    .unwrap();

    let response = app
        .oneshot(
            Request::post("/oauth/approve")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(form_body))
                .unwrap(),
        )
        .await
        .unwrap();
    let location = response
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        location.contains("error=access_denied"),
        "location: {location}"
    );
    assert!(
        location.contains("Invalid%20PIN") || location.contains("Invalid+PIN"),
        "location: {location}"
    );
}
