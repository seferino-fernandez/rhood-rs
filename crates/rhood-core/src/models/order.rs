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

/// Specifies whether an option order opens a new position or closes an existing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum OptionPositionEffect {
    /// Open a new long or short option position.
    Open,
    /// Close an existing long or short option position.
    Close,
}

impl fmt::Display for OptionPositionEffect {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => write!(formatter, "open"),
            Self::Close => write!(formatter, "close"),
        }
    }
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

impl fmt::Display for TimeInForce {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gfd => write!(formatter, "gfd"),
            Self::Gtc => write!(formatter, "gtc"),
        }
    }
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum OrderAmount {
    /// Whole or fractional share quantity (e.g., 10.0 shares, 0.5 shares).
    Quantity(f64),
    /// Dollar intent converted to a six-decimal share estimate before submission.
    DollarAmount(f64),
}

impl OrderAmount {
    /// Returns the [`TimeInForce`] the brokerage requires for this amount, if
    /// it constrains the choice at all.
    ///
    /// Dollar-based orders convert to a fractional share estimate against the
    /// current ask, so they are only accepted good-for-day. Share quantities
    /// carry no such constraint and return `None`, leaving the duration to the
    /// caller.
    ///
    /// This exists so adapters can ask rather than know. Preparation validates
    /// against this same function, so a caller that honours it cannot be
    /// rejected for the duration, and the rule has exactly one definition.
    #[must_use]
    pub fn required_time_in_force(&self) -> Option<TimeInForce> {
        match self {
            Self::Quantity(_) => None,
            Self::DollarAmount(_) => Some(TimeInForce::Gfd),
        }
    }
}

/// The outcome of attempting to cancel every open order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelAllOutcome {
    /// IDs of orders that were successfully cancelled.
    pub cancelled: Vec<String>,
    /// Orders that could not be cancelled, with the reason for each failure.
    pub failed: Vec<CancelAllFailure>,
}

/// A single order that could not be cancelled as part of a bulk cancellation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelAllFailure {
    /// The order ID, or `None` when the open-order response did not include one.
    pub order_id: Option<String>,
    /// The cancellation error, or the reason an attempt could not be made.
    pub error: String,
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

/// Caller intent for a stock order before brokerage identities are resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockOrderIntent {
    /// Canonical brokerage account number from account discovery.
    ///
    /// May be omitted only when exactly one eligible account exists.
    pub account_number: Option<String>,
    /// Ticker symbol of the stock to trade.
    pub symbol: String,
    /// Order amount - either a share quantity or a dollar amount.
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

/// Caller intent for a single-leg option order before contract resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionOrderIntent {
    /// Canonical brokerage account number from account discovery.
    ///
    /// May be omitted only when exactly one eligible account exists.
    pub account_number: Option<String>,
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
    /// Whether this order opens a new position or closes an existing one.
    pub position_effect: OptionPositionEffect,
    /// Duration policy for the order.
    pub time_in_force: TimeInForce,
}

/// Caller intent for an order that core can prepare without submitting.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "intent")]
pub enum OrderIntent {
    /// Stock order intent.
    Stock(StockOrderIntent),
    /// Single-leg option order intent.
    Option(OptionOrderIntent),
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
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
    pub extended_hours: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ask_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bid_price: Option<String>,
    pub override_day_trade_checks: bool,
    pub override_dtbp_checks: bool,
    pub ref_id: String,
    pub order_form_version: u8,
}

/// Human-review fields fixed by stock-order preparation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StockOrderSummary {
    /// Canonical account number placed in the broker account URL.
    pub account_number: String,
    /// Safe masked account label for human presentation.
    pub account_label: String,
    /// Uppercase ticker symbol.
    pub symbol: String,
    /// Exact broker instrument URL selected during preparation.
    pub instrument: String,
    /// Exact share quantity submitted to the broker.
    pub quantity: String,
    /// Buy or sell side.
    pub side: Side,
    /// Market or limit order type.
    pub order_type: OrderType,
    /// Duration policy.
    pub time_in_force: TimeInForce,
    /// Trigger condition.
    pub trigger: Trigger,
    /// Trading session.
    pub market_hours: MarketHours,
    /// Broker extended-hours flag derived from the trading session.
    pub extended_hours: bool,
    /// Broker submission price, including the selected market collar.
    pub price: Option<String>,
    /// Stop trigger price when present.
    pub stop_price: Option<String>,
    /// Fresh ask quote included for immediate market orders.
    pub ask_price: Option<String>,
    /// Fresh bid quote included for immediate market orders.
    pub bid_price: Option<String>,
    /// Original dollar intent when converted to fractional shares.
    pub requested_dollar_amount: Option<String>,
    /// Quote used to convert dollar intent to shares.
    pub conversion_price: Option<String>,
    /// Per-attempt client reference sent to the broker.
    pub client_ref: String,
    /// Private broker order-form contract version.
    pub order_form_version: u8,
}

