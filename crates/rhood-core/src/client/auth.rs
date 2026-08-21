//! Login cascade and OAuth token refresh for [`RobinhoodClient`].
//!
//! Owns the multi-step login flow (cache -> validate -> refresh -> headless OAuth),
//! token extraction/persistence, and SMS/email challenge response handling.

use super::{DEFAULT_TOKEN_TYPE, RobinhoodClient};
use crate::api::paths;
use crate::auth::{AuthState, CachedToken};
use crate::models::auth::{
    ChallengeResponsePayload, ChallengeResponseResult, LoginPayload, OAuthResponse,
    RefreshTokenPayload,
};
use crate::{ChallengeType, Result, RhoodError};
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

impl RobinhoodClient {
    /// Unified login that cascades through all available authentication strategies.
    ///
    /// The cascade order is:
    /// 1. **Cache** - load token from disk
    /// 2. **Validate** - confirm the cached token is accepted by the server
    /// 3. **Refresh** - if validation fails, try refreshing the access token
    /// 4. **Headless** - if refresh fails, perform a full OAuth password grant
    ///
    /// If the headless login encounters a challenge (SMS/email), the error
    /// [`RhoodError::ChallengeRequired`] is returned with the challenge details.
    /// The caller should collect the code from the user and call
    /// [`submit_challenge_response()`](Self::submit_challenge_response) to complete authentication.
    ///
    /// # Arguments
    ///
    /// * `username` - Robinhood account email/username
    /// * `password` - Robinhood account password
    /// * `mfa_secret` - Optional base32-encoded TOTP secret for automated MFA
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::ChallengeRequired`] if SMS/email verification is needed.
    /// Returns [`RhoodError::DeviceVerificationRequired`] if push verification is needed
    /// (for push challenges, the library polls automatically during `login_headless`).
    /// Returns cache, transport, or API errors.
    ///
    /// In particular, an insecure token-cache file permission mode is returned
    /// so the caller can correct it before logging in again.
    pub async fn login(
        &self,
        username: &str,
        password: &str,
        mfa_secret: Option<&str>,
    ) -> Result<()> {
        if self
            .recover_from_cache_at(Timestamp::now().as_second())
            .await?
        {
            return Ok(());
        }

        // Full headless login
        tracing::debug!("Attempting headless login");
        *self.auth_state.write().await = AuthState::Unauthenticated;
        self.login_headless_without_cache(username, password, mfa_secret)
            .await
    }

    /// Attempts to restore an authenticated session from the on-disk token cache.
    ///
    /// Loads the cached token and checks its local access-token expiry. An expired
    /// access token is refreshed directly; otherwise the access token is validated
    /// via [`validate_token()`](Self::validate_token) before refresh is attempted.
    /// Returns `true` if the client is now authenticated and `false` if the cache is
    /// absent or the server rejects recovery without an actionable error.
    ///
    /// # Errors
    ///
    /// Returns cache permission, filesystem I/O, HTTP transport/body, response
    /// parsing, incomplete token-pair, expiry calculation, persistence, and cache
    /// cleanup errors. Once cached credentials are staged for recovery, every error
    /// leaves the live authentication state unauthenticated.
    pub async fn login_from_cache(&self) -> Result<bool> {
        self.recover_from_cache_at(Timestamp::now().as_second())
            .await
    }

    async fn recover_from_cache_at(&self, unix_timestamp: i64) -> Result<bool> {
        let Some(cached) = self.token_cache.load()? else {
            tracing::debug!("No cached token found");
            return Ok(false);
        };
        let access_token_expired = cached.access_token_is_expired_at(unix_timestamp);
        tracing::debug!(access_token_expired, "Found cached token");
        self.device_token
            .write()
            .await
            .clone_from(&cached.device_token);
        *self.auth_state.write().await = AuthState::Authenticated {
            access_token: cached.access_token.clone(),
            token_type: cached.token_type.clone(),
            refresh_token: cached.refresh_token.clone(),
        };

        let recovery = async {
            if access_token_expired {
                tracing::debug!("Cached access token is expired, refreshing without validation");
            } else {
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
            }

            self.try_refresh_token_at(unix_timestamp).await
        }
        .await;

        if recovery.as_ref().is_ok_and(|restored| *restored) {
            tracing::debug!("Cached authentication recovery succeeded");
        } else {
            tracing::debug!("Cached authentication recovery failed, clearing auth state");
            *self.auth_state.write().await = AuthState::Unauthenticated;
        }

        recovery
    }

    /// Validates the current access token by making a lightweight API call.
    ///
    /// Returns `Ok(true)` if the token is accepted by the server, `Ok(false)`
    /// if the server returns 401 or 403 (token revoked or invalid), and
    /// `Err` on network/transport errors.
    ///
    /// Uses `GET /positions/?nonzero=true` as the validation endpoint because
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
    /// rejection, or the refresh token itself has expired which is indicated by the
    /// server returning a `verification_workflow` in the response).
    async fn try_refresh_token_at(&self, unix_timestamp: i64) -> Result<bool> {
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
        let body = res.text().await?;
        tracing::debug!(
            status = status.as_u16(),
            body_len = body.len(),
            "Token refresh response"
        );

        let parsed = serde_json::from_str::<OAuthResponse>(&body);
        if parsed
            .as_ref()
            .is_ok_and(|data| data.verification_workflow.is_some())
        {
            tracing::debug!("Refresh token expired (verification_workflow in response)");
            *self.auth_state.write().await = AuthState::Unauthenticated;
            self.token_cache.clear()?;
            return Ok(false);
        }

        if !status.is_success() {
            return Ok(false);
        }

        let data = parsed.map_err(|err| RhoodError::Api {
            status: status.as_u16(),
            message: format!("Failed to parse token refresh response: {err}"),
        })?;
        let access_token = data
            .access_token
            .as_deref()
            .filter(|token| !token.is_empty())
            .ok_or_else(|| RhoodError::Api {
                status: status.as_u16(),
                message: "Incomplete token refresh response: missing access_token".into(),
            })?;
        let refresh_token = data
            .refresh_token
            .as_deref()
            .filter(|token| !token.is_empty())
            .ok_or_else(|| RhoodError::Api {
                status: status.as_u16(),
                message: "Incomplete token refresh response: missing refresh_token".into(),
            })?;
        let token_type = data
            .token_type
            .as_deref()
            .unwrap_or(DEFAULT_TOKEN_TYPE)
            .to_string();
        let device_token = self.device_token.read().await.clone();
        let token_expiry_secs =
            i64::try_from(self.config.auth.token_expiry_secs).map_err(|_conversion_error| {
                RhoodError::InvalidParameter("Token expiry duration exceeds i64::MAX".into())
            })?;
        let expires_at = unix_timestamp
            .checked_add(token_expiry_secs)
            .ok_or_else(|| {
                RhoodError::InvalidParameter("Token expiry timestamp overflowed".into())
            })?;
        let rotated = CachedToken {
            access_token: SecretString::from(access_token),
            refresh_token: SecretString::from(refresh_token),
            token_type: token_type.clone(),
            device_token,
            expires_at: Some(expires_at),
        };

        self.token_cache.save(&rotated)?;
        *self.auth_state.write().await = AuthState::Authenticated {
            access_token: rotated.access_token.clone(),
            token_type,
            refresh_token: rotated.refresh_token.clone(),
        };
        Ok(true)
    }

    /// Submit the initial OAuth2 password grant. Sets `auth_state` based on
    /// the response (Authenticated, MfaRequired, DeviceVerification, or Challenged).
    pub async fn login_headless(
        &self,
        username: &str,
        password: &str,
        mfa_secret: Option<&str>,
    ) -> Result<()> {
        if self
            .recover_from_cache_at(Timestamp::now().as_second())
            .await?
        {
            return Ok(());
        }

        self.login_headless_without_cache(username, password, mfa_secret)
            .await
    }

    async fn login_headless_without_cache(
        &self,
        username: &str,
        password: &str,
        mfa_secret: Option<&str>,
    ) -> Result<()> {
        let mfa_code = if let Some(secret) = mfa_secret {
            let totp = totp_rs::Builder::new()
                .with_algorithm(totp_rs::Algorithm::SHA1)
                .with_digits(6)
                .with_skew(1)
                .with_step_duration(30)
                .with_secret(totp_rs::Secret::try_from_base32(secret).map_err(|err| {
                    RhoodError::InvalidParameter(format!("Invalid MFA secret: {err}"))
                })?)
                .build()
                .map_err(|err| RhoodError::InvalidParameter(format!("TOTP error: {err}")))?;
            // `Totp::generate_current` panics if the clock is before the Unix epoch;
            // reading the clock here keeps that a recoverable error, as it was before
            // totp-rs 6 moved the fallibility out of the return type.
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|err: SystemTimeError| {
                    RhoodError::InvalidParameter(format!("TOTP generation failed: {err}"))
                })?
                .as_secs();
            Some(totp.generate(now).to_string())
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
        tracing::debug!(
            status = status.as_u16(),
            body_len = body.len(),
            "Login response"
        );

        let data: OAuthResponse = serde_json::from_str(&body).map_err(|err| {
            tracing::error!(status = status.as_u16(), "Failed to parse login response");
            RhoodError::Api {
                status: status.as_u16(),
                message: format!("Failed to parse login response: {err}"),
            }
        })?;

        // A non-success status is always an API error, regardless of any auth-shaped
        // fields in the response body. Preserve an API-provided detail when present.
        if !status.is_success() {
            if let Some(detail) = &data.detail {
                return Err(RhoodError::Api {
                    status: status.as_u16(),
                    message: detail.clone(),
                });
            }
            return Err(RhoodError::Api {
                status: status.as_u16(),
                message: format!(
                    "Login failed with no actionable response: {}",
                    super::transport::redacted_response_body_message(&body)
                ),
            });
        }

        // Surface API error detail when no actionable auth fields are present
        if data.access_token.is_none()
            && data.mfa_required.is_none()
            && data.verification_workflow.is_none()
            && data.challenge.is_none()
            && let Some(detail) = &data.detail
        {
            return Err(RhoodError::Api {
                status: status.as_u16(),
                message: detail.clone(),
            });
        }

        // Device verification: run the pathfinder flow, then retry login
        if let Some(workflow) = &data.verification_workflow {
            let workflow_id = workflow.id.clone();
            tracing::info!("Device verification required, approve on your Robinhood app");
            self.handle_device_verification(&workflow_id).await?;

            // Retry the original login after device is verified
            tracing::info!("Device verified, now completing login");
            let res = self.http.post(&token_url).form(&payload).send().await?;
            let retry_status = res.status();
            let retry_body = res.text().await.unwrap_or_default();
            tracing::debug!(
                status = retry_status.as_u16(),
                body_len = retry_body.len(),
                "Login retry response"
            );
            let data: OAuthResponse =
                serde_json::from_str(&retry_body).map_err(|err| RhoodError::Api {
                    status: retry_status.as_u16(),
                    message: format!("Failed to parse login retry response: {err}"),
                })?;
            if !retry_status.is_success() {
                if let Some(detail) = &data.detail {
                    return Err(RhoodError::Api {
                        status: retry_status.as_u16(),
                        message: detail.clone(),
                    });
                }
                return Err(RhoodError::Api {
                    status: retry_status.as_u16(),
                    message: format!(
                        "Login failed with no actionable response: {}",
                        super::transport::redacted_response_body_message(&retry_body)
                    ),
                });
            }
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

        #[expect(
            clippy::arithmetic_side_effects,
            reason = "the externally configured AuthConfig::token_expiry_secs value is assumed to fit a signed Unix timestamp; no in-code bound enforces this"
        )]
        let cached = CachedToken {
            access_token,
            refresh_token,
            token_type,
            device_token: self.device_token.read().await.clone(),
            expires_at: Some({
                Timestamp::now().as_second() + self.config.auth.token_expiry_secs.cast_signed()
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
            // Challenge validated so the caller should re-attempt login
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
    /// challenge response only validates the device. A fresh OAuth password
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

        // Step 2: Challenge validated so re-attempt login
        tracing::debug!("Challenge validated, re-attempting login");
        *self.auth_state.write().await = AuthState::Unauthenticated;
        self.login_headless(username, password, mfa_secret).await
    }
}

#[cfg(test)]
#[expect(
    clippy::let_underscore_must_use,
    reason = "compile-only async helpers prove public method signatures without executing requests"
)]
mod tests {
    use super::super::{default_oauth_response, test_config, test_config_with_tempdir};
    use super::*;
    use crate::models::auth::{ChallengeDetail, VerificationWorkflow};
    use secrecy::ExposeSecret;
    use wiremock::matchers::{body_string_contains, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[cfg(unix)]
    fn set_mode(path: &std::path::Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

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

        assert!(
            !client
                .try_refresh_token_at(Timestamp::now().as_second())
                .await
                .unwrap()
        );
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

        assert!(
            client
                .try_refresh_token_at(Timestamp::now().as_second())
                .await
                .unwrap()
        );
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
    async fn generic_refresh_rejection_preserves_cached_pair() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad refresh"))
            .mount(&server)
            .await;
        let (dir, client) = authenticated_client_for_server(&server.uri(), "old-refresh").await;
        client
            .token_cache
            .save(&CachedToken {
                access_token: SecretString::from("old-access"),
                refresh_token: SecretString::from("old-refresh"),
                token_type: "Bearer".into(),
                device_token: "old-device".into(),
                expires_at: Some(0),
            })
            .unwrap();

        assert!(!client.login_from_cache().await.unwrap());
        assert!(matches!(
            client.auth_state().await,
            AuthState::Unauthenticated
        ));
        let persisted = client.token_cache.load().unwrap().unwrap();
        assert_eq!(persisted.access_token.expose_secret(), "old-access");
        assert_eq!(persisted.refresh_token.expose_secret(), "old-refresh");
        drop(dir);
    }

    #[tokio::test]
    async fn login_from_cache_clears_expired_refresh_for_workflow_on_any_auth_status() {
        for status in [200, 400, 401, 403] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/oauth2/token/"))
                .respond_with(
                    ResponseTemplate::new(status).set_body_json(serde_json::json!({
                        "verification_workflow": {
                            "id": "wf-expired",
                            "workflow_status": "issued"
                        }
                    })),
                )
                .expect(1)
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
                    access_token: SecretString::from("expired-access"),
                    refresh_token: SecretString::from("expired-refresh"),
                    token_type: "Bearer".into(),
                    device_token: "retained-device".into(),
                    expires_at: Some(0),
                })
                .unwrap();

            assert!(!client.login_from_cache().await.unwrap(), "status {status}");
            assert!(
                matches!(client.auth_state().await, AuthState::Unauthenticated),
                "status {status}"
            );
            assert!(!cache_path.exists(), "status {status}");
            server.verify().await;
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn workflow_cleanup_failure_clears_live_state_and_preserves_cached_pair() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "verification_workflow": {
                    "id": "wf-expired",
                    "workflow_status": "issued"
                }
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
                access_token: SecretString::from("expired-access"),
                refresh_token: SecretString::from("retained-refresh"),
                token_type: "Bearer".into(),
                device_token: "retained-device".into(),
                expires_at: Some(0),
            })
            .unwrap();
        set_mode(dir.path(), 0o500);

        let result = client.login_from_cache().await;

        set_mode(dir.path(), 0o700);
        let error = result.unwrap_err();
        assert!(matches!(
            error,
            RhoodError::Io(ref source)
                if source.kind() == std::io::ErrorKind::PermissionDenied
        ));
        assert!(matches!(
            client.auth_state().await,
            AuthState::Unauthenticated
        ));
        let persisted = client.token_cache.load().unwrap().unwrap();
        assert_eq!(persisted.access_token.expose_secret(), "expired-access");
        assert_eq!(persisted.refresh_token.expose_secret(), "retained-refresh");
    }

    #[tokio::test]
    async fn login_falls_through_to_password_grant_after_refresh_workflow() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/positions/"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .and(body_string_contains("grant_type=refresh_token"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "verification_workflow": {
                    "id": "wf-expired",
                    "workflow_status": "issued"
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .and(body_string_contains("grant_type=password"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "password-access",
                "token_type": "Bearer",
                "refresh_token": "password-refresh"
            })))
            .expect(1)
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
                access_token: SecretString::from("expired-access"),
                refresh_token: SecretString::from("expired-refresh"),
                token_type: "Bearer".into(),
                device_token: "retained-device".into(),
                expires_at: Some(0),
            })
            .unwrap();

        client.login("user", "pass", None).await.unwrap();

        assert_eq!(
            client.auth_state().await.authorization_header().as_deref(),
            Some("Bearer password-access")
        );
        let persisted = client.token_cache.load().unwrap().unwrap();
        assert_eq!(persisted.access_token.expose_secret(), "password-access");
        assert_eq!(persisted.refresh_token.expose_secret(), "password-refresh");
        server.verify().await;
    }

    #[tokio::test]
    async fn incomplete_refresh_success_clears_live_state_and_preserves_cached_pair() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "partial-access",
                "token_type": "Bearer"
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
                access_token: SecretString::from("old-access"),
                refresh_token: SecretString::from("retained-refresh"),
                token_type: "Bearer".into(),
                device_token: "retained-device".into(),
                expires_at: Some(0),
            })
            .unwrap();

        let error = client.login_from_cache().await.unwrap_err();

        assert!(matches!(
            error,
            RhoodError::Api { message, .. } if message.contains("missing refresh_token")
        ));
        assert!(matches!(
            client.auth_state().await,
            AuthState::Unauthenticated
        ));
        let persisted = client.token_cache.load().unwrap().unwrap();
        assert_eq!(persisted.access_token.expose_secret(), "old-access");
        assert_eq!(persisted.refresh_token.expose_secret(), "retained-refresh");
    }

    #[tokio::test]
    async fn refresh_expiry_overflow_clears_live_state_and_preserves_cached_pair() {
        const NOW: i64 = i64::MAX - 1;

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "rotated-access",
                "token_type": "Bearer",
                "refresh_token": "rotated-refresh"
            })))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("token.json");
        let mut config = test_config(cache_path.to_str().unwrap());
        config.api.base_url = server.uri();
        config.auth.token_expiry_secs = 2;
        let client = RobinhoodClient::with_config(config).unwrap();
        client
            .token_cache
            .save(&CachedToken {
                access_token: SecretString::from("old-access"),
                refresh_token: SecretString::from("retained-refresh"),
                token_type: "Bearer".into(),
                device_token: "retained-device".into(),
                expires_at: Some(NOW - 1),
            })
            .unwrap();

        let error = client.recover_from_cache_at(NOW).await.unwrap_err();

        assert!(matches!(
            error,
            RhoodError::InvalidParameter(message)
                if message == "Token expiry timestamp overflowed"
        ));
        assert!(matches!(
            client.auth_state().await,
            AuthState::Unauthenticated
        ));
        let persisted = client.token_cache.load().unwrap().unwrap();
        assert_eq!(persisted.access_token.expose_secret(), "old-access");
        assert_eq!(persisted.refresh_token.expose_secret(), "retained-refresh");
    }

    #[tokio::test]
    async fn oversized_refresh_expiry_clears_live_state_and_preserves_cached_pair() {
        const NOW: i64 = 1_700_000_000;

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "rotated-access",
                "token_type": "Bearer",
                "refresh_token": "rotated-refresh"
            })))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("token.json");
        let mut config = test_config(cache_path.to_str().unwrap());
        config.api.base_url = server.uri();
        config.auth.token_expiry_secs = i64::MAX.cast_unsigned() + 1;
        let client = RobinhoodClient::with_config(config).unwrap();
        client
            .token_cache
            .save(&CachedToken {
                access_token: SecretString::from("old-access"),
                refresh_token: SecretString::from("retained-refresh"),
                token_type: "Bearer".into(),
                device_token: "retained-device".into(),
                expires_at: Some(NOW - 1),
            })
            .unwrap();

        let error = client.recover_from_cache_at(NOW).await.unwrap_err();

        assert!(matches!(
            error,
            RhoodError::InvalidParameter(message)
                if message == "Token expiry duration exceeds i64::MAX"
        ));
        assert!(matches!(
            client.auth_state().await,
            AuthState::Unauthenticated
        ));
        let persisted = client.token_cache.load().unwrap().unwrap();
        assert_eq!(persisted.access_token.expose_secret(), "old-access");
        assert_eq!(persisted.refresh_token.expose_secret(), "retained-refresh");
    }

    #[tokio::test]
    async fn malformed_refresh_success_preserves_cached_pair() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not-json"))
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
                access_token: SecretString::from("old-access"),
                refresh_token: SecretString::from("retained-refresh"),
                token_type: "Bearer".into(),
                device_token: "retained-device".into(),
                expires_at: Some(0),
            })
            .unwrap();

        let error = client.login_from_cache().await.unwrap_err();

        assert!(matches!(
            error,
            RhoodError::Api { message, .. } if message.contains("Failed to parse token refresh response")
        ));
        assert!(matches!(
            client.auth_state().await,
            AuthState::Unauthenticated
        ));
        let persisted = client.token_cache.load().unwrap().unwrap();
        assert_eq!(persisted.access_token.expose_secret(), "old-access");
        assert_eq!(persisted.refresh_token.expose_secret(), "retained-refresh");
    }

    #[tokio::test]
    async fn refresh_transport_failure_preserves_cached_pair() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let server_uri = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("token.json");
        let mut config = test_config(cache_path.to_str().unwrap());
        config.api.base_url = server_uri;
        let client = RobinhoodClient::with_config(config).unwrap();
        client
            .token_cache
            .save(&CachedToken {
                access_token: SecretString::from("old-access"),
                refresh_token: SecretString::from("retained-refresh"),
                token_type: "Bearer".into(),
                device_token: "retained-device".into(),
                expires_at: Some(0),
            })
            .unwrap();

        let error = client.login_from_cache().await.unwrap_err();
        assert!(matches!(error, RhoodError::Http(_)));
        assert!(matches!(
            client.auth_state().await,
            AuthState::Unauthenticated
        ));

        let persisted = client.token_cache.load().unwrap().unwrap();
        assert_eq!(persisted.access_token.expose_secret(), "old-access");
        assert_eq!(persisted.refresh_token.expose_secret(), "retained-refresh");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn refresh_save_failure_does_not_publish_or_persist_rotated_pair() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "rotated-access",
                "token_type": "Bearer",
                "refresh_token": "rotated-refresh"
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
                access_token: SecretString::from("old-access"),
                refresh_token: SecretString::from("retained-refresh"),
                token_type: "Bearer".into(),
                device_token: "retained-device".into(),
                expires_at: Some(0),
            })
            .unwrap();
        set_mode(dir.path(), 0o500);

        let result = client.login_from_cache().await;

        set_mode(dir.path(), 0o700);
        let error = result.unwrap_err();
        assert!(matches!(
            error,
            RhoodError::Io(ref source)
                if source.kind() == std::io::ErrorKind::PermissionDenied
        ));
        assert!(matches!(
            client.auth_state().await,
            AuthState::Unauthenticated
        ));
        let persisted = client.token_cache.load().unwrap().unwrap();
        assert_eq!(persisted.access_token.expose_secret(), "old-access");
        assert_eq!(persisted.refresh_token.expose_secret(), "retained-refresh");
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
                expires_at: Some(Timestamp::now().as_second() + 60),
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
    async fn expired_cached_access_refreshes_directly_with_retained_credentials() {
        const NOW: i64 = 1_700_000_000;

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/positions/"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains("refresh_token=retained-refresh"))
            .and(body_string_contains("device_token=retained-device"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "rotated-access",
                "token_type": "Bearer",
                "refresh_token": "rotated-refresh"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("token.json");
        let mut config = test_config(cache_path.to_str().unwrap());
        config.api.base_url = server.uri();
        let expected_expiry = NOW + config.auth.token_expiry_secs.cast_signed();
        let client = RobinhoodClient::with_config(config).unwrap();
        client
            .token_cache
            .save(&CachedToken {
                access_token: SecretString::from("expired-access"),
                refresh_token: SecretString::from("retained-refresh"),
                token_type: "Bearer".into(),
                device_token: "retained-device".into(),
                expires_at: Some(NOW - 1),
            })
            .unwrap();

        assert!(client.recover_from_cache_at(NOW).await.unwrap());

        let state = client.auth_state().await;
        assert_eq!(
            state.authorization_header().as_deref(),
            Some("Bearer rotated-access")
        );
        assert_eq!(
            state.refresh_token().unwrap().expose_secret(),
            "rotated-refresh"
        );
        assert_eq!(&*client.device_token.read().await, "retained-device");
        let persisted = client.token_cache.load().unwrap().unwrap();
        assert_eq!(persisted.access_token.expose_secret(), "rotated-access");
        assert_eq!(persisted.refresh_token.expose_secret(), "rotated-refresh");
        assert_eq!(persisted.device_token, "retained-device");
        assert_eq!(persisted.expires_at, Some(expected_expiry));
        server.verify().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn login_restores_owner_only_cached_token() {
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
                expires_at: Some(Timestamp::now().as_second() + 60),
            })
            .unwrap();
        set_mode(&cache_path, 0o600);

        client.login("user", "pass", None).await.unwrap();

        assert_eq!(
            client.auth_state().await.authorization_header().as_deref(),
            Some("Bearer cached-access")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn login_rejects_world_readable_cached_token() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("token.json");
        let client =
            RobinhoodClient::with_config(test_config(cache_path.to_str().unwrap())).unwrap();
        client
            .token_cache
            .save(&CachedToken {
                access_token: SecretString::from("cached-access"),
                refresh_token: SecretString::from("cached-refresh"),
                token_type: "Bearer".into(),
                device_token: "cached-device".into(),
                expires_at: Some(Timestamp::now().as_second() + 60),
            })
            .unwrap();
        set_mode(&cache_path, 0o644);

        let error = client.login("user", "pass", None).await.unwrap_err();

        assert!(
            error
                .to_string()
                .contains(&cache_path.display().to_string())
        );
        assert!(error.to_string().contains("chmod 600"));
    }

    #[tokio::test]
    async fn login_falls_through_when_cache_is_absent() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .and(body_string_contains("grant_type=password"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "login-access",
                "token_type": "Bearer",
                "refresh_token": "login-refresh"
            })))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("token.json");
        let mut config = test_config(cache_path.to_str().unwrap());
        config.api.base_url = server.uri();
        let client = RobinhoodClient::with_config(config).unwrap();

        client.login("user", "pass", None).await.unwrap();

        assert_eq!(
            client.auth_state().await.authorization_header().as_deref(),
            Some("Bearer login-access")
        );
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
    async fn login_headless_rejects_non_success_response_with_access_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
                "access_token": "server-error-token"
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
                status: 500,
                message
            } if message.contains("Login failed with no actionable response")
                && !message.contains("server-error-token")
        ));
        assert!(!client.is_authenticated().await);
    }

    #[tokio::test]
    async fn login_headless_handles_mfa_required_on_successful_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "mfa_required": true
            })))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let err = client
            .login_headless("user", "pass", None)
            .await
            .unwrap_err();

        assert!(matches!(err, RhoodError::InvalidParameter(_)));
        assert!(matches!(client.auth_state().await, AuthState::MfaRequired));
    }

    #[tokio::test]
    async fn login_headless_handles_challenge_on_successful_password_grant() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .and(body_string_contains("grant_type=password"))
            .and(body_string_contains("username=user"))
            .and(body_string_contains("password=pass"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "challenge": {
                    "id": "ch-login-1",
                    "type": "sms",
                    "status": "issued"
                }
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
            RhoodError::ChallengeRequired(ChallengeType::Sms)
        ));
        assert!(matches!(
            client.auth_state().await,
            AuthState::Challenged {
                challenge_type: ChallengeType::Sms,
                challenge_id,
            } if challenge_id == "ch-login-1"
        ));
    }

    #[tokio::test]
    async fn login_headless_rejects_non_success_retry_with_access_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "verification_workflow": {
                    "id": "wf-1",
                    "workflow_status": "issued"
                }
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/pathfinder/user_machine/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "machine-1"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/pathfinder/inquiries/machine-1/user_view/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "context": {
                    "sheriff_challenge": {
                        "id": "challenge-1",
                        "type": "prompt",
                        "status": "validated"
                    }
                },
                "type_context": null
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/pathfinder/inquiries/machine-1/user_view/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "context": null,
                "type_context": {
                    "result": "workflow_status_approved"
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token/"))
            .respond_with(ResponseTemplate::new(502).set_body_json(serde_json::json!({
                "access_token": "retry-error-token"
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
                status: 502,
                message
            } if message.contains("Login failed with no actionable response")
                && !message.contains("retry-error-token")
        ));
        assert!(!client.is_authenticated().await);
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
