use crate::output::{OutputFormat, output};
use clap::Subcommand;
use rhood_core::models::option::OptionType;
use rhood_core::models::stock::IndexQuote;
use rhood_core::{RhoodConfig, RobinhoodClient};

#[derive(Subcommand)]
pub enum IndexCommand {
    /// Get real-time index quotes (SPX, NDX, VIX, RUT, XSP)
    Quote {
        /// Index symbols
        #[arg(required = true)]
        symbols: Vec<String>,
    },
    /// Search for index option contracts
    Options {
        /// Index symbol (e.g., "SPX")
        symbol: String,
        /// Strike price
        #[arg(long)]
        strike: Option<f64>,
        /// Expiration date (YYYY-MM-DD)
        #[arg(long, value_parser = crate::utils::validation::parse_date)]
        expiry: String,
        /// Option type
        #[arg(long = "option-type", alias = "type", value_enum)]
        option_type: OptionType,
    },
}

pub async fn run(
    cmd: &IndexCommand,
    format: OutputFormat,
    config: RhoodConfig,
) -> anyhow::Result<()> {
    let client = RobinhoodClient::with_config(config)?;
    super::ensure_logged_in(&client).await?;
    match cmd {
        IndexCommand::Quote { symbols } => {
            let mut quotes = Vec::new();
            for symbol in symbols {
                let quote = client.get_index_quote(symbol).await?;
                quotes.push(quote);
            }
            let headers = &[
                "Symbol",
                "Value",
                "Venue Timestamp",
                "Instrument ID",
                "Updated",
            ];
            let rows = index_quote_rows(&quotes);
            output(format, headers, &rows, &quotes);
        }
        IndexCommand::Options {
            symbol,
            strike,
            expiry,
            option_type,
        } => {
            let strike_str = strike.map(|price| format!("{price:.4}"));
            let options = client
                .find_index_options(symbol, expiry, *option_type, strike_str.as_deref())
                .await?;
            let headers = &["ID", "Type", "Strike", "Expiration", "State"];
            let rows: Vec<Vec<String>> = options
                .iter()
                .map(|option| {
                    vec![
                        option.id.clone().unwrap_or_default(),
                        option.option_type.clone().unwrap_or_default(),
                        option.strike_price.clone().unwrap_or_default(),
                        option.expiration_date.clone().unwrap_or_default(),
                        option.state.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            output(format, headers, &rows, &options);
        }
    }
    Ok(())
}

fn index_quote_rows(quotes: &[IndexQuote]) -> Vec<Vec<String>> {
    quotes
        .iter()
        .map(|quote| {
            vec![
                quote.symbol.clone().unwrap_or_default(),
                quote.value.clone().unwrap_or_default(),
                quote.venue_timestamp.clone().unwrap_or_default(),
                quote.instrument_id.clone().unwrap_or_default(),
                quote.updated_at.clone().unwrap_or_default(),
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhood_core::models::stock::IndexQuote;

    fn make_quote(instrument_id: Option<String>) -> IndexQuote {
        IndexQuote {
            symbol: Some("SPX".to_string()),
            value: Some("5000.00".to_string()),
            venue_timestamp: Some("2024-01-01T00:00:00Z".to_string()),
            instrument_id,
            state: Some("".to_string()),
            updated_at: Some("2024-01-01T00:00:01Z".to_string()),
        }
    }

    #[test]
    fn instrument_id_appears_in_column_3() {
        let quotes = vec![make_quote(Some("abc123-uuid-goes-here".to_string()))];
        let rows = index_quote_rows(&quotes);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), 5);
        assert_eq!(rows[0][3], "abc123-uuid-goes-here");
    }

    #[test]
    fn missing_instrument_id_renders_empty_string() {
        let quotes = vec![make_quote(None)];
        let rows = index_quote_rows(&quotes);
        assert_eq!(rows[0].len(), 5);
        assert_eq!(rows[0][3], "");
    }
}
