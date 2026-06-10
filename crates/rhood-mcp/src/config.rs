//! MCP server configuration.
//!
//! [`ServerConfig`] composes the core [`RhoodConfig`](rhood_core::RhoodConfig)
//! with MCP-specific settings via `#[serde(flatten)]`, so a single TOML file
//! serves both the API client and the MCP server.

use std::path::Path;

use rhood_core::config::resolve_secret;
use rhood_core::env::{Env, SystemEnv, env_non_empty, env_non_empty_u64};
use secrecy::SecretString;
use serde::Deserialize;

const ENV_MCP_HOST: &str = "RHOOD_MCP_HOST";
const ENV_MCP_PORT: &str = "RHOOD_MCP_PORT";
const ENV_MCP_TOKEN: &str = "RHOOD_MCP_TOKEN";
const ENV_MCP_TOKEN_FILE: &str = "RHOOD_MCP_TOKEN_FILE";
const ENV_MCP_AUTH_MODE: &str = "RHOOD_MCP_AUTH_MODE";
const ENV_MCP_BASE_URL: &str = "RHOOD_MCP_BASE_URL";
const ENV_MCP_OAUTH_PIN: &str = "RHOOD_MCP_OAUTH_PIN";
const ENV_MCP_OAUTH_PIN_FILE: &str = "RHOOD_MCP_OAUTH_PIN_FILE";
const ENV_MCP_OAUTH_TOKEN_EXPIRY_SECS: &str = "RHOOD_MCP_OAUTH_TOKEN_EXPIRY_SECS";
const ENV_MCP_OAUTH_AUTH_CODE_TTL_SECS: &str = "RHOOD_MCP_OAUTH_AUTH_CODE_TTL_SECS";
const ENV_MCP_OAUTH_CSRF_NONCE_TTL_SECS: &str = "RHOOD_MCP_OAUTH_CSRF_NONCE_TTL_SECS";
const ENV_MCP_OAUTH_SWEEP_INTERVAL_SECS: &str = "RHOOD_MCP_OAUTH_SWEEP_INTERVAL_SECS";
const ENV_MCP_OAUTH_CORS_ORIGINS: &str = "RHOOD_MCP_OAUTH_CORS_ORIGINS";
const ENV_MCP_MAX_RESPONSE_BYTES: &str = "RHOOD_MCP_MAX_RESPONSE_BYTES";

const DEFAULT_MCP_HOST: &str = "127.0.0.1";
const DEFAULT_MCP_PORT: u16 = 8080;
/// Default ceiling on the first-call lazy-auth wait (seconds). Generous enough
/// for a mobile-approval device-verification round trip, but bounded so one
/// stuck login cannot hold the auth gate and wedge every other first call.
const DEFAULT_LAZY_AUTH_TIMEOUT_SECS: u64 = 180;
const DEFAULT_MCP_AUTH_MODE: &str = "token";
const DEFAULT_OAUTH_TOKEN_EXPIRY_SECS: u64 = 3600;
const DEFAULT_OAUTH_AUTH_CODE_TTL_SECS: u64 = 60;
const DEFAULT_OAUTH_CSRF_NONCE_TTL_SECS: u64 = 600;
const DEFAULT_OAUTH_SWEEP_INTERVAL_SECS: u64 = 300;
/// Default ceiling on a single tool response payload, in bytes (256 KiB).
const DEFAULT_MCP_MAX_RESPONSE_BYTES: usize = 256 * 1024;

/// CORS configuration for the MCP OAuth routes.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OAuthCorsConfig {
    /// Allowed origins for CORS preflight and simple requests.
    ///
    /// Use `["*"]` to allow all origins. Defaults to localhost variants.
    pub origins: Vec<String>,
}

impl Default for OAuthCorsConfig {
    fn default() -> Self {
        Self {
            origins: vec![
                "http://localhost".to_string(),
                "http://127.0.0.1".to_string(),
                "http://[::1]".to_string(),
            ],
        }
    }
}

