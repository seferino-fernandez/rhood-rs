use rhood_core::models::document::DocumentType;
use rhood_core::models::option::OptionType;
use rhood_core::models::order::{MarketHours, OrderType, Side, Trigger};
use rhood_core::models::recurring::{RecurringFrequency, RecurringSource, RecurringState};
use rhood_core::models::stock::{HistoricalInterval, HistoricalSpan};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

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
            .map_err(|_| format!("expected a number, got string {s:?}")),
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

#[derive(Deserialize, JsonSchema)]
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
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfirmOrderParams {
    /// The pending_order_id returned by place_stock_order
    pub pending_order_id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelOrderParams {
    /// The order ID to cancel
    pub order_id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelOptionOrderParams {
    /// The option order ID to cancel
    pub order_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct OptionChainParams {
    /// Stock symbol to get option chain for
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    pub symbol: String,
}

#[derive(Deserialize, JsonSchema)]
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

#[derive(Deserialize, JsonSchema)]
pub struct OptionQuoteParams {
    /// Stock symbol (e.g. "NKE")
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    pub symbol: String,
    /// List of option contracts to quote
    pub contracts: Vec<OptionQuoteContract>,
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
/// Deliberately excludes `RecurringState::Deleted` — cancellation goes through
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
    /// Schedule ID to update
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
    /// Schedule ID to cancel
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
    /// Watchlist name or ID — call get_watchlists first to discover valid names/IDs.
    #[schemars(length(max = 100))]
    pub name: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WatchlistModifyParams {
    /// Watchlist name or ID — call get_watchlists first to discover valid names/IDs.
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
mod tests {
    use super::*;

    #[test]
    fn stock_order_accepts_string_and_number_prices() {
        let from_str: PlaceStockOrderParams = serde_json::from_str(
            r#"{"symbol":"F","side":"buy","quantity":"1","order_type":"limit","limit_price":"15.00"}"#,
        )
        .unwrap();
        let from_num: PlaceStockOrderParams = serde_json::from_str(
            r#"{"symbol":"F","side":"buy","quantity":1,"order_type":"limit","limit_price":15.0}"#,
        )
        .unwrap();
        assert_eq!(from_str.limit_price, Some(15.0));
        assert_eq!(from_str.quantity, Some(1.0));
        assert_eq!(from_num.limit_price, Some(15.0));
    }

    #[test]
    fn option_order_accepts_string_prices() {
        let parsed: PlaceOptionOrderParams = serde_json::from_str(
            r#"{"symbol":"AAPL","expiration_date":"2026-06-18","strike_price":"400","option_type":"call","side":"buy","quantity":"1","limit_price":"0.01"}"#,
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
