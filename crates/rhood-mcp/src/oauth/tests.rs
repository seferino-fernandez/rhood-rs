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

const TEST_CLIENT_ID: &str = "configured-test-client";
const TEST_REDIRECT_URI: &str = "http://localhost/cb";

fn registered_clients() -> Vec<model::RegisteredClient> {
    vec![model::RegisteredClient {
        client_id: TEST_CLIENT_ID.to_string(),
        client_name: "Configured Test Client".to_string(),
        redirect_uris: vec![TEST_REDIRECT_URI.to_string()],
    }]
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
    build_test_router_with_clients(server_read_only, oauth_pin, registered_clients())
}

fn build_test_router_with_clients(
    server_read_only: bool,
    oauth_pin: Option<String>,
    registered_clients: Vec<model::RegisteredClient>,
) -> (Router, store::OAuthStore) {
    let store_instance = store::OAuthStore::with_registered_clients(
        store::OAuthStoreConfig::default(),
        registered_clients,
    );
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

async fn assert_authorization_error_is_local_for_untrusted_targets(
    validation_fields: &[(&str, &str)],
) {
    let (app, _store) = build_test_router(false, None);

    for (client_id, redirect_uri) in [
        ("unknown-client", TEST_REDIRECT_URI),
        (TEST_CLIENT_ID, "http://localhost/wrong-callback"),
    ] {
        let mut query_fields = vec![
            ("client_id", client_id),
            ("redirect_uri", redirect_uri),
            ("state", "attacker-state"),
        ];
        query_fields.extend_from_slice(validation_fields);
        let query = serde_urlencoded::to_string(query_fields).unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/oauth/authorize?{query}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(response.headers().get(header::LOCATION).is_none());
        let json = body_to_json(response.into_body()).await;
        assert_eq!(json["error"], "invalid_request");
        assert_eq!(
            json["error_description"],
            "Unknown client_id or redirect_uri mismatch"
        );
    }
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
    assert!(json.get("registration_endpoint").is_none());
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
async fn dynamic_client_registration_is_not_routed() {
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
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn authorization_rejects_unknown_and_redirect_mismatched_clients() {
    let (app, _store) = build_test_router(false, None);
    let challenge = pkce::challenge_from_verifier("test-verifier");

    for (client_id, redirect_uri) in [
        ("unknown-client", TEST_REDIRECT_URI),
        (TEST_CLIENT_ID, "http://localhost/wrong-callback"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(format!(
                    "/oauth/authorize?response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256",
                    urlencoding::encode(client_id),
                    urlencoding::encode(redirect_uri),
                    challenge
                ))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = body_to_json(response.into_body()).await;
        assert_eq!(json["error"], "invalid_request");
        assert!(
            json["error_description"]
                .as_str()
                .unwrap()
                .contains("Unknown client_id or redirect_uri mismatch")
        );
    }
}

#[tokio::test]
async fn authorization_invalid_response_type_does_not_redirect_untrusted_target() {
    assert_authorization_error_is_local_for_untrusted_targets(&[
        ("response_type", "token"),
        ("code_challenge", "challenge"),
        ("code_challenge_method", "S256"),
    ])
    .await;
}

#[tokio::test]
async fn authorization_unsupported_code_challenge_method_does_not_redirect_untrusted_target() {
    assert_authorization_error_is_local_for_untrusted_targets(&[
        ("response_type", "code"),
        ("code_challenge", "challenge"),
        ("code_challenge_method", "plain"),
    ])
    .await;
}

#[tokio::test]
async fn authorization_missing_code_challenge_method_does_not_redirect_untrusted_target() {
    assert_authorization_error_is_local_for_untrusted_targets(&[
        ("response_type", "code"),
        ("code_challenge", "challenge"),
    ])
    .await;
}

#[tokio::test]
async fn authorization_missing_code_challenge_does_not_redirect_untrusted_target() {
    assert_authorization_error_is_local_for_untrusted_targets(&[
        ("response_type", "code"),
        ("code_challenge_method", "S256"),
    ])
    .await;
}

#[tokio::test]
async fn authorization_protocol_errors_redirect_trusted_target_and_preserve_state() {
    let (app, _store) = build_test_router(false, None);
    let client_state = "client state /?&=+";

    for (validation_fields, expected_error) in [
        (
            vec![
                ("response_type", "token"),
                ("code_challenge", "challenge"),
                ("code_challenge_method", "S256"),
            ],
            "unsupported_response_type",
        ),
        (
            vec![
                ("response_type", "code"),
                ("code_challenge", "challenge"),
                ("code_challenge_method", "plain"),
            ],
            "invalid_request",
        ),
        (
            vec![("response_type", "code"), ("code_challenge", "challenge")],
            "invalid_request",
        ),
        (
            vec![("response_type", "code"), ("code_challenge_method", "S256")],
            "invalid_request",
        ),
    ] {
        let mut query_fields = vec![
            ("client_id", TEST_CLIENT_ID),
            ("redirect_uri", TEST_REDIRECT_URI),
            ("state", client_state),
        ];
        query_fields.extend_from_slice(&validation_fields);
        let query = serde_urlencoded::to_string(query_fields).unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/oauth/authorize?{query}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        let parsed = url::Url::parse(location).unwrap();
        let mut redirect_target = parsed.clone();
        redirect_target.set_query(None);
        assert_eq!(redirect_target.as_str(), TEST_REDIRECT_URI);

        let query: std::collections::HashMap<_, _> = parsed.query_pairs().collect();
        assert_eq!(&query["error"], expected_error);
        assert_eq!(&query["state"], client_state);
    }
}

#[tokio::test]
async fn full_oauth_flow() {
    let (app, test_store) = build_test_router(false, None);
    let client_state = "client state /?&=+";

    // 1. Generate PKCE verifier and challenge
    let verifier = "test-verifier-string-that-is-long-enough-for-pkce";
    let challenge = pkce::challenge_from_verifier(verifier);

    // 2. Submit approval (simulating consent form POST)
    let csrf_nonce = test_store.create_csrf_nonce().await;
    let form_body = serde_urlencoded::to_string([
        ("client_id", TEST_CLIENT_ID),
        ("redirect_uri", TEST_REDIRECT_URI),
        ("scope", "read write"),
        ("state", client_state),
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

    // Extract code from redirect URL
    let redirect_url = url::Url::parse(location).unwrap();
    let echoed_state = redirect_url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1;
    assert_eq!(echoed_state, client_state);
    let auth_code = redirect_url
        .query_pairs()
        .find(|(key, _)| key == "code")
        .unwrap()
        .1
        .to_string();

    // 3. Exchange code for token
    let token_body = serde_urlencoded::to_string([
        ("grant_type", "authorization_code"),
        ("code", auth_code.as_str()),
        ("client_id", TEST_CLIENT_ID),
        ("redirect_uri", TEST_REDIRECT_URI),
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

    // 4. Access protected route with token
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

    // 5. Invalid token gets 401
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

    // 6. Refresh token
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

    // 7. Old token is revoked
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

    // 8. New token works
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

    let verifier = "correct-verifier";
    let challenge = pkce::challenge_from_verifier(verifier);

    // Approve
    let csrf_nonce = test_store.create_csrf_nonce().await;
    let form_body = serde_urlencoded::to_string([
        ("client_id", TEST_CLIENT_ID),
        ("redirect_uri", TEST_REDIRECT_URI),
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
        ("client_id", TEST_CLIENT_ID),
        ("redirect_uri", TEST_REDIRECT_URI),
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

    let challenge = pkce::challenge_from_verifier("test-verifier");

    let response = app
        .oneshot(
            Request::get(format!(
                "/oauth/authorize?response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256",
                TEST_CLIENT_ID,
                urlencoding::encode(TEST_REDIRECT_URI),
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
async fn consent_page_uses_the_trusted_client_and_validated_request_document() {
    let (app, _store) = build_test_router(false, None);
    let query = serde_urlencoded::to_string([
        ("response_type", "code"),
        ("client_id", TEST_CLIENT_ID),
        ("redirect_uri", TEST_REDIRECT_URI),
        ("scope", "read write"),
        ("state", "client state"),
        ("code_challenge", "validated-challenge"),
        ("code_challenge_method", "S256"),
    ])
    .unwrap();

    let response = app
        .oneshot(
            Request::get(format!("/oauth/authorize?{query}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_to_string(response.into_body()).await;
    for server_value in [
        r#"data-client-name="Configured%20Test%20Client""#,
        r#"data-client-id="configured-test-client""#,
        r#"data-redirect-uri="http%3A%2F%2Flocalhost%2Fcb""#,
        r#"data-scope="read%20write""#,
        r#"data-state="client%20state""#,
        r#"data-code-challenge="validated-challenge""#,
        r#"data-code-challenge-method="S256""#,
        r#"data-csrf-nonce="rhood_csrf_"#,
    ] {
        assert!(body.contains(server_value), "missing {server_value}");
    }
    assert!(body.contains("Redirect destination:"));
    assert!(body.contains(r#"id="redirect-destination""#));
    assert!(!body.contains("URLSearchParams"));
    assert!(!body.contains("window.location.search"));
}

#[tokio::test]
async fn consent_page_does_not_emit_untrusted_markup_raw() {
    const HOSTILE_CLIENT_NAME: &str = r#"Trusted Client"><script>alert('name')</script>"#;
    const HOSTILE_STATE: &str = r#""><img src=x onerror=alert('state')>"#;
    let clients = vec![model::RegisteredClient {
        client_id: TEST_CLIENT_ID.to_string(),
        client_name: HOSTILE_CLIENT_NAME.to_string(),
        redirect_uris: vec![TEST_REDIRECT_URI.to_string()],
    }];
    let (app, _store) = build_test_router_with_clients(false, None, clients);
    let query = serde_urlencoded::to_string([
        ("response_type", "code"),
        ("client_id", TEST_CLIENT_ID),
        ("redirect_uri", TEST_REDIRECT_URI),
        ("state", HOSTILE_STATE),
        ("code_challenge", "validated-challenge"),
        ("code_challenge_method", "S256"),
    ])
    .unwrap();

    let response = app
        .oneshot(
            Request::get(format!("/oauth/authorize?{query}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_to_string(response.into_body()).await;
    assert!(!body.contains(HOSTILE_CLIENT_NAME));
    assert!(!body.contains(HOSTILE_STATE));
    assert!(body.contains(
        r#"data-client-name="Trusted%20Client%22%3E%3Cscript%3Ealert%28%27name%27%29%3C%2Fscript%3E""#
    ));
    assert!(
        body.contains(
            r#"data-state="%22%3E%3Cimg%20src%3Dx%20onerror%3Dalert%28%27state%27%29%3E""#
        )
    );
    assert!(body.contains(r#"data-scope="read""#));
}

#[tokio::test]
async fn pin_required_rejects_wrong_pin() {
    let (app, test_store) = build_test_router(false, Some("secret-pin".to_string()));

    let challenge = pkce::challenge_from_verifier("verifier");

    // Approve with wrong pin
    let csrf_nonce = test_store.create_csrf_nonce().await;
    let form_body = serde_urlencoded::to_string([
        ("client_id", TEST_CLIENT_ID),
        ("redirect_uri", TEST_REDIRECT_URI),
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
