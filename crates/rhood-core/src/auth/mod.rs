//! Authentication primitives for the Robinhood API client.
//!
//! Provides `AuthState`, a state machine that tracks the current authentication
//! phase (unauthenticated, challenged, MFA-required, device-verification, or
//! authenticated), and `TokenCache` / `CachedToken` for persisting OAuth
//! tokens to disk.

mod auth_state;
mod token_cache;

pub use auth_state::AuthState;
pub use token_cache::{CachedToken, TokenCache};
