use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};

use super::handler::RhoodTools;
use super::params::*;
use super::types::format_tool_error;

#[tool_router(router = index_router, vis = "pub(super)")]
impl RhoodTools {
    #[tool(
        name = "get_index_quotes",
        description = "Get real-time market data for index symbols (SPX, NDX, VIX, RUT, XSP)",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_index_quotes(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<IndexQuoteParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let mut quotes = Vec::new();
        for symbol in &params.symbols {
            let quote = client
                .get_index_quote(symbol)
                .await
                .map_err(|rhood_error| format_tool_error(&rhood_error))?;
            quotes.push(quote);
        }
        serde_json::to_string_pretty(&quotes).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_index_option_chain",
        description = "Get option chain metadata for an index symbol",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_index_option_chain(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<IndexOptionChainParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let chain = client
            .get_index_option_chain(&params.symbol)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&chain).map_err(|error| error.to_string())
    }

    #[tool(
        name = "find_index_options",
        description = "Search for index option contracts by expiration, type, and optional strike",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn find_index_options(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<FindIndexOptionsParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let strike_str = params.strike_price.map(|price| format!("{price:.4}"));
        let options = client
            .find_index_options(
                &params.symbol,
                &params.expiration_date,
                params.option_type,
                strike_str.as_deref(),
            )
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&options).map_err(|error| error.to_string())
    }
}
