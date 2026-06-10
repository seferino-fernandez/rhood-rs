use crate::output::{OutputFormat, output};
use clap::Subcommand;
use rhood_core::models::order::{
    MarketHours, OrderAmount, OrderType, Side, StockOrderRequest, TimeInForce, Trigger,
};
use rhood_core::{RhoodConfig, RobinhoodClient};

#[derive(Subcommand)]
pub enum OrderCommand {
    /// Place a buy order
    Buy {
        symbol: String,
        /// Number of shares (mutually exclusive with --dollar-amount)
        #[arg(long, group = "amount_spec")]
        quantity: Option<f64>,
        /// Dollar amount to invest (mutually exclusive with --quantity)
        #[arg(long, group = "amount_spec")]
        dollar_amount: Option<f64>,
        /// Order type
        #[arg(long, value_enum, default_value_t = OrderType::Market)]
        r#type: OrderType,
        /// Limit price (required for limit orders)
        #[arg(long)]
        limit: Option<f64>,
        /// Trigger
        #[arg(long, value_enum, default_value_t = Trigger::Immediate)]
        trigger: Trigger,
        /// Stop price (required for stop trigger)
        #[arg(long)]
        stop_price: Option<f64>,
        /// Market hours
        #[arg(long, value_enum, default_value_t = MarketHours::RegularHours)]
        market_hours: MarketHours,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Place a sell order
    Sell {
        symbol: String,
        #[arg(long)]
        quantity: f64,
        /// Order type
        #[arg(long, value_enum, default_value_t = OrderType::Market)]
        r#type: OrderType,
        /// Limit price (required for limit orders)
        #[arg(long)]
        limit: Option<f64>,
        /// Trigger
        #[arg(long, value_enum, default_value_t = Trigger::Immediate)]
        trigger: Trigger,
        /// Stop price (required for stop trigger)
        #[arg(long)]
        stop_price: Option<f64>,
        /// Market hours
        #[arg(long, value_enum, default_value_t = MarketHours::RegularHours)]
        market_hours: MarketHours,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// List orders (all or filtered by status)
    List {
        /// Filter by status: open
        #[arg(long)]
        status: Option<String>,
        /// Filter orders updated since this date (YYYY-MM-DD)
        #[arg(long)]
        since: Option<String>,
    },
    /// Cancel an order by ID
    Cancel { order_id: String },
}

/// Returns `true` if the user confirmed, `false` if they declined.
/// Errors if not a terminal and `--yes` was not passed.
fn confirm_order(yes: bool) -> anyhow::Result<bool> {
    if yes {
        return Ok(true);
    }
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("not a terminal: pass --yes to place orders non-interactively");
    }
    let confirmed = dialoguer::Confirm::new()
        .with_prompt("Confirm?")
        .default(false)
        .interact()?;
    Ok(confirmed)
}

/// Human-facing message for a just-placed order, warning if it was rejected.
fn placed_order_message(id: &str, state: Option<&str>) -> String {
    match state {
        Some(s) if s.eq_ignore_ascii_case("rejected") || s.eq_ignore_ascii_case("failed") => {
            format!("Order {id} was not placed (state: {s})")
        }
        Some(s) => format!("Order placed: {id} (state: {s})"),
        None => format!("Order placed: {id}"),
    }
}

pub async fn run(
    cmd: &OrderCommand,
    format: OutputFormat,
    config: RhoodConfig,
) -> anyhow::Result<()> {
    let read_only = config.read_only;
    let client = RobinhoodClient::with_config(config)?;
    super::ensure_logged_in(&client).await?;
    match cmd {
        OrderCommand::Buy {
            symbol,
            quantity,
            dollar_amount,
            r#type,
            limit,
            trigger,
            stop_price,
            market_hours,
            yes,
        } => {
            if read_only {
                anyhow::bail!("read-only mode: pass --read-write to place orders");
            }
            if *r#type == OrderType::Limit && limit.is_none() {
                anyhow::bail!("Limit price required for limit orders (--limit)");
            }
            let amount = match (quantity, dollar_amount) {
                (Some(q), None) => OrderAmount::Quantity(*q),
                (None, Some(d)) => OrderAmount::DollarAmount(*d),
                (None, None) => anyhow::bail!("Specify --quantity or --dollar-amount"),
                _ => unreachable!("clap group prevents both"),
            };
            let amount_str = match amount {
                OrderAmount::Quantity(q) => format!("{q}x"),
                OrderAmount::DollarAmount(d) => format!("${d:.2} of"),
            };
            let type_str = if *r#type == OrderType::Market {
                "market".to_string()
            } else {
                format!("limit @ ${:.2}", limit.unwrap())
            };
            println!("Buy {amount_str} {symbol} at {type_str}");
            if !confirm_order(*yes)? {
                println!("Order cancelled.");
                return Ok(());
            }
            let req = StockOrderRequest {
                symbol: symbol.clone(),
                amount,
                side: Side::Buy,
                order_type: *r#type,
                limit_price: *limit,
                trigger: *trigger,
                stop_price: *stop_price,
                time_in_force: TimeInForce::Gtc,
                market_hours: *market_hours,
            };
            let order = client.place_stock_order(&req).await?;
            let id = order.id.clone().unwrap_or_else(|| "unknown".into());
            println!("{}", placed_order_message(&id, order.state.as_deref()));
        }
        OrderCommand::Sell {
            symbol,
            quantity,
            r#type,
            limit,
            trigger,
            stop_price,
            market_hours,
            yes,
        } => {
            if read_only {
                anyhow::bail!("read-only mode: pass --read-write to place orders");
            }
            if *r#type == OrderType::Limit && limit.is_none() {
                anyhow::bail!("Limit price required for limit orders (--limit)");
            }
            let type_str = if *r#type == OrderType::Market {
                "market".to_string()
            } else {
                format!("limit @ ${:.2}", limit.unwrap())
            };
            println!("Sell {quantity}x {symbol} at {type_str}");
            if !confirm_order(*yes)? {
                println!("Order cancelled.");
                return Ok(());
            }
            let req = StockOrderRequest {
                symbol: symbol.clone(),
                amount: OrderAmount::Quantity(*quantity),
                side: Side::Sell,
                order_type: *r#type,
                limit_price: *limit,
                trigger: *trigger,
                stop_price: *stop_price,
                time_in_force: TimeInForce::Gtc,
                market_hours: *market_hours,
            };
            let order = client.place_stock_order(&req).await?;
            let id = order.id.clone().unwrap_or_else(|| "unknown".into());
            println!("{}", placed_order_message(&id, order.state.as_deref()));
        }
        OrderCommand::List { status, since } => {
            let mut orders = match status.as_deref() {
                Some("open") => client.get_open_stock_orders().await?,
                _ => client.get_all_stock_orders(since.as_deref()).await?,
            };
            // Best-effort symbol resolution: don't fail the listing if a
            // per-instrument lookup errors out.
            let _ = client.enrich_order_symbols(&mut orders).await;
            let headers = &["ID", "Symbol", "Side", "Qty", "State", "Type", "Created"];
            let rows: Vec<Vec<String>> = orders
                .iter()
                .map(|order| {
                    vec![
                        order.id.clone().unwrap_or_default(),
                        order.symbol.clone().unwrap_or_default(),
                        order.side.clone().unwrap_or_default(),
                        order.quantity.clone().unwrap_or_default(),
                        order.state.clone().unwrap_or_default(),
                        order.order_type.clone().unwrap_or_default(),
                        order.created_at.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            output(format, headers, &rows, &orders);
        }
        OrderCommand::Cancel { order_id } => {
            client.cancel_stock_order(order_id).await?;
            println!("Order {order_id} cancelled.");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::placed_order_message;

    #[test]
    fn rejected_state_warns() {
        let msg = placed_order_message("abc123", Some("rejected"));
        assert!(
            msg.contains("was not placed"),
            "expected 'was not placed' in: {msg}"
        );
        assert!(msg.contains("rejected"), "expected state in: {msg}");
        assert!(msg.contains("abc123"), "expected order id in: {msg}");
    }

    #[test]
    fn rejected_state_case_insensitive() {
        let msg = placed_order_message("abc123", Some("Rejected"));
        assert!(msg.contains("was not placed"), "msg: {msg}");
        assert!(msg.contains("Rejected"), "msg: {msg}");
    }

    #[test]
    fn failed_state_warns() {
        let msg = placed_order_message("xyz789", Some("failed"));
        assert!(
            msg.contains("was not placed"),
            "expected 'was not placed' in: {msg}"
        );
        assert!(msg.contains("failed"), "expected state in: {msg}");
        assert!(msg.contains("xyz789"), "expected order id in: {msg}");
    }

    #[test]
    fn normal_state_shows_placed_with_state() {
        let msg = placed_order_message("order42", Some("confirmed"));
        assert!(
            msg.starts_with("Order placed:"),
            "expected 'Order placed:' prefix: {msg}"
        );
        assert!(msg.contains("order42"), "expected order id in: {msg}");
        assert!(msg.contains("confirmed"), "expected state in: {msg}");
    }

    #[test]
    fn queued_state_shows_placed_with_state() {
        let msg = placed_order_message("order99", Some("queued"));
        assert!(msg.starts_with("Order placed:"), "msg: {msg}");
        assert!(msg.contains("queued"), "msg: {msg}");
    }

    #[test]
    fn none_state_shows_placed_no_state() {
        let msg = placed_order_message("order00", None);
        assert_eq!(msg, "Order placed: order00");
    }
}