/// Human-review fields fixed by option-order preparation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OptionOrderSummary {
    /// Canonical account number placed in the broker account URL.
    pub account_number: String,
    /// Safe masked account label for human presentation.
    pub account_label: String,
    /// Uppercase underlying ticker symbol.
    pub symbol: String,
    /// Exact broker option-contract URL selected during preparation.
    pub option_contract: String,
    /// Expiration date of the selected contract.
    pub expiration_date: String,
    /// Strike price of the selected contract.
    pub strike_price: String,
    /// Call or put contract type.
    pub option_type: String,
    /// Buy or sell side.
    pub side: Side,
    /// Open or close position effect.
    pub position_effect: OptionPositionEffect,
    /// Debit for buys and credit for sells.
    pub direction: String,
    /// Exact contract quantity sent to the broker.
    pub quantity: String,
    /// Limit price per contract sent to the broker.
    pub limit_price: String,
    /// Broker order type fixed during preparation.
    pub order_type: String,
    /// Broker trigger fixed during preparation.
    pub trigger: String,
    /// Duration policy.
    pub time_in_force: TimeInForce,
    /// Per-attempt client reference sent to the broker.
    pub client_ref: String,
}

/// Typed human-review projection of an immutable staged order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "summary")]
pub enum OrderSummary {
    /// Stock-order execution terms.
    Stock(StockOrderSummary),
    /// Option-order execution terms.
    Option(OptionOrderSummary),
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct OptionOrderPayload {
    pub account: String,
    pub direction: String,
    pub time_in_force: TimeInForce,
    pub legs: Vec<OptionLeg>,
    #[serde(rename = "type")]
    pub order_type: String,
    pub trigger: String,
    pub price: String,
    pub quantity: String,
    pub override_day_trade_checks: bool,
    pub override_dtbp_checks: bool,
    pub ref_id: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct OptionLeg {
    pub position_effect: OptionPositionEffect,
    pub side: Side,
    pub ratio_quantity: u32,
    pub option: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub(crate) enum StagedOrderPayload {
    Stock(StockOrderPayload),
    Option(OptionOrderPayload),
}

/// Immutable, broker-ready order that has not been submitted.
///
/// Values are created by [`RobinhoodClient::prepare_order`](crate::RobinhoodClient::prepare_order)
/// and consumed by [`RobinhoodClient::submit_order`](crate::RobinhoodClient::submit_order).
/// The exact wire payload is private. Callers should present [`Self::summary`]
/// before authorising submission.
#[derive(Debug, Serialize, Deserialize)]
pub struct StagedOrder {
    pub(crate) payload: StagedOrderPayload,
    pub(crate) summary: OrderSummary,
    pub(crate) prepared_at_unix_ms: i64,
    pub(crate) expires_at_unix_ms: i64,
}

impl StagedOrder {
    /// Returns the typed execution terms that a user-facing adapter must show.
    #[must_use]
    pub fn summary(&self) -> &OrderSummary {
        &self.summary
    }

    /// Returns the Unix timestamp in milliseconds after which submission fails.
    #[must_use]
    pub fn expires_at_unix_ms(&self) -> i64 {
        self.expires_at_unix_ms
    }

    /// Returns the frozen client reference used to reconcile uncertain writes.
    #[must_use]
    pub fn client_reference(&self) -> &str {
        match &self.payload {
            StagedOrderPayload::Stock(payload) => &payload.ref_id,
            StagedOrderPayload::Option(payload) => &payload.ref_id,
        }
    }
}

/// Brokerage response from submitting a [`StagedOrder`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "order")]
pub enum SubmittedOrder {
    /// Submitted stock order.
    Stock(StockOrder),
    /// Submitted option order.
    Option(OptionOrder),
}

impl SubmittedOrder {
    /// Returns the brokerage-assigned order ID when present.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Stock(order) => order.id.as_deref(),
            Self::Option(order) => order.id.as_deref(),
        }
    }

    /// Returns the brokerage order state when present.
    #[must_use]
    pub fn state(&self) -> Option<&str> {
        match self {
            Self::Stock(order) => order.state.as_deref(),
            Self::Option(order) => order.state.as_deref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CancelAllFailure, CancelAllOutcome};

    #[test]
    fn cancel_all_outcome_serializes_round_trip() {
        let outcome = CancelAllOutcome {
            cancelled: vec!["order-1".to_string(), "order-2".to_string()],
            failed: vec![
                CancelAllFailure {
                    order_id: Some("order-3".to_string()),
                    error: "already filled".to_string(),
                },
                CancelAllFailure {
                    order_id: None,
                    error: "open order was missing an ID".to_string(),
                },
            ],
        };

        let json = serde_json::to_string(&outcome).unwrap();
        let round_tripped: CancelAllOutcome = serde_json::from_str(&json).unwrap();

        assert_eq!(round_tripped, outcome);
    }
}
