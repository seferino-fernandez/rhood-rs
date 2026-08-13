mod runtime;

use crate::config::ServerConfig;
#[cfg(test)]
use crate::middleware::cors::oauth_cors_layer;
use crate::shared::create_authenticated_client;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use runtime::{HttpPlan, HttpRuntime};

pub async fn serve(config: &ServerConfig) -> anyhow::Result<()> {
    let plan = HttpPlan::validate(config)?;
    let addr = plan.addr();
    let auth_mode = plan.auth_name();

    tracing::debug!("Authenticating Robinhood client...");
    let client = create_authenticated_client(config).await?;
    tracing::info!("Robinhood client authenticated successfully");
    let runtime = HttpRuntime::assemble(plan, client);

    tracing::info!(
        "Starting HTTP MCP server on {} (auth_mode={})",
        addr,
        auth_mode
    );
    let listener = match TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(error) => {
            runtime.shutdown().await;
            return Err(error.into());
        }
    };
    let serve_result = axum::serve(listener, runtime.router())
        .with_graceful_shutdown(shutdown_signal(runtime.cancellation_token()))
        .await;
    runtime.shutdown().await;
    serve_result.map_err(Into::into)
}

/// Waits for SIGINT (Ctrl+C) or SIGTERM (Unix), then cancels `cancellation_token`
/// so background tasks (e.g. the OAuth store sweep) exit alongside the HTTP server.
///
/// SIGTERM handling matters for container orchestrators (Docker, Kubernetes) that
/// request graceful shutdown via SIGTERM before escalating to SIGKILL.
async fn shutdown_signal(cancellation_token: CancellationToken) {
    #[expect(
        clippy::expect_used,
        reason = "installing the process SIGINT handler is assumed to succeed; no recovery path is implemented"
    )]
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install SIGINT handler");
    };

    #[cfg(unix)]
    #[expect(
        clippy::expect_used,
        reason = "installing the Unix SIGTERM handler is assumed to succeed; no recovery path is implemented"
    )]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => tracing::info!("Received SIGINT, shutting down HTTP MCP server"),
        () = terminate => tracing::info!("Received SIGTERM, shutting down HTTP MCP server"),
    }

    cancellation_token.cancel();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OAuthCorsConfig;
    use axum::body::Body;
    use tower::ServiceExt;

    /// Build a minimal router with the CORS layer applied and send a preflight request.
    /// Returns the `access-control-allow-origin` header value if present.
    async fn preflight_origin(
        cors_config: &OAuthCorsConfig,
        request_origin: &str,
    ) -> Option<String> {
        let cors_layer = oauth_cors_layer(cors_config.clone());
        let router = axum::Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(cors_layer);

        let request = axum::http::Request::builder()
            .method("OPTIONS")
            .uri("/test")
            .header("origin", request_origin)
            .header("access-control-request-method", "GET")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();
        response
            .headers()
            .get("access-control-allow-origin")
            .map(|header_val| header_val.to_str().unwrap().to_string())
    }

    #[tokio::test]
    async fn cors_allows_configured_origin() {
        let cors_config = OAuthCorsConfig {
            origins: vec!["http://localhost".to_string()],
        };
        let allowed = preflight_origin(&cors_config, "http://localhost").await;
        assert_eq!(allowed.as_deref(), Some("http://localhost"));
    }

    #[tokio::test]
    async fn cors_rejects_unconfigured_origin() {
        let cors_config = OAuthCorsConfig {
            origins: vec!["http://localhost".to_string()],
        };
        let allowed = preflight_origin(&cors_config, "http://evil.com").await;
        assert!(
            allowed.is_none(),
            "unconfigured origin should not receive CORS header, got: {allowed:?}"
        );
    }

    #[tokio::test]
    async fn cors_wildcard_allows_any_origin() {
        let cors_config = OAuthCorsConfig {
            origins: vec!["*".to_string()],
        };
        let allowed = preflight_origin(&cors_config, "http://anything.example.com").await;
        assert_eq!(allowed.as_deref(), Some("*"));
    }

    #[tokio::test]
    async fn cors_multiple_origins_allowed() {
        let cors_config = OAuthCorsConfig {
            origins: vec![
                "http://localhost".to_string(),
                "https://app.example.com".to_string(),
            ],
        };
        let first = preflight_origin(&cors_config, "http://localhost").await;
        assert_eq!(first.as_deref(), Some("http://localhost"));

        let second = preflight_origin(&cors_config, "https://app.example.com").await;
        assert_eq!(second.as_deref(), Some("https://app.example.com"));
    }

    #[tokio::test]
    async fn cors_default_config_allows_localhost() {
        let cors_config = OAuthCorsConfig::default();
        let allowed = preflight_origin(&cors_config, "http://127.0.0.1").await;
        assert_eq!(allowed.as_deref(), Some("http://127.0.0.1"));
    }
}

