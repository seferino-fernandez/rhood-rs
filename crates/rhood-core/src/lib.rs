//! Async Rust client for the Robinhood trading API.
//!
//! Provides authenticated access to stock quotes, options chains, order
//! placement, and account data through [`RobinhoodClient`].
//!
//! # Example
//!
//! ```no_run
//! use rhood_core::RobinhoodClient;
//!
//! # async fn run() -> rhood_core::Result<()> {
//! let client = RobinhoodClient::new()?;
//! client.login_from_cache().await?;
//!
//! let quotes = client.get_quotes(&["AAPL", "TSLA"]).await?;
//! for quote in quotes {
//!     println!("{:?}: {:?}", quote.symbol, quote.last_trade_price);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Modules
//!
//! - [`auth`] — Device token generation, token caching, and auth state machine
//! - [`client`] — [`RobinhoodClient`] struct with authenticated HTTP helpers
//! - [`config`] — [`RhoodConfig`] loaded from TOML files and environment variables
//! - [`endpoints`] — API methods organized by domain (stocks, options, orders, account)
//! - [`error`] — [`RhoodError`] enum with [`thiserror`] integration
//! - [`models`] — Serde response types for API data
//! - [`pagination`] — Generic paginated response wrappers
//! - [`api`] — Robinhood API path constants

#![warn(missing_docs)]
#![forbid(unsafe_code)]

/// Robinhood API path constants organized by domain.
pub mod api;
/// Authentication state machine, token caching, and device token generation.
pub mod auth;
/// The [`RobinhoodClient`] struct and its HTTP transport methods.
pub mod client;
/// Configuration loaded from TOML files, environment variables, and defaults.
pub mod config;
/// Endpoint methods on [`RobinhoodClient`] organized by domain.
pub mod endpoints;
/// Environment variable abstraction ([`Env`](env::Env)/[`SystemEnv`](env::SystemEnv)/[`MapEnv`](env::MapEnv))
/// plus [`env_non_empty`](env::env_non_empty) helpers threaded through the config loader
/// so tests can inject env values without mutating process state.
pub mod env;
/// Error types for this crate.
pub mod error;
/// Serde response structs for Robinhood API data.
pub mod models;
/// Generic paginated and results response wrappers.
pub mod pagination;
/// In-memory caches for identity/metadata lookups (symbol ↔ id, etc.).
pub mod resolver_cache;
/// Small shared helpers (e.g. URL parsing utilities).
pub mod util;

pub use client::RobinhoodClient;
pub use config::RhoodConfig;
pub use error::{ChallengeType, RhoodError};
pub use resolver_cache::ResolverCache;

/// A specialized [`Result`](std::result::Result) type for this crate.
///
/// All fallible operations in `rhood-core` return this type.
pub type Result<T> = std::result::Result<T, RhoodError>;
