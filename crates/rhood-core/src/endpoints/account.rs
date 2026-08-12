use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::models::account::*;
use crate::pagination::ResultsResponse;
use crate::{Result, RhoodError};

pub(crate) fn validate_account_number(account_number: &str) -> Result<()> {
    if account_number.is_empty()
        || !account_number
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return Err(RhoodError::InvalidParameter(
            "account_number must contain only ASCII letters and digits".to_string(),
        ));
    }
    Ok(())
}

impl RobinhoodClient {
    /// Discovers every brokerage account, including IRA accounts.
    ///
    /// The Bonfire response is intentionally modeled as a tolerant descriptor.
    /// Call [`resolve_account`](Self::resolve_account) before an
    /// account-sensitive operation.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_accounts(&self) -> Result<Vec<AccountDescriptor>> {
        let response: ResultsResponse<AccountDescriptor> =
            self.get(&self.bonfire_url(paths::UNIFIED_ACCOUNTS)).await?;
        Ok(response.results)
    }

    /// Resolves an optional canonical account number against fresh discovery.
    ///
    /// When `account_number` is omitted, resolution succeeds only when exactly
    /// one discovered account has a non-empty canonical number. An explicit
    /// number must match exactly one discovered account.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidParameter`], [`RhoodError::NoEligibleAccount`],
    /// [`RhoodError::UnknownAccount`], [`RhoodError::AmbiguousAccount`], or
    /// [`RhoodError::InvalidAccountResponse`] when selection cannot produce one
    /// unambiguous account. Also returns transport and deserialization errors.
    pub async fn resolve_account(&self, account_number: Option<&str>) -> Result<AccountDescriptor> {
        if let Some(requested) = account_number {
            validate_account_number(requested)?;
        }
        let accounts = self.get_accounts().await?;
        let eligible: Vec<AccountDescriptor> = accounts
            .into_iter()
            .filter(|account| account.canonical_account_number().is_some())
            .collect();

        if let Some(requested) = account_number {
            let mut matches = eligible.into_iter().filter(|account| {
                account
                    .canonical_account_number()
                    .is_some_and(|number| number == requested)
            });
            let selected = matches.next().ok_or_else(|| RhoodError::UnknownAccount {
                account: mask_account_number(requested),
            })?;
            if matches.next().is_some() {
                return Err(RhoodError::InvalidAccountResponse(
                    "duplicate canonical account numbers".to_string(),
                ));
            }
            return Ok(selected);
        }

        match eligible.as_slice() {
            [] => Err(RhoodError::NoEligibleAccount),
            [selected] => Ok(selected.clone()),
            _ => Err(RhoodError::AmbiguousAccount {
                choices: eligible
                    .iter()
                    .map(AccountDescriptor::selection_label)
                    .collect::<Vec<_>>()
                    .join(", "),
            }),
        }
    }

    /// Fetches the unified account summary.
    ///
    /// Returns a comprehensive snapshot including buying power, equity,
    /// cash, and margin health from the bonfire API.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidParameter`] if the account number is not a
    /// canonical selector. Also returns an error on HTTP or deserialization
    /// failures.
    pub async fn get_account_summary(&self, account_number: &str) -> Result<AccountSummary> {
        validate_account_number(account_number)?;
        let url = format!(
            "{}/accounts/{}{}",
            self.config().api.bonfire_url,
            account_number,
            paths::ACCOUNT_SUMMARY_SUFFIX
        );
        self.get(&url).await
    }

    /// Fetches the basic account profile information.
    ///
    /// Returns details such as account number, type, and status.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidParameter`] if the account number is not a
    /// canonical selector. Also returns an error on HTTP or deserialization
    /// failures.
    pub async fn get_account_profile(&self, account_number: &str) -> Result<AccountProfile> {
        validate_account_number(account_number)?;
        let url = format!("{}{account_number}/", self.api_url(paths::ACCOUNTS));
        self.get(&url).await
    }

    /// Fetches the portfolio profile containing equity, market value, and
    /// related financial summaries.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidParameter`] if the account number is not a
    /// canonical selector. Also returns an error on HTTP or deserialization
    /// failures.
    pub async fn get_portfolio(&self, account_number: &str) -> Result<PortfolioProfile> {
        validate_account_number(account_number)?;
        let url = format!("{}{account_number}/", self.api_url(paths::PORTFOLIOS));
        self.get(&url).await
    }

    /// Fetches Robinhood's portfolio performance chart rendering model.
    ///
    /// The Bonfire response contains plotted returns and presentation metadata,
    /// not historical equity candles. Robinhood's current replacement endpoint
    /// accepts the account in the path and no legacy interval, span, or bounds
    /// query parameters.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::InvalidParameter`] if the account number is not a
    /// canonical selector. Also returns an error on HTTP or deserialization
    /// failures.
    pub async fn get_portfolio_performance(
        &self,
        account_number: &str,
    ) -> Result<PortfolioPerformance> {
        validate_account_number(account_number)?;
        let url = format!(
            "{}{account_number}",
            self.bonfire_url(paths::PORTFOLIO_PERFORMANCE)
        );
        self.get(&url).await
    }

    /// Fetches all stock positions with a non-zero quantity.
    ///
    /// Excludes positions that have been fully closed (quantity of zero).
    ///
    /// Instrument References are enriched with Ticker Symbols on a best-effort,
    /// fill-only basis. Raw positions are retained when enrichment is incomplete.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_positions(&self, account_number: &str) -> Result<Vec<Position>> {
        validate_account_number(account_number)?;
        let mut positions: Vec<Position> = self
            .get_paginated(
                &self.api_url(paths::POSITIONS),
                &[("account_number", account_number), ("nonzero", "true")],
            )
            .await?;
        self.enrich_instrument_identities("get_positions", &mut positions)
            .await;
        Ok(positions)
    }

    /// Fetches all stock positions, including those with a zero quantity.
    ///
    /// Instrument References are enriched with Ticker Symbols on a best-effort,
    /// fill-only basis. Raw positions are retained when enrichment is incomplete.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_all_positions(&self, account_number: &str) -> Result<Vec<Position>> {
        validate_account_number(account_number)?;
        let mut positions: Vec<Position> = self
            .get_paginated(
                &self.api_url(paths::POSITIONS),
                &[("account_number", account_number)],
            )
            .await?;
        self.enrich_instrument_identities("get_all_positions", &mut positions)
            .await;
        Ok(positions)
    }
}

