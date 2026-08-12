use crate::output::{OutputFormat, output};
use clap::Subcommand;
use rhood_core::models::order::{
    MarketHours, OrderAmount, OrderType, PreparedStockOrderPreview, Side, StockOrderRequest,
    TimeInForce, Trigger,
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
        /// Dollar intent of at least $1, converted to a GFD fractional-share estimate
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

fn prepared_order_message(preview: &PreparedStockOrderPreview, account_label: &str) -> String {
    let side = match preview.side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    };
    let amount = preview.requested_dollar_amount.as_ref().map_or_else(
        || format!("{} shares", preview.quantity),
        |dollars| format!("${dollars}, resolved to {} shares", preview.quantity),
    );
    let price = preview
        .price
        .as_deref()
        .map_or_else(|| "n/a".to_string(), |price| format!("${price}"));
    let quote = match (&preview.bid_price, &preview.ask_price) {
        (Some(bid), Some(ask)) => format!("bid ${bid}, ask ${ask}"),
        _ => "n/a".to_string(),
    };
    format!(
        "Prepared stock order\n  Account: {account_label}\n  Trade: {} {amount} {}\n  Type: {}, trigger {}, price {price}\n  Session: {}, extended hours {}, time in force {}\n  Quote: {quote}\n  Client ref: {}\n  Form version: {}",
        side,
        preview.symbol,
        preview.order_type,
        preview.trigger,
        preview.market_hours,
        preview.extended_hours,
        preview.time_in_force,
        preview.client_ref,
        preview.order_form_version,
    )
}

pub async fn run(
    cmd: &OrderCommand,
    format: OutputFormat,
    config: RhoodConfig,
    requested_account: Option<&str>,
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
                #[expect(
                    clippy::unreachable,
                    reason = "Clap's amount_spec group prevents both amount flags from reaching this command"
                )]
                _ => unreachable!("clap group prevents both"),
            };
            let time_in_force = if matches!(amount, OrderAmount::DollarAmount(_)) {
                TimeInForce::Gfd
            } else {
                TimeInForce::Gtc
            };
            let mut req = StockOrderRequest {
                account_number: "PENDING".to_string(),
                symbol: symbol.clone(),
                amount,
                side: Side::Buy,
                order_type: *r#type,
                limit_price: *limit,
                trigger: *trigger,
                stop_price: *stop_price,
                time_in_force,
                market_hours: *market_hours,
            };
            rhood_core::endpoints::orders::validate_stock_order(&req)?;
            let account = super::resolve_account(&client, requested_account).await?;
            req.account_number = account
                .canonical_account_number()
                .ok_or_else(|| anyhow::anyhow!("resolved account has no canonical number"))?
                .to_string();
            let prepared = client.prepare_stock_order(&req).await?;
            println!(
                "{}",
                prepared_order_message(&prepared.preview(), &account.selection_label())
            );
            if !confirm_order(*yes)? {
                println!("Order cancelled.");
                return Ok(());
            }
            let order = client.submit_prepared_stock_order(prepared).await?;
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
            let mut req = StockOrderRequest {
                account_number: "PENDING".to_string(),
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
            rhood_core::endpoints::orders::validate_stock_order(&req)?;
            let account = super::resolve_account(&client, requested_account).await?;
            req.account_number = account
                .canonical_account_number()
                .ok_or_else(|| anyhow::anyhow!("resolved account has no canonical number"))?
                .to_string();
            let prepared = client.prepare_stock_order(&req).await?;
            println!(
                "{}",
                prepared_order_message(&prepared.preview(), &account.selection_label())
            );
            if !confirm_order(*yes)? {
                println!("Order cancelled.");
                return Ok(());
            }
            let order = client.submit_prepared_stock_order(prepared).await?;
            let id = order.id.clone().unwrap_or_else(|| "unknown".into());
            println!("{}", placed_order_message(&id, order.state.as_deref()));
        }
        OrderCommand::List { status, since } => {
            let account = super::resolve_account_number(&client, requested_account).await?;
            let orders = match status.as_deref() {
                Some("open") => client.get_open_stock_orders(&account).await?,
                _ => {
                    client
                        .get_all_stock_orders(&account, since.as_deref())
                        .await?
                }
            };
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
