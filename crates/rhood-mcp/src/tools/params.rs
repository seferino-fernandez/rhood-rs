use rhood_core::models::document::DocumentType;
use rhood_core::models::option::OptionType;
use rhood_core::models::order::{MarketHours, OrderType, Side, TimeInForce, Trigger};
use rhood_core::models::recurring::{RecurringFrequency, RecurringSource, RecurringState};
use rhood_core::models::stock::{HistoricalInterval, HistoricalSpan};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use std::borrow::Cow;
use url::Url;

const OPTION_INSTRUMENT_URL_PATTERN: &str =
    r"^https://api\.robinhood\.com/options/instruments/[^/?#]+/?$";

/// Accepts a JSON number or a numeric string (e.g. `15` or `"15.00"`).
#[derive(Deserialize)]
#[serde(untagged)]
enum NumOrStr {
    Num(f64),
    Str(String),
}

fn coerce(value: NumOrStr) -> Result<f64, String> {
    match value {
        NumOrStr::Num(n) => Ok(n),
        NumOrStr::Str(s) => s
            .trim()
            .parse::<f64>()
            .map_err(|error| format!("expected a number, got string {s:?}: {error}")),
    }
}

/// serde `deserialize_with` for a required `f64` that also accepts numeric strings.
pub fn f64_or_str<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    coerce(NumOrStr::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}

