/// Convert a [`RhoodError`](rhood_core::RhoodError) into a structured JSON error string
/// suitable for MCP tool responses.
pub fn format_tool_error(err: &rhood_core::RhoodError) -> String {
    let code = tool_error_code(err);
    let message = tool_error_message(err);
    serde_json::json!({ "error": code, "message": message }).to_string()
}

/// The stable, machine-readable code a model matches on.
///
/// Deliberately exhaustive: `RhoodError` is not `#[non_exhaustive]`, so
/// omitting the wildcard makes a new core variant a compile error here rather
/// than a silent degradation to a generic code. Every failure a model could
/// respond to differently must be distinguishable without parsing prose.
fn tool_error_code(err: &rhood_core::RhoodError) -> &'static str {
    match err {
        rhood_core::RhoodError::NotAuthenticated => "not_authenticated",
        rhood_core::RhoodError::ChallengeRequired(_) => "challenge_required",
        rhood_core::RhoodError::TokenExpired => "token_expired",
        rhood_core::RhoodError::DeviceVerificationRequired => "device_verification_required",
        rhood_core::RhoodError::InvalidSymbol(_) => "invalid_symbol",
        rhood_core::RhoodError::RateLimited { .. } => "rate_limited",
        rhood_core::RhoodError::ReadOnlyMode => "read_only",
        rhood_core::RhoodError::InvalidParameter(_) => "invalid_parameter",
        rhood_core::RhoodError::InvalidOrder(_) => "invalid_order",
        rhood_core::RhoodError::StagedOrderExpired => "staged_order_expired",
        rhood_core::RhoodError::SubmissionUncertain { .. } => "submission_uncertain",
        rhood_core::RhoodError::NoEligibleAccount => "no_eligible_account",
        rhood_core::RhoodError::UnknownAccount { .. } => "unknown_account",
        rhood_core::RhoodError::AmbiguousAccount { .. } => "ambiguous_account",
        rhood_core::RhoodError::InvalidAccountResponse(_) => "invalid_account_response",
        rhood_core::RhoodError::RequestChunkFailed { source, .. } => tool_error_code(source),
        rhood_core::RhoodError::InvalidMarketDataResponse { .. } => "invalid_market_data_response",
        rhood_core::RhoodError::Api { .. } => "api_error",
        rhood_core::RhoodError::Http(_) => "network_error",
        rhood_core::RhoodError::Timeout(_) => "timeout",
        // Serialization and filesystem failures are ours, not the caller's.
        // They share one code and carry no source detail: a model cannot act
        // on a serde position or a local path, and neither belongs in a tool
        // response.
        rhood_core::RhoodError::Json(_) | rhood_core::RhoodError::Io(_) => "internal_error",
    }
}

/// The human-readable half of a tool error.
///
/// Most variants render themselves. The exceptions either sharpen the wording
/// for a model, or withhold an internal source that must not leave the server.
fn tool_error_message(err: &rhood_core::RhoodError) -> String {
    match err {
        rhood_core::RhoodError::InvalidSymbol(sym) => format!("Symbol not found: {sym}"),
        rhood_core::RhoodError::RateLimited { retry_after_secs } => {
            format!("Rate limited - retry after {retry_after_secs}s")
        }
        rhood_core::RhoodError::InvalidParameter(msg) => msg.clone(),
        rhood_core::RhoodError::Api { status, message } => normalize_api_body(*status, message),
        rhood_core::RhoodError::Json(_) | rhood_core::RhoodError::Io(_) => {
            "The server could not complete the request due to an internal error".to_string()
        }
        _ => err.to_string(),
    }
}

