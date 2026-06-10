//! Configuration for the rhood-core client.
//!
//! [`RhoodConfig`] is loaded from a TOML file, environment variables, and
//! built-in defaults. Environment variables override TOML values, and secrets
//! can be supplied via `*_file` fields that read from disk.

use std::path::Path;

use secrecy::SecretString;
use serde::Deserialize;

use crate::{Result, RhoodError};

use crate::env::{Env, SystemEnv, env_non_empty, env_non_empty_u64};

const DEFAULT_CLIENT_ID: &str = "c82SH0WZOsabOXGP2sxqcj34FxkvfnWRZBKlBjFS";
const DEFAULT_TOKEN_EXPIRY_SECS: u64 = 86400;
const DEFAULT_TOKEN_CACHE_PATH: &str = "~/.rhood/.rhood-token";
const DEFAULT_BASE_URL: &str = "https://api.robinhood.com";
const DEFAULT_PHOENIX_URL: &str = "https://phoenix.robinhood.com";
const DEFAULT_BONFIRE_URL: &str = "https://bonfire.robinhood.com";
const DEFAULT_DV_POLL_INTERVAL_SECS: u64 = 5;
const DEFAULT_DV_TIMEOUT_SECS: u64 = 120;
const DEFAULT_LOG_LEVEL: &str = "info";
const DEFAULT_HTTP_REQUEST_TIMEOUT_SECS: u64 = 30;
const DEFAULT_HTTP_CONNECT_TIMEOUT_SECS: u64 = 10;
const DEFAULT_CACHE_ENABLED: bool = true;
const DEFAULT_CACHE_INSTRUMENT_TTL_SECS: u64 = 86_400; // 24h
const DEFAULT_CACHE_INSTRUMENT_MAX_ENTRIES: u64 = 10_000;
const DEFAULT_CACHE_INSTRUMENT_ID_TTL_SECS: u64 = 86_400; // 24h
const DEFAULT_CACHE_INSTRUMENT_ID_MAX_ENTRIES: u64 = 50_000;
const DEFAULT_CACHE_INDEX_TTL_SECS: u64 = 604_800; // 7d
const DEFAULT_CACHE_INDEX_MAX_ENTRIES: u64 = 100;
const DEFAULT_CACHE_FUTURES_TTL_SECS: u64 = 3_600; // 1h
const DEFAULT_CACHE_FUTURES_MAX_ENTRIES: u64 = 500;
const DEFAULT_CACHE_ENRICHMENT_BATCH_SIZE: usize = 50;

const ENV_CONFIG: &str = "RHOOD_CONFIG";
const ENV_USERNAME: &str = "RHOOD_USERNAME";
const ENV_PASSWORD: &str = "RHOOD_PASSWORD";
const ENV_MFA: &str = "RHOOD_MFA";
const ENV_CLIENT_ID: &str = "RHOOD_CLIENT_ID";
const ENV_DEVICE_TOKEN: &str = "RHOOD_DEVICE_TOKEN";
const ENV_TOKEN_EXPIRY_SECS: &str = "RHOOD_TOKEN_EXPIRY_SECS";
const ENV_TOKEN_CACHE_PATH: &str = "RHOOD_TOKEN_CACHE_PATH";
const ENV_API_URL: &str = "RHOOD_API_URL";
const ENV_PHOENIX_URL: &str = "RHOOD_PHOENIX_URL";
const ENV_BONFIRE_URL: &str = "RHOOD_BONFIRE_URL";
const ENV_DV_POLL_INTERVAL_SECS: &str = "RHOOD_DV_POLL_INTERVAL_SECS";
const ENV_DV_TIMEOUT_SECS: &str = "RHOOD_DV_TIMEOUT_SECS";
const ENV_LOG_LEVEL: &str = "RHOOD_LOG_LEVEL";
const ENV_READ_ONLY: &str = "RHOOD_READ_ONLY";
const ENV_PASSWORD_FILE: &str = "RHOOD_PASSWORD_FILE";
const ENV_MFA_FILE: &str = "RHOOD_MFA_FILE";
const ENV_DEVICE_TOKEN_FILE: &str = "RHOOD_DEVICE_TOKEN_FILE";
const ENV_HTTP_REQUEST_TIMEOUT_SECS: &str = "RHOOD_HTTP_REQUEST_TIMEOUT_SECS";
const ENV_HTTP_CONNECT_TIMEOUT_SECS: &str = "RHOOD_HTTP_CONNECT_TIMEOUT_SECS";
const ENV_CACHE_ENABLED: &str = "RHOOD_CACHE_ENABLED";
const ENV_CACHE_INSTRUMENT_TTL_SECS: &str = "RHOOD_CACHE_INSTRUMENT_TTL_SECS";
const ENV_CACHE_INSTRUMENT_MAX_ENTRIES: &str = "RHOOD_CACHE_INSTRUMENT_MAX_ENTRIES";
const ENV_CACHE_INSTRUMENT_ID_TTL_SECS: &str = "RHOOD_CACHE_INSTRUMENT_ID_TTL_SECS";
const ENV_CACHE_INSTRUMENT_ID_MAX_ENTRIES: &str = "RHOOD_CACHE_INSTRUMENT_ID_MAX_ENTRIES";
const ENV_CACHE_INDEX_TTL_SECS: &str = "RHOOD_CACHE_INDEX_TTL_SECS";
const ENV_CACHE_INDEX_MAX_ENTRIES: &str = "RHOOD_CACHE_INDEX_MAX_ENTRIES";
const ENV_CACHE_FUTURES_TTL_SECS: &str = "RHOOD_CACHE_FUTURES_TTL_SECS";
const ENV_CACHE_FUTURES_MAX_ENTRIES: &str = "RHOOD_CACHE_FUTURES_MAX_ENTRIES";
const ENV_CACHE_ENRICHMENT_BATCH_SIZE: &str = "RHOOD_CACHE_ENRICHMENT_BATCH_SIZE";

