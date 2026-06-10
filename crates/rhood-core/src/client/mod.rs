//! The [`RobinhoodClient`] struct and its HTTP transport layer.
//!
//! All authenticated Robinhood API interactions flow through this module.
//! Domain-specific endpoint methods are defined in [`crate::endpoints`] as
//! `impl` blocks on `RobinhoodClient`.

use crate::auth::{AuthState, TokenCache};
use crate::config::{HttpConfig, RhoodConfig};
use crate::resolver_cache::ResolverCache;
use crate::{Result, RhoodError};
use reqwest::header::{HeaderMap, HeaderValue};
use secrecy::ExposeSecret;
#[cfg(any(test, feature = "test-helpers"))]
use secrecy::SecretString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Maximum consecutive server errors before aborting a polling loop.
const MAX_SERVER_ERROR_RETRIES: u32 = 5;

/// Default retry-after interval (in seconds) when the server does not provide one.
const DEFAULT_RETRY_AFTER_SECS: u64 = 60;

/// Default token type when the OAuth response omits it.
const DEFAULT_TOKEN_TYPE: &str = "Bearer";

/// Header name required by all Robinhood futures endpoints.
const FUTURES_CONTRACT_HEADER: &str = "Rh-Contract-Protected";

/// Header value required by all Robinhood futures endpoints.
const FUTURES_CONTRACT_HEADER_VALUE: &str = "true";

/// Authenticated client for the Robinhood REST API.
///
/// Holds HTTP transport, authentication state, device token, and configuration.
/// Domain-specific methods (stocks, options, orders, account) are defined in
/// [`crate::endpoints`] as `impl` blocks on this type.
///
/// # Authentication
///
/// Construct with [`new`](Self::new) or [`with_config`](Self::with_config),
/// then call [`login`](Self::login) before any endpoint method. `login`
/// follows a cascade: cached token → live validation → refresh →
/// headless OAuth. If the server requires a verification code,
/// `login` returns [`RhoodError::ChallengeRequired`] — collect the code
/// from the user and complete the flow with
/// [`submit_challenge_response`](Self::submit_challenge_response).
///
/// # Cloning
///
/// `RobinhoodClient` is cheap to clone: the HTTP transport, authentication
/// state, device token, and configuration are all reference-counted
/// internally. Cloning the client shares the same underlying auth state, so
/// a refresh performed on one clone is visible on all others. The intended
/// pattern for concurrent use is to construct a single client and clone it
/// into each task.
///
/// # Timeouts
///
/// Every outbound call is bounded by two knobs on
/// [`HttpConfig`]: `request_timeout_secs` is the
/// total-call ceiling (headers + body) and `connect_timeout_secs` is the TCP
/// connect ceiling. Defaults are 30s and 10s. A hung upstream is aborted
/// with a timeout error rather than blocking the caller indefinitely.
/// Override via `RHOOD_HTTP_REQUEST_TIMEOUT_SECS` /
/// `RHOOD_HTTP_CONNECT_TIMEOUT_SECS` env vars or the corresponding CLI
/// flags on `rhood-mcp serve`.
///
/// # Example
///
/// ```no_run
/// use rhood_core::RobinhoodClient;
///
/// # async fn run() -> rhood_core::Result<()> {
/// let client = RobinhoodClient::new()?;
/// client.login_from_cache().await?;
/// let portfolio = client.get_portfolio().await?;
/// println!("equity: {:?}", portfolio.equity);
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct RobinhoodClient {
    http: reqwest::Client,
    auth_state: Arc<RwLock<AuthState>>,
    token_cache: TokenCache,
    device_token: Arc<RwLock<String>>,
    config: Arc<RhoodConfig>,
    read_only: bool,
    pub(crate) resolvers: ResolverCache,
}

impl RobinhoodClient {
    /// Creates a new client using the default configuration loaded from
    /// disk and environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if configuration loading or HTTP client construction fails.
    pub fn new() -> Result<Self> {
        let config = RhoodConfig::load(None)?;
        Self::with_config(config)
    }

