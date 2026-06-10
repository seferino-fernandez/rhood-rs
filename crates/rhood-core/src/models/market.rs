//! Market and trading-hours model types for the Robinhood API.
//!
//! Contains structs describing stock exchanges and their operating hours.

use serde::{Deserialize, Serialize};

/// Represents a stock exchange or market venue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Market {
    /// API URL for this market resource.
    pub url: Option<String>,
    /// API URL for today's trading hours.
    pub todays_hours: Option<String>,
    /// Market Identifier Code (MIC) for the exchange.
    pub mic: Option<String>,
    /// Operating MIC for the exchange.
    pub operating_mic: Option<String>,
    /// Short acronym for the exchange (e.g., "NYSE", "NASDAQ").
    pub acronym: Option<String>,
    /// Full name of the exchange.
    pub name: Option<String>,
    /// City where the exchange is located.
    pub city: Option<String>,
    /// Country where the exchange is located.
    pub country: Option<String>,
    /// IANA timezone of the exchange (e.g., "US/Eastern").
    pub timezone: Option<String>,
    /// Website URL for the exchange.
    pub website: Option<String>,
}

/// Represents the trading hours for a market on a specific date.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketHours {
    /// Date these hours apply to (YYYY-MM-DD).
    pub date: Option<String>,
    /// Indicates whether the market is open on this date.
    pub is_open: Option<bool>,
    /// Timestamp when regular trading opens.
    pub opens_at: Option<String>,
    /// Timestamp when regular trading closes.
    pub closes_at: Option<String>,
    /// Timestamp when extended (pre-market) trading opens.
    pub extended_opens_at: Option<String>,
    /// Timestamp when extended (after-hours) trading closes.
    pub extended_closes_at: Option<String>,
    /// API URL for the previous day's open hours.
    pub previous_open_hours: Option<String>,
    /// API URL for the next day's open hours.
    pub next_open_hours: Option<String>,
}
