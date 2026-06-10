//! User profile and day trade model types.

use serde::{Deserialize, Serialize};

/// Robinhood user profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    /// Robinhood user ID.
    pub id: Option<String>,
    /// Username (usually email).
    pub username: Option<String>,
    /// First name.
    pub first_name: Option<String>,
    /// Last name.
    pub last_name: Option<String>,
    /// Email address.
    pub email: Option<String>,
    /// When the account was created.
    pub created_at: Option<String>,
}

/// A recent day trade record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayTrade {
    /// Instrument URL.
    pub instrument: Option<String>,
    /// Ticker symbol.
    pub symbol: Option<String>,
    /// Trade date.
    pub date: Option<String>,
    /// Trade settlement date.
    pub settlement_date: Option<String>,
}

/// Day-trade status: the recent equity/option day trades, a count, and the
/// pattern-day-trader flag (derived from the account's margin balances).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayTradeCheck {
    /// Recent equity day trades (raw API objects).
    pub equity_day_trades: Vec<serde_json::Value>,
    /// Recent option day trades (raw API objects).
    pub option_day_trades: Vec<serde_json::Value>,
    /// Total number of recent day trades (equity + option).
    pub day_trade_count: i64,
    /// Whether the account is flagged as a pattern day trader.
    pub flagged_as_pattern_day_trader: bool,
}
