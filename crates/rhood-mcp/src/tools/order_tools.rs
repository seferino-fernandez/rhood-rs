use rhood_core::models::order::{
    MarketHours, OptionOrderIntent, OrderAmount, OrderIntent, StockOrderIntent, Trigger,
};
use rmcp::handler::server::tool::{InputResponses as ToolInputResponses, RequestState};
use rmcp::model::{CallToolResponse, Extensions};
use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};

use super::confirmation::{ConfirmationError, ConfirmationRound};
use super::handler::RhoodTools;
use super::params::*;
use super::types::format_tool_error;

const STOCK_ORDER_TOOL: &str = "place_stock_order";
const OPTION_ORDER_TOOL: &str = "place_option_order";

fn format_confirmation_error(error: &ConfirmationError) -> String {
    let (code, message) = match error {
        ConfirmationError::MissingRoundData => (
            "confirmation_missing_round_data",
            "Confirmation request state or response is missing; stage the order again and answer the elicitation",
        ),
        ConfirmationError::MalformedResponse => (
            "confirmation_malformed_response",
            "Trade Confirmation response is invalid; correct the response and retry",
        ),
        ConfirmationError::Declined => (
            "confirmation_declined",
            "Trade Confirmation was declined; the order was not submitted",
        ),
        ConfirmationError::Cancelled => (
            "confirmation_cancelled",
            "Trade Confirmation was cancelled; the order was not submitted",
        ),
        ConfirmationError::Expired => (
            "confirmation_expired",
            "Confirmation expired; stage the order again before submitting",
        ),
        ConfirmationError::InvalidState => (
            "confirmation_invalid_state",
            "Confirmation is no longer valid; stage the order again",
        ),
        ConfirmationError::AlreadyConsumed => (
            "confirmation_already_consumed",
            "Confirmation was already used; stage a fresh order before submitting again",
        ),
        ConfirmationError::StateUnavailable => (
            "confirmation_state_unavailable",
            "Confirmation state is unavailable; stage the order again later",
        ),
        ConfirmationError::PrincipalUnavailable => (
            "confirmation_principal_unavailable",
            "Authenticated OAuth request has no Principal; authenticate again before staging the order",
        ),
        ConfirmationError::PreparationFailure(source)
        | ConfirmationError::SubmissionFailure(source) => return format_tool_error(source),
        ConfirmationError::InternalFailure(detail) => {
            tracing::error!(%detail, "MCP order Confirmation failed internally");
            (
                "confirmation_internal_failure",
                "Confirmation could not be completed because of an internal server error",
            )
        }
    };
    serde_json::json!({ "error": code, "message": message }).to_string()
}

fn stock_order_intent(params: PlaceStockOrderParams) -> Result<OrderIntent, String> {
    let amount = match (params.quantity, params.dollar_amount) {
        (Some(quantity), None) => OrderAmount::Quantity(quantity),
        (None, Some(dollar_amount)) => OrderAmount::DollarAmount(dollar_amount),
        (Some(_), Some(_)) => return Err("Specify quantity or dollar_amount, not both".into()),
        (None, None) => return Err("Specify quantity or dollar_amount".into()),
    };
    Ok(OrderIntent::Stock(StockOrderIntent {
        account_number: params.account_number,
        symbol: params.symbol,
        amount,
        side: params.side,
        order_type: params.order_type,
        limit_price: params.limit_price,
        trigger: params.trigger.unwrap_or(Trigger::Immediate),
        stop_price: params.stop_price,
        time_in_force: params.time_in_force,
        market_hours: params.market_hours.unwrap_or(MarketHours::RegularHours),
    }))
}

fn option_order_intent(params: PlaceOptionOrderParams) -> OrderIntent {
    OrderIntent::Option(OptionOrderIntent {
        account_number: params.account_number,
        symbol: params.symbol,
        expiration_date: params.expiration_date,
        strike_price: params.strike_price,
        option_type: params.option_type.to_string(),
        side: params.side,
        quantity: params.quantity,
        limit_price: params.limit_price,
        position_effect: params.position_effect,
        time_in_force: params.time_in_force,
    })
}

