use rhood_core::models::futures::FuturesQuote;
use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

use super::handler::RhoodTools;
use super::types::format_tool_error;

#[derive(Deserialize, JsonSchema)]
struct FuturesContractParams {
    /// Futures contract symbol (e.g., "ESH26")
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    symbol: String,
}

#[derive(Deserialize, JsonSchema)]
struct FuturesQuoteParams {
    /// Futures contract symbols (e.g., ["ESH26", "NQH26"])
    #[schemars(length(max = 50))]
    symbols: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
struct FuturesOrderHistoryParams {
    /// Filter orders updated since this date (ISO 8601, e.g. "2025-01-01")
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    since: Option<String>,
}

fn serialize_futures_positions(
    positions: &[rhood_core::models::futures::FuturesPosition],
) -> Result<String, String> {
    serde_json::to_string_pretty(positions).map_err(|error| error.to_string())
}

/// A futures quote with the caller's requested alias echoed under a distinct
/// `requested_symbol` key.
///
/// The flattened `FuturesQuote` keeps its authoritative wire `symbol` (e.g.
/// `"/ESM26:XCME"`); `requested_symbol` is the alias the caller passed (e.g.
/// `"ESM26"`). Because the added key is `requested_symbol`, never `symbol`,
/// there is no duplicate-key collision.
#[derive(Serialize)]
struct FuturesQuoteView {
    #[serde(flatten)]
    quote: FuturesQuote,
    requested_symbol: String,
}

/// Wraps each owned `FuturesQuote` with its requested alias, backfilling the
/// wire `symbol` from the alias only when the upstream payload omitted it.
fn enrich_futures_quotes(quotes: Vec<FuturesQuote>, requested: &[String]) -> Vec<FuturesQuoteView> {
    quotes
        .into_iter()
        .zip(requested.iter())
        .map(|(mut quote, req)| {
            if quote.symbol.is_none() {
                quote.symbol = Some(req.clone());
            }
            FuturesQuoteView {
                quote,
                requested_symbol: req.clone(),
            }
        })
        .collect()
}

#[tool_router(router = futures_router, vis = "pub(super)")]
impl RhoodTools {
    #[tool(
        name = "get_futures_contract",
        description = "Look up a futures contract by symbol (e.g., ESH26)",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_futures_contract(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<FuturesContractParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let contract = client
            .get_futures_contract(&params.symbol)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&contract).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_futures_quotes",
        description = "Get real-time futures quotes for one or more symbols",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_futures_quotes(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<FuturesQuoteParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let symbol_refs: Vec<&str> = params.symbols.iter().map(String::as_str).collect();
        let quotes = client
            .get_futures_quotes(&symbol_refs)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        // Echo each requested alias under `requested_symbol`, distinct from the
        // authoritative wire `symbol` (e.g. requested "ESM26" vs wire
        // "/ESM26:XCME"). Positional zip relies on the upstream `?ids=` batch
        // returning quotes in request order; `get_futures_quotes` errors if any
        // symbol fails to resolve, so a short/reordered batch is not expected.
        let enriched = enrich_futures_quotes(quotes, &params.symbols);
        serde_json::to_string_pretty(&enriched).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_all_futures_orders",
        description = "Get futures order history (all states). Requires a futures account.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_all_futures_orders(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<FuturesOrderHistoryParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let orders = client
            .get_all_futures_orders(params.since.as_deref())
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&orders).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_futures_positions",
        description = "Get all futures positions for the futures account",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_futures_positions(&self, peer: Peer<RoleServer>) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let positions = client
            .get_futures_positions()
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serialize_futures_positions(&positions)
    }

    #[tool(
        name = "get_futures_account",
        description = "Discover the Robinhood futures account ID",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_futures_account(&self, peer: Peer<RoleServer>) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let account_id = client
            .get_futures_account_id()
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        let result = serde_json::json!({
            "futures_account_id": account_id,
        });
        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{RhoodTools, enrich_futures_quotes, serialize_futures_positions};
    use rhood_core::models::futures::FuturesQuote;

    #[test]
    fn futures_positions_tool_is_registered_without_parameters_and_read_only() {
        let tool = RhoodTools::futures_router()
            .list_all()
            .into_iter()
            .find(|tool| tool.name == "get_futures_positions")
            .expect("get_futures_positions must be advertised");

        assert_eq!(
            tool.input_schema.get("properties"),
            Some(&serde_json::json!({}))
        );
        assert!(
            tool.annotations
                .as_ref()
                .is_some_and(|annotations| annotations.read_only_hint == Some(true))
        );
    }

    #[test]
    fn futures_positions_serialization_is_pretty_and_preserves_raw_objects() {
        let positions = vec![
            serde_json::from_value(serde_json::json!({
                "mark": 5182.25,
                "metadata": {"source": "wire"},
                "legs": [{"side": "long"}]
            }))
            .unwrap(),
        ];

        let json = serialize_futures_positions(&positions).unwrap();

        assert_eq!(
            json,
            r#"[
  {
    "legs": [
      {
        "side": "long"
      }
    ],
    "mark": 5182.25,
    "metadata": {
      "source": "wire"
    }
  }
]"#
        );
    }

    #[test]
    fn futures_view_has_single_symbol_and_requested_symbol() {
        let quote: FuturesQuote = serde_json::from_str(r#"{"symbol":"/ESM26:XCME"}"#).unwrap();
        let out = enrich_futures_quotes(vec![quote], &["ESM26".to_string()]);
        let json = serde_json::to_string(&out).unwrap();
        assert_eq!(
            json.matches("\"symbol\"").count(),
            1,
            "dup symbol key: {json}"
        );
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value[0]["symbol"], "/ESM26:XCME");
        assert_eq!(value[0]["requested_symbol"], "ESM26");
    }

    #[test]
    fn futures_view_backfills_symbol_when_wire_missing() {
        let quote: FuturesQuote = serde_json::from_str(r#"{"symbol":null}"#).unwrap();
        let out = enrich_futures_quotes(vec![quote], &["ESM26".to_string()]);
        let value: serde_json::Value = serde_json::to_value(&out).unwrap();
        assert_eq!(value[0]["symbol"], "ESM26");
        assert_eq!(value[0]["requested_symbol"], "ESM26");
    }
}
