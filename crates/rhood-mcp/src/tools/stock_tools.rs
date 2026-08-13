use rhood_core::models::stock::{HistoricalInterval, HistoricalSpan};
use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::handler::{AccountSelectorParams, RhoodTools};
use super::types::format_tool_error;

#[derive(Deserialize, JsonSchema)]
struct StockQuoteParams {
    /// Stock ticker symbols, e.g. `["AAPL", "TSLA"]`
    #[schemars(length(max = 50))]
    symbols: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
struct StockHistoryParams {
    /// Stock ticker symbol
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    symbol: String,
    /// Candle interval
    #[serde(default = "default_interval")]
    interval: HistoricalInterval,
    /// Time span
    #[serde(default = "default_span")]
    span: HistoricalSpan,
}

fn default_interval() -> HistoricalInterval {
    HistoricalInterval::Hour
}

fn default_span() -> HistoricalSpan {
    HistoricalSpan::Week
}

#[derive(Deserialize, JsonSchema)]
struct FundamentalsParams {
    /// Stock ticker symbols, e.g. `["AAPL", "TSLA"]`
    #[schemars(length(max = 50))]
    symbols: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
struct LatestPricesParams {
    /// Stock ticker symbols, e.g. `["AAPL", "TSLA"]`
    #[schemars(length(max = 50))]
    symbols: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
struct StockOrderHistoryParams {
    /// Canonical brokerage account number from get_accounts. May be omitted only when exactly one account exists.
    account_number: Option<String>,
    /// Filter orders updated since this date (ISO 8601, e.g. "2025-01-01")
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    since: Option<String>,
}

/// Object projection for `get_latest_prices` so each entry is
/// `{"symbol": "...", "price": "..."}` instead of a positional `[sym, price]`
/// tuple.
#[derive(serde::Serialize)]
struct LatestPrice {
    symbol: String,
    price: String,
}

/// Serializes candles compactly: a `columns` legend plus an array of
/// positional `[begins_at, open, high, low, close, volume]` tuples. Cuts the
/// token footprint of long histories versus one verbose object per candle.
///
/// This columnar shape intentionally differs from the object-per-row quote
/// tools; it is not an inconsistency to be normalized.
pub(crate) fn compact_history(
    symbol: &str,
    candles: &[rhood_core::models::stock::Candle],
) -> serde_json::Value {
    let rows: Vec<serde_json::Value> = candles
        .iter()
        .map(|candle| {
            serde_json::json!([
                candle.begins_at,
                candle.open_price,
                candle.high_price,
                candle.low_price,
                candle.close_price,
                candle.volume,
            ])
        })
        .collect();
    serde_json::json!({
        "symbol": symbol,
        "columns": ["begins_at", "open", "high", "low", "close", "volume"],
        "candles": rows,
    })
}

#[tool_router(router = stock_router, vis = "pub(super)")]
impl RhoodTools {
    #[tool(
        name = "get_stock_quotes",
        description = "Get full-detail current quotes for stock ticker symbols, including bid/ask, sizes, timestamps, previous close, and trading state. Use get_latest_prices for price-only queries.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_stock_quotes(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<StockQuoteParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let refs: Vec<&str> = params.symbols.iter().map(String::as_str).collect();
        let quotes = client
            .get_quotes(&refs)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&quotes).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_stock_history",
        description = "Get historical price candles for a stock",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_stock_history(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<StockHistoryParams>,
    ) -> Result<String, String> {
        use rhood_core::models::stock::{HistoricalBounds, HistoricalOpts};

        let client = self.ensure_client(&peer).await?;
        let opts = HistoricalOpts {
            interval: params.interval,
            span: params.span,
            bounds: HistoricalBounds::Regular,
        };
        let candles = client
            .get_stock_historicals(&[params.symbol.as_str()], &opts)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        let compact = compact_history(&params.symbol, &candles);
        serde_json::to_string(&compact).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_fundamentals",
        description = "Get fundamental data (sector, P/E, market cap, dividend yield) for stock symbols",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_fundamentals(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<FundamentalsParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let refs: Vec<&str> = params.symbols.iter().map(String::as_str).collect();
        let fundamentals = client
            .get_fundamentals(&refs)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&fundamentals).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_latest_prices",
        description = "Get the compact current trade price for each symbol when only one price per symbol is needed; prefers the extended-hours trade price when present, otherwise the regular last-trade price. Use get_stock_quotes for full quote detail.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_latest_prices(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<LatestPricesParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let refs: Vec<&str> = params.symbols.iter().map(String::as_str).collect();
        let prices = client
            .get_latest_prices(&refs)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        let rows: Vec<LatestPrice> = prices
            .into_iter()
            .map(|(symbol, price)| LatestPrice { symbol, price })
            .collect();
        serde_json::to_string_pretty(&rows).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_open_orders",
        description = "List all open stock orders",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_open_orders(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<AccountSelectorParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let account =
            Self::resolve_account_number(&client, params.account_number.as_deref()).await?;
        let orders = client
            .get_open_stock_orders(&account)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&orders).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_all_stock_orders",
        description = "Get full stock order history (all states: filled, cancelled, queued, etc.)",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_all_stock_orders(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<StockOrderHistoryParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let account =
            Self::resolve_account_number(&client, params.account_number.as_deref()).await?;
        let orders = client
            .get_all_stock_orders(&account, params.since.as_deref())
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&orders).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{RhoodTools, StockHistoryParams, compact_history};
    use rhood_core::models::order::StockOrder;
    use rhood_core::models::stock::{Candle, Fundamentals, HistoricalInterval, HistoricalSpan};

    fn candle(t: &str, o: &str, c: &str) -> Candle {
        Candle {
            begins_at: Some(t.into()),
            open_price: Some(o.into()),
            close_price: Some(c.into()),
            high_price: Some("2".into()),
            low_price: Some("1".into()),
            volume: Some(100),
            session: Some("reg".into()),
            interpolated: None,
            symbol: Some("AAPL".into()),
        }
    }

    #[test]
    fn compact_history_uses_columns_and_tuples() {
        let candles = vec![candle("2026-01-02", "1.5", "1.7")];
        let out = compact_history("AAPL", &candles);
        assert_eq!(out["symbol"], "AAPL");
        assert_eq!(out["columns"][0], "begins_at");
        assert_eq!(out["candles"][0][0], "2026-01-02");
        assert_eq!(out["candles"][0][1], "1.5");
        assert_eq!(out["candles"][0][5], 100);
    }

    #[test]
    fn compact_history_is_smaller_than_verbose() {
        let candles: Vec<Candle> = (0..200)
            .map(|i| candle(&format!("d{i}"), "1", "2"))
            .collect();
        let compact = serde_json::to_string(&compact_history("AAPL", &candles)).unwrap();
        let verbose = serde_json::to_string(&candles).unwrap();
        assert!(
            compact.len() * 2 < verbose.len(),
            "compact={} verbose={}",
            compact.len(),
            verbose.len()
        );
    }

    #[test]
    fn latest_price_serializes_as_object() {
        let row = super::LatestPrice {
            symbol: "AAPL".into(),
            price: "311.35".into(),
        };
        let value = serde_json::to_value([row]).unwrap();
        assert_eq!(value[0]["symbol"], "AAPL");
        assert_eq!(value[0]["price"], "311.35");
        assert!(value[0].is_object(), "expected object, got {value}");
    }

    #[test]
    fn fundamentals_serialize_authoritative_core_symbols_without_backfill() {
        let fundamentals = Fundamentals {
            symbol: Some("TSLA".into()),
            ..Default::default()
        };

        let json = serde_json::to_string(&[fundamentals]).unwrap();
        assert_eq!(
            json.matches("\"symbol\"").count(),
            1,
            "duplicate symbol key: {json}"
        );
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value[0]["symbol"], "TSLA");
    }

