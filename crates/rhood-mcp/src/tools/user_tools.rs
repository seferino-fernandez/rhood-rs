use rmcp::{Peer, RoleServer, tool, tool_router};

use super::handler::RhoodTools;
use super::types::format_tool_error;

#[tool_router(router = user_router, vis = "pub(super)")]
impl RhoodTools {
    #[tool(
        name = "get_user_profile",
        description = "Get the authenticated user's profile (name, email, creation date)",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_user_profile(&self, peer: Peer<RoleServer>) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let user = client
            .get_user_profile()
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&user).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_day_trades",
        description = "Get recent day trades and pattern day trader status",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_day_trades(&self, peer: Peer<RoleServer>) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let check = client
            .get_day_trades()
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&check).map_err(|error| error.to_string())
    }
}
