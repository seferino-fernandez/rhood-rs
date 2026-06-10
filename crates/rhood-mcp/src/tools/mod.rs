//! MCP tool implementations exposing the Robinhood API.
//!
//! Each `*_tools.rs` module groups tools by domain. [`handler::RhoodTools`]
//! is the central dispatcher that owns the authenticated client and routes
//! every tool call to the appropriate domain handler.

mod account_tools;
mod enrichment;
mod futures_tools;
mod handler;
mod income_tools;
mod index_tools;
mod market_tools;
mod option_tools;
mod order_tools;
mod params;
mod recurring_tools;
mod research_tools;
mod schema;
mod server;
mod stock_tools;
mod types;
mod user_tools;
mod watchlist_tools;

pub use handler::{LazyAuthHook, RhoodTools};