/// Top-level configuration for the Robinhood API client.
///
/// Loaded via [`RhoodConfig::load`] from a TOML file, environment variables,
/// and built-in defaults. Defaults to read-only mode.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RhoodConfig {
    /// When `true`, order placement and cancellation are blocked.
    pub read_only: bool,
    /// Authentication credentials and token caching settings.
    pub auth: AuthConfig,
    /// API base URLs.
    pub api: ApiConfig,
    /// Device verification polling and timeout settings.
    pub device_verification: DeviceVerificationConfig,
    /// Logging configuration.
    pub log: LogConfig,
    /// HTTP transport timeouts shared by all outbound clients.
    pub http: HttpConfig,
    /// Identity/metadata resolver caches (symbol ↔ instrument id, etc.).
    pub cache: CacheConfig,
}

impl Default for RhoodConfig {
    fn default() -> Self {
        Self {
            read_only: true,
            auth: AuthConfig::default(),
            api: ApiConfig::default(),
            device_verification: DeviceVerificationConfig::default(),
            log: LogConfig::default(),
            http: HttpConfig::default(),
            cache: CacheConfig::default(),
        }
    }
}

/// Authentication credentials and token caching settings.
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    /// Robinhood account username (email or phone).
    pub username: Option<String>,
    /// Robinhood account password. Mutually exclusive with `password_file`.
    pub password: Option<SecretString>,
    /// Base32-encoded TOTP secret for MFA. Mutually exclusive with `mfa_secret_file`.
    pub mfa_secret: Option<SecretString>,
    /// OAuth2 client ID for the Robinhood API.
    pub client_id: String,
    /// Persistent device token. Mutually exclusive with `device_token_file`.
    pub device_token: Option<SecretString>,
    /// OAuth token lifetime in seconds.
    pub token_expiry_secs: u64,
    /// Filesystem path for the on-disk token cache (tilde-expanded).
    pub token_cache_path: String,
    /// Path to a file containing the password (alternative to inline `password`).
    pub password_file: Option<String>,
    /// Path to a file containing the MFA secret (alternative to inline `mfa_secret`).
    pub mfa_secret_file: Option<String>,
    /// Path to a file containing the device token (alternative to inline `device_token`).
    pub device_token_file: Option<String>,
}

/// Robinhood API base URLs.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ApiConfig {
    /// Base URL for the main Robinhood REST API.
    pub base_url: String,
    /// Base URL for the Phoenix unified accounts API.
    pub phoenix_url: String,
    /// Base URL for the Bonfire API (recurring investments, unified transfers).
    pub bonfire_url: String,
}

/// Settings for the device verification polling loop.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DeviceVerificationConfig {
    /// Seconds between poll attempts during device verification.
    pub poll_interval_secs: u64,
    /// Maximum seconds to wait for device approval before timing out.
    pub timeout_secs: u64,
}

/// Bounded timeouts for every outbound HTTP call.
///
/// `request_timeout_secs` is the total-call ceiling (headers + body). A slow
/// or hung upstream is aborted with a timeout error — preventing a single
/// stuck call from starving concurrent tool invocations. `connect_timeout_secs`
/// is the TCP-connect ceiling; it triggers before the request timeout when
/// the host is unreachable.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct HttpConfig {
    /// Total request timeout in seconds (headers + body).
    pub request_timeout_secs: u64,
    /// TCP connect timeout in seconds.
    pub connect_timeout_secs: u64,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            request_timeout_secs: DEFAULT_HTTP_REQUEST_TIMEOUT_SECS,
            connect_timeout_secs: DEFAULT_HTTP_CONNECT_TIMEOUT_SECS,
        }
    }
}

/// Identity/metadata caches for symbol ↔ ID resolution.
///
/// Financial data is **never** cached (quotes, prices, positions, orders,
/// etc.). Only immutable lookups: instrument metadata, index instruments,
/// futures contracts, and the singleton futures account ID.
///
/// Set `enabled = false` to bypass all caches and hit upstream on every call.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CacheConfig {
    /// Master switch. When `false`, every cached wrapper falls through to the
    /// underlying resolver and nothing is inserted into the caches.
    pub enabled: bool,
    /// Time-to-live for the symbol → instrument cache, in seconds.
    pub instrument_ttl_secs: u64,
    /// Maximum entries in the symbol → instrument cache.
    pub instrument_max_entries: u64,
    /// Time-to-live for the uuid → symbol reverse map, in seconds.
    pub instrument_id_ttl_secs: u64,
    /// Maximum entries in the uuid → symbol reverse map.
    pub instrument_id_max_entries: u64,
    /// Time-to-live for the symbol → index instrument cache, in seconds.
    pub index_ttl_secs: u64,
    /// Maximum entries in the symbol → index instrument cache.
    pub index_max_entries: u64,
    /// Time-to-live for the symbol → futures contract cache, in seconds.
    pub futures_ttl_secs: u64,
    /// Maximum entries in the symbol → futures contract cache.
    pub futures_max_entries: u64,
    /// Batch size for `resolve_symbols` chunked `?ids=` query-string requests.
    pub enrichment_batch_size: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: DEFAULT_CACHE_ENABLED,
            instrument_ttl_secs: DEFAULT_CACHE_INSTRUMENT_TTL_SECS,
            instrument_max_entries: DEFAULT_CACHE_INSTRUMENT_MAX_ENTRIES,
            instrument_id_ttl_secs: DEFAULT_CACHE_INSTRUMENT_ID_TTL_SECS,
            instrument_id_max_entries: DEFAULT_CACHE_INSTRUMENT_ID_MAX_ENTRIES,
            index_ttl_secs: DEFAULT_CACHE_INDEX_TTL_SECS,
            index_max_entries: DEFAULT_CACHE_INDEX_MAX_ENTRIES,
            futures_ttl_secs: DEFAULT_CACHE_FUTURES_TTL_SECS,
            futures_max_entries: DEFAULT_CACHE_FUTURES_MAX_ENTRIES,
            enrichment_batch_size: DEFAULT_CACHE_ENRICHMENT_BATCH_SIZE,
        }
    }
}