/// End-to-end tests against a real [`StreamableHttpService`], driven with
/// `tower::ServiceExt::oneshot` rather than a bound socket.
///
/// `RobinhoodClient::with_config` performs no I/O, and neither `initialize`,
/// `tools/list`, nor the read-only rejection path ever reaches the upstream API,
/// so these need no network and no mock server.
#[cfg(test)]
mod wire_tests {
    use super::*;
    use crate::config::McpConfig;
    use crate::oauth::model::RegisteredClient;
    use crate::tools::{FallbackPrincipal, RhoodTools, TOOLS_LIST_PAGE_SIZE, ToolRateLimiter};
    use axum::{Router, body::Body};
    use rhood_core::{RhoodConfig, RobinhoodClient};
    use rmcp::{
        handler::server::tool::{ToolRoute, ToolRouter},
        model::{
            CallToolResponse, CallToolResult, ContentBlock, CreateTaskResult, InputRequiredResult,
            MetaObject, ProtocolVersion, Task, TaskStatus, Tool,
        },
        transport::streamable_http_server::{
            session::local::LocalSessionManager,
            tower::{StreamableHttpServerConfig, StreamableHttpService},
        },
    };
    use secrecy::SecretString;
    use serde_json::{Value, json};
    use std::sync::Arc;
    use std::time::Duration;
    use tower::ServiceExt;
    use url::Url;

    const V_MODERN: &str = "2026-07-28";
    const V_LEGACY: &str = "2025-11-25";
    const TEST_TOOL: &str = "test_tool_result";

    fn config_for(auth_mode: &str, read_only: bool) -> ServerConfig {
        let mut config = ServerConfig::default();
        config.core.read_only = read_only;
        config.mcp.auth_mode = auth_mode.to_string();
        match auth_mode {
            "token" => config.mcp.token = Some(SecretString::from("test-token")),
            "oauth" => {
                config.mcp.base_url = Some("http://localhost".to_string());
                config.mcp.registered_clients = vec![RegisteredClient {
                    client_id: "wire-client".to_string(),
                    client_name: "Wire Client".to_string(),
                    redirect_uris: vec!["http://localhost/callback".to_string()],
                }];
            }
            "none" => {}
            other => panic!("unsupported test auth mode: {other}"),
        }
        config
    }

    fn assembled_runtime(config: &ServerConfig) -> HttpRuntime {
        let client =
            RobinhoodClient::with_config(RhoodConfig::default()).expect("default config builds");
        let plan = HttpPlan::validate(config).expect("test HTTP plan is valid");
        HttpRuntime::assemble(plan, client)
    }

    fn plan_error(config: &ServerConfig) -> String {
        match HttpPlan::validate(config) {
            Ok(_) => panic!("test HTTP plan must be rejected"),
            Err(error) => error.to_string(),
        }
    }

    fn none_runtime(read_only: bool) -> HttpRuntime {
        assembled_runtime(&config_for("none", read_only))
    }

    fn synthetic_result_router(response: CallToolResponse, max_response_bytes: usize) -> Router {
        let client =
            RobinhoodClient::with_config(RhoodConfig::default()).expect("default config builds");
        let mcp_config = McpConfig {
            max_response_bytes,
            ..McpConfig::default()
        };
        let rate_limiter =
            ToolRateLimiter::new(&mcp_config.rate_limit).expect("default rate limits are valid");
        let service = StreamableHttpService::new(
            move || {
                let mut tools = RhoodTools::new_eager(
                    client.clone(),
                    false,
                    &mcp_config,
                    rate_limiter.clone(),
                    FallbackPrincipal::HttpNone,
                );
                let response = response.clone();
                tools.tool_router = ToolRouter::new().with_route(ToolRoute::new_dyn(
                    Tool::new(
                        TEST_TOOL,
                        "Return a predetermined test result",
                        Arc::new(Default::default()),
                    ),
                    move |_context| {
                        let response = response.clone();
                        Box::pin(async move { Ok(response) })
                    },
                ));
                Ok(tools)
            },
            Arc::<LocalSessionManager>::default(),
            StreamableHttpServerConfig::default()
                .with_legacy_session_mode(false)
                .with_stateless_protocol_metadata_required(true)
                .with_allowed_hosts(McpConfig::default().allowed_hosts),
        );
        Router::new().nest_service("/mcp", service)
    }

    /// Sends one JSON-RPC message and returns the status, headers, and every
    /// JSON payload in the response (SSE `data:` frames, or a lone JSON body).
    async fn post(
        router: &Router,
        body: Value,
        session_id: Option<&str>,
        protocol_version: Option<&str>,
    ) -> (axum::http::StatusCode, axum::http::HeaderMap, Vec<Value>) {
        post_with_headers(
            router,
            body,
            session_id,
            protocol_version,
            "localhost",
            None,
        )
        .await
    }

