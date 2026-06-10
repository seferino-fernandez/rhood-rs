use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
use uuid::Uuid;

const CLIENT_ID_PREFIX: &str = "rhood_cid_";
const CLIENT_SECRET_PREFIX: &str = "rhood_cs_";
const CSRF_NONCE_PREFIX: &str = "rhood_csrf_";
const AUTH_CODE_PREFIX: &str = "rhood_ac_";
const ACCESS_TOKEN_PREFIX: &str = "rhood_at_";
const REFRESH_TOKEN_PREFIX: &str = "rhood_rt_";

/// Configuration for the in-memory OAuth store's TTL and expiry behavior.
#[derive(Debug, Clone)]
pub struct OAuthStoreConfig {
    /// How long an authorization code remains valid before expiring.
    pub auth_code_ttl: Duration,
    /// How long a CSRF nonce remains valid before expiring.
    pub csrf_nonce_ttl: Duration,
    /// How long an access token remains valid (in seconds).
    pub token_expiry_secs: u64,
}

impl Default for OAuthStoreConfig {
    fn default() -> Self {
        Self {
            auth_code_ttl: Duration::from_secs(60),
            csrf_nonce_ttl: Duration::from_secs(600),
            token_expiry_secs: 3600,
        }
    }
}

impl OAuthStoreConfig {
    /// Build an [`OAuthStoreConfig`] from the application's [`McpConfig`](crate::config::McpConfig).
    pub fn from_mcp_config(mcp: &crate::config::McpConfig) -> Self {
        Self {
            auth_code_ttl: Duration::from_secs(mcp.oauth_auth_code_ttl_secs),
            csrf_nonce_ttl: Duration::from_secs(mcp.oauth_csrf_nonce_ttl_secs),
            token_expiry_secs: mcp.oauth_token_expiry_secs,
        }
    }
}

/// In-memory store for OAuth 2.1 state: registered clients, authorization codes,
/// access tokens, refresh tokens, and CSRF nonces.
#[derive(Clone)]
pub struct OAuthStore {
    inner: Arc<RwLock<StoreInner>>,
    config: OAuthStoreConfig,
}

struct StoreInner {
    clients: HashMap<String, RegisteredClient>,
    auth_codes: HashMap<String, AuthCode>,
    access_tokens: HashMap<String, AccessToken>,
    refresh_tokens: HashMap<String, String>,
    csrf_nonces: HashMap<String, Instant>,
}

/// A dynamically registered OAuth client.
#[derive(Clone, Debug)]
pub struct RegisteredClient {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub client_name: Option<String>,
    pub redirect_uris: Vec<String>,
}

/// A single-use authorization code issued after user consent.
#[derive(Clone, Debug)]
pub struct AuthCode {
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: Vec<String>,
    pub code_challenge: String,
    pub created_at: Instant,
}

/// An issued access token with associated metadata.
#[derive(Clone, Debug)]
pub struct AccessToken {
    pub token: String,
    pub client_id: String,
    pub scope: Vec<String>,
    pub created_at: Instant,
    pub expires_in: u64,
    pub refresh_token: String,
}

impl OAuthStore {
    /// Create a new store with the given configuration.
    pub fn new(config: OAuthStoreConfig) -> Self {
        Self {
            inner: Arc::new(RwLock::new(StoreInner {
                clients: HashMap::new(),
                auth_codes: HashMap::new(),
                access_tokens: HashMap::new(),
                refresh_tokens: HashMap::new(),
                csrf_nonces: HashMap::new(),
            })),
            config,
        }
    }

    /// Register a new dynamic client and return its credentials.
    pub async fn register_client(
        &self,
        client_name: Option<String>,
        redirect_uris: Vec<String>,
    ) -> RegisteredClient {
        let client_id = format!("{}{}", CLIENT_ID_PREFIX, Uuid::new_v4());
        let client_secret = format!("{}{}", CLIENT_SECRET_PREFIX, Uuid::new_v4());
        let client = RegisteredClient {
            client_id: client_id.clone(),
            client_secret: Some(client_secret),
            client_name,
            redirect_uris,
        };
        self.inner
            .write()
            .await
            .clients
            .insert(client_id, client.clone());
        client
    }

    /// Check whether `redirect_uri` is registered for the given `client_id`.
    pub async fn validate_redirect_uri(&self, client_id: &str, redirect_uri: &str) -> bool {
        let inner = self.inner.read().await;
        inner
            .clients
            .get(client_id)
            .is_some_and(|client| client.redirect_uris.iter().any(|uri| uri == redirect_uri))
    }

