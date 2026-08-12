use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};

use super::handler::RhoodTools;
use super::params::*;
use super::types::format_tool_error;

#[tool_router(router = research_router, vis = "pub(super)")]
impl RhoodTools {
    #[tool(
        name = "get_earnings",
        description = "Get earnings data (EPS estimates, actuals, report dates) for a stock symbol",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_earnings(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<EarningsParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let earnings = client
            .get_earnings(&params.symbol)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&earnings).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_ratings",
        description = "Get analyst ratings (buy/hold/sell counts and percentages) for a stock symbol",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_ratings(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<RatingsParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let rating = client
            .get_ratings(&params.symbol)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&rating).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_news",
        description = "Get recent news articles for a stock symbol",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_news(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<NewsParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let articles = client
            .get_news(&params.symbol)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;

        serde_json::to_string_pretty(&articles).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_splits",
        description = "Get stock split history for a symbol",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_splits(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<SplitsParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let splits = client
            .get_splits(&params.symbol)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&splits).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_tags",
        description = "Get instruments associated with a tag (e.g. '100-most-popular', 'technology')",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_tags(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<TagsParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let tag = client
            .get_tags(&params.tag)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;

        serde_json::to_string_pretty(&tag).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use rhood_core::models::research::{NewsArticle, StockSplit, TagResult};

    #[test]
    fn research_tools_serialize_core_identity_fields_without_duplicates() {
        let mut article: NewsArticle = serde_json::from_str(
            r#"{"related_instruments":["instrument-ref"],"related_symbols":["WIRE"]}"#,
        )
        .unwrap();
        article.related_symbols = vec![Some("AAPL".to_string())];
        let mut split: StockSplit =
            serde_json::from_str(r#"{"instrument":"instrument-ref","symbol":"WIRE"}"#).unwrap();
        split.symbol = Some("AAPL".to_string());
        let mut tag: TagResult = serde_json::from_str(
            r#"{"instruments":["instrument-ref"],"instrument_symbols":["WIRE"]}"#,
        )
        .unwrap();
        tag.instrument_symbols = vec![Some("AAPL".to_string())];

        for (value, field) in [
            (serde_json::to_string(&article).unwrap(), "related_symbols"),
            (serde_json::to_string(&split).unwrap(), "symbol"),
            (serde_json::to_string(&tag).unwrap(), "instrument_symbols"),
        ] {
            assert_eq!(value.matches(&format!("\"{field}\"")).count(), 1);
        }
    }
}
