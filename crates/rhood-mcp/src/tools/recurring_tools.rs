use rhood_core::models::recurring::{RecurringFrequency, RecurringSource, RecurringState};
use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::handler::{AccountSelectorParams, RhoodTools};
use super::types::format_tool_error;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateRecurringInvestmentParams {
    /// Canonical brokerage account number from get_accounts. May be omitted only when exactly one account exists.
    account_number: Option<String>,
    /// Stock symbol (e.g. "TSLA")
    #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
    symbol: String,
    /// Dollar amount per recurrence
    #[schemars(range(min = 0.0))]
    amount: f64,
    /// Recurrence frequency
    frequency: RecurringFrequency,
    /// Start date in YYYY-MM-DD format
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    start_date: String,
    /// Source of funds (default: buying_power)
    #[serde(default)]
    source_of_funds: RecurringSource,
}

/// States an MCP caller may set on `update_recurring_investment`.
///
/// Deliberately excludes `RecurringState::Deleted` - cancellation goes through
/// the dedicated `cancel_recurring_investment` tool, so `"deleted"` is not a
/// valid input here and is rejected with a structured `-32602` error.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum UpdatableRecurringState {
    /// Resume the schedule.
    Active,
    /// Pause the schedule (it persists but executes no investments).
    Paused,
}

impl From<UpdatableRecurringState> for RecurringState {
    fn from(state: UpdatableRecurringState) -> Self {
        match state {
            UpdatableRecurringState::Active => RecurringState::Active,
            UpdatableRecurringState::Paused => RecurringState::Paused,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UpdateRecurringInvestmentParams {
    /// Schedule ID to update - call get_recurring_investments first to discover valid IDs.
    schedule_id: String,
    /// New dollar amount (optional)
    #[schemars(range(min = 0.0))]
    amount: Option<f64>,
    /// New frequency (optional)
    frequency: Option<RecurringFrequency>,
    /// New state (optional)
    state: Option<UpdatableRecurringState>,
    /// New start date (optional)
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    start_date: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CancelRecurringInvestmentParams {
    /// Schedule ID to cancel - call get_recurring_investments first to discover valid IDs.
    schedule_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct NextInvestmentDateParams {
    /// Recurrence frequency
    frequency: RecurringFrequency,
    /// Start date in YYYY-MM-DD format
    #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
    start_date: String,
}

#[tool_router(router = recurring_router, vis = "pub(super)")]
impl RhoodTools {
    #[tool(
        name = "get_recurring_investments",
        description = "List all recurring investment schedules",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_recurring_investments(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<AccountSelectorParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let account =
            Self::resolve_account_number(&client, params.account_number.as_deref()).await?;
        let investments = client
            .get_recurring_investments(&account)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&investments).map_err(|error| error.to_string())
    }

    #[tool(
        name = "create_recurring_investment",
        description = "Create a new recurring investment schedule for a stock symbol",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn create_recurring_investment(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<CreateRecurringInvestmentParams>,
    ) -> Result<String, String> {
        use rhood_core::models::recurring::CreateRecurringRequest;

        let client = self.ensure_client(&peer).await?;
        let account =
            Self::resolve_account_number(&client, params.account_number.as_deref()).await?;
        let req = CreateRecurringRequest {
            account_number: account,
            symbol: params.symbol,
            amount: params.amount,
            frequency: params.frequency,
            start_date: params.start_date,
            source_of_funds: params.source_of_funds,
        };
        let result = client
            .create_recurring_investment(&req)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())
    }

    #[tool(
        name = "update_recurring_investment",
        description = "Update an existing recurring investment (amount, frequency, state, start date)",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn update_recurring_investment(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<UpdateRecurringInvestmentParams>,
    ) -> Result<String, String> {
        use rhood_core::models::recurring::UpdateRecurringRequest;

        let client = self.ensure_client(&peer).await?;
        let req = UpdateRecurringRequest {
            amount: params.amount,
            frequency: params.frequency,
            state: params.state.map(Into::into),
            start_date: params.start_date,
        };
        let result = client
            .update_recurring_investment(&params.schedule_id, &req)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())
    }

    #[tool(
        name = "cancel_recurring_investment",
        description = "Cancel a recurring investment schedule",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn cancel_recurring_investment(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<CancelRecurringInvestmentParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let result = client
            .cancel_recurring_investment(&params.schedule_id)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())
    }

    #[tool(
        name = "get_next_investment_date",
        description = "Look up the next scheduled investment date for a given frequency and start date",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_next_investment_date(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<NextInvestmentDateParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let result = client
            .get_next_investment_date(params.frequency, &params.start_date)
            .await
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CancelRecurringInvestmentParams, UpdatableRecurringState, UpdateRecurringInvestmentParams,
    };

    #[test]
    fn update_recurring_state_rejects_deleted() {
        let err = serde_json::from_str::<UpdateRecurringInvestmentParams>(
            r#"{"schedule_id":"s1","state":"deleted"}"#,
        );
        assert!(
            err.is_err(),
            "state \"deleted\" must be rejected on the MCP surface"
        );
    }

    #[test]
    fn update_recurring_state_accepts_active_and_paused() {
        let active: UpdateRecurringInvestmentParams =
            serde_json::from_str(r#"{"schedule_id":"s1","state":"active"}"#).unwrap();
        assert!(matches!(
            active.state,
            Some(UpdatableRecurringState::Active)
        ));
        let paused: UpdateRecurringInvestmentParams =
            serde_json::from_str(r#"{"schedule_id":"s1","state":"paused"}"#).unwrap();
        assert!(matches!(
            paused.state,
            Some(UpdatableRecurringState::Paused)
        ));
    }

    #[test]
    fn recurring_id_schema_descriptions_name_discovery_tool() {
        for (schema, expected_description) in [
            (
                schemars::schema_for!(UpdateRecurringInvestmentParams),
                "Schedule ID to update - call get_recurring_investments first to discover valid IDs.",
            ),
            (
                schemars::schema_for!(CancelRecurringInvestmentParams),
                "Schedule ID to cancel - call get_recurring_investments first to discover valid IDs.",
            ),
        ] {
            let json = serde_json::to_value(&schema).unwrap();
            assert_eq!(
                json["properties"]["schedule_id"]["description"], expected_description,
                "schema description for schedule_id"
            );
        }
    }
}
