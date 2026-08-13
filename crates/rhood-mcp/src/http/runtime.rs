use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Router, middleware,
    routing::{get, post},
};
use rhood_core::RobinhoodClient;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager,
    tower::{StreamableHttpServerConfig, StreamableHttpService},
};
use secrecy::ExposeSecret;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::config::{McpConfig, OAuthCorsConfig, ServerConfig};
use crate::middleware::cors::oauth_cors_layer;
use crate::middleware::oauth::{OAuthMiddlewareState, oauth_bearer_auth};
use crate::middleware::static_bearer_auth::{StaticAuthToken, static_bearer_auth};
use crate::oauth;
use crate::oauth::model::{OAuthEndpointState, RegisteredClient};
use crate::oauth::store::{OAuthStore, OAuthStoreConfig};
use crate::shared::resolve_base_url;
use crate::tools::{FallbackPrincipal, RhoodTools, ToolRateLimiter};

pub(super) struct HttpPlan {
    addr: SocketAddr,
    read_only: bool,
    mcp: McpConfig,
    auth: AuthMode,
    rate_limiter: Arc<ToolRateLimiter>,
}

enum AuthMode {
    Token(StaticAuthToken),
    OAuth(OAuthPlan),
    None,
}

struct OAuthPlan {
    base_url: String,
    registered_clients: Vec<RegisteredClient>,
    store_config: OAuthStoreConfig,
    sweep_interval: Duration,
    oauth_pin: Option<String>,
    cors: OAuthCorsConfig,
}

impl HttpPlan {
    pub(super) fn validate(config: &ServerConfig) -> anyhow::Result<Self> {
        let addr: SocketAddr = format!("{}:{}", config.mcp.host, config.mcp.port).parse()?;
        let auth = match config.mcp.auth_mode.as_str() {
            "token" => {
                let Some(token) = config.mcp.token.as_ref() else {
                    anyhow::bail!(
                        "auth_mode=\"token\" requires a token. Set RHOOD_MCP_TOKEN, \
                        pass --token <value>, or add [mcp] token = \"...\" to config.toml"
                    );
                };
                AuthMode::Token(StaticAuthToken(token.expose_secret().to_string()))
            }
            "oauth" => {
                config.mcp.validate_registered_clients()?;
                AuthMode::OAuth(OAuthPlan {
                    base_url: resolve_base_url(config, &addr),
                    registered_clients: config.mcp.registered_clients.clone(),
                    store_config: OAuthStoreConfig::from_mcp_config(&config.mcp),
                    sweep_interval: Duration::from_secs(config.mcp.oauth_sweep_interval_secs),
                    oauth_pin: config
                        .mcp
                        .oauth_pin
                        .as_ref()
                        .map(|secret| secret.expose_secret().to_string()),
                    cors: config.mcp.oauth_cors.clone(),
                })
            }
            "none" => {
                if !addr.ip().is_loopback() {
                    anyhow::bail!(
                        "auth_mode=\"none\" is only allowed on loopback addresses. \
                        Current bind address: {addr}"
                    );
                }
                AuthMode::None
            }
            other => {
                anyhow::bail!("Unknown auth_mode: {other}. Use \"token\", \"oauth\", or \"none\"")
            }
        };
        let rate_limiter = ToolRateLimiter::new(&config.mcp.rate_limit)?;

        Ok(Self {
            addr,
            read_only: config.core.read_only,
            mcp: config.mcp.clone(),
            auth,
            rate_limiter,
        })
    }

    pub(super) const fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub(super) const fn auth_name(&self) -> &'static str {
        match self.auth {
            AuthMode::Token(_) => "token",
            AuthMode::OAuth(_) => "oauth",
            AuthMode::None => "none",
        }
    }
}

pub(super) struct HttpRuntime {
    router: Router,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
}

