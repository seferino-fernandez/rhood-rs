use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};

use super::handler::RhoodTools;
use super::params::IpoAccessCardsParams;
use super::types::format_tool_error;

#[tool_router(router = ipo_router, vis = "pub(super)")]
impl RhoodTools {
    #[tool(
        name = "get_ipo_access_list",
        description = "Get the authenticated user's raw IPO Access offering list view model. Returns an empty_state when no IPO is currently available and may expose instrument IDs for get_ipo_access_cards.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_ipo_access_list(&self, peer: Peer<RoleServer>) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let list = client
            .get_ipo_access_list()
            .await
            .map_err(|error| format_tool_error(&error))?;
        serde_json::to_string_pretty(&list).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_ipo_access_cards",
        description = "Get raw IPO Access offering cards for 1 to 50 Robinhood instrument UUIDs. Use instrument IDs from get_ipo_access_list or another instrument lookup.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_ipo_access_cards(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<IpoAccessCardsParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let id_refs: Vec<&str> = params.instrument_ids.iter().map(String::as_str).collect();
        let cards = client
            .get_ipo_access_cards(&id_refs)
            .await
            .map_err(|error| format_tool_error(&error))?;
        serde_json::to_string_pretty(&cards).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::RhoodTools;
    use serde_json::Value;

    #[test]
    fn ipo_access_tools_are_registered_read_only_with_bounded_cards_input() {
        let tools = RhoodTools::ipo_router().list_all();
        let list = tools
            .iter()
            .find(|tool| tool.name == "get_ipo_access_list")
            .unwrap();
        let cards = tools
            .iter()
            .find(|tool| tool.name == "get_ipo_access_cards")
            .unwrap();

        assert_eq!(
            list.input_schema.get("properties"),
            Some(&serde_json::json!({}))
        );
        assert!(
            list.annotations
                .as_ref()
                .is_some_and(|annotations| annotations.read_only_hint == Some(true))
        );
        assert!(
            cards
                .annotations
                .as_ref()
                .is_some_and(|annotations| annotations.read_only_hint == Some(true))
        );
        let schema = Value::Object((*cards.input_schema).clone());
        assert_eq!(schema["properties"]["instrument_ids"]["minItems"], 1);
        assert_eq!(schema["properties"]["instrument_ids"]["maxItems"], 50);
        assert_eq!(schema["required"], serde_json::json!(["instrument_ids"]));
    }
}
