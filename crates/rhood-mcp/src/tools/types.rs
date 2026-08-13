/// Convert a [`RhoodError`](rhood_core::RhoodError) into a structured JSON error string
/// suitable for MCP tool responses.
pub fn format_tool_error(err: &rhood_core::RhoodError) -> String {
    let (code, message) = match err {
        rhood_core::RhoodError::NotAuthenticated => ("not_authenticated", err.to_string()),
        rhood_core::RhoodError::InvalidSymbol(sym) => {
            ("invalid_symbol", format!("Symbol not found: {sym}"))
        }
        rhood_core::RhoodError::RateLimited { retry_after_secs } => (
            "rate_limited",
            format!("Rate limited - retry after {retry_after_secs}s"),
        ),
        rhood_core::RhoodError::ReadOnlyMode => ("read_only", err.to_string()),
        rhood_core::RhoodError::InvalidParameter(msg) => ("invalid_parameter", msg.clone()),
        rhood_core::RhoodError::StagedOrderExpired => ("staged_order_expired", err.to_string()),
        rhood_core::RhoodError::SubmissionUncertain { .. } => {
            ("submission_uncertain", err.to_string())
        }
        rhood_core::RhoodError::NoEligibleAccount => ("no_eligible_account", err.to_string()),
        rhood_core::RhoodError::UnknownAccount { .. } => ("unknown_account", err.to_string()),
        rhood_core::RhoodError::AmbiguousAccount { .. } => ("ambiguous_account", err.to_string()),
        rhood_core::RhoodError::InvalidAccountResponse(_) => {
            ("invalid_account_response", err.to_string())
        }
        rhood_core::RhoodError::RequestChunkFailed { source, .. } => {
            (tool_error_code(source), err.to_string())
        }
        rhood_core::RhoodError::InvalidMarketDataResponse { .. } => {
            ("invalid_market_data_response", err.to_string())
        }
        rhood_core::RhoodError::Api { status, message } => {
            ("api_error", normalize_api_body(*status, message))
        }
        _ => ("error", err.to_string()),
    };
    serde_json::json!({ "error": code, "message": message }).to_string()
}

fn tool_error_code(err: &rhood_core::RhoodError) -> &'static str {
    match err {
        rhood_core::RhoodError::NotAuthenticated => "not_authenticated",
        rhood_core::RhoodError::InvalidSymbol(_) => "invalid_symbol",
        rhood_core::RhoodError::RateLimited { .. } => "rate_limited",
        rhood_core::RhoodError::ReadOnlyMode => "read_only",
        rhood_core::RhoodError::InvalidParameter(_) => "invalid_parameter",
        rhood_core::RhoodError::StagedOrderExpired => "staged_order_expired",
        rhood_core::RhoodError::SubmissionUncertain { .. } => "submission_uncertain",
        rhood_core::RhoodError::NoEligibleAccount => "no_eligible_account",
        rhood_core::RhoodError::UnknownAccount { .. } => "unknown_account",
        rhood_core::RhoodError::AmbiguousAccount { .. } => "ambiguous_account",
        rhood_core::RhoodError::InvalidAccountResponse(_) => "invalid_account_response",
        rhood_core::RhoodError::RequestChunkFailed { source, .. } => tool_error_code(source),
        rhood_core::RhoodError::InvalidMarketDataResponse { .. } => "invalid_market_data_response",
        rhood_core::RhoodError::Api { .. } => "api_error",
        _ => "error",
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
}
