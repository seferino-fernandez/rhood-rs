//! Options-related model types for the Robinhood API.
//!
//! Contains structs for option chains, option instruments, and option
//! positions (calls and puts).

use std::fmt;

use serde::{Deserialize, Serialize};

/// Type of option contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum OptionType {
    /// A call option — the right to buy at the strike price.
    Call,
    /// A put option — the right to sell at the strike price.
    Put,
}

impl fmt::Display for OptionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Call => write!(f, "call"),
            Self::Put => write!(f, "put"),
        }
    }
}

/// Represents an options chain for an underlying stock symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionChain {
    /// Unique identifier for the option chain.
    pub id: Option<String>,
    /// Ticker symbol of the underlying stock.
    pub symbol: Option<String>,
    /// Indicates whether the user can open a new position in this chain.
    pub can_open_position: Option<bool>,
    /// Cash component of the option (for adjusted options).
    pub cash_component: Option<String>,
    /// Available expiration dates for this chain (YYYY-MM-DD).
    pub expiration_dates: Option<Vec<String>>,
    /// Contract multiplier applied to the trade value (typically "100.0000").
    pub trade_value_multiplier: Option<String>,
    /// Underlying instruments associated with this chain.
    pub underlying_instruments: Option<Vec<serde_json::Value>>,
    /// Minimum tick size configuration for the chain.
    pub min_ticks: Option<serde_json::Value>,
}

/// Represents a specific option contract (call or put) at a given strike and expiration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionInstrument {
    /// Identifier for the parent option chain.
    pub chain_id: Option<String>,
    /// Ticker symbol of the underlying stock.
    pub chain_symbol: Option<String>,
    /// Timestamp when the instrument record was created.
    pub created_at: Option<String>,
    /// Expiration date of the contract (YYYY-MM-DD).
    pub expiration_date: Option<String>,
    /// Unique identifier for this option instrument.
    pub id: Option<String>,
    /// Date the option was issued.
    pub issue_date: Option<String>,
    /// Minimum tick size configuration for this instrument.
    pub min_ticks: Option<serde_json::Value>,
    /// Robinhood-specific tradability status.
    pub rhs_tradability: Option<String>,
    /// Current state of the instrument (e.g., "active", "expired").
    pub state: Option<String>,
    /// Strike price of the option contract.
    pub strike_price: Option<String>,
    /// General tradability status.
    pub tradability: Option<String>,
    /// Type of option contract ("call" or "put").
    #[serde(rename = "type")]
    pub option_type: Option<String>,
    /// Timestamp when the instrument was last updated.
    pub updated_at: Option<String>,
    /// API URL for this option instrument resource.
    pub url: Option<String>,
}

/// Represents an option position held in a Robinhood account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionPosition {
    /// API URL for the account holding this position.
    pub account: Option<String>,
    /// Average price paid per contract.
    pub average_price: Option<String>,
    /// Identifier for the parent option chain.
    pub chain_id: Option<String>,
    /// Ticker symbol of the underlying stock.
    pub chain_symbol: Option<String>,
    /// Unique identifier for this option position.
    pub id: Option<String>,
    /// API URL for the associated option instrument.
    pub option: Option<String>,
    /// Number of contracts held.
    pub quantity: Option<String>,
    /// Position type (e.g., "long", "short").
    #[serde(rename = "type")]
    pub position_type: Option<String>,
    /// Timestamp when the position was created.
    pub created_at: Option<String>,
    /// Timestamp when the position was last updated.
    pub updated_at: Option<String>,
}

/// Input specification for looking up a specific option contract.
///
/// Used with [`RobinhoodClient::get_option_market_data`](crate::client::RobinhoodClient)
/// to identify contracts by strike, expiration, and type.
#[derive(Debug, Clone)]
pub struct OptionContractSpec<'a> {
    /// Strike price as a string (e.g., `"50.0000"`).
    pub strike_price: &'a str,
    /// Expiration date in YYYY-MM-DD format.
    pub expiration_date: &'a str,
    /// Contract type: `"call"` or `"put"`.
    pub option_type: &'a str,
}

/// Live market data for a specific option contract.
///
/// Returned by the `/marketdata/options/` endpoint. Contains quote prices,
/// Greeks, volume, open interest, and probability estimates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionMarketData {
    /// API URL of the option instrument.
    pub instrument: Option<String>,
    /// Unique identifier for the option instrument.
    pub instrument_id: Option<String>,

    /// Current bid price.
    pub bid_price: Option<String>,
    /// Current ask price.
    pub ask_price: Option<String>,
    /// Price of the most recent trade.
    pub last_trade_price: Option<String>,
    /// Mid-point of bid and ask (mark price).
    pub mark_price: Option<String>,
    /// Break-even price at expiration.
    pub break_even_price: Option<String>,
    /// Adjusted mark price.
    pub adjusted_mark_price: Option<String>,
    /// Closing price from the previous trading session.
    pub previous_close_price: Option<String>,
    /// Highest trade price today.
    pub high_price: Option<String>,
    /// Lowest trade price today.
    pub low_price: Option<String>,

    /// Delta (rate of change vs underlying price).
    pub delta: Option<String>,
    /// Gamma (rate of change of delta).
    pub gamma: Option<String>,
    /// Theta (time decay per day).
    pub theta: Option<String>,
    /// Vega (sensitivity to implied volatility).
    pub vega: Option<String>,
    /// Rho (sensitivity to interest rate changes).
    pub rho: Option<String>,
    /// Implied volatility of the contract.
    pub implied_volatility: Option<String>,

    /// Number of contracts traded today.
    pub volume: Option<i64>,
    /// Total outstanding contracts.
    pub open_interest: Option<i64>,

    /// Probability of profit for a long position (0.0-1.0).
    pub chance_of_profit_long: Option<String>,
    /// Probability of profit for a short position (0.0-1.0).
    pub chance_of_profit_short: Option<String>,

    /// Timestamp when the market data was last updated.
    pub updated_at: Option<String>,
}
