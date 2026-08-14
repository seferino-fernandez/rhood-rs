use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::models::market::*;
use crate::models::watchlist::WatchlistItem;
use crate::pagination::ResultsResponse;
use crate::{Result, RhoodError};

impl RobinhoodClient {
    /// Fetches a list of all available markets (e.g., NYSE, NASDAQ).
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_markets(&self) -> Result<Vec<Market>> {
        self.get_paginated(&self.api_url(paths::MARKETS), &[]).await
    }

    /// Fetches market hours for a specific market and date.
    ///
    /// The `mic` parameter is a Market Identifier Code (e.g., `"XNYS"`) and
    /// `date` is in `YYYY-MM-DD` format.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_market_hours(&self, mic: &str, date: &str) -> Result<MarketHours> {
        let url = format!("{}{mic}/hours/{date}/", self.api_url(paths::MARKETS));
        self.get(&url).await
    }

    /// Fetches today's market hours for a market identified by its MIC code.
    ///
    /// Resolves the market from the full market list and follows its
    /// `todays_hours` URL.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidParameter`] if the MIC code is unknown
    /// or the market has no today's-hours URL. Also returns an error on HTTP
    /// or deserialization failures.
    pub async fn get_market_today_hours(&self, mic: &str) -> Result<MarketHours> {
        let markets = self.get_markets().await?;
        let market = markets
            .iter()
            .find(|market| market.mic.as_deref() == Some(mic))
            .ok_or_else(|| RhoodError::InvalidParameter(format!("Unknown market: {mic}")))?;
        let hours_url = market
            .todays_hours
            .as_deref()
            .ok_or_else(|| RhoodError::InvalidParameter("No today's hours URL".into()))?;
        self.get(hours_url).await
    }

    /// Fetches the top 20 daily movers from Robinhood's curated list.
    ///
    /// Uses the `/discovery/lists/items/` endpoint with Robinhood's daily
    /// movers list ID. This route returns one non-paginated list of enriched
    /// items with live price and change data.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_daily_movers(&self) -> Result<Vec<WatchlistItem>> {
        let resp: ResultsResponse<WatchlistItem> = self
            .get_with_params(
                &self.api_url(paths::WATCHLIST_ITEMS),
                &[("list_id", paths::DAILY_MOVERS_LIST_ID)],
            )
            .await?;
        Ok(resp.results)
    }
}

#[cfg(test)]
mod endpoint_tests {
    use crate::client::RobinhoodClient;
    use crate::config::RhoodConfig;
    use crate::models::watchlist::WatchlistItem;
    use secrecy::SecretString;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn client_for_server(base_url: &str) -> (tempfile::TempDir, RobinhoodClient) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = RhoodConfig::default();
        config.auth.token_cache_path = dir
            .path()
            .join("nonexistent-token.json")
            .to_str()
            .unwrap()
            .to_string();
        config.api.base_url = base_url.to_string();
        let client = RobinhoodClient::with_config(config).unwrap();
        client
            .inject_test_auth(
                SecretString::from("access-token"),
                "Bearer".to_string(),
                SecretString::from("refresh-token"),
            )
            .await;
        (dir, client)
    }

    #[tokio::test]
    async fn get_daily_movers_deserializes_non_paginated_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/discovery/lists/items/"))
            .and(query_param(
                "list_id",
                "eddbebe5-34cc-4df1-953c-d3e3cb55bc19",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "has_futures_contracts": false,
                "results": [
                    {"id": "mover-1", "symbol": "AAPL"},
                    {"id": "mover-2", "symbol": "MSFT"}
                ]
            })))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let movers = client.get_daily_movers().await.unwrap();

        let ids: Vec<_> = movers
            .iter()
            .filter_map(|item: &WatchlistItem| item.id.as_deref())
            .collect();
        assert_eq!(ids, ["mover-1", "mover-2"]);
        let symbols: Vec<_> = movers
            .iter()
            .filter_map(|item| item.symbol.as_deref())
            .collect();
        assert_eq!(symbols, ["AAPL", "MSFT"]);
        server.verify().await;
    }
}

#[cfg(test)]
mod tests {
    use crate::models::market::{Market, MarketHours};

    #[test]
    fn market_deserializes() {
        let json = r#"{
            "mic": "XNYS",
            "acronym": "NYSE",
            "name": "New York Stock Exchange",
            "city": "New York",
            "country": "US",
            "timezone": "US/Eastern",
            "todays_hours": "https://api.robinhood.com/markets/XNYS/hours/2026-03-31/"
        }"#;
        let market: Market = serde_json::from_str(json).unwrap();
        assert_eq!(market.mic.as_deref(), Some("XNYS"));
        assert_eq!(market.acronym.as_deref(), Some("NYSE"));
        assert!(market.todays_hours.is_some());
    }

    #[test]
    fn market_hours_deserializes() {
        let json = r#"{
            "date": "2026-03-31",
            "is_open": true,
            "opens_at": "2026-03-31T13:30:00Z",
            "closes_at": "2026-03-31T20:00:00Z",
            "extended_opens_at": "2026-03-31T09:00:00Z",
            "extended_closes_at": "2026-03-31T22:00:00Z"
        }"#;
        let hours: MarketHours = serde_json::from_str(json).unwrap();
        assert_eq!(hours.date.as_deref(), Some("2026-03-31"));
        assert_eq!(hours.is_open, Some(true));
        assert!(hours.opens_at.is_some());
        assert!(hours.extended_opens_at.is_some());
    }

    #[test]
    fn market_hours_closed_day() {
        let json = r#"{ "date": "2026-04-05", "is_open": false }"#;
        let hours: MarketHours = serde_json::from_str(json).unwrap();
        assert_eq!(hours.is_open, Some(false));
        assert!(hours.opens_at.is_none());
    }
}
