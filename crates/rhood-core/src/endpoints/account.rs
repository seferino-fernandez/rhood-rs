use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::models::account::*;
use crate::pagination::ResultsResponse;
use crate::{Result, RhoodError};

impl RobinhoodClient {
    /// Fetches the unified account summary.
    ///
    /// Returns a comprehensive snapshot including buying power, equity,
    /// cash, and margin health from the bonfire API.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::NotAuthenticated`] if the account number
    /// cannot be determined. Also returns an error on HTTP or
    /// deserialization failures.
    pub async fn get_account_summary(&self) -> Result<AccountSummary> {
        let profile = self.get_account_profile().await?;
        let account_number = profile.account_number.ok_or(RhoodError::NotAuthenticated)?;
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
    /// Returns [`RhoodError::NotAuthenticated`] if no account is found.
    /// Also returns an error on HTTP or deserialization failures.
    pub async fn get_account_profile(&self) -> Result<AccountProfile> {
        let resp: ResultsResponse<AccountProfile> = self
            .get_with_params(
                &self.api_url(paths::ACCOUNTS),
                &[("default_to_all_accounts", "true")],
            )
            .await?;
        resp.results
            .into_iter()
            .next()
            .ok_or(RhoodError::NotAuthenticated)
    }

    /// Fetches the portfolio profile containing equity, market value, and
    /// related financial summaries.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::NotAuthenticated`] if no portfolio is found.
    /// Also returns an error on HTTP or deserialization failures.
    pub async fn get_portfolio(&self) -> Result<PortfolioProfile> {
        let resp: ResultsResponse<PortfolioProfile> =
            self.get(&self.api_url(paths::PORTFOLIOS)).await?;
        resp.results
            .into_iter()
            .next()
            .ok_or(RhoodError::NotAuthenticated)
    }

    /// Fetches all stock positions with a non-zero quantity.
    ///
    /// Excludes positions that have been fully closed (quantity of zero).
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_positions(&self) -> Result<Vec<Position>> {
        let mut positions: Vec<Position> = self
            .get_paginated(&self.api_url(paths::POSITIONS), &[("nonzero", "true")])
            .await?;
        let _ = self.enrich_position_symbols(&mut positions).await;
        Ok(positions)
    }

    /// Fetches all stock positions, including those with a zero quantity.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_all_positions(&self) -> Result<Vec<Position>> {
        let mut positions: Vec<Position> = self
            .get_paginated(&self.api_url(paths::POSITIONS), &[])
            .await?;
        let _ = self.enrich_position_symbols(&mut positions).await;
        Ok(positions)
    }

    /// Backfills `symbol` on each position by resolving its instrument URL to a
    /// ticker via a single batched `/instruments/?ids=` request. Best-effort:
    /// positions whose URL can't be parsed or resolved are left with `symbol = None`.
    pub async fn enrich_position_symbols(&self, positions: &mut [Position]) -> Result<()> {
        let uuids: Vec<String> = positions
            .iter()
            .filter(|p| p.symbol.is_none())
            .filter_map(|p| p.instrument.as_deref())
            .filter_map(|url| crate::util::instrument_id_from_url(url))
            .map(|id| id.to_string())
            .collect();
        if uuids.is_empty() {
            return Ok(());
        }
        let map = self.resolve_symbols(&uuids).await?;
        for p in positions.iter_mut() {
            if p.symbol.is_none()
                && let Some(url) = p.instrument.as_deref()
                && let Some(id) = crate::util::instrument_id_from_url(url)
                && let Some(sym) = map.get(id)
            {
                p.symbol = Some(sym.clone());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::models::account::{AccountProfile, AccountSummary, PortfolioProfile, Position};

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
    fn enrich_position_symbols_applies_map() {
        // Test the pure mapping logic: given a position with an instrument URL
        // and a resolved map, symbol should be set after the apply step.
        let uuid = "450dfc6d-5510-4d40-abfb-f633b7d9be3e";
        let url = format!("https://api.robinhood.com/instruments/{uuid}/");
        let mut pos = Position {
            account: None,
            instrument: Some(url.clone()),
            symbol: None,
            average_buy_price: Some("100.00".to_string()),
            quantity: Some("1.0000".to_string()),
            shares_held_for_buys: None,
            shares_held_for_sells: None,
            created_at: None,
            updated_at: None,
        };

        // Simulate what enrich_position_symbols does after calling resolve_symbols
        let mut map = std::collections::HashMap::new();
        map.insert(uuid.to_string(), "AAPL".to_string());

        let positions: &mut [Position] = std::slice::from_mut(&mut pos);
        for p in positions.iter_mut() {
            if p.symbol.is_none()
                && let Some(instrument_url) = p.instrument.as_deref()
                && let Some(id) = crate::util::instrument_id_from_url(instrument_url)
                && let Some(sym) = map.get(id)
            {
                p.symbol = Some(sym.clone());
            }
        }

        assert_eq!(pos.symbol.as_deref(), Some("AAPL"));
    }

    #[test]
    fn enrich_position_symbols_skips_already_set() {
        let uuid = "450dfc6d-5510-4d40-abfb-f633b7d9be3e";
        let url = format!("https://api.robinhood.com/instruments/{uuid}/");
        let mut pos = Position {
            account: None,
            instrument: Some(url),
            symbol: Some("EXISTING".to_string()),
            average_buy_price: None,
            quantity: None,
            shares_held_for_buys: None,
            shares_held_for_sells: None,
            created_at: None,
            updated_at: None,
        };

        let mut map = std::collections::HashMap::new();
        map.insert(uuid.to_string(), "REPLACED".to_string());

        // Only apply if symbol is None (mirrors enrich logic)
        let positions: &mut [Position] = std::slice::from_mut(&mut pos);
        for p in positions.iter_mut() {
            if p.symbol.is_none()
                && let Some(instrument_url) = p.instrument.as_deref()
                && let Some(id) = crate::util::instrument_id_from_url(instrument_url)
                && let Some(sym) = map.get(id)
            {
                p.symbol = Some(sym.clone());
            }
        }

        // Should not be replaced because symbol was already set
        assert_eq!(pos.symbol.as_deref(), Some("EXISTING"));
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
