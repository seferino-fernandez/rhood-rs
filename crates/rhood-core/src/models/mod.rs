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
/// IPO Access list and card view models.
pub mod ipo;
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
pub use futures::{FuturesContract, FuturesOrder, FuturesPosition, FuturesQuote};
pub use ipo::{IpoAccessCard, IpoAccessList};
pub use market::*;
pub use option::{
    AggregateOptionPosition, OptionChain, OptionContractSpec, OptionInstrument, OptionMarketData,
    OptionPosition, OptionType,
};
pub use order::{
    OptionOrder, OptionOrderIntent, OptionOrderSummary, OptionPositionEffect, OrderAmount,
    OrderIntent, OrderSummary, OrderType, Side, StagedOrder, StockOrder, StockOrderIntent,
    StockOrderSummary, SubmittedOrder, TimeInForce, Trigger,
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
