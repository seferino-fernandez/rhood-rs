use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;

use axum::http::request::Parts;
use rhood_core::models::order::{
    MarketHours, OptionOrderRequest, OrderAmount, OrderType, Side, StockOrderRequest, TimeInForce,
    Trigger,
};
use rmcp::handler::server::tool::{InputResponses as ToolInputResponses, RequestState};
use rmcp::model::{
    CallToolResponse, CallToolResult, ContentBlock, ElicitRequest, ElicitRequestParams,
    ElicitResult, ElicitationAction, ElicitationSchema, Extensions, InputRequest,
    InputRequiredResult, InputResponses, RequestStateCodec, SealOptions,
};
use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::handler::{OrderRedemptionStore, RhoodTools};
use super::params::*;
use super::types::format_tool_error;
use crate::middleware::oauth::{OAuthAuthenticated, OAuthPrincipal};

const MAX_SYMBOL_LENGTH: usize = 6;
const CONFIRMATION_REQUEST_ID: &str = "trade_confirmation";
const STOCK_ORDER_TOOL: &str = "place_stock_order";
const OPTION_ORDER_TOOL: &str = "place_option_order";
const STDIO_PRINCIPAL: &str = "transport:stdio";
const NON_OAUTH_HTTP_PRINCIPAL: &str = "transport:http-non-oauth";

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "order")]
enum StagedOrder {
    Stock(PlaceStockOrderParams),
    Option(PlaceOptionOrderParams),
}

enum OrderRequest {
    Stock(StockOrderRequest),
    Option(OptionOrderRequest),
}

struct PreparedOrder {
    request: OrderRequest,
    summary: String,
}

struct OrderRoundRequest {
    order: StagedOrder,
    request_state: Option<String>,
    input_responses: Option<InputResponses>,
}

/// Validates that a symbol string looks like a valid stock ticker.
/// Does not verify the symbol exists, that happens at confirmation.
fn validate_symbol_format(symbol: &str) -> Result<(), String> {
    if symbol.is_empty() {
        return Err("Symbol cannot be empty".into());
    }
    if symbol.len() > MAX_SYMBOL_LENGTH {
        return Err(format!("Symbol too long: '{symbol}' (max 6 characters)"));
    }
    if !symbol
        .chars()
        .all(|symbol_char| symbol_char.is_ascii_alphanumeric() || symbol_char == '.')
    {
        return Err(format!(
            "Invalid symbol format: '{symbol}' (must be alphanumeric)"
        ));
    }
    Ok(())
}

/// Rejects a numeric order field that is not strictly positive.
///
/// schemars `range(min = 0.0)` is advertised-only; serde does not enforce it,
/// so this guards against negative/zero amounts reaching the brokerage at
/// runtime. Uses an ordering comparison (`>`), not float equality.
fn validate_positive(name: &str, value: f64) -> Result<(), String> {
    if value > 0.0 {
        Ok(())
    } else {
        Err(format!("{name} must be greater than 0"))
    }
}

fn time_in_force_name(time_in_force: TimeInForce) -> &'static str {
    match time_in_force {
        TimeInForce::Gfd => "gfd (good for day)",
        TimeInForce::Gtc => "gtc (good till cancelled)",
    }
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

