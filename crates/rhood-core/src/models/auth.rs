use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub(crate) struct RefreshTokenPayload {
    pub client_id: String,
    pub grant_type: &'static str,
    pub refresh_token: String,
    pub scope: &'static str,
    pub device_token: String,
}

#[derive(Serialize)]
pub(crate) struct LoginPayload {
    pub client_id: String,
    pub expires_in: String,
    pub grant_type: &'static str,
    pub username: String,
    pub password: String,
    pub scope: &'static str,
    pub device_token: String,
    pub try_passkeys: &'static str,
    pub token_request_path: &'static str,
    pub create_read_only_secondary_token: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mfa_code: Option<String>,
}

impl fmt::Debug for RefreshTokenPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RefreshTokenPayload")
            .field("client_id", &self.client_id)
            .field("grant_type", &self.grant_type)
            .field("refresh_token", &"[REDACTED]")
            .field("scope", &self.scope)
            .field("device_token", &"[REDACTED]")
            .finish()
    }
}

impl fmt::Debug for LoginPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginPayload")
            .field("client_id", &self.client_id)
            .field("expires_in", &self.expires_in)
            .field("grant_type", &self.grant_type)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .field("scope", &self.scope)
            .field("device_token", &"[REDACTED]")
            .field("try_passkeys", &self.try_passkeys)
            .field("token_request_path", &self.token_request_path)
            .field(
                "create_read_only_secondary_token",
                &self.create_read_only_secondary_token,
            )
            .field("mfa_code", &self.mfa_code.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ChallengeResponsePayload {
    pub response: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct PathfinderMachinePayload {
    pub device_id: String,
    pub flow: &'static str,
    pub input: PathfinderInput,
}

#[derive(Debug, Serialize)]
pub(crate) struct PathfinderInput {
    pub workflow_id: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct WorkflowConfirmPayload {
    pub sequence: u32,
    pub user_input: WorkflowUserInput,
}

#[derive(Debug, Serialize)]
pub(crate) struct WorkflowUserInput {
    pub status: &'static str,
}

#[derive(Deserialize)]
pub(crate) struct OAuthResponse {
    pub access_token: Option<String>,
    pub token_type: Option<String>,
    pub refresh_token: Option<String>,
    #[serde(rename = "expires_in")]
    pub _expires_in: Option<u64>,
    #[serde(rename = "scope")]
    pub _scope: Option<String>,
    #[serde(rename = "user_uuid")]
    pub _user_uuid: Option<String>,
    #[serde(rename = "backup_code")]
    pub _backup_code: Option<String>,
    pub mfa_required: Option<bool>,
    #[serde(rename = "mfa_code")]
    pub _mfa_code: Option<String>,
    pub verification_workflow: Option<VerificationWorkflow>,
    pub challenge: Option<ChallengeDetail>,
    /// Captures error detail from non-success responses (e.g. `{"detail": "..."}`)
    pub detail: Option<String>,
}

impl fmt::Debug for OAuthResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthResponse")
            .field(
                "access_token",
                &self.access_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("token_type", &self.token_type)
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("_expires_in", &self._expires_in)
            .field("_scope", &self._scope)
            .field("_user_uuid", &self._user_uuid)
            .field(
                "_backup_code",
                &self._backup_code.as_ref().map(|_| "[REDACTED]"),
            )
            .field("mfa_required", &self.mfa_required)
            .field("_mfa_code", &self._mfa_code.as_ref().map(|_| "[REDACTED]"))
            .field("verification_workflow", &self.verification_workflow)
            .field("challenge", &self.challenge)
            .field("detail", &self.detail)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{LoginPayload, OAuthResponse, RefreshTokenPayload};

    #[test]
    fn oauth_response_debug_redacts_authentication_values() {
        let response = OAuthResponse {
            access_token: Some("access-token-should-not-appear".into()),
            token_type: Some("Bearer".into()),
            refresh_token: Some("refresh-token-should-not-appear".into()),
            _expires_in: None,
            _scope: None,
            _user_uuid: None,
            _backup_code: Some("backup-code-should-not-appear".into()),
            mfa_required: None,
            _mfa_code: Some("mfa-code-should-not-appear".into()),
            verification_workflow: None,
            challenge: None,
            detail: None,
        };

        let debug = format!("{response:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("access-token-should-not-appear"));
        assert!(!debug.contains("refresh-token-should-not-appear"));
        assert!(!debug.contains("backup-code-should-not-appear"));
        assert!(!debug.contains("mfa-code-should-not-appear"));
    }

    #[test]
    fn credential_payload_debug_redacts_authentication_values() {
        let refresh = RefreshTokenPayload {
            client_id: "client-id".into(),
            grant_type: "refresh_token",
            refresh_token: "refresh-token-should-not-appear".into(),
            scope: "internal",
            device_token: "device-token-should-not-appear".into(),
        };
        let login_with_mfa = LoginPayload {
            client_id: "client-id".into(),
            expires_in: "3600".into(),
            grant_type: "password",
            username: "user@example.com".into(),
            password: "password-should-not-appear".into(),
            scope: "internal",
            device_token: "device-token-should-not-appear".into(),
            try_passkeys: "false",
            token_request_path: "/login",
            create_read_only_secondary_token: "true",
            mfa_code: Some("mfa-code-should-not-appear".into()),
        };
        let login_without_mfa = LoginPayload {
            client_id: "client-id".into(),
            expires_in: "3600".into(),
            grant_type: "password",
            username: "user@example.com".into(),
            password: "password-should-not-appear".into(),
            scope: "internal",
            device_token: "device-token-should-not-appear".into(),
            try_passkeys: "false",
            token_request_path: "/login",
            create_read_only_secondary_token: "true",
            mfa_code: None,
        };

        let refresh_debug = format!("{refresh:?}");
        let login_with_mfa_debug = format!("{login_with_mfa:?}");
        let login_without_mfa_debug = format!("{login_without_mfa:?}");

        for secret in [
            "refresh-token-should-not-appear",
            "device-token-should-not-appear",
            "password-should-not-appear",
            "mfa-code-should-not-appear",
        ] {
            assert!(!refresh_debug.contains(secret));
            assert!(!login_with_mfa_debug.contains(secret));
        }
        assert!(login_with_mfa_debug.contains("mfa_code: Some(\"[REDACTED]\")"));
        assert!(login_without_mfa_debug.contains("mfa_code: None"));
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct VerificationWorkflow {
    pub id: String,
    #[serde(rename = "workflow_status")]
    pub _workflow_status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChallengeDetail {
    pub id: String,
    #[serde(rename = "type")]
    pub challenge_type: String,
    #[serde(rename = "status")]
    pub _status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PathfinderMachineResponse {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PathfinderInquiryResponse {
    pub context: Option<InquiryContext>,
    #[serde(rename = "http_status")]
    pub _http_status: Option<u16>,
    #[serde(rename = "locality")]
    pub _locality: Option<String>,
    #[serde(rename = "page")]
    pub _page: Option<String>,
    #[serde(rename = "polling_interval")]
    pub _polling_interval: Option<u64>,
    #[serde(rename = "prev_state_name")]
    pub _prev_state_name: Option<String>,
    #[serde(rename = "sequence")]
    pub _sequence: Option<u32>,
    #[serde(rename = "should_replace_current_page")]
    pub _should_replace_current_page: Option<bool>,
    #[serde(rename = "state_name")]
    pub _state_name: Option<String>,
    #[serde(rename = "type")]
    pub _response_type: Option<String>,
    pub type_context: Option<InquiryTypeContext>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct InquiryContext {
    #[serde(rename = "fallback_cta_text")]
    pub _fallback_cta_text: Option<String>,
    pub sheriff_challenge: Option<SheriffChallenge>,
    #[serde(rename = "sheriff_flow_id")]
    pub _sheriff_flow_id: Option<String>,
    #[serde(rename = "verification_workflow_id")]
    pub _verification_workflow_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SheriffChallenge {
    pub id: String,
    #[serde(rename = "type")]
    pub challenge_type: String,
    pub status: String,
    #[serde(rename = "expires_at")]
    pub _expires_at: Option<String>,
    #[serde(rename = "remaining_attempts")]
    pub _remaining_attempts: Option<u32>,
    #[serde(rename = "remaining_retries")]
    pub _remaining_retries: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct InquiryTypeContext {
    pub result: Option<String>,
    #[serde(rename = "result_type")]
    pub _result_type: Option<String>,
    #[serde(rename = "page")]
    pub _page: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PushStatusResponse {
    pub challenge_status: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChallengeResponseResult {
    pub status: Option<String>,
}
