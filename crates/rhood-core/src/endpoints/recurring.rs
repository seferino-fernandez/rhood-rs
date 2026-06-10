use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::models::recurring::{
    CreateRecurringAssetPayload, CreateRecurringPayload, CreateRecurringRequest, MoneyAmount,
    NextInvestmentDate, RecurringFrequency, RecurringInvestment, RecurringState,
    UpdateRecurringPayload, UpdateRecurringRequest,
};
use crate::{Result, RhoodError};

impl RobinhoodClient {
    /// Fetches all recurring investment schedules.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_recurring_investments(&self) -> Result<Vec<RecurringInvestment>> {
        self.get_paginated(&self.bonfire_url(paths::RECURRING_SCHEDULES), &[])
            .await
    }

    /// Creates a new recurring investment schedule.
    ///
    /// Resolves the symbol to its instrument ID and discovers the account
    /// number before submitting. Requires writable mode.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::ReadOnlyMode`] if the client is in read-only mode.
    /// Returns [`RhoodError::InvalidSymbol`] if the symbol cannot be resolved.
    /// Also returns an error on HTTP or deserialization failures.
    pub async fn create_recurring_investment(
        &self,
        create_recurring_request: &CreateRecurringRequest,
    ) -> Result<RecurringInvestment> {
        self.require_writable()?;

        let instrument = self
            .cached_instrument(&create_recurring_request.symbol)
            .await?
            .ok_or_else(|| RhoodError::InvalidSymbol(create_recurring_request.symbol.clone()))?;
        let instrument_id = instrument
            .id
            .clone()
            .ok_or_else(|| RhoodError::InvalidSymbol(create_recurring_request.symbol.clone()))?;

        let profile = self.get_account_profile().await?;
        let account_number = profile.account_number.ok_or(RhoodError::NotAuthenticated)?;

        let payload = CreateRecurringPayload {
            account_number: account_number.clone(),
            amount: MoneyAmount {
                amount: format!("{:.2}", create_recurring_request.amount),
                currency_code: "USD".to_string(),
            },
            frequency: create_recurring_request.frequency.to_string(),
            start_date: create_recurring_request.start_date.clone(),
            investment_asset: CreateRecurringAssetPayload {
                asset_id: instrument_id,
                asset_symbol: create_recurring_request.symbol.to_uppercase(),
                asset_type: "equity".to_string(),
            },
            source_of_funds: create_recurring_request.source_of_funds.to_string(),
            ref_id: uuid::Uuid::new_v4().to_string(),
            is_backup_ach_enabled: false,
        };

        let url = format!(
            "{}?account_number={}",
            self.bonfire_url(paths::RECURRING_SCHEDULES),
            account_number
        );
        self.post_json(&url, &payload).await
    }

    /// Updates an existing recurring investment schedule.
    ///
    /// Can change amount, frequency, state (pause/resume), or start date.
    /// Requires writable mode.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::ReadOnlyMode`] if the client is in read-only mode.
    /// Also returns an error on HTTP or deserialization failures.
    pub async fn update_recurring_investment(
        &self,
        schedule_id: &str,
        req: &UpdateRecurringRequest,
    ) -> Result<RecurringInvestment> {
        self.require_writable()?;

        let payload = UpdateRecurringPayload {
            amount: req.amount.map(|amount| MoneyAmount {
                amount: format!("{amount:.2}"),
                currency_code: "USD".to_string(),
            }),
            frequency: req.frequency.map(|frequency| frequency.to_string()),
            state: req.state.map(|state| state.to_string()),
            start_date: req.start_date.clone(),
        };

        let url = format!(
            "{}{schedule_id}/",
            self.bonfire_url(paths::RECURRING_SCHEDULES)
        );
        self.patch_json(&url, &payload).await
    }

    /// Cancels a recurring investment schedule by setting its state to "deleted".
    ///
    /// Requires writable mode.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::ReadOnlyMode`] if the client is in read-only mode.
    /// Also returns an error on HTTP or deserialization failures.
    pub async fn cancel_recurring_investment(
        &self,
        schedule_id: &str,
    ) -> Result<RecurringInvestment> {
        let cancel_req = UpdateRecurringRequest {
            amount: None,
            frequency: None,
            state: Some(RecurringState::Deleted),
            start_date: None,
        };
        self.update_recurring_investment(schedule_id, &cancel_req)
            .await
    }

    /// Looks up the next scheduled investment date for a given frequency
    /// and start date.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_next_investment_date(
        &self,
        frequency: RecurringFrequency,
        start_date: &str,
    ) -> Result<NextInvestmentDate> {
        let url = format!(
            "{}equity/next_investment_date/",
            self.bonfire_url(paths::RECURRING_SCHEDULES)
        );
        let frequency_string = frequency.to_string();
        self.get_with_params(
            &url,
            &[
                ("frequency", frequency_string.as_str()),
                ("start_date", start_date),
            ],
        )
        .await
    }
}

