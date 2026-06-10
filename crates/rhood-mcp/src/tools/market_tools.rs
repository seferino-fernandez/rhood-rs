use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};

use super::handler::RhoodTools;
use super::params::*;
use super::types::format_tool_error;
use rhood_core::models::watchlist::WatchlistItem;

/// Decision-relevant projection of a daily-mover row. Drops list-membership
/// metadata, tradability/IPO flags, timestamps, and the redundant rolling
/// change pair to cut the token footprint of the 20-item list.
#[derive(serde::Serialize)]
struct DailyMoverView {
    symbol: Option<String>,
    name: Option<String>,
    price: Option<f64>,
    previous_close: Option<f64>,
    one_day_dollar_change: Option<f64>,
    one_day_percent_change: Option<f64>,
    volume: Option<f64>,
    market_cap: Option<f64>,
}

impl From<&WatchlistItem> for DailyMoverView {
    fn from(item: &WatchlistItem) -> Self {
        Self {
            symbol: item.symbol.clone(),
            name: item.name.clone(),
            price: item.price,
            previous_close: item.previous_close,
            one_day_dollar_change: item.one_day_dollar_change,
            one_day_percent_change: item.one_day_percent_change,
            volume: item.volume,
            market_cap: item.market_cap,
        }
    }
}

#[tool_router(router = market_router, vis = "pub(super)")]
impl RhoodTools {
    #[tool(
        name = "get_markets",
        description = "List all available markets (exchanges) such as NYSE, NASDAQ",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_markets(&self, peer: Peer<RoleServer>) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let data = client
            .get_markets()
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&data).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_market_hours",
        description = "Get market hours for a specific exchange and date",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_market_hours(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<MarketHoursParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let data = client
            .get_market_hours(&params.mic, &params.date)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&data).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_market_today_hours",
        description = "Get today's market hours for a specific exchange",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_market_today_hours(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<MarketTodayHoursParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let data = client
            .get_market_today_hours(&params.mic)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&data).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_daily_movers",
        description = "Get the top 20 daily movers — a single combined list of the biggest gainers and losers today. There is no direction filter; the result mixes up and down movers.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_daily_movers(&self, peer: Peer<RoleServer>) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let data = client
            .get_daily_movers()
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        let views: Vec<DailyMoverView> = data.iter().map(DailyMoverView::from).collect();
        serde_json::to_string_pretty(&views).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::DailyMoverView;
    use rhood_core::models::watchlist::WatchlistItem;

    #[test]
    fn mover_view_keeps_core_fields_and_drops_metadata() {
        let item: WatchlistItem = serde_json::from_str(
            r#"{"symbol":"REPL","name":"Replimune","price":12.5,"previous_close":6.6,
                "one_day_percent_change":89.0,"list_id":"abc","object_id":"uuid",
                "created_at":"2020-01-01","us_tradability":"tradable"}"#,
        )
        .unwrap();
        let view = DailyMoverView::from(&item);
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("list_id"), "metadata leaked: {json}");
        assert!(!json.contains("object_id"), "metadata leaked: {json}");
        assert!(!json.contains("us_tradability"), "metadata leaked: {json}");
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["symbol"], "REPL");
        assert_eq!(value["one_day_percent_change"], 89.0);
    }
}