fn prepare_stock_order(params: &PlaceStockOrderParams) -> Result<PreparedOrder, String> {
    validate_symbol_format(&params.symbol)?;
    if params.order_type == OrderType::Limit && params.limit_price.is_none() {
        return Err("limit_price required for limit orders".into());
    }
    if let Some(quantity) = params.quantity {
        validate_positive("quantity", quantity)?;
    }
    if let Some(dollar_amount) = params.dollar_amount {
        validate_positive("dollar_amount", dollar_amount)?;
    }
    if let Some(limit_price) = params.limit_price {
        validate_positive("limit_price", limit_price)?;
    }
    if let Some(stop_price) = params.stop_price {
        validate_positive("stop_price", stop_price)?;
    }

    let amount = match (params.quantity, params.dollar_amount) {
        (Some(quantity), None) => OrderAmount::Quantity(quantity),
        (None, Some(dollar_amount)) => OrderAmount::DollarAmount(dollar_amount),
        (Some(_), Some(_)) => return Err("Specify quantity or dollar_amount, not both".into()),
        (None, None) => return Err("Specify quantity or dollar_amount".into()),
    };
    let trigger = params.trigger.unwrap_or(Trigger::Immediate);
    let market_hours = params.market_hours.unwrap_or(MarketHours::RegularHours);
    let amount_summary = match amount {
        OrderAmount::Quantity(quantity) => format!("{quantity} shares"),
        OrderAmount::DollarAmount(dollar_amount) => format!("${dollar_amount:.2}"),
    };
    let limit_price_summary = params.limit_price.map_or_else(
        || "not applicable".to_string(),
        |limit_price| format!("${limit_price:.2}"),
    );
    let stop_price_summary = params.stop_price.map_or_else(
        || "not applicable".to_string(),
        |stop_price| format!("${stop_price:.2}"),
    );
    let summary = format!(
        "Confirm stock order: side={}; amount={}; symbol={}; order type={}; limit price={}; trigger={}; stop price={}; time in force={}; market hours={market_hours}",
        side_name(params.side),
        amount_summary,
        params.symbol,
        params.order_type,
        limit_price_summary,
        trigger,
        stop_price_summary,
        time_in_force_name(params.time_in_force),
    );
    let request = StockOrderRequest {
        symbol: params.symbol.clone(),
        amount,
        side: params.side,
        order_type: params.order_type,
        limit_price: params.limit_price,
        trigger,
        stop_price: params.stop_price,
        time_in_force: params.time_in_force,
        market_hours,
    };
    Ok(PreparedOrder {
        request: OrderRequest::Stock(request),
        summary,
    })
}

fn prepare_option_order(params: &PlaceOptionOrderParams) -> Result<PreparedOrder, String> {
    validate_symbol_format(&params.symbol)?;
    validate_positive("strike_price", params.strike_price)?;
    validate_positive("quantity", params.quantity)?;
    validate_positive("limit_price", params.limit_price)?;
    let (position_effect, credit_or_debit) = match params.side {
        Side::Buy => ("open".to_string(), "debit".to_string()),
        Side::Sell => ("close".to_string(), "credit".to_string()),
    };
    let summary = format!(
        "Confirm option order: side={}; quantity={} contracts; symbol={}; option type={}; strike price=${:.2}; expiration={}; order type=limit; limit price=${:.2}; position effect={}; credit or debit={}; time in force={}",
        side_name(params.side),
        params.quantity,
        params.symbol,
        params.option_type,
        params.strike_price,
        params.expiration_date,
        params.limit_price,
        position_effect,
        credit_or_debit,
        time_in_force_name(params.time_in_force),
    );
    let request = OptionOrderRequest {
        symbol: params.symbol.clone(),
        expiration_date: params.expiration_date.clone(),
        strike_price: params.strike_price,
        option_type: params.option_type.to_string(),
        side: params.side,
        quantity: params.quantity,
        limit_price: params.limit_price,
        position_effect,
        credit_or_debit,
        time_in_force: params.time_in_force,
    };
    Ok(PreparedOrder {
        request: OrderRequest::Option(request),
        summary,
    })
}

fn prepare_order(order: &StagedOrder) -> Result<PreparedOrder, String> {
    match order {
        StagedOrder::Stock(params) => prepare_stock_order(params),
        StagedOrder::Option(params) => prepare_option_order(params),
    }
}

fn request_principal(extensions: &Extensions) -> Result<String, String> {
    let Some(parts) = extensions.get::<Parts>() else {
        return Ok(STDIO_PRINCIPAL.to_string());
    };
    if let Some(authorization) = parts.extensions.get::<OAuthPrincipal>()
        && !authorization.principal.is_empty()
    {
        return Ok(authorization.principal.clone());
    }
    if parts.extensions.get::<OAuthAuthenticated>().is_some() {
        return Err(
            "Authenticated OAuth request has no Principal; authenticate again before staging the order"
                .to_string(),
        );
    }
    Ok(NON_OAUTH_HTTP_PRINCIPAL.to_string())
}

