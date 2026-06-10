use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};

use super::handler::RhoodTools;
use super::params::*;
use super::types::format_tool_error;

#[tool_router(router = watchlist_router, vis = "pub(super)")]
impl RhoodTools {
    #[tool(
        name = "get_watchlists",
        description = "List all user watchlists",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_watchlists(&self, peer: Peer<RoleServer>) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let lists = client
            .get_watchlists()
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&lists).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_watchlist_items",
        description = "Get items in a named watchlist",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_watchlist_items(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<WatchlistNameParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let items = client
            .get_watchlist_items(&params.name)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&items).map_err(|error| error.to_string())
    }

    #[tool(
        name = "add_to_watchlist",
        description = "Add stock symbols to a watchlist",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn add_to_watchlist(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<WatchlistModifyParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let refs: Vec<&str> = params.symbols.iter().map(String::as_str).collect();
        client
            .add_to_watchlist(&params.name, &refs)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        let result = serde_json::json!({
            "watchlist": params.name,
            "requested": params.symbols,
            "requested_count": params.symbols.len(),
        });
        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())
    }

    #[tool(
        name = "remove_from_watchlist",
        description = "Remove stock symbols from a watchlist",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn remove_from_watchlist(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<WatchlistModifyParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let refs: Vec<&str> = params.symbols.iter().map(String::as_str).collect();
        let removed = client
            .remove_from_watchlist(&params.name, &refs)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        let result = serde_json::json!({
            "watchlist": params.name,
            "requested": params.symbols.len(),
            "removed": removed,
        });
        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())
    }
}
