use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};

use super::handler::RhoodTools;
use super::params::*;
use super::types::format_tool_error;

/// Pairs each `OptionMarketData` result with the contract identity the caller
/// supplied (underlying symbol, strike, expiration, type), so multi-contract
/// responses don't have to be correlated by array position. `OptionMarketData`
/// has none of these fields, so flattening it adds no duplicate keys.
#[derive(serde::Serialize)]
struct OptionQuoteView {
    symbol: String,
    strike_price: f64,
    expiration_date: String,
    option_type: String,
    #[serde(flatten)]
    data: rhood_core::models::option::OptionMarketData,
}

#[tool_router(router = option_router, vis = "pub(super)")]
impl RhoodTools {
    #[tool(
        name = "get_option_positions",
        description = "Get all open option positions (contracts currently held)",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_option_positions(&self, peer: Peer<RoleServer>) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let positions = client
            .get_open_option_positions()
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&positions).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_option_chain",
        description = "Get option chain metadata (available expiration dates, multiplier) for a stock symbol",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_option_chain(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<OptionChainParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let chain = client
            .get_option_chain(&params.symbol)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&chain).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_option_quotes",
        description = "Get live quotes for specific option contracts including bid/ask, Greeks, volume, and open interest",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_option_quotes(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<OptionQuoteParams>,
    ) -> Result<String, String> {
        use rhood_core::models::option::OptionContractSpec;

        if params.contracts.is_empty() {
            return Err("At least one contract is required".into());
        }

        let client = self.ensure_client(&peer).await?;
        let strike_strings: Vec<String> = params
            .contracts
            .iter()
            .map(|contract| format!("{:.4}", contract.strike_price))
            .collect();
        let type_strings: Vec<String> = params
            .contracts
            .iter()
            .map(|contract| contract.option_type.to_string())
            .collect();

        let specs: Vec<OptionContractSpec<'_>> = params
            .contracts
            .iter()
            .zip(strike_strings.iter())
            .zip(type_strings.iter())
            .map(|((contract, strike_str), type_str)| OptionContractSpec {
                strike_price: strike_str,
                expiration_date: &contract.expiration_date,
                option_type: type_str,
            })
            .collect();

        let data = client
            .get_option_market_data(&params.symbol, &specs)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        // Upstream preserves request order, so zip the input contracts back
        // onto each result to restore strike/expiration/type identity.
        let views: Vec<OptionQuoteView> = data
            .into_iter()
            .zip(params.contracts.iter())
            .map(|(data, contract)| OptionQuoteView {
                symbol: params.symbol.clone(),
                strike_price: contract.strike_price,
                expiration_date: contract.expiration_date.clone(),
                option_type: contract.option_type.to_string(),
                data,
            })
            .collect();
        serde_json::to_string_pretty(&views).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_option_orders",
        description = "Get full option order history (all states: filled, cancelled, queued, etc.)",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_option_orders(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<OptionOrderHistoryParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let orders = client
            .get_all_option_orders(params.since.as_deref())
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&orders).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_open_option_orders",
        description = "List all open option orders",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_open_option_orders(&self, peer: Peer<RoleServer>) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let orders = client
            .get_open_option_orders()
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&orders).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::OptionQuoteView;
    use rhood_core::models::option::OptionMarketData;

    #[test]
    fn option_quote_view_carries_contract_identity() {
        let data: OptionMarketData =
            serde_json::from_str(r#"{"instrument":null,"instrument_id":"x","bid_price":"1.20"}"#)
                .unwrap();
        let view = OptionQuoteView {
            symbol: "AAPL".into(),
            strike_price: 310.0,
            expiration_date: "2026-06-18".into(),
            option_type: "call".into(),
            data,
        };
        let value = serde_json::to_value(view).unwrap();
        assert_eq!(value["symbol"], "AAPL");
        assert_eq!(value["strike_price"], 310.0);
        assert_eq!(value["expiration_date"], "2026-06-18");
        assert_eq!(value["option_type"], "call");
        assert_eq!(value["bid_price"], "1.20");
    }
}