// Network-exercising tests for the endpoint methods. They build a client
// pointed at a wiremock server and use the crate-internal `inject_test_auth`
// (compiled under `cfg(test)`) to get past `require_auth`. These run on every
// `cargo test` / `cargo nextest run` and count toward coverage.
#[cfg(test)]
mod endpoint_tests {
    use crate::client::RobinhoodClient;
    use crate::config::RhoodConfig;
    use crate::models::recurring::{
        CreateRecurringRequest, RecurringFrequency, RecurringInvestment, RecurringSource,
        UpdateRecurringRequest,
    };
    use crate::{Result, RhoodError};
    use secrecy::SecretString;
    use wiremock::matchers::{body_string_contains, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Builds a client whose every base URL points at `base_url`, with the
    /// given read-only setting and an injected authenticated session.
    async fn client_for_server(
        base_url: &str,
        read_only: bool,
    ) -> (tempfile::TempDir, RobinhoodClient) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = RhoodConfig::default();
        config.auth.token_cache_path = dir
            .path()
            .join("nonexistent-token.json")
            .to_str()
            .unwrap()
            .to_string();
        config.read_only = read_only;
        config.api.base_url = base_url.to_string();
        config.api.phoenix_url = base_url.to_string();
        config.api.bonfire_url = base_url.to_string();
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

    /// A read-only client never reaches the network; its base URL is unused.
    async fn read_only_client() -> (tempfile::TempDir, RobinhoodClient) {
        client_for_server("https://unused.invalid", true).await
    }

    fn sample_create_request() -> CreateRecurringRequest {
        CreateRecurringRequest {
            symbol: "tsla".to_string(),
            amount: 10.0,
            frequency: RecurringFrequency::Weekly,
            start_date: "2026-04-07".to_string(),
            source_of_funds: RecurringSource::BuyingPower,
        }
    }

    #[tokio::test]
    async fn get_recurring_investments_returns_all_schedules() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/recurring_schedules/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [
                    {"id": "sched-001", "frequency": "weekly", "state": "active"},
                    {"id": "sched-002", "frequency": "monthly", "state": "paused"}
                ],
                "next": null,
                "previous": null
            })))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri(), true).await;

        let schedules = client.get_recurring_investments().await.unwrap();

        assert_eq!(schedules.len(), 2);
        assert_eq!(schedules[0].id.as_deref(), Some("sched-001"));
        assert_eq!(schedules[1].state.as_deref(), Some("paused"));
    }

    #[tokio::test]
    async fn create_recurring_investment_blocked_in_read_only_mode() {
        let (_dir, client) = read_only_client().await;
        let err = client
            .create_recurring_investment(&sample_create_request())
            .await
            .unwrap_err();
        assert!(matches!(err, RhoodError::ReadOnlyMode));
    }

    #[tokio::test]
    async fn create_recurring_investment_happy_path_posts_expected_payload() {
        let server = MockServer::start().await;
        // Symbol resolution.
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .and(query_param("symbol", "TSLA"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": "inst-tsla", "symbol": "TSLA"}],
                "next": null
            })))
            .mount(&server)
            .await;
        // Account discovery.
        Mock::given(method("GET"))
            .and(path("/accounts/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"account_number": "ACC-123"}],
                "next": null
            })))
            .mount(&server)
            .await;
        // The actual create call. Assert the derived payload fields.
        Mock::given(method("POST"))
            .and(path("/recurring_schedules/"))
            .and(query_param("account_number", "ACC-123"))
            .and(body_string_contains("\"amount\":\"10.00\""))
            .and(body_string_contains("\"frequency\":\"weekly\""))
            .and(body_string_contains("\"asset_symbol\":\"TSLA\""))
            .and(body_string_contains("\"asset_type\":\"equity\""))
            .and(body_string_contains("\"source_of_funds\":\"buying_power\""))
            .and(body_string_contains("\"is_backup_ach_enabled\":false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "sched-new",
                "account_number": "ACC-123",
                "frequency": "weekly",
                "state": "active"
            })))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri(), false).await;

        let created = client
            .create_recurring_investment(&sample_create_request())
            .await
            .unwrap();

        assert_eq!(created.id.as_deref(), Some("sched-new"));
        assert_eq!(created.account_number.as_deref(), Some("ACC-123"));
    }

    #[tokio::test]
    async fn create_recurring_investment_unknown_symbol_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [],
                "next": null
            })))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri(), false).await;

        let err = client
            .create_recurring_investment(&sample_create_request())
            .await
            .unwrap_err();

        assert!(matches!(err, RhoodError::InvalidSymbol(symbol) if symbol == "tsla"));
    }

    #[tokio::test]
    async fn create_recurring_investment_instrument_without_id_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"symbol": "TSLA"}],
                "next": null
            })))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri(), false).await;

        let err = client
            .create_recurring_investment(&sample_create_request())
            .await
            .unwrap_err();

        assert!(matches!(err, RhoodError::InvalidSymbol(_)));
    }

    #[tokio::test]
    async fn create_recurring_investment_missing_account_number_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": "inst-tsla", "symbol": "TSLA"}],
                "next": null
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/accounts/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"buying_power": "100.00"}],
                "next": null
            })))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri(), false).await;

        let err = client
            .create_recurring_investment(&sample_create_request())
            .await
            .unwrap_err();

        assert!(matches!(err, RhoodError::NotAuthenticated));
    }

    #[tokio::test]
    async fn update_recurring_investment_blocked_in_read_only_mode() {
        let (_dir, client) = read_only_client().await;
        let req = UpdateRecurringRequest {
            amount: Some(15.0),
            frequency: None,
            state: None,
            start_date: None,
        };
        let err = client
            .update_recurring_investment("sched-001", &req)
            .await
            .unwrap_err();
        assert!(matches!(err, RhoodError::ReadOnlyMode));
    }

    #[tokio::test]
    async fn update_recurring_investment_patches_only_set_fields() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/recurring_schedules/sched-001/"))
            .and(body_string_contains("\"amount\":{\"amount\":\"20.00\""))
            .and(body_string_contains("\"frequency\":\"biweekly\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "sched-001",
                "frequency": "biweekly",
                "state": "active"
            })))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri(), false).await;

        let req = UpdateRecurringRequest {
            amount: Some(20.0),
            frequency: Some(RecurringFrequency::Biweekly),
            state: None,
            start_date: None,
        };
        let updated = client
            .update_recurring_investment("sched-001", &req)
            .await
            .unwrap();

        assert_eq!(updated.frequency.as_deref(), Some("biweekly"));
    }

    #[tokio::test]
    async fn update_recurring_investment_omits_unset_fields() {
        let server = MockServer::start().await;
        // An empty update serializes to `{}` because every field skips when None.
        Mock::given(method("PATCH"))
            .and(path("/recurring_schedules/sched-001/"))
            .and(body_string_contains("{}"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "sched-001"
            })))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri(), false).await;

        let req = UpdateRecurringRequest {
            amount: None,
            frequency: None,
            state: None,
            start_date: None,
        };
        let updated = client
            .update_recurring_investment("sched-001", &req)
            .await
            .unwrap();

        assert_eq!(updated.id.as_deref(), Some("sched-001"));
    }

    #[tokio::test]
    async fn cancel_recurring_investment_sends_deleted_state() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/recurring_schedules/sched-001/"))
            .and(body_string_contains("\"state\":\"deleted\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "sched-001",
                "state": "deleted"
            })))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri(), false).await;

        let cancelled = client
            .cancel_recurring_investment("sched-001")
            .await
            .unwrap();

        assert_eq!(cancelled.state.as_deref(), Some("deleted"));
    }

    #[tokio::test]
    async fn cancel_recurring_investment_blocked_in_read_only_mode() {
        let (_dir, client) = read_only_client().await;
        let err = client
            .cancel_recurring_investment("sched-001")
            .await
            .unwrap_err();
        assert!(matches!(err, RhoodError::ReadOnlyMode));
    }

    #[tokio::test]
    async fn get_next_investment_date_sends_frequency_and_start_date() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/recurring_schedules/equity/next_investment_date/"))
            .and(query_param("frequency", "monthly"))
            .and(query_param("start_date", "2026-06-01"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "frequency": "monthly",
                "next_investment_date": "2026-06-01",
                "start_date": "2026-06-01"
            })))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri(), true).await;

        let next = client
            .get_next_investment_date(RecurringFrequency::Monthly, "2026-06-01")
            .await
            .unwrap();

        assert_eq!(next.next_investment_date.as_deref(), Some("2026-06-01"));
        assert_eq!(next.frequency.as_deref(), Some("monthly"));
    }

    #[tokio::test]
    async fn get_recurring_investments_propagates_server_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/recurring_schedules/"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri(), true).await;

        let result: Result<Vec<RecurringInvestment>> = client.get_recurring_investments().await;

        assert!(result.is_err());
    }
}

