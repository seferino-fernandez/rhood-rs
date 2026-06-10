use clap::Parser;
use rhood_core::RobinhoodClient;
use rmcp::model::{LoggingLevel, LoggingMessageNotificationParam};
use rmcp::{Peer, RoleServer};
use secrecy::{ExposeSecret, SecretString};

use crate::config::ServerConfig;

/// Runs the Robinhood login flow on demand, sending MCP logging notifications
/// to the connected peer as authentication progresses.
///
/// When `config.core.auth.username` is set, delegates to
/// [`RobinhoodClient::login`], whose internal cascade tries the on-disk token
/// cache, then token refresh, then a headless password grant.
///
/// When no username is configured, falls back to
/// [`RobinhoodClient::login_from_cache`] so a user who ran `rhood login` in a
/// terminal can drive the MCP server from launchers (e.g., Claude Desktop)
/// that don't inherit shell env vars. If the cache is empty or stale, returns
/// a clear error instructing the caller what to set.
///
/// Used by the stdio transport's lazy-auth hook on the first tool call.
pub(crate) async fn authenticate_on_demand(
    client: &RobinhoodClient,
    config: &ServerConfig,
    peer: &Peer<RoleServer>,
) -> Result<(), String> {
    let _ = peer
        .notify_logging_message(LoggingMessageNotificationParam::new(
            LoggingLevel::Info,
            serde_json::json!("Authenticating with Robinhood..."),
        ))
        .await;

    let Some(username) = config.core.auth.username.as_ref() else {
        return authenticate_from_cache_only(client, peer).await;
    };
    let password_secret =
        config.core.auth.password.as_ref().ok_or_else(|| {
            "Set RHOOD_PASSWORD env var or add password to config.toml".to_string()
        })?;
    // Materialize the MFA secret as an owned String before any .await so we
    // don't hold a borrow of `config` across await points — which would become
    // a compile error if `config` is ever moved behind a lock.
    let mfa: Option<String> = config
        .core
        .auth
        .mfa_secret
        .as_ref()
        .map(|secret| secret.expose_secret().to_owned());

    let _ = peer
        .notify_logging_message(LoggingMessageNotificationParam::new(
            LoggingLevel::Info,
            serde_json::json!(
                "Logging in \u{2014} device verification may be required. \
                 Please check your Robinhood app if prompted."
            ),
        ))
        .await;

    client
        .login(username, password_secret.expose_secret(), mfa.as_deref())
        .await
        .map_err(|err| format!("Robinhood login failed: {err}"))?;

    let _ = peer
        .notify_logging_message(LoggingMessageNotificationParam::new(
            LoggingLevel::Info,
            serde_json::json!("Robinhood login successful"),
        ))
        .await;

    Ok(())
}

/// Attempts to authenticate using only the on-disk token cache.
///
/// Used when no username is configured — the caller has no way to perform a
/// fresh password grant, so a cached token is the only path.
async fn authenticate_from_cache_only(
    client: &RobinhoodClient,
    peer: &Peer<RoleServer>,
) -> Result<(), String> {
    match client.login_from_cache().await {
        Ok(true) => {
            let _ = peer
                .notify_logging_message(LoggingMessageNotificationParam::new(
                    LoggingLevel::Info,
                    serde_json::json!("Restored Robinhood session from cached token"),
                ))
                .await;
            Ok(())
        }
        Ok(false) => Err(
            "Not logged in. Run `rhood login` to create a cached token, \
             or set RHOOD_USERNAME and RHOOD_PASSWORD in the server env."
                .to_string(),
        ),
        Err(err) => Err(format!("Cached-token login failed: {err}")),
    }
}

#[derive(Parser)]
#[command(
    name = "rhood-mcp",
    about = "MCP server exposing Robinhood trading operations",
    long_about = "An MCP (Model Context Protocol) server that exposes Robinhood brokerage \
                  operations as tools. Supports stdio and HTTP transports with static token, \
                  OAuth 2.1 (PKCE), or no-auth modes.\n\n\
                  Configuration is loaded from ~/.config/rhood/config.toml with environment \
                  variable overrides (RHOOD_MCP_*).",
    version
)]
pub struct Args {
    /// Path to config.toml (overrides `RHOOD_CONFIG` and the platform default)
    #[arg(long, value_name = "PATH")]
    pub config: Option<std::path::PathBuf>,

    /// Path to the on-disk token cache (overrides `auth.token_cache_path`
    /// and `RHOOD_TOKEN_CACHE_PATH`)
    #[arg(long, value_name = "PATH")]
    pub token_cache: Option<std::path::PathBuf>,

    /// Transport protocol: "stdio" or "http"
    #[arg(long, default_value = "stdio", value_parser = ["stdio", "http"])]
    pub transport: String,

    /// Bind address for HTTP transport [default: 127.0.0.1]
    #[arg(long)]
    pub host: Option<String>,

    /// Bind port for HTTP transport [default: 8080]
    #[arg(long)]
    pub port: Option<u16>,

    /// Static bearer token for auth_mode=token [env: RHOOD_MCP_TOKEN]
    #[arg(long)]
    pub token: Option<String>,

    /// Enable write operations (order placement and cancellation).
    /// By default the server runs in read-only mode.
    #[arg(long)]
    pub read_write: bool,

    /// External base URL for OAuth metadata discovery.
    /// Falls back to http://{host}:{port} if unset [env: RHOOD_MCP_BASE_URL]
    #[arg(long)]
    pub base_url: Option<String>,

    /// Authentication mode: "token", "oauth", or "none".
    /// "none" is restricted to loopback addresses.
    #[arg(long, value_parser = ["token", "oauth", "none"])]
    pub auth_mode: Option<String>,

