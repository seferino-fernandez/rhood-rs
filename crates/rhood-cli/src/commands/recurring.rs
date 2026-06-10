use crate::output::{OutputFormat, output};
use clap::Subcommand;
use rhood_core::models::recurring::{
    CreateRecurringRequest, RecurringFrequency, RecurringInvestment, RecurringSource,
    RecurringState, UpdateRecurringRequest,
};
use rhood_core::{RhoodConfig, RobinhoodClient};

#[derive(Subcommand)]
pub enum RecurringCommand {
    /// List all recurring investment schedules
    List,
    /// Create a new recurring investment
    Create {
        /// Stock symbol (e.g., "TSLA")
        symbol: String,
        /// Dollar amount per recurrence
        #[arg(long)]
        amount: f64,
        /// Recurrence frequency
        #[arg(long, value_enum)]
        frequency: RecurringFrequency,
        /// Start date (YYYY-MM-DD)
        #[arg(long, value_parser = crate::utils::validation::parse_date)]
        start_date: String,
        /// Source of funds
        #[arg(long, value_enum, default_value_t = RecurringSource::BuyingPower)]
        source: RecurringSource,
    },
    /// Update an existing recurring investment
    Update {
        /// Schedule ID to update
        schedule_id: String,
        /// New dollar amount
        #[arg(long)]
        amount: Option<f64>,
        /// New frequency
        #[arg(long, value_enum)]
        frequency: Option<RecurringFrequency>,
        /// New state
        #[arg(long, value_enum)]
        state: Option<RecurringState>,
        /// New start date
        #[arg(long, value_parser = crate::utils::validation::parse_date)]
        start_date: Option<String>,
    },
    /// Cancel a recurring investment
    Cancel {
        /// Schedule ID to cancel
        schedule_id: String,
    },
    /// Look up the next investment date
    NextDate {
        /// Recurrence frequency
        #[arg(long, value_enum)]
        frequency: RecurringFrequency,
        /// Start date (YYYY-MM-DD)
        #[arg(long, value_parser = crate::utils::validation::parse_date)]
        start_date: String,
    },
}

/// Build table rows for a slice of [`RecurringInvestment`] values.
///
/// Columns: ID, Symbol, Amount, Frequency, State, Start Date —
/// matching the headers used by every recurring subcommand that
/// returns a schedule.
fn recurring_rows(items: &[RecurringInvestment]) -> Vec<Vec<String>> {
    items
        .iter()
        .map(|recurring| {
            let symbol = recurring
                .investment_asset
                .as_ref()
                .and_then(|asset| asset.asset_symbol.clone())
                .unwrap_or_default();
            let amount = recurring
                .amount
                .as_ref()
                .map(|money| money.amount.clone())
                .unwrap_or_default();
            vec![
                recurring.id.clone().unwrap_or_default(),
                symbol,
                amount,
                recurring.frequency.clone().unwrap_or_default(),
                recurring.state.clone().unwrap_or_default(),
                recurring.start_date.clone().unwrap_or_default(),
            ]
        })
        .collect()
}

