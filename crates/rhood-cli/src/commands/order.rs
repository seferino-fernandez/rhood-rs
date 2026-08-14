use crate::output::{OutputFormat, output};
use clap::Subcommand;
use rhood_core::models::order::{
    MarketHours, OrderAmount, OrderIntent, OrderSummary, OrderType, Side, StagedOrder,
    StockOrderIntent, StockOrderSummary, SubmittedOrder, TimeInForce, Trigger,
};
use rhood_core::{RhoodConfig, RobinhoodClient};
use std::future::Future;
use std::io::Write;

/// Order-history scope accepted by the order-listing command.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum OrderStatus {
    /// Show only open (cancellable) orders.
    Open,
    /// Show orders in every state.
    All,
}

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
        /// Filter by status
        #[arg(long, value_enum, default_value = "all")]
        status: OrderStatus,
        /// Filter orders updated since this date (YYYY-MM-DD)
        #[arg(long, value_parser = crate::utils::validation::parse_date)]
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

fn staged_order_message(summary: &StockOrderSummary) -> String {
    let side = match summary.side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    };
    let amount = summary.requested_dollar_amount.as_ref().map_or_else(
        || format!("{} shares", summary.quantity),
        |dollars| format!("${dollars}, resolved to {} shares", summary.quantity),
    );
    let price = summary
        .price
        .as_deref()
        .map_or_else(|| "n/a".to_string(), |price| format!("${price}"));
    let stop_price = summary
        .stop_price
        .as_deref()
        .map_or_else(|| "n/a".to_string(), |price| format!("${price}"));
    let quote = match (&summary.bid_price, &summary.ask_price) {
        (Some(bid), Some(ask)) => format!("bid ${bid}, ask ${ask}"),
        _ => "n/a".to_string(),
    };
    let conversion_price = summary
        .conversion_price
        .as_deref()
        .map_or_else(|| "n/a".to_string(), |price| format!("${price}"));
    format!(
        "Staged stock order\n  Account: {}\n  Trade: {} {amount} {}\n  Instrument: {}\n  Type: {}, trigger {}, price {price}, stop price {stop_price}\n  Session: {}, extended hours {}, time in force {}\n  Quote: {quote}, conversion price {conversion_price}\n  Client ref: {}\n  Form version: {}",
        summary.account_label,
        side,
        summary.symbol,
        summary.instrument,
        summary.order_type,
        summary.trigger,
        summary.market_hours,
        summary.extended_hours,
        summary.time_in_force,
        summary.client_ref,
        summary.order_form_version,
    )
}

/// Stages an order through core, shows the human the exact execution terms,
/// asks for Confirmation, and submits at most once.
///
/// This is the whole trade-authorisation path for both `buy` and `sell`, kept
/// in one place so the two subcommands cannot drift apart in what they show or
/// when they submit.
async fn stage_confirm_submit(
    client: &RobinhoodClient,
    intent: OrderIntent,
    yes: bool,
    format: OutputFormat,
    writers: StageWriters<'_>,
) -> anyhow::Result<()> {
    stage_confirm_submit_with(
        format,
        intent,
        |intent| client.prepare_order(intent),
        || confirm_order(yes),
        |staged| client.submit_order(staged),
        writers,
        |outcome, out| render_order_outcome(format, outcome, out),
    )
    .await
}

/// [`stage_confirm_submit`] with its effects supplied by the caller.
///
/// Preparation, Confirmation, submission, and outcome rendering are parameters
/// so tests can observe them without a brokerage, a terminal, or a network.
/// Production always reaches this through [`stage_confirm_submit`], which binds
/// them to the real client, real prompt, and real renderer.
enum OrderOutcome<'a> {
    Cancelled,
    Submitted(&'a SubmittedOrder),
}

struct StageWriters<'a> {
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

async fn stage_confirm_submit_with<PrepareFut, SubmitFut, RenderOutcome>(
    format: OutputFormat,
    intent: OrderIntent,
    prepare: impl FnOnce(OrderIntent) -> PrepareFut,
    confirm: impl FnOnce() -> anyhow::Result<bool>,
    submit: impl FnOnce(StagedOrder) -> SubmitFut,
    writers: StageWriters<'_>,
    render_outcome: RenderOutcome,
) -> anyhow::Result<()>
where
    PrepareFut: Future<Output = rhood_core::Result<StagedOrder>>,
    SubmitFut: Future<Output = rhood_core::Result<SubmittedOrder>>,
    RenderOutcome: for<'a> FnOnce(OrderOutcome<'a>, &mut dyn Write) -> anyhow::Result<()>,
{
    let staged = prepare(intent).await?;
    let OrderSummary::Stock(summary) = staged.summary() else {
        anyhow::bail!("core returned an option summary for a stock order");
    };
    if matches!(format, OutputFormat::Table) {
        writeln!(writers.out, "{}", staged_order_message(summary))?;
    } else {
        writeln!(writers.err, "{}", staged_order_message(summary))?;
    }
    if !confirm()? {
        if matches!(format, OutputFormat::Table) {
            writeln!(writers.out, "Order cancelled.")?;
            return Ok(());
        }
        writeln!(writers.err, "Order cancelled.")?;
        return render_outcome(OrderOutcome::Cancelled, writers.out);
    }
    let order = submit(staged).await?;
    render_outcome(OrderOutcome::Submitted(&order), writers.out)
}

