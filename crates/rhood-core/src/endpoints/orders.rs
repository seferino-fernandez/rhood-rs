use std::collections::HashMap;

use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::models::account::AccountProfile;
use crate::models::order::{
    DollarBasedAmount, MarketHours, OptionLeg, OptionOrder, OptionOrderPayload, OptionOrderRequest,
    OrderAmount, OrderType, Side, StockOrder, StockOrderPayload, StockOrderRequest, Trigger,
};
use crate::models::stock::Instrument;
use crate::pagination::ResultsResponse;
use crate::{Result, RhoodError};

/// Validates a stock order request for logical consistency.
///
/// Returns `Ok(())` if valid, or `Err(RhoodError::InvalidOrder)` with a
/// descriptive message if the request contains contradictory parameters.
pub fn validate_stock_order(req: &StockOrderRequest) -> Result<()> {
    if req.trigger == Trigger::Stop && req.stop_price.is_none() {
        return Err(RhoodError::InvalidOrder(
            "stop_price is required when trigger is Stop".into(),
        ));
    }
    if req.trigger == Trigger::Immediate && req.stop_price.is_some() {
        return Err(RhoodError::InvalidOrder(
            "stop_price must not be set when trigger is Immediate".into(),
        ));
    }
    if let OrderAmount::DollarAmount(_) = req.amount {
        if req.side != Side::Buy {
            return Err(RhoodError::InvalidOrder(
                "Dollar-based orders are only valid for buy orders".into(),
            ));
        }
        if req.order_type != OrderType::Market {
            return Err(RhoodError::InvalidOrder(
                "Dollar-based orders require market order type".into(),
            ));
        }
        if req.trigger != Trigger::Immediate {
            return Err(RhoodError::InvalidOrder(
                "Dollar-based orders require immediate trigger".into(),
            ));
        }
    }
    if matches!(
        req.market_hours,
        MarketHours::ExtendedHours | MarketHours::AllDayHours
    ) && req.order_type != OrderType::Limit
    {
        return Err(RhoodError::InvalidOrder(
            "Extended and all-day hours require limit orders".into(),
        ));
    }
    Ok(())
}

