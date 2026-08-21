use std::collections::{BTreeMap, HashMap, hash_map::Entry};
use std::future::Future;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::http::request::Parts;
use rhood_core::models::order::{
    OptionOrderSummary, OrderIntent, OrderSummary, Side, StagedOrder, StockOrderSummary,
    SubmittedOrder, TimeInForce,
};
use rhood_core::{RhoodError, RobinhoodClient};
use rmcp::model::{
    CallToolResponse, CallToolResult, ContentBlock, ElicitRequest, ElicitRequestParams,
    ElicitResult, ElicitationAction, ElicitationSchema, Extensions, InputRequest,
    InputRequiredResult, InputResponses, RequestStateCodec, RequestStateError, SealOptions,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::middleware::oauth::{OAuthAuthenticated, OAuthPrincipal};

const CONFIRMATION_REQUEST_ID: &str = "trade_confirmation";
const STDIO_PRINCIPAL: &str = "transport:stdio";
const NON_OAUTH_HTTP_PRINCIPAL: &str = "transport:http-non-oauth";
pub(super) const ORDER_CONFIRMATION_TTL: Duration = Duration::from_secs(600);
pub(super) const ORDER_REDEMPTION_RETENTION: Duration = Duration::from_secs(1_200);

/// One MCP order-confirmation round supplied by the tool adapter.
pub(super) struct ConfirmationRound {
    pub(super) request_state: Option<String>,
    pub(super) input_responses: Option<InputResponses>,
}

#[derive(Debug)]
pub(super) enum ConfirmationError {
    MissingRoundData,
    MalformedResponse,
    Declined,
    Cancelled,
    Expired,
    InvalidState,
    AlreadyConsumed,
    StateUnavailable,
    PrincipalUnavailable,
    PreparationFailure(RhoodError),
    SubmissionFailure(RhoodError),
    InternalFailure(String),
}

/// Owns the complete MCP trade Confirmation state machine.
pub(super) struct OrderConfirmation {
    codec: RequestStateCodec,
    redemptions: Arc<OrderRedemptionStore>,
    ttl: Duration,
}

pub(super) fn process_order_confirmation() -> Arc<OrderConfirmation> {
    static CONFIRMATION: OnceLock<Arc<OrderConfirmation>> = OnceLock::new();
    Arc::clone(CONFIRMATION.get_or_init(|| {
        let mut key = Vec::with_capacity(32);
        key.extend_from_slice(Uuid::new_v4().as_bytes());
        key.extend_from_slice(Uuid::new_v4().as_bytes());
        Arc::new(OrderConfirmation {
            codec: RequestStateCodec::new(key),
            redemptions: Arc::new(OrderRedemptionStore::new(ORDER_REDEMPTION_RETENTION)),
            ttl: ORDER_CONFIRMATION_TTL,
        })
    }))
}

impl OrderConfirmation {
    pub(super) async fn advance(
        &self,
        client: &RobinhoodClient,
        extensions: &Extensions,
        tool_name: &str,
        intent: OrderIntent,
        round: ConfirmationRound,
    ) -> Result<CallToolResponse, ConfirmationError> {
        let principal = request_principal(extensions)?;
        self.advance_with(
            &principal,
            tool_name,
            intent,
            round,
            |intent| async move { client.prepare_order(intent).await },
            |staged| async move { client.submit_order(staged).await },
        )
        .await
    }

    async fn advance_with<P, PFut, S, SFut>(
        &self,
        principal: &str,
        tool_name: &str,
        intent: OrderIntent,
        round: ConfirmationRound,
        prepare: P,
        submit: S,
    ) -> Result<CallToolResponse, ConfirmationError>
    where
        P: FnOnce(OrderIntent) -> PFut,
        PFut: Future<Output = Result<StagedOrder, RhoodError>>,
        S: FnOnce(StagedOrder) -> SFut,
        SFut: Future<Output = Result<SubmittedOrder, RhoodError>>,
    {
        let binding = associated_data(principal, tool_name, &intent)?;
        let Some(request_state) = round.request_state else {
            if round.input_responses.is_some() {
                return Err(ConfirmationError::MissingRoundData);
            }
            let staged = prepare(intent)
                .await
                .map_err(ConfirmationError::PreparationFailure)?;
            let summary = render_summary(staged.summary());
            let ttl = effective_ttl(&staged, self.ttl)?;
            return self.stage(&binding, &staged, summary, ttl);
        };

        let action = match parse_action(round.input_responses)? {
            action @ (ElicitationAction::Accept
            | ElicitationAction::Decline
            | ElicitationAction::Cancel) => action,
            _ => return Err(ConfirmationError::MalformedResponse),
        };
        let staged: StagedOrder = self
            .codec
            .open_json_with(&request_state, &binding)
            .map_err(|error| request_state_error(&error))?;
        self.redemptions.consume(&request_state)?;
        match action {
            ElicitationAction::Accept => {}
            ElicitationAction::Decline => return Err(ConfirmationError::Declined),
            ElicitationAction::Cancel => return Err(ConfirmationError::Cancelled),
            _ => return Err(ConfirmationError::MalformedResponse),
        }

        let summary = render_summary(staged.summary());
        let order = submit(staged)
            .await
            .map_err(ConfirmationError::SubmissionFailure)?;
        let result = serde_json::json!({
            "order_id": order.id(),
            "state": order.state(),
            "status": "submitted",
            "summary": summary,
        });
        let content = serde_json::to_string_pretty(&result)
            .map_err(|error| ConfirmationError::InternalFailure(error.to_string()))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(content)]).into())
    }

    fn stage(
        &self,
        associated_data: &[u8],
        staged: &StagedOrder,
        summary: String,
        ttl: Duration,
    ) -> Result<CallToolResponse, ConfirmationError> {
        let request_state = self
            .codec
            .seal_json_with(
                staged,
                &SealOptions::new().associated_data(associated_data).ttl(ttl),
            )
            .map_err(|error| ConfirmationError::InternalFailure(error.to_string()))?;
        let requested_schema = ElicitationSchema::builder()
            .build()
            .map_err(|error| ConfirmationError::InternalFailure(error.to_string()))?;
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
}