/// serde `deserialize_with` for an `Option<f64>` that also accepts numeric strings.
pub fn opt_f64_or_str<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<NumOrStr>::deserialize(deserializer)? {
        None => Ok(None),
        Some(inner) => coerce(inner).map(Some).map_err(serde::de::Error::custom),
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct StockQuoteParams {
    /// Stock ticker symbols, e.g. `["AAPL", "TSLA"]`
    #[schemars(length(max = 50))]
    pub symbols: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct StockHistoryParams {
    /// Stock ticker symbol
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    pub symbol: String,
    /// Candle interval
    #[serde(default = "default_interval")]
    pub interval: HistoricalInterval,
    /// Time span
    #[serde(default = "default_span")]
    pub span: HistoricalSpan,
}

pub fn default_interval() -> HistoricalInterval {
    HistoricalInterval::Hour
}

pub fn default_span() -> HistoricalSpan {
    HistoricalSpan::Week
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceStockOrderParams {
    /// Stock symbol
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    pub symbol: String,
    /// Buy or sell
    pub side: Side,
    /// Number of shares (mutually exclusive with dollar_amount)
    #[serde(default, deserialize_with = "opt_f64_or_str")]
    #[schemars(range(min = 0.0))]
    pub quantity: Option<f64>,
    /// Dollar amount to invest (mutually exclusive with quantity, buy only)
    #[serde(default, deserialize_with = "opt_f64_or_str")]
    #[schemars(range(min = 0.0))]
    pub dollar_amount: Option<f64>,
    /// Market or limit
    pub order_type: OrderType,
    /// Required for limit orders
    #[serde(default, deserialize_with = "opt_f64_or_str")]
    #[schemars(range(min = 0.0))]
    pub limit_price: Option<f64>,
    /// Trigger (default: immediate)
    pub trigger: Option<Trigger>,
    /// Required when trigger is "stop"
    #[serde(default, deserialize_with = "opt_f64_or_str")]
    #[schemars(range(min = 0.0))]
    pub stop_price: Option<f64>,
    /// Trading session (default: regular_hours)
    pub market_hours: Option<MarketHours>,
    /// Duration policy for the order
    pub time_in_force: TimeInForce,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelOrderParams {
    /// The order ID to cancel - call get_open_orders first to discover valid IDs.
    pub order_id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelOptionOrderParams {
    /// The option order ID to cancel - call get_open_option_orders first to discover valid IDs.
    pub order_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct OptionChainParams {
    /// Stock symbol to get option chain for
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    pub symbol: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct FindOptionsParams {
    /// Stock symbol (e.g. "AAPL")
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    pub symbol: String,
    /// Expiration date in YYYY-MM-DD format
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    pub expiration_date: String,
    /// "call" or "put"
    pub option_type: OptionType,
    /// Strike price (optional filter)
    pub strike_price: Option<f64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceOptionOrderParams {
    /// Stock symbol (e.g. "AAPL")
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    pub symbol: String,
    /// Expiration date in YYYY-MM-DD format
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    pub expiration_date: String,
    /// Strike price
    #[serde(deserialize_with = "f64_or_str")]
    #[schemars(range(min = 0.0))]
    pub strike_price: f64,
    /// Option type
    pub option_type: OptionType,
    /// Buy or sell
    pub side: Side,
    /// Number of contracts
    #[serde(deserialize_with = "f64_or_str")]
    #[schemars(range(min = 0.0))]
    pub quantity: f64,
    /// Limit price per contract
    #[serde(deserialize_with = "f64_or_str")]
    #[schemars(range(min = 0.0))]
    pub limit_price: f64,
    /// Duration policy for the order
    pub time_in_force: TimeInForce,
}

#[derive(Deserialize, JsonSchema)]
pub struct FundamentalsParams {
    /// Stock ticker symbols, e.g. `["AAPL", "TSLA"]`
    #[schemars(length(max = 50))]
    pub symbols: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct LatestPricesParams {
    /// Stock ticker symbols, e.g. `["AAPL", "TSLA"]`
    #[schemars(length(max = 50))]
    pub symbols: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct OptionQuoteContract {
    /// Strike price (e.g. 50.0)
    #[schemars(range(min = 0.0))]
    pub strike_price: f64,
    /// Expiration date in YYYY-MM-DD format
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    pub expiration_date: String,
    /// Option type
    pub option_type: OptionType,
}

pub enum OptionQuoteParams {
    /// Discover contracts from their attributes, then quote them.
    Contracts {
        symbol: String,
        contracts: Vec<OptionQuoteContract>,
    },
    /// Quote held positions directly from their option-instrument URLs.
    InstrumentUrls { instrument_urls: Vec<String> },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionQuoteParamsInput {
    symbol: Option<String>,
    contracts: Option<Vec<OptionQuoteContract>>,
    instrument_urls: Option<Vec<String>>,
}

#[expect(
    dead_code,
    reason = "this schema-only mirror is consumed through its JsonSchema implementation"
)]
#[derive(JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
enum OptionQuoteParamsSchema {
    Contracts {
        /// Stock symbol (e.g. "NKE")
        #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
        symbol: String,
        /// Non-empty list of option contracts to discover and quote
        #[schemars(length(min = 1))]
        contracts: Vec<OptionQuoteContract>,
    },
    InstrumentUrls {
        /// Non-empty list of option-instrument URLs returned by get_option_positions
        #[schemars(
            length(min = 1),
            inner(regex(pattern = OPTION_INSTRUMENT_URL_PATTERN))
        )]
        instrument_urls: Vec<String>,
    },
}

impl<'de> Deserialize<'de> for OptionQuoteParams {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let input = OptionQuoteParamsInput::deserialize(deserializer)?;
        match (input.symbol, input.contracts, input.instrument_urls) {
            (Some(symbol), Some(contracts), None) => {
                if contracts.is_empty() {
                    return Err(serde::de::Error::custom(
                        "contracts must contain at least one option contract",
                    ));
                }
                Ok(Self::Contracts { symbol, contracts })
            }
            (None, None, Some(instrument_urls)) => {
                if instrument_urls.is_empty() {
                    return Err(serde::de::Error::custom(
                        "instrument_urls must contain at least one option-instrument URL",
                    ));
                }
                let instrument_urls = instrument_urls
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| {
                        canonicalize_option_instrument_url(&value).map_err(|error| {
                            serde::de::Error::custom(format!(
                                "invalid instrument_urls[{index}]: {error}"
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>, D::Error>>()?;
                Ok(Self::InstrumentUrls { instrument_urls })
            }
            (None, None, None) => Err(serde::de::Error::custom(
                "provide exactly one quote mode: {symbol, contracts} or {instrument_urls}",
            )),
            (Some(_), None, None) | (None, Some(_), None) => Err(serde::de::Error::custom(
                "symbol and contracts must be provided together",
            )),
            _ => Err(serde::de::Error::custom(
                "instrument_urls cannot be combined with symbol or contracts; choose exactly one quote mode",
            )),
        }
    }
}

impl JsonSchema for OptionQuoteParams {
    fn schema_name() -> Cow<'static, str> {
        "OptionQuoteParams".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut schema = OptionQuoteParamsSchema::json_schema(generator);
        let object = schema.ensure_object();
        if let Some(variants) = object.remove("anyOf") {
            let mut properties = serde_json::Map::new();
            if let Some(branches) = variants.as_array() {
                for branch in branches {
                    if let Some(branch_properties) = branch
                        .get("properties")
                        .and_then(serde_json::Value::as_object)
                    {
                        properties.extend(branch_properties.clone());
                    }
                }
            }
            object.insert("type".to_string(), serde_json::json!("object"));
            object.insert("properties".to_string(), properties.into());
            object.insert("oneOf".to_string(), variants);
        }
        schema
    }
}

fn canonicalize_option_instrument_url(value: &str) -> Result<String, String> {
    let parsed =
        Url::parse(value).map_err(|error| format!("expected an absolute HTTPS URL: {error}"))?;
    if parsed.scheme() != "https" {
        return Err("URL scheme must be https".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("URL must not contain userinfo".to_string());
    }
    if parsed.host_str() != Some("api.robinhood.com") {
        return Err("URL host must be exactly api.robinhood.com".to_string());
    }

    let (_raw_scheme, after_scheme) = value
        .split_once("://")
        .ok_or_else(|| "expected an absolute HTTPS URL".to_string())?;
    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let authority = after_scheme
        .get(..authority_end)
        .ok_or_else(|| "URL authority is not valid UTF-8".to_string())?;
    if authority.contains(':') {
        return Err("URL must not contain an explicit port".to_string());
    }
    if !authority.eq_ignore_ascii_case("api.robinhood.com") {
        return Err("URL host must be exactly api.robinhood.com".to_string());
    }
    if parsed.query().is_some() {
        return Err("URL must not contain a query".to_string());
    }
    if parsed.fragment().is_some() {
        return Err("URL must not contain a fragment".to_string());
    }

    fn exact_instrument_id(path: &str) -> Option<&str> {
        path.strip_prefix("/options/instruments/")
            .map(|remainder| remainder.strip_suffix('/').unwrap_or(remainder))
            .filter(|instrument_id| !instrument_id.is_empty() && !instrument_id.contains('/'))
    }
    let raw_path = after_scheme
        .get(authority_end..)
        .ok_or_else(|| "URL path is not valid UTF-8".to_string())?;
    let Some(raw_instrument_id) = exact_instrument_id(raw_path) else {
        return Err(
            "URL path must be /options/instruments/<non-empty-id> with no additional segment"
                .to_string(),
        );
    };
    if exact_instrument_id(parsed.path()).is_none() {
        return Err(
            "URL path must be /options/instruments/<non-empty-id> with no additional segment"
                .to_string(),
        );
    }

    let normalized_id = raw_instrument_id.to_ascii_lowercase();
    let has_encoded_separator = normalized_id.contains("%2f") || normalized_id.contains("%5c");
    let has_encoded_traversal = normalized_id.contains("%2e%2e")
        || normalized_id.contains("%2e.")
        || normalized_id.contains(".%2e");
    if has_encoded_separator || has_encoded_traversal {
        return Err(
            "option-instrument ID must not contain percent-encoded path separators or traversal"
                .to_string(),
        );
    }

    let mut canonical = parsed.to_string();
    if !canonical.ends_with('/') {
        canonical.push('/');
    }
    Ok(canonical)
}

#[derive(Deserialize, JsonSchema)]
pub struct MarketHoursParams {
    /// Market Identifier Code (e.g. "XNYS", "XNAS")
    #[schemars(length(max = 8), regex(pattern = r"^[A-Z]{1,8}$"))]
    pub mic: String,
    /// Date in YYYY-MM-DD format
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    pub date: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct MarketTodayHoursParams {
    /// Market Identifier Code (e.g. "XNYS", "XNAS")
    #[schemars(length(max = 8), regex(pattern = r"^[A-Z]{1,8}$"))]
    pub mic: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct StockOrderHistoryParams {
    /// Filter orders updated since this date (ISO 8601, e.g. "2025-01-01")
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    pub since: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct OptionOrderHistoryParams {
    /// Filter orders updated since this date (ISO 8601, e.g. "2025-01-01")
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    pub since: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct FuturesContractParams {
    /// Futures contract symbol (e.g., "ESH26")
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    pub symbol: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct FuturesQuoteParams {
    /// Futures contract symbols (e.g., ["ESH26", "NQH26"])
    #[schemars(length(max = 50))]
    pub symbols: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct FuturesOrderHistoryParams {
    /// Filter orders updated since this date (ISO 8601, e.g. "2025-01-01")
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    pub since: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct IndexQuoteParams {
    /// Index symbols (e.g., ["SPX", "NDX", "VIX"])
    #[schemars(length(max = 50))]
    pub symbols: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct IndexOptionChainParams {
    /// Index symbol (e.g., "SPX")
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    pub symbol: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct FindIndexOptionsParams {
    /// Index symbol (e.g., "SPX")
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    pub symbol: String,
    /// Expiration date in YYYY-MM-DD format
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    pub expiration_date: String,
    /// "call" or "put"
    pub option_type: OptionType,
    /// Strike price (optional filter)
    pub strike_price: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DividendParams {
    /// Filter dividends updated since this date (ISO 8601, e.g. "2025-01-01")
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    pub since: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRecurringInvestmentParams {
    /// Stock symbol (e.g. "TSLA")
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    pub symbol: String,
    /// Dollar amount per recurrence
    #[schemars(range(min = 0.0))]
    pub amount: f64,
    /// Recurrence frequency
    pub frequency: RecurringFrequency,
    /// Start date in YYYY-MM-DD format
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    pub start_date: String,
    /// Source of funds (default: buying_power)
    #[serde(default)]
    pub source_of_funds: RecurringSource,
}

/// States an MCP caller may set on `update_recurring_investment`.
///
/// Deliberately excludes `RecurringState::Deleted` - cancellation goes through
/// the dedicated `cancel_recurring_investment` tool, so `"deleted"` is not a
/// valid input here and is rejected with a structured `-32602` error.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UpdatableRecurringState {
    /// Resume the schedule.
    Active,
    /// Pause the schedule (it persists but executes no investments).
    Paused,
}

impl From<UpdatableRecurringState> for RecurringState {
    fn from(state: UpdatableRecurringState) -> Self {
        match state {
            UpdatableRecurringState::Active => RecurringState::Active,
            UpdatableRecurringState::Paused => RecurringState::Paused,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateRecurringInvestmentParams {
    /// Schedule ID to update - call get_recurring_investments first to discover valid IDs.
    pub schedule_id: String,
    /// New dollar amount (optional)
    #[schemars(range(min = 0.0))]
    pub amount: Option<f64>,
    /// New frequency (optional)
    pub frequency: Option<RecurringFrequency>,
    /// New state (optional)
    pub state: Option<UpdatableRecurringState>,
    /// New start date (optional)
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    pub start_date: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelRecurringInvestmentParams {
    /// Schedule ID to cancel - call get_recurring_investments first to discover valid IDs.
    pub schedule_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct NextInvestmentDateParams {
    /// Recurrence frequency
    pub frequency: RecurringFrequency,
    /// Start date in YYYY-MM-DD format
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    pub start_date: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct EarningsParams {
    /// Stock ticker symbol (e.g. "AAPL")
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    pub symbol: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct RatingsParams {
    /// Stock ticker symbol (e.g. "AAPL")
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    pub symbol: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct NewsParams {
    /// Stock ticker symbol (e.g. "AAPL")
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    pub symbol: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct SplitsParams {
    /// Stock ticker symbol (e.g. "AAPL")
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    pub symbol: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct TagsParams {
    /// Tag slug (e.g. "100-most-popular")
    #[schemars(length(max = 100))]
    pub tag: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WatchlistNameParams {
    /// Watchlist name or ID - call get_watchlists first to discover valid names/IDs.
    #[schemars(length(max = 100))]
    pub name: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WatchlistModifyParams {
    /// Watchlist name or ID - call get_watchlists first to discover valid names/IDs.
    #[schemars(length(max = 100))]
    pub name: String,
    /// Stock symbols to add or remove
    #[schemars(length(max = 50))]
    pub symbols: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DocumentsParams {
    /// Document type filter (optional)
    pub doc_type: Option<DocumentType>,
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    reason = "these deserialization tests verify exact parsing of string-encoded decimal literals"
)]
mod tests {
    use super::*;

    const CONTRACT: &str =
        r#"{"strike_price":310.0,"expiration_date":"2026-06-18","option_type":"call"}"#;

    #[test]
    fn option_quotes_accepts_attribute_mode_unchanged() {
        let json = format!(r#"{{"symbol":"AAPL","contracts":[{CONTRACT}]}}"#);
        let parsed: OptionQuoteParams = serde_json::from_str(&json).unwrap();
        match parsed {
            OptionQuoteParams::Contracts { symbol, contracts } => {
                assert_eq!(symbol, "AAPL");
                assert_eq!(contracts.len(), 1);
                assert_eq!(contracts[0].strike_price, 310.0);
            }
            OptionQuoteParams::InstrumentUrls { .. } => panic!("expected attribute mode"),
        }
    }

    #[test]
    fn option_quotes_accepts_and_canonicalizes_instrument_url_mode() {
        let parsed: OptionQuoteParams = serde_json::from_str(
            r#"{"instrument_urls":["https://api.robinhood.com/options/instruments/not-a-uuid","https://api.robinhood.com/options/instruments/already-slash/"]}"#,
        )
        .unwrap();
        match parsed {
            OptionQuoteParams::InstrumentUrls { instrument_urls } => assert_eq!(
                instrument_urls,
                [
                    "https://api.robinhood.com/options/instruments/not-a-uuid/",
                    "https://api.robinhood.com/options/instruments/already-slash/",
                ]
            ),
            OptionQuoteParams::Contracts { .. } => panic!("expected instrument URL mode"),
        }
    }

    #[test]
    fn option_quotes_rejects_every_invalid_mode_combination() {
        let contracts = format!(r#"[{CONTRACT}]"#);
        let cases = [
            ("missing both modes", "{}".to_string()),
            ("symbol without contracts", r#"{"symbol":"AAPL"}"#.to_string()),
            ("contracts without symbol", format!(r#"{{"contracts":{contracts}}}"#)),
            (
                "symbol mixed with URLs",
                r#"{"symbol":"AAPL","instrument_urls":["https://api.robinhood.com/options/instruments/id/"]}"#.to_string(),
            ),
            (
                "contracts mixed with URLs",
                format!(r#"{{"contracts":{contracts},"instrument_urls":["https://api.robinhood.com/options/instruments/id/"]}}"#),
            ),
            (
                "both complete modes",
                format!(r#"{{"symbol":"AAPL","contracts":{contracts},"instrument_urls":["https://api.robinhood.com/options/instruments/id/"]}}"#),
            ),
            ("empty contracts", r#"{"symbol":"AAPL","contracts":[]}"#.to_string()),
            ("empty URLs", r#"{"instrument_urls":[]}"#.to_string()),
        ];

        for (name, json) in cases {
            let error = serde_json::from_str::<OptionQuoteParams>(&json)
                .err()
                .unwrap_or_else(|| panic!("{name} must fail"));
            let message = error.to_string();
            assert!(
                message.contains("mode")
                    || message.contains("together")
                    || message.contains("at least one"),
                "{name} must return an actionable error, got: {message}"
            );
        }
    }

    #[test]
    fn option_quotes_rejects_invalid_instrument_url_shapes() {
        let cases = [
            "options/instruments/id/",
            "http://api.robinhood.com/options/instruments/id/",
            "https://example.com/options/instruments/id/",
            "https://api.robinhood.com.evil.test/options/instruments/id/",
            "https://user@api.robinhood.com/options/instruments/id/",
            "https://api.robinhood.com:8443/options/instruments/id/",
            "https://api.robinhood.com:443/options/instruments/id/",
            "https://api.robinhood.com/options/instruments/id/?query=yes",
            "https://api.robinhood.com/options/instruments/id/#fragment",
            "https://api.robinhood.com/options/instruments/",
            "https://api.robinhood.com/options/instruments/id/extra",
            "https://api.robinhood.com/options/instruments/id//",
            "https://api.robinhood.com/instruments/id/",
            "https://api.robinhood.com/options/instruments/./",
            "https://api.robinhood.com/options/instruments/%2e%2e/",
        ];

        for value in cases {
            let json = serde_json::json!({"instrument_urls": [value]}).to_string();
            let error = serde_json::from_str::<OptionQuoteParams>(&json)
                .err()
                .unwrap_or_else(|| panic!("invalid URL must fail: {value}"));
            assert!(
                error.to_string().contains("instrument_urls[0]"),
                "URL error must identify the invalid element for {value}: {error}"
            );
        }
    }

    #[test]
    fn option_quotes_rejects_encoded_path_syntax_in_nonempty_instrument_ids() {
        let cases = [
            (
                "lowercase encoded slash",
                "https://api.robinhood.com/options/instruments/id%2f../",
            ),
            (
                "uppercase encoded slash",
                "https://api.robinhood.com/options/instruments/id%2Fchild/",
            ),
            (
                "encoded traversal after a nonempty prefix",
                "https://api.robinhood.com/options/instruments/held%2E%2e/",
            ),
        ];

        for (name, value) in cases {
            let json = serde_json::json!({"instrument_urls": [value]}).to_string();
            let error = serde_json::from_str::<OptionQuoteParams>(&json)
                .err()
                .unwrap_or_else(|| panic!("{name} must fail: {value}"));
            let message = error.to_string();
            assert!(
                message.contains("percent-encoded path separators or traversal"),
                "{name} must identify the forbidden encoded path syntax, got: {message}"
            );
        }
    }

    #[test]
    fn stock_order_accepts_string_and_number_prices() {
        let from_str: PlaceStockOrderParams = serde_json::from_str(
            r#"{"symbol":"F","side":"buy","quantity":"1","order_type":"limit","limit_price":"15.00","time_in_force":"gtc"}"#,
        )
        .unwrap();
        let from_num: PlaceStockOrderParams = serde_json::from_str(
            r#"{"symbol":"F","side":"buy","quantity":1,"order_type":"limit","limit_price":15.0,"time_in_force":"gfd"}"#,
        )
        .unwrap();
        assert_eq!(from_str.limit_price, Some(15.0));
        assert_eq!(from_str.quantity, Some(1.0));
        assert_eq!(from_num.limit_price, Some(15.0));
    }

    #[test]
    fn option_order_accepts_string_prices() {
        let parsed: PlaceOptionOrderParams = serde_json::from_str(
            r#"{"symbol":"AAPL","expiration_date":"2026-06-18","strike_price":"400","option_type":"call","side":"buy","quantity":"1","limit_price":"0.01","time_in_force":"gtc"}"#,
        )
        .unwrap();
        assert_eq!(parsed.strike_price, 400.0);
        assert_eq!(parsed.quantity, 1.0);
        assert_eq!(parsed.limit_price, 0.01);
    }

    #[test]
    fn rejects_non_numeric_string() {
        let err = serde_json::from_str::<PlaceOptionOrderParams>(
            r#"{"symbol":"AAPL","expiration_date":"2026-06-18","strike_price":"abc","option_type":"call","side":"buy","quantity":"1","limit_price":"0.01"}"#,
        );
        assert!(err.is_err());
    }

    #[test]
    fn update_recurring_state_rejects_deleted() {
        let err = serde_json::from_str::<UpdateRecurringInvestmentParams>(
            r#"{"schedule_id":"s1","state":"deleted"}"#,
        );
        assert!(
            err.is_err(),
            "state \"deleted\" must be rejected on the MCP surface"
        );
    }

    #[test]
    fn update_recurring_state_accepts_active_and_paused() {
        let active: UpdateRecurringInvestmentParams =
            serde_json::from_str(r#"{"schedule_id":"s1","state":"active"}"#).unwrap();
        assert!(matches!(
            active.state,
            Some(UpdatableRecurringState::Active)
        ));
        let paused: UpdateRecurringInvestmentParams =
            serde_json::from_str(r#"{"schedule_id":"s1","state":"paused"}"#).unwrap();
        assert!(matches!(
            paused.state,
            Some(UpdatableRecurringState::Paused)
        ));
    }

    #[test]
    fn stock_history_rejects_unknown_interval() {
        let err = serde_json::from_str::<StockHistoryParams>(
            r#"{"symbol":"AAPL","interval":"bogus","span":"week"}"#,
        );
        assert!(err.is_err(), "unknown interval must be rejected");
    }

    #[test]
    fn stock_history_accepts_known_enum_values() {
        let p: StockHistoryParams =
            serde_json::from_str(r#"{"symbol":"AAPL","interval":"5minute","span":"5year"}"#)
                .unwrap();
        assert!(matches!(p.interval, HistoricalInterval::FiveMinute));
        assert!(matches!(p.span, HistoricalSpan::FiveYear));
    }

    #[test]
    fn place_stock_order_rejects_unknown_side() {
        let err = serde_json::from_str::<PlaceStockOrderParams>(
            r#"{"symbol":"F","side":"hodl","order_type":"market","quantity":1}"#,
        );
        assert!(err.is_err(), "unknown side must be rejected");
    }

    #[test]
    fn symbol_schema_is_bounded() {
        let schema = schemars::schema_for!(EarningsParams);
        let json = serde_json::to_value(&schema).unwrap();
        let sym = &json["properties"]["symbol"];
        assert!(
            sym.get("maxLength").is_some() || sym.get("pattern").is_some(),
            "symbol must carry a length/pattern bound: {sym}"
        );
    }

    #[test]
    fn strike_price_schema_has_minimum() {
        let schema = schemars::schema_for!(PlaceOptionOrderParams);
        let json = serde_json::to_value(&schema).unwrap();
        assert!(
            json["properties"]["strike_price"].get("minimum").is_some(),
            "strike_price must declare a minimum"
        );
    }

    #[test]
    fn opaque_id_schema_descriptions_name_discovery_tools() {
        let cases = [
            (
                schemars::schema_for!(UpdateRecurringInvestmentParams),
                "schedule_id",
                "Schedule ID to update - call get_recurring_investments first to discover valid IDs.",
            ),
            (
                schemars::schema_for!(CancelRecurringInvestmentParams),
                "schedule_id",
                "Schedule ID to cancel - call get_recurring_investments first to discover valid IDs.",
            ),
            (
                schemars::schema_for!(CancelOrderParams),
                "order_id",
                "The order ID to cancel - call get_open_orders first to discover valid IDs.",
            ),
            (
                schemars::schema_for!(CancelOptionOrderParams),
                "order_id",
                "The option order ID to cancel - call get_open_option_orders first to discover valid IDs.",
            ),
        ];

        for (schema, field, expected_description) in cases {
            let json = serde_json::to_value(&schema).unwrap();
            assert_eq!(
                json["properties"][field]["description"], expected_description,
                "schema description for {field}"
            );
        }
    }

    #[test]
    fn place_stock_order_rejects_unknown_field() {
        let err = serde_json::from_str::<PlaceStockOrderParams>(
            r#"{"symbol":"F","side":"buy","order_type":"market","quantity":1,"evil":"x"}"#,
        );
        assert!(
            err.is_err(),
            "unknown field must be rejected on write structs"
        );
    }
}