pub async fn run(
    cmd: &RecurringCommand,
    format: OutputFormat,
    config: RhoodConfig,
) -> anyhow::Result<()> {
    let client = RobinhoodClient::with_config(config)?;
    super::ensure_logged_in(&client).await?;
    match cmd {
        RecurringCommand::List => {
            let investments = client.get_recurring_investments().await?;
            let headers = &["ID", "Symbol", "Amount", "Frequency", "State", "Start Date"];
            let rows = recurring_rows(&investments);
            output(format, headers, &rows, &investments);
        }
        RecurringCommand::Create {
            symbol,
            amount,
            frequency,
            start_date,
            source,
        } => {
            let req = CreateRecurringRequest {
                symbol: symbol.clone(),
                amount: *amount,
                frequency: *frequency,
                start_date: start_date.clone(),
                source_of_funds: *source,
            };
            let result = client.create_recurring_investment(&req).await?;
            let headers = &["ID", "Symbol", "Amount", "Frequency", "State", "Start Date"];
            let rows = recurring_rows(std::slice::from_ref(&result));
            output(format, headers, &rows, &result);
        }
        RecurringCommand::Update {
            schedule_id,
            amount,
            frequency,
            state,
            start_date,
        } => {
            let req = UpdateRecurringRequest {
                amount: *amount,
                frequency: *frequency,
                state: *state,
                start_date: start_date.clone(),
            };
            let result = client
                .update_recurring_investment(schedule_id, &req)
                .await?;
            let headers = &["ID", "Symbol", "Amount", "Frequency", "State", "Start Date"];
            let rows = recurring_rows(std::slice::from_ref(&result));
            output(format, headers, &rows, &result);
        }
        RecurringCommand::Cancel { schedule_id } => {
            let result = client.cancel_recurring_investment(schedule_id).await?;
            let headers = &["ID", "Symbol", "Amount", "Frequency", "State", "Start Date"];
            let rows = recurring_rows(std::slice::from_ref(&result));
            output(format, headers, &rows, &result);
        }
        RecurringCommand::NextDate {
            frequency,
            start_date,
        } => {
            let result = client
                .get_next_investment_date(*frequency, start_date)
                .await?;
            let headers = &["Frequency", "Start Date", "Next Investment Date"];
            let rows = vec![vec![
                result.frequency.clone().unwrap_or_default(),
                result.start_date.clone().unwrap_or_default(),
                result.next_investment_date.clone().unwrap_or_default(),
            ]];
            output(format, headers, &rows, &result);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhood_core::models::recurring::{InvestmentAsset, MoneyAmount, NextInvestmentDate};

    fn make_recurring(
        id: &str,
        symbol: &str,
        amount: &str,
        frequency: &str,
        state: &str,
        start_date: &str,
    ) -> RecurringInvestment {
        RecurringInvestment {
            id: Some(id.to_string()),
            account_number: None,
            amount: Some(MoneyAmount {
                amount: amount.to_string(),
                currency_code: "USD".to_string(),
            }),
            frequency: Some(frequency.to_string()),
            start_date: Some(start_date.to_string()),
            state: Some(state.to_string()),
            investment_asset: Some(InvestmentAsset {
                asset_id: None,
                asset_symbol: Some(symbol.to_string()),
                asset_type: None,
            }),
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn recurring_rows_produces_correct_columns() {
        let item = make_recurring("abc-123", "TSLA", "25.00", "weekly", "active", "2026-06-01");
        let rows = recurring_rows(std::slice::from_ref(&item));
        assert_eq!(rows.len(), 1, "should produce exactly one row");
        let row = &rows[0];
        assert_eq!(row.len(), 6, "row must have 6 columns");
        assert_eq!(row[0], "abc-123", "column 0 is ID");
        assert_eq!(row[1], "TSLA", "column 1 is Symbol");
        assert_eq!(row[2], "25.00", "column 2 is Amount");
        assert_eq!(row[3], "weekly", "column 3 is Frequency");
        assert_eq!(row[4], "active", "column 4 is State");
        assert_eq!(row[5], "2026-06-01", "column 5 is Start Date");
    }

    #[test]
    fn recurring_rows_handles_missing_optional_fields() {
        let item = RecurringInvestment {
            id: None,
            account_number: None,
            amount: None,
            frequency: None,
            start_date: None,
            state: None,
            investment_asset: None,
            created_at: None,
            updated_at: None,
        };
        let rows = recurring_rows(std::slice::from_ref(&item));
        assert_eq!(rows.len(), 1);
        // All optional fields should fall back to empty string
        for cell in &rows[0] {
            assert_eq!(cell, "", "missing optional fields should be empty string");
        }
    }

    #[test]
    fn recurring_rows_handles_multiple_items() {
        let items = vec![
            make_recurring("id-1", "AAPL", "10.00", "monthly", "active", "2026-01-01"),
            make_recurring("id-2", "MSFT", "50.00", "biweekly", "paused", "2026-02-01"),
        ];
        let rows = recurring_rows(&items);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0], "id-1");
        assert_eq!(rows[1][0], "id-2");
    }

    #[test]
    fn next_date_row_uses_correct_fields() {
        let result = NextInvestmentDate {
            frequency: Some("weekly".to_string()),
            next_investment_date: Some("2026-06-08".to_string()),
            start_date: Some("2026-06-01".to_string()),
        };
        let row = [
            result.frequency.clone().unwrap_or_default(),
            result.start_date.clone().unwrap_or_default(),
            result.next_investment_date.clone().unwrap_or_default(),
        ];
        assert_eq!(row.len(), 3, "NextDate row must have 3 columns");
        assert_eq!(row[0], "weekly", "column 0 is Frequency");
        assert_eq!(row[1], "2026-06-01", "column 1 is Start Date");
        assert_eq!(row[2], "2026-06-08", "column 2 is Next Investment Date");
    }

    #[test]
    fn next_date_row_handles_missing_fields() {
        let result = NextInvestmentDate {
            frequency: None,
            next_investment_date: None,
            start_date: None,
        };
        let row = [
            result.frequency.clone().unwrap_or_default(),
            result.start_date.clone().unwrap_or_default(),
            result.next_investment_date.clone().unwrap_or_default(),
        ];
        assert_eq!(row.len(), 3);
        for cell in &row {
            assert_eq!(cell, "");
        }
    }
}