fn associated_data(
    principal: &str,
    tool_name: &str,
    order: &StagedOrder,
) -> Result<Vec<u8>, String> {
    let request = serde_json::to_vec(order).map_err(|error| error.to_string())?;
    let request_digest = Sha256::digest(request);
    let principal_len = u64::try_from(principal.len()).map_err(|_conversion_error| {
        "Principal is too long to bind to request state".to_string()
    })?;
    let tool_name_len = u64::try_from(tool_name.len()).map_err(|_conversion_error| {
        "Tool name is too long to bind to request state".to_string()
    })?;
    let mut binding = Vec::new();
    binding.extend_from_slice(&principal_len.to_be_bytes());
    binding.extend_from_slice(principal.as_bytes());
    binding.extend_from_slice(&tool_name_len.to_be_bytes());
    binding.extend_from_slice(tool_name.as_bytes());
    binding.extend_from_slice(&request_digest);
    Ok(binding)
}

fn stage_order(
    codec: &RequestStateCodec,
    ttl: Duration,
    associated_data: &[u8],
    order: &StagedOrder,
    summary: String,
) -> Result<CallToolResponse, String> {
    let request_state = codec
        .seal_json_with(
            order,
            &SealOptions::new().associated_data(associated_data).ttl(ttl),
        )
        .map_err(|error| error.to_string())?;
    let requested_schema = ElicitationSchema::builder()
        .build()
        .map_err(|error| error.to_string())?;
    let elicitation = ElicitRequest::new(ElicitRequestParams::FormElicitationParams {
        meta: None,
        message: summary,
        requested_schema,
    });
    let mut requests = BTreeMap::new();
    requests.insert(
        CONFIRMATION_REQUEST_ID.to_string(),
        InputRequest::Elicitation(elicitation),
    );
    Ok(InputRequiredResult::new(Some(requests), Some(request_state)).into())
}

fn verify_confirmation(input_responses: Option<InputResponses>) -> Result<(), String> {
    let responses = input_responses.ok_or_else(|| {
        "Confirmation response is missing; stage the order again and accept the elicitation"
            .to_string()
    })?;
    let response = responses.get(CONFIRMATION_REQUEST_ID).ok_or_else(|| {
        "Trade Confirmation response is missing; stage the order again and accept the elicitation"
            .to_string()
    })?;
    let elicitation: ElicitResult = serde_json::from_value(response.clone()).map_err(|error| {
        format!("Trade Confirmation response is invalid; stage the order again: {error}")
    })?;
    match elicitation.action {
        ElicitationAction::Accept => Ok(()),
        ElicitationAction::Decline => {
            Err("Trade Confirmation was declined; the order was not submitted".to_string())
        }
        ElicitationAction::Cancel => {
            Err("Trade Confirmation was cancelled; the order was not submitted".to_string())
        }
        _ => Err("Trade Confirmation was not accepted; the order was not submitted".to_string()),
    }
}

async fn run_order_round<F, Fut>(
    codec: &RequestStateCodec,
    redemptions: &OrderRedemptionStore,
    ttl: Duration,
    principal: &str,
    tool_name: &str,
    round: OrderRoundRequest,
    submit: F,
) -> Result<CallToolResponse, String>
where
    F: FnOnce(OrderRequest) -> Fut,
    Fut: Future<Output = Result<Option<String>, String>>,
{
    let prepared = prepare_order(&round.order)?;
    let binding = associated_data(principal, tool_name, &round.order)?;
    let Some(request_state) = round.request_state else {
        if round.input_responses.is_some() {
            return Err(
                "Confirmation request state is missing; stage the order again before submitting"
                    .to_string(),
            );
        }
        return stage_order(codec, ttl, &binding, &round.order, prepared.summary);
    };

    verify_confirmation(round.input_responses)?;
    let staged_order = codec
        .open_json_with(&request_state, &binding)
        .map_err(|error| RhoodTools::request_state_error(&error))?;
    redemptions.consume(&request_state)?;
    let prepared = prepare_order(&staged_order)?;
    let order_id = submit(prepared.request).await?;
    let result = serde_json::json!({
        "order_id": order_id,
        "status": "submitted",
        "summary": prepared.summary,
    });
    let content = serde_json::to_string_pretty(&result).map_err(|error| error.to_string())?;
    Ok(CallToolResult::success(vec![ContentBlock::text(content)]).into())
}