impl HttpRuntime {
    pub(super) fn assemble(plan: HttpPlan, client: RobinhoodClient) -> Self {
        let cancellation_token = CancellationToken::new();
        let auth_name = plan.auth_name();
        let fallback_principal = match &plan.auth {
            AuthMode::Token(_) => FallbackPrincipal::HttpToken,
            AuthMode::OAuth(_) => FallbackPrincipal::HttpOAuth,
            AuthMode::None => FallbackPrincipal::HttpNone,
        };
        let session_manager: Arc<LocalSessionManager> = Arc::default();
        let mcp_config = plan.mcp.clone();
        let read_only = plan.read_only;
        let rate_limiter = plan.rate_limiter;
        let service: StreamableHttpService<RhoodTools, LocalSessionManager> =
            StreamableHttpService::new(
                move || {
                    Ok(RhoodTools::new_eager(
                        client.clone(),
                        read_only,
                        &mcp_config,
                        rate_limiter.clone(),
                        fallback_principal,
                    ))
                },
                session_manager,
                StreamableHttpServerConfig::default()
                    .with_legacy_session_mode(false)
                    .with_stateless_protocol_metadata_required(true)
                    .with_cancellation_token(cancellation_token.child_token())
                    .with_allowed_hosts(plan.mcp.allowed_hosts.clone()),
            );

        tracing::debug!(auth_mode = auth_name, "Building router");
        let mut tasks = Vec::new();
        let router = match plan.auth {
            AuthMode::Token(token) => {
                tracing::info!("Using static bearer token authentication");
                Router::new()
                    .nest_service("/mcp", service)
                    .layer(middleware::from_fn_with_state(token, static_bearer_auth))
            }
            AuthMode::OAuth(oauth_plan) => {
                tracing::info!("Using OAuth 2.1 authentication with PKCE");
                let store = OAuthStore::with_registered_clients(
                    oauth_plan.store_config,
                    oauth_plan.registered_clients,
                );
                tasks.push(
                    store.clone().spawn_sweep_task(
                        oauth_plan.sweep_interval,
                        cancellation_token.child_token(),
                    ),
                );

                let endpoint_state = Arc::new(OAuthEndpointState {
                    store: store.clone(),
                    base_url: oauth_plan.base_url.clone(),
                    server_read_only: plan.read_only,
                    oauth_pin: oauth_plan.oauth_pin,
                });
                let middleware_state = OAuthMiddlewareState {
                    store,
                    resource_metadata_url: format!(
                        "{}/.well-known/oauth-protected-resource",
                        oauth_plan.base_url
                    ),
                };

                tracing::debug!(
                    base_url = %oauth_plan.base_url,
                    pin_required = endpoint_state.oauth_pin.is_some(),
                    "OAuth routes configured"
                );

                let oauth_routes = Router::new()
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
                    .layer(oauth_cors_layer(oauth_plan.cors))
                    .with_state(endpoint_state);
                let mcp_routes = Router::new().nest_service("/mcp", service).layer(
                    middleware::from_fn_with_state(middleware_state, oauth_bearer_auth),
                );

                oauth_routes.merge(mcp_routes)
            }
            AuthMode::None => {
                tracing::warn!("Running with auth_mode=none, no authentication on MCP endpoints");
                Router::new().nest_service("/mcp", service)
            }
        };

        if !plan.addr.ip().is_loopback() {
            tracing::warn!(
                "MCP server binding to non-loopback address {}. \
                 Ensure network access is intentional and properly secured.",
                plan.addr
            );
            if plan.mcp.allowed_hosts == McpConfig::default().allowed_hosts {
                tracing::warn!(
                    "[mcp] allowed_hosts is still the loopback default {:?}, but the server is \
                     bound to a non-loopback address. Requests carrying any other Host header \
                     will be rejected with 403 by DNS-rebinding validation. Set allowed_hosts \
                     in config.toml or RHOOD_MCP_ALLOWED_HOSTS to the hostnames clients will use.",
                    plan.mcp.allowed_hosts
                );
            }
        }

        Self {
            router,
            cancellation_token,
            tasks,
        }
    }

    pub(super) fn router(&self) -> Router {
        self.router.clone()
    }

    pub(super) fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }

    #[cfg(test)]
    pub(super) fn background_task_count(&self) -> usize {
        self.tasks.len()
    }

    pub(super) async fn shutdown(self) {
        self.cancellation_token.cancel();
        for task in self.tasks {
            if let Err(error) = task.await {
                tracing::warn!(%error, "HTTP runtime task failed during shutdown");
            }
        }
    }
}
