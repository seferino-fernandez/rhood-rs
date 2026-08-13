use rhood_core::models::order::{
    MarketHours, OptionOrderIntent, OrderAmount, OrderIntent, StockOrderIntent, Trigger,
};
use rmcp::handler::server::tool::{InputResponses as ToolInputResponses, RequestState};
use rmcp::model::{CallToolResponse, Extensions};
use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};

use self::params::{
    CancelOptionOrderParams, CancelOrderParams, PlaceOptionOrderParams, PlaceStockOrderParams,
};
use super::confirmation::{ConfirmationError, ConfirmationRound};
use super::handler::RhoodTools;
use super::types::format_tool_error;

mod params {
    use rhood_core::models::option::OptionType;
    use rhood_core::models::order::{
        MarketHours, OptionPositionEffect, OrderType, Side, TimeInForce, Trigger,
    };
    use schemars::JsonSchema;
    use serde::{Deserialize, Deserializer, Serialize};

    /// Accepts a JSON number or a numeric string (e.g. `15` or `"15.00"`).
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Num(f64),
        Str(String),
    }

    fn coerce(value: NumOrStr) -> Result<f64, String> {
        match value {
            NumOrStr::Num(n) => Ok(n),
            NumOrStr::Str(s) => s
                .trim()
                .parse::<f64>()
                .map_err(|error| format!("expected a number, got string {s:?}: {error}")),
        }
    }

    /// serde `deserialize_with` for a required `f64` that also accepts numeric strings.
    fn f64_or_str<'de, D>(deserializer: D) -> Result<f64, D::Error>
    where
        D: Deserializer<'de>,
    {
        coerce(NumOrStr::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }

    /// serde `deserialize_with` for an `Option<f64>` that also accepts numeric strings.
    fn opt_f64_or_str<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Option::<NumOrStr>::deserialize(deserializer)? {
            None => Ok(None),
            Some(inner) => coerce(inner).map(Some).map_err(serde::de::Error::custom),
        }
    }

    #[derive(Clone, Deserialize, Serialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    pub(super) struct PlaceStockOrderParams {
        /// Canonical brokerage account number from get_accounts. May be omitted only when exactly one account exists.
        pub(super) account_number: Option<String>,
        /// Stock symbol
        #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
        pub(super) symbol: String,
        /// Buy or sell
        pub(super) side: Side,
        /// Number of shares (mutually exclusive with dollar_amount)
        #[serde(default, deserialize_with = "opt_f64_or_str")]
        #[schemars(range(min = 0.0))]
        pub(super) quantity: Option<f64>,
        /// Dollar intent of at least $1. Mutually exclusive with quantity; requires buy, market, immediate, and GFD. Converted from the current ask to a six-decimal share estimate.
        #[serde(default, deserialize_with = "opt_f64_or_str")]
        #[schemars(range(min = 0.0))]
        pub(super) dollar_amount: Option<f64>,
        /// Market or limit
        pub(super) order_type: OrderType,
        /// Required for limit orders
        #[serde(default, deserialize_with = "opt_f64_or_str")]
        #[schemars(range(min = 0.0))]
        pub(super) limit_price: Option<f64>,
        /// Trigger (default: immediate)
        pub(super) trigger: Option<Trigger>,
        /// Required when trigger is "stop"
        #[serde(default, deserialize_with = "opt_f64_or_str")]
        #[schemars(range(min = 0.0))]
        pub(super) stop_price: Option<f64>,
        /// Trading session (default: regular_hours)
        pub(super) market_hours: Option<MarketHours>,
        /// Duration policy for the order
        pub(super) time_in_force: TimeInForce,
    }

    #[derive(Clone, Deserialize, Serialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    pub(super) struct PlaceOptionOrderParams {
        /// Canonical brokerage account number from get_accounts. May be omitted only when exactly one account exists.
        pub(super) account_number: Option<String>,
        /// Stock symbol (e.g. "AAPL")
        #[schemars(length(max = 12), regex(pattern = r"^[A-Za-z0-9.]{1,12}$"))]
        pub(super) symbol: String,
        /// Expiration date in YYYY-MM-DD format
        #[schemars(regex(pattern = r"^\d{4}-\d{2}-\d{2}$"))]
        pub(super) expiration_date: String,
        /// Strike price
        #[serde(deserialize_with = "f64_or_str")]
        #[schemars(range(min = 0.0))]
        pub(super) strike_price: f64,
        /// Option type
        pub(super) option_type: OptionType,
        /// Buy or sell
        pub(super) side: Side,
        /// Whether to open a new position or close an existing one. Use `open` with buy for
        /// buy-to-open or sell for sell-to-open; use `close` with sell for sell-to-close or buy
        /// for buy-to-close.
        pub(super) position_effect: OptionPositionEffect,
        /// Number of contracts
        #[serde(deserialize_with = "f64_or_str")]
        #[schemars(range(min = 0.0))]
        pub(super) quantity: f64,
        /// Limit price per contract
        #[serde(deserialize_with = "f64_or_str")]
        #[schemars(range(min = 0.0))]
        pub(super) limit_price: f64,
        /// Duration policy for the order
        pub(super) time_in_force: TimeInForce,
    }

    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    pub(super) struct CancelOrderParams {
        /// The order ID to cancel - call get_open_orders first to discover valid IDs.
        pub(super) order_id: String,
    }

    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    pub(super) struct CancelOptionOrderParams {
        /// The option order ID to cancel - call get_open_option_orders first to discover valid IDs.
        pub(super) order_id: String,
    }
}

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
    clippy::float_cmp,
    reason = "these adapter tests assert rejected Result states without unwrapping, and compare exactly parsed decimal parameter values"
)]
mod tests {
    use rhood_core::RhoodError;
    use rhood_core::models::option::OptionType;
    use rhood_core::models::order::{
        OptionPositionEffect, OrderIntent, OrderType, Side, TimeInForce,
    };

