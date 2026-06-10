use chrono::Utc;
use rhood_core::models::order::{
    MarketHours, OptionOrderRequest, OrderAmount, OrderType, Side, StockOrderRequest, TimeInForce,
    Trigger,
};
use rmcp::{Peer, RoleServer, handler::server::wrapper::Parameters, tool, tool_router};
use uuid::Uuid;

use super::handler::RhoodTools;
use super::params::*;
use super::types::{PendingOrder, PendingOrderKind, format_tool_error};

const MAX_SYMBOL_LENGTH: usize = 6;

/// Validates that a symbol string looks like a valid stock ticker.
/// Does not verify the symbol exists — that happens at confirmation.
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

/// Builds the JSON response returned when a stock or option order is staged.
fn staged_order_response(pending_id: &str, summary: &str, staged_at: &str) -> serde_json::Value {
    serde_json::json!({
        "pending_order_id": pending_id,
        "summary": summary,
        "status": "awaiting_confirmation",
        "staged_at": staged_at,
    })
}

#[tool_router(router = order_router, vis = "pub(super)")]
impl RhoodTools {
    #[tool(
        name = "place_stock_order",
        description = "Stage a stock order for confirmation. Returns pending_order_id - call confirm_order to execute.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn place_stock_order(
        &self,
        _peer: Peer<RoleServer>,
        Parameters(params): Parameters<PlaceStockOrderParams>,
    ) -> Result<String, String> {
        validate_symbol_format(&params.symbol)?;
        let side = params.side;
        let order_type = params.order_type;
        let trigger = params.trigger.unwrap_or(Trigger::Immediate);
        let market_hours = params.market_hours.unwrap_or(MarketHours::RegularHours);
        if order_type == OrderType::Limit && params.limit_price.is_none() {
            return Err("limit_price required for limit orders".into());
        }
        if let Some(q) = params.quantity {
            validate_positive("quantity", q)?;
        }
        if let Some(d) = params.dollar_amount {
            validate_positive("dollar_amount", d)?;
        }
        if let Some(p) = params.limit_price {
            validate_positive("limit_price", p)?;
        }
        if let Some(p) = params.stop_price {
            validate_positive("stop_price", p)?;
        }

        let amount = match (params.quantity, params.dollar_amount) {
            (Some(q), None) => OrderAmount::Quantity(q),
            (None, Some(d)) => OrderAmount::DollarAmount(d),
            (Some(_), Some(_)) => return Err("Specify quantity or dollar_amount, not both".into()),
            (None, None) => return Err("Specify quantity or dollar_amount".into()),
        };

        let request = StockOrderRequest {
            symbol: params.symbol.clone(),
            amount,
            side,
            order_type,
            limit_price: params.limit_price,
            trigger,
            stop_price: params.stop_price,
            time_in_force: TimeInForce::Gtc,
            market_hours,
        };

        let pending_id = Uuid::new_v4().to_string();
        let side_str = if side == Side::Buy { "Buy" } else { "Sell" };
        let type_str = if order_type == OrderType::Market {
            "market".to_string()
        } else {
            format!("limit @ ${:.2}", params.limit_price.unwrap())
        };
        let amount_str = match amount {
            OrderAmount::Quantity(q) => format!("{q}x"),
            OrderAmount::DollarAmount(d) => format!("${d:.2} of"),
        };
        let summary = format!("{side_str} {amount_str} {} at {type_str}", params.symbol);

        let pending = PendingOrder {
            summary: summary.clone(),
            kind: PendingOrderKind::Stock(request),
            created_at: Utc::now(),
        };
        let staged_at = pending.created_at.to_rfc3339();
        self.pending_orders
            .lock()
            .await
            .insert(pending_id.clone(), pending);

        let result = staged_order_response(&pending_id, &summary, &staged_at);
        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())
    }

    #[tool(
        name = "place_option_order",
        description = "Stage an option order for confirmation. Returns pending_order_id - call confirm_order to execute.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn place_option_order(
        &self,
        _peer: Peer<RoleServer>,
        Parameters(params): Parameters<PlaceOptionOrderParams>,
    ) -> Result<String, String> {
        validate_symbol_format(&params.symbol)?;
        validate_positive("strike_price", params.strike_price)?;
        validate_positive("quantity", params.quantity)?;
        validate_positive("limit_price", params.limit_price)?;
        let side = params.side;
        let option_type_str = params.option_type.to_string();
        let (position_effect, credit_or_debit) = match side {
            Side::Buy => ("open".to_string(), "debit".to_string()),
            Side::Sell => ("close".to_string(), "credit".to_string()),
        };

        let request = OptionOrderRequest {
            symbol: params.symbol.clone(),
            expiration_date: params.expiration_date.clone(),
            strike_price: params.strike_price,
            option_type: option_type_str,
            side,
            quantity: params.quantity,
            limit_price: params.limit_price,
            position_effect,
            credit_or_debit,
            time_in_force: TimeInForce::Gtc,
        };

        let pending_id = uuid::Uuid::new_v4().to_string();
        let side_str = if side == Side::Buy { "Buy" } else { "Sell" };
        let summary = format!(
            "{side_str} {}x {} {} ${} {} @ ${:.2}",
            params.quantity,
            params.symbol,
            params.option_type,
            params.strike_price,
            params.expiration_date,
            params.limit_price
        );

        let pending = PendingOrder {
            summary: summary.clone(),
            kind: PendingOrderKind::Option(request),
            created_at: Utc::now(),
        };
        let staged_at = pending.created_at.to_rfc3339();
        self.pending_orders
            .lock()
            .await
            .insert(pending_id.clone(), pending);

        let result = staged_order_response(&pending_id, &summary, &staged_at);
        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())
    }

    #[tool(
        name = "confirm_order",
        description = "Confirm and submit a staged order",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn confirm_order(
        &self,
        peer: Peer<RoleServer>,
        Parameters(params): Parameters<ConfirmOrderParams>,
    ) -> Result<String, String> {
        let pending = self
            .pending_orders
            .lock()
            .await
            .remove(&params.pending_order_id);
        let pending = pending.ok_or("No pending order found with that ID")?;

        let client = self.ensure_client(&peer).await?;
        let order_id = match pending.kind {
            PendingOrderKind::Stock(request) => {
                let order = client
                    .place_stock_order(&request)
                    .await
                    .map_err(|rhood_error| format_tool_error(&rhood_error))?;
                order.id
            }
            PendingOrderKind::Option(request) => {
                let order = client
                    .place_option_order(&request)
                    .await
                    .map_err(|rhood_error| format_tool_error(&rhood_error))?;
                order.id
            }
        };

        let result = serde_json::json!({
            "order_id": order_id,
            "status": "submitted",
            "summary": pending.summary
        });
        serde_json::to_string_pretty(&result).map_err(|error| error.to_string())
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
mod tests {
    use super::{staged_order_response, validate_positive, validate_symbol_format};
    use chrono::Utc;

    #[test]
    fn staged_order_response_has_expected_shape() {
        let staged_at = Utc::now().to_rfc3339();
        let value = staged_order_response("pid-123", "Buy 1x AAPL at market", &staged_at);
        assert_eq!(value["pending_order_id"], "pid-123");
        assert_eq!(value["summary"], "Buy 1x AAPL at market");
        assert_eq!(value["status"], "awaiting_confirmation");
        // staged_at must be a present, parseable RFC3339 timestamp
        let echoed = value["staged_at"]
            .as_str()
            .expect("staged_at must be a string");
        assert_eq!(echoed, staged_at);
        assert!(
            chrono::DateTime::parse_from_rfc3339(echoed).is_ok(),
            "staged_at must be valid RFC3339: {echoed}"
        );
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
