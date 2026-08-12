use crate::api::paths;
use crate::client::RobinhoodClient;
use crate::endpoints::account::validate_account_number;
use crate::models::order::{
    CancelAllFailure, CancelAllOutcome, MarketHours, OptionLeg, OptionOrder, OptionOrderPayload,
    OptionOrderRequest, OrderAmount, OrderType, PreparedStockOrder, Side, StockOrder,
    StockOrderPayload, StockOrderRequest, TimeInForce, Trigger,
};
use crate::models::stock::StockQuote;
use crate::{Result, RhoodError};

const STOCK_ORDER_FORM_VERSION: u8 = 7;
const PREPARED_STOCK_ORDER_TTL_MS: i64 = 60_000;

/// Validates a stock order request for logical consistency.
///
/// Returns `Ok(())` if valid, or `Err(RhoodError::InvalidOrder)` with a
/// descriptive message if the request contains contradictory parameters.
pub fn validate_stock_order(req: &StockOrderRequest) -> Result<()> {
    match req.amount {
        OrderAmount::Quantity(quantity) => {
            validate_positive_order_value("quantity", quantity)?;
        }
        OrderAmount::DollarAmount(dollars) => {
            validate_positive_order_value("dollar amount", dollars)?;
            if dollars < 1.0 {
                return Err(RhoodError::InvalidOrder(
                    "Dollar-based orders require at least $1.00".into(),
                ));
            }
        }
    }
    match (req.order_type, req.limit_price) {
        (OrderType::Limit, None) => {
            return Err(RhoodError::InvalidOrder(
                "limit_price is required for limit orders".into(),
            ));
        }
        (OrderType::Market, Some(_)) => {
            return Err(RhoodError::InvalidOrder(
                "limit_price must not be set for market orders".into(),
            ));
        }
        (_, Some(price)) => validate_positive_order_value("limit_price", price)?,
        (_, None) => {}
    }
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
    if let Some(stop_price) = req.stop_price {
        validate_positive_order_value("stop_price", stop_price)?;
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
        if req.time_in_force != TimeInForce::Gfd {
            return Err(RhoodError::InvalidOrder(
                "Dollar-based orders require good-for-day time in force".into(),
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

fn validate_positive_order_value(name: &str, value: f64) -> Result<()> {
    if !value.is_finite() || value <= 0.0 {
        return Err(RhoodError::InvalidOrder(format!(
            "{name} must be a finite positive number"
        )));
    }
    Ok(())
}

fn positive_quote_field<'a>(name: &str, value: Option<&'a str>) -> Result<(&'a str, f64)> {
    let text = value.ok_or_else(|| {
        RhoodError::InvalidOrder(format!("Immediate market order quote has no {name}"))
    })?;
    let parsed = text.parse::<f64>().map_err(|_parse_error| {
        RhoodError::InvalidOrder(format!("Immediate market order quote has invalid {name}"))
    })?;
    validate_positive_order_value(name, parsed)?;
    Ok((text, parsed))
}

fn format_share_quantity(quantity: f64) -> String {
    let formatted = format!("{quantity:.6}");
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_string()
}

fn fractional_quantity_for_dollars(dollars: f64, ask_price: f64) -> Result<String> {
    let quantity = ((dollars / ask_price) * 1_000_000.0).ceil() / 1_000_000.0;
    validate_positive_order_value("resolved fractional quantity", quantity)?;
    Ok(format_share_quantity(quantity))
}

fn quote_for_symbol(quotes: Vec<StockQuote>, symbol: &str) -> Result<StockQuote> {
    quotes
        .into_iter()
        .find(|quote| {
            quote
                .symbol
                .as_deref()
                .is_some_and(|quoted| quoted.eq_ignore_ascii_case(symbol))
        })
        .ok_or_else(|| RhoodError::InvalidSymbol(symbol.to_string()))
}

impl RobinhoodClient {
    async fn get_stock_orders_raw(
        &self,
        account_number: &str,
        since: Option<&str>,
    ) -> Result<Vec<StockOrder>> {
        validate_account_number(account_number)?;
        let mut params: Vec<(&str, &str)> = vec![("account_numbers", account_number)];
        if let Some(date) = since {
            params.push(("updated_at[gte]", date));
        }
        self.get_paginated(&self.api_url(paths::STOCK_ORDERS), &params)
            .await
    }

    /// Fetches all stock orders, including completed, cancelled, and pending.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_all_stock_orders(
        &self,
        account_number: &str,
        since: Option<&str>,
    ) -> Result<Vec<StockOrder>> {
        let mut orders = self.get_stock_orders_raw(account_number, since).await?;
        self.enrich_instrument_identities("get_all_stock_orders", &mut orders)
            .await;
        Ok(orders)
    }

    /// Fetches only open (cancellable) stock orders.
    ///
    /// Filters the full order list to those with a non-null cancel URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying orders request fails.
    pub async fn get_open_stock_orders(&self, account_number: &str) -> Result<Vec<StockOrder>> {
        let mut orders = self.get_stock_orders_raw(account_number, None).await?;
        orders.retain(|order| order.cancel.is_some());
        self.enrich_instrument_identities("get_open_stock_orders", &mut orders)
            .await;
        Ok(orders)
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

    /// Prepares a stock order without submitting it.
    ///
    /// Validation completes before network access. Immediate market orders use
    /// one fresh bid/ask quote to fix the broker collar. Dollar buys use that
    /// same ask quote and round the estimated share quantity upward to six
    /// decimal places. The returned value must be submitted within 60 seconds.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::ReadOnlyMode`] if the client is in read-only
    /// mode. Returns [`RhoodError::InvalidSymbol`] if the symbol cannot be
    /// resolved. Also returns an error on HTTP or deserialization failures.
    pub async fn prepare_stock_order(&self, req: &StockOrderRequest) -> Result<PreparedStockOrder> {
        self.require_writable()?;
        validate_stock_order(req)?;
        validate_account_number(&req.account_number)?;
        let instrument = self
            .instrument_for_symbol(&req.symbol)
            .await?
            .ok_or_else(|| RhoodError::InvalidSymbol(req.symbol.clone()))?;
        let instrument_url = instrument
            .url
            .clone()
            .ok_or_else(|| RhoodError::InvalidSymbol(req.symbol.clone()))?;
        let account_url = format!("{}{}/", self.api_url(paths::ACCOUNTS), req.account_number);

        let needs_quote = req.order_type == OrderType::Market && req.trigger == Trigger::Immediate;
        let quote = if needs_quote {
            let quotes = self.get_quotes(&[&req.symbol]).await?;
            Some(quote_for_symbol(quotes, &req.symbol)?)
        } else {
            None
        };
        let (ask_price, ask_value, bid_price) = if let Some(quote) = quote {
            let (ask_price, ask_value) =
                positive_quote_field("ask_price", quote.ask_price.as_deref())?;
            let (bid_price, _bid_value) =
                positive_quote_field("bid_price", quote.bid_price.as_deref())?;
            (
                Some(ask_price.to_string()),
                Some(ask_value),
                Some(bid_price.to_string()),
            )
        } else {
            (None, None, None)
        };

        let (quantity, requested_dollar_amount, conversion_price) = match req.amount {
            OrderAmount::Quantity(quantity) => (format_share_quantity(quantity), None, None),
            OrderAmount::DollarAmount(dollars) => {
                let ask_value = ask_value.ok_or_else(|| {
                    RhoodError::InvalidOrder(
                        "Dollar-based order preparation requires an ask quote".into(),
                    )
                })?;
                (
                    fractional_quantity_for_dollars(dollars, ask_value)?,
                    Some(format!("{dollars:.2}")),
                    ask_price.clone(),
                )
            }
        };

        let price = match (req.order_type, req.trigger, req.side) {
            (OrderType::Limit, _, _) => req.limit_price.map(|price| format!("{price:.2}")),
            (OrderType::Market, Trigger::Stop, _) => {
                req.stop_price.map(|price| format!("{price:.2}"))
            }
            (OrderType::Market, Trigger::Immediate, Side::Buy) => ask_price.clone(),
            (OrderType::Market, Trigger::Immediate, Side::Sell) => bid_price.clone(),
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
            extended_hours: req.market_hours != MarketHours::RegularHours,
            price,
            stop_price: req.stop_price.map(|price| format!("{price:.2}")),
            ask_price,
            bid_price,
            override_day_trade_checks: false,
            override_dtbp_checks: false,
            ref_id: uuid::Uuid::new_v4().to_string(),
            order_form_version: STOCK_ORDER_FORM_VERSION,
        };

        Ok(PreparedStockOrder {
            payload,
            account_number: req.account_number.clone(),
            prepared_at_unix_ms: chrono::Utc::now().timestamp_millis(),
            requested_dollar_amount,
            conversion_price,
        })
    }

    /// Consumes and submits a recently prepared stock order exactly once.
    ///
    /// Redirects and automatic retries are disabled for this write. A transport
    /// failure after dispatch returns [`RhoodError::SubmissionUncertain`] with
    /// the client reference needed to reconcile recent orders.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::PreparedOrderExpired`] after 60 seconds, or a
    /// submission, authentication, API, or response error.
    pub async fn submit_prepared_stock_order(
        &self,
        prepared: PreparedStockOrder,
    ) -> Result<StockOrder> {
        self.require_writable()?;
        let age_ms = chrono::Utc::now()
            .timestamp_millis()
            .checked_sub(prepared.prepared_at_unix_ms)
            .ok_or(RhoodError::PreparedOrderExpired)?;
        if !(0..=PREPARED_STOCK_ORDER_TTL_MS).contains(&age_ms) {
            return Err(RhoodError::PreparedOrderExpired);
        }
        self.submit_stock_order_form(&self.api_url(paths::STOCK_ORDERS), &prepared.payload)
            .await
    }

    /// Prepares and immediately submits a stock order.
    ///
    /// Interactive callers should use [`prepare_stock_order`](Self::prepare_stock_order),
    /// show its preview for confirmation, then call
    /// [`submit_prepared_stock_order`](Self::submit_prepared_stock_order).
    ///
    /// # Errors
    ///
    /// Returns any preparation or submission error.
    pub async fn place_stock_order(&self, req: &StockOrderRequest) -> Result<StockOrder> {
        let prepared = self.prepare_stock_order(req).await?;
        self.submit_prepared_stock_order(prepared).await
    }

    /// Fetches all option orders, including completed, cancelled, and pending.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn get_all_option_orders(
        &self,
        account_number: &str,
        since: Option<&str>,
    ) -> Result<Vec<OptionOrder>> {
        validate_account_number(account_number)?;
        let mut params: Vec<(&str, &str)> = vec![("account_numbers", account_number)];
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
    pub async fn get_open_option_orders(&self, account_number: &str) -> Result<Vec<OptionOrder>> {
        let orders = self.get_all_option_orders(account_number, None).await?;
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
        validate_account_number(&req.account_number)?;
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
        let account_url = format!("{}{}/", self.api_url(paths::ACCOUNTS), req.account_number);

        let payload = OptionOrderPayload {
            account: account_url,
            direction: match req.side {
                Side::Buy => "debit".to_string(),
                Side::Sell => "credit".to_string(),
            },
            time_in_force: req.time_in_force,
            legs: vec![OptionLeg {
                position_effect: req.position_effect,
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
    /// Fetches open orders, then attempts to cancel each one. Requires writable
    /// mode. Returns the IDs cancelled successfully and every per-order failure.
    /// An open order without an ID is returned as a failure because it cannot be
    /// cancelled by ID.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::ReadOnlyMode`] if the client is in read-only mode.
    /// Returns an error if fetching the open orders fails.
    pub async fn cancel_all_stock_orders(&self, account_number: &str) -> Result<CancelAllOutcome> {
        self.require_writable()?;
        let open = self.get_open_stock_orders(account_number).await?;
        let mut outcome = CancelAllOutcome::default();
        for order in open {
            let Some(id) = order.id else {
                outcome.failed.push(CancelAllFailure {
                    order_id: None,
                    error: "open order is missing an ID".to_string(),
                });
                continue;
            };
            match self.cancel_stock_order(&id).await {
                Ok(()) => outcome.cancelled.push(id),
                Err(error) => outcome.failed.push(CancelAllFailure {
                    order_id: Some(id),
                    error: error.to_string(),
                }),
            }
        }
        Ok(outcome)
    }

    /// Cancels all open (cancellable) option orders.
    ///
    /// Fetches open orders, then attempts to cancel each one. Requires writable
    /// mode. Returns the IDs cancelled successfully and every per-order failure.
    /// An open order without an ID is returned as a failure because it cannot be
    /// cancelled by ID.
    ///
    /// # Errors
    ///
    /// Returns [`RhoodError::ReadOnlyMode`] if the client is in read-only mode.
    /// Returns an error if fetching the open orders fails.
    pub async fn cancel_all_option_orders(&self, account_number: &str) -> Result<CancelAllOutcome> {
        self.require_writable()?;
        let open = self.get_open_option_orders(account_number).await?;
        let mut outcome = CancelAllOutcome::default();
        for order in open {
            let Some(id) = order.id else {
                outcome.failed.push(CancelAllFailure {
                    order_id: None,
                    error: "open order is missing an ID".to_string(),
                });
                continue;
            };
            match self.cancel_option_order(&id).await {
                Ok(()) => outcome.cancelled.push(id),
                Err(error) => outcome.failed.push(CancelAllFailure {
                    order_id: Some(id),
                    error: error.to_string(),
                }),
            }
        }
        Ok(outcome)
    }

    /// Temporary compatibility forward for stock-order identity enrichment.
    pub async fn enrich_order_symbols(&self, orders: &mut [StockOrder]) -> Result<()> {
        self.enrich_instrument_identities("enrich_order_symbols", orders)
            .await;
        Ok(())
    }
}

#[cfg(test)]
#[expect(
    clippy::assertions_on_result_states,
    reason = "these validation tests intentionally assert Result state without unwrapping"
)]
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
            account_number: "ABC123".to_string(),
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
            account_number: "ABC123".to_string(),
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
            account_number: "ABC123".to_string(),
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
            account_number: "ABC123".to_string(),
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
            account_number: "ABC123".to_string(),
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
            account_number: "ABC123".to_string(),
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
            account_number: "ABC123".to_string(),
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
            account_number: "ABC123".to_string(),
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
            account_number: "ABC123".to_string(),
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
            account_number: "ABC123".to_string(),
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
            account_number: "ABC123".to_string(),
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
            account_number: "ABC123".to_string(),
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
            account_number: "ABC123".to_string(),
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

#[cfg(test)]
mod endpoint_tests {
    use std::collections::HashMap;

    use crate::RhoodError;
    use crate::client::RobinhoodClient;
    use crate::config::RhoodConfig;
    use crate::models::order::{
        CancelAllFailure, MarketHours, OptionOrderRequest, OptionPositionEffect, OrderAmount,
        OrderType, Side, StockOrderRequest, TimeInForce, Trigger,
    };
    use secrecy::SecretString;
    use wiremock::matchers::{body_partial_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn client_for_server(base_url: &str) -> (tempfile::TempDir, RobinhoodClient) {
        client_for_server_with_timeout(base_url, 30).await
    }

    async fn client_for_server_with_timeout(
        base_url: &str,
        request_timeout_secs: u64,
    ) -> (tempfile::TempDir, RobinhoodClient) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = RhoodConfig {
            read_only: false,
            ..RhoodConfig::default()
        };
        config.auth.token_cache_path = dir
            .path()
            .join("nonexistent-token.json")
            .to_str()
            .unwrap()
            .to_string();
        config.api.base_url = base_url.to_string();
        config.api.phoenix_url = base_url.to_string();
        config.api.bonfire_url = base_url.to_string();
        config.http.request_timeout_secs = request_timeout_secs;
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

    fn stock_request() -> StockOrderRequest {
        StockOrderRequest {
            account_number: "ABC123".to_string(),
            symbol: "aapl".to_string(),
            amount: OrderAmount::Quantity(2.0),
            side: Side::Buy,
            order_type: OrderType::Market,
            limit_price: None,
            trigger: Trigger::Immediate,
            stop_price: None,
            time_in_force: TimeInForce::Gfd,
            market_hours: MarketHours::RegularHours,
        }
    }

    async fn mount_stock_instrument(server: &MockServer) {
        let instrument_url = format!("{}/instruments/instrument-aapl/", server.uri());
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .and(query_param("symbol", "AAPL"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": "instrument-aapl", "symbol": "AAPL", "url": instrument_url}],
                "next": null
            })))
            .expect(1)
            .mount(server)
            .await;
    }

    async fn mount_stock_quote(server: &MockServer, ask: &str, bid: &str) {
        Mock::given(method("GET"))
            .and(path("/quotes/"))
            .and(query_param("symbols", "AAPL"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"symbol": "AAPL", "ask_price": ask, "bid_price": bid}]
            })))
            .expect(1)
            .mount(server)
            .await;
    }

    fn form_fields(prepared: &crate::models::order::PreparedStockOrder) -> HashMap<String, String> {
        let encoded = serde_urlencoded::to_string(&prepared.payload).unwrap();
        serde_urlencoded::from_str(&encoded).unwrap()
    }

    #[tokio::test]
    async fn prepare_market_orders_pins_side_specific_collar_and_protocol_fields() {
        for (side, expected_price) in [(Side::Buy, "101.25"), (Side::Sell, "100.75")] {
            let server = MockServer::start().await;
            mount_stock_instrument(&server).await;
            mount_stock_quote(&server, "101.25", "100.75").await;
            let (_dir, client) = client_for_server(&server.uri()).await;
            let mut request = stock_request();
            request.side = side;

            let prepared = client.prepare_stock_order(&request).await.unwrap();
            let preview = prepared.preview();
            let fields = form_fields(&prepared);

            assert_eq!(preview.price.as_deref(), Some(expected_price));
            assert_eq!(preview.ask_price.as_deref(), Some("101.25"));
            assert_eq!(preview.bid_price.as_deref(), Some("100.75"));
            assert_eq!(preview.order_form_version, 7);
            assert_eq!(
                fields.get("price").map(String::as_str),
                Some(expected_price)
            );
            assert_eq!(fields.get("ask_price").map(String::as_str), Some("101.25"));
            assert_eq!(fields.get("bid_price").map(String::as_str), Some("100.75"));
            assert_eq!(
                fields.get("extended_hours").map(String::as_str),
                Some("false")
            );
            assert_eq!(
                fields.get("override_day_trade_checks").map(String::as_str),
                Some("false")
            );
            assert_eq!(
                fields.get("override_dtbp_checks").map(String::as_str),
                Some("false")
            );
            assert_eq!(
                fields.get("order_form_version").map(String::as_str),
                Some("7")
            );
            uuid::Uuid::parse_str(&preview.client_ref).unwrap();
            assert_eq!(fields.len(), 17);
            server.verify().await;
        }
    }

    #[tokio::test]
    async fn prepare_limit_and_stop_market_orders_do_not_fetch_quotes() {
        let server = MockServer::start().await;
        mount_stock_instrument(&server).await;
        let (_dir, client) = client_for_server(&server.uri()).await;
        let mut limit = stock_request();
        limit.order_type = OrderType::Limit;
        limit.limit_price = Some(99.5);
        limit.market_hours = MarketHours::AllDayHours;

        let prepared = client.prepare_stock_order(&limit).await.unwrap();
        let preview = prepared.preview();

        assert_eq!(preview.price.as_deref(), Some("99.50"));
        assert!(preview.ask_price.is_none());
        assert!(preview.bid_price.is_none());
        assert!(preview.extended_hours);
        server.verify().await;

        let server = MockServer::start().await;
        mount_stock_instrument(&server).await;
        let (_dir, client) = client_for_server(&server.uri()).await;
        let mut stop_market = stock_request();
        stop_market.trigger = Trigger::Stop;
        stop_market.stop_price = Some(98.25);

        let prepared = client.prepare_stock_order(&stop_market).await.unwrap();
        let preview = prepared.preview();

        assert_eq!(preview.price.as_deref(), Some("98.25"));
        assert_eq!(preview.stop_price.as_deref(), Some("98.25"));
        assert!(preview.ask_price.is_none());
        assert!(preview.bid_price.is_none());
        server.verify().await;
    }

    #[tokio::test]
    async fn dollar_buy_converts_with_ask_and_rounds_up_to_six_decimals() {
        let server = MockServer::start().await;
        mount_stock_instrument(&server).await;
        mount_stock_quote(&server, "315.53", "311.19").await;
        let (_dir, client) = client_for_server(&server.uri()).await;
        let mut request = stock_request();
        request.amount = OrderAmount::DollarAmount(1.0);

        let prepared = client.prepare_stock_order(&request).await.unwrap();
        let preview = prepared.preview();

        assert_eq!(preview.quantity, "0.00317");
        assert_eq!(preview.requested_dollar_amount.as_deref(), Some("1.00"));
        assert_eq!(preview.conversion_price.as_deref(), Some("315.53"));
        assert!(!form_fields(&prepared).contains_key("dollar_based_amount"));
        server.verify().await;
    }

    #[tokio::test]
    async fn invalid_stock_order_fails_before_network_access() {
        let server = MockServer::start().await;
        let (_dir, client) = client_for_server(&server.uri()).await;
        let mut cases = Vec::new();
        let mut market_with_limit = stock_request();
        market_with_limit.limit_price = Some(10.0);
        cases.push(market_with_limit);
        let mut limit_without_price = stock_request();
        limit_without_price.order_type = OrderType::Limit;
        cases.push(limit_without_price);
        let mut invalid_quantity = stock_request();
        invalid_quantity.amount = OrderAmount::Quantity(f64::NAN);
        cases.push(invalid_quantity);
        let mut small_dollar_order = stock_request();
        small_dollar_order.amount = OrderAmount::DollarAmount(0.99);
        cases.push(small_dollar_order);
        let mut gtc_dollar_order = stock_request();
        gtc_dollar_order.amount = OrderAmount::DollarAmount(5.0);
        gtc_dollar_order.time_in_force = TimeInForce::Gtc;
        cases.push(gtc_dollar_order);

        for request in cases {
            let error = client.prepare_stock_order(&request).await.unwrap_err();
            assert!(matches!(error, RhoodError::InvalidOrder(_)));
        }
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn submit_consumes_fresh_preparation_without_retrying_server_errors() {
        let server = MockServer::start().await;
        mount_stock_instrument(&server).await;
        mount_stock_quote(&server, "101.25", "100.75").await;
        Mock::given(method("POST"))
            .and(path("/orders/"))
            .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
                "detail": "rejected once"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;
        let prepared = client.prepare_stock_order(&stock_request()).await.unwrap();

        let error = client
            .submit_prepared_stock_order(prepared)
            .await
            .unwrap_err();

        assert!(matches!(error, RhoodError::Api { status: 500, .. }));
        server.verify().await;
    }

    #[tokio::test]
    async fn timed_out_submission_returns_client_reference_for_reconciliation() {
        let server = MockServer::start().await;
        mount_stock_instrument(&server).await;
        mount_stock_quote(&server, "101.25", "100.75").await;
        Mock::given(method("POST"))
            .and(path("/orders/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_secs(2))
                    .set_body_json(serde_json::json!({"id": "late-order"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server_with_timeout(&server.uri(), 1).await;
        let prepared = client.prepare_stock_order(&stock_request()).await.unwrap();
        let client_ref = prepared.preview().client_ref;

        let error = client
            .submit_prepared_stock_order(prepared)
            .await
            .unwrap_err();

        assert!(matches!(
            &error,
            RhoodError::SubmissionUncertain {
                client_ref: actual,
                ..
            } if actual == &client_ref
        ));
        assert!(
            error
                .to_string()
                .contains("Reconcile open and recent orders")
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn stale_preparation_fails_before_submission() {
        let server = MockServer::start().await;
        mount_stock_instrument(&server).await;
        mount_stock_quote(&server, "101.25", "100.75").await;
        let (_dir, client) = client_for_server(&server.uri()).await;
        let mut prepared = client.prepare_stock_order(&stock_request()).await.unwrap();
        prepared.prepared_at_unix_ms -= 60_001;
        let request_count = server.received_requests().await.unwrap().len();

        let error = client
            .submit_prepared_stock_order(prepared)
            .await
            .unwrap_err();

        assert!(matches!(error, RhoodError::PreparedOrderExpired));
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            request_count
        );
    }

    #[tokio::test]
    async fn stock_submission_does_not_follow_redirects() {
        let server = MockServer::start().await;
        mount_stock_instrument(&server).await;
        mount_stock_quote(&server, "101.25", "100.75").await;
        Mock::given(method("POST"))
            .and(path("/orders/"))
            .respond_with(
                ResponseTemplate::new(307)
                    .insert_header("Location", format!("{}/redirected/", server.uri())),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/redirected/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "must-not-submit"
            })))
            .expect(0)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;
        let prepared = client.prepare_stock_order(&stock_request()).await.unwrap();

        let error = client
            .submit_prepared_stock_order(prepared)
            .await
            .unwrap_err();

        assert!(matches!(error, RhoodError::Api { status: 307, .. }));
        server.verify().await;
    }

    async fn assert_option_order_action(
        side: Side,
        position_effect: OptionPositionEffect,
        expected_direction: &str,
    ) {
        let server = MockServer::start().await;
        let option_url = format!("{}/options/instruments/contract-aapl/", server.uri());
        let account_url = format!("{}/accounts/ABC123/", server.uri());

        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .and(query_param("symbol", "AAPL"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"symbol": "AAPL", "tradable_chain_id": "chain-aapl"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/options/instruments/"))
            .and(query_param("chain_id", "chain-aapl"))
            .and(query_param("expiration_dates", "2026-09-18"))
            .and(query_param("type", "call"))
            .and(query_param("state", "active"))
            .and(query_param("strike_price", "200.0000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{
                    "chain_id": "chain-aapl",
                    "chain_symbol": "AAPL",
                    "expiration_date": "2026-09-18",
                    "id": "contract-aapl",
                    "state": "active",
                    "strike_price": "200.0000",
                    "type": "call",
                    "url": option_url
                }],
                "next": null,
                "previous": null
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/options/orders/"))
            .and(body_partial_json(serde_json::json!({
                "account": account_url,
                "direction": expected_direction,
                "time_in_force": "gtc",
                "legs": [{
                    "position_effect": position_effect,
                    "side": side,
                    "ratio_quantity": 1,
                    "option": option_url
                }],
                "type": "limit",
                "trigger": "immediate",
                "price": "2.50",
                "quantity": "1"
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "placed-order"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;
        let request = OptionOrderRequest {
            account_number: "ABC123".to_string(),
            symbol: "AAPL".to_string(),
            expiration_date: "2026-09-18".to_string(),
            strike_price: 200.0,
            option_type: "call".to_string(),
            side,
            quantity: 1.0,
            limit_price: 2.5,
            position_effect,
            time_in_force: TimeInForce::Gtc,
        };

        let order = client.place_option_order(&request).await.unwrap();

        assert_eq!(order.id.as_deref(), Some("placed-order"));
        server.verify().await;
    }

    #[tokio::test]
    async fn place_option_order_supports_all_four_position_actions() {
        for (side, position_effect, expected_direction) in [
            (Side::Buy, OptionPositionEffect::Open, "debit"),
            (Side::Sell, OptionPositionEffect::Close, "credit"),
            (Side::Sell, OptionPositionEffect::Open, "credit"),
            (Side::Buy, OptionPositionEffect::Close, "debit"),
        ] {
            assert_option_order_action(side, position_effect, expected_direction).await;
        }
    }

    #[tokio::test]
    async fn get_all_stock_orders_enriches_once_and_preserves_wire_symbol() {
        const INSTRUMENT_ID: &str = "450dfc6d-5510-4d40-abfb-f633b7d9be3e";
        let server = MockServer::start().await;
        let reference = format!("https://api.robinhood.com/instruments/{INSTRUMENT_ID}/");
        Mock::given(method("GET"))
            .and(path("/orders/"))
            .and(query_param("account_numbers", "ABC123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [
                    {"id": "missing", "instrument": reference.clone()},
                    {"id": "wire", "instrument": reference, "symbol": "WIRE"}
                ],
                "next": null,
                "previous": null
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .and(query_param("ids", INSTRUMENT_ID))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": INSTRUMENT_ID, "symbol": "AAPL"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let orders = client.get_all_stock_orders("ABC123", None).await.unwrap();

        assert_eq!(orders[0].symbol.as_deref(), Some("AAPL"));
        assert_eq!(orders[1].symbol.as_deref(), Some("WIRE"));
        server.verify().await;
    }

    #[tokio::test]
    async fn get_open_stock_orders_does_not_resolve_closed_order_only_references() {
        const INSTRUMENT_ID: &str = "450dfc6d-5510-4d40-abfb-f633b7d9be3e";
        let server = MockServer::start().await;
        let reference = format!("https://api.robinhood.com/instruments/{INSTRUMENT_ID}/");
        Mock::given(method("GET"))
            .and(path("/orders/"))
            .and(query_param("account_numbers", "ABC123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": "closed", "instrument": reference, "cancel": null}],
                "next": null,
                "previous": null
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/instruments/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{"id": INSTRUMENT_ID, "symbol": "AAPL"}]
            })))
            .expect(0)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let orders = client.get_open_stock_orders("ABC123").await.unwrap();

        assert!(orders.is_empty());
        server.verify().await;
    }

    #[tokio::test]
    async fn cancel_all_stock_orders_reports_failures_and_attempts_every_identified_order() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/orders/"))
            .and(query_param("account_numbers", "ABC123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [
                    {"id": "stock-success", "cancel": "https://example.test/stock-success/cancel/"},
                    {"id": "stock-failure", "cancel": "https://example.test/stock-failure/cancel/"},
                    {"id": "stock-later", "cancel": "https://example.test/stock-later/cancel/"},
                    {"id": null, "cancel": "https://example.test/missing-id/cancel/"}
                ],
                "next": null,
                "previous": null
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/orders/stock-success/cancel/"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/orders/stock-failure/cancel/"))
            .respond_with(
                ResponseTemplate::new(500)
                    .set_body_string(r#"{"detail":"stock cancellation failed"}"#),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/orders/stock-later/cancel/"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let outcome = client.cancel_all_stock_orders("ABC123").await.unwrap();

        assert_eq!(outcome.cancelled, ["stock-success", "stock-later"]);
        assert_eq!(
            outcome.failed,
            [
                CancelAllFailure {
                    order_id: Some("stock-failure".to_string()),
                    error: "API error (500): stock cancellation failed".to_string(),
                },
                CancelAllFailure {
                    order_id: None,
                    error: "open order is missing an ID".to_string(),
                },
            ]
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn cancel_all_option_orders_reports_failures_and_attempts_every_identified_order() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/options/orders/"))
            .and(query_param("account_numbers", "ABC123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [
                    {"id": "option-success", "cancel_url": "https://example.test/option-success/cancel/"},
                    {"id": "option-failure", "cancel_url": "https://example.test/option-failure/cancel/"},
                    {"id": "option-later", "cancel_url": "https://example.test/option-later/cancel/"},
                    {"id": null, "cancel_url": "https://example.test/missing-id/cancel/"}
                ],
                "next": null,
                "previous": null
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/options/orders/option-success/cancel/"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/options/orders/option-failure/cancel/"))
            .respond_with(
                ResponseTemplate::new(500)
                    .set_body_string(r#"{"detail":"option cancellation failed"}"#),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/options/orders/option-later/cancel/"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let (_dir, client) = client_for_server(&server.uri()).await;

        let outcome = client.cancel_all_option_orders("ABC123").await.unwrap();

        assert_eq!(outcome.cancelled, ["option-success", "option-later"]);
        assert_eq!(
            outcome.failed,
            [
                CancelAllFailure {
                    order_id: Some("option-failure".to_string()),
                    error: "API error (500): option cancellation failed".to_string(),
                },
                CancelAllFailure {
                    order_id: None,
                    error: "open order is missing an ID".to_string(),
                },
            ]
        );
        server.verify().await;
    }
}