    use super::{
        CancelOptionOrderParams, CancelOrderParams, PlaceOptionOrderParams, PlaceStockOrderParams,
        RhoodTools, format_confirmation_error, option_order_intent, stock_order_intent,
    };
    use crate::tools::confirmation::ConfirmationError;

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

    #[test]
    fn stock_order_accepts_string_and_number_prices() {
        let from_str: PlaceStockOrderParams = serde_json::from_str(
            r#"{"symbol":"F","side":"buy","quantity":"1","order_type":"limit","limit_price":"15.00","time_in_force":"gtc"}"#,
        )
        .unwrap();
        let from_num: PlaceStockOrderParams = serde_json::from_str(
            r#"{"symbol":"F","side":"buy","quantity":1,"order_type":"limit","limit_price":15.0,"time_in_force":"gfd"}"#,
        )
        .unwrap();
        assert_eq!(from_str.limit_price, Some(15.0));
        assert_eq!(from_str.quantity, Some(1.0));
        assert_eq!(from_num.limit_price, Some(15.0));
    }

    #[test]
    fn option_order_accepts_string_prices() {
        let parsed: PlaceOptionOrderParams = serde_json::from_str(
            r#"{"symbol":"AAPL","expiration_date":"2026-06-18","strike_price":"400","option_type":"call","side":"buy","position_effect":"open","quantity":"1","limit_price":"0.01","time_in_force":"gtc"}"#,
        )
        .unwrap();
        assert_eq!(parsed.strike_price, 400.0);
        assert_eq!(parsed.quantity, 1.0);
        assert_eq!(parsed.limit_price, 0.01);
    }

    #[test]
    fn rejects_non_numeric_string() {
        let err = serde_json::from_str::<PlaceOptionOrderParams>(
            r#"{"symbol":"AAPL","expiration_date":"2026-06-18","strike_price":"abc","option_type":"call","side":"buy","position_effect":"open","quantity":"1","limit_price":"0.01","time_in_force":"gtc"}"#,
        );
        assert!(err.is_err());
    }

