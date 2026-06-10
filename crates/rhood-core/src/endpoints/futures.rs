use std::sync::Arc;

use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::models::futures::*;
use crate::pagination::ResultsResponse;
use crate::{Result, RhoodError};

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
        wrapper
            .data
            .into_iter()
            .next()
            .map(|item| item.data)
            .ok_or_else(|| RhoodError::InvalidSymbol(symbol.to_string()))
    }

    /// Fetches real-time futures quotes for multiple symbols in a single request.
    ///
    /// Resolves each symbol to its instrument ID concurrently via the resolver
    /// cache, then batches the quote request. Cache hits return immediately;
    /// misses fan out via [`futures::future::try_join_all`] so N upstream
    /// contract lookups run in parallel rather than sequentially.
    ///
    /// # Errors
    ///
    /// Returns an error if any symbol cannot be resolved or on HTTP failures.
    pub async fn get_futures_quotes(&self, symbols: &[&str]) -> Result<Vec<FuturesQuote>> {
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
        Ok(wrapper.data.into_iter().map(|item| item.data).collect())
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
    /// `Ok(None)` when the user has no futures account — matching the error
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
    use crate::models::futures::{
        FuturesContract, FuturesContractWrapper, FuturesOrder, FuturesQuoteDataWrapper,
    };

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
