use crate::output::{OutputFormat, output};
use crate::utils::validation::STRIKE_PRICE_DECIMALS;
use clap::Subcommand;
use rhood_core::models::option::{OptionContractSpec, OptionType};
use rhood_core::models::order::OptionOrder;
use rhood_core::{RhoodConfig, RobinhoodClient};

#[derive(Subcommand)]
pub enum OptionCommand {
    /// List your open option positions
    Positions,
    /// List option orders (all or open only)
    Orders {
        /// Show only open orders (otherwise lists all)
        #[arg(long)]
        open: bool,
        /// Filter orders updated since this date (YYYY-MM-DD)
        #[arg(long, value_parser = crate::utils::validation::parse_date)]
        since: Option<String>,
    },
    /// Cancel an open option order by ID
    CancelOrder {
        #[arg(value_parser = crate::utils::validation::parse_uuid)]
        order_id: String,
    },
    /// Get live quotes for option contracts (bid/ask, Greeks, volume, OI)
    Quote {
        /// Stock symbol (e.g. NKE)
        symbol: String,
        /// Strike prices (one per contract, matched positionally with --expiry and --type)
        #[arg(long, required = true)]
        strike: Vec<f64>,
        /// Expiration dates in YYYY-MM-DD format (one per contract)
        #[arg(long, required = true, value_parser = crate::utils::validation::parse_date)]
        expiry: Vec<String>,
        /// Contract type (one per contract)
        #[arg(long = "type", value_enum, required = true)]
        option_type: Vec<OptionType>,
        /// Show full detail (Greeks, probability, high/low)
        #[arg(long)]
        detail: bool,
    },
}

