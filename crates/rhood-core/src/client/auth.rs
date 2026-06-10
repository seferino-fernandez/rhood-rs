//! Login cascade and OAuth token refresh for [`RobinhoodClient`].
//!
//! Owns the multi-step login flow (cache → validate → refresh → headless OAuth),
//! token extraction/persistence, and SMS/email challenge response handling.

use super::{DEFAULT_TOKEN_TYPE, RobinhoodClient};
use crate::api::paths;
use crate::auth::{AuthState, CachedToken};
use crate::models::auth::{
    ChallengeResponsePayload, ChallengeResponseResult, LoginPayload, OAuthResponse,
    RefreshTokenPayload,
};
use crate::{ChallengeType, Result, RhoodError};
use chrono::Utc;
use secrecy::{ExposeSecret, SecretString};
use std::time::SystemTimeError;

impl RobinhoodClient {
    /// Unified login that cascades through all available authentication strategies.
    ///
    /// The cascade order is:
    /// 1. **Cache** — load token from disk
    /// 2. **Validate** — confirm the cached token is accepted by the server
    /// 3. **Refresh** — if validation fails, try refreshing the access token
    /// 4. **Headless** — if refresh fails, perform a full OAuth password grant
    ///
    /// If the headless login encounters a challenge (SMS/email), the error
    /// [`RhoodError::ChallengeRequired`] is returned with the challenge details.
    /// The caller should collect the code from the user and call
    /// [`submit_challenge_response()`](Self::submit_challenge_response) to complete authentication.
    ///
    /// # Arguments
    ///
    /// * `username` — Robinhood account email/username
    /// * `password` — Robinhood account password
    /// * `mfa_secret` — Optional base32-encoded TOTP secret for automated MFA
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::ChallengeRequired`] if SMS/email verification is needed.
    /// Returns [`RhoodError::DeviceVerificationRequired`] if push verification is needed
    /// (for push challenges, the library polls automatically during `login_headless`).
    /// Returns transport or API errors on network failures.
    pub async fn login(
        &self,
        username: &str,
        password: &str,
        mfa_secret: Option<&str>,
    ) -> Result<()> {
        // Step 1: Try loading from cache
        if let Ok(Some(cached)) = self.token_cache.load() {
            tracing::debug!("Found cached token, restoring auth state");
            self.device_token
                .write()
                .await
                .clone_from(&cached.device_token);
            *self.auth_state.write().await = AuthState::Authenticated {
                access_token: cached.access_token.clone(),
                token_type: cached.token_type.clone(),
                refresh_token: cached.refresh_token.clone(),
            };

            // Step 2: Validate cached token with a live API call
            match self.validate_token().await {
                Ok(true) => {
                    tracing::debug!("Cached token validated successfully");
                    return Ok(());
                }
                Ok(false) => {
                    tracing::debug!("Cached token rejected by server, trying refresh");
                }
                Err(err) => {
                    tracing::warn!(%err, "Token validation failed with error, trying refresh");
                }
            }

            // Step 3: Try refreshing the token
            match self.try_refresh_token().await {
                Ok(true) => {
                    tracing::debug!("Token refresh succeeded");
                    return Ok(());
                }
                Ok(false) => {
                    tracing::debug!("Token refresh failed, falling through to headless login");
                }
                Err(err) => {
                    tracing::warn!(%err, "Token refresh error, falling through to headless login");
                }
            }
        }

        // Step 4: Full headless login
        tracing::debug!("Attempting headless login");
        *self.auth_state.write().await = AuthState::Unauthenticated;
        self.login_headless(username, password, mfa_secret).await
    }

    /// Attempts to restore an authenticated session from the on-disk token cache.
    ///
    /// Loads the cached token, validates it with a live API call via
    /// [`validate_token()`](Self::validate_token), and on failure attempts
    /// to refresh it. Returns `true` if the client is now authenticated,
    /// `false` if all recovery strategies failed.
    ///
    /// # Errors
    ///
    /// Returns an error on I/O failures or HTTP transport errors.
    pub async fn login_from_cache(&self) -> Result<bool> {
        let Some(cached) = self.token_cache.load()? else {
            tracing::debug!("No cached token found");
            return Ok(false);
        };
        tracing::debug!("Found cached token, validating");
        self.device_token
            .write()
            .await
            .clone_from(&cached.device_token);
        *self.auth_state.write().await = AuthState::Authenticated {
            access_token: cached.access_token.clone(),
            token_type: cached.token_type.clone(),
            refresh_token: cached.refresh_token.clone(),
        };

        // Validate with a live API call
        match self.validate_token().await {
            Ok(true) => {
                tracing::debug!("Cached token is valid");
                return Ok(true);
            }
            Ok(false) => {
                tracing::debug!("Cached token validation failed, attempting refresh");
            }
            Err(err) => {
                tracing::debug!(%err, "Token validation error, attempting refresh");
            }
        }

        // Token validation failed — try refresh before giving up
        if self.try_refresh_token().await? {
            tracing::debug!("Token refresh succeeded");
            return Ok(true);
        }

        tracing::debug!("Token refresh failed, clearing auth state");
        *self.auth_state.write().await = AuthState::Unauthenticated;
        Ok(false)
    }

    /// Validates the current access token by making a lightweight API call.
    ///
    /// Returns `Ok(true)` if the token is accepted by the server, `Ok(false)`
    /// if the server returns 401 or 403 (token revoked or invalid), and
    /// `Err` on network/transport errors.
    ///
    /// Uses `GET /positions/?nonzero=true` as the validation endpoint —
    /// it returns a small payload and is always available for authenticated users.
    pub async fn validate_token(&self) -> Result<bool> {
        let auth = match self.auth_state.read().await.authorization_header() {
            Some(header) => header,
            None => return Ok(false),
        };
        let url = self.api_url(paths::POSITIONS);
        let res = self
            .http
            .get(&url)
            .header("Authorization", &auth)
            .query(&[("nonzero", "true")])
            .send()
            .await?;
        let status = res.status().as_u16();
        Ok(status != 401 && status != 403 && res.status().is_success())
    }

    /// Attempt to refresh the access token using the stored refresh_token.
    ///
    /// Returns `Ok(true)` if refresh succeeded and state is now Authenticated.
    /// Returns `Ok(false)` if refresh failed gracefully (no refresh token, server
    /// rejection, or the refresh token itself has expired — indicated by the
    /// server returning a `verification_workflow` in the response).
    async fn try_refresh_token(&self) -> Result<bool> {
        let refresh_token = match self.auth_state.read().await.refresh_token() {
            Some(rt) if !rt.expose_secret().is_empty() => rt.clone(),
            _ => return Ok(false),
        };

        let payload = RefreshTokenPayload {
            client_id: self.config.auth.client_id.clone(),
            grant_type: "refresh_token",
            refresh_token: refresh_token.expose_secret().to_string(),
            scope: "internal",
            device_token: self.device_token.read().await.clone(),
        };

        let token_url = self.api_url(paths::TOKEN);
        tracing::debug!("Attempting token refresh");
        let res = self.http.post(&token_url).form(&payload).send().await?;
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        tracing::debug!(status = status.as_u16(), "Token refresh response");

        if !status.is_success() {
            return Ok(false);
        }

        let data: OAuthResponse = serde_json::from_str(&body).map_err(|err| RhoodError::Api {
            status: status.as_u16(),
            message: format!("Failed to parse token refresh response: {err}"),
        })?;

        // If the refresh response contains a verification_workflow, the refresh
        // token itself has expired — a full re-authentication is required.
        // Return false to let the cascade fall through to headless login.
        if data.verification_workflow.is_some() {
            tracing::debug!("Refresh token expired (verification_workflow in response)");
            return Ok(false);
        }

        match self.extract_tokens(&data).await {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Submit the initial OAuth2 password grant. Sets `auth_state` based on
    /// the response (Authenticated, MfaRequired, DeviceVerification, or Challenged).
    pub async fn login_headless(
        &self,
        username: &str,
        password: &str,
        mfa_secret: Option<&str>,
    ) -> Result<()> {
        if self.login_from_cache().await? {
            return Ok(());
        }

        let mfa_code = if let Some(secret) = mfa_secret {
            let totp = totp_rs::TOTP::new(
                totp_rs::Algorithm::SHA1,
                6,
                1,
                30,
                totp_rs::Secret::Encoded(secret.to_string())
                    .to_bytes()
                    .map_err(|err| {
                        RhoodError::InvalidParameter(format!("Invalid MFA secret: {err}"))
                    })?,
            )
            .map_err(|err| RhoodError::InvalidParameter(format!("TOTP error: {err}")))?;
            let code = totp.generate_current().map_err(|err: SystemTimeError| {
                RhoodError::InvalidParameter(format!("TOTP generation failed: {err}"))
            })?;
            Some(code)
        } else {
            None
        };

        let payload = LoginPayload {
            client_id: self.config.auth.client_id.clone(),
            expires_in: self.config.auth.token_expiry_secs.to_string(),
            grant_type: "password",
            username: username.to_string(),
            password: password.to_string(),
            scope: "internal",
            device_token: self.device_token.read().await.clone(),
            try_passkeys: "false",
            token_request_path: "/login",
            create_read_only_secondary_token: "true",
            mfa_code,
        };

        let token_url = self.api_url(paths::TOKEN);
        tracing::debug!(url = %token_url, "Sending login request");
        let res = self.http.post(&token_url).form(&payload).send().await?;
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        tracing::debug!(status = status.as_u16(), "Login response");

        let data: OAuthResponse = serde_json::from_str(&body).map_err(|err| {
            tracing::error!(status = status.as_u16(), "Failed to parse login response");
            RhoodError::Api {
                status: status.as_u16(),
                message: format!("Failed to parse login response: {err}"),
            }
        })?;

        // Surface API error detail when no actionable auth fields are present
        if data.access_token.is_none()
            && data.mfa_required.is_none()
            && data.verification_workflow.is_none()
            && data.challenge.is_none()
        {
            if let Some(detail) = &data.detail {
                return Err(RhoodError::Api {
                    status: status.as_u16(),
                    message: detail.clone(),
                });
            }
            if !status.is_success() {
                return Err(RhoodError::Api {
                    status: status.as_u16(),
                    message: format!("Login failed with no actionable response (body: {body})"),
                });
            }
        }

        // Device verification: run the pathfinder flow, then retry login
        if let Some(workflow) = &data.verification_workflow {
            let workflow_id = workflow.id.clone();
            tracing::info!("Device verification required — approve on your Robinhood app");
            self.handle_device_verification(&workflow_id).await?;

            // Retry the original login after device is verified
            tracing::info!("Device verified — completing login");
            let res = self.http.post(&token_url).form(&payload).send().await?;
            let retry_status = res.status();
            let retry_body = res.text().await.unwrap_or_default();
            tracing::debug!(status = retry_status.as_u16(), body = %retry_body, "Login retry response");
            let data: OAuthResponse =
                serde_json::from_str(&retry_body).map_err(|err| RhoodError::Api {
                    status: retry_status.as_u16(),
                    message: format!(
                        "Failed to parse login retry response: {err} (body: {retry_body})"
                    ),
                })?;
            return self
                .handle_login_response(&data, mfa_secret.is_none())
                .await;
        }

        self.handle_login_response(&data, mfa_secret.is_none())
            .await
    }

    /// Handle the OAuth2 response, transitioning auth_state appropriately.
    async fn handle_login_response(
        &self,
        data: &OAuthResponse,
        mfa_secret_absent: bool,
    ) -> Result<()> {
        // Device verification required (should not reach here from login_headless,
        // but kept as a fallback for direct callers)
        if let Some(workflow) = &data.verification_workflow {
            tracing::debug!(workflow_id = %workflow.id, "Device verification required");
            *self.auth_state.write().await = AuthState::DeviceVerification {
                workflow_id: workflow.id.clone(),
            };
            return Err(RhoodError::DeviceVerificationRequired);
        }

        // MFA challenge required
        if data.mfa_required == Some(true) {
            tracing::debug!("MFA required");
            *self.auth_state.write().await = AuthState::MfaRequired;
            if mfa_secret_absent {
                return Err(RhoodError::InvalidParameter(
                    "MFA required but no mfa_secret provided".into(),
                ));
            }
        }

        // SMS/email challenge
        if let Some(challenge) = &data.challenge {
            tracing::debug!(
                challenge_type = %challenge.challenge_type,
                challenge_id = %challenge.id,
                "Challenge required"
            );
            let challenge_type = match challenge.challenge_type.as_str() {
                "sms" => ChallengeType::Sms,
                "email" => ChallengeType::Email,
                _ => ChallengeType::Prompt,
            };
            *self.auth_state.write().await = AuthState::Challenged {
                challenge_type: challenge_type.clone(),
                challenge_id: challenge.id.clone(),
            };
            return Err(RhoodError::ChallengeRequired(challenge_type));
        }

        // Success — extract tokens
        tracing::debug!("Extracting tokens from login response");
        self.extract_tokens(data).await
    }

    /// Extract access/refresh tokens from a successful OAuth2 response,
    /// transition to Authenticated, and persist to cache.
    async fn extract_tokens(&self, data: &OAuthResponse) -> Result<()> {
        let access_token =
            SecretString::from(
                data.access_token
                    .as_deref()
                    .ok_or_else(|| RhoodError::Api {
                        status: 401,
                        message: "No access_token in response".into(),
                    })?,
            );
        let token_type = data
            .token_type
            .as_deref()
            .unwrap_or(DEFAULT_TOKEN_TYPE)
            .to_string();
        let refresh_token = SecretString::from(data.refresh_token.as_deref().unwrap_or(""));

        *self.auth_state.write().await = AuthState::Authenticated {
            access_token: access_token.clone(),
            token_type: token_type.clone(),
            refresh_token: refresh_token.clone(),
        };

        let cached = CachedToken {
            access_token,
            refresh_token,
            token_type,
            device_token: self.device_token.read().await.clone(),
            expires_at: Some({
                Utc::now().timestamp() + self.config.auth.token_expiry_secs as i64
            }),
        };
        self.token_cache.save(&cached)?;
        Ok(())
    }

    /// Respond to an SMS/email challenge with the user-provided code.
    /// On success, transitions to Authenticated.
    pub async fn respond_to_challenge(&self, code: &str) -> Result<()> {
        let challenge_id = match &*self.auth_state.read().await {
            AuthState::Challenged { challenge_id, .. } => challenge_id.clone(),
            _ => {
                return Err(RhoodError::InvalidParameter(
                    "No pending challenge to respond to".into(),
                ));
            }
        };

        let url = format!("{}{challenge_id}/respond/", self.api_url(paths::CHALLENGE));
        let payload = ChallengeResponsePayload {
            response: code.to_string(),
        };

        let res = self.http.post(&url).form(&payload).send().await?;
        let data: ChallengeResponseResult = res.json().await?;
        tracing::debug!(body = ?data, "Challenge response");

        if data.status.as_deref() == Some("validated") {
            // Challenge validated — the caller should re-attempt login
            *self.auth_state.write().await = AuthState::Unauthenticated;
            Ok(())
        } else {
            Err(RhoodError::Api {
                status: 400,
                message: "Challenge response not validated".into(),
            })
        }
    }

    /// Respond to an SMS/email challenge and re-attempt login.
    ///
    /// This is the full challenge-response flow:
    /// 1. POSTs the user-provided code to the challenge endpoint
    /// 2. If validated, re-attempts login with the provided credentials
    /// 3. On success, transitions to `Authenticated` and caches tokens
    ///
    /// The caller must provide the original login credentials because the
    /// challenge response only validates the device — a fresh OAuth password
    /// grant is still required to obtain tokens.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidParameter`] if no challenge is pending.
    /// Returns [`RhoodError::Api`] if the challenge response is rejected.
    /// Returns any login error from the re-attempted `login_headless()` call.
    pub async fn submit_challenge_response(
        &self,
        challenge_id: &str,
        code: &str,
        username: &str,
        password: &str,
        mfa_secret: Option<&str>,
    ) -> Result<()> {
        // Step 1: Submit the challenge response
        let url = format!("{}{challenge_id}/respond/", self.api_url(paths::CHALLENGE));
        let payload = ChallengeResponsePayload {
            response: code.to_string(),
        };

        let res = self.http.post(&url).form(&payload).send().await?;
        let data: ChallengeResponseResult = res.json().await?;
        tracing::debug!(body = ?data, "Challenge response");

        if data.status.as_deref() != Some("validated") {
            return Err(RhoodError::Api {
                status: 400,
                message: "Challenge response not validated".into(),
            });
        }

        // Step 2: Challenge validated — re-attempt login
        tracing::debug!("Challenge validated, re-attempting login");
        *self.auth_state.write().await = AuthState::Unauthenticated;
        self.login_headless(username, password, mfa_secret).await
    }
}

