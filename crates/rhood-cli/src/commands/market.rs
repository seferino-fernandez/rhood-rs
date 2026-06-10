use crate::output::{OutputFormat, output};
use clap::Subcommand;
use rhood_core::{RhoodConfig, RobinhoodClient};

#[derive(Subcommand)]
pub enum MarketCommand {
    /// List all available markets (exchanges)
    List,
    /// Get market hours for a specific date
    Hours {
        /// Market Identifier Code (e.g. XNYS, XNAS)
        mic: String,
        /// Date in YYYY-MM-DD format
        #[arg(value_parser = crate::utils::validation::parse_date)]
        date: String,
    },
    /// Get today's market hours
    Today {
        /// Market Identifier Code (e.g. XNYS, XNAS)
        mic: String,
    },
    /// Top 20 daily movers
    Movers,
}

pub async fn run(
    cmd: &MarketCommand,
    format: OutputFormat,
    config: RhoodConfig,
) -> anyhow::Result<()> {
    let client = RobinhoodClient::with_config(config)?;
    super::ensure_logged_in(&client).await?;
    match cmd {
        MarketCommand::List => {
            let markets = client.get_markets().await?;
            let headers = &["MIC", "Acronym", "Name", "City", "Country", "Timezone"];
            let rows: Vec<Vec<String>> = markets
                .iter()
                .map(|market| {
                    vec![
                        market.mic.clone().unwrap_or_default(),
                        market.acronym.clone().unwrap_or_default(),
                        market.name.clone().unwrap_or_default(),
                        market.city.clone().unwrap_or_default(),
                        market.country.clone().unwrap_or_default(),
                        market.timezone.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            output(format, headers, &rows, &markets);
        }
        MarketCommand::Hours { mic, date } => {
            let hours = client.get_market_hours(mic, date).await?;
            let headers = &[
                "Date",
                "Open?",
                "Opens At",
                "Closes At",
                "Ext Opens",
                "Ext Closes",
            ];
            let rows = vec![vec![
                hours.date.clone().unwrap_or_default(),
                hours
                    .is_open
                    .map_or("unknown".into(), |open| open.to_string()),
                hours.opens_at.clone().unwrap_or_default(),
                hours.closes_at.clone().unwrap_or_default(),
                hours.extended_opens_at.clone().unwrap_or_default(),
                hours.extended_closes_at.clone().unwrap_or_default(),
            ]];
            output(format, headers, &rows, &hours);
        }
        MarketCommand::Today { mic } => {
            let hours = client.get_market_today_hours(mic).await?;
            let headers = &[
                "Date",
                "Open?",
                "Opens At",
                "Closes At",
                "Ext Opens",
                "Ext Closes",
            ];
            let rows = vec![vec![
                hours.date.clone().unwrap_or_default(),
                hours
                    .is_open
                    .map_or("unknown".into(), |open| open.to_string()),
                hours.opens_at.clone().unwrap_or_default(),
                hours.closes_at.clone().unwrap_or_default(),
                hours.extended_opens_at.clone().unwrap_or_default(),
                hours.extended_closes_at.clone().unwrap_or_default(),
            ]];
            output(format, headers, &rows, &hours);
        }
        MarketCommand::Movers => {
            let movers = client.get_daily_movers().await?;
            if matches!(format, OutputFormat::Table) {
                eprintln!("Top 20 Daily Movers ({} stocks)", movers.len());
            }
            let headers = &["Symbol", "Name", "Price", "Change %", "Volume"];
            let rows: Vec<Vec<String>> = movers
                .iter()
                .map(|item| {
                    vec![
                        item.symbol.clone().unwrap_or_default(),
                        item.name.clone().unwrap_or_default(),
                        item.price
                            .map_or_else(String::new, |price| format!("{price:.2}")),
                        item.one_day_percent_change
                            .map_or_else(String::new, |percent_change| {
                                format!("{percent_change:+.2}%")
                            }),
                        item.volume
                            .map_or_else(String::new, |volume| format!("{}", volume as i64)),
                    ]
                })
                .collect();
            output(format, headers, &rows, &movers);
        }
    }
    Ok(())
}
