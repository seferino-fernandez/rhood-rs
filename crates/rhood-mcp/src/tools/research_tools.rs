use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};

use crate::tools::enrichment::{
    EnrichedNewsArticle, EnrichedSplit, collect_uuids, extract_uuid_from_url,
    resolve_urls_to_symbols, safe_resolve_symbols,
};

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

        let all_urls: Vec<Option<String>> = articles
            .iter()
            .flat_map(|article| {
                article
                    .related_instruments
                    .iter()
                    .flatten()
                    .map(|url| Some(url.clone()))
            })
            .collect();
        let uuids = collect_uuids(all_urls.iter());
        let resolved = safe_resolve_symbols(&client, &uuids).await;

        let enriched: Vec<EnrichedNewsArticle> = articles
            .iter()
            .map(|article| {
                let related_symbols = article
                    .related_instruments
                    .as_ref()
                    .map(|urls| {
                        urls.iter()
                            .filter_map(|url| {
                                extract_uuid_from_url(url)
                                    .and_then(|uuid| resolved.get(&uuid).cloned())
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                EnrichedNewsArticle {
                    article,
                    related_symbols,
                }
            })
            .collect();

        serde_json::to_string_pretty(&enriched).map_err(|error| error.to_string())
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
        let urls: Vec<Option<String>> = splits
            .iter()
            .map(|stock_split| stock_split.instrument.clone())
            .collect();
        let symbols = resolve_urls_to_symbols(&client, &urls).await;
        let enriched: Vec<EnrichedSplit> = splits
            .iter()
            .zip(symbols.iter())
            .map(|(split, symbol)| EnrichedSplit {
                split,
                symbol: symbol.clone(),
            })
            .collect();
        serde_json::to_string_pretty(&enriched).map_err(|error| error.to_string())
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

        let instrument_urls: Vec<Option<String>> = tag
            .instruments
            .iter()
            .flatten()
            .map(|url| Some(url.clone()))
            .collect();
        let resolved = super::enrichment::resolve_urls_to_symbols(&client, &instrument_urls).await;
        let instrument_symbols: Vec<String> = resolved.into_iter().flatten().collect();

        let enriched = super::enrichment::EnrichedTagResult {
            tag: &tag,
            instrument_symbols,
        };
        serde_json::to_string_pretty(&enriched).map_err(|error| error.to_string())
    }
}
