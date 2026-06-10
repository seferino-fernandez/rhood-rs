use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};

use super::handler::RhoodTools;
use super::params::*;
use super::types::format_tool_error;

#[tool_router(router = recurring_router, vis = "pub(super)")]
impl RhoodTools {
    #[tool(
        name = "get_recurring_investments",
        description = "List all recurring investment schedules",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_recurring_investments(&self, peer: Peer<RoleServer>) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        let investments = client
            .get_recurring_investments()
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
        let req = CreateRecurringRequest {
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