#[tool_router(router = order_router, vis = "pub(super)")]
impl RhoodTools {
    #[tool(
        name = "place_stock_order",
        description = "Stage a stock order and request human Confirmation through MCP elicitation; the accepted retry submits it.",
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
        let principal = request_principal(&extensions)?;
        run_order_round(
            &self.request_state_codec,
            &self.order_redemptions,
            self.order_confirmation_ttl,
            &principal,
            STOCK_ORDER_TOOL,
            OrderRoundRequest {
                order: StagedOrder::Stock(params),
                request_state,
                input_responses,
            },
            |request| async move {
                let OrderRequest::Stock(request) = request else {
                    return Err("Sealed request state contains the wrong order type".to_string());
                };
                let client = self.ensure_client(&peer).await?;
                let order = client
                    .place_stock_order(&request)
                    .await
                    .map_err(|error| format_tool_error(&error))?;
                Ok(order.id)
            },
        )
        .await
    }

    #[tool(
        name = "place_option_order",
        description = "Stage an option order and request human Confirmation through MCP elicitation; the accepted retry submits it.",
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
        let principal = request_principal(&extensions)?;
        run_order_round(
            &self.request_state_codec,
            &self.order_redemptions,
            self.order_confirmation_ttl,
            &principal,
            OPTION_ORDER_TOOL,
            OrderRoundRequest {
                order: StagedOrder::Option(params),
                request_state,
                input_responses,
            },
            |request| async move {
                let OrderRequest::Option(request) = request else {
                    return Err("Sealed request state contains the wrong order type".to_string());
                };
                let client = self.ensure_client(&peer).await?;
                let order = client
                    .place_option_order(&request)
                    .await
                    .map_err(|error| format_tool_error(&error))?;
                Ok(order.id)
            },
        )
        .await
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
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
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
            .map_err(|rhood_error| format_tool_error(&rhood_error))?;
        let result = serde_json::json!({ "order_id": params.order_id, "status": "cancelled" });
        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
