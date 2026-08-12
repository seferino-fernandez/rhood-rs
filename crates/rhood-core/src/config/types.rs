use secrecy::SecretString;
use serde::Deserialize;

use crate::{Result, RhoodError};

pub(super) const DEFAULT_CLIENT_ID: &str = "c82SH0WZOsabOXGP2sxqcj34FxkvfnWRZBKlBjFS";
pub(super) const DEFAULT_TOKEN_EXPIRY_SECS: u64 = 86400;
pub(super) const DEFAULT_TOKEN_CACHE_PATH: &str = "~/.rhood/.rhood-token";
pub(super) const DEFAULT_BASE_URL: &str = "https://api.robinhood.com";
pub(super) const DEFAULT_PHOENIX_URL: &str = "https://phoenix.robinhood.com";
pub(super) const DEFAULT_BONFIRE_URL: &str = "https://bonfire.robinhood.com";
pub(super) const DEFAULT_DV_POLL_INTERVAL_SECS: u64 = 5;
pub(super) const DEFAULT_DV_TIMEOUT_SECS: u64 = 120;
pub(super) const DEFAULT_LOG_LEVEL: &str = "info";
pub(super) const DEFAULT_HTTP_REQUEST_TIMEOUT_SECS: u64 = 30;
pub(super) const DEFAULT_HTTP_CONNECT_TIMEOUT_SECS: u64 = 10;
pub(super) const DEFAULT_HTTP_READ_REQUEST_INTERVAL_MS: u64 = 250;
pub(super) const DEFAULT_HTTP_READ_MAX_ATTEMPTS: u32 = 3;
pub(super) const DEFAULT_HTTP_READ_RETRY_BACKOFF_MS: u64 = 250;
pub(super) const DEFAULT_CACHE_ENABLED: bool = true;
pub(super) const DEFAULT_CACHE_INSTRUMENT_TTL_SECS: u64 = 86_400; // 24h
pub(super) const DEFAULT_CACHE_INSTRUMENT_MAX_ENTRIES: u64 = 10_000;
pub(super) const DEFAULT_CACHE_INSTRUMENT_ID_TTL_SECS: u64 = 86_400; // 24h
pub(super) const DEFAULT_CACHE_INSTRUMENT_ID_MAX_ENTRIES: u64 = 50_000;
pub(super) const DEFAULT_CACHE_INDEX_TTL_SECS: u64 = 604_800; // 7d
pub(super) const DEFAULT_CACHE_INDEX_MAX_ENTRIES: u64 = 100;
pub(super) const DEFAULT_CACHE_FUTURES_TTL_SECS: u64 = 3_600; // 1h
pub(super) const DEFAULT_CACHE_FUTURES_MAX_ENTRIES: u64 = 500;
pub(super) const DEFAULT_CACHE_ENRICHMENT_BATCH_SIZE: usize = 50;

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

/// Bounded timeouts and authenticated-read resilience settings.
///
/// `request_timeout_secs` is the total-call ceiling (headers + body). A slow
/// or hung upstream is aborted with a timeout error which prevents a single
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
    /// Minimum interval between authenticated GET request starts, in milliseconds.
    pub read_request_interval_ms: u64,
    /// Maximum number of attempts for retryable authenticated GET requests.
    pub read_max_attempts: u32,
    /// Initial exponential backoff for retryable authenticated GET requests, in milliseconds.
    pub read_retry_backoff_ms: u64,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            request_timeout_secs: DEFAULT_HTTP_REQUEST_TIMEOUT_SECS,
            connect_timeout_secs: DEFAULT_HTTP_CONNECT_TIMEOUT_SECS,
            read_request_interval_ms: DEFAULT_HTTP_READ_REQUEST_INTERVAL_MS,
            read_max_attempts: DEFAULT_HTTP_READ_MAX_ATTEMPTS,
            read_retry_backoff_ms: DEFAULT_HTTP_READ_RETRY_BACKOFF_MS,
        }
    }
}

impl HttpConfig {
    pub(crate) fn validate(&self) -> Result<()> {
        for (field, value) in [
            ("read_request_interval_ms", self.read_request_interval_ms),
            ("read_retry_backoff_ms", self.read_retry_backoff_ms),
        ] {
            if value == 0 {
                return Err(RhoodError::InvalidParameter(format!(
                    "http.{field} must be greater than zero"
                )));
            }
        }
        if self.read_max_attempts == 0 {
            return Err(RhoodError::InvalidParameter(
                "http.read_max_attempts must be greater than zero".to_string(),
            ));
        }
        Ok(())
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
    /// Batch size for Instrument Identity Enrichment `?ids=` requests.
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
