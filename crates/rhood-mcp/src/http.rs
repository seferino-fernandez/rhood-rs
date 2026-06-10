use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::config::ServerConfig;
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
            {
                let mut config = StreamableHttpServerConfig::default();
                config.stateful_mode = true;
                config.cancellation_token = cancellation_token.child_token();
                config
            },
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
            tracing::warn!("Running with auth_mode=none — no authentication on MCP endpoints");
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
