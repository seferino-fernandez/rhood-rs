//! Serde model types for Robinhood API responses.
//!
//! Provides structs and enums representing accounts, stock quotes, market
//! data, options, and order payloads returned by the Robinhood REST API.

/// Account profile, portfolio, and position types.
pub mod account;
pub(crate) mod auth;
/// Dividend and interest payment types.
pub mod dividend;
/// Account document types.
pub mod document;
/// Futures contract, quote, and order types.
pub mod futures;
/// Market and trading-hours types.
pub mod market;
/// Options chain, instrument, and position types.
pub mod option;
/// Order types, request builders, and trade enums.
pub mod order;
/// Recurring investment schedule types.
pub mod recurring;
/// Research and discovery types (earnings, ratings, news, splits, tags).
pub mod research;
/// Stock quote, fundamentals, instrument, and historical data types.
pub mod stock;
/// Unified transfer types.
pub mod transfer;
/// User profile and day trade types.
pub mod user;
/// Watchlist types.
pub mod watchlist;

pub use account::*;
pub use dividend::{Dividend, InterestPayment, MoneyAmount};
pub use document::*;
pub use futures::{FuturesContract, FuturesOrder, FuturesQuote};
pub use market::*;
pub use option::{
    OptionChain, OptionContractSpec, OptionInstrument, OptionMarketData, OptionPosition, OptionType,
};
pub use order::{
    OptionOrder, OptionOrderRequest, OrderAmount, OrderType, Side, StockOrder, StockOrderRequest,
    TimeInForce, Trigger,
};
pub use recurring::{
    CreateRecurringRequest, NextInvestmentDate, RecurringFrequency, RecurringInvestment,
    RecurringSource, RecurringState, UpdateRecurringRequest,
};
pub use research::*;
pub use stock::*;
pub use transfer::*;
pub use user::*;
pub use watchlist::{Watchlist, WatchlistChildInfo, WatchlistItem};
