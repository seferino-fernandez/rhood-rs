use std::collections::HashMap;

use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::endpoints::account::validate_account_number;
use crate::endpoints::request_batch::{ChunkLimit, chunk_failed, plan_request_chunks};
use crate::models::option::*;
use crate::pagination::ResultsResponse;
use crate::{Result, RhoodError};

/// Index symbols supported for index options trading.
pub const INDEX_SYMBOLS: &[&str] = &["SPX", "NDX", "VIX", "RUT", "XSP"];

const OPTION_MARKET_DATA_ITEM_LIMIT: usize = 17;

/// Maps an index symbol to the chain symbol used for weekly option contract lookups.
///
/// Most index symbols have weekly variants with different suffixes.
/// Non-index symbols pass through unchanged.
pub fn index_chain_symbol(symbol: &str) -> &str {
    match symbol {
        "SPX" => "SPXW",
        "NDX" => "NDXP",
        "VIX" => "VIXW",
        "RUT" => "RUTW",
        _ => symbol,
    }
}

impl RobinhoodClient {
    /// Fetches the option chain for a given stock symbol.
    ///
    /// Resolves the symbol to its instrument and then retrieves the
    /// associated tradable chain.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidSymbol`] if the symbol has no tradable
    /// option chain. Also returns an error on HTTP or deserialization failures.
    pub async fn get_option_chain(&self, symbol: &str) -> Result<OptionChain> {
        let instrument = self.cached_instrument(symbol).await?;
        let chain_id = instrument
            .and_then(|instrument| instrument.tradable_chain_id.clone())
            .ok_or_else(|| RhoodError::InvalidSymbol(symbol.to_string()))?;
        let url = format!("{}{chain_id}/", self.api_url(paths::OPTION_CHAINS));
        self.get(&url).await
    }

    /// Searches for option contracts matching the specified criteria.
    ///
    /// Filters by symbol, expiration date, option type (`"call"` or `"put"`),
    /// and optionally a specific strike price. Only active contracts are
    /// returned.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidSymbol`] if the symbol has no tradable
    /// option chain. Also returns an error on HTTP or deserialization failures.
    pub async fn find_options(
        &self,
        symbol: &str,
        expiration_date: &str,
        option_type: &str,
        strike_price: Option<&str>,
    ) -> Result<Vec<OptionInstrument>> {
        let instrument = self.cached_instrument(symbol).await?;
        let chain_id = instrument
            .and_then(|instrument| instrument.tradable_chain_id.clone())
            .ok_or_else(|| RhoodError::InvalidSymbol(symbol.to_string()))?;
        let mut params: Vec<(&str, &str)> = vec![
            ("chain_id", &chain_id),
            ("expiration_dates", expiration_date),
            ("type", option_type),
            ("state", "active"),
        ];
        if let Some(strike) = strike_price {
            params.push(("strike_price", strike));
        }
        self.get_paginated(&self.api_url(paths::OPTION_INSTRUMENTS), &params)
            .await
    }

