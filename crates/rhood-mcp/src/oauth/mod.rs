//! OAuth 2.1 + PKCE authorization server for the HTTP transport.
//!
//! Implements RFC 6749 (authorization code grant), RFC 7636 (PKCE), and
//! RFC 8414 (authorization server metadata) so that MCP clients which
//! understand standard OAuth flows can connect without bespoke integration.
//!
//! - [`endpoints`] — `/authorize`, `/token`, `/register`, and the well-known
//!   metadata endpoints.
//! - [`model`] — models used for oauth.
//! - [`pkce`] — code-challenge generation and verification.
//! - [`store`] — in-memory storage for authorization codes, access tokens,
//!   CSRF nonces, and registered clients with periodic eviction.

pub mod endpoints;
pub mod model;
pub mod pkce;
pub mod store;

#[cfg(test)]
mod tests;