/// Logging configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    /// Tracing filter level (e.g., `"info"`, `"debug"`, `"trace"`).
    pub level: String,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            username: None,
            password: None,
            mfa_secret: None,
            client_id: DEFAULT_CLIENT_ID.to_string(),
            device_token: None,
            token_expiry_secs: DEFAULT_TOKEN_EXPIRY_SECS,
            token_cache_path: DEFAULT_TOKEN_CACHE_PATH.to_string(),
            password_file: None,
            mfa_secret_file: None,
            device_token_file: None,
        }
    }
}

impl std::fmt::Debug for AuthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthConfig")
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "[REDACTED]"))
            .field(
                "mfa_secret",
                &self.mfa_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field("client_id", &self.client_id)
            .field(
                "device_token",
                &self.device_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("token_expiry_secs", &self.token_expiry_secs)
            .field("token_cache_path", &self.token_cache_path)
            .finish()
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            phoenix_url: DEFAULT_PHOENIX_URL.to_string(),
            bonfire_url: DEFAULT_BONFIRE_URL.to_string(),
        }
    }
}

impl Default for DeviceVerificationConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: DEFAULT_DV_POLL_INTERVAL_SECS,
            timeout_secs: DEFAULT_DV_TIMEOUT_SECS,
        }
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: DEFAULT_LOG_LEVEL.to_string(),
        }
    }
}

fn read_secret_file(path: &str, field_name: &str) -> Result<String> {
    let p = Path::new(path);
    if !p.exists() {
        return Err(RhoodError::InvalidParameter(format!(
            "Secret file not found: {path} (from {field_name})"
        )));
    }
    std::fs::read_to_string(p)
        .map(|contents| contents.trim().to_string())
        .map_err(|e| {
            RhoodError::InvalidParameter(format!("Failed to read secret file {path}: {e}"))
        })
}

/// Resolves a secret value from either a direct value or a file path.
///
/// If both `direct` and `file_path` are set, returns an error. If only `file_path`
/// is set, reads the file, trims whitespace, and stores the result in `direct`.
pub fn resolve_secret(
    direct: &mut Option<SecretString>,
    file_path: Option<String>,
    field_name: &str,
) -> Result<()> {
    let file_field = format!("{field_name}_file");
    match (direct.is_some(), file_path) {
        (true, Some(_)) => Err(RhoodError::InvalidParameter(format!(
            "Conflicting config: both '{field_name}' and '{file_field}' are set. Use one or the other."
        ))),
        (false, Some(path)) => {
            *direct = Some(SecretString::from(read_secret_file(&path, &file_field)?));
            Ok(())
        }
        _ => Ok(()),
    }
}

fn expand_tilde(path: &str) -> String {
    if !path.starts_with('~') {
        return path.to_string();
    }
    let home = dirs::home_dir().map_or_else(
        || "~".to_string(),
        |home| home.to_string_lossy().to_string(),
    );
    if path == "~" {
        home
    } else {
        // path starts with "~/" or "~something" — only expand "~/"
        if let Some(rest) = path.strip_prefix("~/") {
            format!("{home}/{rest}")
        } else {
            path.to_string()
        }
    }
}

fn strip_trailing_slash(url: &mut String) {
    while url.ends_with('/') {
        url.pop();
    }
}

impl RhoodConfig {
    /// Loads configuration from a TOML file, environment variables, and defaults.
    ///
    /// Resolution order:
    /// 1. If `path` is `Some`, use that file (error if it does not exist).
    /// 2. Otherwise check `RHOOD_CONFIG`, platform config dir, and XDG paths.
    /// 3. If no file is found, start with defaults.
    /// 4. Apply environment variable overrides.
    /// 5. Resolve `*_file` secrets and normalize paths/URLs.
    ///
    /// Reads environment variables from the real process environment via
    /// [`SystemEnv`]. For test injection, see [`RhoodConfig::load_with_env`].
    ///
    /// # Errors
    ///
    /// Returns an error if an explicitly provided file does not exist, the TOML
    /// is invalid, or secret file resolution fails.
    pub fn load(path: Option<&Path>) -> Result<Self> {
        Self::load_with_env(path, &SystemEnv)
    }

    /// Like [`RhoodConfig::load`], but reads env vars from the given [`Env`].
    ///
    /// Intended for tests: pass a [`MapEnv`](crate::env::MapEnv) preloaded with the env values the
    /// test cares about. Production code should call [`RhoodConfig::load`].
    ///
    /// # Errors
    ///
    /// Returns an error if an explicitly provided file does not exist, the TOML
    /// is invalid, or secret file resolution fails.
    pub fn load_with_env(path: Option<&Path>, env: &impl Env) -> Result<Self> {
        let explicit = path.is_some();
        let file_path = Self::resolve_path(path, env);

        let mut config = match file_path {
            Some(ref p) if p.exists() => {
                let contents = std::fs::read_to_string(p)?;
                toml::from_str::<RhoodConfig>(&contents).map_err(|error| {
                    RhoodError::InvalidParameter(format!("Invalid config TOML: {error}"))
                })?
            }
            Some(_) if explicit => {
                return Err(RhoodError::InvalidParameter(format!(
                    "Config file not found: {}",
                    path.unwrap().display()
                )));
            }
            _ => RhoodConfig::default(),
        };

        config.apply_env_overrides(env);
        config.resolve_secret_files()?;
        config.normalize();
        Ok(config)
    }

