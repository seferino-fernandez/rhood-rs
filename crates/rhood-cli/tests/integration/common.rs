//! Shared helpers for CLI integration tests.
//!
//! These tests drive the `rhood` binary via `assert_cmd` against the live
//! Robinhood API. They require valid credentials on disk (via
//! `~/.config/rhood/config.toml` or the `RHOOD_USERNAME`/`RHOOD_PASSWORD`
//! env vars) and are marked `#[ignore]` so `cargo test` / `just test`
//! skips them. Run explicitly via `just integration`.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "live-API test helpers use explicit diagnostics when the CLI binary or JSON output is unavailable"
)]

use assert_cmd::Command;
use jiff::{Timestamp, ToSpan, civil::Date, tz::TimeZone};
use rhood_core::models::option::OptionType;
use rhood_core::{RhoodConfig, RobinhoodClient};
use secrecy::ExposeSecret;
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
pub const STOCK_INSTRUMENT_ID: &str = "450dfc6d-5510-4d40-abfb-f633b7d9be3e";
pub const POPULAR_TAG: &str = "100-most-popular";
pub const MARKET_MIC: &str = "XNYS";

pub struct OptionFixture {
    pub expiration: String,
    pub strike: String,
}

fn utc_today() -> Date {
    Timestamp::now().to_zoned(TimeZone::UTC).date()
}

/// Returns today's UTC date, which is accepted by both market-hours and
/// recurring-date discovery without becoming stale in the repository.
pub fn today() -> String {
    utc_today().to_string()
}

pub fn one_year_ago() -> String {
    utc_today()
        .checked_sub(365.days())
        .expect("one year ago is representable")
        .to_string()
}

/// Chooses the current (or next, once expiration is near) quarterly E-mini S&P
/// contract instead of pinning the suite to an expired contract forever.
pub fn futures_symbol() -> String {
    let today = utc_today();
    let quarters = [(3, 'H'), (6, 'M'), (9, 'U'), (12, 'Z')];
    let (code, year) = quarters
        .iter()
        .find(|(month, _)| *month > today.month() || (*month == today.month() && today.day() <= 15))
        .map_or_else(
            || {
                (
                    'H',
                    today
                        .year()
                        .checked_add(1)
                        .expect("next futures contract year is representable"),
                )
            },
            |(_, code)| (*code, today.year()),
        );
    format!("ES{code}{:02}", year.rem_euclid(100))
}

/// Discovers an active contract from the live chain so option tests do not
/// encode an expiration or strike that will inevitably rot.
pub fn discover_option_fixture(symbol: &str, index: bool) -> OptionFixture {
    let mut config = RhoodConfig::load(None).expect("load live rhood config");
    config.read_only = true;
    let username = config.auth.username.clone();
    let password = config.auth.password.clone();
    let mfa = config
        .auth
        .mfa_secret
        .as_ref()
        .map(|secret| secret.expose_secret().to_owned());
    let client = RobinhoodClient::with_config(config).expect("build live Robinhood client");
    let runtime = tokio::runtime::Runtime::new().expect("create fixture discovery runtime");

    runtime.block_on(async move {
        if !client
            .login_from_cache()
            .await
            .expect("restore fixture discovery session")
        {
            let username = username.expect("live config has auth.username or a token cache");
            let password = password.expect("live config has auth.password or a token cache");
            client
                .login(username.as_str(), password.expose_secret(), mfa.as_deref())
                .await
                .expect("authenticate fixture discovery client");
        }
        let chain = if index {
            client.get_index_option_chain(symbol).await
        } else {
            client.get_option_chain(symbol).await
        }
        .expect("load option chain");
        let cutoff = today();
        let expiration = chain
            .expiration_dates
            .unwrap_or_default()
            .into_iter()
            .find(|date| date >= &cutoff)
            .expect("option chain has a current expiration");
        let contracts = if index {
            client
                .find_index_options(symbol, &expiration, OptionType::Call, None)
                .await
        } else {
            client.find_options(symbol, &expiration, "call", None).await
        }
        .expect("discover active option contracts");
        let strike = contracts
            .into_iter()
            .find_map(|contract| contract.strike_price)
            .expect("active option chain contains a strike");
        OptionFixture { expiration, strike }
    })
}
