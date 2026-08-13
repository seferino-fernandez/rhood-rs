use std::collections::HashMap;
use std::sync::Arc;

use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::models::futures::*;
use crate::pagination::ResultsResponse;
use crate::{Result, RhoodError};

/// Returns the instrument ID a quote must carry to be attributable to a
/// requested symbol.
///
/// Mirrors the stock market-data contract: a response item that cannot
/// identify itself fails the call rather than being matched by position, which
/// is the only other thing available and is wrong whenever the batch is
/// reordered or short.
fn quote_instrument_id(instrument_id: Option<&str>) -> Result<String> {
    instrument_id
        .map(str::trim)
        .filter(|instrument_id| !instrument_id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| RhoodError::InvalidMarketDataResponse {
            endpoint: "futures quotes".to_string(),
            message: "response item omitted its instrument identifier".into(),
        })
}

impl RobinhoodClient {
    /// Fetches a futures contract by its symbol (e.g., "/ESH26" or "ESH26").
    ///
    /// # Errors
    ///
    /// Returns an error if the contract is not found or on HTTP failures.
    pub async fn get_futures_contract(&self, symbol: &str) -> Result<FuturesContract> {
        let url = format!(
            "{}symbol/{}/",
            self.api_url(paths::FUTURES_CONTRACTS),
            symbol.to_uppercase()
        );
        let wrapper: FuturesContractWrapper = self.get_futures(&url).await?;
        Ok(wrapper.result)
    }

    /// Cached wrapper around [`get_futures_contract`](Self::get_futures_contract).
    ///
    /// Returns an [`Arc<FuturesContract>`] shared with other callers for the
    /// configured TTL. When caching is disabled (`CacheConfig::enabled = false`)
    /// the upstream endpoint is hit on every call.
    pub async fn cached_futures_contract(&self, symbol: &str) -> Result<Arc<FuturesContract>> {
        let key = symbol.to_uppercase();
        if !self.resolvers.enabled {
            return Ok(Arc::new(self.get_futures_contract(&key).await?));
        }
        if let Some(hit) = self.resolvers.futures_contracts.get(&key).await {
            return Ok(hit);
        }
        let contract = self.get_futures_contract(&key).await?;
        let wrapped = Arc::new(contract);
        self.resolvers
            .futures_contracts
            .insert(key, wrapped.clone())
            .await;
        Ok(wrapped)
    }

    /// Fetches a real-time futures quote by resolving a symbol to its instrument ID.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidSymbol`] if the contract cannot be found
    /// or has no instrument ID. Also returns an error on HTTP failures.
    pub async fn get_futures_quote(&self, symbol: &str) -> Result<FuturesQuote> {
        let contract = self.cached_futures_contract(symbol).await?;
        let instrument_id = contract
            .id
            .clone()
            .ok_or_else(|| RhoodError::InvalidSymbol(symbol.to_string()))?;
        let params = [("ids", instrument_id.as_str())];
        let wrapper: FuturesQuoteDataWrapper = self
            .get_futures_with_params(&self.api_url(paths::FUTURES_QUOTES), &params)
            .await?;
        // ADR-0004 rule 1 applies to single-item requests too. "We only asked
        // for one" is exactly the reasoning that left this method trusting
        // response position while the batch method below correlates by ID.
        let quote = wrapper
            .data
            .into_iter()
            .next()
            .map(|item| item.data)
            .ok_or_else(|| RhoodError::InvalidSymbol(symbol.to_string()))?;
        if quote_instrument_id(quote.instrument_id.as_deref())? != instrument_id {
            return Err(RhoodError::InvalidMarketDataResponse {
                endpoint: "futures quotes".to_string(),
                message: "response item named an instrument outside its request".into(),
            });
        }
        Ok(quote)
    }

