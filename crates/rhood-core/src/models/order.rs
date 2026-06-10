//! Order-related model types for the Robinhood API.
//!
//! Contains enums for order parameters (side, type, time-in-force) as well as
//! response structs for stock and option orders, and their corresponding
//! request builders.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Represents the side (direction) of a trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Side {
    /// Indicates a buy order to acquire shares or contracts.
    Buy,
    /// Indicates a sell order to dispose of shares or contracts.
    Sell,
}

/// Represents the execution type of an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum OrderType {
    /// Executes immediately at the best available market price.
    Market,
    /// Executes only at the specified limit price or better.
    Limit,
}

impl fmt::Display for OrderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Market => write!(f, "market"),
            Self::Limit => write!(f, "limit"),
        }
    }
}

/// Represents the duration policy for an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum TimeInForce {
    /// Good for day -- the order expires at the end of the current trading session.
    Gfd,
    /// Good till cancelled -- the order remains active until explicitly cancelled.
    Gtc,
}

/// Specifies when the order should trigger execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    /// Execute the order immediately (standard market/limit behavior).
    Immediate,
    /// Execute only after the stop price is reached.
    Stop,
}

impl fmt::Display for Trigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Immediate => write!(f, "immediate"),
            Self::Stop => write!(f, "stop"),
        }
    }
}

/// Specifies which trading session the order is eligible for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MarketHours {
    /// Standard market hours (9:30 AM - 4:00 PM ET).
    #[cfg_attr(feature = "clap", value(name = "regular"))]
    RegularHours,
    /// Pre-market and after-hours sessions.
    #[cfg_attr(feature = "clap", value(name = "extended"))]
    ExtendedHours,
    /// 24-hour trading session (available for select stocks).
    #[cfg_attr(feature = "clap", value(name = "all-day"))]
    AllDayHours,
}

impl fmt::Display for MarketHours {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RegularHours => write!(f, "regular"),
            Self::ExtendedHours => write!(f, "extended"),
            Self::AllDayHours => write!(f, "all-day"),
        }
    }
}

/// Specifies the quantity semantics for a stock order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OrderAmount {
    /// Whole or fractional share quantity (e.g., 10.0 shares, 0.5 shares).
    Quantity(f64),
    /// Dollar amount to invest — broker calculates share count (e.g., $50.00).
    DollarAmount(f64),
}

/// Represents a stock order response from the Robinhood API.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StockOrder {
    /// Unique identifier for the order.
    pub id: Option<String>,
    /// API URL for the account that placed the order.
    pub account: Option<String>,
    /// API URL for the instrument being ordered.
    pub instrument: Option<String>,
    /// Ticker symbol of the stock.
    pub symbol: Option<String>,
    /// Order side ("buy" or "sell").
    pub side: Option<String>,
    /// Requested quantity of shares.
    pub quantity: Option<String>,
    /// Limit price, if applicable.
    pub price: Option<String>,
    /// Average execution price of filled shares.
    pub average_price: Option<String>,
    /// Cumulative quantity of shares filled so far.
    pub cumulative_quantity: Option<String>,
    /// Current state of the order (e.g., "queued", "filled", "cancelled").
    pub state: Option<String>,
    /// Timestamp when the order was created.
    pub created_at: Option<String>,
    /// Timestamp when the order was last updated.
    pub updated_at: Option<String>,
    /// API URL to cancel this order, if cancellation is available.
    pub cancel: Option<String>,
    /// Type of order ("market" or "limit").
    #[serde(rename = "type")]
    pub order_type: Option<String>,
    /// Time-in-force policy for the order.
    pub time_in_force: Option<String>,
    /// Indicates whether the order is eligible for extended-hours trading.
    pub extended_hours: Option<bool>,
    /// Stop price, if this is a stop or stop-limit order.
    pub stop_price: Option<String>,
    /// Trigger type for the order ("immediate" or "stop").
    pub trigger: Option<String>,
}

