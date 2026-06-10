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
            let quotes = client.get_futures_quotes(&symbol_refs).await?;
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

fn futures_account_rows(id: &str) -> Vec<Vec<String>> {
    if id.is_empty() {
        Vec::new()
    } else {
        vec![vec![id.to_string()]]
    }
}

#[cfg(test)]
mod tests {
    use super::futures_account_rows;

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