    /// PIN required on the OAuth consent screen.
    /// When set, users must enter this PIN to approve authorization requests
    /// [env: RHOOD_MCP_OAUTH_PIN]
    #[arg(long)]
    pub oauth_pin: Option<String>,

    /// Total HTTP request timeout in seconds (overrides RHOOD_HTTP_REQUEST_TIMEOUT_SECS)
    #[arg(long)]
    pub http_request_timeout_secs: Option<u64>,

    /// TCP connect timeout in seconds (overrides RHOOD_HTTP_CONNECT_TIMEOUT_SECS)
    #[arg(long)]
    pub http_connect_timeout_secs: Option<u64>,

    /// Disable the resolver cache entirely (overrides RHOOD_CACHE_ENABLED).
    #[arg(long)]
    pub cache_disabled: bool,

    /// Instrument metadata TTL in seconds
    /// (overrides RHOOD_CACHE_INSTRUMENT_TTL_SECS).
    #[arg(long)]
    pub cache_instrument_ttl_secs: Option<u64>,

    /// Index instrument TTL in seconds (overrides RHOOD_CACHE_INDEX_TTL_SECS).
    #[arg(long)]
    pub cache_index_ttl_secs: Option<u64>,

    /// Futures contract TTL in seconds (overrides RHOOD_CACHE_FUTURES_TTL_SECS).
    #[arg(long)]
    pub cache_futures_ttl_secs: Option<u64>,
}

pub fn apply_cli_overrides(config: &mut ServerConfig, args: &Args) {
    if let Some(path) = args.token_cache.as_deref() {
        config.core.auth.token_cache_path = path.to_string_lossy().into_owned();
        config.core.normalize();
    }
    if let Some(host) = &args.host {
        config.mcp.host.clone_from(host);
    }
    if let Some(port) = args.port {
        config.mcp.port = port;
    }
    if let Some(token) = &args.token {
        config.mcp.token = Some(SecretString::from(token.clone()));
    }
    if args.read_write {
        config.core.read_only = false;
    }
    if let Some(base_url) = &args.base_url {
        config.mcp.base_url = Some(base_url.clone());
    }
    if let Some(auth_mode) = &args.auth_mode {
        config.mcp.auth_mode.clone_from(auth_mode);
    }
    if let Some(pin) = &args.oauth_pin {
        config.mcp.oauth_pin = Some(SecretString::from(pin.clone()));
    }
    if let Some(secs) = args.http_request_timeout_secs {
        config.core.http.request_timeout_secs = secs;
    }
    if let Some(secs) = args.http_connect_timeout_secs {
        config.core.http.connect_timeout_secs = secs;
    }
    if args.cache_disabled {
        config.core.cache.enabled = false;
    }
    if let Some(secs) = args.cache_instrument_ttl_secs {
        config.core.cache.instrument_ttl_secs = secs;
    }
    if let Some(secs) = args.cache_index_ttl_secs {
        config.core.cache.index_ttl_secs = secs;
    }
    if let Some(secs) = args.cache_futures_ttl_secs {
        config.core.cache.futures_ttl_secs = secs;
    }
}

/// Resolve the external base URL for OAuth metadata.
///
/// Uses `mcp.base_url` from config if set, otherwise falls back to
/// `http://{addr}`. Logs a warning when the resolved URL uses plain HTTP
/// on a non-loopback address.
pub fn resolve_base_url(config: &ServerConfig, addr: &std::net::SocketAddr) -> String {
    let base_url = config
        .mcp
        .base_url
        .clone()
        .unwrap_or_else(|| format!("http://{addr}"));

    if !base_url.starts_with("https://") && !addr.ip().is_loopback() {
        tracing::warn!(
            %base_url,
            "OAuth base_url uses plain HTTP on a non-loopback address. \
             Set [mcp] base_url = \"https://...\" for production deployments."
        );
    }

    base_url
}

pub async fn create_authenticated_client(config: &ServerConfig) -> anyhow::Result<RobinhoodClient> {
    let client = RobinhoodClient::with_config(config.core.clone())?;
    let username =
        config.core.auth.username.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Not logged in. Run `rhood login` or set RHOOD_USERNAME")
        })?;
    let password_secret = config.core.auth.password.as_ref().ok_or_else(|| {
        anyhow::anyhow!("Set RHOOD_PASSWORD env var or add password to config.toml")
    })?;
    let mfa = config
        .core
        .auth
        .mfa_secret
        .as_ref()
        .map(|secret| secret.expose_secret());
    client
        .login(username, password_secret.expose_secret(), mfa)
        .await?;
    Ok(client)
}

#[cfg(test)]
mod tests {
    use crate::config::ServerConfig;
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn token_cache_cli_override_wins_and_tilde_expands() {
        let mut config = ServerConfig::default();
        let args = Args {
            config: None,
            token_cache: Some(PathBuf::from("~/custom/.rhood-token")),
            transport: "stdio".into(),
            host: None,
            port: None,
            token: None,
            read_write: false,
            base_url: None,
            auth_mode: None,
            oauth_pin: None,
            http_request_timeout_secs: None,
            http_connect_timeout_secs: None,
            cache_disabled: false,
            cache_instrument_ttl_secs: None,
            cache_index_ttl_secs: None,
            cache_futures_ttl_secs: None,
        };
        apply_cli_overrides(&mut config, &args);
        let expanded = &config.core.auth.token_cache_path;
        assert!(
            !expanded.starts_with('~'),
            "CLI override should be tilde-expanded, got {expanded}"
        );
        assert!(
            expanded.ends_with("/custom/.rhood-token"),
            "CLI override should be preserved, got {expanded}"
        );
    }
}