    /// Parses configuration from a TOML string, then applies environment
    /// overrides and secret file resolution.
    ///
    /// Reads env vars from [`SystemEnv`]. For test injection, see
    /// [`RhoodConfig::from_toml_with_env`].
    ///
    /// # Errors
    ///
    /// Returns an error if the TOML is invalid or secret file resolution fails.
    pub fn from_toml(toml_str: &str) -> Result<Self> {
        Self::from_toml_with_env(toml_str, &SystemEnv)
    }

    /// Like [`RhoodConfig::from_toml`], but reads env vars from the given [`Env`].
    ///
    /// # Errors
    ///
    /// Returns an error if the TOML is invalid or secret file resolution fails.
    pub fn from_toml_with_env(toml_str: &str, env: &impl Env) -> Result<Self> {
        let mut config: RhoodConfig = toml::from_str(toml_str).map_err(|error| {
            RhoodError::InvalidParameter(format!("Invalid config TOML: {error}"))
        })?;
        config.apply_env_overrides(env);
        config.resolve_secret_files()?;
        config.normalize();
        Ok(config)
    }

    fn resolve_path(explicit: Option<&Path>, env: &impl Env) -> Option<std::path::PathBuf> {
        if let Some(p) = explicit {
            return Some(p.to_path_buf());
        }
        if let Some(env_path) = env_non_empty(env, ENV_CONFIG) {
            return Some(std::path::PathBuf::from(env_path));
        }
        // Check platform config dir first (e.g. ~/Library/Application Support on macOS),
        // then fall back to XDG-style ~/.config for cross-platform consistency.
        if let Some(platform_path) =
            dirs::config_dir().map(|dir| dir.join("rhood").join("config.toml"))
            && platform_path.exists()
        {
            return Some(platform_path);
        }
        if let Some(home) = dirs::home_dir() {
            let xdg = home.join(".config").join("rhood").join("config.toml");
            if xdg.exists() {
                return Some(xdg);
            }
        }
        dirs::config_dir().map(|dir| dir.join("rhood").join("config.toml"))
    }

    /// Applies environment variable overrides, resolves secret files, and
    /// normalizes paths/URLs, reading env from [`SystemEnv`].
    ///
    /// Called automatically by [`load`](Self::load) and [`from_toml`](Self::from_toml).
    /// Also useful when a downstream crate (e.g. `rhood-mcp`) deserializes its own
    /// config struct that embeds `RhoodConfig` via `#[serde(flatten)]`.
    ///
    /// For test injection, see
    /// [`apply_env_overrides_and_normalize_with_env`](Self::apply_env_overrides_and_normalize_with_env).
    ///
    /// # Errors
    ///
    /// Returns an error if secret file resolution fails.
    pub fn apply_env_overrides_and_normalize(&mut self) -> Result<()> {
        self.apply_env_overrides_and_normalize_with_env(&SystemEnv)
    }

    /// Like [`apply_env_overrides_and_normalize`](Self::apply_env_overrides_and_normalize),
    /// but reads env vars from the given [`Env`].
    ///
    /// # Errors
    ///
    /// Returns an error if secret file resolution fails.
    pub fn apply_env_overrides_and_normalize_with_env(&mut self, env: &impl Env) -> Result<()> {
        self.apply_env_overrides(env);
        self.resolve_secret_files()?;
        self.normalize();
        Ok(())
    }

