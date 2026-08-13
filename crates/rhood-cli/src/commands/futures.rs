use crate::output::{OutputFormat, output};
use clap::Subcommand;
use rhood_core::{RhoodConfig, RobinhoodClient};

#[derive(Subcommand)]
pub enum FuturesCommand {
    /// Look up a futures contract by symbol
    Contract {
        /// Futures contract symbol (e.g., "ESH26")
        symbol: String,
    },
    /// Get real-time futures quotes
    Quote {
        /// Futures contract symbols (e.g., ESH26 NQM26)
        #[arg(required = true)]
        symbols: Vec<String>,
    },
    /// List futures order history
    Orders {
        /// Filter orders updated since this date (YYYY-MM-DD)
        #[arg(long, value_parser = crate::utils::validation::parse_date)]
        since: Option<String>,
    },
    /// List futures positions
    Positions,
    /// Show futures account ID
    Account,
}

pub async fn run(
    cmd: &FuturesCommand,
    format: OutputFormat,
    config: RhoodConfig,
) -> anyhow::Result<()> {
    let client = RobinhoodClient::with_config(config)?;
    super::ensure_logged_in(&client).await?;
    match cmd {
        FuturesCommand::Contract { symbol } => {
            let contract = client.get_futures_contract(symbol).await?;
            let headers = &[
                "ID",
                "Symbol",
                "Display",
                "Description",
                "Multiplier",
                "Expiration",
                "State",
            ];
            let rows = vec![vec![
                contract.id.clone().unwrap_or_default(),
                contract.symbol.clone().unwrap_or_default(),
                contract.display_symbol.clone().unwrap_or_default(),
                contract.description.clone().unwrap_or_default(),
                contract.multiplier.clone().unwrap_or_default(),
                contract.expiration.clone().unwrap_or_default(),
                contract.state.clone().unwrap_or_default(),
            ]];
            output(format, headers, &rows, &[contract]);
        }
        FuturesCommand::Quote { symbols } => {
            let symbol_refs: Vec<&str> = symbols.iter().map(String::as_str).collect();
            // Core pairs each quote with the symbol it answers; the CLI renders
            // the authoritative wire symbol from the quote itself, so it drops
            // the alias here rather than changing its output shape.
            let quotes: Vec<_> = client
                .get_futures_quotes(&symbol_refs)
                .await?
                .into_iter()
                .map(|(_requested_symbol, quote)| quote)
                .collect();
            let headers = &["Symbol", "Bid", "Ask", "Last", "State", "Updated"];
            let rows: Vec<Vec<String>> = quotes
                .iter()
                .map(|quote| {
                    vec![
                        quote.symbol.clone().unwrap_or_default(),
                        quote.bid_price.clone().unwrap_or_default(),
                        quote.ask_price.clone().unwrap_or_default(),
                        quote.last_trade_price.clone().unwrap_or_default(),
                        quote.state.clone().unwrap_or_default(),
                        quote.updated_at.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            output(format, headers, &rows, &quotes);
        }
        FuturesCommand::Orders { since } => {
            let orders = client.get_all_futures_orders(since.as_deref()).await?;
            let headers = &["ID", "State", "Qty", "Filled", "Avg Price", "Created"];
            let rows: Vec<Vec<String>> = orders
                .iter()
                .map(|order| {
                    vec![
                        order.order_id.clone().unwrap_or_default(),
                        order.order_state.clone().unwrap_or_default(),
                        order.quantity.clone().unwrap_or_default(),
                        order.filled_quantity.clone().unwrap_or_default(),
                        order.average_price.clone().unwrap_or_default(),
                        order.created_at.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            output(format, headers, &rows, &orders);
        }
        FuturesCommand::Positions => {
            let positions = client.get_futures_positions().await?;
            let headers = &["Position"];
            let rows = futures_position_rows(&positions)?;
            output(format, headers, &rows, &positions);
        }
        FuturesCommand::Account => {
            let id = client.get_futures_account_id().await?.unwrap_or_default();
            #[derive(serde::Serialize)]
            struct FuturesAccountId {
                futures_account_id: String,
            }
            let payload = FuturesAccountId {
                futures_account_id: id.clone(),
            };
            let headers = &["Futures Account ID"];
            let rows = futures_account_rows(&id);
            output(format, headers, &rows, &payload);
        }
    }
    Ok(())
}

fn futures_position_rows(
    positions: &[rhood_core::models::futures::FuturesPosition],
) -> serde_json::Result<Vec<Vec<String>>> {
    positions
        .iter()
        .map(|position| serde_json::to_string(position).map(|json| vec![json]))
        .collect()
}

fn futures_account_rows(id: &str) -> Vec<Vec<String>> {
    if id.is_empty() {
        Vec::new()
    } else {
        vec![vec![id.to_string()]]
    }
}

#[cfg(test)]
mod tests {
    use super::{FuturesCommand, futures_account_rows, futures_position_rows};
    use clap::{Parser, Subcommand};

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: TestCommand,
    }

    #[derive(Subcommand)]
    enum TestCommand {
        Futures {
            #[command(subcommand)]
            command: FuturesCommand,
        },
    }

    #[test]
    fn positions_subcommand_accepts_no_parameters() {
        let cli = TestCli::try_parse_from(["rhood", "futures", "positions"]).unwrap();
        assert!(matches!(
            cli.command,
            TestCommand::Futures {
                command: FuturesCommand::Positions
            }
        ));
        assert!(TestCli::try_parse_from(["rhood", "futures", "positions", "unexpected"]).is_err());
    }

    #[test]
    fn futures_position_rows_are_single_compact_json_cells_and_handle_empty_input() {
        let position = serde_json::from_value(serde_json::json!({
            "contract": {"symbol": "/ESM26"},
            "quantity": "2"
        }))
        .unwrap();

        assert_eq!(
            futures_position_rows(&[position]).unwrap(),
            vec![vec![
                r#"{"contract":{"symbol":"/ESM26"},"quantity":"2"}"#.to_string()
            ]]
        );
        assert!(futures_position_rows(&[]).unwrap().is_empty());
    }

    #[test]
    fn futures_positions_serialize_as_the_raw_array() {
        let positions: Vec<rhood_core::models::futures::FuturesPosition> = vec![
            serde_json::from_value(serde_json::json!({
                "quantity": "2",
                "metadata": {"source": "wire"}
            }))
            .unwrap(),
        ];

        assert_eq!(
            serde_json::to_value(&positions).unwrap(),
            serde_json::json!([{
                "quantity": "2",
                "metadata": {"source": "wire"}
            }])
        );
    }

    #[test]
    fn empty_id_returns_no_rows() {
        let rows = futures_account_rows("");
        assert!(rows.is_empty());
    }

    #[test]
    fn present_id_returns_one_row() {
        let id = "abc-123-def";
        let rows = futures_account_rows(id);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], vec![id.to_string()]);
    }
}
