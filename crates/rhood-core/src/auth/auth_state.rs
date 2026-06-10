//! Authentication state machine for the Robinhood login flow.
//!
//! [`AuthState`] models every phase of the multi-step authentication process,
//! from unauthenticated through various challenge types to fully authenticated.

use secrecy::{ExposeSecret, SecretString};

use crate::ChallengeType;

/// Represents the current phase of Robinhood authentication.
///
/// Transitions follow the pattern:
/// `Unauthenticated` -> (`Challenged` | `MfaRequired` | `DeviceVerification`) -> `Authenticated`.
#[derive(Clone)]
pub enum AuthState {
    /// Indicates that no authentication attempt has been made or credentials have been cleared.
    Unauthenticated,
    /// Indicates the server issued a challenge that must be answered before authentication can proceed.
    Challenged {
        /// The type of challenge issued (e.g., SMS or email).
        challenge_type: ChallengeType,
        /// The server-assigned identifier for this challenge.
        challenge_id: String,
    },
    /// Indicates the server requires a multi-factor authentication code (e.g., TOTP).
    MfaRequired,
    /// Indicates the server requires device verification before proceeding.
    DeviceVerification {
        /// The server-assigned workflow identifier for the device verification flow.
        workflow_id: String,
    },
    /// Indicates successful authentication with valid OAuth tokens.
    Authenticated {
        /// The OAuth access token used to authorize API requests.
        access_token: SecretString,
        /// The token type prefix for the `Authorization` header (typically `"Bearer"`).
        token_type: String,
        /// The OAuth refresh token used to obtain a new access token when the current one expires.
        refresh_token: SecretString,
    },
}

impl std::fmt::Debug for AuthState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthenticated => write!(formatter, "Unauthenticated"),
            Self::Challenged { challenge_type, .. } => formatter
                .debug_struct("Challenged")
                .field("challenge_type", challenge_type)
                .finish(),
            Self::MfaRequired => write!(formatter, "MfaRequired"),
            Self::DeviceVerification { workflow_id } => formatter
                .debug_struct("DeviceVerification")
                .field("workflow_id", workflow_id)
                .finish(),
            Self::Authenticated { token_type, .. } => formatter
                .debug_struct("Authenticated")
                .field("access_token", &"[REDACTED]")
                .field("token_type", token_type)
                .field("refresh_token", &"[REDACTED]")
                .finish(),
        }
    }
}

impl AuthState {
    /// Returns `true` if the state is [`AuthState::Authenticated`].
    pub fn is_authenticated(&self) -> bool {
        matches!(self, Self::Authenticated { .. })
    }

    /// Returns the formatted `Authorization` header value (e.g., `"Bearer <token>"`),
    /// or `None` if the state is not [`AuthState::Authenticated`].
    pub fn authorization_header(&self) -> Option<String> {
        match self {
            Self::Authenticated {
                access_token,
                token_type,
                ..
            } => Some(format!("{token_type} {}", access_token.expose_secret())),
            _ => None,
        }
    }

    /// Returns a reference to the OAuth refresh token, or `None` if the state
    /// is not [`AuthState::Authenticated`].
    pub fn refresh_token(&self) -> Option<&SecretString> {
        match self {
            Self::Authenticated { refresh_token, .. } => Some(refresh_token),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unauthenticated_state() {
        let state = AuthState::Unauthenticated;
        assert!(!state.is_authenticated());
        assert!(state.authorization_header().is_none());
        assert!(state.refresh_token().is_none());
    }

    #[test]
    fn authenticated_state() {
        let state = AuthState::Authenticated {
            access_token: SecretString::from("tok123"),
            token_type: "Bearer".into(),
            refresh_token: SecretString::from("ref456"),
        };
        assert!(state.is_authenticated());
        assert_eq!(state.authorization_header().unwrap(), "Bearer tok123");
        assert_eq!(state.refresh_token().unwrap().expose_secret(), "ref456");
    }

    #[test]
    fn challenged_state() {
        let state = AuthState::Challenged {
            challenge_type: ChallengeType::Sms,
            challenge_id: "abc".into(),
        };
        assert!(!state.is_authenticated());
        assert!(state.authorization_header().is_none());
    }

    #[test]
    fn mfa_required_state() {
        let state = AuthState::MfaRequired;
        assert!(!state.is_authenticated());
    }

    #[test]
    fn device_verification_state() {
        let state = AuthState::DeviceVerification {
            workflow_id: "wf-123".into(),
        };
        assert!(!state.is_authenticated());
        let debug = format!("{state:?}");
        assert!(debug.contains("wf-123"));
    }

    #[test]
    fn debug_redacts_tokens() {
        let state = AuthState::Authenticated {
            access_token: SecretString::from("super_secret"),
            token_type: "Bearer".into(),
            refresh_token: SecretString::from("refresh_secret"),
        };
        let debug = format!("{state:?}");
        assert!(!debug.contains("super_secret"));
        assert!(!debug.contains("refresh_secret"));
        assert!(debug.contains("[REDACTED]"));
    }
}