    async fn post_with_headers(
        router: &Router,
        body: Value,
        session_id: Option<&str>,
        protocol_version: Option<&str>,
        host: &str,
        bearer_token: Option<&str>,
    ) -> (axum::http::StatusCode, axum::http::HeaderMap, Vec<Value>) {
        let mut request = axum::http::Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("host", host)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if let Some(id) = session_id {
            request = request.header("mcp-session-id", id);
        }
        // rmcp cross-validates this header against `_meta.protocolVersion`, so a
        // request declaring one must send the other. Omitted on `initialize`,
        // where the client does not yet know the negotiated version.
        if let Some(version) = protocol_version {
            request = request.header("mcp-protocol-version", version);

            // SEP-2243 routing headers, required from 2026-07-28 onward and
            // validated by the server against the body. Derived here the same
            // way a real client derives them, so these tests exercise the
            // validation path rather than side-stepping it.
            if version >= V_MODERN {
                if let Some(method) = body.get("method").and_then(Value::as_str) {
                    request = request.header("mcp-method", method);
                }
                let name = body
                    .get("params")
                    .and_then(|params| params.get("name").or_else(|| params.get("uri")))
                    .and_then(Value::as_str);
                if let Some(name) = name {
                    request = request.header("mcp-name", name);
                }
            }
        }
        if let Some(token) = bearer_token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let request = request
            .body(Body::from(body.to_string()))
            .expect("request builds");

        let response = router.clone().oneshot(request).await.expect("infallible");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(Body::new(response.into_body()), usize::MAX)
            .await
            .expect("body collects");
        let text = String::from_utf8_lossy(&bytes).to_string();

        // Responses are `text/event-stream` unless the body is bare JSON.
        let payloads = if text.contains("data:") {
            text.lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .filter_map(|data| serde_json::from_str(data.trim()).ok())
                .collect()
        } else {
            serde_json::from_str::<Value>(&text).into_iter().collect()
        };
        (status, headers, payloads)
    }

    fn initialize(version: &str) -> Value {
        initialize_with_capabilities(version, json!({}))
    }