/// Extracts (strike, expiry, option_type) display strings from an option order's legs.
///
/// - Single leg: returns that leg's `strike_price`, `expiration_date`, and `type`.
/// - Multiple legs (spread): comma-joins each field across all legs so the user can see
///   every contract involved (e.g. strikes `"310.0000,320.0000"`).
/// - No legs / missing field: returns an empty string for that column.
///
/// Legs are stored as `serde_json::Value` because the API returns them as free-form JSON.
fn leg_columns(order: &OptionOrder) -> (String, String, String) {
    let legs = match order.legs.as_deref() {
        Some(legs) if !legs.is_empty() => legs,
        _ => return (String::new(), String::new(), String::new()),
    };

    let extract = |field: &str| -> String {
        legs.iter()
            .map(|leg| {
                leg.get(field)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(",")
    };

    let strike = extract("strike_price");
    let expiry = extract("expiration_date");
    // The API uses the key "option_type" for the contract type within a leg.
    let opt_type = extract("option_type");

    (strike, expiry, opt_type)
}

pub async fn run(
    cmd: &OptionCommand,
    format: OutputFormat,
    config: RhoodConfig,
) -> anyhow::Result<()> {
    let client = RobinhoodClient::with_config(config)?;
    super::ensure_logged_in(&client).await?;
    match cmd {
        OptionCommand::Positions => {
            let positions = client.get_open_option_positions().await?;
            let headers = &["Symbol", "Type", "Quantity", "Avg Price"];
            let rows: Vec<Vec<String>> = positions
                .iter()
                .map(|position| {
                    vec![
                        position.chain_symbol.clone().unwrap_or_default(),
                        position.position_type.clone().unwrap_or_default(),
                        position.quantity.clone().unwrap_or_default(),
                        position.average_price.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            output(format, headers, &rows, &positions);
        }
        OptionCommand::Orders { open, since } => {
            let orders = if *open {
                client.get_open_option_orders().await?
            } else {
                client.get_all_option_orders(since.as_deref()).await?
            };
            let headers = &[
                "ID",
                "Symbol",
                "Strike",
                "Expiry",
                "Type",
                "Direction",
                "Qty",
                "Price",
                "State",
                "Created",
            ];
            let rows: Vec<Vec<String>> = orders
                .iter()
                .map(|order| {
                    let (strike, expiry, opt_type) = leg_columns(order);
                    vec![
                        order.id.clone().unwrap_or_default(),
                        order.chain_symbol.clone().unwrap_or_default(),
                        strike,
                        expiry,
                        opt_type,
                        order.direction.clone().unwrap_or_default(),
                        order.quantity.clone().unwrap_or_default(),
                        order.price.clone().unwrap_or_default(),
                        order.state.clone().unwrap_or_default(),
                        order.created_at.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            output(format, headers, &rows, &orders);
        }
        OptionCommand::CancelOrder { order_id } => {
            client.cancel_option_order(order_id).await?;
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
            output(format, headers, &rows, &payload);
        }
        OptionCommand::Quote {
            symbol,
            strike,
            expiry,
            option_type,
            detail,
        } => {
            if strike.len() != expiry.len() || strike.len() != option_type.len() {
                anyhow::bail!(
                    "Mismatched contract args: got {} --strike, {} --expiry, {} --type (must be equal)",
                    strike.len(),
                    expiry.len(),
                    option_type.len()
                );
            }

            let strike_strings: Vec<String> = strike
                .iter()
                .map(|strike_str| format!("{strike_str:.STRIKE_PRICE_DECIMALS$}"))
                .collect();
            let type_strings: Vec<String> = option_type
                .iter()
                .map(|opt_type| opt_type.to_string())
                .collect();

            let specs: Vec<OptionContractSpec<'_>> = strike_strings
                .iter()
                .zip(expiry.iter())
                .zip(type_strings.iter())
                .map(|((strike_str, exp), opt_type)| OptionContractSpec {
                    strike_price: strike_str,
                    expiration_date: exp,
                    option_type: opt_type,
                })
                .collect();

            let data = client.get_option_market_data(symbol, &specs).await?;

            if *detail {
                let headers = &[
                    "Symbol",
                    "Strike",
                    "Expiry",
                    "Type",
                    "Bid",
                    "Ask",
                    "Last",
                    "Mark",
                    "Delta",
                    "Gamma",
                    "Theta",
                    "Vega",
                    "IV",
                    "Vol",
                    "OI",
                    "P(Profit)",
                    "Break-Even",
                    "High",
                    "Low",
                    "Prev Close",
                ];
                let rows: Vec<Vec<String>> = data
                    .iter()
                    .zip(specs.iter())
                    .map(|(market_data, spec)| {
                        vec![
                            symbol.to_uppercase(),
                            spec.strike_price.to_string(),
                            spec.expiration_date.to_string(),
                            spec.option_type.to_string(),
                            market_data.bid_price.clone().unwrap_or_default(),
                            market_data.ask_price.clone().unwrap_or_default(),
                            market_data.last_trade_price.clone().unwrap_or_default(),
                            market_data.mark_price.clone().unwrap_or_default(),
                            market_data.delta.clone().unwrap_or_default(),
                            market_data.gamma.clone().unwrap_or_default(),
                            market_data.theta.clone().unwrap_or_default(),
                            market_data.vega.clone().unwrap_or_default(),
                            market_data.implied_volatility.clone().unwrap_or_default(),
                            market_data
                                .volume
                                .map_or_else(String::new, |volume| volume.to_string()),
                            market_data
                                .open_interest
                                .map_or_else(String::new, |interest| interest.to_string()),
                            market_data
                                .chance_of_profit_long
                                .clone()
                                .unwrap_or_default(),
                            market_data.break_even_price.clone().unwrap_or_default(),
                            market_data.high_price.clone().unwrap_or_default(),
                            market_data.low_price.clone().unwrap_or_default(),
                            market_data.previous_close_price.clone().unwrap_or_default(),
                        ]
                    })
                    .collect();
                output(format, headers, &rows, &data);
            } else {
                let headers = &[
                    "Symbol", "Strike", "Expiry", "Type", "Bid", "Ask", "Last", "Mark", "Delta",
                    "Vol", "OI",
                ];
                let rows: Vec<Vec<String>> = data
                    .iter()
                    .zip(specs.iter())
                    .map(|(market_data, spec)| {
                        vec![
                            symbol.to_uppercase(),
                            spec.strike_price.to_string(),
                            spec.expiration_date.to_string(),
                            spec.option_type.to_string(),
                            market_data.bid_price.clone().unwrap_or_default(),
                            market_data.ask_price.clone().unwrap_or_default(),
                            market_data.last_trade_price.clone().unwrap_or_default(),
                            market_data.mark_price.clone().unwrap_or_default(),
                            market_data.delta.clone().unwrap_or_default(),
                            market_data
                                .volume
                                .map_or_else(String::new, |volume| volume.to_string()),
                            market_data
                                .open_interest
                                .map_or_else(String::new, |interest| interest.to_string()),
                        ]
                    })
                    .collect();
                output(format, headers, &rows, &data);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhood_core::models::order::OptionOrder;

    fn make_leg(strike: &str, expiry: &str, opt_type: &str) -> serde_json::Value {
        serde_json::json!({
            "strike_price": strike,
            "expiration_date": expiry,
            "option_type": opt_type
        })
    }

    fn base_order() -> OptionOrder {
        OptionOrder {
            id: Some("ord-001".to_string()),
            chain_id: None,
            chain_symbol: Some("AAPL".to_string()),
            direction: Some("debit".to_string()),
            legs: None,
            premium: None,
            price: Some("1.50".to_string()),
            processed_premium: None,
            quantity: Some("1.0000".to_string()),
            state: Some("filled".to_string()),
            time_in_force: None,
            order_type: None,
            created_at: Some("2026-05-01T10:00:00Z".to_string()),
            updated_at: None,
            cancel_url: None,
        }
    }

    #[test]
    fn leg_columns_single_leg_returns_fields() {
        let mut order = base_order();
        order.legs = Some(vec![make_leg("150.0000", "2026-06-20", "call")]);

        let (strike, expiry, opt_type) = leg_columns(&order);

        assert_eq!(strike, "150.0000");
        assert_eq!(expiry, "2026-06-20");
        assert_eq!(opt_type, "call");
    }

    #[test]
    fn leg_columns_two_legs_comma_joins_fields() {
        // Multi-leg spread: each field is joined with a comma across legs.
        let mut order = base_order();
        order.legs = Some(vec![
            make_leg("310.0000", "2026-06-20", "call"),
            make_leg("320.0000", "2026-06-20", "call"),
        ]);

        let (strike, expiry, opt_type) = leg_columns(&order);

        assert_eq!(strike, "310.0000,320.0000");
        assert_eq!(expiry, "2026-06-20,2026-06-20");
        assert_eq!(opt_type, "call,call");
    }

    #[test]
    fn leg_columns_no_legs_returns_empty_strings() {
        let order = base_order(); // legs: None

        let (strike, expiry, opt_type) = leg_columns(&order);

        assert_eq!(strike, "");
        assert_eq!(expiry, "");
        assert_eq!(opt_type, "");
    }

    #[test]
    fn leg_columns_empty_legs_vec_returns_empty_strings() {
        let mut order = base_order();
        order.legs = Some(vec![]);

        let (strike, expiry, opt_type) = leg_columns(&order);

        assert_eq!(strike, "");
        assert_eq!(expiry, "");
        assert_eq!(opt_type, "");
    }

    #[test]
    fn leg_columns_missing_leg_field_yields_empty_segment() {
        // Leg missing "option_type" — should fall back to empty string for that column.
        let mut order = base_order();
        order.legs = Some(vec![serde_json::json!({
            "strike_price": "200.0000",
            "expiration_date": "2026-07-18"
            // "option_type" intentionally absent
        })]);

        let (strike, expiry, opt_type) = leg_columns(&order);

        assert_eq!(strike, "200.0000");
        assert_eq!(expiry, "2026-07-18");
        assert_eq!(opt_type, "");
    }
}
