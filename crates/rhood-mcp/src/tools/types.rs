use chrono::{DateTime, Utc};
use rhood_core::models::order::{OptionOrderRequest, StockOrderRequest};

/// Tool names that perform write operations (order placement, cancellation).
/// Filtered out of tool listings and blocked from execution in read-only mode.
pub const WRITE_TOOLS: &[&str] = &[
    "place_stock_order",
    "place_option_order",
    "confirm_order",
    "cancel_order",
    "cancel_option_order",
    "create_recurring_investment",
    "update_recurring_investment",
    "cancel_recurring_investment",
    "add_to_watchlist",
    "remove_from_watchlist",
];

/// A staged order awaiting user confirmation before submission.
#[derive(Debug, Clone)]
pub struct PendingOrder {
    pub summary: String,
    pub kind: PendingOrderKind,
    /// When the order was staged. Informational staging metadata.
    pub created_at: DateTime<Utc>,
}

/// The concrete order request wrapped inside a [`PendingOrder`].
#[derive(Debug, Clone)]
pub enum PendingOrderKind {
    Stock(StockOrderRequest),
    Option(OptionOrderRequest),
}

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
            format!("Rate limited — retry after {retry_after_secs}s"),
        ),
        rhood_core::RhoodError::ReadOnlyMode => ("read_only", err.to_string()),
        rhood_core::RhoodError::InvalidParameter(msg) => ("invalid_parameter", msg.clone()),
        rhood_core::RhoodError::Api { status, message } => {
            ("api_error", normalize_api_body(*status, message))
        }
        _ => ("error", err.to_string()),
    };
    serde_json::json!({ "error": code, "message": message }).to_string()
}

/// Turns a raw upstream error body into a clean, LLM-friendly message.
fn normalize_api_body(status: u16, body: &str) -> String {
    let trimmed = body.trim();
    // Named missing instruments/symbols.
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(missing) = value.get("missing_instruments").and_then(|m| m.as_array()) {
            let names: Vec<String> = missing
                .iter()
                .filter_map(|name| name.as_str().map(str::to_string))
                .collect();
            return format!("unknown symbol(s): {}", names.join(", "));
        }
        if let Some(detail) = value
            .get("message")
            .or_else(|| value.get("detail"))
            .and_then(|detail_value| detail_value.as_str())
        {
            return format!("API error ({status}): {detail}");
        }
        if let Some(detail) = value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(|error_value| error_value.as_str())
        {
            return format!("API error ({status}): {detail}");
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
}