#[cfg(test)]
mod tests {
    use crate::models::account::{
        AccountProfile, AccountSummary, PortfolioPerformance, PortfolioProfile, Position,
    };

    #[test]
    fn account_profile_deserializes() {
        let json = r#"{
            "account_number": "ABC123",
            "buying_power": "5000.00",
            "cash": "1000.00",
            "type": "margin",
            "created_at": "2025-01-01T00:00:00Z"
        }"#;
        let profile: AccountProfile = serde_json::from_str(json).unwrap();
        assert_eq!(profile.account_number.as_deref(), Some("ABC123"));
        assert_eq!(profile.buying_power.as_deref(), Some("5000.00"));
        assert_eq!(profile.account_type.as_deref(), Some("margin"));
    }

    #[test]
    fn account_summary_deserializes_real_api_shape() {
        let json = r#"{
            "account_buying_power": {"amount": "4987.64", "currency_code": "USD", "currency_id": "1072fc76"},
            "total_equity": {"amount": "2372.89", "currency_code": "USD", "currency_id": "1072fc76"},
            "total_market_value": {"amount": "5", "currency_code": "USD", "currency_id": "1072fc76"},
            "uninvested_cash": {"amount": "2367.89", "currency_code": "USD", "currency_id": "1072fc76"},
            "withdrawable_cash": {"amount": "2367.89", "currency_code": "USD", "currency_id": "1072fc76"},
            "portfolio_equity": {"amount": "2372.89", "currency_code": "USD", "currency_id": "1072fc76"},
            "near_margin_call": false,
            "account_number": "767920911",
            "brokerage_account_type": "individual",
            "has_futures_account": true,
            "margin_health": {
                "margin_health_state": "healthy",
                "margin_buffer": "1.0000",
                "margin_buffer_amount": {"currency_code": "USD", "currency_id": "1072fc76", "amount": "2372.89"}
            }
        }"#;
        let summary: AccountSummary = serde_json::from_str(json).unwrap();
        let buying_power = summary.account_buying_power.unwrap();
        assert_eq!(buying_power.amount.as_deref(), Some("4987.64"));
        assert_eq!(buying_power.currency_code.as_deref(), Some("USD"));
        assert_eq!(summary.account_number.as_deref(), Some("767920911"));
        assert_eq!(summary.near_margin_call, Some(false));
        assert_eq!(summary.has_futures_account, Some(true));
        let margin = summary.margin_health.unwrap();
        assert_eq!(margin.margin_health_state.as_deref(), Some("healthy"));
        assert_eq!(margin.margin_buffer.as_deref(), Some("1.0000"));
        assert!(summary.crypto.is_none());
    }

    #[test]
    fn position_deserializes() {
        let json = r#"{
            "instrument": "https://api.robinhood.com/instruments/abc/",
            "average_buy_price": "150.00",
            "quantity": "10.0000",
            "shares_held_for_sells": "0.0000"
        }"#;
        let pos: Position = serde_json::from_str(json).unwrap();
        assert_eq!(pos.quantity.as_deref(), Some("10.0000"));
        assert_eq!(pos.average_buy_price.as_deref(), Some("150.00"));
        assert_eq!(pos.symbol, None);
    }

    #[test]
    fn position_deserializes_symbol_field() {
        let json = r#"{
            "instrument": "https://api.robinhood.com/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e/",
            "average_buy_price": "175.50",
            "quantity": "5.0000",
            "symbol": "AAPL"
        }"#;
        let pos: Position = serde_json::from_str(json).unwrap();
        assert_eq!(pos.symbol.as_deref(), Some("AAPL"));
        assert_eq!(pos.quantity.as_deref(), Some("5.0000"));
    }

    #[test]
    fn position_symbol_serializes_round_trip() {
        let pos = Position {
            account: None,
            instrument: Some(
                "https://api.robinhood.com/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e/"
                    .to_string(),
            ),
            symbol: Some("TSLA".to_string()),
            average_buy_price: Some("250.00".to_string()),
            clearing_average_cost: None,
            clearing_cost_basis: None,
            quantity: Some("3.0000".to_string()),
            shares_held_for_buys: None,
            shares_held_for_sells: None,
            created_at: None,
            updated_at: None,
        };
        let json = serde_json::to_string(&pos).unwrap();
        let deserialized: Position = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.symbol.as_deref(), Some("TSLA"));
    }

    #[test]
    fn portfolio_profile_deserializes_real_api_shape() {
        let json = r#"{
            "url": "https://api.robinhood.com/portfolios/767920911/",
            "account": "https://api.robinhood.com/accounts/767920911/",
            "start_date": "2023-06-08",
            "market_value": "0.0000",
            "equity": "1036.2900",
            "extended_hours_market_value": "0.0000",
            "extended_hours_equity": "1036.2900",
            "extended_hours_portfolio_equity": "1036.2900",
            "last_core_market_value": "0.0000",
            "last_core_equity": "1036.2900",
            "last_core_portfolio_equity": "1036.2900",
            "excess_margin": "1036.2900",
            "excess_maintenance": "1036.2900",
            "excess_margin_with_uncleared_deposits": "1036.2900",
            "excess_maintenance_with_uncleared_deposits": "1036.2900",
            "equity_previous_close": "1036.2900",
            "portfolio_equity_previous_close": "1036.2900",
            "adjusted_equity_previous_close": "1036.2900",
            "adjusted_portfolio_equity_previous_close": "1036.2900",
            "withdrawable_amount": "1036.29",
            "unwithdrawable_deposits": "0.0000",
            "unwithdrawable_grants": "0.0000",
            "is_primary_account": true,
            "non_usd_currency_equity": "0.0000"
        }"#;
        let portfolio: PortfolioProfile = serde_json::from_str(json).unwrap();
        assert_eq!(portfolio.start_date.as_deref(), Some("2023-06-08"));
        assert_eq!(portfolio.equity.as_deref(), Some("1036.2900"));
        assert_eq!(portfolio.excess_maintenance.as_deref(), Some("1036.2900"));
        assert_eq!(
            portfolio.equity_previous_close.as_deref(),
            Some("1036.2900")
        );
        assert_eq!(portfolio.is_primary_account, Some(true));
        assert_eq!(portfolio.non_usd_currency_equity.as_deref(), Some("0.0000"));
        assert_eq!(
            portfolio.last_core_portfolio_equity.as_deref(),
            Some("1036.2900")
        );
    }

    #[test]
    fn portfolio_performance_preserves_raw_chart_components_and_unknown_fields() {
        // Sanitized from the source-backed chart contract. Values are returns
        // and rendering metadata, not portfolio equity candles.
        let json = serde_json::json!({
            "account_number": "ABC123",
            "lines": [{
                "identifier": "returns",
                "segments": [{"points": [{"x": 1, "y": "0.0125", "label": "+1.25%"}]}]
            }],
            "x_axis": {"labels": ["9:30 AM", "4:00 PM"]},
            "y_axis": {"format": "percent"},
            "legend_data": {"primary": "Portfolio"},
            "fills": [{"identifier": "positive"}],
            "overlays": [{"identifier": "market_hours"}],
            "future_chart_component": {"retained": true}
        });

        let performance: PortfolioPerformance = serde_json::from_value(json.clone()).unwrap();

        assert_eq!(performance.account_number.as_deref(), Some("ABC123"));
        assert_eq!(
            performance.lines.as_ref().unwrap()[0]["identifier"],
            "returns"
        );
        assert_eq!(
            performance.additional_fields["future_chart_component"],
            json["future_chart_component"]
        );
        assert_eq!(serde_json::to_value(performance).unwrap(), json);
    }

    #[test]
    fn portfolio_performance_known_components_are_optional() {
        let performance: PortfolioPerformance = serde_json::from_str("{}").unwrap();

        assert!(performance.account_number.is_none());
        assert!(performance.lines.is_none());
        assert!(performance.x_axis.is_none());
        assert!(performance.y_axis.is_none());
        assert!(performance.legend_data.is_none());
        assert!(performance.fills.is_none());
        assert!(performance.overlays.is_none());
        assert!(performance.additional_fields.is_empty());
    }

    #[test]
    fn account_profile_deserializes_with_margin_balances() {
        let json = r#"{
            "account_number": "767920911",
            "type": "margin",
            "state": "active",
            "buying_power": "2493.8200",
            "cash": "2367.8900",
            "drip_enabled": true,
            "has_futures_account": true,
            "margin_balances": {
                "cash": "2367.8900",
                "day_trade_buying_power": "2493.8200",
                "overnight_buying_power": "2493.8200",
                "leverage_enabled": true,
                "day_trades_protection": true,
                "is_primary_account": true,
                "is_pdt_forever": false
            },
            "instant_eligibility": {
                "state": "ok",
                "reason": "",
                "additional_deposit_needed": "0.0000",
                "created_at": "2023-06-08T18:08:00.938554Z"
            }
        }"#;
        let account: AccountProfile = serde_json::from_str(json).unwrap();
        assert_eq!(account.account_number.as_deref(), Some("767920911"));
        assert_eq!(account.account_type.as_deref(), Some("margin"));
        assert_eq!(account.state.as_deref(), Some("active"));
        assert_eq!(account.drip_enabled, Some(true));
        assert_eq!(account.has_futures_account, Some(true));
        let margin = account.margin_balances.unwrap();
        assert_eq!(margin.cash.as_deref(), Some("2367.8900"));
        assert_eq!(margin.day_trade_buying_power.as_deref(), Some("2493.8200"));
        assert_eq!(margin.leverage_enabled, Some(true));
        assert_eq!(margin.is_pdt_forever, Some(false));
        let eligibility = account.instant_eligibility.unwrap();
        assert_eq!(eligibility.state.as_deref(), Some("ok"));
    }
}

