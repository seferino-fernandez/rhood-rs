//! Configuration for the rhood-core client.
//!
//! [`RhoodConfig`] is loaded from a TOML file, environment variables, and
//! built-in defaults. Environment variables override TOML values, and secrets
//! can be supplied via `*_file` fields that read from disk.

mod loader;
mod secrets;
mod types;

pub use secrets::{ensure_secret_file_permissions, resolve_secret};
pub use types::{
    ApiConfig, AuthConfig, CacheConfig, DeviceVerificationConfig, HttpConfig, LogConfig,
    RhoodConfig,
};

#[cfg(test)]
mod tests;
