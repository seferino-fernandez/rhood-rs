//! Error types for the `rhood-core` crate.
//!
//! All fallible operations return [`RhoodError`] through the crate-level
//! [`Result`](crate::Result) type alias.

use thiserror::Error;

/// Formats an API error for display.
///
/// If the body is JSON containing a top-level `"message"` or `"detail"` field,
/// extracts it for a cleaner user-facing message. Otherwise falls back to
/// the raw body.
fn display_api_error(status: u16, body: &str) -> String {
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) {
        // Try common error shapes: {"message": "..."}, {"detail": "..."}, {"error": {"message": "..."}}
        if let Some(msg) = parsed
            .get("message")
            .or_else(|| parsed.get("detail"))
            .and_then(|val| val.as_str())
        {
            return format!("API error ({status}): {msg}");
        }
        if let Some(msg) = parsed
            .get("error")
            .and_then(|err| err.get("message"))
            .and_then(|val| val.as_str())
        {
            return format!("API error ({status}): {msg}");
        }
    }
    format!("API error ({status}): {body}")
}

/// The type of authentication challenge issued by Robinhood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChallengeType {
    /// An SMS code was sent to the user's phone.
    Sms,
    /// A verification code was sent to the user's email.
    Email,
    /// A push notification was sent to the Robinhood mobile app.
    Prompt,
}

impl std::fmt::Display for ChallengeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sms => write!(f, "SMS"),
            Self::Email => write!(f, "email"),
            Self::Prompt => write!(f, "app prompt"),
        }
    }
}

/// Errors that can occur when interacting with the Robinhood API.
///
/// # Example
///
/// ```no_run
/// use rhood_core::{RobinhoodClient, RhoodError};
///
/// # async fn run(username: &str, password: &str) -> Result<(), RhoodError> {
/// let client = RobinhoodClient::new()?;
/// match client.login(username, password, None).await {
///     Ok(()) => println!("authenticated"),
///     Err(RhoodError::ChallengeRequired(challenge_type)) => {
///         println!("verification needed via {challenge_type}");
///     }
///     Err(RhoodError::DeviceVerificationRequired) => {
///         println!("run `rhood login` interactively first");
///     }
///     Err(other) => return Err(other),
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Error)]
pub enum RhoodError {
    /// The client is not authenticated and cannot make API calls.
    #[error("Not authenticated — run `rhood login` first")]
    NotAuthenticated,

    /// The server issued an authentication challenge that must be answered.
    #[error("Authentication challenge required: {0}")]
    ChallengeRequired(ChallengeType),

    /// The access token has expired and automatic refresh failed.
    #[error("Token expired and refresh failed")]
    TokenExpired,

    /// The Robinhood API returned a non-success HTTP status.
    #[error("{}", display_api_error(*.status, message))]
    Api {
        /// HTTP status code from the API response.
        status: u16,
        /// Human-readable error message or response body.
        message: String,
    },

    /// The API returned HTTP 429, indicating the client should back off.
    #[error("Rate limited — retry after {retry_after_secs}s")]
    RateLimited {
        /// Suggested number of seconds to wait before retrying.
        retry_after_secs: u64,
    },

    /// The requested ticker symbol was not found.
    #[error("Symbol not found: {0}")]
    InvalidSymbol(String),

    /// A parameter provided to an API method was invalid.
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    /// A write operation was attempted while the client is in read-only mode.
    #[error("Operation blocked — client is in read-only mode")]
    ReadOnlyMode,

    /// An order request contains invalid or contradictory parameters.
    #[error("Invalid order: {0}")]
    InvalidOrder(String),

    /// Device verification is required before the client can authenticate.
    #[error("Device verification required — run `rhood login` interactively first")]
    DeviceVerificationRequired,

    /// An HTTP transport error from the underlying HTTP client.
    #[error(transparent)]
    Http(#[from] reqwest::Error),

    /// A JSON serialization or deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// A filesystem I/O error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// An operation timed out waiting for a response or approval.
    #[error("Timeout: {0}")]
    Timeout(String),
}

#[cfg(test)]
mod tests {
    use super::RhoodError;

    #[test]
    fn rhood_error_display_messages() {
        let err = RhoodError::NotAuthenticated;
        assert!(err.to_string().contains("Not authenticated"));

        let err = RhoodError::ReadOnlyMode;
        assert!(err.to_string().contains("read-only"));

        let err = RhoodError::InvalidSymbol("XYZ".into());
        assert!(err.to_string().contains("XYZ"));

        let err = RhoodError::RateLimited {
            retry_after_secs: 30,
        };
        assert!(err.to_string().contains("30"));

        let err = RhoodError::Api {
            status: 404,
            message: "Not found".into(),
        };
        assert!(err.to_string().contains("404"));
        assert!(err.to_string().contains("Not found"));
    }

    #[test]
    fn api_error_display_extracts_json_message() {
        let err = RhoodError::Api {
            status: 404,
            message: r#"{"code":5,"message":"futures contract not found","details":[]}"#.into(),
        };
        let display = err.to_string();
        assert_eq!(display, "API error (404): futures contract not found");
    }

    #[test]
    fn api_error_display_extracts_nested_error_message() {
        let err = RhoodError::Api {
            status: 400,
            message: r#"{"status":"FAILURE","error":{"code":3,"message":"invalid argument"}}"#
                .into(),
        };
        let display = err.to_string();
        assert_eq!(display, "API error (400): invalid argument");
    }

    #[test]
    fn api_error_display_extracts_detail_field() {
        let err = RhoodError::Api {
            status: 403,
            message: r#"{"detail":"Permission denied"}"#.into(),
        };
        assert_eq!(err.to_string(), "API error (403): Permission denied");
    }

    #[test]
    fn api_error_display_falls_back_to_raw_body() {
        let err = RhoodError::Api {
            status: 500,
            message: "Internal server error".into(),
        };
        assert_eq!(err.to_string(), "API error (500): Internal server error");
    }
}