    /// Create a single-use CSRF nonce for the consent form.
    pub async fn create_csrf_nonce(&self) -> String {
        let nonce = format!("{}{}", CSRF_NONCE_PREFIX, Uuid::new_v4());
        self.inner
            .write()
            .await
            .csrf_nonces
            .insert(nonce.clone(), Instant::now());
        nonce
    }

    /// Consume and validate a CSRF nonce. Returns `true` if valid and not expired.
    pub async fn consume_csrf_nonce(&self, nonce: &str) -> bool {
        let mut inner = self.inner.write().await;
        match inner.csrf_nonces.remove(nonce) {
            Some(created) => created.elapsed() <= self.config.csrf_nonce_ttl,
            None => false,
        }
    }

    /// Issue a single-use authorization code bound to the given PKCE challenge.
    pub async fn create_auth_code(
        &self,
        client_id: String,
        redirect_uri: String,
        scope: Vec<String>,
        code_challenge: String,
    ) -> String {
        let code = format!("{}{}", AUTH_CODE_PREFIX, Uuid::new_v4());
        let auth_code = AuthCode {
            client_id,
            redirect_uri,
            scope,
            code_challenge,
            created_at: Instant::now(),
        };
        self.inner
            .write()
            .await
            .auth_codes
            .insert(code.clone(), auth_code);
        code
    }

    /// Consume a single-use authorization code. Returns `None` if the code is
    /// invalid, already used, or expired.
    pub async fn consume_auth_code(&self, code: &str) -> Option<AuthCode> {
        let mut inner = self.inner.write().await;
        let auth_code = inner.auth_codes.remove(code)?;
        if auth_code.created_at.elapsed() > self.config.auth_code_ttl {
            return None;
        }
        Some(auth_code)
    }

    /// Issue a new access token and refresh token pair for the given client and scopes.
    pub async fn create_access_token(&self, client_id: String, scope: Vec<String>) -> AccessToken {
        let token_value = format!("{}{}", ACCESS_TOKEN_PREFIX, Uuid::new_v4());
        let refresh_value = format!("{}{}", REFRESH_TOKEN_PREFIX, Uuid::new_v4());
        let token = AccessToken {
            token: token_value.clone(),
            client_id,
            scope,
            created_at: Instant::now(),
            expires_in: self.config.token_expiry_secs,
            refresh_token: refresh_value.clone(),
        };
        let mut inner = self.inner.write().await;
        inner
            .access_tokens
            .insert(token_value.clone(), token.clone());
        inner.refresh_tokens.insert(refresh_value, token_value);
        token
    }

    /// Validate an access token. Returns `None` if the token is unknown or expired.
    pub async fn validate_access_token(&self, token: &str) -> Option<AccessToken> {
        let inner = self.inner.read().await;
        let access_token = inner.access_tokens.get(token)?;
        if access_token.created_at.elapsed() > Duration::from_secs(access_token.expires_in) {
            return None;
        }
        Some(access_token.clone())
    }

    /// Rotate a refresh token: revoke the old access/refresh pair and issue new ones.
    pub async fn refresh_access_token(&self, refresh_token: &str) -> Option<AccessToken> {
        let mut inner = self.inner.write().await;
        let old_token_key = inner.refresh_tokens.remove(refresh_token)?;
        let old_access_token = inner.access_tokens.remove(&old_token_key)?;

        let new_token_value = format!("{}{}", ACCESS_TOKEN_PREFIX, Uuid::new_v4());
        let new_refresh_value = format!("{}{}", REFRESH_TOKEN_PREFIX, Uuid::new_v4());
        let new_token = AccessToken {
            token: new_token_value.clone(),
            client_id: old_access_token.client_id,
            scope: old_access_token.scope,
            created_at: Instant::now(),
            expires_in: old_access_token.expires_in,
            refresh_token: new_refresh_value.clone(),
        };
        inner
            .access_tokens
            .insert(new_token_value.clone(), new_token.clone());
        inner
            .refresh_tokens
            .insert(new_refresh_value, new_token_value);
        Some(new_token)
    }

    /// Remove all expired auth codes, CSRF nonces, and access/refresh token pairs.
    pub async fn sweep_expired(&self) {
        let mut inner = self.inner.write().await;
        let auth_code_ttl = self.config.auth_code_ttl;
        let csrf_nonce_ttl = self.config.csrf_nonce_ttl;

        inner
            .auth_codes
            .retain(|_, code| code.created_at.elapsed() <= auth_code_ttl);
        inner
            .csrf_nonces
            .retain(|_, created| created.elapsed() <= csrf_nonce_ttl);

        let expired_token_keys: Vec<String> = inner
            .access_tokens
            .iter()
            .filter(|(_, access_token)| {
                access_token.created_at.elapsed() > Duration::from_secs(access_token.expires_in)
            })
            .map(|(key, _)| key.clone())
            .collect();

        for token_key in expired_token_keys {
            if let Some(access_token) = inner.access_tokens.remove(&token_key) {
                inner.refresh_tokens.remove(&access_token.refresh_token);
            }
        }
    }