/// Configuration for the MCP (Model Context Protocol) server binary.
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct McpConfig {
    /// Bind address for the HTTP transport.
    pub host: String,
    /// Bind port for the HTTP transport.
    pub port: u16,
    /// Optional public base URL for OAuth redirect URIs.
    pub base_url: Option<String>,
    /// Static bearer token for token-based auth. Mutually exclusive with `token_file`.
    pub token: Option<SecretString>,
    /// Ceiling on the first-call lazy-auth wait (stdio transport), in seconds.
    pub lazy_auth_timeout_secs: u64,
    /// Path to a file containing the bearer token (alternative to inline `token`).
    pub token_file: Option<String>,
    /// Authentication mode: `"token"` or `"oauth"`.
    pub auth_mode: String,
    /// PIN code for OAuth authorization. Mutually exclusive with `oauth_pin_file`.
    pub oauth_pin: Option<SecretString>,
    /// Path to a file containing the OAuth PIN (alternative to inline `oauth_pin`).
    pub oauth_pin_file: Option<String>,
    /// OAuth access token lifetime in seconds.
    pub oauth_token_expiry_secs: u64,
    /// TTL for OAuth authorization codes in seconds.
    pub oauth_auth_code_ttl_secs: u64,
    /// TTL for OAuth CSRF nonces in seconds.
    pub oauth_csrf_nonce_ttl_secs: u64,
    /// Interval between sweeps of expired OAuth entries in seconds.
    pub oauth_sweep_interval_secs: u64,
    /// CORS configuration for OAuth routes.
    #[serde(default)]
    pub oauth_cors: OAuthCorsConfig,
    /// Ceiling on a single tool response payload, in bytes. Oversized
    /// responses are replaced with a bounded JSON error. Very small values (a
    /// few KiB, or 0) make every response overflow into that bounded error.
    pub max_response_bytes: usize,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            host: DEFAULT_MCP_HOST.to_string(),
            port: DEFAULT_MCP_PORT,
            base_url: None,
            token: None,
            lazy_auth_timeout_secs: DEFAULT_LAZY_AUTH_TIMEOUT_SECS,
            token_file: None,
            auth_mode: DEFAULT_MCP_AUTH_MODE.to_string(),
            oauth_pin: None,
            oauth_pin_file: None,
            oauth_token_expiry_secs: DEFAULT_OAUTH_TOKEN_EXPIRY_SECS,
            oauth_auth_code_ttl_secs: DEFAULT_OAUTH_AUTH_CODE_TTL_SECS,
            oauth_csrf_nonce_ttl_secs: DEFAULT_OAUTH_CSRF_NONCE_TTL_SECS,
            oauth_sweep_interval_secs: DEFAULT_OAUTH_SWEEP_INTERVAL_SECS,
            oauth_cors: OAuthCorsConfig::default(),
            max_response_bytes: DEFAULT_MCP_MAX_RESPONSE_BYTES,
        }
    }
}

impl std::fmt::Debug for McpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("base_url", &self.base_url)
            .field("token", &self.token.as_ref().map(|_| "[REDACTED]"))
            .field("lazy_auth_timeout_secs", &self.lazy_auth_timeout_secs)
            .field("auth_mode", &self.auth_mode)
            .field("oauth_pin", &self.oauth_pin.as_ref().map(|_| "[REDACTED]"))
            .field("oauth_token_expiry_secs", &self.oauth_token_expiry_secs)
            .field("oauth_auth_code_ttl_secs", &self.oauth_auth_code_ttl_secs)
            .field("oauth_csrf_nonce_ttl_secs", &self.oauth_csrf_nonce_ttl_secs)
            .field("oauth_sweep_interval_secs", &self.oauth_sweep_interval_secs)
            .field("oauth_cors", &self.oauth_cors)
            .field("max_response_bytes", &self.max_response_bytes)
            .finish()
    }
}

/// Top-level MCP server configuration.
///
/// Combines the core Robinhood API client config with MCP-specific settings.
/// The `#[serde(flatten)]` attribute means the TOML format is flat — `[auth]`,
/// `[api]`, and `[mcp]` sections all live in the same file.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Core Robinhood API client configuration.
    #[serde(flatten)]
    pub core: rhood_core::RhoodConfig,
    /// MCP server settings.
    #[serde(default)]
    pub mcp: McpConfig,
}

impl ServerConfig {
    /// Loads server configuration from a TOML file, environment variables, and defaults.
    ///
    /// Delegates core config loading to [`RhoodConfig::load`](rhood_core::RhoodConfig::load),
    /// then applies MCP-specific environment overrides and secret resolution.
    ///
    /// Reads environment variables from the real process environment via
    /// [`SystemEnv`]. For test injection, see [`ServerConfig::load_with_env`].
    pub fn load(path: Option<&Path>) -> anyhow::Result<Self> {
        Self::load_with_env(path, &SystemEnv)
    }