#[cfg(test)]
mod endpoint_tests {
    use crate::RhoodError;
    use crate::client::RobinhoodClient;
    use crate::config::RhoodConfig;
    use secrecy::SecretString;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const INSTRUMENT_ID: &str = "450dfc6d-5510-4d40-abfb-f633b7d9be3e";

    async fn client_for_server(server: &MockServer) -> (tempfile::TempDir, RobinhoodClient) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = RhoodConfig::default();
        config.auth.token_cache_path = dir
            .path()
            .join("nonexistent-token.json")
            .to_string_lossy()
            .into_owned();
        config.api.base_url = server.uri();
        config.api.phoenix_url = server.uri();
        config.api.bonfire_url = server.uri();
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

    async fn mount_accounts(server: &MockServer, accounts: serde_json::Value) {
        Mock::given(method("GET"))
            .and(path("/accounts/unified/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": accounts
            })))
            .expect(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn get_accounts_uses_bonfire_discovery_and_preserves_unknown_fields() {
        let server = MockServer::start().await;
        mount_accounts(
            &server,
            serde_json::json!([{
                "account_number": "ABC123",
                "rhs_account_number": 987,
                "brokerage_account_type": "individual",
                "equities": {"rhs_account_number": "equity-route"},
                "future_field": "retained"
            }]),
        )
        .await;
        let (_dir, client) = client_for_server(&server).await;

        let accounts = client.get_accounts().await.unwrap();

        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].canonical_account_number(), Some("ABC123"));
        assert_eq!(
            accounts[0].additional_fields.get("future_field"),
            Some(&serde_json::json!("retained"))
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn resolve_account_auto_selects_only_account_and_matches_explicit_account() {
        for requested in [None, Some("IRA456")] {
            let server = MockServer::start().await;
            mount_accounts(
                &server,
                serde_json::json!([{
                    "account_number": "IRA456",
                    "brokerage_account_type": "ira_roth"
                }]),
            )
            .await;
            let (_dir, client) = client_for_server(&server).await;

            let selected = client.resolve_account(requested).await.unwrap();

            assert_eq!(selected.canonical_account_number(), Some("IRA456"));
            server.verify().await;
        }
    }

    #[tokio::test]
    async fn resolve_account_fails_closed_for_none_unknown_multiple_and_duplicates() {
        let cases = [
            (
                serde_json::json!([]),
                None,
                "no eligible brokerage account was found",
            ),
            (
                serde_json::json!([{"account_number": "ABC123"}]),
                Some("ZZZ999"),
                "unknown brokerage account",
            ),
            (
                serde_json::json!([
                    {"account_number": "ABC123", "brokerage_account_type": "individual"},
                    {"account_number": "IRA456", "brokerage_account_type": "ira_roth"}
                ]),
                None,
                "multiple brokerage accounts",
            ),
            (
                serde_json::json!([
                    {"account_number": "ABC123"},
                    {"account_number": "ABC123"}
                ]),
                Some("ABC123"),
                "invalid unified account response: duplicate canonical account numbers",
            ),
        ];

        for (accounts, requested, expected) in cases {
            let server = MockServer::start().await;
            mount_accounts(&server, accounts).await;
            let (_dir, client) = client_for_server(&server).await;

            let error = client.resolve_account(requested).await.unwrap_err();

            assert!(
                error.to_string().to_ascii_lowercase().contains(expected),
                "got: {error}"
            );
            if let RhoodError::UnknownAccount { account } = error {
                assert_eq!(account, "****Z999");
            }
            server.verify().await;
        }
    }

    #[tokio::test]
    async fn account_reads_use_selected_paths_and_position_filters() {
        let server = MockServer::start().await;
        for endpoint in [
            "/accounts/ABC123/",
            "/portfolios/ABC123/",
            "/accounts/ABC123/unified/",
            "/portfolio/performance/ABC123",
        ] {
            Mock::given(method("GET"))
                .and(path(endpoint))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
                .expect(1)
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/positions/"))
            .and(query_param("account_number", "ABC123"))
            .and(query_param("nonzero", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [],
                "next": null
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server).await;

        client.get_account_profile("ABC123").await.unwrap();
        client.get_portfolio("ABC123").await.unwrap();
        client.get_portfolio_performance("ABC123").await.unwrap();
        client.get_account_summary("ABC123").await.unwrap();
        assert!(client.get_positions("ABC123").await.unwrap().is_empty());
        server.verify().await;
    }

    #[tokio::test]
    async fn get_positions_enriches_duplicates_in_one_resolution_pass() {
        let server = MockServer::start().await;
        let reference = format!("https://api.robinhood.com/instruments/{INSTRUMENT_ID}/");
        Mock::given(method("GET"))
            .and(path("/positions/"))
            .and(query_param("account_number", "ABC123"))
            .and(query_param("nonzero", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [
                    {"instrument": reference.clone()},
                    {"instrument": INSTRUMENT_ID},
                    {"instrument": reference, "symbol": "WIRE"}
                ],
                "next": null
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .and(query_param("ids", INSTRUMENT_ID))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": INSTRUMENT_ID, "symbol": "AAPL"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server).await;

        let positions = client.get_positions("ABC123").await.unwrap();

        assert_eq!(positions[0].symbol.as_deref(), Some("AAPL"));
        assert_eq!(positions[1].symbol.as_deref(), Some("AAPL"));
        assert_eq!(positions[2].symbol.as_deref(), Some("WIRE"));
        server.verify().await;
    }

    #[test]
    fn account_number_rejects_path_and_query_injection() {
        for invalid in ["", "ABC-123", "../ABC123", "ABC123?all=true"] {
            let error = super::validate_account_number(invalid).unwrap_err();
            assert!(matches!(error, RhoodError::InvalidParameter(_)));
        }
    }

    #[tokio::test]
    async fn portfolio_performance_rejects_invalid_account_before_http() {
        let server = MockServer::start().await;
        let (_dir, client) = client_for_server(&server).await;

        let error = client
            .get_portfolio_performance("ABC123?span=year")
            .await
            .unwrap_err();

        assert!(matches!(error, RhoodError::InvalidParameter(_)));
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}