    fn initialize_with_capabilities(version: &str, capabilities: Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": version,
                "capabilities": capabilities,
                "clientInfo": { "name": "wire-test", "version": "0" }
            }
        })
    }

    /// Lifecycle metadata every stateless 2026-07-28 request carries in place of
    /// a session (SEP-2575).
    fn modern_meta() -> Value {
        modern_meta_with_capabilities(json!({}))
    }

    fn modern_meta_with_capabilities(capabilities: Value) -> Value {
        json!({
            "io.modelcontextprotocol/protocolVersion": V_MODERN,
            "io.modelcontextprotocol/clientInfo": { "name": "wire-test", "version": "0" },
            "io.modelcontextprotocol/clientCapabilities": capabilities
        })
    }

    fn modern_tools_list() -> Value {
        modern_tools_list_with_cursor(None)
    }

    fn modern_tools_list_with_cursor(cursor: Option<&str>) -> Value {
        let mut params = json!({ "_meta": modern_meta() });
        if let Some(cursor) = cursor {
            params["cursor"] = Value::String(cursor.to_string());
        }
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": params
        })
    }

    fn modern_tools_call(name: &str) -> Value {
        modern_tools_call_with_arguments(name, json!({}))
    }

    fn modern_tools_call_with_arguments(name: &str, arguments: Value) -> Value {
        modern_tools_call_with_capabilities(name, arguments, json!({}))
    }

    fn modern_tools_call_with_capabilities(
        name: &str,
        arguments: Value,
        capabilities: Value,
    ) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments,
                "_meta": modern_meta_with_capabilities(capabilities)
            }
        })
    }

    fn meta_with_marker(marker: &str) -> Option<MetaObject> {
        Some(
            serde_json::from_value(json!({
                "unrelated": marker,
                "io.modelcontextprotocol/serverInfo": { "name": "not-rhood-mcp" }
            }))
            .expect("test metadata deserializes"),
        )
    }

    fn expected_server_info() -> Value {
        json!({
            "name": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
            "description": env!("CARGO_PKG_DESCRIPTION")
        })
    }

    fn result_of(payloads: &[Value]) -> Value {
        payloads
            .iter()
            .find(|payload| payload.get("result").is_some())
            .unwrap_or_else(|| panic!("no result in payloads: {payloads:?}"))["result"]
            .clone()
    }

    fn error_of(payloads: &[Value]) -> Value {
        payloads
            .iter()
            .find(|payload| payload.get("error").is_some())
            .unwrap_or_else(|| panic!("no error in payloads: {payloads:?}"))["error"]
            .clone()
    }

    fn tool_names(result: &Value) -> Vec<String> {
        result["tools"]
            .as_array()
            .expect("tools is an array")
            .iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_string))
            .collect()
    }

    async fn all_tools(router: &Router) -> Vec<Value> {
        let mut tools = Vec::new();
        let mut cursor = None;
        for _ in 0..10 {
            let (status, _, payloads) = post(
                router,
                modern_tools_list_with_cursor(cursor.as_deref()),
                None,
                Some(V_MODERN),
            )
            .await;
            assert!(
                status.is_success(),
                "tools/list page failed: {status} {payloads:?}"
            );
            let result = result_of(&payloads);
            tools.extend(
                result["tools"]
                    .as_array()
                    .expect("tools is an array")
                    .iter()
                    .cloned(),
            );
            cursor = result["nextCursor"].as_str().map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        assert!(cursor.is_none(), "tools/list pagination must terminate");
        tools
    }

    async fn all_tool_names(router: &Router) -> Vec<String> {
        all_tools(router)
            .await
            .into_iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_string))
            .collect()
    }

    async fn annotation_declared_write_tool_names() -> Vec<String> {
        let router = none_runtime(false).router();
        let _ = post(&router, initialize(V_MODERN), None, None).await;
        let names: Vec<_> = all_tools(&router)
            .await
            .into_iter()
            .filter(|tool| tool["annotations"]["readOnlyHint"] == Value::Bool(false))
            .filter_map(|tool| tool["name"].as_str().map(str::to_string))
            .collect();
        assert!(
            !names.is_empty(),
            "the live router must declare at least one write tool"
        );
        names
    }

    async fn request(
        router: &Router,
        request: axum::http::Request<Body>,
    ) -> axum::response::Response {
        router.clone().oneshot(request).await.expect("infallible")
    }

    async fn response_text(response: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body collects");
        String::from_utf8(bytes.to_vec()).expect("response is UTF-8")
    }

    async fn oauth_access_token(router: &Router, scope: &str) -> String {
        const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        const REDIRECT_URI: &str = "http://localhost/callback";

        let query = serde_urlencoded::to_string([
            ("response_type", "code"),
            ("client_id", "wire-client"),
            ("redirect_uri", REDIRECT_URI),
            ("scope", scope),
            ("state", "wire-state"),
            ("code_challenge", CHALLENGE),
            ("code_challenge_method", "S256"),
        ])
        .expect("authorize query serializes");
        let authorize = request(
            router,
            axum::http::Request::get(format!("/oauth/authorize?{query}"))
                .header("host", "localhost")
                .body(Body::empty())
                .expect("authorize request builds"),
        )
        .await;
        assert_eq!(authorize.status(), axum::http::StatusCode::OK);
        let html = response_text(authorize).await;
        let encoded_nonce = html
            .split_once("data-csrf-nonce=\"")
            .and_then(|(_, rest)| rest.split_once('"').map(|(nonce, _)| nonce))
            .expect("consent page carries a CSRF nonce");
        let nonce = urlencoding::decode(encoded_nonce)
            .expect("CSRF nonce decodes")
            .into_owned();

        let approval = serde_urlencoded::to_string([
            ("client_id", "wire-client"),
            ("redirect_uri", REDIRECT_URI),
            ("scope", scope),
            ("state", "wire-state"),
            ("code_challenge", CHALLENGE),
            ("code_challenge_method", "S256"),
            ("approved", "true"),
            ("csrf_nonce", nonce.as_str()),
        ])
        .expect("approval form serializes");
        let approval = request(
            router,
            axum::http::Request::post("/oauth/approve")
                .header("host", "localhost")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(approval))
                .expect("approval request builds"),
        )
        .await;
        assert_eq!(
            approval.status(),
            axum::http::StatusCode::SEE_OTHER,
            "approval must redirect"
        );
        let location = approval
            .headers()
            .get(axum::http::header::LOCATION)
            .expect("approval has a Location header")
            .to_str()
            .expect("Location is text");
        let redirect = Url::parse(location).expect("approval redirect is a URL");
        let code = redirect
            .query_pairs()
            .find_map(|(key, value)| (key == "code").then(|| value.into_owned()))
            .expect("approval redirect carries a code");

        let exchange = serde_urlencoded::to_string([
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("client_id", "wire-client"),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", VERIFIER),
        ])
        .expect("token form serializes");
        let exchange = request(
            router,
            axum::http::Request::post("/oauth/token")
                .header("host", "localhost")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(exchange))
                .expect("token request builds"),
        )
        .await;
        assert_eq!(exchange.status(), axum::http::StatusCode::OK);
        let body: Value =
            serde_json::from_str(&response_text(exchange).await).expect("token response is JSON");
        body["access_token"]
            .as_str()
            .expect("token response carries an access token")
            .to_string()
    }

    #[test]
    fn phase_one_validates_every_http_input() {
        let mut token = config_for("token", false);
        token.mcp.token = None;
        assert!(plan_error(&token).contains("requires a token"));

        let mut oauth = config_for("oauth", false);
        oauth.mcp.registered_clients.clear();
        assert!(plan_error(&oauth).contains("requires at least one"));

        let mut none = config_for("none", false);
        none.mcp.host = "0.0.0.0".to_string();
        assert!(plan_error(&none).contains("only allowed on loopback"));

        let mut rate_limited = config_for("none", false);
        rate_limited.mcp.rate_limit.read.global_burst = 0;
        assert!(plan_error(&rate_limited).contains("global_burst must be greater than zero"));
    }

    #[tokio::test]
    async fn serve_validates_the_plan_before_brokerage_authentication() {
        let mut config = config_for("none", false);
        config.mcp.host = "not-an-ip-address".to_string();
        let error = tokio::time::timeout(Duration::from_secs(1), serve(&config))
            .await
            .expect("plan validation must not wait for brokerage authentication")
            .unwrap_err();
        assert!(error.to_string().contains("invalid socket address"));
    }

    #[tokio::test]
    async fn token_mode_traverses_the_assembled_auth_and_mcp_route() {
        let router = assembled_runtime(&config_for("token", false)).router();
        let (unauthorized, _, _) = post(&router, initialize(V_MODERN), None, None).await;
        assert_eq!(unauthorized, axum::http::StatusCode::UNAUTHORIZED);

        let (status, _, payloads) = post_with_headers(
            &router,
            initialize(V_MODERN),
            None,
            None,
            "localhost",
            Some("test-token"),
        )
        .await;
        assert!(status.is_success(), "initialize failed: {payloads:?}");
        assert_eq!(result_of(&payloads)["protocolVersion"], V_MODERN);
    }

    #[tokio::test]
    async fn none_mode_traverses_the_assembled_mcp_route_without_authentication() {
        let router = none_runtime(false).router();
        let (status, _, payloads) = post(&router, initialize(V_MODERN), None, None).await;
        assert!(status.is_success(), "initialize failed: {payloads:?}");
        assert_eq!(result_of(&payloads)["protocolVersion"], V_MODERN);
    }

    #[tokio::test]
    async fn oauth_mode_exposes_routes_and_enforces_real_bearer_middleware() {
        let runtime = assembled_runtime(&config_for("oauth", false));
        assert_eq!(runtime.background_task_count(), 1);
        let router = runtime.router();

        let metadata = request(
            &router,
            axum::http::Request::get("/.well-known/oauth-protected-resource")
                .header("host", "localhost")
                .body(Body::empty())
                .expect("metadata request builds"),
        )
        .await;
        assert_eq!(metadata.status(), axum::http::StatusCode::OK);
        let metadata: Value = serde_json::from_str(&response_text(metadata).await)
            .expect("resource metadata is JSON");
        assert_eq!(metadata["resource"], "http://localhost");

        let (unauthorized, headers, _) = post(&router, initialize(V_MODERN), None, None).await;
        assert_eq!(unauthorized, axum::http::StatusCode::UNAUTHORIZED);
        assert!(headers.contains_key(axum::http::header::WWW_AUTHENTICATE));

        let token = oauth_access_token(&router, "read").await;
        let (status, _, payloads) = post_with_headers(
            &router,
            initialize(V_MODERN),
            None,
            None,
            "localhost",
            Some(&token),
        )
        .await;
        assert!(status.is_success(), "initialize failed: {payloads:?}");

        tokio::time::timeout(Duration::from_secs(1), runtime.shutdown())
            .await
            .expect("OAuth sweep task must terminate after cancellation");
    }

    #[tokio::test]
    async fn assembled_router_enforces_allowed_hosts() {
        let mut config = config_for("none", false);
        config.mcp.allowed_hosts = vec!["expected.example".to_string()];
        let router = assembled_runtime(&config).router();

        let (forbidden, _, _) = post(&router, initialize(V_MODERN), None, None).await;
        assert_eq!(forbidden, axum::http::StatusCode::FORBIDDEN);

        let (status, _, payloads) = post_with_headers(
            &router,
            initialize(V_MODERN),
            None,
            None,
            "expected.example",
            None,
        )
        .await;
        assert!(status.is_success(), "initialize failed: {payloads:?}");
    }

    /// Per SEP-2567 the 2026-07-28 protocol removed sessions, so a modern client
    /// is served statelessly.
    #[tokio::test]
    async fn modern_client_negotiates_2026_07_28_without_a_session() {
        let router = none_runtime(false).router();
        let (status, headers, payloads) = post(&router, initialize(V_MODERN), None, None).await;

        assert!(
            status.is_success(),
            "initialize failed: {status} {payloads:?}"
        );
        assert_eq!(result_of(&payloads)["protocolVersion"], V_MODERN);
        assert!(
            headers.get("mcp-session-id").is_none(),
            "2026-07-28 requests must be stateless, got {headers:?}"
        );
    }

    /// The server serves only 2026-07-28, so an older client receives the protocol
    /// negotiation error instead of falling back to a legacy session.
    #[tokio::test]
    async fn legacy_client_is_rejected_with_unsupported_protocol_version() {
        let router = none_runtime(false).router();
        let (status, _, payloads) = post(&router, initialize(V_LEGACY), None, None).await;

        assert!(
            status.is_success(),
            "initialize must return a JSON-RPC error: {status} {payloads:?}"
        );
        assert_eq!(error_of(&payloads)["code"], -32022);
    }

    /// A modern peer gets the SEP-2322 `resultType` discriminator and the
    /// SEP-2549 cache hints set in `ServerHandler::list_tools`.
    #[tokio::test]
    async fn modern_tools_list_carries_result_type_and_cache_hints() {
        let router = none_runtime(false).router();
        let _ = post(&router, initialize(V_MODERN), None, None).await;
        let (status, _, payloads) = post(&router, modern_tools_list(), None, Some(V_MODERN)).await;

        assert!(
            status.is_success(),
            "tools/list failed: {status} {payloads:?}"
        );
        let result = result_of(&payloads);
        assert_eq!(result["resultType"], "complete");
        assert_eq!(result["ttlMs"], 300_000);
        assert_eq!(result["cacheScope"], "private");
        assert_eq!(tool_names(&result).len(), TOOLS_LIST_PAGE_SIZE);
        assert!(result["nextCursor"].as_str().is_some());
    }

    #[tokio::test]
    async fn malformed_tools_list_cursor_is_an_invalid_params_error() {
        let router = none_runtime(false).router();
        let _ = post(&router, initialize(V_MODERN), None, None).await;
        let (status, _, payloads) = post(
            &router,
            modern_tools_list_with_cursor(Some("not-a-cursor")),
            None,
            Some(V_MODERN),
        )
        .await;

        assert_eq!(
            status,
            axum::http::StatusCode::BAD_REQUEST,
            "invalid params must reject the HTTP request: {payloads:?}"
        );
        let error = error_of(&payloads);
        assert_eq!(error["code"], -32602);
        assert!(
            error["message"]
                .as_str()
                .is_some_and(|message| message.contains("pass next_cursor"))
        );
    }

    /// Strict stateless mode requires the per-request protocol version in
    /// `_meta`; a matching HTTP header alone is insufficient.
    #[tokio::test]
    async fn tools_list_missing_protocol_metadata_is_rejected() {
        let router = none_runtime(false).router();
        let body = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {
                "_meta": {
                    "io.modelcontextprotocol/clientInfo": {
                        "name": "wire-test",
                        "version": "0"
                    },
                    "io.modelcontextprotocol/clientCapabilities": {}
                }
            }
        });

        let (status, _, payloads) = post(&router, body, None, Some(V_LEGACY)).await;
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(error_of(&payloads)["code"], -32602);
    }

    /// First of the three read-only enforcement points: `list_tools` must not
    /// advertise anything that places, cancels, or modifies an order.
    #[tokio::test]
    async fn read_only_server_advertises_no_write_tools() {
        let write_tools = annotation_declared_write_tool_names().await;
        let router = none_runtime(true).router();
        let _ = post(&router, initialize(V_MODERN), None, None).await;
        let names = all_tool_names(&router).await;
        assert!(
            !names.is_empty(),
            "read-only mode must still advertise read tools"
        );
        for write_tool in write_tools {
            assert!(
                !names.iter().any(|name| name == &write_tool),
                "read-only mode must not advertise `{write_tool}`, got {names:?}"
            );
        }
    }

    /// The complement: with the gate off, every write tool is reachable. Guards
    /// against a filter that over-matches and hides tools unconditionally.
    #[tokio::test]
    async fn read_write_server_advertises_every_write_tool() {
        let write_tools = annotation_declared_write_tool_names().await;
        let router = none_runtime(false).router();
        let _ = post(&router, initialize(V_MODERN), None, None).await;
        let names = all_tool_names(&router).await;
        for write_tool in write_tools {
            assert!(
                names.iter().any(|name| name == &write_tool),
                "read-write mode must advertise `{write_tool}`, got {names:?}"
            );
        }
    }

    /// Second enforcement point, and the one that actually protects the account:
    /// hiding a tool from `tools/list` means nothing if a client can still call
    /// it by name.
    #[tokio::test]
    async fn read_only_server_rejects_a_write_tool_call() {
        let write_tools = annotation_declared_write_tool_names().await;
        let router = none_runtime(true).router();
        let _ = post(&router, initialize(V_MODERN), None, None).await;
        for write_tool in write_tools {
            let (_, _, payloads) = post(
                &router,
                modern_tools_call(&write_tool),
                None,
                Some(V_MODERN),
            )
            .await;

            let error = error_of(&payloads);
            assert_eq!(
                error["code"], -32602,
                "expected INVALID_PARAMS for {write_tool}, got {error}"
            );
            assert!(
                error.get("_meta").is_none(),
                "JSON-RPC errors are not tool results: {error}"
            );
            let message = error["message"].as_str().unwrap_or_default();
            assert!(
                message.contains("read-only"),
                "rejection must say why: {message}"
            );
        }
    }

    #[tokio::test]
    async fn oauth_write_scope_reaches_http_dispatch() {
        let runtime = assembled_runtime(&config_for("oauth", false));
        let router = runtime.router();
        let token = oauth_access_token(&router, "read write").await;
        let _ = post_with_headers(
            &router,
            initialize(V_MODERN),
            None,
            None,
            "localhost",
            Some(&token),
        )
        .await;

        let (_, _, payloads) = post_with_headers(
            &router,
            modern_tools_call("place_stock_order"),
            None,
            Some(V_MODERN),
            "localhost",
            Some(&token),
        )
        .await;
        let result = result_of(&payloads);
        assert_eq!(result["isError"], true);
        assert!(
            !result["content"][0]["text"]
                .as_str()
                .is_some_and(|message| message.contains("requires the 'write' OAuth scope")),
            "write-scoped request must reach production tool dispatch: {result}"
        );
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn oauth_read_scope_denies_write_scope_before_http_dispatch() {
        let runtime = assembled_runtime(&config_for("oauth", false));
        let router = runtime.router();
        let token = oauth_access_token(&router, "read").await;
        let _ = post_with_headers(
            &router,
            initialize(V_MODERN),
            None,
            None,
            "localhost",
            Some(&token),
        )
        .await;

        let (status, _, payloads) = post_with_headers(
            &router,
            modern_tools_call("place_stock_order"),
            None,
            Some(V_MODERN),
            "localhost",
            Some(&token),
        )
        .await;

        assert!(
            !status.is_server_error(),
            "unexpected HTTP failure: {status}"
        );
        let error = error_of(&payloads);
        assert_eq!(error["code"], -32602, "expected INVALID_PARAMS: {error}");
        assert!(
            error["message"]
                .as_str()
                .is_some_and(|message| message.contains("write")),
            "rejection must identify the required scope: {error}"
        );
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn oauth_read_scope_rejects_every_annotation_declared_write_tool() {
        let write_tools = annotation_declared_write_tool_names().await;
        let runtime = assembled_runtime(&config_for("oauth", false));
        let router = runtime.router();
        let token = oauth_access_token(&router, "read").await;
        let _ = post_with_headers(
            &router,
            initialize(V_MODERN),
            None,
            None,
            "localhost",
            Some(&token),
        )
        .await;

        for write_tool in write_tools {
            let (status, _, payloads) = post_with_headers(
                &router,
                modern_tools_call(&write_tool),
                None,
                Some(V_MODERN),
                "localhost",
                Some(&token),
            )
            .await;

            assert!(
                !status.is_server_error(),
                "unexpected HTTP failure for {write_tool}: {status}"
            );
            let error = error_of(&payloads);
            assert_eq!(
                error["code"], -32602,
                "expected INVALID_PARAMS for {write_tool}: {error}"
            );
            assert!(
                error["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("write")),
                "rejection must identify the required scope: {error}"
            );
        }
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn non_oauth_http_write_scope_reaches_dispatch() {
        let router = none_runtime(false).router();
        let _ = post(&router, initialize(V_MODERN), None, None).await;

        let (_, _, payloads) = post(
            &router,
            modern_tools_call("place_stock_order"),
            None,
            Some(V_MODERN),
        )
        .await;
        let result = result_of(&payloads);
        assert_eq!(result["isError"], true);
        assert!(
            !result["content"][0]["text"]
                .as_str()
                .is_some_and(|message| message.contains("OAuth scope")),
            "none-mode request must reach production tool dispatch: {result}"
        );
    }

    #[tokio::test]
    async fn call_tool_complete_carries_authoritative_server_info() {
        let mut complete = CallToolResult::success(vec![ContentBlock::text("complete")]);
        complete.meta = meta_with_marker("complete");
        let router =
            synthetic_result_router(complete.into(), McpConfig::default().max_response_bytes);
        let _ = post(&router, initialize(V_MODERN), None, None).await;
        let (status, _, payloads) =
            post(&router, modern_tools_call(TEST_TOOL), None, Some(V_MODERN)).await;

        assert!(
            status.is_success(),
            "tools/call failed: {status} {payloads:?}"
        );
        let result = result_of(&payloads);
        assert_eq!(result["resultType"], "complete");
        assert_eq!(result["content"][0]["text"], "complete");
        assert_eq!(result["_meta"]["unrelated"], "complete");
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/serverInfo"],
            expected_server_info()
        );
    }

    #[tokio::test]
    async fn stateless_http_handlers_share_rate_limit_and_stop_dispatch() {
        let mut config = config_for("none", false);
        config.mcp.rate_limit.read.per_principal_per_minute = 1;
        config.mcp.rate_limit.read.per_principal_burst = 2;
        config.mcp.rate_limit.read.global_per_minute = 1_000;
        config.mcp.rate_limit.read.global_burst = 100;
        let router = assembled_runtime(&config).router();
        let _ = post(&router, initialize(V_MODERN), None, None).await;

        for _burst_call in 0..2 {
            let (status, _, payloads) = post(
                &router,
                modern_tools_call("get_stock_quotes"),
                None,
                Some(V_MODERN),
            )
            .await;
            assert!(status.is_success(), "tool error is a result: {payloads:?}");
            let result = result_of(&payloads);
            assert_eq!(result["isError"], true);
            assert_ne!(result["structuredContent"]["error"], "rate_limited");
        }

        let (status, _, payloads) = post(
            &router,
            modern_tools_call("get_stock_quotes"),
            None,
            Some(V_MODERN),
        )
        .await;
        assert!(
            status.is_success(),
            "rate limit is a tool result: {payloads:?}"
        );
        let result = result_of(&payloads);
        assert_eq!(result["isError"], true);
        assert_eq!(result["structuredContent"]["error"], "rate_limited");
        assert_eq!(result["structuredContent"]["class"], "read");
        assert_eq!(result["structuredContent"]["layer"], "principal");
        assert_eq!(result["structuredContent"]["retry_after_seconds"], 60);
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/serverInfo"],
            expected_server_info()
        );
    }

    /// The structured rejection is exempt from the response budget: even a
    /// deliberately tiny `max_response_bytes` must not replace it (and its
    /// retry guidance) with `response_too_large`.
    #[tokio::test]
    async fn rate_limit_rejection_survives_a_tiny_response_budget() {
        let mut config = config_for("none", false);
        config.mcp.max_response_bytes = 1;
        config.mcp.rate_limit.read.per_principal_per_minute = 1;
        config.mcp.rate_limit.read.per_principal_burst = 2;
        config.mcp.rate_limit.read.global_per_minute = 1_000;
        config.mcp.rate_limit.read.global_burst = 100;
        let router = assembled_runtime(&config).router();
        let _ = post(&router, initialize(V_MODERN), None, None).await;

        for _burst_call in 0..2 {
            let (status, _, payloads) = post(
                &router,
                modern_tools_call("get_stock_quotes"),
                None,
                Some(V_MODERN),
            )
            .await;
            assert!(status.is_success(), "tool error is a result: {payloads:?}");
            assert_eq!(result_of(&payloads)["isError"], true);
        }

        let (status, _, payloads) = post(
            &router,
            modern_tools_call("get_stock_quotes"),
            None,
            Some(V_MODERN),
        )
        .await;
        assert!(status.is_success(), "rejection is a tool result");
        let result = result_of(&payloads);
        assert_eq!(result["isError"], true);
        assert_eq!(result["structuredContent"]["error"], "rate_limited");
        assert_eq!(result["structuredContent"]["retry_after_seconds"], 60);
    }

    #[tokio::test]
    async fn call_tool_task_carries_authoritative_server_info() {
        let task = Task::new(
            "task-id",
            TaskStatus::Working,
            "2026-08-10T00:00:00Z",
            "2026-08-10T00:00:00Z",
        );
        let mut task = CreateTaskResult::new(task);
        task.meta = meta_with_marker("task");
        let router = synthetic_result_router(task.into(), McpConfig::default().max_response_bytes);
        let capabilities = json!({
            "extensions": { "io.modelcontextprotocol/tasks": {} }
        });
        let _ = post(
            &router,
            initialize_with_capabilities(V_MODERN, capabilities.clone()),
            None,
            None,
        )
        .await;
        let (status, _, payloads) = post(
            &router,
            modern_tools_call_with_capabilities(TEST_TOOL, json!({}), capabilities),
            None,
            Some(V_MODERN),
        )
        .await;

        assert!(
            status.is_success(),
            "tools/call failed: {status} {payloads:?}"
        );
        let result = result_of(&payloads);
        assert_eq!(result["resultType"], "task");
        assert_eq!(result["taskId"], "task-id");
        assert_eq!(result["_meta"]["unrelated"], "task");
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/serverInfo"],
            expected_server_info()
        );
    }

    #[tokio::test]
    async fn call_tool_budget_replacement_still_carries_server_info() {
        let oversized = CallToolResult::success(vec![ContentBlock::text("x".repeat(2048))]);
        let router = synthetic_result_router(oversized.into(), 256);
        let _ = post(&router, initialize(V_MODERN), None, None).await;
        let (status, _, payloads) =
            post(&router, modern_tools_call(TEST_TOOL), None, Some(V_MODERN)).await;

        assert!(
            status.is_success(),
            "tools/call failed: {status} {payloads:?}"
        );
        let result = result_of(&payloads);
        assert_eq!(result["resultType"], "complete");
        let text = result["content"][0]["text"]
            .as_str()
            .expect("replacement has text content");
        assert!(text.contains("response_too_large"), "got: {text}");
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/serverInfo"],
            expected_server_info()
        );
    }

    #[tokio::test]
    async fn tools_call_result_serializes_rhood_server_info_metadata() {
        let router = synthetic_result_router(InputRequiredResult::new(None, None).into(), 262_144);
        let _ = post(&router, initialize(V_MODERN), None, None).await;
        let (status, _, payloads) =
            post(&router, modern_tools_call(TEST_TOOL), None, Some(V_MODERN)).await;

        assert!(
            status.is_success(),
            "tools/call failed: {status} {payloads:?}"
        );
        let result = result_of(&payloads);
        assert_eq!(result["resultType"], "input_required");
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/serverInfo"],
            expected_server_info()
        );
    }

    /// `ProtocolVersion::LATEST` still resolves to 2025-11-25 in rmcp 3.0, which
    /// is why `get_info` names 2026-07-28 explicitly. If this ever fails, the SDK
    /// moved its default and the hardcoded constant should be revisited.
    #[test]
    fn sdk_latest_still_lags_the_version_we_advertise() {
        assert_eq!(ProtocolVersion::LATEST, ProtocolVersion::V_2025_11_25);
    }
}