    #[test]
    fn stock_orders_serialize_core_owned_wire_symbols_once() {
        let order: StockOrder = serde_json::from_str(
            r#"{"instrument":"https://api.robinhood.com/instruments/abc/","symbol":"RKLB"}"#,
        )
        .unwrap();

        let json = serde_json::to_string(&[order]).unwrap();
        assert_eq!(
            json.matches("\"symbol\"").count(),
            1,
            "duplicate symbol key: {json}"
        );
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value[0]["symbol"], "RKLB");
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
        let params: StockHistoryParams =
            serde_json::from_str(r#"{"symbol":"AAPL","interval":"5minute","span":"5year"}"#)
                .unwrap();
        assert!(matches!(params.interval, HistoricalInterval::FiveMinute));
        assert!(matches!(params.span, HistoricalSpan::FiveYear));
    }

    #[test]
    fn stock_price_tools_advertise_reciprocal_selection_guidance() {
        let tools = RhoodTools::stock_router().list_all();
        let description = |name: &str| {
            tools
                .iter()
                .find(|tool| tool.name == name)
                .and_then(|tool| tool.description.clone())
                .unwrap_or_else(|| panic!("tool {name} must advertise a description"))
        };

        let latest_prices = description("get_latest_prices");
        assert!(
            latest_prices.contains("compact") && latest_prices.contains("get_stock_quotes"),
            "get_latest_prices must recommend itself for compact price-only queries and point to get_stock_quotes for full detail: {latest_prices}"
        );

        let stock_quotes = description("get_stock_quotes");
        assert!(
            stock_quotes.contains("bid/ask")
                && stock_quotes.contains("sizes")
                && stock_quotes.contains("timestamps")
                && stock_quotes.contains("previous close")
                && stock_quotes.contains("trading state")
                && stock_quotes.contains("get_latest_prices"),
            "get_stock_quotes must describe full quote detail and point to get_latest_prices for price-only queries: {stock_quotes}"
        );
    }
}
