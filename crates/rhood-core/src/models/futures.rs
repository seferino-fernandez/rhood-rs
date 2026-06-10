//! Futures-related model types for the Robinhood API.
//!
//! Contains structs for futures contracts, quotes, orders, and account
//! discovery. Read-only: no order-placement endpoints are implemented.

use serde::{Deserialize, Serialize};

/// Wrapper for the futures contract API response.
///
/// The API returns `{"result": {...}}` rather than the contract directly.
#[derive(Debug, Deserialize)]
pub struct FuturesContractWrapper {
    /// The unwrapped contract data.
    pub result: FuturesContract,
}

/// Represents a futures contract from the Robinhood API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesContract {
    /// Unique identifier for the contract.
    pub id: Option<String>,
    /// Full contract symbol (e.g., "/ESH26:XCME").
    pub symbol: Option<String>,
    /// Shortened display symbol (e.g., "/ESH26").
    #[serde(rename = "displaySymbol")]
    pub display_symbol: Option<String>,
    /// Human-readable description of the contract.
    pub description: Option<String>,
    /// Contract multiplier (e.g., "50" for E-mini S&P 500).
    pub multiplier: Option<String>,
    /// Contract expiration date.
    pub expiration: Option<String>,
    /// Tradability status of the contract.
    pub tradability: Option<String>,
    /// Current state of the contract (e.g., "active").
    pub state: Option<String>,
}

/// Wrapper for the futures quote API response.
///
/// The API returns `{"data": [{"data": {...}}]}` with double nesting.
#[derive(Debug, Deserialize)]
pub struct FuturesQuoteDataWrapper {
    /// Outer data array.
    pub data: Vec<FuturesQuoteDataItem>,
}

/// Inner wrapper for a single futures quote data item.
#[derive(Debug, Deserialize)]
pub struct FuturesQuoteDataItem {
    /// The unwrapped quote data.
    pub data: FuturesQuote,
}

/// Represents a real-time futures quote from the Robinhood API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesQuote {
    /// Full contract symbol echoed by the quote (e.g. "/ESM26:XCME").
    pub symbol: Option<String>,
    /// Current bid price.
    pub bid_price: Option<String>,
    /// Number of contracts at the bid price.
    pub bid_size: Option<i64>,
    /// Current ask price.
    pub ask_price: Option<String>,
    /// Number of contracts at the ask price.
    pub ask_size: Option<i64>,
    /// Price of the most recent trade.
    pub last_trade_price: Option<String>,
    /// Size of the most recent trade.
    pub last_trade_size: Option<i64>,
    /// Current state of the quote (e.g., "active").
    pub state: Option<String>,
    /// Timestamp when the quote was last updated.
    pub updated_at: Option<String>,
    /// Identifier of the underlying futures instrument.
    pub instrument_id: Option<String>,
}

/// Represents a futures order from the Robinhood API.
///
/// Uses `serde_json::Value` for nested P&L and fee structures to preserve
/// all data without requiring exhaustive struct definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrder {
    /// Unique identifier for the order.
    pub order_id: Option<String>,
    /// Current state of the order (e.g., "FILLED", "CANCELLED").
    pub order_state: Option<String>,
    /// Requested quantity of contracts.
    pub quantity: Option<String>,
    /// Number of contracts filled.
    pub filled_quantity: Option<String>,
    /// Average execution price.
    pub average_price: Option<String>,
    /// Individual legs of the order.
    pub order_legs: Option<Vec<serde_json::Value>>,
    /// Realized profit/loss data.
    pub realized_pnl: Option<serde_json::Value>,
    /// Total fees charged.
    pub total_fee: Option<serde_json::Value>,
    /// Timestamp when the order was created.
    pub created_at: Option<String>,
    /// Timestamp when the order was last updated.
    pub updated_at: Option<String>,
}

/// Represents a Robinhood account from the Ceres API.
///
/// Used internally for futures account discovery.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesAccount {
    /// Unique identifier for the account.
    pub id: Option<String>,
    /// Type of account (e.g., "FUTURES").
    pub account_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{FuturesContract, FuturesQuote};

    #[test]
    fn contract_maps_camelcase_display_symbol() {
        let json = r#"{"id":"x","symbol":"/ESM26:XCME","displaySymbol":"/ESM26","state":"active"}"#;
        let c: FuturesContract = serde_json::from_str(json).unwrap();
        assert_eq!(c.display_symbol.as_deref(), Some("/ESM26"));
    }

    #[test]
    fn quote_retains_symbol() {
        let json = r#"{"symbol":"/ESM26:XCME","bid_price":"1.0","ask_price":"2.0"}"#;
        let q: FuturesQuote = serde_json::from_str(json).unwrap();
        assert_eq!(q.symbol.as_deref(), Some("/ESM26:XCME"));
    }
}