#[expect(
    clippy::assertions_on_result_states,
    reason = "these validation tests deliberately assert accepted and rejected Result states without unwrapping successes"
)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use axum::http::Request;
    use rhood_core::models::option::OptionType;
    use rhood_core::models::order::{OrderType, Side, TimeInForce};
    use rhood_core::{RhoodConfig, RobinhoodClient};
    use rmcp::model::{
        CallToolResponse, ElicitRequestParams, ElicitResult, ElicitationAction, Extensions,
        InputRequest, InputRequiredResult, InputResponses,
    };
    use tokio::sync::Barrier;

    use crate::config::McpConfig;
    use crate::middleware::oauth::{OAuthAuthenticated, OAuthPrincipal};
    use crate::tools::handler::{ORDER_REDEMPTION_RETENTION, OrderRedemptionStore, RhoodTools};

    use super::{
        CONFIRMATION_REQUEST_ID, NON_OAUTH_HTTP_PRINCIPAL, OPTION_ORDER_TOOL, OrderRoundRequest,
        STDIO_PRINCIPAL, STOCK_ORDER_TOOL, StagedOrder, request_principal, run_order_round,
        validate_positive, validate_symbol_format,
    };
    use crate::tools::ToolRateLimiter;
    use crate::tools::params::{PlaceOptionOrderParams, PlaceStockOrderParams};

    fn tools() -> RhoodTools {
        let client = RobinhoodClient::with_config(RhoodConfig::default())
            .expect("default config builds a client");
        let hook = Arc::new(
            |_client: RobinhoodClient, _peer: rmcp::Peer<rmcp::RoleServer>| {
                Box::pin(async { Ok(()) })
                    as std::pin::Pin<
                        Box<dyn std::future::Future<Output = Result<(), String>> + Send>,
                    >
            },
        );
        let config = McpConfig::default();
        let rate_limiter =
            ToolRateLimiter::new(&config.rate_limit).expect("default rate limits are valid");
        let mut tools = RhoodTools::new_lazy(client, hook, false, &config, rate_limiter);
        tools.order_redemptions = Arc::new(OrderRedemptionStore::new(ORDER_REDEMPTION_RETENTION));
        tools
    }

    fn extensions_with_parts(parts: axum::http::request::Parts) -> Extensions {
        let mut extensions = Extensions::default();
        extensions.insert(parts);
        extensions
    }

    #[test]
    fn request_principal_without_http_parts_uses_stdio_principal() {
        assert_eq!(
            request_principal(&Extensions::default()).expect("stdio Principal is available"),
            STDIO_PRINCIPAL
        );
    }

    #[test]
    fn request_principal_uses_validated_oauth_client_id() {
        let (mut parts, _body) = Request::new(()).into_parts();
        parts.extensions.insert(OAuthPrincipal {
            principal: "registered-client".to_string(),
            scopes: vec!["read".to_string(), "write".to_string()],
        });

        assert_eq!(
            request_principal(&extensions_with_parts(parts))
                .expect("validated OAuth Principal is available"),
            "registered-client"
        );
    }

    #[test]
    fn request_principal_rejects_oauth_marker_without_validated_principal() {
        let (mut parts, _body) = Request::new(()).into_parts();
        parts.extensions.insert(OAuthAuthenticated);

        let error = request_principal(&extensions_with_parts(parts))
            .expect_err("OAuth marker without a Principal must fail closed");
        assert!(error.contains("no Principal"), "got: {error}");
    }

    #[test]
    fn request_principal_for_ordinary_http_is_bounded() {
        let (parts, _body) = Request::new(()).into_parts();

        assert_eq!(
            request_principal(&extensions_with_parts(parts))
                .expect("non-OAuth HTTP Principal is available"),
            NON_OAUTH_HTTP_PRINCIPAL
        );
    }

    fn stock_order() -> PlaceStockOrderParams {
        PlaceStockOrderParams {
            symbol: "AAPL".to_string(),
            side: Side::Buy,
            quantity: Some(2.0),
            dollar_amount: None,
            order_type: OrderType::Limit,
            limit_price: Some(187.25),
            trigger: None,
            stop_price: None,
            market_hours: None,
            time_in_force: TimeInForce::Gtc,
        }
    }

    fn option_order() -> PlaceOptionOrderParams {
        PlaceOptionOrderParams {
            symbol: "AAPL".to_string(),
            expiration_date: "2026-09-18".to_string(),
            strike_price: 200.0,
            option_type: OptionType::Call,
            side: Side::Buy,
            quantity: 1.0,
            limit_price: 2.5,
            time_in_force: TimeInForce::Gfd,
        }
    }

    fn round(
        order: StagedOrder,
        request_state: Option<String>,
        input_responses: Option<InputResponses>,
    ) -> OrderRoundRequest {
        OrderRoundRequest {
            order,
            request_state,
            input_responses,
        }
    }

    fn accepted_confirmation() -> InputResponses {
        let mut responses = InputResponses::new();
        responses.insert(
            CONFIRMATION_REQUEST_ID.to_string(),
            serde_json::to_value(ElicitResult::new(ElicitationAction::Accept))
                .expect("accepted elicitation serializes"),
        );
        responses
    }

    fn input_required(response: CallToolResponse) -> InputRequiredResult {
        match response {
            CallToolResponse::InputRequired(result) => result,
            _ => panic!("staging must return input_required"),
        }
    }

    fn sealed_state(result: &InputRequiredResult) -> String {
        result
            .request_state
            .clone()
            .expect("staged order carries sealed request state")
    }

    #[tokio::test]
    async fn staged_orders_return_input_required_without_submitting() {
        let tools = tools();
        let submissions = Arc::new(AtomicUsize::new(0));
        let stock_submissions = Arc::clone(&submissions);
        let stock = run_order_round(
            &tools.request_state_codec,
            &tools.order_redemptions,
            tools.order_confirmation_ttl,
            "principal-a",
            STOCK_ORDER_TOOL,
            round(StagedOrder::Stock(stock_order()), None, None),
            move |_| async move {
                let _previous = stock_submissions.fetch_add(1, Ordering::SeqCst);
                Ok(Some("stock-order-id".to_string()))
            },
        )
        .await
        .expect("valid stock order stages");
        let option_submissions = Arc::clone(&submissions);
        let option = run_order_round(
            &tools.request_state_codec,
            &tools.order_redemptions,
            tools.order_confirmation_ttl,
            "principal-a",
            OPTION_ORDER_TOOL,
            round(StagedOrder::Option(option_order()), None, None),
            move |_| async move {
                let _previous = option_submissions.fetch_add(1, Ordering::SeqCst);
                Ok(Some("option-order-id".to_string()))
            },
        )
        .await
        .expect("valid option order stages");

        assert!(matches!(stock, CallToolResponse::InputRequired(_)));
        assert!(matches!(option, CallToolResponse::InputRequired(_)));
        assert_eq!(submissions.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn accepted_confirmation_cannot_be_replayed() {
        let tools = tools();
        let staged = run_order_round(
            &tools.request_state_codec,
            &tools.order_redemptions,
            tools.order_confirmation_ttl,
            "principal-a",
            STOCK_ORDER_TOOL,
            round(StagedOrder::Stock(stock_order()), None, None),
            |_| async { Ok(Some("unused".to_string())) },
        )
        .await
        .expect("valid order stages");
        let state = sealed_state(&input_required(staged));
        let submissions = Arc::new(AtomicUsize::new(0));
        let counted_submissions = Arc::clone(&submissions);

        let submitted = run_order_round(
            &tools.request_state_codec,
            &tools.order_redemptions,
            tools.order_confirmation_ttl,
            "principal-a",
            STOCK_ORDER_TOOL,
            round(
                StagedOrder::Stock(stock_order()),
                Some(state.clone()),
                Some(accepted_confirmation()),
            ),
            move |_| async move {
                let _previous = counted_submissions.fetch_add(1, Ordering::SeqCst);
                Ok(Some("submitted-order-id".to_string()))
            },
        )
        .await
        .expect("accepted Confirmation submits");

        assert!(matches!(submitted, CallToolResponse::Complete(_)));
        let replay_submissions = Arc::clone(&submissions);
        let error = run_order_round(
            &tools.request_state_codec,
            &tools.order_redemptions,
            tools.order_confirmation_ttl,
            "principal-a",
            STOCK_ORDER_TOOL,
            round(
                StagedOrder::Stock(stock_order()),
                Some(state),
                Some(accepted_confirmation()),
            ),
            move |_| async move {
                let _previous = replay_submissions.fetch_add(1, Ordering::SeqCst);
                Ok(Some("duplicate-order-id".to_string()))
            },
        )
        .await
        .expect_err("an already-used Confirmation must fail closed");

        assert!(error.contains("already used"), "got: {error}");
        assert!(error.contains("stage a fresh order"), "got: {error}");
        assert_eq!(submissions.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_confirmations_submit_exactly_once() {
        let tools = tools();
        let staged = run_order_round(
            &tools.request_state_codec,
            &tools.order_redemptions,
            tools.order_confirmation_ttl,
            "principal-a",
            STOCK_ORDER_TOOL,
            round(StagedOrder::Stock(stock_order()), None, None),
            |_| async { Ok(Some("unused".to_string())) },
        )
        .await
        .expect("valid order stages");
        let state = sealed_state(&input_required(staged));
        let submissions = Arc::new(AtomicUsize::new(0));
        let start = Arc::new(Barrier::new(3));
        let first = tokio::spawn({
            let tools = tools.clone();
            let submissions = Arc::clone(&submissions);
            let start = Arc::clone(&start);
            let state = state.clone();
            async move {
                start.wait().await;
                run_order_round(
                    &tools.request_state_codec,
                    &tools.order_redemptions,
                    tools.order_confirmation_ttl,
                    "principal-a",
                    STOCK_ORDER_TOOL,
                    round(
                        StagedOrder::Stock(stock_order()),
                        Some(state),
                        Some(accepted_confirmation()),
                    ),
                    move |_| async move {
                        let _previous = submissions.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        Ok(Some("first-order-id".to_string()))
                    },
                )
                .await
            }
        });
        let second = tokio::spawn({
            let tools = tools.clone();
            let submissions = Arc::clone(&submissions);
            let start = Arc::clone(&start);
            async move {
                start.wait().await;
                run_order_round(
                    &tools.request_state_codec,
                    &tools.order_redemptions,
                    tools.order_confirmation_ttl,
                    "principal-a",
                    STOCK_ORDER_TOOL,
                    round(
                        StagedOrder::Stock(stock_order()),
                        Some(state),
                        Some(accepted_confirmation()),
                    ),
                    move |_| async move {
                        let _previous = submissions.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        Ok(Some("second-order-id".to_string()))
                    },
                )
                .await
            }
        });

        start.wait().await;
        let (first_result, second_result) = tokio::join!(first, second);
        let first_result = first_result.expect("first Confirmation task completes");
        let second_result = second_result.expect("second Confirmation task completes");

        assert!(matches!(
            (&first_result, &second_result),
            (Ok(_), Err(_)) | (Err(_), Ok(_))
        ));
        let replay_error = first_result.err().or_else(|| second_result.err());
        assert!(replay_error.is_some_and(|error| error.contains("already used")));
        assert_eq!(submissions.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn separately_staged_order_is_unaffected_by_prior_confirmation() {
        let tools = tools();
        let submissions = Arc::new(AtomicUsize::new(0));

        for symbol in ["AAPL", "MSFT"] {
            let mut staged_order = stock_order();
            staged_order.symbol = symbol.to_string();
            let staged = run_order_round(
                &tools.request_state_codec,
                &tools.order_redemptions,
                tools.order_confirmation_ttl,
                "principal-a",
                STOCK_ORDER_TOOL,
                round(StagedOrder::Stock(staged_order), None, None),
                |_| async { Ok(Some("unused".to_string())) },
            )
            .await
            .expect("valid order stages");
            let state = sealed_state(&input_required(staged));
            let counted_submissions = Arc::clone(&submissions);
            let mut confirmed_order = stock_order();
            confirmed_order.symbol = symbol.to_string();

            run_order_round(
                &tools.request_state_codec,
                &tools.order_redemptions,
                tools.order_confirmation_ttl,
                "principal-a",
                STOCK_ORDER_TOOL,
                round(
                    StagedOrder::Stock(confirmed_order),
                    Some(state),
                    Some(accepted_confirmation()),
                ),
                move |_| async move {
                    let _previous = counted_submissions.fetch_add(1, Ordering::SeqCst);
                    Ok(Some("submitted-order-id".to_string()))
                },
            )
            .await
            .expect("distinct staged order submits");
        }

        assert_eq!(submissions.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn tampered_request_state_is_rejected_without_submission() {
        let tools = tools();
        let staged = run_order_round(
            &tools.request_state_codec,
            &tools.order_redemptions,
            tools.order_confirmation_ttl,
            "principal-a",
            STOCK_ORDER_TOOL,
            round(StagedOrder::Stock(stock_order()), None, None),
            |_| async { Ok(Some("unused".to_string())) },
        )
        .await
        .expect("valid order stages");
        let mut state = sealed_state(&input_required(staged));
        state.push('A');
        let submissions = Arc::new(AtomicUsize::new(0));
        let counted_submissions = Arc::clone(&submissions);

        let error = run_order_round(
            &tools.request_state_codec,
            &tools.order_redemptions,
            tools.order_confirmation_ttl,
            "principal-a",
            STOCK_ORDER_TOOL,
            round(
                StagedOrder::Stock(stock_order()),
                Some(state),
                Some(accepted_confirmation()),
            ),
            move |_| async move {
                let _previous = counted_submissions.fetch_add(1, Ordering::SeqCst);
                Ok(Some("must-not-submit".to_string()))
            },
        )
        .await
        .expect_err("tampered request state must fail closed");

        assert!(error.contains("no longer valid"), "got: {error}");
        assert_eq!(submissions.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn different_principal_cannot_submit_staged_order() {
        let tools = tools();
        let staged = run_order_round(
            &tools.request_state_codec,
            &tools.order_redemptions,
            tools.order_confirmation_ttl,
            "principal-a",
            STOCK_ORDER_TOOL,
            round(StagedOrder::Stock(stock_order()), None, None),
            |_| async { Ok(Some("unused".to_string())) },
        )
        .await
        .expect("valid order stages");
        let state = sealed_state(&input_required(staged));
        let submissions = Arc::new(AtomicUsize::new(0));
        let counted_submissions = Arc::clone(&submissions);

        let error = run_order_round(
            &tools.request_state_codec,
            &tools.order_redemptions,
            tools.order_confirmation_ttl,
            "principal-b",
            STOCK_ORDER_TOOL,
            round(
                StagedOrder::Stock(stock_order()),
                Some(state),
                Some(accepted_confirmation()),
            ),
            move |_| async move {
                let _previous = counted_submissions.fetch_add(1, Ordering::SeqCst);
                Ok(Some("must-not-submit".to_string()))
            },
        )
        .await
        .expect_err("a different Principal must fail closed");

        assert!(error.contains("different Principal"), "got: {error}");
        assert_eq!(submissions.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn expired_request_state_has_actionable_error() {
        let tools = tools();
        let staged = run_order_round(
            &tools.request_state_codec,
            &tools.order_redemptions,
            Duration::ZERO,
            "principal-a",
            STOCK_ORDER_TOOL,
            round(StagedOrder::Stock(stock_order()), None, None),
            |_| async { Ok(Some("unused".to_string())) },
        )
        .await
        .expect("valid order stages");
        let state = sealed_state(&input_required(staged));
        tokio::time::sleep(Duration::from_millis(2)).await;

        let error = run_order_round(
            &tools.request_state_codec,
            &tools.order_redemptions,
            Duration::ZERO,
            "principal-a",
            STOCK_ORDER_TOOL,
            round(
                StagedOrder::Stock(stock_order()),
                Some(state),
                Some(accepted_confirmation()),
            ),
            |_| async { Ok(Some("must-not-submit".to_string())) },
        )
        .await
        .expect_err("expired request state must fail closed");

        assert!(error.contains("expired"), "got: {error}");
        assert!(error.contains("stage the order again"), "got: {error}");
    }

    #[tokio::test]
    async fn limit_order_elicitation_shows_every_execution_attribute() {
        let tools = tools();
        let staged = run_order_round(
            &tools.request_state_codec,
            &tools.order_redemptions,
            tools.order_confirmation_ttl,
            "principal-a",
            STOCK_ORDER_TOOL,
            round(StagedOrder::Stock(stock_order()), None, None),
            |_| async { Ok(Some("unused".to_string())) },
        )
        .await
        .expect("valid order stages");
        let required = input_required(staged);
        let request = required
            .input_requests
            .as_ref()
            .and_then(|requests| requests.get(CONFIRMATION_REQUEST_ID))
            .expect("staged order carries a Confirmation elicitation");
        let InputRequest::Elicitation(request) = request else {
            panic!("trade Confirmation must use elicitation/create");
        };
        let ElicitRequestParams::FormElicitationParams { message, .. } = &request.params else {
            panic!("trade Confirmation must use form elicitation");
        };

        for expected in [
            "side=buy",
            "amount=2 shares",
            "symbol=AAPL",
            "order type=limit",
            "limit price=$187.25",
            "time in force=gtc (good till cancelled)",
        ] {
            assert!(
                message.contains(expected),
                "missing {expected:?}: {message}"
            );
        }
    }

    #[test]
    fn valid_symbols_accepted() {
        assert!(validate_symbol_format("AAPL").is_ok());
        assert!(validate_symbol_format("MSFT").is_ok());
        assert!(validate_symbol_format("BRK.B").is_ok());
        assert!(validate_symbol_format("X").is_ok());
        assert!(validate_symbol_format("VZ").is_ok());
    }

    #[test]
    fn empty_symbol_rejected() {
        let result = validate_symbol_format("");
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("empty"),
            "error should mention empty"
        );
    }

    #[test]
    fn symbol_too_long_rejected() {
        let result = validate_symbol_format("TOOLONG1");
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("too long"),
            "error should mention too long"
        );
    }

    #[test]
    fn symbol_with_special_chars_rejected() {
        assert!(validate_symbol_format("AA PL").is_err());
        assert!(validate_symbol_format("$AAPL").is_err());
        assert!(validate_symbol_format("AAPL!").is_err());
        assert!(validate_symbol_format("AA/PL").is_err());
    }

    #[test]
    fn six_char_symbol_accepted() {
        assert!(validate_symbol_format("ABCDEF").is_ok());
    }

    #[test]
    fn seven_char_symbol_rejected() {
        assert!(validate_symbol_format("ABCDEFG").is_err());
    }

    #[test]
    fn validate_positive_accepts_positive() {
        assert!(validate_positive("quantity", 1.0).is_ok());
        assert!(validate_positive("limit_price", 0.01).is_ok());
        assert!(validate_positive("strike_price", 400.0).is_ok());
    }

    #[test]
    fn validate_positive_rejects_zero() {
        let err = validate_positive("quantity", 0.0).unwrap_err();
        assert_eq!(err, "quantity must be greater than 0");
    }

    #[test]
    fn validate_positive_rejects_negative() {
        let err = validate_positive("dollar_amount", -5.0).unwrap_err();
        assert_eq!(err, "dollar_amount must be greater than 0");
    }
}
