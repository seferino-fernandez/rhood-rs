//! Endpoint implementations for the Robinhood API.
//!
//! Each submodule provides `impl` blocks on [`crate::client::RobinhoodClient`]
//! organized by domain: account info, stock quotes, options, orders, and
//! market data.

/// Account profile, portfolio, and position queries.
pub mod account;
/// Dividend and interest payment queries.
pub mod dividends;
/// Account document queries.
pub mod documents;
/// Futures contracts, quotes, and order history.
pub mod futures;
/// Market listings and trading hours.
pub mod markets;
/// Options chains, instruments, and positions.
pub mod options;
/// Stock and option order management.
pub mod orders;
/// Recurring investment schedule management.
pub mod recurring;
/// Research and discovery: earnings, ratings, news, splits, tags.
pub mod research;
/// Stock quotes, fundamentals, historicals, and instrument lookup.
pub mod stocks;
/// Unified transfer queries.
pub mod transfers;
/// User profile and day trade queries.
pub mod user;
/// Watchlist CRUD operations.
pub mod watchlists;

#[cfg(test)]
mod test_support;