#[cfg(test)]
mod tests {
    use super::super::{default_oauth_response, test_config, test_config_with_tempdir};
    use super::*;
    use crate::models::auth::{ChallengeDetail, VerificationWorkflow};
    use secrecy::ExposeSecret;
    use wiremock::matchers::{body_string_contains, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn client_for_server(base_url: &str) -> (tempfile::TempDir, RobinhoodClient) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = test_config_with_tempdir(&dir);
        config.api.base_url = base_url.to_string();
        let client = RobinhoodClient::with_config(config).unwrap();
        (dir, client)
    }

    async fn authenticated_client_for_server(
        base_url: &str,
        refresh_token: &str,
    ) -> (tempfile::TempDir, RobinhoodClient) {
        let (dir, client) = client_for_server(base_url).await;
        *client.auth_state.write().await = AuthState::Authenticated {
            access_token: SecretString::from("old-access"),
            token_type: "Bearer".to_string(),
            refresh_token: SecretString::from(refresh_token),
        };
        (dir, client)
    }

    #[tokio::test]
    async fn handle_login_response_device_verification() {
        let dir = tempfile::tempdir().unwrap();
        let client = RobinhoodClient::with_config(test_config_with_tempdir(&dir)).unwrap();
        let data = OAuthResponse {
            verification_workflow: Some(VerificationWorkflow {
                id: "wf-abc".into(),
                _workflow_status: None,
            }),
            ..default_oauth_response()
        };
        let err = client.handle_login_response(&data, true).await.unwrap_err();
        assert!(matches!(err, RhoodError::DeviceVerificationRequired));
        assert!(matches!(
            client.auth_state().await,
            AuthState::DeviceVerification { workflow_id } if workflow_id == "wf-abc"
        ));
    }

