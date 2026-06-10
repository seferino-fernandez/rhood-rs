use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};

use crate::tools::enrichment::{
    apply_resolved_symbols, enrich_fundamentals, resolve_urls_to_symbols,
};

use super::handler::RhoodTools;
use super::params::*;
use super::types::format_tool_error;

/// Object projection for `get_latest_prices` so each entry is
/// `{"symbol": "...", "price": "..."}` instead of a positional `[sym, price]`
/// tuple — matching the shape every other tool returns.
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
        description = "Get current quotes for stock ticker symbols",
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
        let enriched = enrich_fundamentals(&fundamentals, &params.symbols);
        serde_json::to_string_pretty(&enriched).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_latest_prices",
        description = "Get latest trade price per symbol (prefers ext-hours price when available)",
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
    async fn get_open_orders(&self, peer: Peer<RoleServer>) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let orders = client
            .get_open_stock_orders()
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        let instrument_urls: Vec<Option<String>> = orders
            .iter()
            .map(|order| order.instrument.clone())
            .collect();
        let resolved = resolve_urls_to_symbols(&client, &instrument_urls).await;
        let enriched = apply_resolved_symbols(orders, &resolved);
        serde_json::to_string_pretty(&enriched).map_err(|error| error.to_string())
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
        let orders = client
            .get_all_stock_orders(params.since.as_deref())
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        let instrument_urls: Vec<Option<String>> = orders
            .iter()
            .map(|order| order.instrument.clone())
            .collect();
        let resolved_symbols = resolve_urls_to_symbols(&client, &instrument_urls).await;
        let enriched_orders = apply_resolved_symbols(orders, &resolved_symbols);
        serde_json::to_string_pretty(&enriched_orders).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::compact_history;
    use rhood_core::models::stock::Candle;

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
}