#[tool_router(router = order_router, vis = "pub(super)")]
impl RhoodTools {
    #[tool(
        name = "place_stock_order",
        description = "Prepare an exact stock order in core, request human Confirmation through MCP elicitation, then submit that sealed Staged Order once on the accepted retry.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn place_stock_order(
        &self,
        peer: Peer<RoleServer>,
        extensions: Extensions,
        RequestState(request_state): RequestState,
        ToolInputResponses(input_responses): ToolInputResponses,
        Parameters(params): Parameters<PlaceStockOrderParams>,
    ) -> Result<CallToolResponse, String> {
        let intent = stock_order_intent(params)?;
        let client = self.ensure_client(&peer).await?;
        self.order_confirmation
            .advance(
                &client,
                &extensions,
                STOCK_ORDER_TOOL,
                intent,
                ConfirmationRound {
                    request_state,
                    input_responses,
                },
            )
            .await
            .map_err(|error| format_confirmation_error(&error))
    }

    #[tool(
        name = "place_option_order",
        description = "Prepare an exact single-leg option limit order in core, including the selected contract and client reference, request human Confirmation through MCP elicitation, then submit that sealed Staged Order once on the accepted retry.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn place_option_order(
        &self,
        peer: Peer<RoleServer>,
        extensions: Extensions,
        RequestState(request_state): RequestState,
        ToolInputResponses(input_responses): ToolInputResponses,
        Parameters(params): Parameters<PlaceOptionOrderParams>,
    ) -> Result<CallToolResponse, String> {
        let intent = option_order_intent(params);
        let client = self.ensure_client(&peer).await?;
        self.order_confirmation
            .advance(
                &client,
                &extensions,
                OPTION_ORDER_TOOL,
                intent,
                ConfirmationRound {
                    request_state,
                    input_responses,
                },
            )
            .await
            .map_err(|error| format_confirmation_error(&error))
    }

    #[tool(
        name = "cancel_order",
        description = "Cancel an open stock order",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn cancel_order(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<CancelOrderParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        client
            .cancel_stock_order(&params.order_id)
            .await
            .map_err(|error| format_tool_error(&error))?;
        let result = serde_json::json!({ "order_id": params.order_id, "status": "cancelled" });
        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())
    }

    #[tool(
        name = "cancel_option_order",
        description = "Cancel an open option order",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn cancel_option_order(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<CancelOptionOrderParams>,
    ) -> Result<String, String> {
        let client = self.ensure_client(&peer).await?;
        client
            .cancel_option_order(&params.order_id)
            .await
            .map_err(|error| format_tool_error(&error))?;
        let result = serde_json::json!({ "order_id": params.order_id, "status": "cancelled" });
        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
#[expect(
    clippy::assertions_on_result_states,
    reason = "these adapter tests intentionally assert accepted and rejected Result states"
)]
mod tests {
    use rhood_core::RhoodError;
    use rhood_core::models::option::OptionType;
    use rhood_core::models::order::{
        OptionPositionEffect, OrderIntent, OrderType, Side, TimeInForce,
    };

    use super::{format_confirmation_error, option_order_intent, stock_order_intent};
    use crate::tools::confirmation::ConfirmationError;
    use crate::tools::params::{PlaceOptionOrderParams, PlaceStockOrderParams};

    fn rendered_error(error: &ConfirmationError) -> serde_json::Value {
        serde_json::from_str(&format_confirmation_error(error)).unwrap()
    }

    #[test]
    fn confirmation_errors_have_stable_structured_codes_and_messages() {
        let cases = [
            (
                ConfirmationError::MissingRoundData,
                "confirmation_missing_round_data",
                "Confirmation request state or response is missing; stage the order again and answer the elicitation",
            ),
            (
                ConfirmationError::MalformedResponse,
                "confirmation_malformed_response",
                "Trade Confirmation response is invalid; correct the response and retry",
            ),
            (
                ConfirmationError::Declined,
                "confirmation_declined",
                "Trade Confirmation was declined; the order was not submitted",
            ),
            (
                ConfirmationError::Cancelled,
                "confirmation_cancelled",
                "Trade Confirmation was cancelled; the order was not submitted",
            ),
            (
                ConfirmationError::Expired,
                "confirmation_expired",
                "Confirmation expired; stage the order again before submitting",
            ),
            (
                ConfirmationError::AlreadyConsumed,
                "confirmation_already_consumed",
                "Confirmation was already used; stage a fresh order before submitting again",
            ),
            (
                ConfirmationError::StateUnavailable,
                "confirmation_state_unavailable",
                "Confirmation state is unavailable; stage the order again later",
            ),
            (
                ConfirmationError::PrincipalUnavailable,
                "confirmation_principal_unavailable",
                "Authenticated OAuth request has no Principal; authenticate again before staging the order",
            ),
        ];

        for (error, code, message) in cases {
            assert_eq!(
                rendered_error(&error),
                serde_json::json!({ "error": code, "message": message })
            );
        }
    }

