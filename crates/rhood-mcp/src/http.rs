use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::config::{McpConfig, ServerConfig};
use crate::middleware::cors::oauth_cors_layer;
use crate::middleware::oauth::{OAuthMiddlewareState, oauth_bearer_auth};
use crate::middleware::static_bearer_auth::{StaticAuthToken, static_bearer_auth};
use crate::oauth;
use crate::oauth::model::OAuthEndpointState;
use crate::oauth::store::{OAuthStore, OAuthStoreConfig};
use crate::shared::{create_authenticated_client, resolve_base_url};
use crate::tools::RhoodTools;
use axum::{
    middleware::{self},
    routing::{get, post},
};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager,
    tower::{StreamableHttpServerConfig, StreamableHttpService},
};
use secrecy::ExposeSecret;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub async fn serve(config: &ServerConfig) -> anyhow::Result<()> {
    let addr: SocketAddr = format!("{}:{}", config.mcp.host, config.mcp.port).parse()?;
    match config.mcp.auth_mode.as_str() {
        "token" => {
            if config.mcp.token.is_none() {
                anyhow::bail!(
                    "auth_mode=\"token\" requires a token. Set RHOOD_MCP_TOKEN, \
                     pass --token <value>, or add [mcp] token = \"...\" to config.toml"
                );
            }
        }
        "none" => {
            if !addr.ip().is_loopback() {
                anyhow::bail!(
                    "auth_mode=\"none\" is only allowed on loopback addresses. \
                     Current bind address: {addr}"
                );
            }
        }
        "oauth" => {}
        other => anyhow::bail!("Unknown auth_mode: {other}. Use \"token\", \"oauth\", or \"none\""),
    }

    tracing::debug!("Authenticating Robinhood client...");
    let client = create_authenticated_client(config).await?;
    tracing::info!("Robinhood client authenticated successfully");
    let read_only = config.core.read_only;
    let mcp_config = config.mcp.clone();
    let shared_pending_orders = Arc::new(Mutex::new(HashMap::new()));

    let cancellation_token = CancellationToken::new();
    let session_manager: Arc<LocalSessionManager> = Arc::default();
    let service: StreamableHttpService<RhoodTools, LocalSessionManager> =
        StreamableHttpService::new(
            move || {
                Ok(RhoodTools::new_eager(
                    client.clone(),
                    read_only,
                    &mcp_config,
                    shared_pending_orders.clone(),
                ))
            },
            session_manager,
            StreamableHttpServerConfig::default()
                // Sessions apply only to protocol versions older than 2026-07-28.
                // Per SEP-2567 the 2026-07-28 protocol removed them, so modern
                // clients are served statelessly regardless of this setting;
                // keeping it on preserves sessions and resumability for older
                // clients.
                .with_legacy_session_mode(true)
                .with_cancellation_token(cancellation_token.child_token())
                // Guards against DNS rebinding. Defaults to loopback only, so a
                // non-loopback deployment must list its own hostnames here.
                .with_allowed_hosts(config.mcp.allowed_hosts.clone()),
        );

    tracing::debug!(auth_mode = %config.mcp.auth_mode, "Building router");
    let router = match config.mcp.auth_mode.as_str() {
        "token" => {
            tracing::info!("Using static bearer token authentication");
            let token = config
                .mcp
                .token
                .as_ref()
                .map(|secret| secret.expose_secret().to_string())
                .unwrap();
            axum::Router::new()
                .nest_service("/mcp", service)
                .layer(middleware::from_fn_with_state(
                    StaticAuthToken(token),
                    static_bearer_auth,
                ))
        }
        "oauth" => {
            tracing::info!("Using OAuth 2.1 authentication with PKCE");
            let base_url = resolve_base_url(config, &addr);
            let store_config = OAuthStoreConfig::from_mcp_config(&config.mcp);
            let sweep_interval = Duration::from_secs(config.mcp.oauth_sweep_interval_secs);
            let store = OAuthStore::new(store_config);

            store
                .clone()
                .spawn_sweep_task(sweep_interval, cancellation_token.child_token());

            let endpoint_state = Arc::new(OAuthEndpointState {
                store: store.clone(),
                base_url: base_url.clone(),
                server_read_only: config.core.read_only,
                oauth_pin: config
                    .mcp
                    .oauth_pin
                    .as_ref()
                    .map(|secret| secret.expose_secret().to_string()),
            });

            let middleware_state = OAuthMiddlewareState {
                store,
                resource_metadata_url: format!("{base_url}/.well-known/oauth-protected-resource"),
            };

            tracing::debug!(
                %base_url,
                pin_required = endpoint_state.oauth_pin.is_some(),
                "OAuth routes configured"
            );

            let cors_layer = oauth_cors_layer(config.mcp.oauth_cors.clone());

            let oauth_routes = axum::Router::new()
                .route(
                    "/.well-known/oauth-protected-resource",
                    get(oauth::endpoints::protected_resource_metadata),
                )
                .route(
                    "/.well-known/oauth-protected-resource/mcp",
                    get(oauth::endpoints::protected_resource_metadata),
                )
                .route(
                    "/.well-known/oauth-authorization-server",
                    get(oauth::endpoints::authorization_server_metadata)
                        .options(oauth::endpoints::authorization_server_metadata),
                )
                .route(
                    "/.well-known/openid-configuration",
                    get(oauth::endpoints::authorization_server_metadata)
                        .options(oauth::endpoints::authorization_server_metadata),
                )
                .route("/oauth/authorize", get(oauth::endpoints::authorize))
                .route("/oauth/approve", post(oauth::endpoints::approve))
                .route(
                    "/oauth/token",
                    post(oauth::endpoints::token).options(oauth::endpoints::token),
                )
                .route(
                    "/oauth/register",
                    post(oauth::endpoints::register).options(oauth::endpoints::register),
                )
                .layer(cors_layer)
                .with_state(endpoint_state);

            let mcp_routes = axum::Router::new().nest_service("/mcp", service).layer(
                middleware::from_fn_with_state(middleware_state, oauth_bearer_auth),
            );

            oauth_routes.merge(mcp_routes)
        }
        "none" => {
            tracing::warn!("Running with auth_mode=none, no authentication on MCP endpoints");
            axum::Router::new().nest_service("/mcp", service)
        }
        _ => unreachable!(),
    };

    if !addr.ip().is_loopback() {
        tracing::warn!(
            "MCP server binding to non-loopback address {}. \
             Ensure network access is intentional and properly secured.",
            addr
        );
        // The `Host` allow-list is a separate axis from the bind address, and
        // its loopback default silently 403s every off-loopback request before
        // it reaches a handler. Warn rather than bail: binding 0.0.0.0 inside a
        // container while still being addressed as `localhost` is legitimate.
        if config.mcp.allowed_hosts == McpConfig::default().allowed_hosts {
            tracing::warn!(
                "[mcp] allowed_hosts is still the loopback default {:?}, but the server is \
                 bound to a non-loopback address. Requests carrying any other Host header \
                 will be rejected with 403 by DNS-rebinding validation. Set allowed_hosts \
                 in config.toml or RHOOD_MCP_ALLOWED_HOSTS to the hostnames clients will use.",
                config.mcp.allowed_hosts
            );
        }
    }

    tracing::info!(
        "Starting HTTP MCP server on {} (auth_mode={})",
        addr,
        config.mcp.auth_mode
    );
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal(cancellation_token))
        .await?;

    Ok(())
}

