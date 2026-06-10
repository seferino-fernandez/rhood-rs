use crate::output::{OutputFormat, output};
use clap::Subcommand;
use rhood_core::models::account::AccountSummary;
use rhood_core::models::document::DocumentType;
use rhood_core::models::user::DayTradeCheck;
use rhood_core::{RhoodConfig, RobinhoodClient};

#[derive(Subcommand)]
pub enum AccountCommand {
    /// List your stock positions
    Positions,
    /// View portfolio summary (equity, market value)
    Portfolio,
    /// View account profile (account number, buying power, cash)
    Profile,
    /// View unified account summary (buying power, equity, cash)
    BuyingPower,
    /// List all stock positions including closed (zero quantity)
    AllPositions,
    /// View dividend payment history
    Dividends {
        /// Filter dividends updated since this date (YYYY-MM-DD)
        #[arg(long, value_parser = crate::utils::validation::parse_date)]
        since: Option<String>,
        /// Show only the total dividend amount earned
        #[arg(long)]
        total: bool,
    },
    /// View interest/sweep payment history
    Interest,
    /// View all transfers (ACH, wire, debit card)
    Transfers,
    /// List account documents (statements, tax forms)
    Documents {
        /// Filter by document type
        #[arg(long, value_enum)]
        doc_type: Option<DocumentType>,
    },
    /// View recent day trades
    DayTrades,
    /// View user profile
    UserProfile,
}

