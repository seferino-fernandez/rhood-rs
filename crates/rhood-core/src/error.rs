//! Error types for the `rhood-core` crate.
//!
//! All fallible operations return [`RhoodError`] through the crate-level
//! [`Result`](crate::Result) type alias.

use thiserror::Error;

/// Structured diagnostic content extracted from a JSON API error body.
///
/// Callers must include a wildcard arm because additional recognized
/// diagnostics may be added in future releases.
///
/// ```compile_fail
/// use rhood_core::error::ApiErrorDetail;
///
/// fn render(detail: ApiErrorDetail) {
///     match detail {
///         ApiErrorDetail::MissingInstruments(_) => {}
///         ApiErrorDetail::Message(_) => {}
///     }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ApiErrorDetail {
    /// Instrument symbols the API did not recognize.
    MissingInstruments(Vec<String>),
    /// A human-readable diagnostic supplied by the API.
    Message(String),
}

/// Extracts a recognized diagnostic from a JSON API error body.
///
/// Non-empty `missing_instruments` takes precedence, followed by top-level
/// `message`, top-level `detail`, and nested `error.message`. Returns `None`
/// when the body is not JSON or contains none of those diagnostics.
pub fn extract_api_error_detail(body: &str) -> Option<ApiErrorDetail> {
    let parsed = serde_json::from_str::<serde_json::Value>(body).ok()?;

    let missing_instruments = parsed
        .get("missing_instruments")
        .and_then(serde_json::Value::as_array)
        .map(|missing| {
            missing
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|missing| !missing.is_empty());
    if let Some(missing_instruments) = missing_instruments {
        return Some(ApiErrorDetail::MissingInstruments(missing_instruments));
    }

    parsed
        .get("message")
        .or_else(|| parsed.get("detail"))
        .and_then(serde_json::Value::as_str)
        .map(|message| ApiErrorDetail::Message(message.to_owned()))
        .or_else(|| {
            parsed
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(serde_json::Value::as_str)
                .map(|message| ApiErrorDetail::Message(message.to_owned()))
        })
}

/// Formats an API error for display.
///
/// If the body is JSON containing an allowlisted diagnostic field, extracts it
/// for a cleaner user-facing message. Otherwise falls back to the sanitized
/// diagnostic message.
fn display_api_error(status: u16, body: &str) -> String {
    if let Some(detail) = extract_api_error_detail(body) {
        #[expect(
            unreachable_patterns,
            reason = "keeps in-crate rendering forward-compatible with non-exhaustive ApiErrorDetail"
        )]
        let message = match detail {
            ApiErrorDetail::MissingInstruments(symbols) => {
                format!("unknown symbol(s): {}", symbols.join(", "))
            }
            ApiErrorDetail::Message(message) => message,
            _ => "upstream returned an unrecognized diagnostic".to_string(),
        };
        return format!("API error ({status}): {message}");
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
    #[error("Not authenticated, run `rhood login` first")]
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
        /// Sanitized API diagnostic or locally constructed error context.
        message: String,
    },

    /// The API returned HTTP 429, indicating the client should back off.
    #[error("Rate limited, retry after {retry_after_secs}s")]
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
    #[error("Operation blocked, client is in read-only mode")]
    ReadOnlyMode,

    /// An order request contains invalid or contradictory parameters.
    #[error("Invalid order: {0}")]
    InvalidOrder(String),

    /// Device verification is required before the client can authenticate.
    #[error("Device verification required, run `rhood login` interactively first")]
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
    fn api_error_display_extracts_missing_instruments() {
        let err = RhoodError::Api {
            status: 404,
            message: r#"{"missing_instruments":["NOTAREALSYM"]}"#.into(),
        };
        assert_eq!(
            err.to_string(),
            "API error (404): unknown symbol(s): NOTAREALSYM"
        );
    }

    #[test]
    fn api_error_display_uses_detail_when_missing_instruments_is_empty() {
        let err = RhoodError::Api {
            status: 400,
            message: r#"{"missing_instruments":[],"detail":"insufficient buying power"}"#.into(),
        };
        assert_eq!(
            err.to_string(),
            "API error (400): insufficient buying power"
        );
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
