use std::path::Path;

use secrecy::SecretString;

use super::{RhoodConfig, ensure_secret_file_permissions, resolve_secret};
use crate::env::{Env, SystemEnv, env_non_empty, env_parsed};
use crate::{Result, RhoodError};

pub(super) const ENV_CONFIG: &str = "RHOOD_CONFIG";
pub(super) const ENV_USERNAME: &str = "RHOOD_USERNAME";
pub(super) const ENV_PASSWORD: &str = "RHOOD_PASSWORD";
pub(super) const ENV_MFA: &str = "RHOOD_MFA";
pub(super) const ENV_CLIENT_ID: &str = "RHOOD_CLIENT_ID";
pub(super) const ENV_DEVICE_TOKEN: &str = "RHOOD_DEVICE_TOKEN";
pub(super) const ENV_TOKEN_EXPIRY_SECS: &str = "RHOOD_TOKEN_EXPIRY_SECS";
pub(super) const ENV_TOKEN_CACHE_PATH: &str = "RHOOD_TOKEN_CACHE_PATH";
pub(super) const ENV_API_URL: &str = "RHOOD_API_URL";
pub(super) const ENV_PHOENIX_URL: &str = "RHOOD_PHOENIX_URL";
pub(super) const ENV_BONFIRE_URL: &str = "RHOOD_BONFIRE_URL";
pub(super) const ENV_DV_POLL_INTERVAL_SECS: &str = "RHOOD_DV_POLL_INTERVAL_SECS";
pub(super) const ENV_DV_TIMEOUT_SECS: &str = "RHOOD_DV_TIMEOUT_SECS";
pub(super) const ENV_LOG_LEVEL: &str = "RHOOD_LOG_LEVEL";
pub(super) const ENV_READ_ONLY: &str = "RHOOD_READ_ONLY";
pub(super) const ENV_PASSWORD_FILE: &str = "RHOOD_PASSWORD_FILE";
pub(super) const ENV_MFA_FILE: &str = "RHOOD_MFA_FILE";
pub(super) const ENV_DEVICE_TOKEN_FILE: &str = "RHOOD_DEVICE_TOKEN_FILE";
pub(super) const ENV_HTTP_REQUEST_TIMEOUT_SECS: &str = "RHOOD_HTTP_REQUEST_TIMEOUT_SECS";
pub(super) const ENV_HTTP_CONNECT_TIMEOUT_SECS: &str = "RHOOD_HTTP_CONNECT_TIMEOUT_SECS";
pub(super) const ENV_HTTP_READ_REQUEST_INTERVAL_MS: &str = "RHOOD_HTTP_READ_REQUEST_INTERVAL_MS";
pub(super) const ENV_HTTP_READ_MAX_ATTEMPTS: &str = "RHOOD_HTTP_READ_MAX_ATTEMPTS";
pub(super) const ENV_HTTP_READ_RETRY_BACKOFF_MS: &str = "RHOOD_HTTP_READ_RETRY_BACKOFF_MS";
pub(super) const ENV_CACHE_ENABLED: &str = "RHOOD_CACHE_ENABLED";
pub(super) const ENV_CACHE_INSTRUMENT_TTL_SECS: &str = "RHOOD_CACHE_INSTRUMENT_TTL_SECS";
pub(super) const ENV_CACHE_INSTRUMENT_MAX_ENTRIES: &str = "RHOOD_CACHE_INSTRUMENT_MAX_ENTRIES";
pub(super) const ENV_CACHE_INSTRUMENT_ID_TTL_SECS: &str = "RHOOD_CACHE_INSTRUMENT_ID_TTL_SECS";
pub(super) const ENV_CACHE_INSTRUMENT_ID_MAX_ENTRIES: &str =
    "RHOOD_CACHE_INSTRUMENT_ID_MAX_ENTRIES";
