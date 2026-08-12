use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::models::research::*;
use crate::pagination::ResultsResponse;
use crate::{Result, RhoodError};

impl RobinhoodClient {
    /// Resolves a ticker symbol to its Robinhood instrument ID.
    ///
    /// Routes through the private Instrument Identity resolver.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidSymbol`] if the symbol is not found.
    pub async fn resolve_instrument_id(&self, symbol: &str) -> Result<String> {
        self.instrument_for_symbol(symbol)
            .await?
            .and_then(|instrument| instrument.id.clone())
            .ok_or_else(|| RhoodError::InvalidSymbol(symbol.to_string()))
    }

    /// Fetches earnings data for a ticker symbol.
    ///
    /// Returns all available earnings records (historical and upcoming).
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_earnings(&self, symbol: &str) -> Result<Vec<Earnings>> {
        let uppercased = symbol.to_uppercase();
        let params = [("symbol", uppercased.as_str())];
        let resp: ResultsResponse<Earnings> = self
            .get_with_params(&self.api_url(paths::EARNINGS), &params)
            .await?;
        Ok(resp.results)
    }

    /// Fetches analyst ratings for a ticker symbol.
    ///
    /// Requires instrument ID resolution (one extra API call).
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidSymbol`] if the symbol is not found.
    /// Returns an error on HTTP or deserialization failures.
    pub async fn get_ratings(&self, symbol: &str) -> Result<Rating> {
        let instrument_id = self.resolve_instrument_id(symbol).await?;
        let url = format!("{}{instrument_id}/", self.api_url(paths::RATINGS));
        self.get(&url).await
    }

    /// Fetches recent news articles for a ticker symbol.
    ///
    /// Returns paginated results collected into a single vector.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_news(&self, symbol: &str) -> Result<Vec<NewsArticle>> {
        let uppercased = symbol.to_uppercase();
        let params = [("symbol", uppercased.as_str())];
        let mut articles: Vec<NewsArticle> = self
            .get_paginated(&self.api_url(paths::NEWS), &params)
            .await?;
        let references = articles
            .iter()
            .flat_map(|article| article.related_instruments.iter().flatten())
            .cloned()
            .collect::<Vec<_>>();
        let mut symbols = self
            .resolve_instrument_symbols_best_effort("get_news", &references)
            .await
            .into_iter();
        for article in &mut articles {
            article.related_symbols = article
                .related_instruments
                .as_ref()
                .map(|references| {
                    references
                        .iter()
                        .map(|_| symbols.next().unwrap_or(None))
                        .collect()
                })
                .unwrap_or_default();
        }
        Ok(articles)
    }

    /// Fetches stock split history for a ticker symbol.
    ///
    /// Requires instrument ID resolution (one extra API call). Returns all
    /// splits collected from paginated results.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidSymbol`] if the symbol is not found.
    /// Returns an error on HTTP or deserialization failures.
    pub async fn get_splits(&self, symbol: &str) -> Result<Vec<StockSplit>> {
        let instrument_id = self.resolve_instrument_id(symbol).await?;
        let url = format!(
            "{}{instrument_id}/splits/",
            self.api_url(paths::INSTRUMENTS)
        );
        let mut splits: Vec<StockSplit> = self.get_paginated(&url, &[]).await?;
        self.enrich_instrument_identities("get_splits", &mut splits)
            .await;
        Ok(splits)
    }

    /// Fetches instruments associated with a tag (e.g., "100-most-popular").
    ///
    /// Returns tag metadata, Instrument References, and aligned Ticker Symbols.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_tags(&self, tag: &str) -> Result<TagResult> {
        let url = format!("{}{tag}/", self.api_url(paths::TAGS));
        let mut tag: TagResult = self.get(&url).await?;
        let references = tag.instruments.clone().unwrap_or_default();
        tag.instrument_symbols = self
            .resolve_instrument_symbols_best_effort("get_tags", &references)
            .await;
        Ok(tag)
    }
}