fn render_order_outcome(
    format: OutputFormat,
    outcome: OrderOutcome<'_>,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    #[derive(serde::Serialize)]
    struct PlacementOutcome<'a> {
        outcome: &'static str,
        order_id: Option<&'a str>,
        state: Option<&'a str>,
    }

    match format {
        OutputFormat::Table => match outcome {
            OrderOutcome::Cancelled => writeln!(out, "Order cancelled.")?,
            OrderOutcome::Submitted(order) => {
                let id = order.id().unwrap_or("unknown");
                writeln!(out, "{}", placed_order_message(id, order.state()))?;
            }
        },
        OutputFormat::Json | OutputFormat::Csv => {
            let (outcome, order_id, state) = match &outcome {
                OrderOutcome::Cancelled => ("cancelled", None, None),
                OrderOutcome::Submitted(order) => {
                    let outcome = match order.state() {
                        Some(state)
                            if state.eq_ignore_ascii_case("rejected")
                                || state.eq_ignore_ascii_case("failed") =>
                        {
                            "not_placed"
                        }
                        _ => "placed",
                    };
                    (outcome, order.id(), order.state())
                }
            };
            let payload = PlacementOutcome {
                outcome,
                order_id,
                state,
            };
            let headers = &["Outcome", "Order ID", "State"];
            let rows = vec![vec![
                outcome.to_string(),
                order_id.unwrap_or_default().to_string(),
                state.unwrap_or_default().to_string(),
            ]];
            if matches!(format, OutputFormat::Json) {
                serde_json::to_writer_pretty(&mut *out, &payload)?;
                writeln!(out)?;
            } else {
                output(format, headers, &rows, &payload);
            }
        }
    }
    Ok(())
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
            // Core owns which durations the brokerage accepts for an amount;
            // the CLI only supplies its own default when the choice is free.
            let time_in_force = amount.required_time_in_force().unwrap_or(TimeInForce::Gtc);
            let intent = StockOrderIntent {
                account_number: requested_account.map(str::to_owned),
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
            let mut stdout = std::io::stdout();
            let mut stderr = std::io::stderr();
            stage_confirm_submit(
                &client,
                OrderIntent::Stock(intent),
                *yes,
                format,
                StageWriters {
                    out: &mut stdout,
                    err: &mut stderr,
                },
            )
            .await?;
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
            let intent = StockOrderIntent {
                account_number: requested_account.map(str::to_owned),
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
            let mut stdout = std::io::stdout();
            let mut stderr = std::io::stderr();
            stage_confirm_submit(
                &client,
                OrderIntent::Stock(intent),
                *yes,
                format,
                StageWriters {
                    out: &mut stdout,
                    err: &mut stderr,
                },
            )
            .await?;
        }
        OrderCommand::List { status, since } => {
            let account = super::resolve_account_number(&client, requested_account).await?;
            let orders = match status {
                OrderStatus::Open => {
                    client
                        .get_open_stock_orders(&account, since.as_deref())
                        .await?
                }
                OrderStatus::All => {
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
            #[derive(serde::Serialize)]
            struct Cancelled<'a> {
                cancelled: bool,
                order_id: &'a str,
            }
            let payload = Cancelled {
                cancelled: true,
                order_id,
            };
            let headers = &["Cancelled", "Order ID"];
            let rows = vec![vec!["true".to_string(), order_id.clone()]];
            if matches!(format, OutputFormat::Table) {
                println!("Order {order_id} cancelled.");
            } else {
                output(format, headers, &rows, &payload);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use rhood_core::models::order::{
        MarketHours, OrderAmount, OrderIntent, OrderType, Side, StagedOrder, StockOrderIntent,
        StockOrderSummary, SubmittedOrder, TimeInForce, Trigger,
    };
    use rhood_core::{RhoodConfig, RhoodError, RobinhoodClient};

    use super::{
        StageWriters, placed_order_message, render_order_outcome, stage_confirm_submit,
        stage_confirm_submit_with, staged_order_message,
    };
    use crate::output::OutputFormat;

    #[test]
    fn staged_stock_message_renders_core_summary() {
        let message = staged_order_message(&StockOrderSummary {
            account_number: "ABC123".to_string(),
            account_label: "individual ****C123".to_string(),
            symbol: "AAPL".to_string(),
            instrument: "https://api.robinhood.com/instruments/instrument-aapl/".to_string(),
            quantity: "2".to_string(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            trigger: Trigger::Immediate,
            market_hours: MarketHours::RegularHours,
            extended_hours: false,
            price: Some("187.25".to_string()),
            stop_price: None,
            ask_price: None,
            bid_price: None,
            requested_dollar_amount: None,
            conversion_price: None,
            client_ref: "client-ref".to_string(),
            order_form_version: 7,
        });
        for expected in [
            "Staged stock order",
            "Account: individual ****C123",
            "Trade: buy 2 shares AAPL",
            "Instrument: https://api.robinhood.com/instruments/instrument-aapl/",
            "Type: limit, trigger immediate, price $187.25",
            "Session: regular, extended hours false, time in force gtc",
            "Client ref: client-ref",
            "Form version: 7",
        ] {
            assert!(
                message.contains(expected),
                "missing {expected:?}: {message}"
            );
        }
    }

    /// Compile-forcing guard: destructures `StockOrderSummary` without `..`, so adding a
    /// field to the struct fails to compile here until someone decides whether the CLI
    /// confirmation prompt renders it. Every execution-changing field bound below must
    /// also appear in the rendered message; anything explicitly ignored must justify why
    /// it is safe to omit from the human-facing confirmation surface.
    #[test]
    fn staged_order_message_renders_every_execution_changing_field() {
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
            ref conversion_price,
            ref client_ref,
            order_form_version,
        } = summary;

        let message = staged_order_message(&summary);

        assert!(message.contains(account_label), "message: {message}");
        assert!(message.contains(symbol), "message: {message}");
        assert!(message.contains(instrument), "message: {message}");
        assert!(message.contains(quantity), "message: {message}");
        assert!(message.contains(side_debug_str(side)), "message: {message}");
        assert!(
            message.contains(&order_type.to_string()),
            "message: {message}"
        );
        assert!(
            message.contains(&time_in_force.to_string()),
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
        assert!(
            message.contains(
                conversion_price
                    .as_deref()
                    .expect("conversion_price set in fixture")
            ),
            "message missing conversion price: {message}"
        );
        assert!(message.contains(client_ref), "message: {message}");
        assert!(
            message.contains(&order_form_version.to_string()),
            "message: {message}"
        );
    }

    fn side_debug_str(side: Side) -> &'static str {
        match side {
            Side::Buy => "buy",
            Side::Sell => "sell",
        }
    }

    // End-to-end proofs for the trade-authorisation path. Preparation,
    // Confirmation, and submission are injected, so each test can count how
    // many times each effect ran -- a submission that happens after a decline,
    // or twice after one acceptance, is the failure these exist to catch.

    fn stock_intent() -> OrderIntent {
        OrderIntent::Stock(StockOrderIntent {
            account_number: Some("ABC123".to_string()),
            symbol: "AAPL".to_string(),
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

    fn staged_stock() -> StagedOrder {
        let now = chrono_now_ms();
        serde_json::from_value(serde_json::json!({
            "payload": {
                "kind": "Stock",
                "payload": {
                    "account": "https://api.robinhood.com/accounts/ABC123/",
                    "instrument": "https://api.robinhood.com/instruments/instrument-aapl/",
                    "symbol": "AAPL",
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
                    "ref_id": "cli-client-ref-4f21",
                    "order_form_version": 7
                }
            },
            "summary": {
                "kind": "Stock",
                "summary": {
                    "account_number": "ABC123",
                    "account_label": "individual ****C123",
                    "symbol": "AAPL",
                    "instrument": "https://api.robinhood.com/instruments/instrument-aapl/",
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
                    "client_ref": "cli-client-ref-4f21",
                    "order_form_version": 7
                }
            },
            "prepared_at_unix_ms": now,
            "expires_at_unix_ms": now.saturating_add(60_000)
        }))
        .unwrap()
    }

    fn chrono_now_ms() -> i64 {
        std::time::UNIX_EPOCH
            .elapsed()
            .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or_default()
    }

    fn submitted_stock() -> SubmittedOrder {
        serde_json::from_value(serde_json::json!({
            "kind": "Stock",
            "order": {"id": "stock-order-id", "state": "queued"}
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn json_mode_writes_only_the_outcome_document_to_stdout() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        stage_confirm_submit_with(
            OutputFormat::Json,
            stock_intent(),
            |_| async { Ok(staged_stock()) },
            || Ok(true),
            |_| async { Ok(submitted_stock()) },
            StageWriters {
                out: &mut stdout,
                err: &mut stderr,
            },
            |outcome, out| render_order_outcome(OutputFormat::Json, outcome, out),
        )
        .await
        .unwrap();

        let document: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
        assert_eq!(document["outcome"], "placed");
        assert_eq!(
            stdout,
            concat!(
                "{\n",
                "  \"outcome\": \"placed\",\n",
                "  \"order_id\": \"stock-order-id\",\n",
                "  \"state\": \"queued\"\n",
                "}\n",
            )
            .as_bytes()
        );
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            concat!(
                "Staged stock order\n",
                "  Account: individual ****C123\n",
                "  Trade: buy 2 shares AAPL\n",
                "  Instrument: https://api.robinhood.com/instruments/instrument-aapl/\n",
                "  Type: limit, trigger immediate, price $187.25, stop price n/a\n",
                "  Session: regular, extended hours false, time in force gtc\n",
                "  Quote: n/a, conversion price n/a\n",
                "  Client ref: cli-client-ref-4f21\n",
                "  Form version: 7\n",
            )
        );
    }

    #[tokio::test]
    async fn json_mode_routes_a_decline_to_stderr_and_its_outcome_to_stdout() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        stage_confirm_submit_with(
            OutputFormat::Json,
            stock_intent(),
            |_| async { Ok(staged_stock()) },
            || Ok(false),
            |_| async { panic!("a declined order must never be submitted") },
            StageWriters {
                out: &mut stdout,
                err: &mut stderr,
            },
            |outcome, out| render_order_outcome(OutputFormat::Json, outcome, out),
        )
        .await
        .unwrap();

        assert_eq!(
            stdout,
            concat!(
                "{\n",
                "  \"outcome\": \"cancelled\",\n",
                "  \"order_id\": null,\n",
                "  \"state\": null\n",
                "}\n",
            )
            .as_bytes()
        );
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            concat!(
                "Staged stock order\n",
                "  Account: individual ****C123\n",
                "  Trade: buy 2 shares AAPL\n",
                "  Instrument: https://api.robinhood.com/instruments/instrument-aapl/\n",
                "  Type: limit, trigger immediate, price $187.25, stop price n/a\n",
                "  Session: regular, extended hours false, time in force gtc\n",
                "  Quote: n/a, conversion price n/a\n",
                "  Client ref: cli-client-ref-4f21\n",
                "  Form version: 7\n",
                "Order cancelled.\n",
            )
        );
    }

    #[tokio::test]
    async fn table_mode_keeps_the_existing_stdout_transcript() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        stage_confirm_submit_with(
            OutputFormat::Table,
            stock_intent(),
            |_| async { Ok(staged_stock()) },
            || Ok(true),
            |_| async { Ok(submitted_stock()) },
            StageWriters {
                out: &mut stdout,
                err: &mut stderr,
            },
            |outcome, out| render_order_outcome(OutputFormat::Table, outcome, out),
        )
        .await
        .unwrap();

        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            concat!(
                "Staged stock order\n",
                "  Account: individual ****C123\n",
                "  Trade: buy 2 shares AAPL\n",
                "  Instrument: https://api.robinhood.com/instruments/instrument-aapl/\n",
                "  Type: limit, trigger immediate, price $187.25, stop price n/a\n",
                "  Session: regular, extended hours false, time in force gtc\n",
                "  Quote: n/a, conversion price n/a\n",
                "  Client ref: cli-client-ref-4f21\n",
                "  Form version: 7\n",
                "Order placed: stock-order-id (state: queued)\n",
            )
        );
        assert!(
            stderr.is_empty(),
            "table mode must not use stderr: {stderr:?}"
        );
    }

    #[tokio::test]
    async fn decline_submits_nothing() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        stage_confirm_submit_with(
            OutputFormat::Table,
            stock_intent(),
            |_| async { Ok(staged_stock()) },
            || Ok(false),
            |_| async { panic!("a declined order must never be submitted") },
            StageWriters {
                out: &mut out,
                err: &mut err,
            },
            |outcome, out| render_order_outcome(OutputFormat::Table, outcome, out),
        )
        .await
        .unwrap();

        let rendered = String::from_utf8(out).unwrap();
        assert!(rendered.contains("Staged stock order"), "{rendered}");
        assert!(rendered.contains("Order cancelled."), "{rendered}");
        assert!(!rendered.contains("Order placed"), "{rendered}");
    }

    #[tokio::test]
    async fn interactive_acceptance_submits_exactly_once() {
        let submissions = AtomicUsize::new(0);
        let confirmations = AtomicUsize::new(0);
        let mut out = Vec::new();
        let mut err = Vec::new();
        stage_confirm_submit_with(
            OutputFormat::Table,
            stock_intent(),
            |_| async { Ok(staged_stock()) },
            || {
                confirmations.fetch_add(1, Ordering::SeqCst);
                Ok(true)
            },
            |_| async {
                submissions.fetch_add(1, Ordering::SeqCst);
                Ok(submitted_stock())
            },
            StageWriters {
                out: &mut out,
                err: &mut err,
            },
            |outcome, out| render_order_outcome(OutputFormat::Table, outcome, out),
        )
        .await
        .unwrap();

        assert_eq!(submissions.load(Ordering::SeqCst), 1);
        assert_eq!(confirmations.load(Ordering::SeqCst), 1);
        let rendered = String::from_utf8(out).unwrap();
        assert!(
            rendered.contains("Order placed: stock-order-id"),
            "{rendered}"
        );
    }

    #[tokio::test]
    async fn yes_flag_submits_exactly_once_without_consulting_the_terminal() {
        // `confirm_order(true)` returns before it touches stdin, so the real
        // `--yes` path never prompts. Binding the confirm effect to a panic
        // here proves the flag short-circuits Confirmation rather than merely
        // answering it.
        let submissions = AtomicUsize::new(0);
        let mut out = Vec::new();
        let mut err = Vec::new();
        stage_confirm_submit_with(
            OutputFormat::Table,
            stock_intent(),
            |_| async { Ok(staged_stock()) },
            || super::confirm_order(true),
            |_| async {
                submissions.fetch_add(1, Ordering::SeqCst);
                Ok(submitted_stock())
            },
            StageWriters {
                out: &mut out,
                err: &mut err,
            },
            |outcome, out| render_order_outcome(OutputFormat::Table, outcome, out),
        )
        .await
        .unwrap();

        assert_eq!(submissions.load(Ordering::SeqCst), 1);
        let rendered = String::from_utf8(out).unwrap();
        assert!(
            rendered.contains("Order placed: stock-order-id"),
            "{rendered}"
        );
    }

    #[tokio::test]
    async fn uncertain_submission_reports_reconciliation_and_client_reference() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let error = stage_confirm_submit_with(
            OutputFormat::Table,
            stock_intent(),
            |_| async { Ok(staged_stock()) },
            || Ok(true),
            |staged| {
                let client_ref = staged.client_reference().to_string();
                async move {
                    Err(RhoodError::SubmissionUncertain {
                        client_ref,
                        message: "request timed out after dispatch".to_string(),
                    })
                }
            },
            StageWriters {
                out: &mut out,
                err: &mut err,
            },
            |outcome, out| render_order_outcome(OutputFormat::Table, outcome, out),
        )
        .await
        .unwrap_err();

        let reported = error.to_string();
        assert!(
            reported.contains("cli-client-ref-4f21"),
            "the frozen client reference must survive to the user: {reported}"
        );
        assert!(
            reported.contains("Reconcile"),
            "uncertainty must carry reconciliation guidance: {reported}"
        );
        let rendered = String::from_utf8(out).unwrap();
        assert!(!rendered.contains("Order placed"), "{rendered}");
    }

    #[tokio::test]
    async fn production_path_uses_the_real_client_preparation() {
        // The injected variant above proves the orchestration; this proves the
        // production entry point is wired to the real `prepare_order` rather
        // than to a stand-in. A default configuration is read-only, so core
        // refuses before any network call.
        let client = RobinhoodClient::with_config(RhoodConfig::default()).unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let error = stage_confirm_submit(
            &client,
            stock_intent(),
            true,
            OutputFormat::Table,
            StageWriters {
                out: &mut stdout,
                err: &mut stderr,
            },
        )
        .await
        .unwrap_err();

        assert!(
            error
                .downcast_ref::<RhoodError>()
                .is_some_and(|source| { matches!(source, RhoodError::ReadOnlyMode) }),
            "expected the real preparation path to refuse in read-only mode: {error}"
        );
        assert!(
            stdout.is_empty(),
            "staging failure wrote stdout: {stdout:?}"
        );
        assert!(
            stderr.is_empty(),
            "staging failure wrote stderr: {stderr:?}"
        );
    }

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