    fn apply_env_overrides(&mut self, env: &impl Env) {
        if let Some(v) = env_non_empty(env, ENV_USERNAME) {
            self.auth.username = Some(v);
        }
        if let Some(val) = env_non_empty(env, ENV_PASSWORD) {
            self.auth.password = Some(SecretString::from(val));
            self.auth.password_file = None;
        }
        if let Some(val) = env_non_empty(env, ENV_MFA) {
            self.auth.mfa_secret = Some(SecretString::from(val));
            self.auth.mfa_secret_file = None;
        }
        if let Some(v) = env_non_empty(env, ENV_CLIENT_ID) {
            self.auth.client_id = v;
        }
        if let Some(val) = env_non_empty(env, ENV_DEVICE_TOKEN) {
            self.auth.device_token = Some(SecretString::from(val));
            self.auth.device_token_file = None;
        }
        if let Some(v) = env_non_empty_u64(env, ENV_TOKEN_EXPIRY_SECS) {
            self.auth.token_expiry_secs = v;
        }
        if let Some(v) = env_non_empty(env, ENV_TOKEN_CACHE_PATH) {
            self.auth.token_cache_path = v;
        }
        if let Some(v) = env_non_empty(env, ENV_API_URL) {
            self.api.base_url = v;
        }
        if let Some(v) = env_non_empty(env, ENV_PHOENIX_URL) {
            self.api.phoenix_url = v;
        }
        if let Some(v) = env_non_empty(env, ENV_BONFIRE_URL) {
            self.api.bonfire_url = v;
        }
        if let Some(v) = env_non_empty_u64(env, ENV_DV_POLL_INTERVAL_SECS) {
            self.device_verification.poll_interval_secs = v;
        }
        if let Some(v) = env_non_empty_u64(env, ENV_DV_TIMEOUT_SECS) {
            self.device_verification.timeout_secs = v;
        }
        if let Some(v) = env_non_empty_u64(env, ENV_HTTP_REQUEST_TIMEOUT_SECS) {
            self.http.request_timeout_secs = v;
        }
        if let Some(v) = env_non_empty_u64(env, ENV_HTTP_CONNECT_TIMEOUT_SECS) {
            self.http.connect_timeout_secs = v;
        }
        if let Some(v) = env_non_empty(env, ENV_CACHE_ENABLED) {
            self.cache.enabled = v.eq_ignore_ascii_case("true") || v == "1";
        }
        if let Some(v) = env_non_empty_u64(env, ENV_CACHE_INSTRUMENT_TTL_SECS) {
            self.cache.instrument_ttl_secs = v;
        }
        if let Some(v) = env_non_empty_u64(env, ENV_CACHE_INSTRUMENT_MAX_ENTRIES) {
            self.cache.instrument_max_entries = v;
        }
        if let Some(v) = env_non_empty_u64(env, ENV_CACHE_INSTRUMENT_ID_TTL_SECS) {
            self.cache.instrument_id_ttl_secs = v;
        }
        if let Some(v) = env_non_empty_u64(env, ENV_CACHE_INSTRUMENT_ID_MAX_ENTRIES) {
            self.cache.instrument_id_max_entries = v;
        }
        if let Some(v) = env_non_empty_u64(env, ENV_CACHE_INDEX_TTL_SECS) {
            self.cache.index_ttl_secs = v;
        }
        if let Some(v) = env_non_empty_u64(env, ENV_CACHE_INDEX_MAX_ENTRIES) {
            self.cache.index_max_entries = v;
        }
        if let Some(v) = env_non_empty_u64(env, ENV_CACHE_FUTURES_TTL_SECS) {
            self.cache.futures_ttl_secs = v;
        }
        if let Some(v) = env_non_empty_u64(env, ENV_CACHE_FUTURES_MAX_ENTRIES) {
            self.cache.futures_max_entries = v;
        }
        if let Some(v) = env_non_empty(env, ENV_CACHE_ENRICHMENT_BATCH_SIZE) {
            match v.parse::<usize>() {
                Ok(parsed) if parsed > 0 => self.cache.enrichment_batch_size = parsed,
                _ => tracing::warn!(
                    value = %v,
                    "invalid RHOOD_CACHE_ENRICHMENT_BATCH_SIZE, must be a positive integer; ignoring"
                ),
            }
        }
        if let Some(v) = env_non_empty(env, ENV_LOG_LEVEL) {
            self.log.level = v;
        }
        if let Some(v) = env_non_empty(env, ENV_READ_ONLY) {
            self.read_only = v.eq_ignore_ascii_case("true") || v == "1";
        }
        if let Some(v) = env_non_empty(env, ENV_PASSWORD_FILE) {
            self.auth.password_file = Some(v);
        }
        if let Some(v) = env_non_empty(env, ENV_MFA_FILE) {
            self.auth.mfa_secret_file = Some(v);
        }
        if let Some(v) = env_non_empty(env, ENV_DEVICE_TOKEN_FILE) {
            self.auth.device_token_file = Some(v);
        }
    }

    fn resolve_secret_files(&mut self) -> Result<()> {
        resolve_secret(
            &mut self.auth.password,
            self.auth.password_file.take(),
            "password",
        )?;
        resolve_secret(
            &mut self.auth.mfa_secret,
            self.auth.mfa_secret_file.take(),
            "mfa_secret",
        )?;
        resolve_secret(
            &mut self.auth.device_token,
            self.auth.device_token_file.take(),
            "device_token",
        )?;
        Ok(())
    }

    /// Restores invariants after in-place mutation of configuration fields.
    ///
    /// Expands a leading `~` in [`auth.token_cache_path`](AuthConfig::token_cache_path)
    /// and strips trailing slashes from API base URLs.
    ///
    /// Called automatically by [`load`](Self::load), [`from_toml`](Self::from_toml),
    /// and [`apply_env_overrides_and_normalize`](Self::apply_env_overrides_and_normalize).
    /// Callers that mutate fields directly (e.g. applying CLI flag overrides after
    /// load) should call this to keep the resulting config consistent.
    pub fn normalize(&mut self) {
        self.auth.token_cache_path = expand_tilde(&self.auth.token_cache_path);
        strip_trailing_slash(&mut self.api.base_url);
        strip_trailing_slash(&mut self.api.phoenix_url);
        strip_trailing_slash(&mut self.api.bonfire_url);
    }
}

#[cfg(test)]
mod tests {
    use crate::env::MapEnv;

    use super::*;
    use secrecy::ExposeSecret;
    use std::io::Write as _;

    /// Expose an `Option<SecretString>` as `Option<&str>` for test assertions.
    fn expose_secret_opt(secret: &Option<SecretString>) -> Option<&str> {
        secret.as_ref().map(|val| val.expose_secret())
    }

    #[test]
    fn system_env_reads_process_env() {
        // Smoke test only: setting a concrete process env var here would race
        // with parallel tests reading the same key. We assert the absence of
        // a key that is unlikely to be set in any CI environment instead.
        let env = SystemEnv;
        let value = env.get("RHOOD_UNSET_KEY_FOR_SYSTEM_ENV_SMOKE_TEST");
        assert!(
            value.is_none(),
            "expected unset key to return None, got {value:?}"
        );
    }