    /// Spawn a background task that periodically calls [`sweep_expired`](Self::sweep_expired).
    ///
    /// The task will exit cleanly when `cancel` is cancelled.
    pub fn spawn_sweep_task(
        self,
        interval: Duration,
        cancel: tokio_util::sync::CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = tokio::time::sleep(interval) => {
                        self.sweep_expired().await;
                    }
                    () = cancel.cancelled() => {
                        tracing::debug!("OAuth sweep task shutting down");
                        break;
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_and_get_client() {
        let store = OAuthStore::new(OAuthStoreConfig::default());
        let client = store
            .register_client(Some("test".into()), vec!["http://localhost/cb".into()])
            .await;
        assert!(client.client_id.starts_with("rhood_cid_"));
        assert!(client.client_secret.is_some());
    }

    #[tokio::test]
    async fn validate_redirect_uri() {
        let store = OAuthStore::new(OAuthStoreConfig::default());
        let client = store
            .register_client(None, vec!["http://localhost/cb".into()])
            .await;
        assert!(
            store
                .validate_redirect_uri(&client.client_id, "http://localhost/cb")
                .await
        );
        assert!(
            !store
                .validate_redirect_uri(&client.client_id, "http://evil.com/cb")
                .await
        );
    }

    #[tokio::test]
    async fn auth_code_lifecycle() {
        let store = OAuthStore::new(OAuthStoreConfig::default());
        let code = store
            .create_auth_code(
                "cid".into(),
                "http://localhost/cb".into(),
                vec!["read".into()],
                "challenge123".into(),
            )
            .await;
        assert!(code.starts_with("rhood_ac_"));

        let auth_code = store.consume_auth_code(&code).await.unwrap();
        assert_eq!(auth_code.client_id, "cid");
        assert_eq!(auth_code.code_challenge, "challenge123");

        // Second consume fails (single-use)
        assert!(store.consume_auth_code(&code).await.is_none());
    }

    #[tokio::test]
    async fn access_token_lifecycle() {
        let store = OAuthStore::new(OAuthStoreConfig::default());
        let access_token = store
            .create_access_token("cid".into(), vec!["read".into()])
            .await;
        assert!(access_token.token.starts_with("rhood_at_"));
        assert!(access_token.refresh_token.starts_with("rhood_rt_"));

        let validated = store
            .validate_access_token(&access_token.token)
            .await
            .unwrap();
        assert_eq!(validated.client_id, "cid");
        assert_eq!(validated.scope, vec!["read"]);
    }

    #[tokio::test]
    async fn refresh_token_rotates() {
        let store = OAuthStore::new(OAuthStoreConfig::default());
        let first_token = store
            .create_access_token("cid".into(), vec!["read".into()])
            .await;

        let second_token = store
            .refresh_access_token(&first_token.refresh_token)
            .await
            .unwrap();
        assert_ne!(second_token.token, first_token.token);
        assert_ne!(second_token.refresh_token, first_token.refresh_token);

        // Old token is revoked
        assert!(
            store
                .validate_access_token(&first_token.token)
                .await
                .is_none()
        );

        // Old refresh token is consumed
        assert!(
            store
                .refresh_access_token(&first_token.refresh_token)
                .await
                .is_none()
        );

        // New token works
        assert!(
            store
                .validate_access_token(&second_token.token)
                .await
                .is_some()
        );
    }

    #[tokio::test]
    async fn expired_token_invalid() {
        let config = OAuthStoreConfig {
            token_expiry_secs: 0,
            ..Default::default()
        };
        let store = OAuthStore::new(config);
        let access_token = store
            .create_access_token("cid".into(), vec!["read".into()])
            .await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(
            store
                .validate_access_token(&access_token.token)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn sweep_removes_expired() {
        let config = OAuthStoreConfig {
            token_expiry_secs: 0,
            ..Default::default()
        };
        let store = OAuthStore::new(config);
        let access_token = store
            .create_access_token("cid".into(), vec!["read".into()])
            .await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        store.sweep_expired().await;

        assert!(
            store
                .validate_access_token(&access_token.token)
                .await
                .is_none()
        );
        assert!(
            store
                .refresh_access_token(&access_token.refresh_token)
                .await
                .is_none()
        );
    }
}
