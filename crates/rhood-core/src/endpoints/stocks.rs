use std::collections::HashMap;
use std::sync::Arc;

use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::endpoints::request_batch::{
    ChunkLimit, PlannedInput, chunk_failed, plan_request_chunks,
};
use crate::models::stock::*;
use crate::pagination::ResultsResponse;
use crate::{Result, RhoodError};

/// Batch `/instruments/?ids=` response, tolerant of `null` array entries.
///
/// Robinhood returns a literal `null` in `results` for delisted or otherwise
/// unresolvable ids in the requested batch. Deserializing those positions into
/// `Vec<Instrument>` would fail the entire batch (and, via the best-effort
/// enrichment layer, zero out symbols for *every* item in the request). Using
/// `Vec<Option<Instrument>>` keeps the resolvable instruments and drops the
/// nulls.
#[derive(serde::Deserialize)]
struct InstrumentBatchResponse {
    results: Vec<Option<Instrument>>,
}

const MARKET_DATA_URL_LIMIT: usize = 4_096;
const FUNDAMENTALS_ITEM_LIMIT: usize = 100;

fn response_symbol(
    endpoint: &'static str,
    symbol: Option<&str>,
    chunk: &[PlannedInput],
) -> Result<String> {
    let symbol = symbol
        .map(str::trim)
        .filter(|symbol| !symbol.is_empty())
        .map(str::to_uppercase)
        .ok_or_else(|| RhoodError::InvalidMarketDataResponse {
            endpoint: endpoint.to_string(),
            message: "response item omitted its requested symbol".into(),
        })?;
    if chunk.iter().any(|input| input.value == symbol) {
        Ok(symbol)
    } else {
        Err(RhoodError::InvalidMarketDataResponse {
            endpoint: endpoint.to_string(),
            message: "response item named a symbol outside its request chunk".into(),
        })
    }
}