    #[tokio::test]
    async fn handle_login_response_mfa_required() {
        let dir = tempfile::tempdir().unwrap();
        let client = RobinhoodClient::with_config(test_config_with_tempdir(&dir)).unwrap();
        let data = OAuthResponse {
            mfa_required: Some(true),
            ..default_oauth_response()
        };
        let err = client.handle_login_response(&data, true).await.unwrap_err();
        assert!(matches!(err, RhoodError::InvalidParameter(_)));
        assert!(matches!(client.auth_state().await, AuthState::MfaRequired));
    }

    #[tokio::test]
    async fn handle_login_response_challenge() {
        let dir = tempfile::tempdir().unwrap();
        let client = RobinhoodClient::with_config(test_config_with_tempdir(&dir)).unwrap();
        let data = OAuthResponse {
            challenge: Some(ChallengeDetail {
                id: "ch-123".into(),
                challenge_type: "sms".into(),
                _status: Some("issued".into()),
            }),
            ..default_oauth_response()
        };
        let err = client.handle_login_response(&data, true).await.unwrap_err();
        assert!(matches!(
            err,
            RhoodError::ChallengeRequired(ChallengeType::Sms)
        ));
        assert!(matches!(
            client.auth_state().await,
            AuthState::Challenged {
                challenge_type: ChallengeType::Sms,
                ..
            }
        ));
    }

