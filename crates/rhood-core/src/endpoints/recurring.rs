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