/// Turns a raw upstream error body into a clean, LLM-friendly message.
fn normalize_api_body(status: u16, body: &str) -> String {
    let trimmed = body.trim();
    if let Some(detail) = rhood_core::error::extract_api_error_detail(trimmed) {
        match detail {
            rhood_core::error::ApiErrorDetail::MissingInstruments(symbols) => {
                return format!("unknown symbol(s): {}", symbols.join(", "));
            }
            rhood_core::error::ApiErrorDetail::Message(message) => {
                return format!("API error ({status}): {message}");
            }
            rhood_core::error::ApiErrorDetail::UnusableMissingInstruments => {
                return format!(
                    "API error ({status}): upstream reported unrecognized instruments without usable symbols"
                );
            }
            _ => {
                return format!(
                    "API error ({status}): upstream returned an unrecognized diagnostic"
                );
            }
        }
    }
    // HTML error page -> don't dump markup.
    let looks_like_html =
        trimmed.starts_with('<') || trimmed.to_ascii_lowercase().contains("<!doctype html");
    if looks_like_html {
        return format!("API error ({status}): upstream returned an HTML error page");
    }
    format!("API error ({status}): {trimmed}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhood_core::RhoodError;

    fn msg(err: RhoodError) -> String {
        let s = format_tool_error(&err);
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        v["message"].as_str().unwrap().to_string()
    }

    #[test]
    fn names_missing_instruments() {
        let m = msg(RhoodError::Api {
            status: 404,
            message: r#"{"missing_instruments":["NOTAREALSYM"]}"#.into(),
        });
        assert!(m.contains("NOTAREALSYM"), "got: {m}");
        assert!(!m.contains('{'), "should not leak raw JSON: {m}");
    }

    #[test]
    fn collapses_html_body() {
        let m = msg(RhoodError::Api {
            status: 404,
            message: "<!DOCTYPE html><html><title>Not Found</title></html>".into(),
        });
        assert!(!m.contains('<'), "should not leak HTML: {m}");
        assert!(m.contains("404"));
    }

    #[test]
    fn extracts_nested_error_message() {
        let m = msg(RhoodError::Api {
            status: 400,
            message: r#"{"status":"FAILURE","error":{"code":3,"message":"invalid argument"}}"#
                .into(),
        });
        assert_eq!(m, "API error (400): invalid argument");
        assert!(!m.contains('{'), "should not leak raw JSON: {m}");
    }

    #[test]
    fn uses_detail_when_missing_instruments_is_empty() {
        let m = msg(RhoodError::Api {
            status: 400,
            message: r#"{"missing_instruments":[],"detail":"insufficient buying power"}"#.into(),
        });
        assert_eq!(m, "API error (400): insufficient buying power");
    }

    #[test]
    fn sanitizes_unusable_missing_instruments() {
        for message in [
            r#"{"missing_instruments":[]}"#,
            r#"{"missing_instruments":[17,{"symbol":"NOTAREALSYM"}]}"#,
        ] {
            let m = msg(RhoodError::Api {
                status: 400,
                message: message.into(),
            });
            assert_eq!(
                m,
                "API error (400): upstream reported unrecognized instruments without usable symbols"
            );
            assert!(!m.contains('{'), "raw JSON leaked: {m}");
        }
    }

    #[test]
    fn extracts_top_level_detail_and_message() {
        let detail = msg(RhoodError::Api {
            status: 403,
            message: r#"{"detail":"Permission denied"}"#.into(),
        });
        assert_eq!(detail, "API error (403): Permission denied");

        let message = msg(RhoodError::Api {
            status: 404,
            message: r#"{"message":"futures contract not found"}"#.into(),
        });
        assert_eq!(message, "API error (404): futures contract not found");
    }

    #[test]
    fn preserves_non_json_body() {
        let m = msg(RhoodError::Api {
            status: 500,
            message: "Internal server error".into(),
        });
        assert_eq!(m, "API error (500): Internal server error");
    }

    #[test]
    fn wrapped_chunk_failure_preserves_rate_limit_category_and_context() {
        let error = RhoodError::RequestChunkFailed {
            endpoint: "quotes".into(),
            chunk_index: 2,
            total_chunks: 3,
            source: Box::new(RhoodError::RateLimited {
                retry_after_secs: 17,
            }),
        };
        let formatted: serde_json::Value =
            serde_json::from_str(&format_tool_error(&error)).unwrap();

        assert_eq!(formatted["error"], "rate_limited");
        assert_eq!(
            formatted["message"],
            "quotes request chunk 2 of 3 failed: Rate limited, retry after 17s"
        );
    }

    #[test]
    fn wrapped_chunk_failure_preserves_invalid_market_data_category() {
        let error = RhoodError::RequestChunkFailed {
            endpoint: "fundamentals".into(),
            chunk_index: 1,
            total_chunks: 1,
            source: Box::new(RhoodError::InvalidMarketDataResponse {
                endpoint: "fundamentals".into(),
                message: "response item omitted its requested symbol".into(),
            }),
        };
        let formatted: serde_json::Value =
            serde_json::from_str(&format_tool_error(&error)).unwrap();

        assert_eq!(formatted["error"], "invalid_market_data_response");
        assert_eq!(
            formatted["message"],
            "fundamentals request chunk 1 of 1 failed: Invalid fundamentals response: response item omitted its requested symbol"
        );
    }

    fn code(err: &RhoodError) -> String {
        let formatted: serde_json::Value = serde_json::from_str(&format_tool_error(err)).unwrap();
        formatted["error"].as_str().unwrap().to_string()
    }

    /// Compile-forcing guard: matches `RhoodError` without a wildcard, so a new
    /// core variant fails to compile here until someone decides what code a
    /// model should see for it. `RhoodError` is not `#[non_exhaustive]`, which
    /// is what makes this possible; before BL-72 the production mapper ended in
    /// `_ => "error"` and eight variants quietly shared that one code.
    #[test]
    fn every_core_error_maps_to_its_own_intended_code() {
        let expectations: Vec<(RhoodError, &str)> = vec![
            (RhoodError::NotAuthenticated, "not_authenticated"),
            (
                RhoodError::ChallengeRequired(rhood_core::error::ChallengeType::Sms),
                "challenge_required",
            ),
            (RhoodError::TokenExpired, "token_expired"),
            (
                RhoodError::DeviceVerificationRequired,
                "device_verification_required",
            ),
            (RhoodError::InvalidSymbol("ZZZZ".into()), "invalid_symbol"),
            (
                RhoodError::RateLimited {
                    retry_after_secs: 5,
                },
                "rate_limited",
            ),
            (RhoodError::ReadOnlyMode, "read_only"),
            (
                RhoodError::InvalidParameter("bad".into()),
                "invalid_parameter",
            ),
            (
                RhoodError::InvalidOrder("bad symbol".into()),
                "invalid_order",
            ),
            (RhoodError::StagedOrderExpired, "staged_order_expired"),
            (
                RhoodError::SubmissionUncertain {
                    client_ref: "ref".into(),
                    message: "timed out".into(),
                },
                "submission_uncertain",
            ),
            (RhoodError::NoEligibleAccount, "no_eligible_account"),
            (
                RhoodError::UnknownAccount {
                    account: "****C123".into(),
                },
                "unknown_account",
            ),
            (
                RhoodError::AmbiguousAccount {
                    choices: "individual ****C123, ira ****D456".into(),
                },
                "ambiguous_account",
            ),
            (
                RhoodError::InvalidAccountResponse("bad".into()),
                "invalid_account_response",
            ),
            (
                RhoodError::InvalidMarketDataResponse {
                    endpoint: "quotes".into(),
                    message: "no id".into(),
                },
                "invalid_market_data_response",
            ),
            (
                RhoodError::Api {
                    status: 400,
                    message: "{}".into(),
                },
                "api_error",
            ),
            (RhoodError::Timeout("slow".into()), "timeout"),
            (
                RhoodError::Json(serde_json::from_str::<i32>("nope").unwrap_err()),
                "internal_error",
            ),
            (
                RhoodError::Io(std::io::Error::other("disk")),
                "internal_error",
            ),
        ];

        for (error, expected) in &expectations {
            assert_eq!(&code(error), expected, "wrong code for {error:?}");
            // Exhaustiveness lives here: adding a variant to `RhoodError`
            // without extending this match stops the test compiling.
            match error {
                RhoodError::NotAuthenticated
                | RhoodError::ChallengeRequired(_)
                | RhoodError::TokenExpired
                | RhoodError::DeviceVerificationRequired
                | RhoodError::InvalidSymbol(_)
                | RhoodError::RateLimited { .. }
                | RhoodError::ReadOnlyMode
                | RhoodError::InvalidParameter(_)
                | RhoodError::InvalidOrder(_)
                | RhoodError::StagedOrderExpired
                | RhoodError::SubmissionUncertain { .. }
                | RhoodError::NoEligibleAccount
                | RhoodError::UnknownAccount { .. }
                | RhoodError::AmbiguousAccount { .. }
                | RhoodError::InvalidAccountResponse(_)
                | RhoodError::RequestChunkFailed { .. }
                | RhoodError::InvalidMarketDataResponse { .. }
                | RhoodError::Api { .. }
                | RhoodError::Http(_)
                | RhoodError::Timeout(_)
                | RhoodError::Json(_)
                | RhoodError::Io(_) => {}
            }
        }

        let distinct: std::collections::HashSet<&str> =
            expectations.iter().map(|(_, expected)| *expected).collect();
        assert_eq!(
            distinct.len(),
            expectations.len().saturating_sub(1),
            "only the two internal variants may share a code"
        );
    }

    /// Order validation is the largest consumer of `InvalidOrder`, and BL-63
    /// routed symbol validation through it too. Before BL-72 every one of these
    /// reached a model as `"error"`.
    #[test]
    fn order_validation_failures_are_distinguishable_from_infrastructure() {
        let validation = code(&RhoodError::InvalidOrder(
            "symbol must contain 1 to 12 ASCII letters, numbers, or periods".into(),
        ));
        assert_eq!(validation, "invalid_order");
        assert_ne!(validation, code(&RhoodError::Timeout("slow".into())));
        assert_ne!(validation, code(&RhoodError::TokenExpired));
    }

    #[test]
    fn internal_failures_do_not_leak_their_source() {
        let formatted: serde_json::Value = serde_json::from_str(&format_tool_error(
            &RhoodError::Io(std::io::Error::other("/Users/someone/.rhood/.rhood-token")),
        ))
        .unwrap();

        assert_eq!(formatted["error"], "internal_error");
        let message = formatted["message"].as_str().unwrap();
        assert!(
            !message.contains(".rhood-token"),
            "internal source escaped to the model: {message}"
        );
    }
}