    /// Fetches real-time futures quotes for multiple symbols in a single request.
    ///
    /// Resolves each symbol to its instrument ID concurrently via the resolver
    /// cache, then batches the quote request. Cache hits return immediately;
    /// misses fan out via [`futures::future::try_join_all`] so N upstream
    /// contract lookups run in parallel rather than sequentially.
    ///
    /// Each returned quote is paired with the exact caller-supplied symbol it
    /// answers, correlated through the instrument ID this call resolved rather
    /// than through response position. Pairs follow caller order, and a symbol
    /// the upstream batch did not answer is absent rather than shifting the
    /// symbols after it. Callers must not re-correlate results positionally.
    ///
    /// # Errors
    ///
    /// Returns an error if any symbol cannot be resolved or on HTTP failures.
    /// Returns [`RhoodError::InvalidMarketDataResponse`] if a returned quote
    /// carries no instrument ID, or one outside this request, since neither can
    /// be attributed to a requested symbol without guessing.
    pub async fn get_futures_quotes(
        &self,
        symbols: &[&str],
    ) -> Result<Vec<(String, FuturesQuote)>> {
        let resolved_ids: Vec<String> =
            ::futures::future::try_join_all(symbols.iter().map(|symbol| async move {
                let contract = self.cached_futures_contract(symbol).await?;
                contract
                    .id
                    .clone()
                    .ok_or_else(|| RhoodError::InvalidSymbol((*symbol).to_string()))
            }))
            .await?;
        let joined_ids = resolved_ids.join(",");
        let params = [("ids", joined_ids.as_str())];
        let wrapper: FuturesQuoteDataWrapper = self
            .get_futures_with_params(&self.api_url(paths::FUTURES_QUOTES), &params)
            .await?;

        let mut by_instrument: HashMap<String, FuturesQuote> =
            HashMap::with_capacity(resolved_ids.len());
        for item in wrapper.data {
            let instrument_id = quote_instrument_id(item.data.instrument_id.as_deref())?;
            if !resolved_ids.contains(&instrument_id) {
                return Err(RhoodError::InvalidMarketDataResponse {
                    endpoint: "futures quotes".to_string(),
                    message: "response item named an instrument outside its request".into(),
                });
            }
            by_instrument.entry(instrument_id).or_insert(item.data);
        }

        Ok(symbols
            .iter()
            .zip(resolved_ids.iter())
            .filter_map(|(symbol, instrument_id)| {
                by_instrument
                    .get(instrument_id)
                    .map(|quote| ((*symbol).to_string(), quote.clone()))
            })
            .collect())
    }

    /// Discovers the Robinhood futures account ID.
    ///
    /// Queries the Ceres accounts endpoint and filters for
    /// `accountType == "FUTURES"`. Returns `None` if the user has no
    /// futures account.
    ///
    /// # Errors
    ///
    /// Returns an error on HTTP or deserialization failures.
    pub async fn get_futures_account_id(&self) -> Result<Option<String>> {
        let resp: ResultsResponse<FuturesAccount> = self
            .get_futures(&self.api_url(paths::FUTURES_ACCOUNTS))
            .await?;
        Ok(resp
            .results
            .into_iter()
            .find(|account| account.account_type.as_deref() == Some("FUTURES"))
            .and_then(|account| account.id))
    }

    /// Cached wrapper around [`get_futures_account_id`](Self::get_futures_account_id).
    ///
    /// The futures account id never changes for a given session, so it lives
    /// in a [`tokio::sync::OnceCell`]. When caching is disabled, every call
    /// hits upstream.
    ///
    /// Unlike [`get_futures_account_id`](Self::get_futures_account_id), this
    /// wrapper returns [`RhoodError::InvalidParameter`] rather than
    /// `Ok(None)` when the user has no futures account, matching the error
    /// shape existing call sites already expect.
    pub async fn cached_futures_account_id(&self) -> Result<String> {
        if !self.resolvers.enabled {
            return self
                .get_futures_account_id()
                .await?
                .ok_or_else(|| RhoodError::InvalidParameter("No futures account found".into()));
        }
        let cached = self
            .resolvers
            .futures_account_id
            .get_or_try_init(|| async {
                self.get_futures_account_id()
                    .await?
                    .ok_or_else(|| RhoodError::InvalidParameter("No futures account found".into()))
            })
            .await?;
        Ok(cached.clone())
    }