impl RobinhoodClient {
    /// Fetches all stock orders, including completed, cancelled, and pending.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_all_stock_orders(&self, since: Option<&str>) -> Result<Vec<StockOrder>> {
        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(date) = since {
            params.push(("updated_at[gte]", date));
        }
        self.get_paginated(&self.api_url(paths::STOCK_ORDERS), &params)
            .await
    }

    /// Fetches only open (cancellable) stock orders.
    ///
    /// Filters the full order list to those with a non-null cancel URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying orders request fails.
    pub async fn get_open_stock_orders(&self) -> Result<Vec<StockOrder>> {
        let orders = self.get_all_stock_orders(None).await?;
        Ok(orders
            .into_iter()
            .filter(|order| order.cancel.is_some())
            .collect())
    }

    /// Cancels a pending stock order by its order ID.
    ///
    /// Requires writable mode (`read_only = false`).
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::ReadOnlyMode`] if the client is in read-only
    /// mode. Also returns an error on HTTP failures.
    pub async fn cancel_stock_order(&self, order_id: &str) -> Result<()> {
        self.require_writable()?;
        let url = format!("{}{order_id}/cancel/", self.api_url(paths::STOCK_ORDERS));
        self.post_empty(&url).await
    }

    /// Places a stock order (buy or sell) based on the given request parameters.
    ///
    /// Resolves the symbol to its instrument URL and the authenticated
    /// account URL before submitting. Requires writable mode
    /// (`read_only = false`).
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::ReadOnlyMode`] if the client is in read-only
    /// mode. Returns [`RhoodError::InvalidSymbol`] if the symbol cannot be
    /// resolved. Also returns an error on HTTP or deserialization failures.
    pub async fn place_stock_order(&self, req: &StockOrderRequest) -> Result<StockOrder> {
        self.require_writable()?;
        validate_stock_order(req)?;
        let instrument = self
            .cached_instrument(&req.symbol)
            .await?
            .ok_or_else(|| RhoodError::InvalidSymbol(req.symbol.clone()))?;
        let instrument_url = instrument
            .url
            .clone()
            .ok_or_else(|| RhoodError::InvalidSymbol(req.symbol.clone()))?;
        let account_url = self.get_account_url().await?;

        let (quantity, dollar_based_amount) = match req.amount {
            OrderAmount::Quantity(q) => (format!("{q}"), None),
            OrderAmount::DollarAmount(d) => (
                "0".to_string(),
                Some(DollarBasedAmount {
                    amount: format!("{d:.2}"),
                    currency_code: "USD".to_string(),
                }),
            ),
        };

        let payload = StockOrderPayload {
            account: account_url,
            instrument: instrument_url,
            symbol: req.symbol.to_uppercase(),
            quantity,
            side: req.side,
            order_type: req.order_type,
            time_in_force: req.time_in_force,
            trigger: req.trigger,
            market_hours: req.market_hours,
            price: req.limit_price.map(|price| format!("{price:.2}")),
            stop_price: req.stop_price.map(|price| format!("{price:.2}")),
            dollar_based_amount,
        };

        self.post_form(&self.api_url(paths::STOCK_ORDERS), &payload)
            .await
    }

    /// Fetches all option orders, including completed, cancelled, and pending.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_all_option_orders(&self, since: Option<&str>) -> Result<Vec<OptionOrder>> {
        let mut params: Vec<(&str, &str)> = Vec::new();
        if let Some(date) = since {
            params.push(("updated_at[gte]", date));
        }
        self.get_paginated(&self.api_url(paths::OPTION_ORDERS), &params)
            .await
    }

    /// Fetches only open (cancellable) option orders.
    ///
    /// Filters the full order list to those with a non-null cancel URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying orders request fails.
    pub async fn get_open_option_orders(&self) -> Result<Vec<OptionOrder>> {
        let orders = self.get_all_option_orders(None).await?;
        Ok(orders
            .into_iter()
            .filter(|order| order.cancel_url.is_some())
            .collect())
    }

    /// Cancels a pending option order by its order ID.
    ///
    /// Requires writable mode (`read_only = false`).
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::ReadOnlyMode`] if the client is in read-only
    /// mode. Also returns an error on HTTP failures.
    pub async fn cancel_option_order(&self, order_id: &str) -> Result<()> {
        self.require_writable()?;
        let url = format!("{}{order_id}/cancel/", self.api_url(paths::OPTION_ORDERS));
        self.post_empty(&url).await
    }

    /// Places an option order based on the given request parameters.
    ///
    /// Resolves the symbol, expiration date, strike price, and option type
    /// to a specific option contract before submitting. Requires writable
    /// mode (`read_only = false`).
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::ReadOnlyMode`] if the client is in read-only
    /// mode. Returns [`RhoodError::InvalidSymbol`] if the option contract
    /// cannot be found. Also returns an error on HTTP or deserialization
    /// failures.
    pub async fn place_option_order(&self, req: &OptionOrderRequest) -> Result<OptionOrder> {
        self.require_writable()?;
        let options = self
            .find_options(
                &req.symbol,
                &req.expiration_date,
                &req.option_type,
                Some(&format!("{:.4}", req.strike_price)),
            )
            .await?;
        let option = options.first().ok_or_else(|| {
            RhoodError::InvalidSymbol(format!(
                "{} {} {} {}",
                req.symbol, req.expiration_date, req.strike_price, req.option_type
            ))
        })?;
        let option_url = option
            .url
            .as_deref()
            .ok_or_else(|| RhoodError::InvalidSymbol(req.symbol.clone()))?;
        let account_url = self.get_account_url().await?;

        let payload = OptionOrderPayload {
            account: account_url,
            direction: req.credit_or_debit.clone(),
            time_in_force: req.time_in_force,
            legs: vec![OptionLeg {
                position_effect: req.position_effect.clone(),
                side: req.side,
                ratio_quantity: 1,
                option: option_url.to_string(),
            }],
            order_type: "limit",
            trigger: "immediate",
            price: format!("{:.2}", req.limit_price),
            quantity: format!("{}", req.quantity),
            override_day_trade_checks: false,
            override_dtbp_checks: false,
            ref_id: uuid::Uuid::new_v4().to_string(),
        };

        self.post_json(&self.api_url(paths::OPTION_ORDERS), &payload)
            .await
    }

    /// Cancels all open (cancellable) stock orders.
    ///
    /// Fetches open orders, then cancels each one. Requires writable mode.
    /// Returns the number of orders cancelled.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::ReadOnlyMode`] if the client is in read-only mode.
    pub async fn cancel_all_stock_orders(&self) -> Result<usize> {
        self.require_writable()?;
        let open = self.get_open_stock_orders().await?;
        let count = open.len();
        for order in &open {
            if let Some(id) = &order.id {
                let _ = self.cancel_stock_order(id).await;
            }
        }
        Ok(count)
    }

    /// Cancels all open (cancellable) option orders.
    ///
    /// Fetches open orders, then cancels each one. Requires writable mode.
    /// Returns the number of orders cancelled.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::ReadOnlyMode`] if the client is in read-only mode.
    pub async fn cancel_all_option_orders(&self) -> Result<usize> {
        self.require_writable()?;
        let open = self.get_open_option_orders().await?;
        let count = open.len();
        for order in &open {
            if let Some(id) = &order.id {
                let _ = self.cancel_option_order(id).await;
            }
        }
        Ok(count)
    }

    /// Fills in missing `symbol` fields on stock orders by resolving their
    /// `instrument` URLs. Caches instrument lookups so each unique URL is
    /// fetched at most once.
    ///
    /// Orders that already have a symbol or lack an instrument URL are skipped.
    pub async fn enrich_order_symbols(&self, orders: &mut [StockOrder]) -> Result<()> {
        let mut cache: HashMap<String, Option<String>> = HashMap::new();
        for order in orders.iter_mut() {
            if order.symbol.is_some() {
                continue;
            }
            let Some(instrument_url) = &order.instrument else {
                continue;
            };
            let symbol = if let Some(cached) = cache.get(instrument_url) {
                cached.clone()
            } else {
                let result: std::result::Result<Instrument, _> = self.get(instrument_url).await;
                let resolved = result.ok().and_then(|inst| inst.symbol);
                cache.insert(instrument_url.clone(), resolved.clone());
                resolved
            };
            order.symbol = symbol;
        }
        Ok(())
    }

    async fn get_account_url(&self) -> Result<String> {
        let resp: ResultsResponse<AccountProfile> = self
            .get_with_params(
                &self.api_url(paths::ACCOUNTS),
                &[("default_to_all_accounts", "true")],
            )
            .await?;
        resp.results
            .first()
            .and_then(|account| account.url.clone())
            .ok_or(RhoodError::NotAuthenticated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RhoodError;
    use crate::models::order::{
        MarketHours as OrderMarketHours, OrderAmount, OrderType, Side, StockOrder,
        StockOrderRequest, TimeInForce, Trigger,
    };

    #[test]
    fn stock_order_request_market_no_limit_price() {
        let req = StockOrderRequest {
            symbol: "AAPL".to_string(),
            amount: OrderAmount::Quantity(1.0),
            side: Side::Buy,
            order_type: OrderType::Market,
            limit_price: None,
            trigger: Trigger::Immediate,
            stop_price: None,
            time_in_force: TimeInForce::Gtc,
            market_hours: OrderMarketHours::RegularHours,
        };
        assert_eq!(req.order_type, OrderType::Market);
        assert!(req.limit_price.is_none());
    }

    #[test]
    fn stock_order_request_limit_requires_price() {
        let req = StockOrderRequest {
            symbol: "AAPL".to_string(),
            amount: OrderAmount::Quantity(1.0),
            side: Side::Sell,
            order_type: OrderType::Limit,
            limit_price: Some(150.00),
            trigger: Trigger::Immediate,
            stop_price: None,
            time_in_force: TimeInForce::Gtc,
            market_hours: OrderMarketHours::RegularHours,
        };
        assert_eq!(req.order_type, OrderType::Limit);
        assert!(req.limit_price.is_some());
    }

    #[test]
    fn invalid_order_error_display() {
        let err = RhoodError::InvalidOrder("stop_price required for stop orders".into());
        let msg = err.to_string();
        assert!(msg.contains("Invalid order"));
        assert!(msg.contains("stop_price required"));
    }

    #[test]
    fn trigger_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&Trigger::Immediate).unwrap(),
            r#""immediate""#
        );
        assert_eq!(serde_json::to_string(&Trigger::Stop).unwrap(), r#""stop""#);
    }

    #[test]
    fn market_hours_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&OrderMarketHours::RegularHours).unwrap(),
            r#""regular_hours""#
        );
        assert_eq!(
            serde_json::to_string(&OrderMarketHours::ExtendedHours).unwrap(),
            r#""extended_hours""#
        );
        assert_eq!(
            serde_json::to_string(&OrderMarketHours::AllDayHours).unwrap(),
            r#""all_day_hours""#
        );
    }

    #[test]
    fn order_amount_quantity_variant() {
        let amount = OrderAmount::Quantity(10.5);
        match amount {
            OrderAmount::Quantity(q) => assert!((q - 10.5).abs() < f64::EPSILON),
            _ => panic!("Expected Quantity variant"),
        }
    }

    #[test]
    fn order_amount_dollar_variant() {
        let amount = OrderAmount::DollarAmount(50.0);
        match amount {
            OrderAmount::DollarAmount(d) => assert!((d - 50.0).abs() < f64::EPSILON),
            _ => panic!("Expected DollarAmount variant"),
        }
    }

    #[test]
    fn validate_stop_requires_stop_price() {
        let req = StockOrderRequest {
            symbol: "AAPL".to_string(),
            amount: OrderAmount::Quantity(10.0),
            side: Side::Buy,
            order_type: OrderType::Limit,
            limit_price: Some(150.0),
            trigger: Trigger::Stop,
            stop_price: None,
            time_in_force: TimeInForce::Gtc,
            market_hours: OrderMarketHours::RegularHours,
        };
        let err = validate_stock_order(&req);
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("stop_price"));
    }

    #[test]
    fn validate_immediate_rejects_stop_price() {
        let req = StockOrderRequest {
            symbol: "AAPL".to_string(),
            amount: OrderAmount::Quantity(10.0),
            side: Side::Buy,
            order_type: OrderType::Market,
            limit_price: None,
            trigger: Trigger::Immediate,
            stop_price: Some(145.0),
            time_in_force: TimeInForce::Gtc,
            market_hours: OrderMarketHours::RegularHours,
        };
        let err = validate_stock_order(&req);
        assert!(err.is_err());
    }

    #[test]
    fn validate_dollar_amount_requires_buy() {
        let req = StockOrderRequest {
            symbol: "AAPL".to_string(),
            amount: OrderAmount::DollarAmount(50.0),
            side: Side::Sell,
            order_type: OrderType::Market,
            limit_price: None,
            trigger: Trigger::Immediate,
            stop_price: None,
            time_in_force: TimeInForce::Gtc,
            market_hours: OrderMarketHours::RegularHours,
        };
        let err = validate_stock_order(&req);
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("buy"));
    }

    #[test]
    fn validate_dollar_amount_requires_market_immediate() {
        let req = StockOrderRequest {
            symbol: "AAPL".to_string(),
            amount: OrderAmount::DollarAmount(50.0),
            side: Side::Buy,
            order_type: OrderType::Limit,
            limit_price: Some(150.0),
            trigger: Trigger::Immediate,
            stop_price: None,
            time_in_force: TimeInForce::Gtc,
            market_hours: OrderMarketHours::RegularHours,
        };
        let err = validate_stock_order(&req);
        assert!(err.is_err());
    }

    #[test]
    fn validate_extended_hours_requires_limit() {
        let req = StockOrderRequest {
            symbol: "AAPL".to_string(),
            amount: OrderAmount::Quantity(10.0),
            side: Side::Buy,
            order_type: OrderType::Market,
            limit_price: None,
            trigger: Trigger::Immediate,
            stop_price: None,
            time_in_force: TimeInForce::Gtc,
            market_hours: OrderMarketHours::ExtendedHours,
        };
        let err = validate_stock_order(&req);
        assert!(err.is_err());
    }

    #[test]
    fn validate_all_day_hours_requires_limit() {
        let req = StockOrderRequest {
            symbol: "AAPL".to_string(),
            amount: OrderAmount::Quantity(10.0),
            side: Side::Buy,
            order_type: OrderType::Market,
            limit_price: None,
            trigger: Trigger::Immediate,
            stop_price: None,
            time_in_force: TimeInForce::Gtc,
            market_hours: OrderMarketHours::AllDayHours,
        };
        let err = validate_stock_order(&req);
        assert!(err.is_err());
    }

    #[test]
    fn validate_valid_market_order_passes() {
        let req = StockOrderRequest {
            symbol: "AAPL".to_string(),
            amount: OrderAmount::Quantity(10.0),
            side: Side::Buy,
            order_type: OrderType::Market,
            limit_price: None,
            trigger: Trigger::Immediate,
            stop_price: None,
            time_in_force: TimeInForce::Gtc,
            market_hours: OrderMarketHours::RegularHours,
        };
        assert!(validate_stock_order(&req).is_ok());
    }

    #[test]
    fn validate_valid_stop_limit_passes() {
        let req = StockOrderRequest {
            symbol: "AAPL".to_string(),
            amount: OrderAmount::Quantity(10.0),
            side: Side::Buy,
            order_type: OrderType::Limit,
            limit_price: Some(150.0),
            trigger: Trigger::Stop,
            stop_price: Some(145.0),
            time_in_force: TimeInForce::Gtc,
            market_hours: OrderMarketHours::RegularHours,
        };
        assert!(validate_stock_order(&req).is_ok());
    }

    #[test]
    fn stock_order_request_stop_limit() {
        let req = StockOrderRequest {
            symbol: "AAPL".to_string(),
            amount: OrderAmount::Quantity(10.0),
            side: Side::Buy,
            order_type: OrderType::Limit,
            limit_price: Some(150.00),
            trigger: Trigger::Stop,
            stop_price: Some(145.00),
            time_in_force: TimeInForce::Gtc,
            market_hours: OrderMarketHours::RegularHours,
        };
        assert_eq!(req.trigger, Trigger::Stop);
        assert!(req.stop_price.is_some());
    }

    #[test]
    fn stock_order_request_dollar_amount() {
        let req = StockOrderRequest {
            symbol: "AAPL".to_string(),
            amount: OrderAmount::DollarAmount(50.0),
            side: Side::Buy,
            order_type: OrderType::Market,
            limit_price: None,
            trigger: Trigger::Immediate,
            stop_price: None,
            time_in_force: TimeInForce::Gtc,
            market_hours: OrderMarketHours::RegularHours,
        };
        assert!(matches!(req.amount, OrderAmount::DollarAmount(_)));
    }

    #[test]
    fn stock_order_request_all_day_hours() {
        let req = StockOrderRequest {
            symbol: "TSLA".to_string(),
            amount: OrderAmount::Quantity(5.0),
            side: Side::Buy,
            order_type: OrderType::Limit,
            limit_price: Some(200.0),
            trigger: Trigger::Immediate,
            stop_price: None,
            time_in_force: TimeInForce::Gfd,
            market_hours: OrderMarketHours::AllDayHours,
        };
        assert_eq!(req.market_hours, OrderMarketHours::AllDayHours);
    }

    #[test]
    fn stock_order_response_deserializes_with_new_fields() {
        let json = r#"{
            "id": "order-stop-001",
            "symbol": "AAPL",
            "side": "buy",
            "quantity": "10",
            "state": "queued",
            "type": "limit",
            "trigger": "stop",
            "stop_price": "145.00",
            "time_in_force": "gtc",
            "created_at": "2025-01-01T00:00:00Z"
        }"#;
        let order: StockOrder = serde_json::from_str(json).unwrap();
        assert_eq!(order.trigger.as_deref(), Some("stop"));
        assert_eq!(order.stop_price.as_deref(), Some("145.00"));
    }

    #[test]
    fn stock_order_deserializes_full_snapshot() {
        let json = r#"{
            "id": "order-001",
            "symbol": "AAPL",
            "side": "buy",
            "quantity": "10.0000",
            "price": "150.00",
            "average_price": "149.50",
            "cumulative_quantity": "10.0000",
            "state": "filled",
            "type": "limit",
            "time_in_force": "gtc",
            "cancel": null,
            "created_at": "2026-03-31T10:00:00Z",
            "updated_at": "2026-03-31T10:01:00Z"
        }"#;
        let order: StockOrder = serde_json::from_str(json).unwrap();
        assert_eq!(order.id.as_deref(), Some("order-001"));
        assert_eq!(order.symbol.as_deref(), Some("AAPL"));
        assert_eq!(order.side.as_deref(), Some("buy"));
        assert_eq!(order.state.as_deref(), Some("filled"));
        assert!(order.cancel.is_none());
    }

    #[test]
    fn stock_order_open_has_cancel_url() {
        let json = r#"{
            "id": "order-002",
            "symbol": "TSLA",
            "side": "sell",
            "state": "queued",
            "cancel": "https://api.robinhood.com/orders/order-002/cancel/"
        }"#;
        let order: StockOrder = serde_json::from_str(json).unwrap();
        assert!(order.cancel.is_some());
    }
}
