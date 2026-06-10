//! Environment variable source abstraction used by the config loader.
//!
//! Splitting the "where does an env var come from?" question behind a trait
//! lets production code read from the process environment while tests pass
//! an in-memory [`MapEnv`](crate::env::MapEnv). The config module never calls
//! `std::env::var` directly — every env read goes through an
//! [`&impl Env`](crate::env::Env).
//!
//! Two small helpers — [`env_non_empty`](crate::env::env_non_empty) and
//! [`env_non_empty_u64`](crate::env::env_non_empty_u64) — treat empty strings
//! as "unset" and parse `u64` values, matching how every `RHOOD_*` variable
//! is interpreted across the workspace.

use std::collections::HashMap;

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

    /// Inserts `key` → `value`, returning `self` so calls chain.
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

/// Returns the value of `key` from `env` parsed as `u64`, if set and valid.
///
/// Builds on [`env_non_empty`]: unset, empty, or non-numeric values return
/// `None`, leaving the caller's current value untouched.
pub fn env_non_empty_u64(env: &impl Env, key: &str) -> Option<u64> {
    env_non_empty(env, key).and_then(|value| value.parse().ok())
}