fn request_principal(extensions: &Extensions) -> Result<String, ConfirmationError> {
    let Some(parts) = extensions.get::<Parts>() else {
        return Ok(STDIO_PRINCIPAL.to_string());
    };
    if let Some(authorization) = parts.extensions.get::<OAuthPrincipal>()
        && !authorization.principal.is_empty()
    {
        return Ok(authorization.principal.clone());
    }
    if parts.extensions.get::<OAuthAuthenticated>().is_some() {
        return Err(ConfirmationError::PrincipalUnavailable);
    }
    Ok(NON_OAUTH_HTTP_PRINCIPAL.to_string())
}

fn associated_data(
    principal: &str,
    tool_name: &str,
    intent: &OrderIntent,
) -> Result<Vec<u8>, ConfirmationError> {
    let request = serde_json::to_vec(intent)
        .map_err(|error| ConfirmationError::InternalFailure(error.to_string()))?;
    let request_digest = Sha256::digest(request);
    let principal_len = u64::try_from(principal.len())
        .map_err(|error| ConfirmationError::InternalFailure(error.to_string()))?;
    let tool_name_len = u64::try_from(tool_name.len())
        .map_err(|error| ConfirmationError::InternalFailure(error.to_string()))?;
    let mut binding = Vec::new();
    binding.extend_from_slice(&principal_len.to_be_bytes());
    binding.extend_from_slice(principal.as_bytes());
    binding.extend_from_slice(&tool_name_len.to_be_bytes());
    binding.extend_from_slice(tool_name.as_bytes());
    binding.extend_from_slice(&request_digest);
    Ok(binding)
}

fn effective_ttl(
    staged: &StagedOrder,
    configured_ttl: Duration,
) -> Result<Duration, ConfirmationError> {
    let remaining_ms = staged
        .expires_at_unix_ms()
        .checked_sub(jiff::Timestamp::now().as_millisecond())
        .ok_or_else(|| {
            ConfirmationError::InternalFailure(
                "Staged order expiry exceeds the server clock range".to_string(),
            )
        })?;
    let remaining_ms = u64::try_from(remaining_ms).map_err(|_error| ConfirmationError::Expired)?;
    Ok(configured_ttl.min(Duration::from_millis(remaining_ms)))
}

fn parse_action(
    input_responses: Option<InputResponses>,
) -> Result<ElicitationAction, ConfirmationError> {
    let responses = input_responses.ok_or(ConfirmationError::MissingRoundData)?;
    let response = responses
        .get(CONFIRMATION_REQUEST_ID)
        .ok_or(ConfirmationError::MissingRoundData)?;
    let elicitation: ElicitResult = serde_json::from_value(response.clone())
        .map_err(|_error| ConfirmationError::MalformedResponse)?;
    Ok(elicitation.action)
}

fn render_summary(summary: &OrderSummary) -> String {
    match summary {
        OrderSummary::Stock(summary) => render_stock_summary(summary),
        OrderSummary::Option(summary) => render_option_summary(summary),
    }
}

fn render_stock_summary(summary: &StockOrderSummary) -> String {
    let requested = summary.requested_dollar_amount.as_ref().map_or_else(
        || format!("{} shares", summary.quantity),
        |dollars| format!("${dollars}, resolved to {} shares", summary.quantity),
    );
    let price = summary
        .price
        .as_deref()
        .map_or_else(|| "not applicable".to_string(), |price| format!("${price}"));
    let quote = match (&summary.bid_price, &summary.ask_price) {
        (Some(bid), Some(ask)) => format!("bid=${bid}, ask=${ask}"),
        _ => "not applicable".to_string(),
    };
    format!(
        "Confirm staged stock order: account={}; side={}; amount={requested}; symbol={}; instrument={}; order type={}; submission price={price}; trigger={}; stop price={}; time in force={}; market hours={}; extended hours={}; quote={quote}; client ref={}; form version={}",
        summary.account_label,
        side_name(summary.side),
        summary.symbol,
        summary.instrument,
        summary.order_type,
        summary.trigger,
        summary.stop_price.as_deref().unwrap_or("not applicable"),
        time_in_force_name(summary.time_in_force),
        summary.market_hours,
        summary.extended_hours,
        summary.client_ref,
        summary.order_form_version,
    )
}

fn render_option_summary(summary: &OptionOrderSummary) -> String {
    format!(
        "Confirm staged option order: account={}; side={}; quantity={} contracts; symbol={}; option contract={}; option type={}; strike price=${}; expiration={}; order type={}; trigger={}; limit price=${}; position effect={}; credit or debit={}; time in force={}; client ref={}",
        summary.account_label,
        side_name(summary.side),
        summary.quantity,
        summary.symbol,
        summary.option_contract,
        summary.option_type,
        summary.strike_price,
        summary.expiration_date,
        summary.order_type,
        summary.trigger,
        summary.limit_price,
        summary.position_effect,
        summary.direction,
        time_in_force_name(summary.time_in_force),
        summary.client_ref,
    )
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

fn time_in_force_name(time_in_force: TimeInForce) -> &'static str {
    match time_in_force {
        TimeInForce::Gfd => "gfd (good for day)",
        TimeInForce::Gtc => "gtc (good till cancelled)",
    }
}