    /// Like [`ServerConfig::load`], but reads env vars from the given [`Env`].
    ///
    /// Threads the injected env through both rhood-core's
    /// [`apply_env_overrides_and_normalize_with_env`](rhood_core::RhoodConfig::apply_env_overrides_and_normalize_with_env)
    /// and MCP-specific env overrides, so a test can control every env var the
    /// config loader consults.
    pub fn load_with_env(path: Option<&Path>, env: &impl Env) -> anyhow::Result<Self> {
        let explicit = path.is_some();
        let file_path = Self::resolve_path(path, env);

        let mut config: ServerConfig = match file_path {
            Some(ref path) if path.exists() => {
                let contents = std::fs::read_to_string(path)?;
                toml::from_str(&contents)
                    .map_err(|err| anyhow::anyhow!("Invalid config TOML: {err}"))?
            }
            Some(_) if explicit => {
                anyhow::bail!("Config file not found: {}", path.unwrap().display());
            }
            _ => ServerConfig::default(),
        };

        config
            .core
            .apply_env_overrides_and_normalize_with_env(env)?;
        config.apply_mcp_env_overrides(env);
        config.resolve_mcp_secrets()?;
        Ok(config)
    }

    fn resolve_path(explicit: Option<&Path>, env: &impl Env) -> Option<std::path::PathBuf> {
        if let Some(path) = explicit {
            return Some(path.to_path_buf());
        }
        if let Some(env_path) = env_non_empty(env, "RHOOD_CONFIG") {
            return Some(std::path::PathBuf::from(env_path));
        }
        if let Some(path) = dirs::config_dir().map(|dir| dir.join("rhood").join("config.toml"))
            && path.exists()
        {
            return Some(path);
        }
        if let Some(home) = dirs::home_dir() {
            let xdg = home.join(".config").join("rhood").join("config.toml");
            if xdg.exists() {
                return Some(xdg);
            }
        }
        dirs::config_dir().map(|dir| dir.join("rhood").join("config.toml"))
    }

    fn apply_mcp_env_overrides(&mut self, env: &impl Env) {
        if let Some(host) = env_non_empty(env, ENV_MCP_HOST) {
            self.mcp.host = host;
        }
        if let Some(port) = env_non_empty(env, ENV_MCP_PORT).and_then(|val| val.parse::<u16>().ok())
        {
            self.mcp.port = port;
        }
        if let Some(val) = env_non_empty(env, ENV_MCP_TOKEN) {
            self.mcp.token = Some(SecretString::from(val));
            self.mcp.token_file = None;
        }
        if let Some(val) = env_non_empty(env, ENV_MCP_TOKEN_FILE) {
            self.mcp.token_file = Some(val);
        }
        if let Some(val) = env_non_empty(env, ENV_MCP_BASE_URL) {
            self.mcp.base_url = Some(val);
        }
        if let Some(val) = env_non_empty(env, ENV_MCP_AUTH_MODE) {
            self.mcp.auth_mode = val;
        }
        if let Some(val) = env_non_empty(env, ENV_MCP_OAUTH_PIN) {
            self.mcp.oauth_pin = Some(SecretString::from(val));
            self.mcp.oauth_pin_file = None;
        }
        if let Some(val) = env_non_empty_u64(env, ENV_MCP_OAUTH_TOKEN_EXPIRY_SECS) {
            self.mcp.oauth_token_expiry_secs = val;
        }
        if let Some(val) = env_non_empty(env, ENV_MCP_OAUTH_PIN_FILE) {
            self.mcp.oauth_pin_file = Some(val);
        }
        if let Some(val) = env_non_empty_u64(env, ENV_MCP_OAUTH_AUTH_CODE_TTL_SECS) {
            self.mcp.oauth_auth_code_ttl_secs = val;
        }
        if let Some(val) = env_non_empty_u64(env, ENV_MCP_OAUTH_CSRF_NONCE_TTL_SECS) {
            self.mcp.oauth_csrf_nonce_ttl_secs = val;
        }
        if let Some(val) = env_non_empty_u64(env, ENV_MCP_OAUTH_SWEEP_INTERVAL_SECS) {
            self.mcp.oauth_sweep_interval_secs = val;
        }
        if let Some(origins_csv) = env_non_empty(env, ENV_MCP_OAUTH_CORS_ORIGINS) {
            self.mcp.oauth_cors.origins = origins_csv
                .split(',')
                .map(|origin| origin.trim().to_string())
                .filter(|origin| !origin.is_empty())
                .collect();
        }
        if let Some(val) =
            env_non_empty(env, ENV_MCP_MAX_RESPONSE_BYTES).and_then(|v| v.parse::<usize>().ok())
        {
            self.mcp.max_response_bytes = val;
        }
    }

