use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};

use super::handler::RhoodTools;
use super::params::*;
use super::types::format_tool_error;

fn serialize_futures_positions(
    positions: &[rhood_core::models::futures::FuturesPosition],
) -> Result<String, String> {
    serde_json::to_string_pretty(positions).map_err(|error| error.to_string())
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
        let enriched = super::enrichment::enrich_futures_quotes(quotes, &params.symbols);
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
    use super::{RhoodTools, serialize_futures_positions};

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
}
