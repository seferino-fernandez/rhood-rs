//! Environment variable source abstraction used by the config loader.
//!
//! Splitting the "where does an env var come from?" question behind a trait
//! lets production code read from the process environment while tests pass
//! an in-memory [`MapEnv`](crate::env::MapEnv). The config module never calls
//! `std::env::var` directly, every env read goes through an
//! [`&impl Env`](crate::env::Env).
//!
//! [`env_non_empty`](crate::env::env_non_empty) treats empty strings as
//! "unset", while [`env_parsed`](crate::env::env_parsed) parses typed values
//! and rejects malformed environment overrides.

use std::collections::HashMap;

use crate::{Result, RhoodError};

/// Source of configuration environment variables.
///
/// The production impl ([`SystemEnv`]) reads from the real process
/// environment. Test fakes ([`MapEnv`]) hold an in-memory map.
pub trait Env {
    /// Returns the raw value for `key`, or `None` if unset.
    ///
    /// Mirrors `std::env::var(key).ok()`: empty strings round-trip as
    /// `Some("")`. Call sites that want to treat empty as missing should
    /// layer [`env_non_empty`] on top.
    fn get(&self, key: &str) -> Option<String>;
}

/// Production [`Env`] backed by `std::env::var`. Zero state.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemEnv;

impl Env for SystemEnv {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

/// In-memory [`Env`] for tests. Keys absent from the map return `None`;
/// empty-string values round-trip as `Some("")` to match [`SystemEnv`].
///
/// Construct with [`MapEnv::new`] (empty) or [`MapEnv::default`], then
/// chain [`MapEnv::with`] to insert keys.
#[derive(Debug, Default, Clone)]
pub struct MapEnv {
    vars: HashMap<String, String>,
}

impl MapEnv {
    /// Returns a new empty `MapEnv`. Equivalent to [`MapEnv::default`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts `key` -> `value`, returning `self` so calls chain.
    pub fn with<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.vars.insert(key.into(), value.into());
        self
    }
}

impl Env for MapEnv {
    fn get(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }
}

/// Returns the value of `key` from `env` if it is set and non-empty.
///
/// Treats empty strings as "unset" so a `FOO=` line in a `.env` file or a
/// cleared-but-not-unset shell variable does not clobber a value coming from
/// TOML or defaults.
pub fn env_non_empty(env: &impl Env, key: &str) -> Option<String> {
    env.get(key).filter(|value| !value.is_empty())
}

/// Returns the value of `key` from `env` parsed as `T`, if set and non-empty.
///
/// Builds on [`env_non_empty`]: unset and empty values return `Ok(None)`.
/// A present-but-unparseable value is an error so configuration does not
/// silently retain a default the operator intended to override.
pub fn env_parsed<T>(env: &impl Env, key: &str) -> Result<Option<T>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let Some(value) = env_non_empty(env, key) else {
        return Ok(None);
    };

    value.parse().map(Some).map_err(|error| {
        RhoodError::InvalidParameter(format!(
            "environment variable {key} has invalid value {value:?}: {error}"
        ))
    })
}