pub async fn run(
    cmd: &AccountCommand,
    format: OutputFormat,
    config: RhoodConfig,
) -> anyhow::Result<()> {
    let client = RobinhoodClient::with_config(config)?;
    super::ensure_logged_in(&client).await?;
    match cmd {
        AccountCommand::Positions => {
            let positions = client.get_positions().await?;
            let headers = &["Symbol", "Quantity", "Avg Buy Price"];
            let rows: Vec<Vec<String>> = positions
                .iter()
                .map(|position| {
                    vec![
                        position_symbol_cell(position),
                        position.quantity.clone().unwrap_or_default(),
                        position.average_buy_price.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            output(format, headers, &rows, &positions);
        }
        AccountCommand::Portfolio => {
            let portfolio = client.get_portfolio().await?;
            let headers = &["Equity", "Market Value", "Ext Hours Equity", "Withdrawable"];
            let rows = vec![vec![
                portfolio.equity.clone().unwrap_or_default(),
                portfolio.market_value.clone().unwrap_or_default(),
                portfolio.extended_hours_equity.clone().unwrap_or_default(),
                portfolio.withdrawable_amount.clone().unwrap_or_default(),
            ]];
            output(format, headers, &rows, &portfolio);
        }
        AccountCommand::Profile => {
            let profile = client.get_account_profile().await?;
            let headers = &["Account #", "Type", "Buying Power", "Cash", "Cash Held"];
            let rows = vec![vec![
                profile.account_number.clone().unwrap_or_default(),
                profile.account_type.clone().unwrap_or_default(),
                profile.buying_power.clone().unwrap_or_default(),
                profile.cash.clone().unwrap_or_default(),
                profile.cash_held_for_orders.clone().unwrap_or_default(),
            ]];
            output(format, headers, &rows, &profile);
        }
        AccountCommand::BuyingPower => {
            let summary = client.get_account_summary().await?;
            let headers = &[
                "Buying Power",
                "Total Equity",
                "Portfolio Equity",
                "Total Market Value",
                "Uninvested Cash",
                "Withdrawable Cash",
            ];
            let rows = buying_power_rows(&summary);
            output(format, headers, &rows, &summary);
        }
        AccountCommand::AllPositions => {
            let positions = client.get_all_positions().await?;
            let headers = &["Symbol", "Quantity", "Avg Buy Price"];
            let rows: Vec<Vec<String>> = positions
                .iter()
                .map(|position| {
                    vec![
                        position_symbol_cell(position),
                        position.quantity.clone().unwrap_or_default(),
                        position.average_buy_price.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            output(format, headers, &rows, &positions);
        }
        AccountCommand::Dividends { since, total } => {
            if *total {
                let total_amount = client.get_total_dividends().await?;
                #[derive(serde::Serialize)]
                struct DividendTotal {
                    total: String,
                }
                let payload = DividendTotal {
                    total: total_amount.clone(),
                };
                let headers = &["Total Dividends"];
                let rows = vec![vec![format!("${total_amount}")]];
                output(format, headers, &rows, &payload);
            } else {
                let dividends = client.get_dividends(since.as_deref()).await?;
                let headers = &[
                    "ID",
                    "Symbol",
                    "Amount",
                    "Rate",
                    "State",
                    "Payable Date",
                    "Paid At",
                ];
                let rows: Vec<Vec<String>> = dividends
                    .iter()
                    .map(|dividend| {
                        vec![
                            dividend.id.clone().unwrap_or_default(),
                            dividend.symbol.clone().unwrap_or_default(),
                            dividend.amount.clone().unwrap_or_default(),
                            dividend.rate.clone().unwrap_or_default(),
                            dividend.state.clone().unwrap_or_default(),
                            dividend.payable_date.clone().unwrap_or_default(),
                            dividend.paid_at.clone().unwrap_or_default(),
                        ]
                    })
                    .collect();
                output(format, headers, &rows, &dividends);
            }
        }
        AccountCommand::Interest => {
            let payments = client.get_interest_payments().await?;
            let headers = &["ID", "Amount", "Payout Type", "Pay Date"];
            let rows: Vec<Vec<String>> = payments
                .iter()
                .map(|payment| {
                    vec![
                        payment.display_id(),
                        payment.display_amount(),
                        payment.display_payout_type(),
                        payment.display_pay_date(),
                    ]
                })
                .collect();
            output(format, headers, &rows, &payments);
        }
        AccountCommand::Transfers => {
            let transfers = client.get_transfers().await?;
            let headers = &["ID", "Type", "Amount", "Direction", "State", "Created At"];
            let rows: Vec<Vec<String>> = transfers
                .iter()
                .map(|transfer| {
                    vec![
                        transfer.id.clone().unwrap_or_default(),
                        transfer.transfer_type.clone().unwrap_or_default(),
                        transfer.amount.clone().unwrap_or_default(),
                        transfer.direction.clone().unwrap_or_default(),
                        transfer.state.clone().unwrap_or_default(),
                        transfer.created_at.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            output(format, headers, &rows, &transfers);
        }
        AccountCommand::Documents { doc_type } => {
            let documents = client.get_documents(*doc_type).await?;
            let headers = &["ID", "Type", "Date", "Created At", "Download URL"];
            let rows = document_rows(&documents);
            output(format, headers, &rows, &documents);
        }
        AccountCommand::DayTrades => {
            let check = client.get_day_trades().await?;
            let headers = &[
                "Equity Day Trades",
                "Option Day Trades",
                "Total Day Trade Count",
                "Pattern Day Trader",
            ];
            let rows = day_trades_rows(&check);
            output(format, headers, &rows, &check);
        }
        AccountCommand::UserProfile => {
            let user = client.get_user_profile().await?;
            let headers = &["ID", "Username", "First Name", "Last Name", "Email"];
            let rows = vec![vec![
                user.id.clone().unwrap_or_default(),
                user.username.clone().unwrap_or_default(),
                user.first_name.clone().unwrap_or_default(),
                user.last_name.clone().unwrap_or_default(),
                user.email.clone().unwrap_or_default(),
            ]];
            output(format, headers, &rows, &user);
        }
    }
    Ok(())
}

/// Returns the display value for the first column of a position row.
///
/// Prefers the resolved ticker symbol; falls back to the raw instrument URL
/// when the symbol has not been resolved, and returns an empty string when
/// neither is available.
pub fn position_symbol_cell(position: &rhood_core::models::account::Position) -> String {
    position
        .symbol
        .clone()
        .or_else(|| position.instrument.clone())
        .unwrap_or_default()
}

/// Build a single-row table for the [`AccountSummary`] buying-power view.
pub fn buying_power_rows(summary: &AccountSummary) -> Vec<Vec<String>> {
    let money = |field: &Option<rhood_core::models::dividend::MoneyAmount>| -> String {
        field
            .as_ref()
            .and_then(|m| m.amount.clone())
            .unwrap_or_default()
    };
    vec![vec![
        money(&summary.account_buying_power),
        money(&summary.total_equity),
        money(&summary.portfolio_equity),
        money(&summary.total_market_value),
        money(&summary.uninvested_cash),
        money(&summary.withdrawable_cash),
    ]]
}

/// Build table rows for the documents list view.
///
/// Columns: ID, Type, Date, Created At, Download URL.
pub fn document_rows(documents: &[rhood_core::models::document::Document]) -> Vec<Vec<String>> {
    documents
        .iter()
        .map(|document| {
            vec![
                document.id.clone().unwrap_or_default(),
                document.document_type.clone().unwrap_or_default(),
                document.date.clone().unwrap_or_default(),
                document.created_at.clone().unwrap_or_default(),
                document.download_url.clone().unwrap_or_default(),
            ]
        })
        .collect()
}

/// Build a single-row table for the [`DayTradeCheck`] day-trades view.
pub fn day_trades_rows(check: &DayTradeCheck) -> Vec<Vec<String>> {
    vec![vec![
        check.equity_day_trades.len().to_string(),
        check.option_day_trades.len().to_string(),
        check.day_trade_count.to_string(),
        check.flagged_as_pattern_day_trader.to_string(),
    ]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhood_core::models::account::{AccountSummary, Position};
    use rhood_core::models::dividend::MoneyAmount;
    use rhood_core::models::document::Document;
    use rhood_core::models::user::DayTradeCheck;

    fn money(amount: &str) -> Option<MoneyAmount> {
        Some(MoneyAmount {
            amount: Some(amount.to_string()),
            currency_code: Some("USD".to_string()),
            currency_id: None,
        })
    }

    #[test]
    fn buying_power_rows_columns_match_headers() {
        let headers = &[
            "Buying Power",
            "Total Equity",
            "Portfolio Equity",
            "Total Market Value",
            "Uninvested Cash",
            "Withdrawable Cash",
        ];
        let summary = AccountSummary {
            account_buying_power: money("5000.00"),
            total_equity: money("12000.00"),
            portfolio_equity: money("11500.00"),
            total_market_value: money("10000.00"),
            uninvested_cash: money("2000.00"),
            withdrawable_cash: money("1800.00"),
            ..Default::default()
        };
        let rows = buying_power_rows(&summary);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), headers.len());
        assert_eq!(rows[0][0], "5000.00");
        assert_eq!(rows[0][1], "12000.00");
        assert_eq!(rows[0][5], "1800.00");
    }

    #[test]
    fn buying_power_rows_handles_missing_fields() {
        let headers = &[
            "Buying Power",
            "Total Equity",
            "Portfolio Equity",
            "Total Market Value",
            "Uninvested Cash",
            "Withdrawable Cash",
        ];
        let summary = AccountSummary::default();
        let rows = buying_power_rows(&summary);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), headers.len());
        // All cells should be empty strings when no data
        for cell in &rows[0] {
            assert_eq!(cell, "");
        }
    }

    #[test]
    fn day_trades_rows_columns_match_headers() {
        let headers = &[
            "Equity Day Trades",
            "Option Day Trades",
            "Total Day Trade Count",
            "Pattern Day Trader",
        ];
        let check = DayTradeCheck {
            equity_day_trades: vec![
                serde_json::json!({"symbol": "AAPL"}),
                serde_json::json!({"symbol": "TSLA"}),
            ],
            option_day_trades: vec![serde_json::json!({"symbol": "SPY"})],
            day_trade_count: 3,
            flagged_as_pattern_day_trader: false,
        };
        let rows = day_trades_rows(&check);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), headers.len());
        assert_eq!(rows[0][0], "2"); // equity trades vec length
        assert_eq!(rows[0][1], "1"); // option trades vec length
        assert_eq!(rows[0][2], "3"); // day_trade_count
        assert_eq!(rows[0][3], "false");
    }

    #[test]
    fn day_trades_rows_pdt_flag_true() {
        let check = DayTradeCheck {
            equity_day_trades: vec![],
            option_day_trades: vec![],
            day_trade_count: 5,
            flagged_as_pattern_day_trader: true,
        };
        let rows = day_trades_rows(&check);
        assert_eq!(rows[0][3], "true");
    }

    #[test]
    fn dividend_total_row_has_dollar_prefix() {
        let total_amount = "0.19";
        assert_eq!(format!("${total_amount}"), "$0.19");
    }

    #[test]
    fn document_rows_includes_download_url_as_last_column() {
        let headers = &["ID", "Type", "Date", "Created At", "Download URL"];
        let documents = vec![Document {
            id: Some("abc123".to_string()),
            document_type: Some("account_statement".to_string()),
            date: Some("2024-01-01".to_string()),
            created_at: Some("2024-01-02T00:00:00Z".to_string()),
            updated_at: None,
            url: None,
            download_url: Some("https://example.com/doc.pdf".to_string()),
        }];
        let rows = document_rows(&documents);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), headers.len());
        assert_eq!(rows[0][0], "abc123");
        assert_eq!(rows[0][1], "account_statement");
        assert_eq!(rows[0][2], "2024-01-01");
        assert_eq!(rows[0][3], "2024-01-02T00:00:00Z");
        assert_eq!(rows[0][4], "https://example.com/doc.pdf");
    }

    #[test]
    fn document_rows_missing_download_url_is_empty_string() {
        let documents = vec![Document {
            id: Some("xyz".to_string()),
            document_type: None,
            date: None,
            created_at: None,
            updated_at: None,
            url: None,
            download_url: None,
        }];
        let rows = document_rows(&documents);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), 5);
        assert_eq!(rows[0][4], ""); // missing download_url becomes empty string
    }

    #[test]
    fn document_rows_empty_list_returns_empty_rows() {
        let rows = document_rows(&[]);
        assert!(rows.is_empty());
    }

    fn make_position(symbol: Option<&str>, instrument: Option<&str>) -> Position {
        Position {
            account: None,
            instrument: instrument.map(str::to_string),
            symbol: symbol.map(str::to_string),
            average_buy_price: None,
            quantity: None,
            shares_held_for_buys: None,
            shares_held_for_sells: None,
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn position_symbol_cell_prefers_symbol_over_instrument() {
        let pos = make_position(
            Some("AAPL"),
            Some("https://api.robinhood.com/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e/"),
        );
        assert_eq!(position_symbol_cell(&pos), "AAPL");
    }

    #[test]
    fn position_symbol_cell_falls_back_to_instrument_url() {
        let url = "https://api.robinhood.com/instruments/450dfc6d-5510-4d40-abfb-f633b7d9be3e/";
        let pos = make_position(None, Some(url));
        assert_eq!(position_symbol_cell(&pos), url);
    }

    #[test]
    fn position_symbol_cell_returns_empty_when_both_none() {
        let pos = make_position(None, None);
        assert_eq!(position_symbol_cell(&pos), "");
    }
}