/// Waits for SIGINT (Ctrl+C) or SIGTERM (Unix), then cancels `cancellation_token`
/// so background tasks (e.g. the OAuth store sweep) exit alongside the HTTP server.
///
/// SIGTERM handling matters for container orchestrators (Docker, Kubernetes) that
/// request graceful shutdown via SIGTERM before escalating to SIGKILL.
async fn shutdown_signal(cancellation_token: CancellationToken) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install SIGINT handler");
    };

    #[cfg(unix)]
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
    use crate::tools::WRITE_TOOLS;
    use axum::body::Body;
    use rhood_core::{RhoodConfig, RobinhoodClient};
    use rmcp::model::ProtocolVersion;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    const V_MODERN: &str = "2026-07-28";
    const V_LEGACY: &str = "2025-11-25";

    fn mcp_service(read_only: bool) -> StreamableHttpService<RhoodTools, LocalSessionManager> {
        let client =
            RobinhoodClient::with_config(RhoodConfig::default()).expect("default config builds");
        let pending_orders = Arc::new(Mutex::new(HashMap::new()));
        StreamableHttpService::new(
            move || {
                Ok(RhoodTools::new_eager(
                    client.clone(),
                    read_only,
                    &McpConfig::default(),
                    pending_orders.clone(),
                ))
            },
            Arc::default(),
            StreamableHttpServerConfig::default()
                .with_legacy_session_mode(true)
                .with_allowed_hosts(McpConfig::default().allowed_hosts),
        )
    }

    /// Sends one JSON-RPC message and returns the status, headers, and every
    /// JSON payload in the response (SSE `data:` frames, or a lone JSON body).
    async fn post(
        service: &StreamableHttpService<RhoodTools, LocalSessionManager>,
        body: Value,
        session_id: Option<&str>,
        protocol_version: Option<&str>,
    ) -> (axum::http::StatusCode, axum::http::HeaderMap, Vec<Value>) {
        let mut request = axum::http::Request::builder()
            .method("POST")
            .uri("/")
            // Loopback, to satisfy rmcp's DNS-rebinding guard.
            .header("host", "localhost")
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
        let request = request
            .body(Body::from(body.to_string()))
            .expect("request builds");

        let response = service.clone().oneshot(request).await.expect("infallible");
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
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": version,
                "capabilities": {},
                "clientInfo": { "name": "wire-test", "version": "0" }
            }
        })
    }

    /// Lifecycle metadata every stateless 2026-07-28 request carries in place of
    /// a session (SEP-2575).
    fn modern_meta() -> Value {
        json!({
            "io.modelcontextprotocol/protocolVersion": V_MODERN,
            "io.modelcontextprotocol/clientInfo": { "name": "wire-test", "version": "0" },
            "io.modelcontextprotocol/clientCapabilities": {}
        })
    }

    fn modern_tools_list() -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": { "_meta": modern_meta() }
        })
    }

    fn modern_tools_call(name: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": { "name": name, "arguments": {}, "_meta": modern_meta() }
        })
    }

    fn legacy_tools_list() -> Value {
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })
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

    /// Per SEP-2567 the 2026-07-28 protocol removed sessions, so a modern client
    /// is served statelessly even though `legacy_session_mode` is on.
    #[tokio::test]
    async fn modern_client_negotiates_2026_07_28_without_a_session() {
        let service = mcp_service(false);
        let (status, headers, payloads) = post(&service, initialize(V_MODERN), None, None).await;

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

    /// The other half of dual mode: `legacy_session_mode` still gives pre-2026
    /// clients a session, so they keep resumability.
    #[tokio::test]
    async fn legacy_client_still_gets_a_session() {
        let service = mcp_service(false);
        let (status, headers, payloads) = post(&service, initialize(V_LEGACY), None, None).await;

        assert!(
            status.is_success(),
            "initialize failed: {status} {payloads:?}"
        );
        assert_eq!(result_of(&payloads)["protocolVersion"], V_LEGACY);
        assert!(
            headers.get("mcp-session-id").is_some(),
            "legacy sessions must still be issued, got {headers:?}"
        );
    }

    /// A modern peer gets the SEP-2322 `resultType` discriminator and the
    /// SEP-2549 cache hints set in `ServerHandler::list_tools`.
    #[tokio::test]
    async fn modern_tools_list_carries_result_type_and_cache_hints() {
        let service = mcp_service(false);
        let _ = post(&service, initialize(V_MODERN), None, None).await;
        let (status, _, payloads) = post(&service, modern_tools_list(), None, Some(V_MODERN)).await;

        assert!(
            status.is_success(),
            "tools/list failed: {status} {payloads:?}"
        );
        let result = result_of(&payloads);
        assert_eq!(result["resultType"], "complete");
        assert_eq!(result["ttlMs"], 300_000);
        assert_eq!(result["cacheScope"], "private");
        assert!(
            !tool_names(&result).is_empty(),
            "a read-write server must advertise tools"
        );
    }

    /// The legacy wire shape is unchanged: rmcp clears `resultType` before
    /// replying to a peer that negotiated an older version.
    #[tokio::test]
    async fn legacy_tools_list_omits_result_type() {
        let service = mcp_service(false);
        let (_, headers, _) = post(&service, initialize(V_LEGACY), None, None).await;
        let session = headers
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("legacy initialize issues a session")
            .to_string();

        let (status, _, payloads) = post(&service, legacy_tools_list(), Some(&session), None).await;
        assert!(
            status.is_success(),
            "tools/list failed: {status} {payloads:?}"
        );
        let result = result_of(&payloads);
        assert!(
            result.get("resultType").is_none(),
            "legacy peers must not receive resultType: {result}"
        );
        assert!(!tool_names(&result).is_empty());
    }

    /// First of the three read-only enforcement points: `list_tools` must not
    /// advertise anything that places, cancels, or modifies an order.
    #[tokio::test]
    async fn read_only_server_advertises_no_write_tools() {
        let service = mcp_service(true);
        let _ = post(&service, initialize(V_MODERN), None, None).await;
        let (_, _, payloads) = post(&service, modern_tools_list(), None, Some(V_MODERN)).await;

        let names = tool_names(&result_of(&payloads));
        assert!(
            !names.is_empty(),
            "read-only mode must still advertise read tools"
        );
        for write_tool in WRITE_TOOLS {
            assert!(
                !names.iter().any(|name| name == write_tool),
                "read-only mode must not advertise `{write_tool}`, got {names:?}"
            );
        }
    }

    /// The complement: with the gate off, every write tool is reachable. Guards
    /// against a filter that over-matches and hides tools unconditionally.
    #[tokio::test]
    async fn read_write_server_advertises_every_write_tool() {
        let service = mcp_service(false);
        let _ = post(&service, initialize(V_MODERN), None, None).await;
        let (_, _, payloads) = post(&service, modern_tools_list(), None, Some(V_MODERN)).await;

        let names = tool_names(&result_of(&payloads));
        for write_tool in WRITE_TOOLS {
            assert!(
                names.iter().any(|name| name == write_tool),
                "read-write mode must advertise `{write_tool}`, got {names:?}"
            );
        }
    }

    /// Second enforcement point, and the one that actually protects the account:
    /// hiding a tool from `tools/list` means nothing if a client can still call
    /// it by name.
    #[tokio::test]
    async fn read_only_server_rejects_a_write_tool_call() {
        let service = mcp_service(true);
        let _ = post(&service, initialize(V_MODERN), None, None).await;
        let (_, _, payloads) = post(
            &service,
            modern_tools_call("place_stock_order"),
            None,
            Some(V_MODERN),
        )
        .await;

        let error = error_of(&payloads);
        assert_eq!(
            error["code"], -32602,
            "expected INVALID_PARAMS, got {error}"
        );
        let message = error["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("read-only"),
            "rejection must say why: {message}"
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
