use serde::Deserialize;

use crate::oauth::store::OAuthStore;

#[derive(Debug, Deserialize)]
pub struct ApprovalForm {
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: String,
    pub state: Option<String>,
    pub code_challenge: String,
    #[allow(dead_code)] // validated on GET /authorize; carried through form for completeness
    pub code_challenge_method: String,
    pub approved: String,
    pub pin: Option<String>,
    pub csrf_nonce: String,
}

#[derive(serde::Serialize)]
pub struct AuthorizationServerMetadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: String,
    pub scopes_supported: Vec<String>,
    pub response_types_supported: Vec<String>,
    pub grant_types_supported: Vec<String>,
    pub code_challenge_methods_supported: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AuthorizeQuery {
    pub response_type: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: Option<String>,
    pub state: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
}

/// Shared state for all OAuth endpoint handlers.
#[derive(Clone)]
pub struct OAuthEndpointState {
    pub store: OAuthStore,
    pub base_url: String,
    pub server_read_only: bool,
    pub oauth_pin: Option<String>,
}

#[derive(serde::Serialize)]
pub struct ProtectedResourceMetadata {
    pub resource: String,
    pub authorization_servers: Vec<String>,
    pub scopes_supported: Vec<String>,
    pub bearer_methods_supported: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RegistrationRequest {
    pub client_name: Option<String>,
    pub redirect_uris: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct TokenRequest {
    pub grant_type: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub redirect_uri: String,
    #[serde(default)]
    pub code_verifier: Option<String>,
    #[serde(default)]
    pub refresh_token: String,
}