    #[test]
    fn preparation_and_submission_sources_keep_core_rendering() {
        let preparation = rendered_error(&ConfirmationError::PreparationFailure(
            RhoodError::InvalidSymbol("NOTREAL".to_string()),
        ));
        let submission = rendered_error(&ConfirmationError::SubmissionFailure(
            RhoodError::ReadOnlyMode,
        ));

        assert_eq!(preparation["error"], "invalid_symbol");
        assert_eq!(preparation["message"], "Symbol not found: NOTREAL");
        assert_eq!(submission["error"], "read_only");
        assert_eq!(
            submission["message"],
            "Operation blocked, client is in read-only mode"
        );
    }

    #[test]
    fn submission_uncertainty_preserves_frozen_client_reference() {
        let rendered = rendered_error(&ConfirmationError::SubmissionFailure(
            RhoodError::SubmissionUncertain {
                client_ref: "frozen-client-ref-123".to_string(),
                message: "connection closed before an authoritative response".to_string(),
            },
        ));

        assert_eq!(rendered["error"], "submission_uncertain");
        assert_eq!(
            rendered["message"],
            "Order submission is uncertain for client reference frozen-client-ref-123: connection closed before an authoritative response. Reconcile open and recent orders before retrying"
        );
    }

    #[test]
    fn invalid_state_rendering_collapses_security_sensitive_causes() {
        let rendered = rendered_error(&ConfirmationError::InvalidState);

        assert_eq!(
            rendered,
            serde_json::json!({
                "error": "confirmation_invalid_state",
                "message": "Confirmation is no longer valid; stage the order again",
            })
        );
        let public = rendered.to_string();
        assert!(!public.contains("tamper"));
        assert!(!public.contains("Principal"));
        assert!(!public.contains("binding"));
    }

    #[test]
    fn internal_failure_rendering_redacts_private_detail() {
        let rendered = rendered_error(&ConfirmationError::InternalFailure(
            "secret codec key material and schema diagnostic".to_string(),
        ));

        assert_eq!(rendered["error"], "confirmation_internal_failure");
        assert_eq!(
            rendered["message"],
            "Confirmation could not be completed because of an internal server error"
        );
        assert!(!rendered.to_string().contains("secret codec key material"));
    }

    #[test]
    fn stock_params_require_exactly_one_amount() {
        let params = PlaceStockOrderParams {
            account_number: None,
            symbol: "AAPL".to_string(),
            side: Side::Buy,
            quantity: None,
            dollar_amount: None,
            order_type: OrderType::Market,
            limit_price: None,
            trigger: None,
            stop_price: None,
            market_hours: None,
            time_in_force: TimeInForce::Gfd,
        };
        assert!(stock_order_intent(params).is_err());
    }

    #[test]
    fn option_params_convert_without_broker_policy() {
        let intent = option_order_intent(PlaceOptionOrderParams {
            account_number: Some("ABC123".to_string()),
            symbol: "AAPL".to_string(),
            expiration_date: "2026-09-18".to_string(),
            strike_price: 200.0,
            option_type: OptionType::Call,
            side: Side::Buy,
            position_effect: OptionPositionEffect::Open,
            quantity: 1.0,
            limit_price: 2.5,
            time_in_force: TimeInForce::Gfd,
        });
        let OrderIntent::Option(intent) = intent else {
            panic!("expected option intent");
        };
        assert_eq!(intent.option_type, "call");
        assert_eq!(intent.account_number.as_deref(), Some("ABC123"));
    }
}
