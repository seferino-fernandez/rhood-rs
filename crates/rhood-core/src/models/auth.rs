use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub(crate) struct RefreshTokenPayload {
    pub client_id: String,
    pub grant_type: &'static str,
    pub refresh_token: String,
    pub scope: &'static str,
    pub device_token: String,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Deserialize)]
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