    #[test]
    fn option_order_requires_a_known_position_effect() {
        let missing = serde_json::from_str::<PlaceOptionOrderParams>(
            r#"{"symbol":"AAPL","expiration_date":"2026-06-18","strike_price":"400","option_type":"call","side":"buy","quantity":"1","limit_price":"0.01","time_in_force":"gtc"}"#,
        );
        assert!(missing.is_err(), "position_effect must be required");

        let unknown = serde_json::from_str::<PlaceOptionOrderParams>(
            r#"{"symbol":"AAPL","expiration_date":"2026-06-18","strike_price":"400","option_type":"call","side":"buy","position_effect":"roll","quantity":"1","limit_price":"0.01","time_in_force":"gtc"}"#,
        );
        assert!(
            unknown.is_err(),
            "unknown position effects must be rejected"
        );
    }

    #[test]
    fn place_stock_order_rejects_unknown_side() {
        let err = serde_json::from_str::<PlaceStockOrderParams>(
            r#"{"symbol":"F","side":"hodl","order_type":"market","quantity":1}"#,
        );
        assert!(err.is_err(), "unknown side must be rejected");
    }

    #[test]
    fn strike_price_schema_has_minimum() {
        let schema = schemars::schema_for!(PlaceOptionOrderParams);
        let json = serde_json::to_value(&schema).unwrap();
        assert!(
            json["properties"]["strike_price"].get("minimum").is_some(),
            "strike_price must declare a minimum"
        );
    }

    #[test]
    fn order_id_schema_descriptions_name_discovery_tools() {
        for (schema, expected_description) in [
            (
                schemars::schema_for!(CancelOrderParams),
                "The order ID to cancel - call get_open_orders first to discover valid IDs.",
            ),
            (
                schemars::schema_for!(CancelOptionOrderParams),
                "The option order ID to cancel - call get_open_option_orders first to discover valid IDs.",
            ),
        ] {
            let json = serde_json::to_value(&schema).unwrap();
            assert_eq!(
                json["properties"]["order_id"]["description"], expected_description,
                "schema description for order_id"
            );
        }
    }

    #[test]
    fn place_stock_order_rejects_unknown_field() {
        let err = serde_json::from_str::<PlaceStockOrderParams>(
            r#"{"symbol":"F","side":"buy","order_type":"market","quantity":1,"evil":"x"}"#,
        );
        assert!(
            err.is_err(),
            "unknown field must be rejected on write structs"
        );
    }

    #[test]
    fn place_option_order_requires_an_explicit_position_effect() {
        let tool = RhoodTools::order_router()
            .list_all()
            .into_iter()
            .find(|tool| tool.name == "place_option_order")
            .expect("place_option_order must be advertised");
        let schema = serde_json::Value::Object((*tool.input_schema).clone());
        let required = schema["required"]
            .as_array()
            .expect("place_option_order must list required fields");
        assert!(
            required.contains(&serde_json::json!("position_effect")),
            "position_effect must be required: {required:?}"
        );

        let position_effect_ref = schema["properties"]["position_effect"]["$ref"]
            .as_str()
            .expect("position_effect must reference its enum schema");
        let position_effect_name = position_effect_ref
            .strip_prefix("#/$defs/")
            .expect("position_effect schema reference must use $defs");
        let values: Vec<_> = schema["$defs"][position_effect_name]["oneOf"]
            .as_array()
            .expect("position_effect must be a string enum")
            .iter()
            .map(|variant| {
                variant["const"]
                    .as_str()
                    .expect("position-effect variants must be strings")
            })
            .collect();
        assert_eq!(values, ["open", "close"]);
        let description = schema["properties"]["position_effect"]["description"]
            .as_str()
            .expect("position_effect must explain the four actions");
        for action in [
            "buy-to-open",
            "sell-to-open",
            "sell-to-close",
            "buy-to-close",
        ] {
            assert!(
                description.contains(action),
                "position_effect must explain {action}: {description}"
            );
        }
    }
}