#[cfg(test)]
mod tests {
    use crate::models::recurring::{MoneyAmount, RecurringInvestment};

    #[test]
    fn recurring_investment_deserializes_full() {
        let json = r#"{
            "id": "sched-001",
            "account_number": "ABC123",
            "amount": {"amount": "10.00", "currency_code": "USD"},
            "frequency": "weekly",
            "start_date": "2026-04-07",
            "state": "active",
            "investment_asset": {
                "asset_id": "inst-001",
                "asset_symbol": "TSLA",
                "asset_type": "equity"
            },
            "created_at": "2026-04-01T00:00:00Z",
            "updated_at": "2026-04-01T00:00:00Z"
        }"#;
        let recurring: RecurringInvestment = serde_json::from_str(json).unwrap();
        assert_eq!(recurring.id.as_deref(), Some("sched-001"));
        assert_eq!(recurring.frequency.as_deref(), Some("weekly"));
        assert_eq!(recurring.state.as_deref(), Some("active"));
        let amount = recurring.amount.unwrap();
        assert_eq!(amount.amount, "10.00");
        assert_eq!(amount.currency_code, "USD");
        let asset = recurring.investment_asset.unwrap();
        assert_eq!(asset.asset_symbol.as_deref(), Some("TSLA"));
    }

    #[test]
    fn money_amount_serializes_round_trip() {
        let money = MoneyAmount {
            amount: "25.50".to_string(),
            currency_code: "USD".to_string(),
        };
        let json = serde_json::to_string(&money).unwrap();
        let round_tripped: MoneyAmount = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped.amount, "25.50");
        assert_eq!(round_tripped.currency_code, "USD");
    }
}
