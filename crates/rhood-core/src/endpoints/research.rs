use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::models::research::*;
use crate::pagination::{PaginatedResponse, ResultsResponse};
use crate::{Result, RhoodError};

impl RobinhoodClient {
    /// Resolves a ticker symbol to its Robinhood instrument ID.
    ///
    /// Routes through the resolver cache via
    /// [`cached_instrument`](Self::cached_instrument) so repeated resolutions
    /// for the same symbol within the TTL are served from memory.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidSymbol`] if the symbol is not found.
    pub async fn resolve_instrument_id(&self, symbol: &str) -> Result<String> {
        self.cached_instrument(symbol)
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
        let resp: PaginatedResponse<NewsArticle> = self
            .get_with_params(&self.api_url(paths::NEWS), &params)
            .await?;
        Ok(resp.results)
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
        self.get_paginated(&url, &[]).await
    }

    /// Fetches instruments associated with a tag (e.g., "100-most-popular").
    ///
    /// Returns the tag metadata and a list of instrument URLs. Use
    /// [`get_instrument_by_symbol`](Self::get_instrument_by_symbol) to resolve
    /// individual URLs to symbols if needed.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_tags(&self, tag: &str) -> Result<TagResult> {
        let url = format!("{}{tag}/", self.api_url(paths::TAGS));
        self.get(&url).await
    }
}

#[cfg(test)]
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
