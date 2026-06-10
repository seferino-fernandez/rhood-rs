//! Shared helpers for CLI integration tests.
//!
//! These tests drive the `rhood` binary via `assert_cmd` against the live
//! Robinhood API. They require valid credentials on disk (via
//! `~/.config/rhood/config.toml` or the `RHOOD_USERNAME`/`RHOOD_PASSWORD`
//! env vars) and are marked `#[ignore]` so `cargo test` / `just test`
//! skips them. Run explicitly via `just integration`.

#![allow(dead_code)]

use assert_cmd::Command;
use serde_json::Value;

/// Builds a command for the `rhood` binary with JSON output.
///
/// Every integration test uses JSON output so assertions can run against
/// parsed values rather than fragile table formatting.
pub fn cli() -> Command {
    let mut command = Command::cargo_bin("rhood")
        .expect("rhood binary not found - run `cargo build -p rhood-cli`");
    command.arg("--output").arg("json");
    command
}

/// Parses the stdout of an `assert_cmd` success assertion as JSON.
pub fn parse_json(output: &assert_cmd::assert::Assert) -> Value {
    let stdout = &output.get_output().stdout;
    serde_json::from_slice(stdout).unwrap_or_else(|error| {
        panic!(
            "expected JSON stdout; got error {error}, raw bytes: {:?}",
            String::from_utf8_lossy(stdout)
        )
    })
}

// Symbol fixtures - chosen to be stable, widely-tradable, and
// representative of each asset class.
pub const STOCK_SYMBOL: &str = "AAPL";
pub const STOCK_SYMBOLS_MULTI: &[&str] = &["AAPL", "SPY"];
pub const INDEX_SYMBOL: &str = "SPX";
pub const INDEX_SYMBOLS_MULTI: &[&str] = &["SPX", "NDX"];
pub const FUTURES_SYMBOL: &str = "ESM26";
pub const WATCHLIST_NAME: &str = "My First List";
pub const POPULAR_TAG: &str = "100-most-popular";
pub const MARKET_MIC: &str = "XNYS";
pub const MARKET_DATE: &str = "2026-06-01";
pub const RECURRING_START_DATE: &str = "2026-06-01";