    /// Creates a new client with the given configuration.
    ///
    /// Construction is a pure operation: no filesystem writes occur here. The
    /// token-cache directory is created lazily on first [`TokenCache::save`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client fails to build.
    pub fn with_config(config: RhoodConfig) -> Result<Self> {
        let device_token = config
            .auth
            .device_token
            .as_ref()
            .map(|secret| secret.expose_secret().to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let cache_path = PathBuf::from(&config.auth.token_cache_path);
        let token_cache = TokenCache::with_path(cache_path);
        let http = build_http_client(&config.http)?;
        let read_only = config.read_only;
        let resolvers = ResolverCache::from_config(&config.cache);
        Ok(Self {
            http,
            auth_state: Arc::new(RwLock::new(AuthState::Unauthenticated)),
            token_cache,
            read_only,
            device_token: Arc::new(RwLock::new(device_token)),
            config: Arc::new(config),
            resolvers,
        })
    }

    /// Test-only: injects an authenticated state without performing OAuth.
    ///
    /// **SAFETY:** This method bypasses the entire OAuth flow and accepts
    /// arbitrary tokens. The `test-helpers` feature must never be enabled
    /// in production builds, published binaries, or `--all-features`
    /// invocations of downstream consumers — doing so exposes a public
    /// method that can overwrite the client's authentication state.
    ///
    /// Available only when the `test-helpers` feature is enabled. Used by
    /// integration tests to bypass the full login flow.
    #[cfg(feature = "test-helpers")]
    #[doc(hidden)]
    pub async fn inject_test_auth(
        &self,
        access_token: SecretString,
        token_type: String,
        refresh_token: SecretString,
    ) {
        *self.auth_state.write().await = AuthState::Authenticated {
            access_token,
            token_type,
            refresh_token,
        };
    }

    /// Constructs a full URL by appending `path` to the configured API base URL.
    pub fn api_url(&self, path: &str) -> String {
        format!("{}{}", self.config.api.base_url, path)
    }

    /// Constructs a full URL by appending `path` to the configured Phoenix base URL.
    pub fn phoenix_url(&self, path: &str) -> String {
        format!("{}{}", self.config.api.phoenix_url, path)
    }

    /// Constructs a full URL by appending `path` to the configured Bonfire base URL.
    pub fn bonfire_url(&self, path: &str) -> String {
        format!("{}{}", self.config.api.bonfire_url, path)
    }

    /// Returns a reference to the client's configuration.
    pub fn config(&self) -> &RhoodConfig {
        &self.config
    }

    /// Returns a snapshot of the current authentication state.
    ///
    /// The state is cloned out from behind an internal read lock so the
    /// returned value is an owned snapshot; later mutations on the client
    /// will not be reflected in it.
    pub async fn auth_state(&self) -> AuthState {
        self.auth_state.read().await.clone()
    }

    /// Returns `true` if the client holds valid authentication tokens.
    pub async fn is_authenticated(&self) -> bool {
        self.auth_state.read().await.is_authenticated()
    }

    /// Clears the authentication state and deletes the on-disk token cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the token cache file cannot be deleted.
    pub async fn logout(&self) -> Result<()> {
        *self.auth_state.write().await = AuthState::Unauthenticated;
        self.token_cache.clear()?;
        Ok(())
    }

    async fn require_auth(&self) -> Result<String> {
        self.auth_state
            .read()
            .await
            .authorization_header()
            .ok_or(RhoodError::NotAuthenticated)
    }

    pub(crate) fn require_writable(&self) -> Result<()> {
        if self.read_only {
            return Err(RhoodError::ReadOnlyMode);
        }
        Ok(())
    }
}

fn build_http_client(http_config: &HttpConfig) -> Result<reqwest::Client> {
    let mut headers = HeaderMap::new();
    headers.insert("Accept", HeaderValue::from_static("*/*"));
    headers.insert("Accept-Language", HeaderValue::from_static("en-US,en;q=1"));
    headers.insert(
        "Content-Type",
        HeaderValue::from_static("application/x-www-form-urlencoded; charset=utf-8"),
    );
    headers.insert(
        "X-Robinhood-API-Version",
        HeaderValue::from_static("1.431.4"),
    );
    headers.insert("User-Agent", HeaderValue::from_static("*"));

    reqwest::Client::builder()
        .default_headers(headers)
        .cookie_store(true)
        .timeout(Duration::from_secs(http_config.request_timeout_secs))
        .connect_timeout(Duration::from_secs(http_config.connect_timeout_secs))
        .build()
        .map_err(RhoodError::Http)
}

mod auth;
mod device_verification;
mod transport;

#[cfg(test)]
fn test_config(cache_path: &str) -> crate::config::RhoodConfig {
    let mut cfg = crate::config::RhoodConfig::default();
    cfg.auth.token_cache_path = cache_path.to_string();
    cfg
}

#[cfg(test)]
fn test_config_with_tempdir(dir: &tempfile::TempDir) -> crate::config::RhoodConfig {
    let cache_path = dir.path().join("nonexistent-token.json");
    test_config(cache_path.to_str().unwrap())
}

#[cfg(test)]
fn default_oauth_response() -> crate::models::auth::OAuthResponse {
    crate::models::auth::OAuthResponse {
        access_token: None,
        token_type: None,
        refresh_token: None,
        _expires_in: None,
        _scope: None,
        _user_uuid: None,
        _backup_code: None,
        mfa_required: None,
        _mfa_code: None,
        verification_workflow: None,
        challenge: None,
        detail: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_client_succeeds() {
        assert!(build_http_client(&HttpConfig::default()).is_ok());
    }

    #[tokio::test]
    async fn require_auth_when_unauthenticated() {
        let dir = tempfile::tempdir().unwrap();
        let client = RobinhoodClient::with_config(test_config_with_tempdir(&dir)).unwrap();
        assert!(matches!(
            client.require_auth().await,
            Err(RhoodError::NotAuthenticated)
        ));
    }

    #[test]
    fn url_helpers_compose_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let client = RobinhoodClient::with_config(test_config_with_tempdir(&dir)).unwrap();
        assert_eq!(
            client.api_url("/oauth2/token/"),
            "https://api.robinhood.com/oauth2/token/"
        );
        assert_eq!(
            client.phoenix_url("/accounts/"),
            "https://phoenix.robinhood.com/accounts/"
        );
    }

    #[test]
    fn bonfire_url_helper_composes_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let client = RobinhoodClient::with_config(test_config_with_tempdir(&dir)).unwrap();
        assert_eq!(
            client.bonfire_url("/accounts/unified"),
            "https://bonfire.robinhood.com/accounts/unified"
        );
    }