    fn resolve_mcp_secrets(&mut self) -> anyhow::Result<()> {
        resolve_secret(&mut self.mcp.token, self.mcp.token_file.take(), "token")?;
        resolve_secret(
            &mut self.mcp.oauth_pin,
            self.mcp.oauth_pin_file.take(),
            "oauth_pin",
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    fn expose_secret_opt(secret: &Option<SecretString>) -> Option<&str> {
        secret.as_ref().map(|val| val.expose_secret())
    }

    #[test]
    fn default_mcp_config_has_expected_values() {
        let config = ServerConfig::default();
        assert_eq!(config.mcp.host, "127.0.0.1");
        assert_eq!(config.mcp.port, 8080);
        assert!(config.mcp.token.is_none());
        assert_eq!(config.mcp.auth_mode, "token");
        assert!(config.mcp.oauth_pin.is_none());
        assert_eq!(config.mcp.oauth_token_expiry_secs, 3600);
    }

    #[test]
    fn mcp_config_from_toml() {
        let toml_str = r#"
[mcp]
host = "0.0.0.0"
port = 9090
token = "my-secret-token"
"#;
        let config: ServerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.mcp.host, "0.0.0.0");
        assert_eq!(config.mcp.port, 9090);
        assert_eq!(
            expose_secret_opt(&config.mcp.token),
            Some("my-secret-token")
        );
    }

    #[test]
    fn server_config_flattens_core() {
        let toml_str = r#"
[auth]
username = "alice"

[mcp]
port = 3000
"#;
        let config: ServerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.core.auth.username.as_deref(), Some("alice"));
        assert_eq!(config.mcp.port, 3000);
    }

    #[test]
    fn debug_redacts_mcp_secrets() {
        let toml_str = r#"
[mcp]
token = "super-secret-token"
oauth_pin = "1234"
"#;
        let config: ServerConfig = toml::from_str(toml_str).unwrap();
        let debug_output = format!("{:?}", config.mcp);
        assert!(
            !debug_output.contains("super-secret-token"),
            "Debug should not contain token: {debug_output}"
        );
        assert!(
            !debug_output.contains("\"1234\""),
            "Debug should not contain oauth_pin: {debug_output}"
        );
    }

    #[test]
    fn default_oauth_cors_has_localhost_origins() {
        let config = ServerConfig::default();
        assert_eq!(config.mcp.oauth_cors.origins.len(), 3);
        assert!(
            config
                .mcp
                .oauth_cors
                .origins
                .contains(&"http://localhost".to_string())
        );
        assert!(
            config
                .mcp
                .oauth_cors
                .origins
                .contains(&"http://127.0.0.1".to_string())
        );
        assert!(
            config
                .mcp
                .oauth_cors
                .origins
                .contains(&"http://[::1]".to_string())
        );
    }

    #[test]
    fn oauth_cors_origins_from_toml() {
        let toml_str = r#"
[mcp.oauth_cors]
origins = ["https://app.example.com", "http://localhost:3000"]
"#;
        let config: ServerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.mcp.oauth_cors.origins,
            vec!["https://app.example.com", "http://localhost:3000"]
        );
    }

    #[test]
    fn oauth_cors_wildcard_from_toml() {
        let toml_str = r#"
[mcp.oauth_cors]
origins = ["*"]
"#;
        let config: ServerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.mcp.oauth_cors.origins, vec!["*"]);
    }

    #[test]
    fn default_max_response_bytes_is_256k() {
        let config = ServerConfig::default();
        assert_eq!(config.mcp.max_response_bytes, 256 * 1024);
    }

    #[test]
    fn max_response_bytes_from_toml() {
        let toml_str = r#"
[mcp]
max_response_bytes = 65536
"#;
        let config: ServerConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.mcp.max_response_bytes, 65536);
    }

    #[test]
    fn secret_fields_default_to_none() {
        let config = ServerConfig::default();
        assert!(config.mcp.token.is_none());
        assert!(config.mcp.oauth_pin.is_none());
    }
}