    /// Fetches all futures positions for the discovered futures account.
    ///
    /// The positions endpoint returns full `next` URLs. Each page is fetched
    /// with the futures contract header through the authenticated read policy.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidParameter`] if no futures account exists.
    /// Also returns an error on HTTP or deserialization failures.
    pub async fn get_futures_positions(&self) -> Result<Vec<FuturesPosition>> {
        let account_id = self.cached_futures_account_id().await?;
        let url = format!(
            "{}{account_id}/positions/",
            self.api_url(paths::FUTURES_ACCOUNTS)
        );
        self.get_futures_paginated(&url).await
    }

    /// Fetches all futures orders with optional date filtering.
    ///
    /// Discovers the futures account ID automatically. Uses cursor-based
    /// pagination to fetch all pages.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidParameter`] if no futures account exists.
    /// Also returns an error on HTTP or deserialization failures.
    pub async fn get_all_futures_orders(&self, since: Option<&str>) -> Result<Vec<FuturesOrder>> {
        let account_id = self.cached_futures_account_id().await?;
        let url = format!(
            "{}{account_id}/orders",
            self.api_url(paths::FUTURES_ACCOUNTS)
        );
        let mut params: Vec<(&str, &str)> = vec![("contractType", "OUTRIGHT")];
        if let Some(date) = since {
            params.push(("updated_at[gte]", date));
        }
        self.get_futures_cursor_paginated(&url, &params).await
    }
}

#[cfg(test)]
mod tests {
    use crate::RhoodError;
    use crate::client::RobinhoodClient;
    use crate::config::RhoodConfig;
    use crate::models::futures::{
        FuturesContract, FuturesContractWrapper, FuturesOrder, FuturesQuote,
        FuturesQuoteDataWrapper,
    };
    use secrecy::SecretString;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn client_for_server(server: &MockServer) -> (tempfile::TempDir, RobinhoodClient) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = RhoodConfig::default();
        config.auth.token_cache_path = dir
            .path()
            .join("nonexistent-token.json")
            .to_string_lossy()
            .into_owned();
        config.api.base_url = server.uri();
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