fn join_chunk(chunk: &[PlannedInput]) -> String {
    chunk
        .iter()
        .map(|input| input.value.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

impl RobinhoodClient {
    /// Fetches real-time stock quotes for one or more ticker symbols.
    ///
    /// Symbols are uppercased and stable-deduplicated on the wire. Returned
    /// quotes follow caller order and repeat for duplicate inputs; requested
    /// symbols missing from the response are omitted. Requests are split before
    /// their fully encoded URL exceeds 4,096 bytes.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidParameter`] before HTTP if one symbol cannot
    /// fit the URL limit. If a request chunk fails, returns
    /// [`RhoodError::RequestChunkFailed`] with its one-based position and no
    /// partial results.
    pub async fn get_quotes(&self, symbols: &[&str]) -> Result<Vec<StockQuote>> {
        let requested = symbols
            .iter()
            .map(|symbol| symbol.to_uppercase())
            .collect::<Vec<_>>();
        let url = self.api_url(paths::QUOTES);
        let chunks = plan_request_chunks(
            "quotes",
            &requested,
            ChunkLimit::EncodedUrlBytes {
                maximum: MARKET_DATA_URL_LIMIT,
                url: &url,
                parameter: "symbols",
                fixed_parameters: &[],
            },
        )?;
        let total_chunks = chunks.len();
        let mut by_symbol = HashMap::with_capacity(requested.len());

        for (index, chunk) in chunks.iter().enumerate() {
            let joined = join_chunk(chunk);
            let params = [("symbols", joined.as_str())];
            let resp: ResultsResponse<StockQuote> = self
                .get_with_params(&url, &params)
                .await
                .map_err(|source| {
                    chunk_failed("quotes", index.saturating_add(1), total_chunks, source)
                })?;
            for quote in resp.results {
                if let Some(symbol) = quote.symbol.as_ref() {
                    by_symbol.entry(symbol.to_uppercase()).or_insert(quote);
                }
            }
        }

        Ok(requested
            .iter()
            .filter_map(|symbol| by_symbol.get(symbol).cloned())
            .collect())
    }

    /// Returns the latest trade price for each requested symbol.
    ///
    /// Prefers the extended-hours trade price when available; otherwise falls
    /// back to the last regular-session trade price. Each entry in the
    /// returned vector is a `(symbol, price)` tuple.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying quote request fails.
    pub async fn get_latest_prices(&self, symbols: &[&str]) -> Result<Vec<(String, String)>> {
        let quotes = self.get_quotes(symbols).await?;
        Ok(quotes
            .into_iter()
            .filter_map(|quote| {
                let symbol = quote.symbol?;
                let price = quote
                    .last_extended_hours_trade_price
                    .or(quote.last_trade_price)?;
                Some((symbol, price))
            })
            .collect())
    }

    /// Fetches fundamental data (market cap, P/E ratio, dividend yield, etc.)
    /// for one or more ticker symbols.
    ///
    /// Symbols are uppercased and stable-deduplicated into sequential requests
    /// of at most 100 symbols. Results are reconstructed in caller order and
    /// repeated for duplicate inputs when the API returns the corresponding
    /// fundamental record.
    ///
    /// # Errors
    ///
    /// If a request chunk fails, returns [`RhoodError::RequestChunkFailed`]
    /// with its one-based position and no partial results.
    pub async fn get_fundamentals(&self, symbols: &[&str]) -> Result<Vec<Fundamentals>> {
        let requested = symbols
            .iter()
            .map(|symbol| symbol.to_uppercase())
            .collect::<Vec<_>>();
        let chunks = plan_request_chunks(
            "fundamentals",
            &requested,
            ChunkLimit::ItemCount(FUNDAMENTALS_ITEM_LIMIT),
        )?;
        let total_chunks = chunks.len();
        let url = self.api_url(paths::FUNDAMENTALS);
        let mut by_symbol = HashMap::with_capacity(requested.len());

        for (index, chunk) in chunks.iter().enumerate() {
            let joined = join_chunk(chunk);
            let params = [("symbols", joined.as_str())];
            let resp: ResultsResponse<Fundamentals> = self
                .get_with_params(&url, &params)
                .await
                .map_err(|source| {
                    chunk_failed(
                        "fundamentals",
                        index.saturating_add(1),
                        total_chunks,
                        source,
                    )
                })?;
            for fundamental in resp.results {
                let symbol = response_symbol("fundamentals", fundamental.symbol.as_deref(), chunk)
                    .map_err(|source| {
                        chunk_failed(
                            "fundamentals",
                            index.saturating_add(1),
                            total_chunks,
                            source,
                        )
                    })?;
                by_symbol.entry(symbol).or_insert(fundamental);
            }
        }

        Ok(requested
            .iter()
            .filter_map(|symbol| by_symbol.get(symbol).cloned())
            .collect())
    }

    /// Fetches historical price data (OHLCV candles) for one or more symbols.
    ///
    /// The `opts` parameter controls the candle interval, time span, and
    /// session bounds. Extended and trading bounds are only valid with a
    /// day span; other combinations return an error. Symbols are uppercased and
    /// stable-deduplicated on the wire, and requests are split before the fully
    /// encoded URL (including interval, span, and bounds) exceeds 4,096 bytes.
    /// Candle groups retain caller symbol order and repeat for duplicate inputs.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidParameter`] if extended or trading bounds
    /// are used with a non-day span, or if one symbol cannot fit the URL limit.
    /// A failed request chunk returns [`RhoodError::RequestChunkFailed`] with
    /// its one-based position and no partial results.
    pub async fn get_stock_historicals(
        &self,
        symbols: &[&str],
        opts: &HistoricalOpts,
    ) -> Result<Vec<Candle>> {
        if symbols.is_empty() {
            return Ok(Vec::new());
        }
        if matches!(
            opts.bounds,
            HistoricalBounds::Extended | HistoricalBounds::Trading
        ) && !matches!(opts.span, HistoricalSpan::Day)
        {
            return Err(RhoodError::InvalidParameter(
                "Extended/trading bounds can only be used with day span".into(),
            ));
        }

        let requested = symbols
            .iter()
            .map(|symbol| symbol.to_uppercase())
            .collect::<Vec<_>>();
        let url = self.api_url(paths::HISTORICALS);
        let fixed_parameters = [
            ("interval", opts.interval.as_str()),
            ("span", opts.span.as_str()),
            ("bounds", opts.bounds.as_str()),
        ];
        let chunks = plan_request_chunks(
            "stock historicals",
            &requested,
            ChunkLimit::EncodedUrlBytes {
                maximum: MARKET_DATA_URL_LIMIT,
                url: &url,
                parameter: "symbols",
                fixed_parameters: &fixed_parameters,
            },
        )?;
        let total_chunks = chunks.len();
        let mut by_symbol: HashMap<String, Vec<Candle>> = HashMap::with_capacity(requested.len());

        for (index, chunk) in chunks.iter().enumerate() {
            let joined = join_chunk(chunk);
            let params = [
                ("symbols", joined.as_str()),
                ("interval", opts.interval.as_str()),
                ("span", opts.span.as_str()),
                ("bounds", opts.bounds.as_str()),
            ];
            let resp: ResultsResponse<HistoricalsResult> = self
                .get_with_params(&url, &params)
                .await
                .map_err(|source| {
                    chunk_failed(
                        "stock historicals",
                        index.saturating_add(1),
                        total_chunks,
                        source,
                    )
                })?;

            for result in resp.results {
                let symbol = response_symbol("stock historicals", result.symbol.as_deref(), chunk)
                    .map_err(|source| {
                        chunk_failed(
                            "stock historicals",
                            index.saturating_add(1),
                            total_chunks,
                            source,
                        )
                    })?;
                let candles = by_symbol.entry(symbol.clone()).or_default();
                candles.extend(result.historicals.into_iter().map(|mut candle| {
                    candle.symbol = Some(symbol.clone());
                    candle
                }));
            }
        }

        Ok(requested
            .iter()
            .filter_map(|symbol| by_symbol.get(symbol))
            .flat_map(|candles| candles.iter().cloned())
            .collect())
    }

    /// Looks up a Robinhood instrument by its ticker symbol.
    ///
    /// Returns `Ok(None)` when the symbol does not match any known instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_instrument_by_symbol(&self, symbol: &str) -> Result<Option<Instrument>> {
        let uppercased_symbol = symbol.to_uppercase();
        let params = [("symbol", uppercased_symbol.as_str())];
        let resp: ResultsResponse<Instrument> = self
            .get_with_params(&self.api_url(paths::INSTRUMENTS), &params)
            .await?;
        Ok(resp.results.into_iter().next())
    }

    /// Cached wrapper around [`get_instrument_by_symbol`](Self::get_instrument_by_symbol).
    ///
    /// Returns an [`Arc<Instrument>`] shared with other callers requesting the
    /// same symbol during the TTL configured on the resolver cache. When
    /// caching is disabled (`CacheConfig::enabled = false`), every call hits
    /// upstream and nothing is inserted into the cache.
    ///
    /// Errors are not cached: a failed lookup for one caller does not taint
    /// concurrent callers requesting the same symbol. Successful hits also
    /// populate the reverse `uuid → symbol` map used by
    /// [`resolve_symbols`](Self::resolve_symbols).
    pub async fn cached_instrument(&self, symbol: &str) -> Result<Option<Arc<Instrument>>> {
        let key = symbol.to_uppercase();
        if !self.resolvers.enabled {
            return Ok(self.get_instrument_by_symbol(&key).await?.map(Arc::new));
        }
        if let Some(hit) = self.resolvers.instruments_by_symbol.get(&key).await {
            return Ok(Some(hit));
        }
        let Some(instrument) = self.get_instrument_by_symbol(&key).await? else {
            return Ok(None);
        };
        let wrapped = Arc::new(instrument);
        self.resolvers
            .instruments_by_symbol
            .insert(key.clone(), wrapped.clone())
            .await;
        if let Some(id) = wrapped.id.as_ref() {
            self.resolvers
                .instruments_by_id
                .insert(id.clone(), key)
                .await;
        }
        Ok(Some(wrapped))
    }

    /// Looks up a Robinhood index instrument by its symbol (e.g., "SPX").
    ///
    /// Returns `Ok(None)` when the symbol does not match any known index.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_index_instrument(&self, symbol: &str) -> Result<Option<IndexInstrument>> {
        let uppercased = symbol.to_uppercase();
        let params = [("symbol", uppercased.as_str())];
        let resp: ResultsResponse<IndexInstrument> = self
            .get_with_params(&self.api_url(paths::INDEXES), &params)
            .await?;
        Ok(resp.results.into_iter().next())
    }

    /// Cached wrapper around [`get_index_instrument`](Self::get_index_instrument).
    ///
    /// Returns an [`Arc<IndexInstrument>`] shared with other callers during
    /// the configured TTL. Caching is skipped entirely when
    /// `CacheConfig::enabled = false`.
    pub async fn cached_index_instrument(
        &self,
        symbol: &str,
    ) -> Result<Option<Arc<IndexInstrument>>> {
        let key = symbol.to_uppercase();
        if !self.resolvers.enabled {
            return Ok(self.get_index_instrument(&key).await?.map(Arc::new));
        }
        if let Some(hit) = self.resolvers.index_instruments.get(&key).await {
            return Ok(Some(hit));
        }
        let Some(index) = self.get_index_instrument(&key).await? else {
            return Ok(None);
        };
        let wrapped = Arc::new(index);
        self.resolvers
            .index_instruments
            .insert(key, wrapped.clone())
            .await;
        Ok(Some(wrapped))
    }

    /// Resolves a batch of instrument UUIDs to ticker symbols.
    ///
    /// Consults the resolver cache's `uuid → symbol` map first; only uncached
    /// UUIDs are sent upstream. Misses are chunked into batches of
    /// [`CacheConfig::enrichment_batch_size`](crate::config::CacheConfig::enrichment_batch_size)
    /// to stay under Robinhood's query-string limits, and each chunk is sent
    /// as a single `?ids=uuid1,uuid2,...` request to `/instruments/`. Results
    /// are written back to the cache when enabled.
    ///
    /// The returned map contains only UUIDs that resolve to an instrument with
    /// both an id and a symbol; any UUID whose instrument payload lacks either
    /// field is omitted from the map rather than producing an error.
    ///
    /// # Errors
    ///
    /// Returns an error if any chunked upstream request fails.
    pub async fn resolve_symbols(&self, ids: &[String]) -> Result<HashMap<String, String>> {
        let mut result: HashMap<String, String> = HashMap::with_capacity(ids.len());
        let mut misses: Vec<&str> = Vec::new();
        if self.resolvers.enabled {
            for id in ids {
                if let Some(symbol) = self.resolvers.instruments_by_id.get(id).await {
                    result.insert(id.clone(), symbol);
                } else {
                    misses.push(id.as_str());
                }
            }
        } else {
            misses = ids.iter().map(String::as_str).collect();
        }
        if misses.is_empty() {
            return Ok(result);
        }
        let batch_size = if self.resolvers.enabled {
            self.resolvers.enrichment_batch_size.max(1)
        } else {
            50
        };
        for chunk in misses.chunks(batch_size) {
            let joined = chunk.join(",");
            let params = [("ids", joined.as_str())];
            let resp: InstrumentBatchResponse = self
                .get_with_params(&self.api_url(paths::INSTRUMENTS), &params)
                .await?;
            for instrument in resp.results.into_iter().flatten() {
                if let (Some(id), Some(symbol)) = (instrument.id, instrument.symbol) {
                    if self.resolvers.enabled {
                        self.resolvers
                            .instruments_by_id
                            .insert(id.clone(), symbol.clone())
                            .await;
                    }
                    result.insert(id, symbol);
                }
            }
        }
        Ok(result)
    }

    /// Fetches real-time market data for an index symbol.
    ///
    /// Resolves the symbol to its index ID, then queries the index-specific
    /// market data endpoint.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidSymbol`] if the index is not found.
    /// Also returns an error on HTTP or deserialization failures.
    pub async fn get_index_quote(&self, symbol: &str) -> Result<IndexQuote> {
        let index = self
            .cached_index_instrument(symbol)
            .await?
            .ok_or_else(|| RhoodError::InvalidSymbol(symbol.to_string()))?;
        let id = index
            .id
            .clone()
            .ok_or_else(|| RhoodError::InvalidSymbol(symbol.to_string()))?;
        let url = format!("{}{id}/", self.api_url(paths::INDEX_MARKET_DATA));
        let wrapper: IndexQuoteWrapper = self.get(&url).await?;
        let mut quote = wrapper.data.data;
        // Backfill the symbol from the instrument if the API omits it
        if quote.symbol.is_none() {
            quote.symbol = index.symbol.clone();
        }
        Ok(quote)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RhoodConfig;
    use crate::models::stock::{Fundamentals, Instrument, StockQuote};
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

    fn historical_opts() -> HistoricalOpts {
        HistoricalOpts {
            interval: HistoricalInterval::FiveMinute,
            span: HistoricalSpan::Day,
            bounds: HistoricalBounds::Regular,
        }
    }

    #[tokio::test]
    async fn quotes_preserve_caller_order_duplicates_and_omit_missing_symbols() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/quotes/"))
            .and(query_param("symbols", "MSFT,AAPL,MISSING"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [
                    {"symbol": "AAPL", "last_trade_price": "1"},
                    {"symbol": "MSFT", "last_trade_price": "2"},
                    {"symbol": null, "last_trade_price": "3"}
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let quotes = client
            .get_quotes(&["msft", "AAPL", "msft", "missing"])
            .await
            .unwrap();

        assert_eq!(
            quotes
                .iter()
                .filter_map(|quote| quote.symbol.as_deref())
                .collect::<Vec<_>>(),
            ["MSFT", "AAPL", "MSFT"]
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn quotes_split_before_the_fully_encoded_url_exceeds_the_limit() {
        let server = MockServer::start().await;
        let first = format!("A{}", "A".repeat(2_399));
        let second = format!("B{}", "B".repeat(2_399));
        for symbol in [&first, &second] {
            Mock::given(method("GET"))
                .and(path("/quotes/"))
                .and(query_param("symbols", symbol.as_str()))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "results": [{"symbol": symbol}]
                })))
                .expect(1)
                .mount(&server)
                .await;
        }
        let (_dir, client) = client_for_server(&server.uri()).await;

        let quotes = client
            .get_quotes(&[first.as_str(), second.as_str()])
            .await
            .unwrap();

        assert_eq!(quotes.len(), 2);
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            requests
                .iter()
                .all(|request| request.url.as_str().len() <= 4_096)
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn fundamentals_send_one_hundred_plus_one_and_restore_order() {
        let server = MockServer::start().await;
        let unique = (0..101)
            .map(|index| format!("S{index:03}"))
            .collect::<Vec<_>>();
        let first_joined = unique[..100].join(",");
        let second_joined = unique[100..].join(",");
        let first_results = unique[..100]
            .iter()
            .rev()
            .map(|symbol| serde_json::json!({"symbol": symbol}))
            .collect::<Vec<_>>();
        Mock::given(method("GET"))
            .and(path("/fundamentals/"))
            .and(query_param("symbols", first_joined))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"results": first_results})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/fundamentals/"))
            .and(query_param("symbols", second_joined))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"symbol": unique[100]}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;
        let mut caller = unique.iter().map(String::as_str).collect::<Vec<_>>();
        caller.push(unique[0].as_str());

        let fundamentals = client.get_fundamentals(&caller).await.unwrap();

        let actual = fundamentals
            .iter()
            .filter_map(|fundamental| fundamental.symbol.as_deref())
            .collect::<Vec<_>>();
        let expected = caller.to_vec();
        assert_eq!(actual, expected);
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0]
                .url
                .query_pairs()
                .find(|(name, _)| name == "symbols")
                .unwrap()
                .1
                .split(',')
                .count(),
            100
        );
        assert_eq!(
            requests[1]
                .url
                .query_pairs()
                .find(|(name, _)| name == "symbols")
                .unwrap()
                .1
                .split(',')
                .count(),
            1
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn historicals_count_fixed_parameters_and_restore_duplicate_groups() {
        let server = MockServer::start().await;
        let first = format!("A{}", "A".repeat(2_399));
        let second = format!("B{}", "B".repeat(2_399));
        for (symbol, close) in [(&first, "1"), (&second, "2")] {
            Mock::given(method("GET"))
                .and(path("/quotes/historicals/"))
                .and(query_param("symbols", symbol.as_str()))
                .and(query_param("interval", "5minute"))
                .and(query_param("span", "day"))
                .and(query_param("bounds", "regular"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "results": [{
                        "symbol": symbol,
                        "historicals": [{"close_price": close}]
                    }]
                })))
                .expect(1)
                .mount(&server)
                .await;
        }
        let (_dir, client) = client_for_server(&server.uri()).await;

        let candles = client
            .get_stock_historicals(
                &[second.as_str(), first.as_str(), second.as_str()],
                &historical_opts(),
            )
            .await
            .unwrap();

        assert_eq!(
            candles
                .iter()
                .filter_map(|candle| candle.close_price.as_deref())
                .collect::<Vec<_>>(),
            ["2", "1", "2"]
        );
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            requests
                .iter()
                .all(|request| request.url.as_str().len() <= 4_096)
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn empty_market_data_inputs_make_no_http_requests() {
        let server = MockServer::start().await;
        let (_dir, client) = client_for_server(&server.uri()).await;
        let invalid_empty_opts = HistoricalOpts {
            interval: HistoricalInterval::FiveMinute,
            span: HistoricalSpan::Week,
            bounds: HistoricalBounds::Extended,
        };

        assert!(client.get_quotes(&[]).await.unwrap().is_empty());
        assert!(client.get_fundamentals(&[]).await.unwrap().is_empty());
        assert!(
            client
                .get_stock_historicals(&[], &historical_opts())
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            client
                .get_stock_historicals(&[], &invalid_empty_opts)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn fundamentals_without_a_requested_symbol_fail_the_whole_call() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/fundamentals/"))
            .and(query_param("symbols", "AAPL"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let error = client.get_fundamentals(&["AAPL"]).await.unwrap_err();

        assert!(matches!(
            error,
            RhoodError::RequestChunkFailed { source, .. }
                if matches!(*source, RhoodError::InvalidMarketDataResponse { .. })
        ));
        server.verify().await;
    }

    #[tokio::test]
    async fn historicals_without_a_requested_symbol_fail_the_whole_call() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/quotes/historicals/"))
            .and(query_param("symbols", "AAPL"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"historicals": [{"close_price": "1"}]}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let error = client
            .get_stock_historicals(&["AAPL"], &historical_opts())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            RhoodError::RequestChunkFailed { source, .. }
                if matches!(*source, RhoodError::InvalidMarketDataResponse { .. })
        ));
        server.verify().await;
    }

    #[tokio::test]
    async fn oversized_quote_input_fails_before_http_without_echoing_the_value() {
        let server = MockServer::start().await;
        let (_dir, client) = client_for_server(&server.uri()).await;
        let oversized = "S".repeat(5_000);

        let error = client.get_quotes(&[oversized.as_str()]).await.unwrap_err();

        assert!(matches!(error, RhoodError::InvalidParameter(_)));
        assert!(error.to_string().contains("quotes"));
        assert!(error.to_string().contains("ordinal 1"));
        assert!(!error.to_string().contains(&oversized));
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_later_failed_chunk_stops_the_call_and_returns_chunk_context() {
        let server = MockServer::start().await;
        let symbols = (0..201)
            .map(|index| format!("S{index:03}"))
            .collect::<Vec<_>>();
        Mock::given(method("GET"))
            .and(path("/fundamentals/"))
            .and(query_param("symbols", symbols[..100].join(",")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": []
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/fundamentals/"))
            .and(query_param("symbols", symbols[100..200].join(",")))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .expect(3)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;
        let caller = symbols.iter().map(String::as_str).collect::<Vec<_>>();

        let error = client.get_fundamentals(&caller).await.unwrap_err();

        match error {
            RhoodError::RequestChunkFailed {
                endpoint,
                chunk_index,
                total_chunks,
                source,
            } => {
                assert_eq!(endpoint, "fundamentals");
                assert_eq!(chunk_index, 2);
                assert_eq!(total_chunks, 3);
                assert!(matches!(*source, RhoodError::RateLimited { .. }));
            }
            other => panic!("unexpected error: {other}"),
        }
        assert_eq!(server.received_requests().await.unwrap().len(), 4);
        server.verify().await;
    }

    #[test]
    fn historical_bounds_validation_extended_with_week_span() {
        let opts = HistoricalOpts {
            interval: HistoricalInterval::FiveMinute,
            span: HistoricalSpan::Week,
            bounds: HistoricalBounds::Extended,
        };
        assert!(matches!(
            opts.bounds,
            HistoricalBounds::Extended | HistoricalBounds::Trading
        ));
        assert!(!matches!(opts.span, HistoricalSpan::Day));
    }

    #[test]
    fn historical_bounds_validation_extended_with_day_span() {
        let opts = HistoricalOpts {
            interval: HistoricalInterval::FiveMinute,
            span: HistoricalSpan::Day,
            bounds: HistoricalBounds::Extended,
        };
        assert!(matches!(opts.span, HistoricalSpan::Day));
    }

    #[test]
    fn historical_bounds_validation_regular_with_any_span() {
        let opts = HistoricalOpts {
            interval: HistoricalInterval::Day,
            span: HistoricalSpan::Year,
            bounds: HistoricalBounds::Regular,
        };
        assert!(!matches!(
            opts.bounds,
            HistoricalBounds::Extended | HistoricalBounds::Trading
        ));
    }

    #[tokio::test]
    async fn resolve_symbols_cache_only_path_skips_http() {
        use crate::RhoodConfig;
        let client = RobinhoodClient::with_config(RhoodConfig::default()).unwrap();
        client
            .resolvers
            .instruments_by_id
            .insert("u1".to_string(), "AAPL".to_string())
            .await;
        client
            .resolvers
            .instruments_by_id
            .insert("u2".to_string(), "NVDA".to_string())
            .await;
        let resolved = client
            .resolve_symbols(&["u1".to_string(), "u2".to_string()])
            .await
            .unwrap();
        assert_eq!(resolved.get("u1").map(String::as_str), Some("AAPL"));
        assert_eq!(resolved.get("u2").map(String::as_str), Some("NVDA"));
        assert_eq!(resolved.len(), 2);
    }

    #[test]
    fn historical_bounds_validation_trading_with_day_span() {
        let opts = HistoricalOpts {
            interval: HistoricalInterval::FiveMinute,
            span: HistoricalSpan::Month,
            bounds: HistoricalBounds::Trading,
        };
        let is_invalid = matches!(
            opts.bounds,
            HistoricalBounds::Extended | HistoricalBounds::Trading
        ) && !matches!(opts.span, HistoricalSpan::Day);
        assert!(is_invalid);
    }

    #[test]
    fn fundamentals_deserializes_full_snapshot() {
        let json = r#"{
            "open": "150.00",
            "high": "155.00",
            "low": "149.00",
            "volume": "1200000",
            "market_cap": "2500000000000.00",
            "pe_ratio": "28.50",
            "dividend_yield": "0.55",
            "sector": "Technology",
            "industry": "Consumer Electronics",
            "symbol": "AAPL",
            "ceo": "Tim Cook",
            "num_employees": 164000,
            "year_founded": 1976
        }"#;
        let fund: Fundamentals = serde_json::from_str(json).unwrap();
        assert_eq!(fund.symbol.as_deref(), Some("AAPL"));
        assert_eq!(fund.sector.as_deref(), Some("Technology"));
        assert_eq!(fund.pe_ratio.as_deref(), Some("28.50"));
        assert_eq!(fund.num_employees, Some(164000));
        assert_eq!(fund.year_founded, Some(1976));
    }

    #[test]
    fn fundamentals_handles_missing_fields() {
        let json = r#"{ "symbol": "XYZ" }"#;
        let fund: Fundamentals = serde_json::from_str(json).unwrap();
        assert_eq!(fund.symbol.as_deref(), Some("XYZ"));
        assert!(fund.pe_ratio.is_none());
        assert!(fund.market_cap.is_none());
    }

    #[test]
    fn stock_quote_deserializes_real_api_shape() {
        let json = r#"{
            "ask_price": "261.500000",
            "ask_size": 1108,
            "venue_ask_time": "2026-03-11T00:00:00.231504626Z",
            "bid_price": "258.360000",
            "bid_size": 38,
            "venue_bid_time": "2026-03-11T00:00:00.231504626Z",
            "last_trade_price": "260.720000",
            "venue_last_trade_time": "2026-03-10T19:59:59.976239327Z",
            "last_extended_hours_trade_price": "261.200000",
            "last_non_reg_trade_price": "261.200000",
            "venue_last_non_reg_trade_time": "2026-03-10T23:50:47.288714718Z",
            "previous_close": "259.880000",
            "adjusted_previous_close": "259.880000",
            "previous_close_date": "2026-03-09",
            "symbol": "AAPL",
            "trading_halted": false,
            "has_traded": true,
            "last_trade_price_source": "nls",
            "last_non_reg_trade_price_source": "nls",
            "updated_at": "2026-03-11T00:00:00Z",
            "instrument": "https://api.robinhood.com/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e/",
            "instrument_id": "450dfc6d-5510-4d40-abfb-f633b7d9be3e",
            "state": "active"
        }"#;
        let quote: StockQuote = serde_json::from_str(json).unwrap();
        assert_eq!(quote.symbol.as_deref(), Some("AAPL"));
        assert_eq!(
            quote.instrument_id.as_deref(),
            Some("450dfc6d-5510-4d40-abfb-f633b7d9be3e")
        );
        assert_eq!(quote.state.as_deref(), Some("active"));
        assert_eq!(
            quote.last_non_reg_trade_price.as_deref(),
            Some("261.200000")
        );
        assert_eq!(
            quote.last_non_reg_trade_price_source.as_deref(),
            Some("nls")
        );
        assert!(quote.venue_ask_time.is_some());
        assert!(quote.venue_bid_time.is_some());
        assert!(quote.venue_last_trade_time.is_some());
        assert!(quote.venue_last_non_reg_trade_time.is_some());
    }

    #[test]
    fn fundamentals_deserializes_real_api_shape() {
        let json = r#"{
            "open": "257.740000",
            "high": "262.480000",
            "low": "256.950000",
            "volume": "30587286.000000",
            "overnight_volume": "0.000000",
            "bounds": "regular",
            "market_date": "2026-03-10",
            "average_volume_2_weeks": "41821391.854018",
            "average_volume": "41821391.854018",
            "average_volume_30_days": "45020359.323600",
            "high_52_weeks": "288.620000",
            "high_52_weeks_date": "2025-12-03",
            "dividend_yield": "0.400185",
            "float": "14664480994.799999",
            "low_52_weeks": "169.210100",
            "low_52_weeks_date": "2025-04-08",
            "market_cap": "3827666801057.393066",
            "pb_ratio": "43.326200",
            "pe_ratio": "32.880387",
            "shares_outstanding": "14681139924.276590",
            "description": "Apple, Inc.",
            "instrument": "https://api.robinhood.com/instruments/450dfc6d/",
            "ceo": "Timothy Donald Cook",
            "headquarters_city": "Cupertino",
            "headquarters_state": "California",
            "sector": "Electronic Technology",
            "industry": "Telecommunications Equipment",
            "num_employees": 166000,
            "year_founded": 1976,
            "payable_date": "2026-02-12",
            "ex_dividend_date": "2026-02-09",
            "financial_status_indicator": "CC0",
            "financial_status_description": ""
        }"#;
        let fund: Fundamentals = serde_json::from_str(json).unwrap();
        assert_eq!(fund.overnight_volume.as_deref(), Some("0.000000"));
        assert_eq!(fund.bounds.as_deref(), Some("regular"));
        assert_eq!(fund.market_date.as_deref(), Some("2026-03-10"));
        assert_eq!(
            fund.average_volume_30_days.as_deref(),
            Some("45020359.323600")
        );
        assert_eq!(fund.high_52_weeks_date.as_deref(), Some("2025-12-03"));
        assert_eq!(fund.low_52_weeks_date.as_deref(), Some("2025-04-08"));
        assert_eq!(fund.payable_date.as_deref(), Some("2026-02-12"));
        assert_eq!(fund.ex_dividend_date.as_deref(), Some("2026-02-09"));
        assert_eq!(fund.financial_status_indicator.as_deref(), Some("CC0"));
        assert_eq!(fund.num_employees, Some(166000));
    }

    #[test]
    fn batch_ids_response_deserializes_full_instrument() {
        use crate::models::stock::Instrument;
        use crate::pagination::ResultsResponse;
        // Real shape of GET /instruments/?ids=a,b : a paginated envelope whose
        // results are FULL instrument objects with many fields beyond what
        // `Instrument` declares. This must still deserialize and expose id +
        // symbol (the contract `resolve_symbols` relies on). Guards against a
        // future `deny_unknown_fields` or type change reintroducing null
        // enrichment.
        let json = r#"{
            "next": null,
            "previous": null,
            "results": [
                {
                    "id": "450dfc6d-5510-4d40-abfb-f633b7d9be3e",
                    "url": "https://api.robinhood.com/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e/",
                    "symbol": "AAPL",
                    "simple_name": "Apple",
                    "name": "Apple Inc. Common Stock",
                    "tradeable": true,
                    "bloomberg_unique": "EQ0010169500001000",
                    "day_trade_ratio": "0.2500",
                    "list_date": "1990-01-02",
                    "state": "active"
                }
            ]
        }"#;
        let resp: ResultsResponse<Instrument> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.results.len(), 1);
        assert_eq!(
            resp.results[0].id.as_deref(),
            Some("450dfc6d-5510-4d40-abfb-f633b7d9be3e")
        );
        assert_eq!(resp.results[0].symbol.as_deref(), Some("AAPL"));
    }

    #[test]
    fn batch_ids_response_skips_null_entries() {
        // Robinhood's GET /instruments/?ids=a,b returns a literal `null` in the
        // `results` array for delisted/invalid ids (observed live for a closed
        // zero-quantity position). A plain `Vec<Instrument>` fails the WHOLE
        // batch parse on that null, zeroing out symbol enrichment for every
        // position in the request. The batch response must tolerate nulls and
        // keep the resolvable instruments.
        let json = r#"{
            "next": null,
            "previous": null,
            "results": [
                {
                    "id": "450dfc6d-5510-4d40-abfb-f633b7d9be3e",
                    "symbol": "AAPL",
                    "state": "active"
                },
                null,
                {
                    "id": "18226051-6bfa-4c56-bd9a-d7575f0245c1",
                    "symbol": "VTI",
                    "state": "active"
                }
            ]
        }"#;
        let resp: InstrumentBatchResponse = serde_json::from_str(json).unwrap();
        let resolved: Vec<&Instrument> = resp.results.iter().flatten().collect();
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].symbol.as_deref(), Some("AAPL"));
        assert_eq!(resolved[1].symbol.as_deref(), Some("VTI"));
    }

    #[test]
    fn instrument_deserializes_real_api_shape() {
        let json = r#"{
            "id": "450dfc6d-5510-4d40-abfb-f633b7d9be3e",
            "url": "https://api.robinhood.com/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e/",
            "quote": "https://api.robinhood.com/quotes/AAPL/",
            "fundamentals": "https://api.robinhood.com/fundamentals/AAPL/",
            "splits": "https://api.robinhood.com/instruments/450dfc6d/splits/",
            "state": "active",
            "market": "https://api.robinhood.com/markets/XNAS/",
            "simple_name": "Apple",
            "name": "Apple Inc. Common Stock",
            "tradeable": true,
            "tradability": "tradable",
            "symbol": "AAPL",
            "bloomberg_unique": "EQ0010169500001000",
            "margin_initial_ratio": "0.5000",
            "maintenance_ratio": "0.2500",
            "country": "US",
            "day_trade_ratio": "0.2500",
            "list_date": "1990-01-02",
            "min_tick_size": null,
            "type": "stock",
            "tradable_chain_id": "7dd906e5-7d4b-4161-a3fe-2c3b62038482",
            "rhs_tradability": "tradable",
            "affiliate_tradability": "tradable",
            "fractional_tradability": "tradable",
            "short_selling_tradability": "tradable",
            "default_collar_fraction": "0.05",
            "is_spac": false,
            "is_test": false,
            "extended_hours_fractional_tradability": false,
            "all_day_tradability": "tradable",
            "notional_estimated_quantity_decimals": 6,
            "tax_security_type": "stock",
            "car_required": false,
            "high_risk_maintenance_ratio": "0.2500",
            "low_risk_maintenance_ratio": "0.2500",
            "default_preset_percent_limit": "0.02",
            "affiliate": "rhf",
            "account_type_tradabilities": [
                {
                    "account_type": "individual",
                    "account_type_tradability": "tradable"
                }
            ],
            "issuer_type": "third_party"
        }"#;
        let inst: Instrument = serde_json::from_str(json).unwrap();
        assert_eq!(inst.symbol.as_deref(), Some("AAPL"));
        assert_eq!(inst.state.as_deref(), Some("active"));
        assert_eq!(inst.bloomberg_unique.as_deref(), Some("EQ0010169500001000"));
        assert_eq!(inst.margin_initial_ratio.as_deref(), Some("0.5000"));
        assert_eq!(inst.day_trade_ratio.as_deref(), Some("0.2500"));
        assert_eq!(inst.list_date.as_deref(), Some("1990-01-02"));
        assert_eq!(inst.rhs_tradability.as_deref(), Some("tradable"));
        assert_eq!(inst.short_selling_tradability.as_deref(), Some("tradable"));
        assert_eq!(inst.is_spac, Some(false));
        assert_eq!(inst.is_test, Some(false));
        assert_eq!(inst.extended_hours_fractional_tradability, Some(false));
        assert_eq!(inst.notional_estimated_quantity_decimals, Some(6));
        assert_eq!(inst.tax_security_type.as_deref(), Some("stock"));
        assert_eq!(inst.car_required, Some(false));
        assert_eq!(inst.issuer_type.as_deref(), Some("third_party"));
        let tradabilities = inst.account_type_tradabilities.unwrap();
        assert_eq!(tradabilities.len(), 1);
        assert_eq!(tradabilities[0].account_type.as_deref(), Some("individual"));
    }
}