    #[test]
    fn map_env_get_returns_inserted_values() {
        let env = MapEnv::new()
            .with("RHOOD_USERNAME", "alice")
            .with("RHOOD_API_URL", "https://example.com");
        assert_eq!(env.get("RHOOD_USERNAME").as_deref(), Some("alice"));
        assert_eq!(
            env.get("RHOOD_API_URL").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(env.get("RHOOD_MISSING"), None);
    }

    #[test]
    fn map_env_preserves_empty_strings() {
        // Matches SystemEnv / std::env::var semantics: an empty value is not
        // the same as an unset key.
        let env = MapEnv::new().with("RHOOD_EMPTY", "");
        assert_eq!(env.get("RHOOD_EMPTY").as_deref(), Some(""));
    }

    #[test]
    fn map_env_default_is_empty() {
        let env = MapEnv::default();
        assert_eq!(env.get("ANY_KEY"), None);
    }

    #[test]
    fn default_config_has_expected_values() {
        let cfg = RhoodConfig::default();
        assert_eq!(cfg.auth.client_id, DEFAULT_CLIENT_ID);
        assert_eq!(cfg.auth.token_expiry_secs, DEFAULT_TOKEN_EXPIRY_SECS);
        assert_eq!(cfg.auth.token_cache_path, DEFAULT_TOKEN_CACHE_PATH);
        assert!(cfg.auth.username.is_none());
        assert!(cfg.auth.password.is_none());
        assert!(cfg.auth.mfa_secret.is_none());
        assert!(cfg.auth.device_token.is_none());
        assert_eq!(cfg.api.base_url, DEFAULT_BASE_URL);
        assert_eq!(cfg.api.phoenix_url, DEFAULT_PHOENIX_URL);
        assert_eq!(cfg.api.bonfire_url, DEFAULT_BONFIRE_URL);
        assert_eq!(
            cfg.device_verification.poll_interval_secs,
            DEFAULT_DV_POLL_INTERVAL_SECS
        );
        assert_eq!(
            cfg.device_verification.timeout_secs,
            DEFAULT_DV_TIMEOUT_SECS
        );
        assert_eq!(cfg.log.level, DEFAULT_LOG_LEVEL);
    }

    #[test]
    fn load_from_toml_string() {
        let toml = r#"
[auth]
username = "alice"
password = "secret123"
mfa_secret = "JBSWY3DPEHPK3PXP"
client_id = "custom-client-id"
device_token = "dev-tok-123"
token_expiry_secs = 3600
token_cache_path = "/tmp/token"

[api]
base_url = "https://custom.api.com"
phoenix_url = "https://custom.phoenix.com"

[device_verification]
poll_interval_secs = 10
timeout_secs = 300

[log]
level = "debug"
"#;
        let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
        assert_eq!(cfg.auth.username.as_deref(), Some("alice"));
        assert_eq!(expose_secret_opt(&cfg.auth.password), Some("secret123"));
        assert_eq!(
            expose_secret_opt(&cfg.auth.mfa_secret),
            Some("JBSWY3DPEHPK3PXP")
        );
        assert_eq!(cfg.auth.client_id, "custom-client-id");
        assert_eq!(
            expose_secret_opt(&cfg.auth.device_token),
            Some("dev-tok-123")
        );
        assert_eq!(cfg.auth.token_expiry_secs, 3600);
        assert_eq!(cfg.auth.token_cache_path, "/tmp/token");
        assert_eq!(cfg.api.base_url, "https://custom.api.com");
        assert_eq!(cfg.api.phoenix_url, "https://custom.phoenix.com");
        assert_eq!(cfg.device_verification.poll_interval_secs, 10);
        assert_eq!(cfg.device_verification.timeout_secs, 300);
        assert_eq!(cfg.log.level, "debug");
    }

    #[test]
    fn load_partial_toml_fills_defaults() {
        let toml = r#"
[auth]
username = "bob"
"#;
        let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
        assert_eq!(cfg.auth.username.as_deref(), Some("bob"));
        assert_eq!(cfg.auth.client_id, DEFAULT_CLIENT_ID);
        assert_eq!(cfg.auth.token_expiry_secs, DEFAULT_TOKEN_EXPIRY_SECS);
        assert_eq!(cfg.api.base_url, DEFAULT_BASE_URL);
        assert_eq!(
            cfg.device_verification.timeout_secs,
            DEFAULT_DV_TIMEOUT_SECS
        );
        assert_eq!(cfg.log.level, DEFAULT_LOG_LEVEL);
    }

    #[test]
    fn missing_default_file_returns_defaults() {
        // With no RHOOD_CONFIG env and no file at the default path, load(None) returns defaults.
        let cfg = RhoodConfig::load_with_env(None, &MapEnv::new()).unwrap();
        assert_eq!(cfg.auth.client_id, DEFAULT_CLIENT_ID);
        assert_eq!(cfg.api.base_url, DEFAULT_BASE_URL);
    }

    #[test]
    fn explicit_missing_file_is_error() {
        let result = RhoodConfig::load_with_env(
            Some(Path::new("/tmp/nonexistent-rhood-config.toml")),
            &MapEnv::new(),
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Config file not found"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn load_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("config.toml");
        {
            let mut f = std::fs::File::create(&file_path).unwrap();
            writeln!(
                f,
                r#"
[auth]
username = "charlie"
token_expiry_secs = 7200

[log]
level = "warn"
"#
            )
            .unwrap();
        }
        let cfg = RhoodConfig::load_with_env(Some(&file_path), &MapEnv::new()).unwrap();
        assert_eq!(cfg.auth.username.as_deref(), Some("charlie"));
        assert_eq!(cfg.auth.token_expiry_secs, 7200);
        assert_eq!(cfg.auth.client_id, DEFAULT_CLIENT_ID);
        assert_eq!(cfg.log.level, "warn");
    }

    #[test]
    fn env_vars_override_toml() {
        let env = MapEnv::new()
            .with(ENV_USERNAME, "env-user")
            .with(ENV_API_URL, "https://env.api.com")
            .with(ENV_LOG_LEVEL, "trace");
        let toml = r#"
[auth]
username = "toml-user"

[api]
base_url = "https://toml.api.com"

[log]
level = "debug"
"#;
        let cfg = RhoodConfig::from_toml_with_env(toml, &env).unwrap();
        assert_eq!(cfg.auth.username.as_deref(), Some("env-user"));
        assert_eq!(cfg.api.base_url, "https://env.api.com");
        assert_eq!(cfg.log.level, "trace");
    }

    #[test]
    fn empty_env_var_does_not_override() {
        let env = MapEnv::new().with(ENV_USERNAME, "");
        let toml = r#"
[auth]
username = "toml-user"
"#;
        let cfg = RhoodConfig::from_toml_with_env(toml, &env).unwrap();
        assert_eq!(cfg.auth.username.as_deref(), Some("toml-user"));
    }

    #[test]
    fn tilde_expansion_in_token_cache_path() {
        let toml = r#"
[auth]
token_cache_path = "~/.rhood-token"
"#;
        let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
        assert!(
            !cfg.auth.token_cache_path.starts_with('~'),
            "tilde should be expanded, got: {}",
            cfg.auth.token_cache_path
        );
        let home = dirs::home_dir().unwrap().to_string_lossy().to_string();
        assert_eq!(cfg.auth.token_cache_path, format!("{home}/.rhood-token"));
    }

    #[test]
    fn absolute_path_unchanged() {
        let toml = r#"
[auth]
token_cache_path = "/tmp/my-token"
"#;
        let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
        assert_eq!(cfg.auth.token_cache_path, "/tmp/my-token");
    }

    #[test]
    fn base_url_trailing_slash_normalized() {
        let toml = r#"
[api]
base_url = "https://api.robinhood.com/"
phoenix_url = "https://phoenix.robinhood.com/"
"#;
        let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
        assert_eq!(cfg.api.base_url, "https://api.robinhood.com");
        assert_eq!(cfg.api.phoenix_url, "https://phoenix.robinhood.com");
    }

    #[test]
    fn default_config_read_only_is_true() {
        let cfg = RhoodConfig::default();
        assert!(cfg.read_only);
    }

    #[test]
    fn read_only_from_toml() {
        let toml = "read_only = true\n";
        let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
        assert!(cfg.read_only);
    }

    #[test]
    fn read_only_env_override() {
        let env = MapEnv::new().with("RHOOD_READ_ONLY", "true");
        let cfg = RhoodConfig::from_toml_with_env("read_only = false\n", &env).unwrap();
        assert!(cfg.read_only);
    }

    #[test]
    fn read_only_env_override_false_enables_writes() {
        let env = MapEnv::new().with("RHOOD_READ_ONLY", "false");
        let cfg = RhoodConfig::from_toml_with_env("", &env).unwrap();
        assert!(!cfg.read_only);
    }

    #[test]
    fn secret_file_reads_and_trims() {
        let dir = tempfile::tempdir().unwrap();
        let secret_path = dir.path().join("password.txt");
        std::fs::write(&secret_path, "  hunter2\n  ").unwrap();

        let toml = format!(
            r#"
[auth]
password_file = "{}"
"#,
            secret_path.display()
        );
        let cfg = RhoodConfig::from_toml_with_env(&toml, &MapEnv::new()).unwrap();
        assert_eq!(expose_secret_opt(&cfg.auth.password), Some("hunter2"));
        assert!(
            cfg.auth.password_file.is_none(),
            "password_file should be consumed"
        );
    }

    #[test]
    fn secret_file_conflict_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let secret_path = dir.path().join("password.txt");
        std::fs::write(&secret_path, "from-file").unwrap();

        let toml = format!(
            r#"
[auth]
password = "inline-value"
password_file = "{}"
"#,
            secret_path.display()
        );
        let err = RhoodConfig::from_toml_with_env(&toml, &MapEnv::new()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Conflicting")
                && msg.contains("password")
                && msg.contains("password_file"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn secret_file_env_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let secret_path = dir.path().join("password.txt");
        std::fs::write(&secret_path, "from-file").unwrap();

        let env = MapEnv::new()
            .with(ENV_PASSWORD, "from-env")
            .with(ENV_PASSWORD_FILE, secret_path.to_str().unwrap());
        let err = RhoodConfig::from_toml_with_env("", &env).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Conflicting"), "unexpected error: {msg}");
    }

    #[test]
    fn secret_file_missing_is_error() {
        let toml = r#"
[auth]
password_file = "/tmp/nonexistent-rhood-secret-file-12345"
"#;
        let err = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Secret file not found") && msg.contains("password_file"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn secret_file_env_overrides_toml_file() {
        let dir = tempfile::tempdir().unwrap();
        let secret_path = dir.path().join("password.txt");
        std::fs::write(&secret_path, "from-file").unwrap();

        let env = MapEnv::new().with(ENV_PASSWORD, "from-env");
        let toml = format!(
            r#"
[auth]
password_file = "{}"
"#,
            secret_path.display()
        );
        let cfg = RhoodConfig::from_toml_with_env(&toml, &env).unwrap();
        assert_eq!(expose_secret_opt(&cfg.auth.password), Some("from-env"));
    }

    #[test]
    fn default_config_has_bonfire_url() {
        let cfg = RhoodConfig::default();
        assert_eq!(cfg.api.bonfire_url, "https://bonfire.robinhood.com");
    }

    #[test]
    fn bonfire_url_from_toml() {
        let toml = r#"
[api]
bonfire_url = "https://custom.bonfire.com"
"#;
        let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
        assert_eq!(cfg.api.bonfire_url, "https://custom.bonfire.com");
    }

    #[test]
    fn bonfire_url_env_override() {
        let env = MapEnv::new().with("RHOOD_BONFIRE_URL", "https://env.bonfire.com");
        let cfg = RhoodConfig::from_toml_with_env("", &env).unwrap();
        assert_eq!(cfg.api.bonfire_url, "https://env.bonfire.com");
    }

    #[test]
    fn bonfire_url_trailing_slash_normalized() {
        let toml = r#"
[api]
bonfire_url = "https://bonfire.robinhood.com/"
"#;
        let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
        assert_eq!(cfg.api.bonfire_url, "https://bonfire.robinhood.com");
    }

    #[test]
    fn debug_redacts_auth_secrets() {
        let toml = r#"
[auth]
username = "alice"
password = "hunter2"
mfa_secret = "JBSWY3DPEHPK3PXP"
device_token = "dev-tok-123"
"#;
        let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
        let debug_output = format!("{:?}", cfg.auth);
        assert!(
            debug_output.contains("[REDACTED]"),
            "Debug should contain [REDACTED]: {debug_output}"
        );
        assert!(
            !debug_output.contains("hunter2"),
            "Debug should not contain password: {debug_output}"
        );
        assert!(
            !debug_output.contains("JBSWY3DPEHPK3PXP"),
            "Debug should not contain mfa_secret: {debug_output}"
        );
        assert!(
            !debug_output.contains("dev-tok-123"),
            "Debug should not contain device_token: {debug_output}"
        );
        assert!(
            debug_output.contains("alice"),
            "Debug should show non-secret username: {debug_output}"
        );
    }

    #[test]
    fn secret_fields_deserialize_from_toml() {
        let toml = r#"
[auth]
password = "p@ssword!"
mfa_secret = "TOTP_SECRET"
device_token = "device-123"
"#;
        let cfg = RhoodConfig::from_toml_with_env(toml, &MapEnv::new()).unwrap();
        assert_eq!(expose_secret_opt(&cfg.auth.password), Some("p@ssword!"));
        assert_eq!(expose_secret_opt(&cfg.auth.mfa_secret), Some("TOTP_SECRET"));
        assert_eq!(
            expose_secret_opt(&cfg.auth.device_token),
            Some("device-123")
        );
    }

    #[test]
    fn secret_fields_default_to_none() {
        let cfg = RhoodConfig::default();
        assert!(cfg.auth.password.is_none());
        assert!(cfg.auth.mfa_secret.is_none());
        assert!(cfg.auth.device_token.is_none());
    }

    #[test]
    fn http_config_defaults() {
        let cfg = HttpConfig::default();
        assert_eq!(cfg.request_timeout_secs, DEFAULT_HTTP_REQUEST_TIMEOUT_SECS);
        assert_eq!(cfg.connect_timeout_secs, DEFAULT_HTTP_CONNECT_TIMEOUT_SECS);
    }

    #[test]
    fn http_env_overrides_applied() {
        let env = MapEnv::new()
            .with(ENV_HTTP_REQUEST_TIMEOUT_SECS, "45")
            .with(ENV_HTTP_CONNECT_TIMEOUT_SECS, "5");
        let mut cfg = RhoodConfig::default();
        cfg.apply_env_overrides(&env);
        assert_eq!(cfg.http.request_timeout_secs, 45);
        assert_eq!(cfg.http.connect_timeout_secs, 5);
    }

    #[test]
    fn cache_config_defaults() {
        let cfg = CacheConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.instrument_ttl_secs, DEFAULT_CACHE_INSTRUMENT_TTL_SECS);
        assert_eq!(
            cfg.instrument_max_entries,
            DEFAULT_CACHE_INSTRUMENT_MAX_ENTRIES
        );
        assert_eq!(
            cfg.instrument_id_ttl_secs,
            DEFAULT_CACHE_INSTRUMENT_ID_TTL_SECS
        );
        assert_eq!(
            cfg.instrument_id_max_entries,
            DEFAULT_CACHE_INSTRUMENT_ID_MAX_ENTRIES
        );
        assert_eq!(cfg.index_ttl_secs, DEFAULT_CACHE_INDEX_TTL_SECS);
        assert_eq!(cfg.index_max_entries, DEFAULT_CACHE_INDEX_MAX_ENTRIES);
        assert_eq!(cfg.futures_ttl_secs, DEFAULT_CACHE_FUTURES_TTL_SECS);
        assert_eq!(cfg.futures_max_entries, DEFAULT_CACHE_FUTURES_MAX_ENTRIES);
        assert_eq!(
            cfg.enrichment_batch_size,
            DEFAULT_CACHE_ENRICHMENT_BATCH_SIZE
        );
    }

    #[test]
    fn cache_env_overrides_applied() {
        let env = MapEnv::new()
            .with(ENV_CACHE_ENABLED, "false")
            .with(ENV_CACHE_INSTRUMENT_TTL_SECS, "60")
            .with(ENV_CACHE_INSTRUMENT_MAX_ENTRIES, "1234")
            .with(ENV_CACHE_INSTRUMENT_ID_TTL_SECS, "120")
            .with(ENV_CACHE_INSTRUMENT_ID_MAX_ENTRIES, "5678")
            .with(ENV_CACHE_INDEX_TTL_SECS, "30")
            .with(ENV_CACHE_INDEX_MAX_ENTRIES, "9")
            .with(ENV_CACHE_FUTURES_TTL_SECS, "600")
            .with(ENV_CACHE_FUTURES_MAX_ENTRIES, "50")
            .with(ENV_CACHE_ENRICHMENT_BATCH_SIZE, "25");
        let mut cfg = RhoodConfig::default();
        cfg.apply_env_overrides(&env);
        assert!(!cfg.cache.enabled);
        assert_eq!(cfg.cache.instrument_ttl_secs, 60);
        assert_eq!(cfg.cache.instrument_max_entries, 1234);
        assert_eq!(cfg.cache.instrument_id_ttl_secs, 120);
        assert_eq!(cfg.cache.instrument_id_max_entries, 5678);
        assert_eq!(cfg.cache.index_ttl_secs, 30);
        assert_eq!(cfg.cache.index_max_entries, 9);
        assert_eq!(cfg.cache.futures_ttl_secs, 600);
        assert_eq!(cfg.cache.futures_max_entries, 50);
        assert_eq!(cfg.cache.enrichment_batch_size, 25);
    }
}