    /// Mounts contract lookups for each alias and one quote batch returning
    /// `quote_body`, then asks for `symbols`.
    async fn quotes_against(
        symbols: &[&str],
        contracts: &[(&str, &str)],
        quote_body: serde_json::Value,
    ) -> crate::Result<Vec<(String, FuturesQuote)>> {
        let server = MockServer::start().await;
        for (symbol, instrument_id) in contracts {
            Mock::given(method("GET"))
                .and(path(format!(
                    "/arsenal/v1/futures/contracts/symbol/{symbol}/"
                )))
                .and(header("authorization", "Bearer access-token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "result": {"id": instrument_id, "symbol": symbol}
                })))
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/marketdata/futures/quotes/v1/"))
            .and(header("authorization", "Bearer access-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(quote_body))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server).await;
        client.get_futures_quotes(symbols).await
    }

    /// Mounts one contract lookup and one quote batch, then asks for the
    /// singular quote.
    async fn single_quote_against(
        symbol: &str,
        instrument_id: &str,
        quote_body: serde_json::Value,
    ) -> crate::Result<FuturesQuote> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!(
                "/arsenal/v1/futures/contracts/symbol/{symbol}/"
            )))
            .and(header("authorization", "Bearer access-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": {"id": instrument_id, "symbol": symbol}
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/marketdata/futures/quotes/v1/"))
            .and(header("authorization", "Bearer access-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(quote_body))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server).await;
        client.get_futures_quote(symbol).await
    }

    /// ADR-0004 rule 1 covers single-item requests. This is the sibling the
    /// BL-57 fix left behind: the batch method below correlates by instrument
    /// ID, and for a day this one three lines above still took the first row.
    #[tokio::test]
    async fn single_futures_quote_rejects_an_instrument_it_did_not_request() {
        let error = single_quote_against(
            "ESM26",
            "id-es",
            serde_json::json!({"data": [
                {"data": {"symbol": "/NQM26:XCME", "instrument_id": "id-nq"}}
            ]}),
        )
        .await
        .expect_err("a quote for another instrument must be refused");

        assert!(
            matches!(
                &error,
                RhoodError::InvalidMarketDataResponse { endpoint, .. } if endpoint == "futures quotes"
            ),
            "expected a fail-closed market-data error, got {error}"
        );
    }

    #[tokio::test]
    async fn single_futures_quote_rejects_a_response_without_an_instrument_id() {
        let error = single_quote_against(
            "ESM26",
            "id-es",
            serde_json::json!({"data": [
                {"data": {"symbol": "/ESM26:XCME", "instrument_id": null}}
            ]}),
        )
        .await
        .expect_err("a quote that cannot identify itself must be refused");

        assert!(matches!(
            &error,
            RhoodError::InvalidMarketDataResponse { endpoint, .. } if endpoint == "futures quotes"
        ));
    }

    #[tokio::test]
    async fn single_futures_quote_returns_the_requested_instrument() {
        let quote = single_quote_against(
            "ESM26",
            "id-es",
            serde_json::json!({"data": [
                {"data": {"symbol": "/ESM26:XCME", "instrument_id": "id-es", "last_trade_price": "5182.75"}}
            ]}),
        )
        .await
        .unwrap();

        assert_eq!(quote.symbol.as_deref(), Some("/ESM26:XCME"));
        assert_eq!(quote.last_trade_price.as_deref(), Some("5182.75"));
    }

    /// The defect BL-57 was filed for. The upstream batch answers in an order
    /// that disagrees with the request, which the previous positional pairing
    /// would have silently mislabelled -- attaching NQ's prices to ES, and
    /// shifting every alias after it.
    #[tokio::test]
    async fn futures_quotes_pair_by_instrument_id_not_response_order() {
        let quotes = quotes_against(
            &["ESM26", "NQM26", "CLM26"],
            &[
                ("ESM26", "id-es"),
                ("NQM26", "id-nq"),
                ("CLM26", "id-cl"),
            ],
            serde_json::json!({"data": [
                {"data": {"symbol": "/CLM26:XNYM", "instrument_id": "id-cl", "last_trade_price": "61.10"}},
                {"data": {"symbol": "/NQM26:XCME", "instrument_id": "id-nq", "last_trade_price": "18250.25"}},
                {"data": {"symbol": "/ESM26:XCME", "instrument_id": "id-es", "last_trade_price": "5182.75"}}
            ]}),
        )
        .await
        .unwrap();

        let paired: Vec<(String, String, String)> = quotes
            .into_iter()
            .map(|(requested, quote)| {
                (
                    requested,
                    quote.symbol.unwrap_or_default(),
                    quote.last_trade_price.unwrap_or_default(),
                )
            })
            .collect();
        assert_eq!(
            paired,
            vec![
                (
                    "ESM26".to_string(),
                    "/ESM26:XCME".to_string(),
                    "5182.75".to_string()
                ),
                (
                    "NQM26".to_string(),
                    "/NQM26:XCME".to_string(),
                    "18250.25".to_string()
                ),
                (
                    "CLM26".to_string(),
                    "/CLM26:XNYM".to_string(),
                    "61.10".to_string()
                ),
            ]
        );
    }

    /// A short batch must lose only the symbol it omitted. Under positional
    /// pairing the absent first quote shifted every later alias by one, so this
    /// is the half of the defect that corrupts data the caller does receive.
    #[tokio::test]
    async fn futures_quotes_drop_only_the_unanswered_symbol() {
        let quotes = quotes_against(
            &["ESM26", "NQM26"],
            &[("ESM26", "id-es"), ("NQM26", "id-nq")],
            serde_json::json!({"data": [
                {"data": {"symbol": "/NQM26:XCME", "instrument_id": "id-nq", "last_trade_price": "18250.25"}}
            ]}),
        )
        .await
        .unwrap();

        assert_eq!(quotes.len(), 1);
        let (requested, quote) = quotes.into_iter().next().unwrap();
        assert_eq!(requested, "NQM26");
        assert_eq!(quote.symbol.as_deref(), Some("/NQM26:XCME"));
    }

    #[tokio::test]
    async fn futures_quotes_reject_a_response_item_without_an_instrument_id() {
        let error = quotes_against(
            &["ESM26"],
            &[("ESM26", "id-es")],
            serde_json::json!({"data": [
                {"data": {"symbol": "/ESM26:XCME", "instrument_id": null}}
            ]}),
        )
        .await
        .unwrap_err();

        assert!(
            matches!(
                &error,
                RhoodError::InvalidMarketDataResponse { endpoint, .. } if endpoint == "futures quotes"
            ),
            "expected a fail-closed market-data error, got {error}"
        );
    }

    #[tokio::test]
    async fn futures_quotes_reject_an_instrument_outside_the_request() {
        let error = quotes_against(
            &["ESM26"],
            &[("ESM26", "id-es")],
            serde_json::json!({"data": [
                {"data": {"symbol": "/NQM26:XCME", "instrument_id": "id-nq"}}
            ]}),
        )
        .await
        .unwrap_err();

        assert!(
            matches!(
                &error,
                RhoodError::InvalidMarketDataResponse { endpoint, .. } if endpoint == "futures quotes"
            ),
            "expected a fail-closed market-data error, got {error}"
        );
    }

    #[tokio::test]
    async fn futures_positions_discovers_account_and_uses_exact_positions_path() {
        let server = MockServer::start().await;
        for endpoint in [
            "/ceres/v1/accounts/",
            "/ceres/v1/accounts/futures-account/positions/",
        ] {
            let body = if endpoint.ends_with("positions/") {
                serde_json::json!({
                    "results": [{"quantity": "2", "contract": {"symbol": "/ESM26"}}],
                    "next": null,
                    "previous": null
                })
            } else {
                serde_json::json!({
                    "results": [{"id": "futures-account", "accountType": "FUTURES"}]
                })
            };
            Mock::given(method("GET"))
                .and(path(endpoint))
                .and(header("Authorization", "Bearer access-token"))
                .and(header("Rh-Contract-Protected", "true"))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .expect(1)
                .mount(&server)
                .await;
        }
        let (_dir, client) = client_for_server(&server).await;

        let positions = client.get_futures_positions().await.unwrap();

        assert_eq!(positions.len(), 1);
        assert_eq!(
            positions[0].additional_fields.get("contract"),
            Some(&serde_json::json!({"symbol": "/ESM26"}))
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn futures_positions_returns_empty_vector_for_empty_results() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ceres/v1/accounts/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": "futures-account", "accountType": "FUTURES"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/ceres/v1/accounts/futures-account/positions/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [],
                "next": null,
                "previous": null
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server).await;

        let positions = client.get_futures_positions().await.unwrap();

        assert!(positions.is_empty());
        server.verify().await;
    }

    #[test]
    fn futures_contract_deserializes() {
        let json = r#"{
            "id": "c60db22e-536d-43b4-9083-17717de8d217",
            "symbol": "/ESH26:XCME",
            "displaySymbol": "/ESH26",
            "description": "E-mini S&P 500 Mar 2026",
            "multiplier": "50",
            "expiration": "2026-03-21",
            "tradability": "tradable",
            "state": "active"
        }"#;
        let contract: FuturesContract = serde_json::from_str(json).unwrap();
        assert_eq!(
            contract.id.as_deref(),
            Some("c60db22e-536d-43b4-9083-17717de8d217")
        );
        assert_eq!(contract.symbol.as_deref(), Some("/ESH26:XCME"));
        assert_eq!(contract.display_symbol.as_deref(), Some("/ESH26"));
        assert_eq!(contract.multiplier.as_deref(), Some("50"));
    }

    #[test]
    fn futures_quote_accepts_integer_sizes_from_live_wire() {
        let wire = r#"{
          "status":"SUCCESS",
          "data":[{"status":"SUCCESS","data":{
            "ask_price":"7164.25","ask_size":1,
            "ask_venue_timestamp":"2026-04-17T17:00:00.12674-04:00",
            "bid_price":"7163.75","bid_size":5,
            "bid_venue_timestamp":"2026-04-17T17:00:00.12674-04:00",
            "last_trade_price":"7164.25","last_trade_size":1,
            "last_trade_venue_timestamp":"2026-04-17T16:59:58.64959-04:00",
            "symbol":"/ESM26:XCME",
            "instrument_id":"e7b95e72-9aa2-4779-89e1-c404163799ed",
            "state":"active",
            "updated_at":"2026-04-17T17:00:00.12674-04:00",
            "out_of_band":false
          }}]
        }"#;
        let wrapper: FuturesQuoteDataWrapper = serde_json::from_str(wire).unwrap();
        let quote = &wrapper.data[0].data;
        assert_eq!(quote.ask_size, Some(1));
        assert_eq!(quote.bid_size, Some(5));
        assert_eq!(quote.last_trade_size, Some(1));
        assert_eq!(quote.ask_price.as_deref(), Some("7164.25"));
        assert_eq!(quote.bid_price.as_deref(), Some("7163.75"));
        assert_eq!(
            quote.instrument_id.as_deref(),
            Some("e7b95e72-9aa2-4779-89e1-c404163799ed")
        );
        assert_eq!(quote.state.as_deref(), Some("active"));
    }

    #[test]
    fn futures_order_deserializes() {
        let json = r#"{
            "orderId": "order-123",
            "orderState": "FILLED",
            "quantity": "1",
            "filledQuantity": "1",
            "averagePrice": "6903.50",
            "orderLegs": [{"side": "BUY"}],
            "realizedPnl": {"realizedPnl": {"amount": "-50.00", "currency": "USD"}},
            "totalFee": {"amount": "3.10", "currency": "USD"},
            "createdAt": "2026-01-15T10:00:00Z",
            "updatedAt": "2026-01-15T10:01:00Z"
        }"#;
        let order: FuturesOrder = serde_json::from_str(json).unwrap();
        assert_eq!(order.order_id.as_deref(), Some("order-123"));
        assert_eq!(order.order_state.as_deref(), Some("FILLED"));
        assert_eq!(order.filled_quantity.as_deref(), Some("1"));
        assert!(order.realized_pnl.is_some());
        assert!(order.total_fee.is_some());
    }

    #[test]
    fn futures_contract_wrapper_deserializes() {
        let json = r#"{"result": {"id": "abc", "symbol": "/ESH26:XCME"}}"#;
        let wrapper: FuturesContractWrapper = serde_json::from_str(json).unwrap();
        assert_eq!(wrapper.result.id.as_deref(), Some("abc"));
    }

    #[test]
    fn futures_quote_data_wrapper_deserializes() {
        let json = r#"{"data": [{"data": {"bid_price": "100", "ask_price": "101"}}]}"#;
        let wrapper: FuturesQuoteDataWrapper = serde_json::from_str(json).unwrap();
        assert_eq!(wrapper.data.len(), 1);
        assert_eq!(wrapper.data[0].data.bid_price.as_deref(), Some("100"));
    }
}