    #[test]
    fn config_accessor_returns_config() {
        let dir = tempfile::tempdir().unwrap();
        let client = RobinhoodClient::with_config(test_config_with_tempdir(&dir)).unwrap();
        assert_eq!(client.config().api.base_url, "https://api.robinhood.com");
    }

    #[test]
    fn require_writable_when_explicitly_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = test_config_with_tempdir(&dir);
        cfg.read_only = false;
        let client = RobinhoodClient::with_config(cfg).unwrap();
        assert!(client.require_writable().is_ok());
    }

    #[test]
    fn require_writable_default_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let client = RobinhoodClient::with_config(test_config_with_tempdir(&dir)).unwrap();
        assert!(matches!(
            client.require_writable(),
            Err(RhoodError::ReadOnlyMode)
        ));
    }

    #[test]
    fn require_writable_read_only_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = test_config_with_tempdir(&dir);
        cfg.read_only = true;
        let client = RobinhoodClient::with_config(cfg).unwrap();
        assert!(matches!(
            client.require_writable(),
            Err(RhoodError::ReadOnlyMode)
        ));
    }

    #[tokio::test]
    async fn read_only_blocks_cancel_stock_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = test_config_with_tempdir(&dir);
        cfg.read_only = true;
        let client = RobinhoodClient::with_config(cfg).unwrap();
        // Force authenticated state so we get past require_auth
        *client.auth_state.write().await = AuthState::Authenticated {
            access_token: SecretString::from("fake"),
            token_type: "Bearer".to_string(),
            refresh_token: SecretString::from("fake"),
        };
        let err = client.cancel_stock_order("fake-id").await.unwrap_err();
        assert!(matches!(err, RhoodError::ReadOnlyMode));
    }
}