fn request_state_error(error: &RequestStateError) -> ConfirmationError {
    match error {
        RequestStateError::Expired => ConfirmationError::Expired,
        _ => ConfirmationError::InvalidState,
    }
}

struct OrderRedemptionStore {
    retention: Duration,
    redeemed: Mutex<HashMap<[u8; 32], Instant>>,
}

impl OrderRedemptionStore {
    fn new(retention: Duration) -> Self {
        Self {
            retention,
            redeemed: Mutex::new(HashMap::new()),
        }
    }

    fn consume(&self, request_state: &str) -> Result<(), ConfirmationError> {
        self.consume_at(request_state, Instant::now())
    }

    fn consume_at(&self, request_state: &str, now: Instant) -> Result<(), ConfirmationError> {
        let digest: [u8; 32] = Sha256::digest(request_state.as_bytes()).into();
        let mut redeemed = self.redeemed.lock().map_err(|error| {
            tracing::error!(%error, "Confirmation redemption store lock is unavailable");
            ConfirmationError::StateUnavailable
        })?;
        let expires_at = now
            .checked_add(self.retention)
            .ok_or(ConfirmationError::StateUnavailable)?;
        redeemed.retain(|_digest, recorded_expiry| *recorded_expiry > now);
        match redeemed.entry(digest) {
            Entry::Occupied(_) => Err(ConfirmationError::AlreadyConsumed),
            Entry::Vacant(entry) => {
                entry.insert(expires_at);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::http::Request;
    use rhood_core::RhoodConfig;
    use rhood_core::models::order::{
        MarketHours, OptionOrderIntent, OptionPositionEffect, OrderAmount, OrderType,
        StockOrderIntent, Trigger,
    };
    use rmcp::model::{Extensions, InputRequiredResult};
    use tokio::sync::Barrier;

    use super::*;

    fn confirmation(ttl: Duration) -> Arc<OrderConfirmation> {
        Arc::new(OrderConfirmation {
            codec: RequestStateCodec::new(b"confirmation-test-key"),
            redemptions: Arc::new(OrderRedemptionStore::new(Duration::from_secs(1_200))),
            ttl,
        })
    }

    fn stock_intent_for(symbol: &str) -> OrderIntent {
        OrderIntent::Stock(StockOrderIntent {
            account_number: Some("ABC123".to_string()),
            symbol: symbol.to_string(),
            amount: OrderAmount::Quantity(2.0),
            side: Side::Buy,
            order_type: OrderType::Limit,
            limit_price: Some(187.25),
            trigger: Trigger::Immediate,
            stop_price: None,
            time_in_force: TimeInForce::Gtc,
            market_hours: MarketHours::RegularHours,
        })
    }

    fn stock_intent() -> OrderIntent {
        stock_intent_for("AAPL")
    }

    fn option_intent() -> OrderIntent {
        OrderIntent::Option(OptionOrderIntent {
            account_number: Some("ABC123".to_string()),
            symbol: "AAPL".to_string(),
            expiration_date: "2026-09-18".to_string(),
            strike_price: 200.0,
            option_type: "call".to_string(),
            side: Side::Buy,
            quantity: 1.0,
            limit_price: 2.5,
            position_effect: OptionPositionEffect::Open,
            time_in_force: TimeInForce::Gfd,
        })
    }

    fn staged_stock(lifetime: Duration) -> StagedOrder {
        staged_stock_for(
            lifetime,
            "AAPL",
            "https://api.robinhood.com/instruments/instrument-aapl/",
            "stock-client-ref",
        )
    }

    fn staged_stock_for(
        lifetime: Duration,
        symbol: &str,
        instrument: &str,
        client_ref: &str,
    ) -> StagedOrder {
        let now = jiff::Timestamp::now().as_millisecond();
        let lifetime_ms = i64::try_from(lifetime.as_millis()).unwrap();
        serde_json::from_value(serde_json::json!({
            "payload": {
                "kind": "Stock",
                "payload": {
                    "account": "https://api.robinhood.com/accounts/ABC123/",
                    "instrument": instrument,
                    "symbol": symbol,
                    "quantity": "2",
                    "side": "buy",
                    "type": "limit",
                    "time_in_force": "gtc",
                    "trigger": "immediate",
                    "market_hours": "regular_hours",
                    "extended_hours": false,
                    "price": "187.25",
                    "override_day_trade_checks": false,
                    "override_dtbp_checks": false,
                    "ref_id": client_ref,
                    "order_form_version": 7
                }
            },
            "summary": {
                "kind": "Stock",
                "summary": {
                    "account_number": "ABC123",
                    "account_label": "individual ****C123",
                    "symbol": symbol,
                    "instrument": instrument,
                    "quantity": "2",
                    "side": "buy",
                    "order_type": "limit",
                    "time_in_force": "gtc",
                    "trigger": "immediate",
                    "market_hours": "regular_hours",
                    "extended_hours": false,
                    "price": "187.25",
                    "stop_price": null,
                    "ask_price": null,
                    "bid_price": null,
                    "requested_dollar_amount": null,
                    "conversion_price": null,
                    "client_ref": client_ref,
                    "order_form_version": 7
                }
            },
            "prepared_at_unix_ms": now,
            "expires_at_unix_ms": now.saturating_add(lifetime_ms)
        }))
        .unwrap()
    }

    fn staged_option(lifetime: Duration) -> StagedOrder {
        let now = jiff::Timestamp::now().as_millisecond();
        let lifetime_ms = i64::try_from(lifetime.as_millis()).unwrap();
        serde_json::from_value(serde_json::json!({
            "payload": {
                "kind": "Option",
                "payload": {
                    "account": "https://api.robinhood.com/accounts/ABC123/",
                    "direction": "debit",
                    "time_in_force": "gfd",
                    "legs": [{
                        "position_effect": "open",
                        "side": "buy",
                        "ratio_quantity": 1,
                        "option": "https://api.robinhood.com/options/instruments/contract-aapl/"
                    }],
                    "type": "limit",
                    "trigger": "immediate",
                    "price": "2.50",
                    "quantity": "1",
                    "override_day_trade_checks": false,
                    "override_dtbp_checks": false,
                    "ref_id": "option-client-ref"
                }
            },
            "summary": {
                "kind": "Option",
                "summary": {
                    "account_number": "ABC123",
                    "account_label": "individual ****C123",
                    "symbol": "AAPL",
                    "option_contract": "https://api.robinhood.com/options/instruments/contract-aapl/",
                    "expiration_date": "2026-09-18",
                    "strike_price": "200.0000",
                    "option_type": "call",
                    "side": "buy",
                    "position_effect": "open",
                    "direction": "debit",
                    "quantity": "1",
                    "limit_price": "2.50",
                    "order_type": "limit",
                    "trigger": "immediate",
                    "time_in_force": "gfd",
                    "client_ref": "option-client-ref"
                }
            },
            "prepared_at_unix_ms": now,
            "expires_at_unix_ms": now.saturating_add(lifetime_ms)
        }))
        .unwrap()
    }

    fn submitted_stock() -> SubmittedOrder {
        serde_json::from_value(serde_json::json!({
            "kind": "Stock",
            "order": {"id": "stock-order-id", "state": "queued"}
        }))
        .unwrap()
    }

    fn submitted_option() -> SubmittedOrder {
        serde_json::from_value(serde_json::json!({
            "kind": "Option",
            "order": {"id": "option-order-id", "state": "queued"}
        }))
        .unwrap()
    }

    fn response(action: ElicitationAction) -> InputResponses {
        let mut responses = InputResponses::new();
        responses.insert(
            CONFIRMATION_REQUEST_ID.to_string(),
            serde_json::to_value(ElicitResult::new(action)).unwrap(),
        );
        responses
    }

    fn state(response: CallToolResponse) -> String {
        let CallToolResponse::InputRequired(InputRequiredResult { request_state, .. }) = response
        else {
            panic!("expected input_required");
        };
        request_state.expect("staged order carries request state")
    }

    async fn stage_stock(confirmation: &OrderConfirmation) -> String {
        let staged = confirmation
            .advance_with(
                "principal-a",
                "place_stock_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: None,
                    input_responses: None,
                },
                |_| async { Ok(staged_stock(Duration::from_secs(60))) },
                |_| async { panic!("staging must not submit") },
            )
            .await
            .unwrap();
        state(staged)
    }

    #[test]
    fn redemption_pruning_preserves_live_records() {
        assert!(ORDER_REDEMPTION_RETENTION > ORDER_CONFIRMATION_TTL);
        let store = OrderRedemptionStore::new(Duration::from_secs(10));
        let started_at = Instant::now();
        let live_at = started_at + Duration::from_secs(5);
        let prune_at = started_at + Duration::from_secs(11);

        store.consume_at("expired", started_at).unwrap();
        store.consume_at("live", live_at).unwrap();
        store.consume_at("trigger", prune_at).unwrap();
        store.consume_at("expired", prune_at).unwrap();
        assert!(matches!(
            store.consume_at("live", prune_at).unwrap_err(),
            ConfirmationError::AlreadyConsumed
        ));
    }

    #[test]
    fn redemption_retention_clock_overflow_is_state_unavailable() {
        let store = OrderRedemptionStore::new(Duration::MAX);
        assert!(matches!(
            store.consume_at("state", Instant::now()),
            Err(ConfirmationError::StateUnavailable)
        ));
    }

    #[test]
    fn stock_summary_contains_all_execution_terms() {
        let summary = StockOrderSummary {
            account_number: "ABC123".to_string(),
            account_label: "individual ****C123".to_string(),
            symbol: "AAPL".to_string(),
            instrument: "https://api.robinhood.com/instruments/instrument-aapl/".to_string(),
            quantity: "2".to_string(),
            side: Side::Buy,
            order_type: rhood_core::models::order::OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            trigger: rhood_core::models::order::Trigger::Immediate,
            market_hours: rhood_core::models::order::MarketHours::RegularHours,
            extended_hours: false,
            price: Some("187.25".to_string()),
            stop_price: None,
            ask_price: None,
            bid_price: None,
            requested_dollar_amount: None,
            conversion_price: None,
            client_ref: "client-ref".to_string(),
            order_form_version: 7,
        };
        let message = render_stock_summary(&summary);
        for expected in [
            "account=individual ****C123",
            "side=buy",
            "amount=2 shares",
            "symbol=AAPL",
            "instrument=https://api.robinhood.com/instruments/instrument-aapl/",
            "order type=limit",
            "submission price=$187.25",
            "time in force=gtc (good till cancelled)",
            "client ref=client-ref",
            "form version=7",
        ] {
            assert!(
                message.contains(expected),
                "missing {expected:?}: {message}"
            );
        }
    }

    /// Compile-forcing guard: destructures `StockOrderSummary` without `..`, so adding a
    /// field to the struct fails to compile here until someone decides whether the MCP
    /// confirmation renderer surfaces it. `render_stock_summary` already renders the stop
    /// price correctly; this test exists so a future change cannot silently regress that.
    #[test]
    fn render_stock_summary_covers_every_execution_changing_field() {
        // Fixture values are deliberately long and mutually non-overlapping (no value is
        // a substring of any other, or of the fixed template text) so each `contains`
        // assertion below can only be satisfied by the renderer actually emitting that
        // field -- a short value like "2" or "7" would let coincidental digits elsewhere
        // in the message (e.g. inside the account label) satisfy the assertion for free.
        let summary = StockOrderSummary {
            account_number: "ACCT-JJ88TT".to_string(),
            account_label: "individual-brokerage-nova".to_string(),
            symbol: "ZZQQ".to_string(),
            instrument: "https://api.robinhood.com/instruments/instrument-zzqq/".to_string(),
            quantity: "137.428".to_string(),
            side: Side::Sell,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            trigger: Trigger::Stop,
            market_hours: MarketHours::RegularHours,
            extended_hours: false,
            price: Some("1841.11".to_string()),
            stop_price: Some("9502.75".to_string()),
            ask_price: Some("1842.63".to_string()),
            bid_price: Some("1840.07".to_string()),
            requested_dollar_amount: Some("25317.99".to_string()),
            conversion_price: Some("1843.29".to_string()),
            client_ref: "cli-ref-7f3a9e".to_string(),
            order_form_version: 231,
        };

        let StockOrderSummary {
            // The raw account number is intentionally never shown; the masked
            // `account_label` is the safe human-facing identifier and is asserted below.
            account_number: ref _account_number,
            ref account_label,
            ref symbol,
            ref instrument,
            ref quantity,
            side,
            order_type,
            time_in_force,
            trigger,
            market_hours,
            extended_hours,
            ref price,
            ref stop_price,
            ref ask_price,
            ref bid_price,
            ref requested_dollar_amount,
            // The conversion price is derived context for the dollar->share conversion,
            // not an execution term of the submitted order itself; the confirmation does
            // not currently surface it.
            conversion_price: ref _conversion_price,
            ref client_ref,
            order_form_version,
        } = summary;

        let message = render_stock_summary(&summary);

        assert!(message.contains(account_label), "message: {message}");
        assert!(message.contains(symbol), "message: {message}");
        assert!(message.contains(instrument), "message: {message}");
        assert!(message.contains(quantity), "message: {message}");
        assert!(message.contains(side_name(side)), "message: {message}");
        assert!(
            message.contains(&order_type.to_string()),
            "message: {message}"
        );
        assert!(
            message.contains(time_in_force_name(time_in_force)),
            "message: {message}"
        );
        assert!(message.contains(&trigger.to_string()), "message: {message}");
        assert!(
            message.contains(&market_hours.to_string()),
            "message: {message}"
        );
        assert!(
            message.contains(&extended_hours.to_string()),
            "message: {message}"
        );
        assert!(
            message.contains(price.as_deref().expect("price set in fixture")),
            "message missing price: {message}"
        );
        assert!(
            message.contains(stop_price.as_deref().expect("stop_price set in fixture")),
            "message missing stop price: {message}"
        );
        assert!(
            message.contains(ask_price.as_deref().expect("ask_price set in fixture")),
            "message missing ask price: {message}"
        );
        assert!(
            message.contains(bid_price.as_deref().expect("bid_price set in fixture")),
            "message missing bid price: {message}"
        );
        assert!(
            message.contains(
                requested_dollar_amount
                    .as_deref()
                    .expect("requested_dollar_amount set in fixture")
            ),
            "message missing requested dollar amount: {message}"
        );
        assert!(message.contains(client_ref), "message: {message}");
        assert!(
            message.contains(&order_form_version.to_string()),
            "message: {message}"
        );
    }

    #[tokio::test]
    async fn accepted_confirmation_submits_once_and_replay_fails() {
        let confirmation = confirmation(ORDER_CONFIRMATION_TTL);
        let request_state = stage_stock(&confirmation).await;
        let submissions = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&submissions);
        let result = confirmation
            .advance_with(
                "principal-a",
                "place_stock_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: Some(request_state.clone()),
                    input_responses: Some(response(ElicitationAction::Accept)),
                },
                |_| async { panic!("resumed round must not prepare") },
                move |_| async move {
                    counted.fetch_add(1, Ordering::SeqCst);
                    Ok(submitted_stock())
                },
            )
            .await
            .unwrap();
        assert!(matches!(result, CallToolResponse::Complete(_)));

        let error = confirmation
            .advance_with(
                "principal-a",
                "place_stock_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: Some(request_state),
                    input_responses: Some(response(ElicitationAction::Accept)),
                },
                |_| async { panic!("replay must not prepare") },
                |_| async { panic!("replay must not submit") },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, ConfirmationError::AlreadyConsumed));
        assert_eq!(submissions.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn missing_round_state_or_response_is_typed_and_does_not_submit() {
        let confirmation = confirmation(ORDER_CONFIRMATION_TTL);
        let missing_state = confirmation
            .advance_with(
                "principal-a",
                "place_stock_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: None,
                    input_responses: Some(response(ElicitationAction::Accept)),
                },
                |_| async { panic!("response without state must not prepare") },
                |_| async { panic!("response without state must not submit") },
            )
            .await
            .unwrap_err();
        assert!(matches!(missing_state, ConfirmationError::MissingRoundData));

        let request_state = stage_stock(&confirmation).await;
        for input_responses in [None, Some(InputResponses::new())] {
            let missing_response = confirmation
                .advance_with(
                    "principal-a",
                    "place_stock_order",
                    stock_intent(),
                    ConfirmationRound {
                        request_state: Some(request_state.clone()),
                        input_responses,
                    },
                    |_| async { panic!("resumed round must not prepare") },
                    |_| async { panic!("missing response must not submit") },
                )
                .await
                .unwrap_err();
            assert!(matches!(
                missing_response,
                ConfirmationError::MissingRoundData
            ));
        }

        confirmation
            .advance_with(
                "principal-a",
                "place_stock_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: Some(request_state),
                    input_responses: Some(response(ElicitationAction::Accept)),
                },
                |_| async { panic!("resumed round must not prepare") },
                |_| async { Ok(submitted_stock()) },
            )
            .await
            .expect("corrected response submits");
    }

    #[tokio::test]
    async fn preparation_failure_preserves_typed_core_source() {
        let error = confirmation(ORDER_CONFIRMATION_TTL)
            .advance_with(
                "principal-a",
                "place_stock_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: None,
                    input_responses: None,
                },
                |_| async { Err(RhoodError::InvalidOrder("quantity is invalid".to_string())) },
                |_| async { panic!("failed preparation must not submit") },
            )
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            ConfirmationError::PreparationFailure(RhoodError::InvalidOrder(ref message))
                if message == "quantity is invalid"
        ));
    }

    #[tokio::test]
    async fn failed_submission_still_consumes_confirmation() {
        let confirmation = confirmation(ORDER_CONFIRMATION_TTL);
        let request_state = stage_stock(&confirmation).await;
        let error = confirmation
            .advance_with(
                "principal-a",
                "place_stock_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: Some(request_state.clone()),
                    input_responses: Some(response(ElicitationAction::Accept)),
                },
                |_| async { panic!("resumed round must not prepare") },
                |_| async {
                    Err(RhoodError::SubmissionUncertain {
                        client_ref: "stock-client-ref".to_string(),
                        message: "connection closed".to_string(),
                    })
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ConfirmationError::SubmissionFailure(RhoodError::SubmissionUncertain {
                ref client_ref,
                ..
            }) if client_ref == "stock-client-ref"
        ));

        let replay = confirmation
            .advance_with(
                "principal-a",
                "place_stock_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: Some(request_state),
                    input_responses: Some(response(ElicitationAction::Accept)),
                },
                |_| async { panic!("replay must not prepare") },
                |_| async { panic!("failed submission state must stay consumed") },
            )
            .await
            .unwrap_err();
        assert!(matches!(replay, ConfirmationError::AlreadyConsumed));
    }

    #[tokio::test]
    async fn decline_and_cancel_are_terminal_and_consume_state() {
        for action in [ElicitationAction::Decline, ElicitationAction::Cancel] {
            let confirmation = confirmation(ORDER_CONFIRMATION_TTL);
            let request_state = stage_stock(&confirmation).await;
            let error = confirmation
                .advance_with(
                    "principal-a",
                    "place_stock_order",
                    stock_intent(),
                    ConfirmationRound {
                        request_state: Some(request_state.clone()),
                        input_responses: Some(response(action.clone())),
                    },
                    |_| async { panic!("terminal round must not prepare") },
                    |_| async { panic!("decline or cancel must not submit") },
                )
                .await
                .unwrap_err();
            assert!(matches!(
                (action, error),
                (ElicitationAction::Decline, ConfirmationError::Declined)
                    | (ElicitationAction::Cancel, ConfirmationError::Cancelled)
            ));

            let replay = confirmation
                .advance_with(
                    "principal-a",
                    "place_stock_order",
                    stock_intent(),
                    ConfirmationRound {
                        request_state: Some(request_state),
                        input_responses: Some(response(ElicitationAction::Accept)),
                    },
                    |_| async { panic!("replay must not prepare") },
                    |_| async { panic!("consumed terminal state must not submit") },
                )
                .await
                .unwrap_err();
            assert!(matches!(replay, ConfirmationError::AlreadyConsumed));
        }
    }

    #[tokio::test]
    async fn malformed_response_is_correctable_without_consuming_state() {
        let confirmation = confirmation(ORDER_CONFIRMATION_TTL);
        let request_state = stage_stock(&confirmation).await;
        let mut malformed = InputResponses::new();
        malformed.insert(
            CONFIRMATION_REQUEST_ID.to_string(),
            serde_json::json!({"wrong": "shape"}),
        );
        let error = confirmation
            .advance_with(
                "principal-a",
                "place_stock_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: Some(request_state.clone()),
                    input_responses: Some(malformed),
                },
                |_| async { panic!("resumed round must not prepare") },
                |_| async { panic!("malformed response must not submit") },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, ConfirmationError::MalformedResponse));

        confirmation
            .advance_with(
                "principal-a",
                "place_stock_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: Some(request_state),
                    input_responses: Some(response(ElicitationAction::Accept)),
                },
                |_| async { panic!("resumed round must not prepare") },
                |_| async { Ok(submitted_stock()) },
            )
            .await
            .expect("corrected response submits");
    }

    #[tokio::test]
    async fn tamper_wrong_principal_and_changed_intent_fail_closed() {
        let confirmation = confirmation(ORDER_CONFIRMATION_TTL);
        let request_state = stage_stock(&confirmation).await;
        let mut tampered = request_state.clone();
        tampered.push('A');
        let changed = stock_intent_for("MSFT");

        for (principal, intent, state_value) in [
            ("principal-a", stock_intent(), tampered),
            ("principal-b", stock_intent(), request_state.clone()),
            ("principal-a", changed, request_state),
        ] {
            let error = confirmation
                .advance_with(
                    principal,
                    "place_stock_order",
                    intent,
                    ConfirmationRound {
                        request_state: Some(state_value),
                        input_responses: Some(response(ElicitationAction::Accept)),
                    },
                    |_| async { panic!("invalid state must not prepare") },
                    |_| async { panic!("invalid state must not submit") },
                )
                .await
                .unwrap_err();
            assert!(matches!(error, ConfirmationError::InvalidState));
        }
    }

    #[tokio::test]
    async fn confirmation_state_is_bound_to_its_write_tool() {
        let confirmation = confirmation(ORDER_CONFIRMATION_TTL);
        let request_state = stage_stock(&confirmation).await;
        let submissions = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&submissions);

        let error = confirmation
            .advance_with(
                "principal-a",
                "place_option_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: Some(request_state.clone()),
                    input_responses: Some(response(ElicitationAction::Accept)),
                },
                |_| async { panic!("wrong tool must not prepare") },
                move |_| async move {
                    counted.fetch_add(1, Ordering::SeqCst);
                    Ok(submitted_stock())
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, ConfirmationError::InvalidState));
        assert_eq!(submissions.load(Ordering::SeqCst), 0);

        let counted = Arc::clone(&submissions);
        confirmation
            .advance_with(
                "principal-a",
                "place_stock_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: Some(request_state),
                    input_responses: Some(response(ElicitationAction::Accept)),
                },
                |_| async { panic!("resumed round must not prepare") },
                move |_| async move {
                    counted.fetch_add(1, Ordering::SeqCst);
                    Ok(submitted_stock())
                },
            )
            .await
            .expect("the correctly bound state remains confirmable");
        assert_eq!(submissions.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn confirming_one_staged_order_preserves_another_staged_order() {
        let confirmation = confirmation(ORDER_CONFIRMATION_TTL);
        let first_staged = staged_stock_for(
            Duration::from_secs(60),
            "AAPL",
            "https://api.robinhood.com/instruments/instrument-aapl/",
            "aapl-client-ref",
        );
        let first_expected = serde_json::to_value(&first_staged).unwrap();
        let first_state = confirmation
            .advance_with(
                "principal-a",
                "place_stock_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: None,
                    input_responses: None,
                },
                |_| async move { Ok(first_staged) },
                |_| async { panic!("staging must not submit") },
            )
            .await
            .map(state)
            .unwrap();
        let second_staged = staged_stock_for(
            Duration::from_secs(60),
            "MSFT",
            "https://api.robinhood.com/instruments/instrument-msft/",
            "msft-client-ref",
        );
        let second_expected = serde_json::to_value(&second_staged).unwrap();
        let second_state = confirmation
            .advance_with(
                "principal-a",
                "place_stock_order",
                stock_intent_for("MSFT"),
                ConfirmationRound {
                    request_state: None,
                    input_responses: None,
                },
                |_| async move { Ok(second_staged) },
                |_| async { panic!("staging must not submit") },
            )
            .await
            .map(state)
            .unwrap();
        let submissions = Arc::new(AtomicUsize::new(0));

        let counted = Arc::clone(&submissions);
        confirmation
            .advance_with(
                "principal-a",
                "place_stock_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: Some(first_state),
                    input_responses: Some(response(ElicitationAction::Accept)),
                },
                |_| async { panic!("resumed round must not prepare") },
                move |staged| async move {
                    assert_eq!(serde_json::to_value(staged).unwrap(), first_expected);
                    counted.fetch_add(1, Ordering::SeqCst);
                    Ok(submitted_stock())
                },
            )
            .await
            .expect("the first staged order submits with its sealed identity");

        let counted = Arc::clone(&submissions);
        confirmation
            .advance_with(
                "principal-a",
                "place_stock_order",
                stock_intent_for("MSFT"),
                ConfirmationRound {
                    request_state: Some(second_state),
                    input_responses: Some(response(ElicitationAction::Accept)),
                },
                |_| async { panic!("resumed round must not prepare") },
                move |staged| async move {
                    assert_eq!(serde_json::to_value(staged).unwrap(), second_expected);
                    counted.fetch_add(1, Ordering::SeqCst);
                    Ok(submitted_stock())
                },
            )
            .await
            .expect("the second staged order remains independently confirmable");
        assert_eq!(submissions.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn advance_consumes_accepted_confirmation_before_real_client_submission() {
        let confirmation = confirmation(ORDER_CONFIRMATION_TTL);
        let staged = confirmation
            .advance_with(
                STDIO_PRINCIPAL,
                "place_stock_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: None,
                    input_responses: None,
                },
                |_| async { Ok(staged_stock(Duration::from_secs(60))) },
                |_| async { panic!("staging must not submit") },
            )
            .await
            .unwrap();
        let request_state = state(staged);
        let client = RobinhoodClient::with_config(RhoodConfig::default()).unwrap();

        let error = confirmation
            .advance(
                &client,
                &Extensions::new(),
                "place_stock_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: Some(request_state.clone()),
                    input_responses: Some(response(ElicitationAction::Accept)),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ConfirmationError::SubmissionFailure(RhoodError::ReadOnlyMode)
        ));

        let replay = confirmation
            .advance_with(
                "transport:stdio",
                "place_stock_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: Some(request_state),
                    input_responses: Some(response(ElicitationAction::Accept)),
                },
                |_| async { panic!("replay must not prepare") },
                |_| async { panic!("accepted state must be consumed before replay") },
            )
            .await
            .unwrap_err();
        assert!(matches!(replay, ConfirmationError::AlreadyConsumed));
    }

    #[tokio::test]
    async fn expired_confirmation_fails_before_submission() {
        let confirmation = confirmation(Duration::ZERO);
        let request_state = stage_stock(&confirmation).await;
        tokio::time::sleep(Duration::from_millis(2)).await;
        let error = confirmation
            .advance_with(
                "principal-a",
                "place_stock_order",
                stock_intent(),
                ConfirmationRound {
                    request_state: Some(request_state),
                    input_responses: Some(response(ElicitationAction::Accept)),
                },
                |_| async { panic!("resumed round must not prepare") },
                |_| async { panic!("expired state must not submit") },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, ConfirmationError::Expired));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_confirmations_submit_exactly_once() {
        let confirmation = confirmation(ORDER_CONFIRMATION_TTL);
        let request_state = stage_stock(&confirmation).await;
        let submissions = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(3));
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let confirmation = Arc::clone(&confirmation);
            let request_state = request_state.clone();
            let submissions = Arc::clone(&submissions);
            let barrier = Arc::clone(&barrier);
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                confirmation
                    .advance_with(
                        "principal-a",
                        "place_stock_order",
                        stock_intent(),
                        ConfirmationRound {
                            request_state: Some(request_state),
                            input_responses: Some(response(ElicitationAction::Accept)),
                        },
                        |_| async { panic!("resumed round must not prepare") },
                        move |_| async move {
                            submissions.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(20)).await;
                            Ok(submitted_stock())
                        },
                    )
                    .await
            }));
        }
        barrier.wait().await;
        let first = tasks.remove(0).await.unwrap();
        let second = tasks.remove(0).await.unwrap();
        assert!(matches!(
            (&first, &second),
            (Ok(_), Err(_)) | (Err(_), Ok(_))
        ));
        assert_eq!(submissions.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn option_staged_order_round_trips_exact_contract_and_summary() {
        let confirmation = confirmation(ORDER_CONFIRMATION_TTL);
        let staged = confirmation
            .advance_with(
                "principal-a",
                "place_option_order",
                option_intent(),
                ConfirmationRound {
                    request_state: None,
                    input_responses: None,
                },
                |_| async { Ok(staged_option(Duration::from_secs(600))) },
                |_| async { panic!("staging must not submit") },
            )
            .await
            .unwrap();
        let request_state = state(staged);
        let inspected = Arc::new(Mutex::new(None));
        let captured = Arc::clone(&inspected);
        confirmation
            .advance_with(
                "principal-a",
                "place_option_order",
                option_intent(),
                ConfirmationRound {
                    request_state: Some(request_state),
                    input_responses: Some(response(ElicitationAction::Accept)),
                },
                |_| async { panic!("resumed round must not prepare") },
                move |staged| async move {
                    *captured.lock().unwrap() = Some(staged.client_reference().to_string());
                    Ok(submitted_option())
                },
            )
            .await
            .unwrap();
        assert_eq!(
            inspected.lock().unwrap().as_deref(),
            Some("option-client-ref")
        );
    }

    #[test]
    fn effective_ttl_never_outlives_staged_order() {
        let staged = staged_stock(Duration::from_secs(60));
        let ttl = effective_ttl(&staged, Duration::from_secs(600)).unwrap();
        assert!(ttl <= Duration::from_secs(60));
        assert!(ttl > Duration::from_secs(59));
    }

    #[test]
    fn principal_selection_is_transport_aware_and_oauth_fails_closed() {
        assert_eq!(
            request_principal(&Extensions::new()).unwrap(),
            STDIO_PRINCIPAL
        );

        let (parts, _) = Request::new(()).into_parts();
        let mut extensions = Extensions::new();
        extensions.insert(parts);
        assert_eq!(
            request_principal(&extensions).unwrap(),
            NON_OAUTH_HTTP_PRINCIPAL
        );

        let mut request = Request::new(());
        request.extensions_mut().insert(OAuthAuthenticated);
        request.extensions_mut().insert(OAuthPrincipal {
            principal: "registered-client".to_string(),
            scopes: vec!["write".to_string()],
        });
        let (parts, _) = request.into_parts();
        let mut extensions = Extensions::new();
        extensions.insert(parts);
        assert_eq!(request_principal(&extensions).unwrap(), "registered-client");

        let mut request = Request::new(());
        request.extensions_mut().insert(OAuthAuthenticated);
        let (parts, _) = request.into_parts();
        let mut extensions = Extensions::new();
        extensions.insert(parts);
        assert!(matches!(
            request_principal(&extensions),
            Err(ConfirmationError::PrincipalUnavailable)
        ));
    }
}