    #[test]
    fn login_method_signature_exists() {
        async fn _assert_login(client: &RobinhoodClient) {
            let _ = client.login("user", "pass", None).await;
        }
    }

    #[test]
    fn submit_challenge_response_signature_exists() {
        async fn _assert_method_exists(client: &RobinhoodClient) {
            let _ = client
                .submit_challenge_response("test-id", "123456", "user", "pass", None)
                .await;
        }
    }

    #[test]
    fn refresh_response_with_verification_workflow_is_detected() {
        let data = OAuthResponse {
            verification_workflow: Some(VerificationWorkflow {
                id: "wf-expired".into(),
                _workflow_status: None,
            }),
            ..default_oauth_response()
        };
        assert!(data.verification_workflow.is_some());
        assert!(data.access_token.is_none());
    }

    #[tokio::test]
    async fn respond_to_challenge_requires_challenged_state() {
        let dir = tempfile::tempdir().unwrap();
        let client = RobinhoodClient::with_config(test_config_with_tempdir(&dir)).unwrap();
        let err = client.respond_to_challenge("123456").await.unwrap_err();
        assert!(matches!(err, RhoodError::InvalidParameter(_)));
    }

    #[test]
    fn submit_challenge_response_requires_credentials() {
        async fn _check(client: &RobinhoodClient) {
            // 5 params: challenge_id, code, username, password, mfa_secret
            let _ = client
                .submit_challenge_response("id", "code", "user", "pass", None)
                .await;
        }
    }