    /// Fetches all option positions, including those with a zero quantity.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_option_positions(&self, account_number: &str) -> Result<Vec<OptionPosition>> {
        validate_account_number(account_number)?;
        self.get_paginated(
            &self.api_url(paths::OPTION_POSITIONS),
            &[("account_numbers", account_number)],
        )
        .await
    }

    /// Fetches only open option positions (quantity greater than zero).
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying positions request fails.
    pub async fn get_open_option_positions(
        &self,
        account_number: &str,
    ) -> Result<Vec<OptionPosition>> {
        let positions = self.get_option_positions(account_number).await?;
        Ok(positions
            .into_iter()
            .filter(|position| {
                position
                    .quantity
                    .as_deref()
                    .and_then(|quantity| quantity.parse::<f64>().ok())
                    .is_some_and(|quantity| quantity > 0.0)
            })
            .collect())
    }

    /// Fetches live market data for specific option contracts.
    ///
    /// Resolves each [`OptionContractSpec`] to its instrument URL via
    /// [`find_options`](Self::find_options), then fetches bid/ask, Greeks,
    /// volume, open interest, and probability data from
    /// `/marketdata/options/`. Resolved URLs are stable-deduplicated into
    /// sequential requests of at most 17 URLs, then results are reconstructed
    /// in contract order with duplicate cardinality preserved.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidParameter`] if any contract spec does not
    /// match an active option instrument or if any resolved contract lacks a
    /// market-data record. A failed market-data chunk returns
    /// [`RhoodError::RequestChunkFailed`] with its one-based position and no
    /// partial results.
    pub async fn get_option_market_data(
        &self,
        symbol: &str,
        contracts: &[OptionContractSpec<'_>],
    ) -> Result<Vec<OptionMarketData>> {
        if contracts.is_empty() {
            return Ok(Vec::new());
        }

        let mut instrument_urls: Vec<String> = Vec::with_capacity(contracts.len());

        for spec in contracts {
            let results = self
                .find_options(
                    symbol,
                    spec.expiration_date,
                    spec.option_type,
                    Some(spec.strike_price),
                )
                .await?;

            let instrument = results.into_iter().next().ok_or_else(|| {
                RhoodError::InvalidParameter(format!(
                    "No contract found for {} ${} {} {}",
                    symbol.to_uppercase(),
                    spec.strike_price,
                    spec.option_type,
                    spec.expiration_date,
                ))
            })?;

            let url = instrument.url.ok_or_else(|| {
                RhoodError::InvalidParameter(format!(
                    "Option instrument for {} ${} {} {} has no URL",
                    symbol.to_uppercase(),
                    spec.strike_price,
                    spec.option_type,
                    spec.expiration_date,
                ))
            })?;

            instrument_urls.push(url);
        }

        let results = self
            .get_option_market_data_by_instrument_urls(&instrument_urls)
            .await?;
        for (index, instrument_url) in instrument_urls.iter().enumerate() {
            if !results
                .iter()
                .any(|result| result.instrument.as_deref() == Some(instrument_url.as_str()))
            {
                return Err(RhoodError::InvalidParameter(format!(
                    "option market data is missing for contract at input ordinal {}",
                    index.saturating_add(1)
                )));
            }
        }
        Ok(results)
    }

    /// Fetches live market data for option instrument URLs.
    ///
    /// Sends the supplied URLs directly to `/marketdata/options/` without
    /// performing option-instrument discovery. URLs are stable-deduplicated
    /// into sequential requests of at most 17, with the joined value encoded
    /// exactly once. Results follow caller order and repeat for duplicate input
    /// URLs; missing records are omitted.
    ///
    /// # Errors
    ///
    /// If a request chunk fails, returns [`RhoodError::RequestChunkFailed`]
    /// with its one-based position and no partial results.
    pub async fn get_option_market_data_by_instrument_urls(
        &self,
        instrument_urls: &[String],
    ) -> Result<Vec<OptionMarketData>> {
        let chunks = plan_request_chunks(
            "option market data",
            instrument_urls,
            ChunkLimit::ItemCount(OPTION_MARKET_DATA_ITEM_LIMIT),
        )?;
        let total_chunks = chunks.len();
        let url = self.api_url(paths::OPTION_MARKET_DATA);
        let mut by_instrument = HashMap::with_capacity(instrument_urls.len());

        for (index, chunk) in chunks.iter().enumerate() {
            let joined_instruments = chunk
                .iter()
                .map(|input| input.value.as_str())
                .collect::<Vec<_>>()
                .join(",");
            let params = [("instruments", joined_instruments.as_str())];
            let resp: ResultsResponse<OptionMarketData> = self
                .get_with_params(&url, &params)
                .await
                .map_err(|source| {
                    chunk_failed(
                        "option market data",
                        index.saturating_add(1),
                        total_chunks,
                        source,
                    )
                })?;
            for market_data in resp.results {
                if let Some(instrument) = market_data.instrument.as_ref() {
                    by_instrument
                        .entry(instrument.clone())
                        .or_insert(market_data);
                }
            }
        }

        Ok(instrument_urls
            .iter()
            .filter_map(|instrument| by_instrument.get(instrument).cloned())
            .collect())
    }

    /// Fetches the option chain for an index symbol (e.g., "SPX").
    ///
    /// Resolves the symbol to its index instrument, picks the first
    /// `tradable_chain_ids` entry, and retrieves the chain metadata.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidSymbol`] if the index has no tradable
    /// option chain. Also returns an error on HTTP or deserialization failures.
    pub async fn get_index_option_chain(&self, symbol: &str) -> Result<OptionChain> {
        let index = self
            .cached_index_instrument(symbol)
            .await?
            .ok_or_else(|| RhoodError::InvalidSymbol(symbol.to_string()))?;
        let chain_id = index
            .tradable_chain_ids
            .clone()
            .and_then(|mut ids| {
                ids.sort();
                ids.into_iter().next()
            })
            .ok_or_else(|| RhoodError::InvalidSymbol(symbol.to_string()))?;
        let url = format!("{}{chain_id}/", self.api_url(paths::OPTION_CHAINS));
        self.get(&url).await
    }

    /// Searches for index option contracts matching the specified criteria.
    ///
    /// Applies the weekly suffix mapping (e.g., SPX -> SPXW) and resolves
    /// the chain ID from the index instrument.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidSymbol`] if the index has no tradable
    /// option chain. Also returns an error on HTTP or deserialization failures.
    pub async fn find_index_options(
        &self,
        symbol: &str,
        expiration_date: &str,
        option_type: OptionType,
        strike_price: Option<&str>,
    ) -> Result<Vec<OptionInstrument>> {
        let index = self
            .cached_index_instrument(symbol)
            .await?
            .ok_or_else(|| RhoodError::InvalidSymbol(symbol.to_string()))?;
        let chain_id = index
            .tradable_chain_ids
            .clone()
            .and_then(|mut ids| {
                ids.sort();
                ids.into_iter().next()
            })
            .ok_or_else(|| RhoodError::InvalidSymbol(symbol.to_string()))?;
        let chain_symbol = index_chain_symbol(symbol);
        let option_type_string = option_type.to_string();
        let mut params: Vec<(&str, &str)> = vec![
            ("chain_id", &chain_id),
            ("chain_symbol", chain_symbol),
            ("expiration_dates", expiration_date),
            ("type", option_type_string.as_str()),
            ("state", "active"),
        ];
        if let Some(strike) = strike_price {
            params.push(("strike_price", strike));
        }
        self.get_paginated(&self.api_url(paths::OPTION_INSTRUMENTS), &params)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RhoodConfig;
    use crate::models::option::{OptionContractSpec, OptionMarketData, OptionPosition};
    use crate::models::order::OptionOrder;
    use crate::models::stock::{IndexInstrument, IndexQuoteWrapper};
    use secrecy::SecretString;
    use wiremock::matchers::{method, path, query_param, query_param_is_missing};
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

    async fn mount_equity_option_lookup(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .and(query_param("symbol", "AAPL"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"symbol": "AAPL", "tradable_chain_id": "chain-aapl"}]
            })))
            .expect(1)
            .mount(server)
            .await;
    }

    fn option_search_response() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": [{
                "chain_id": "chain-aapl",
                "chain_symbol": "AAPL",
                "expiration_date": "2026-06-18",
                "id": "call-310",
                "state": "active",
                "strike_price": "310.0000",
                "type": "call"
            }],
            "next": null,
            "previous": null
        }))
    }

    #[tokio::test]
    async fn option_positions_use_plural_account_filter() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/options/positions/"))
            .and(query_param("account_numbers", "ABC123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [],
                "next": null
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        assert!(
            client
                .get_option_positions("ABC123")
                .await
                .unwrap()
                .is_empty()
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn find_options_omits_optional_strike_filter() {
        let server = MockServer::start().await;
        mount_equity_option_lookup(&server).await;
        Mock::given(method("GET"))
            .and(path("/options/instruments/"))
            .and(query_param("chain_id", "chain-aapl"))
            .and(query_param("expiration_dates", "2026-06-18"))
            .and(query_param("type", "call"))
            .and(query_param("state", "active"))
            .and(query_param_is_missing("strike_price"))
            .respond_with(option_search_response())
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let options = client
            .find_options("AAPL", "2026-06-18", "call", None)
            .await
            .unwrap();

        assert_eq!(options.len(), 1);
        assert_eq!(options[0].strike_price.as_deref(), Some("310.0000"));
        server.verify().await;
    }

    #[tokio::test]
    async fn find_options_includes_optional_strike_filter() {
        let server = MockServer::start().await;
        mount_equity_option_lookup(&server).await;
        Mock::given(method("GET"))
            .and(path("/options/instruments/"))
            .and(query_param("chain_id", "chain-aapl"))
            .and(query_param("expiration_dates", "2026-06-18"))
            .and(query_param("type", "call"))
            .and(query_param("state", "active"))
            .and(query_param("strike_price", "310.0000"))
            .respond_with(option_search_response())
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let options = client
            .find_options("AAPL", "2026-06-18", "call", Some("310.0000"))
            .await
            .unwrap();

        assert_eq!(options.len(), 1);
        assert_eq!(options[0].id.as_deref(), Some("call-310"));
        server.verify().await;
    }

    #[tokio::test]
    async fn option_market_data_by_instrument_urls_queries_market_data_without_discovery() {
        let server = MockServer::start().await;
        let instrument_urls = vec![
            "https://api.robinhood.com/options/instruments/held-call/".to_string(),
            "https://api.robinhood.com/options/instruments/held-put/".to_string(),
        ];
        Mock::given(method("GET"))
            .and(path("/marketdata/options/"))
            .and(query_param("instruments", instrument_urls.join(",")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{
                    "instrument": instrument_urls[0],
                    "instrument_id": "held-call",
                    "bid_price": "1.20",
                    "ask_price": "1.30"
                }],
                "next": null,
                "previous": null
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let quotes = client
            .get_option_market_data_by_instrument_urls(&instrument_urls)
            .await
            .unwrap();

        assert_eq!(quotes.len(), 1);
        assert_eq!(
            quotes[0].instrument.as_deref(),
            Some(instrument_urls[0].as_str())
        );
        assert_eq!(quotes[0].instrument_id.as_deref(), Some("held-call"));
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1, "URL mode must make exactly one request");
        assert_eq!(requests[0].url.path(), "/marketdata/options/");
        server.verify().await;
    }

    #[tokio::test]
    async fn option_market_data_sends_seventeen_plus_one_and_restores_order_and_duplicates() {
        let server = MockServer::start().await;
        let unique = (0..18)
            .map(|index| {
                format!("https://api.robinhood.com/options/instruments/option-{index:02}/")
            })
            .collect::<Vec<_>>();
        let first_results = unique[..17]
            .iter()
            .rev()
            .map(|instrument| {
                serde_json::json!({
                    "instrument": instrument,
                    "instrument_id": instrument.trim_end_matches('/').rsplit('/').next()
                })
            })
            .collect::<Vec<_>>();
        Mock::given(method("GET"))
            .and(path("/marketdata/options/"))
            .and(query_param("instruments", unique[..17].join(",")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"results": first_results})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/marketdata/options/"))
            .and(query_param("instruments", unique[17].as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{
                    "instrument": unique[17],
                    "instrument_id": "option-17"
                }]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;
        let mut caller = unique.clone();
        caller.push(unique[0].clone());

        let results = client
            .get_option_market_data_by_instrument_urls(&caller)
            .await
            .unwrap();

        assert_eq!(results.len(), 19);
        assert_eq!(
            results
                .iter()
                .filter_map(|result| result.instrument.as_ref())
                .collect::<Vec<_>>(),
            caller.iter().collect::<Vec<_>>()
        );
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        let decoded = requests
            .iter()
            .map(|request| {
                request
                    .url
                    .query_pairs()
                    .find(|(name, _)| name == "instruments")
                    .unwrap()
                    .1
                    .into_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(decoded, [unique[..17].join(","), unique[17].clone()]);
        server.verify().await;
    }

    #[tokio::test]
    async fn option_market_data_empty_urls_make_no_http_request() {
        let server = MockServer::start().await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        assert!(
            client
                .get_option_market_data_by_instrument_urls(&[])
                .await
                .unwrap()
                .is_empty()
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn contract_market_data_fails_when_a_resolved_contract_has_no_result() {
        let server = MockServer::start().await;
        mount_equity_option_lookup(&server).await;
        let instrument_url = "https://api.robinhood.com/options/instruments/missing-market-data/";
        Mock::given(method("GET"))
            .and(path("/options/instruments/"))
            .and(query_param("chain_id", "chain-aapl"))
            .and(query_param("expiration_dates", "2026-06-18"))
            .and(query_param("type", "call"))
            .and(query_param("state", "active"))
            .and(query_param("strike_price", "310.0000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{
                    "chain_id": "chain-aapl",
                    "chain_symbol": "AAPL",
                    "expiration_date": "2026-06-18",
                    "id": "call-310",
                    "state": "active",
                    "strike_price": "310.0000",
                    "type": "call",
                    "url": instrument_url
                }],
                "next": null,
                "previous": null
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/marketdata/options/"))
            .and(query_param("instruments", instrument_url))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": []
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;
        let contracts = [OptionContractSpec {
            strike_price: "310.0000",
            expiration_date: "2026-06-18",
            option_type: "call",
        }];

        let error = client
            .get_option_market_data("AAPL", &contracts)
            .await
            .unwrap_err();

        assert!(matches!(error, RhoodError::InvalidParameter(_)));
        assert!(error.to_string().contains("input ordinal 1"));
        server.verify().await;
    }

    #[test]
    fn option_contract_spec_fields_pass_through() {
        let spec = OptionContractSpec {
            strike_price: "50.0000",
            expiration_date: "2026-04-02",
            option_type: "put",
        };
        assert_eq!(spec.strike_price, "50.0000");
        assert_eq!(spec.expiration_date, "2026-04-02");
        assert_eq!(spec.option_type, "put");
    }

    #[test]
    fn option_market_data_deserializes_full_snapshot() {
        let json = r#"{
            "instrument": "https://api.robinhood.com/options/instruments/abc/",
            "instrument_id": "abc",
            "bid_price": "1.23",
            "ask_price": "1.35",
            "last_trade_price": "1.30",
            "mark_price": "1.29",
            "break_even_price": "48.71",
            "adjusted_mark_price": "1.29",
            "previous_close_price": "1.40",
            "high_price": "1.50",
            "low_price": "1.10",
            "delta": "-0.3500",
            "gamma": "0.0800",
            "theta": "-0.0500",
            "vega": "0.1200",
            "rho": "-0.0100",
            "implied_volatility": "0.4500",
            "volume": 1204,
            "open_interest": 8923,
            "chance_of_profit_long": "0.35",
            "chance_of_profit_short": "0.65",
            "updated_at": "2026-04-01T16:00:00Z"
        }"#;
        let data: OptionMarketData = serde_json::from_str(json).unwrap();
        assert_eq!(data.bid_price.as_deref(), Some("1.23"));
        assert_eq!(data.ask_price.as_deref(), Some("1.35"));
        assert_eq!(data.delta.as_deref(), Some("-0.3500"));
        assert_eq!(data.volume, Some(1204));
        assert_eq!(data.open_interest, Some(8923));
        assert_eq!(data.chance_of_profit_long.as_deref(), Some("0.35"));
    }

    #[test]
    fn option_market_data_handles_missing_fields() {
        let json = r#"{
            "bid_price": "1.23",
            "ask_price": "1.35"
        }"#;
        let data: OptionMarketData = serde_json::from_str(json).unwrap();
        assert_eq!(data.bid_price.as_deref(), Some("1.23"));
        assert!(data.delta.is_none());
        assert!(data.volume.is_none());
        assert!(data.instrument_id.is_none());
    }

    #[test]
    fn option_position_deserializes_full_snapshot() {
        let json = r#"{
            "account": "https://api.robinhood.com/accounts/ABC123/",
            "average_price": "1.5400",
            "chain_id": "chain-001",
            "chain_symbol": "AAPL",
            "id": "pos-001",
            "option": "https://api.robinhood.com/options/instruments/opt-001/",
            "quantity": "2.0000",
            "type": "long",
            "created_at": "2026-03-15T10:00:00Z",
            "updated_at": "2026-03-31T14:00:00Z"
        }"#;
        let pos: OptionPosition = serde_json::from_str(json).unwrap();
        assert_eq!(pos.chain_symbol.as_deref(), Some("AAPL"));
        assert_eq!(pos.quantity.as_deref(), Some("2.0000"));
        assert_eq!(pos.average_price.as_deref(), Some("1.5400"));
        assert_eq!(pos.position_type.as_deref(), Some("long"));
        assert_eq!(pos.chain_id.as_deref(), Some("chain-001"));
        assert_eq!(pos.id.as_deref(), Some("pos-001"));
    }

    #[test]
    fn option_position_handles_missing_fields() {
        let json = r#"{
            "chain_symbol": "TSLA",
            "quantity": "1.0000",
            "type": "short"
        }"#;
        let pos: OptionPosition = serde_json::from_str(json).unwrap();
        assert_eq!(pos.chain_symbol.as_deref(), Some("TSLA"));
        assert_eq!(pos.position_type.as_deref(), Some("short"));
        assert!(pos.average_price.is_none());
        assert!(pos.account.is_none());
        assert!(pos.id.is_none());
    }

    #[test]
    fn option_position_serializes_round_trip() {
        let json = r#"{
            "account": null,
            "average_price": "3.2000",
            "chain_id": "chain-002",
            "chain_symbol": "NKE",
            "id": "pos-002",
            "option": "https://api.robinhood.com/options/instruments/opt-002/",
            "quantity": "5.0000",
            "type": "long",
            "created_at": "2026-03-20T09:00:00Z",
            "updated_at": "2026-03-30T16:00:00Z"
        }"#;
        let pos: OptionPosition = serde_json::from_str(json).unwrap();
        let serialized = serde_json::to_string(&pos).unwrap();
        let round_tripped: OptionPosition = serde_json::from_str(&serialized).unwrap();
        assert_eq!(round_tripped.chain_symbol.as_deref(), Some("NKE"));
        assert_eq!(round_tripped.quantity.as_deref(), Some("5.0000"));
        assert_eq!(round_tripped.position_type.as_deref(), Some("long"));
    }

    #[test]
    fn option_order_deserializes_full_snapshot() {
        let json = r#"{
            "id": "opt-order-001",
            "chain_id": "chain-001",
            "chain_symbol": "AAPL",
            "direction": "debit",
            "premium": "1.54",
            "price": "1.54",
            "quantity": "2.0000",
            "state": "filled",
            "type": "limit",
            "time_in_force": "gtc",
            "cancel_url": null,
            "created_at": "2026-03-31T10:00:00Z",
            "updated_at": "2026-03-31T10:01:00Z"
        }"#;
        let order: OptionOrder = serde_json::from_str(json).unwrap();
        assert_eq!(order.id.as_deref(), Some("opt-order-001"));
        assert_eq!(order.chain_symbol.as_deref(), Some("AAPL"));
        assert_eq!(order.direction.as_deref(), Some("debit"));
        assert_eq!(order.state.as_deref(), Some("filled"));
        assert!(order.cancel_url.is_none());
    }

    #[test]
    fn option_order_open_has_cancel_url() {
        let json = r#"{
            "id": "opt-order-002",
            "chain_symbol": "NKE",
            "state": "queued",
            "cancel_url": "https://api.robinhood.com/options/orders/opt-order-002/cancel/"
        }"#;
        let order: OptionOrder = serde_json::from_str(json).unwrap();
        assert!(order.cancel_url.is_some());
    }

    #[test]
    fn index_instrument_deserializes() {
        let json = r#"{
            "id": "idx-001",
            "symbol": "SPX",
            "tradable_chain_ids": ["chain-aaa", "chain-bbb"]
        }"#;
        let idx: IndexInstrument = serde_json::from_str(json).unwrap();
        assert_eq!(idx.id.as_deref(), Some("idx-001"));
        assert_eq!(idx.symbol.as_deref(), Some("SPX"));
        let chains = idx.tradable_chain_ids.unwrap();
        assert_eq!(chains.len(), 2);
        assert_eq!(chains[0], "chain-aaa");
    }

    #[test]
    fn index_instrument_deserializes_no_chains() {
        let json = r#"{"id": "idx-002", "symbol": "VIX"}"#;
        let idx: IndexInstrument = serde_json::from_str(json).unwrap();
        assert_eq!(idx.symbol.as_deref(), Some("VIX"));
        assert!(idx.tradable_chain_ids.is_none());
    }

    #[test]
    fn index_quote_deserializes_doubly_nested_wire_response() {
        let wire = r#"{"status":"SUCCESS","data":{"status":"SUCCESS","data":{
            "value":"7126.06",
            "venue_timestamp":"2026-04-17T16:38:34.8016-04:00",
            "symbol":"SPX",
            "instrument_id":"432fbbb8-b82c-454a-852d-eb85382c7066",
            "state":"",
            "updated_at":"2026-04-17T17:57:11.709844895-04:00"
        }}}"#;
        let wrapper: IndexQuoteWrapper = serde_json::from_str(wire).unwrap();
        let quote = &wrapper.data.data;
        assert_eq!(quote.value.as_deref(), Some("7126.06"));
        assert_eq!(
            quote.venue_timestamp.as_deref(),
            Some("2026-04-17T16:38:34.8016-04:00")
        );
        assert_eq!(quote.symbol.as_deref(), Some("SPX"));
        assert_eq!(
            quote.instrument_id.as_deref(),
            Some("432fbbb8-b82c-454a-852d-eb85382c7066")
        );
        // Robinhood returns an empty state string on the wire; we pass it through.
        assert_eq!(quote.state.as_deref(), Some(""));
        assert_eq!(
            quote.updated_at.as_deref(),
            Some("2026-04-17T17:57:11.709844895-04:00")
        );
    }

    #[test]
    fn index_symbols_list_contains_expected() {
        assert!(INDEX_SYMBOLS.contains(&"SPX"));
        assert!(INDEX_SYMBOLS.contains(&"NDX"));
        assert!(INDEX_SYMBOLS.contains(&"VIX"));
        assert!(INDEX_SYMBOLS.contains(&"RUT"));
        assert!(INDEX_SYMBOLS.contains(&"XSP"));
        assert!(!INDEX_SYMBOLS.contains(&"AAPL"));
    }

    #[test]
    fn index_chain_symbol_maps_correctly() {
        assert_eq!(index_chain_symbol("SPX"), "SPXW");
        assert_eq!(index_chain_symbol("NDX"), "NDXP");
        assert_eq!(index_chain_symbol("VIX"), "VIXW");
        assert_eq!(index_chain_symbol("RUT"), "RUTW");
        assert_eq!(index_chain_symbol("XSP"), "XSP");
        assert_eq!(index_chain_symbol("AAPL"), "AAPL");
    }
}