pub(super) const ENV_CACHE_INDEX_TTL_SECS: &str = "RHOOD_CACHE_INDEX_TTL_SECS";
pub(super) const ENV_CACHE_INDEX_MAX_ENTRIES: &str = "RHOOD_CACHE_INDEX_MAX_ENTRIES";
pub(super) const ENV_CACHE_FUTURES_TTL_SECS: &str = "RHOOD_CACHE_FUTURES_TTL_SECS";
pub(super) const ENV_CACHE_FUTURES_MAX_ENTRIES: &str = "RHOOD_CACHE_FUTURES_MAX_ENTRIES";
pub(super) const ENV_CACHE_ENRICHMENT_BATCH_SIZE: &str = "RHOOD_CACHE_ENRICHMENT_BATCH_SIZE";

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
        // path starts with "~/" or "~something", we are only expanding "~/"
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
    /// is invalid, an environment override is malformed, or secret file resolution fails.
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
    /// is invalid, an environment override is malformed, or secret file resolution fails.
    pub fn load_with_env(path: Option<&Path>, env: &impl Env) -> Result<Self> {
        let explicit = path.is_some();
        let file_path = Self::resolve_path(path, env);

        let mut config = match file_path.as_deref() {
            Some(p) if p.exists() => {
                let contents = std::fs::read_to_string(p)?;
                toml::from_str::<RhoodConfig>(&contents).map_err(|error| {
                    RhoodError::InvalidParameter(format!("Invalid config TOML: {error}"))
                })?
            }
            Some(p) if explicit => {
                return Err(RhoodError::InvalidParameter(format!(
                    "Config file not found: {}",
                    p.display()
                )));
            }
            _ => RhoodConfig::default(),
        };

        if config.contains_inline_secrets()
            && let Some(config_path) = file_path.as_deref()
        {
            ensure_secret_file_permissions(config_path)?;
        }

        config.apply_env_overrides(env)?;
        config.resolve_secret_files()?;
        config.normalize();
        config.http.validate()?;
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
    /// Returns an error if the TOML is invalid, an environment override is malformed,
    /// or secret file resolution fails.
    pub fn from_toml(toml_str: &str) -> Result<Self> {
        Self::from_toml_with_env(toml_str, &SystemEnv)
    }

    /// Like [`RhoodConfig::from_toml`], but reads env vars from the given [`Env`].
    ///
    /// # Errors
    ///
    /// Returns an error if the TOML is invalid, an environment override is malformed,
    /// or secret file resolution fails.
    pub fn from_toml_with_env(toml_str: &str, env: &impl Env) -> Result<Self> {
        let mut config: RhoodConfig = toml::from_str(toml_str).map_err(|error| {
            RhoodError::InvalidParameter(format!("Invalid config TOML: {error}"))
        })?;
        config.apply_env_overrides(env)?;
        config.resolve_secret_files()?;
        config.normalize();
        config.http.validate()?;
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
    /// Returns an error if an environment override is malformed or secret file resolution fails.
    pub fn apply_env_overrides_and_normalize(&mut self) -> Result<()> {
        self.apply_env_overrides_and_normalize_with_env(&SystemEnv)
    }

    /// Like [`apply_env_overrides_and_normalize`](Self::apply_env_overrides_and_normalize),
    /// but reads env vars from the given [`Env`].
    ///
    /// # Errors
    ///
    /// Returns an error if an environment override is malformed or secret file resolution fails.
    pub fn apply_env_overrides_and_normalize_with_env(&mut self, env: &impl Env) -> Result<()> {
        self.apply_env_overrides(env)?;
        self.resolve_secret_files()?;
        self.normalize();
        self.http.validate()?;
        Ok(())
    }

    fn apply_env_overrides(&mut self, env: &impl Env) -> Result<()> {
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
        if let Some(v) = env_parsed::<u64>(env, ENV_TOKEN_EXPIRY_SECS)? {
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
        if let Some(v) = env_parsed::<u64>(env, ENV_DV_POLL_INTERVAL_SECS)? {
            self.device_verification.poll_interval_secs = v;
        }
        if let Some(v) = env_parsed::<u64>(env, ENV_DV_TIMEOUT_SECS)? {
            self.device_verification.timeout_secs = v;
        }
        if let Some(v) = env_parsed::<u64>(env, ENV_HTTP_REQUEST_TIMEOUT_SECS)? {
            self.http.request_timeout_secs = v;
        }
        if let Some(v) = env_parsed::<u64>(env, ENV_HTTP_CONNECT_TIMEOUT_SECS)? {
            self.http.connect_timeout_secs = v;
        }
        if let Some(v) = env_parsed::<u64>(env, ENV_HTTP_READ_REQUEST_INTERVAL_MS)? {
            self.http.read_request_interval_ms = v;
        }
        if let Some(v) = env_parsed::<u32>(env, ENV_HTTP_READ_MAX_ATTEMPTS)? {
            self.http.read_max_attempts = v;
        }
        if let Some(v) = env_parsed::<u64>(env, ENV_HTTP_READ_RETRY_BACKOFF_MS)? {
            self.http.read_retry_backoff_ms = v;
        }
        if let Some(v) = env_non_empty(env, ENV_CACHE_ENABLED) {
            self.cache.enabled = v.eq_ignore_ascii_case("true") || v == "1";
        }
        if let Some(v) = env_parsed::<u64>(env, ENV_CACHE_INSTRUMENT_TTL_SECS)? {
            self.cache.instrument_ttl_secs = v;
        }
        if let Some(v) = env_parsed::<u64>(env, ENV_CACHE_INSTRUMENT_MAX_ENTRIES)? {
            self.cache.instrument_max_entries = v;
        }
        if let Some(v) = env_parsed::<u64>(env, ENV_CACHE_INSTRUMENT_ID_TTL_SECS)? {
            self.cache.instrument_id_ttl_secs = v;
        }
        if let Some(v) = env_parsed::<u64>(env, ENV_CACHE_INSTRUMENT_ID_MAX_ENTRIES)? {
            self.cache.instrument_id_max_entries = v;
        }
        if let Some(v) = env_parsed::<u64>(env, ENV_CACHE_INDEX_TTL_SECS)? {
            self.cache.index_ttl_secs = v;
        }
        if let Some(v) = env_parsed::<u64>(env, ENV_CACHE_INDEX_MAX_ENTRIES)? {
            self.cache.index_max_entries = v;
        }
        if let Some(v) = env_parsed::<u64>(env, ENV_CACHE_FUTURES_TTL_SECS)? {
            self.cache.futures_ttl_secs = v;
        }
        if let Some(v) = env_parsed::<u64>(env, ENV_CACHE_FUTURES_MAX_ENTRIES)? {
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
        Ok(())
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

    fn contains_inline_secrets(&self) -> bool {
        self.auth.password.is_some()
            || self.auth.mfa_secret.is_some()
            || self.auth.device_token.is_some()
    }

    /// Restores invariants after in-place mutation of configuration fields.
    ///
    /// Expands a leading `~` in
    /// [`auth.token_cache_path`](super::AuthConfig::token_cache_path)
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