    #[test]
    fn login_cascade_method_exists() {
        async fn _check(client: &RobinhoodClient) {
            let _ = client.login("user", "pass", Some("secret")).await;
            let _ = client.login("user", "pass", None).await;
        }
    }

    #[tokio::test]
    async fn validate_token_returns_false_when_unauthenticated() {
        let dir = tempfile::tempdir().unwrap();
        let client = RobinhoodClient::with_config(test_config_with_tempdir(&dir)).unwrap();
        let result = client.validate_token().await.unwrap();
        assert!(
            !result,
            "validate_token should return false when unauthenticated"
        );
    }

    #[tokio::test]
    async fn validate_token_returns_true_for_successful_positions_probe() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/positions/"))
            .and(query_param("nonzero", "true"))
            .and(header("Authorization", "Bearer old-access"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": []
            })))
            .mount(&server)
            .await;
        let (_dir, client) = authenticated_client_for_server(&server.uri(), "refresh").await;

        assert!(client.validate_token().await.unwrap());
    }

    #[tokio::test]
    async fn validate_token_returns_false_for_unauthorized_probe() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/positions/"))
            .and(query_param("nonzero", "true"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;
        let (_dir, client) = authenticated_client_for_server(&server.uri(), "refresh").await;

        assert!(!client.validate_token().await.unwrap());
    }

    #[tokio::test]
    async fn try_refresh_token_returns_false_without_refresh_token() {
        let server = MockServer::start().await;
        let (_dir, client) = authenticated_client_for_server(&server.uri(), "").await;

        assert!(!client.try_refresh_token().await.unwrap());
    }

    #[tokio::test]
    async fn try_refresh_token_updates_auth_state_and_cache_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains("refresh_token=old-refresh"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "new-access",
                "token_type": "Token",
                "refresh_token": "new-refresh"
            })))
            .mount(&server)
            .await;
        let (dir, client) = authenticated_client_for_server(&server.uri(), "old-refresh").await;

        assert!(client.try_refresh_token().await.unwrap());
        let state = client.auth_state().await;
        assert_eq!(
            state.authorization_header().as_deref(),
            Some("Token new-access")
        );

        let cache = client.token_cache.load().unwrap().unwrap();
        assert_eq!(cache.access_token.expose_secret(), "new-access");
        drop(dir);
    }

    #[tokio::test]
    async fn try_refresh_token_returns_false_on_server_rejection() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad refresh"))
            .mount(&server)
            .await;
        let (_dir, client) = authenticated_client_for_server(&server.uri(), "old-refresh").await;

        assert!(!client.try_refresh_token().await.unwrap());
        assert_eq!(
            client.auth_state().await.authorization_header().as_deref(),
            Some("Bearer old-access")
        );
    }

    #[tokio::test]
    async fn try_refresh_token_returns_false_when_refresh_requires_verification() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "verification_workflow": {
                    "id": "wf-expired",
                    "workflow_status": "issued"
                }
            })))
            .mount(&server)
            .await;
        let (_dir, client) = authenticated_client_for_server(&server.uri(), "old-refresh").await;

        assert!(!client.try_refresh_token().await.unwrap());
    }

    #[tokio::test]
    async fn login_from_cache_restores_valid_cached_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/positions/"))
            .and(query_param("nonzero", "true"))
            .and(header("Authorization", "Bearer cached-access"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": []
            })))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("token.json");
        let mut config = test_config(cache_path.to_str().unwrap());
        config.api.base_url = server.uri();
        let client = RobinhoodClient::with_config(config).unwrap();
        client
            .token_cache
            .save(&CachedToken {
                access_token: SecretString::from("cached-access"),
                refresh_token: SecretString::from("cached-refresh"),
                token_type: "Bearer".into(),
                device_token: "cached-device".into(),
                expires_at: Some(Utc::now().timestamp() + 60),
            })
            .unwrap();

        assert!(client.login_from_cache().await.unwrap());
        assert_eq!(
            client.auth_state().await.authorization_header().as_deref(),
            Some("Bearer cached-access")
        );
        assert_eq!(&*client.device_token.read().await, "cached-device");
    }

    #[tokio::test]
    async fn login_headless_surfaces_api_detail_when_no_actionable_fields_exist() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "detail": "invalid login"
            })))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let err = client
            .login_headless("user", "pass", None)
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            RhoodError::Api {
                status: 400,
                message
            } if message == "invalid login"
        ));
    }

    #[tokio::test]
    async fn login_headless_extracts_tokens_on_successful_password_grant() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .and(body_string_contains("grant_type=password"))
            .and(body_string_contains("username=user"))
            .and(body_string_contains("password=pass"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "login-access",
                "token_type": "Bearer",
                "refresh_token": "login-refresh"
            })))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        client.login_headless("user", "pass", None).await.unwrap();

        assert_eq!(
            client.auth_state().await.authorization_header().as_deref(),
            Some("Bearer login-access")
        );
    }

    #[tokio::test]
    async fn respond_to_challenge_validated_resets_to_unauthenticated() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/challenge/ch-1/respond/"))
            .and(body_string_contains("response=123456"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "validated"
            })))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;
        *client.auth_state.write().await = AuthState::Challenged {
            challenge_type: ChallengeType::Sms,
            challenge_id: "ch-1".into(),
        };

        client.respond_to_challenge("123456").await.unwrap();

        assert!(matches!(
            client.auth_state().await,
            AuthState::Unauthenticated
        ));
    }

    #[tokio::test]
    async fn submit_challenge_response_rejects_unvalidated_status_before_retrying_login() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/challenge/ch-1/respond/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "pending"
            })))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let err = client
            .submit_challenge_response("ch-1", "123456", "user", "pass", None)
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            RhoodError::Api {
                status: 400,
                message
            } if message == "Challenge response not validated"
        ));
    }

    #[tokio::test]
    async fn handle_login_response_email_challenge() {
        let dir = tempfile::tempdir().unwrap();
        let client = RobinhoodClient::with_config(test_config_with_tempdir(&dir)).unwrap();
        let data = OAuthResponse {
            challenge: Some(ChallengeDetail {
                id: "ch-email-1".into(),
                challenge_type: "email".into(),
                _status: Some("issued".into()),
            }),
            ..default_oauth_response()
        };
        let err = client.handle_login_response(&data, true).await.unwrap_err();
        assert!(matches!(
            err,
            RhoodError::ChallengeRequired(ChallengeType::Email)
        ));
        assert!(matches!(
            client.auth_state().await,
            AuthState::Challenged {
                challenge_type: ChallengeType::Email,
                challenge_id,
            } if challenge_id == "ch-email-1"
        ));
    }

    #[tokio::test]
    async fn handle_login_response_prompt_challenge() {
        let dir = tempfile::tempdir().unwrap();
        let client = RobinhoodClient::with_config(test_config_with_tempdir(&dir)).unwrap();
        let data = OAuthResponse {
            challenge: Some(ChallengeDetail {
                id: "ch-prompt-1".into(),
                challenge_type: "prompt".into(),
                _status: Some("issued".into()),
            }),
            ..default_oauth_response()
        };
        let err = client.handle_login_response(&data, true).await.unwrap_err();
        assert!(matches!(
            err,
            RhoodError::ChallengeRequired(ChallengeType::Prompt)
        ));
    }

    #[tokio::test]
    async fn extract_tokens_success() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("tokens.json");
        let client =
            RobinhoodClient::with_config(test_config(cache_path.to_str().unwrap())).unwrap();

        let data = OAuthResponse {
            access_token: Some("access123".into()),
            token_type: Some("Bearer".into()),
            refresh_token: Some("refresh456".into()),
            ..default_oauth_response()
        };
        client.extract_tokens(&data).await.unwrap();

        assert!(client.is_authenticated().await);
        assert_eq!(
            client.auth_state().await.authorization_header().unwrap(),
            "Bearer access123"
        );
        // Verify token was cached to disk
        assert!(cache_path.exists());
    }

    #[tokio::test]
    async fn extract_tokens_missing_access_token() {
        let dir = tempfile::tempdir().unwrap();
        let client = RobinhoodClient::with_config(test_config_with_tempdir(&dir)).unwrap();
        let data = default_oauth_response();
        let err = client.extract_tokens(&data).await.unwrap_err();
        assert!(matches!(err, RhoodError::Api { status: 401, .. }));
    }

    #[tokio::test]
    async fn extract_tokens_default_token_type() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("tokens.json");
        let client =
            RobinhoodClient::with_config(test_config(cache_path.to_str().unwrap())).unwrap();

        let data = OAuthResponse {
            access_token: Some("tok".into()),
            token_type: None, // Should default to "Bearer"
            refresh_token: Some("ref".into()),
            ..default_oauth_response()
        };
        client.extract_tokens(&data).await.unwrap();
        assert_eq!(
            client.auth_state().await.authorization_header().unwrap(),
            "Bearer tok"
        );
    }
}