/// Represents an option order response from the Robinhood API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionOrder {
    /// Unique identifier for the order.
    pub id: Option<String>,
    /// Identifier for the option chain.
    pub chain_id: Option<String>,
    /// Ticker symbol of the underlying stock.
    pub chain_symbol: Option<String>,
    /// Direction of the order (e.g., "debit", "credit").
    pub direction: Option<String>,
    /// Individual legs of the option order.
    pub legs: Option<Vec<serde_json::Value>>,
    /// Estimated premium for the order.
    pub premium: Option<String>,
    /// Limit price per contract.
    pub price: Option<String>,
    /// Actual premium processed upon execution.
    pub processed_premium: Option<String>,
    /// Number of contracts in the order.
    pub quantity: Option<String>,
    /// Current state of the order (e.g., "queued", "filled", "cancelled").
    pub state: Option<String>,
    /// Time-in-force policy for the order.
    pub time_in_force: Option<String>,
    /// Type of order ("market" or "limit").
    #[serde(rename = "type")]
    pub order_type: Option<String>,
    /// Timestamp when the order was created.
    pub created_at: Option<String>,
    /// Timestamp when the order was last updated.
    pub updated_at: Option<String>,
    /// API URL to cancel this order, if cancellation is available.
    pub cancel_url: Option<String>,
}

/// Represents a request to place a stock order.
#[derive(Debug, Clone)]
pub struct StockOrderRequest {
    /// Ticker symbol of the stock to trade.
    pub symbol: String,
    /// Order amount — either a share quantity or a dollar amount.
    pub amount: OrderAmount,
    /// Side of the trade (buy or sell).
    pub side: Side,
    /// Execution type (market or limit).
    pub order_type: OrderType,
    /// Limit price per share; required when `order_type` is [`OrderType::Limit`].
    pub limit_price: Option<f64>,
    /// Trigger condition for the order.
    pub trigger: Trigger,
    /// Stop price; required when `trigger` is [`Trigger::Stop`].
    pub stop_price: Option<f64>,
    /// Duration policy for the order.
    pub time_in_force: TimeInForce,
    /// Trading session eligibility for the order.
    pub market_hours: MarketHours,
}

/// Represents a request to place an option order.
#[derive(Debug, Clone)]
pub struct OptionOrderRequest {
    /// Ticker symbol of the underlying stock.
    pub symbol: String,
    /// Expiration date of the option contract (YYYY-MM-DD).
    pub expiration_date: String,
    /// Strike price of the option contract.
    pub strike_price: f64,
    /// Type of option contract ("call" or "put").
    pub option_type: String,
    /// Side of the trade (buy or sell).
    pub side: Side,
    /// Number of contracts to trade.
    pub quantity: f64,
    /// Limit price per contract.
    pub limit_price: f64,
    /// Position effect ("open" or "close").
    pub position_effect: String,
    /// Whether the order results in a credit or debit ("credit" or "debit").
    pub credit_or_debit: String,
    /// Duration policy for the order.
    pub time_in_force: TimeInForce,
}

/// Dollar-based amount for fractional/dollar orders.
#[derive(Debug, Serialize)]
pub(crate) struct DollarBasedAmount {
    pub amount: String,
    pub currency_code: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct StockOrderPayload {
    pub account: String,
    pub instrument: String,
    pub symbol: String,
    pub quantity: String,
    pub side: Side,
    #[serde(rename = "type")]
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    pub trigger: Trigger,
    pub market_hours: MarketHours,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dollar_based_amount: Option<DollarBasedAmount>,
}

#[derive(Debug, Serialize)]
pub(crate) struct OptionOrderPayload {
    pub account: String,
    pub direction: String,
    pub time_in_force: TimeInForce,
    pub legs: Vec<OptionLeg>,
    #[serde(rename = "type")]
    pub order_type: &'static str,
    pub trigger: &'static str,
    pub price: String,
    pub quantity: String,
    pub override_day_trade_checks: bool,
    pub override_dtbp_checks: bool,
    pub ref_id: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct OptionLeg {
    pub position_effect: String,
    pub side: Side,
    pub ratio_quantity: u32,
    pub option: String,
}