#[cfg(test)]
mod endpoint_tests {
    use crate::client::RobinhoodClient;
    use crate::config::RhoodConfig;
    use secrecy::SecretString;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const ID_A: &str = "450dfc6d-5510-4d40-abfb-f633b7d9be3e";
    const ID_B: &str = "18226051-6bfa-4c56-bd9a-d7575f0245c1";

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
    async fn get_news_follows_next_page_in_order() {
        let server = MockServer::start().await;
        let next_url = format!("{}/midlands/news/?cursor=page-2", server.uri());
        Mock::given(method("GET"))
            .and(path("/midlands/news/"))
            .and(query_param("symbol", "AAPL"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"uuid": "news-1", "title": "First"}],
                "next": next_url,
                "previous": null
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/midlands/news/"))
            .and(query_param("cursor", "page-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"uuid": "news-2", "title": "Second"}],
                "next": null,
                "previous": null
            })))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let articles = client.get_news("aapl").await.unwrap();

        let ids: Vec<_> = articles
            .iter()
            .filter_map(|article| article.uuid.as_deref())
            .collect();
        assert_eq!(ids, ["news-1", "news-2"]);
    }

    #[tokio::test]
    async fn get_news_enriches_aligned_misses_in_one_resolution_pass() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/midlands/news/"))
            .and(query_param("symbol", "AAPL"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{
                    "uuid": "news-1",
                    "related_instruments": [
                        format!("https://api.robinhood.com/instruments/{ID_A}/"),
                        format!("https://api.robinhood.com/instruments/{ID_B}/"),
                        format!("https://api.robinhood.com/instruments/{ID_A}/")
                    ]
                }],
                "next": null,
                "previous": null
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .and(query_param("ids", format!("{ID_A},{ID_B}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": ID_A, "symbol": "AAPL"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let articles = client.get_news("aapl").await.unwrap();

        assert_eq!(
            articles[0].related_symbols,
            vec![Some("AAPL".to_string()), None, Some("AAPL".to_string())]
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn get_tags_preserves_reference_cardinality() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/midlands/tags/tag/technology/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "Technology",
                "instruments": [ID_A, "invalid-reference", ID_A]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .and(query_param("ids", ID_A))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": ID_A, "symbol": "AAPL"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let tag = client.get_tags("technology").await.unwrap();

        assert_eq!(
            tag.instrument_symbols,
            vec![Some("AAPL".to_string()), None, Some("AAPL".to_string())]
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn get_splits_enriches_once_without_replacing_wire_identity() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .and(query_param("symbol", "AAPL"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": ID_A, "symbol": "AAPL"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/instruments/{ID_A}/splits/")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [
                    {"instrument": format!("https://api.robinhood.com/instruments/{ID_A}/")},
                    {"instrument": format!("https://api.robinhood.com/instruments/{ID_B}/")}
                ],
                "next": null,
                "previous": null
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .and(query_param("ids", ID_B))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": ID_B, "symbol": "VTI"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let splits = client.get_splits("AAPL").await.unwrap();

        assert_eq!(splits[0].symbol.as_deref(), Some("AAPL"));
        assert_eq!(splits[1].symbol.as_deref(), Some("VTI"));
        server.verify().await;
    }
}

#[cfg(test)]
#[expect(
    clippy::let_underscore_must_use,
    reason = "compile-only async helpers prove public endpoint signatures without executing requests"
)]
mod tests {
    use crate::models::research::{Earnings, NewsArticle, RatingSummary, StockSplit, TagResult};

    #[test]
    fn earnings_deserializes_full() {
        let json = r#"{
            "symbol": "AAPL",
            "instrument": "https://api.robinhood.com/instruments/abc/",
            "year": 2026,
            "quarter": 1,
            "eps": {"estimate": "1.50", "actual": "1.65"},
            "report": {"date": "2026-04-25", "timing": "am", "verified": true}
        }"#;
        let e: Earnings = serde_json::from_str(json).unwrap();
        assert_eq!(e.symbol.as_deref(), Some("AAPL"));
        assert_eq!(e.year, Some(2026));
        assert_eq!(e.quarter, Some(1));
        let eps = e.eps.unwrap();
        assert_eq!(eps.estimate.as_deref(), Some("1.50"));
        assert_eq!(eps.actual.as_deref(), Some("1.65"));
        let report = e.report.unwrap();
        assert_eq!(report.timing.as_deref(), Some("am"));
    }

    #[test]
    fn earnings_handles_missing_fields() {
        let json = r#"{"symbol": "TSLA"}"#;
        let e: Earnings = serde_json::from_str(json).unwrap();
        assert_eq!(e.symbol.as_deref(), Some("TSLA"));
        assert!(e.eps.is_none());
        assert!(e.report.is_none());
    }

    #[test]
    fn rating_summary_computed_fields() {
        let summary = RatingSummary {
            num_buy_ratings: Some(10),
            num_hold_ratings: Some(5),
            num_sell_ratings: Some(2),
        };
        assert_eq!(summary.total(), 17);
        let buy_pct = summary.buy_pct();
        assert!((buy_pct - 58.82).abs() < 0.1);
    }

    #[test]
    fn rating_summary_zero_total() {
        let summary = RatingSummary {
            num_buy_ratings: None,
            num_hold_ratings: None,
            num_sell_ratings: None,
        };
        assert_eq!(summary.total(), 0);
        assert!((summary.buy_pct() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn news_article_deserializes() {
        let json = r#"{
            "uuid": "news-001",
            "title": "Apple Reports Record Quarter",
            "source": "Reuters",
            "url": "https://example.com/article",
            "summary": "Apple beat estimates",
            "published_at": "2026-04-01T10:00:00Z",
            "related_instruments": ["https://api.robinhood.com/instruments/abc/"],
            "preview_image_url": "https://example.com/img.jpg"
        }"#;
        let article: NewsArticle = serde_json::from_str(json).unwrap();
        assert_eq!(
            article.title.as_deref(),
            Some("Apple Reports Record Quarter")
        );
        assert_eq!(article.source.as_deref(), Some("Reuters"));
        assert!(article.related_instruments.is_some());
        assert!(article.related_symbols.is_empty());
    }

    #[test]
    fn stock_split_deserializes() {
        let json = r#"{
            "url": "https://api.robinhood.com/instruments/abc/splits/s1/",
            "instrument": "https://api.robinhood.com/instruments/abc/",
            "execution_date": "2022-06-06",
            "multiplier": "4.00000000",
            "divisor": "1.00000000"
        }"#;
        let split: StockSplit = serde_json::from_str(json).unwrap();
        assert_eq!(split.execution_date.as_deref(), Some("2022-06-06"));
        assert_eq!(split.multiplier.as_deref(), Some("4.00000000"));
        assert!(split.symbol.is_none());
    }

    #[test]
    fn tag_result_deserializes() {
        let json = r#"{
            "name": "Top 100 Most Popular",
            "slug": "100-most-popular",
            "instruments": [
                "https://api.robinhood.com/instruments/aaa/",
                "https://api.robinhood.com/instruments/bbb/"
            ]
        }"#;
        let tag: TagResult = serde_json::from_str(json).unwrap();
        assert_eq!(tag.name.as_deref(), Some("Top 100 Most Popular"));
        assert_eq!(tag.slug.as_deref(), Some("100-most-popular"));
        let instruments = tag.instruments.unwrap();
        assert_eq!(instruments.len(), 2);
        assert!(tag.instrument_symbols.is_empty());
    }

    #[test]
    fn derived_identity_fields_ignore_upstream_values() {
        let article: NewsArticle =
            serde_json::from_str(r#"{"related_symbols":["UNTRUSTED"],"related_instruments":[]}"#)
                .unwrap();
        let split: StockSplit = serde_json::from_str(r#"{"symbol":"UNTRUSTED"}"#).unwrap();
        let tag: TagResult =
            serde_json::from_str(r#"{"instrument_symbols":["UNTRUSTED"],"instruments":[]}"#)
                .unwrap();

        assert!(article.related_symbols.is_empty());
        assert!(split.symbol.is_none());
        assert!(tag.instrument_symbols.is_empty());
    }

    #[test]
    fn resolve_instrument_id_signature_exists() {
        async fn _assert(client: &crate::RobinhoodClient) {
            let _ = client.resolve_instrument_id("AAPL").await;
        }
    }

    #[test]
    fn research_endpoint_signatures_exist() {
        async fn _assert(client: &crate::RobinhoodClient) {
            let _ = client.get_earnings("AAPL").await;
            let _ = client.get_ratings("AAPL").await;
            let _ = client.get_news("AAPL").await;
            let _ = client.get_splits("AAPL").await;
            let _ = client.get_tags("100-most-popular").await;
        }
    }
}
