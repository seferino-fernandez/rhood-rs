//! Live-API integration tests against the unofficial Robinhood OAuth surface.
//!
//! Run with: `just integration` (passes `--ignored` to cargo test).

mod account;
mod common;
mod documents;
mod futures;
mod income;
mod indexes;
mod market;
mod options;
mod orders;
mod recurring;
mod stocks;
mod watchlists;
