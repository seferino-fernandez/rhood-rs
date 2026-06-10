use crate::output::{OutputFormat, output};
use clap::Subcommand;
use rhood_core::{RhoodConfig, RobinhoodClient};

#[derive(Subcommand)]
pub enum WatchlistCommand {
    /// List all watchlists
    List,
    /// Show items in a watchlist
    Show {
        /// Watchlist name
        name: String,
    },
    /// Add symbols to a watchlist
    Add {
        /// Watchlist name
        name: String,
        /// Symbols to add
        #[arg(required = true)]
        symbols: Vec<String>,
    },
    /// Remove symbols from a watchlist
    Remove {
        /// Watchlist name
        name: String,
        /// Symbols to remove
        #[arg(required = true)]
        symbols: Vec<String>,
    },
}

pub async fn run(
    cmd: &WatchlistCommand,
    format: OutputFormat,
    config: RhoodConfig,
) -> anyhow::Result<()> {
    let client = RobinhoodClient::with_config(config)?;
    super::ensure_logged_in(&client).await?;
    match cmd {
        WatchlistCommand::List => {
            let lists = client.get_watchlists().await?;
            let headers = &["", "Name", "Items", "ID"];
            let rows: Vec<Vec<String>> = lists
                .iter()
                .map(|list| {
                    vec![
                        list.icon_emoji.clone().unwrap_or_default(),
                        list.display_name.clone().unwrap_or_default(),
                        list.item_count
                            .map_or_else(String::new, |count| count.to_string()),
                        list.id.clone().unwrap_or_default(),
                    ]
                })
                .collect();
            output(format, headers, &rows, &lists);
        }
        WatchlistCommand::Show { name } => {
            let list = client.get_watchlist(name).await?;
            let emoji = list.icon_emoji.as_deref().unwrap_or("");
            let display_name = list.display_name.as_deref().unwrap_or(name);
            let count = list.item_count.unwrap_or(0);
            let items = client.get_watchlist_items(name).await?;
            if matches!(format, OutputFormat::Table) {
                eprintln!("{emoji} {display_name} ({count} items)");
            }
            let headers = &["Symbol", "Name", "Price", "Change %", "Volume"];
            let rows: Vec<Vec<String>> = items
                .iter()
                .map(|item| {
                    vec![
                        item.symbol.clone().unwrap_or_default(),
                        item.name.clone().unwrap_or_default(),
                        item.price
                            .map_or_else(String::new, |price| format!("{price:.2}")),
                        item.one_day_percent_change
                            .map_or_else(String::new, |perc_change| format!("{perc_change:+.2}%")),
                        item.volume
                            .map_or_else(String::new, |volume| format!("{}", volume as i64)),
                    ]
                })
                .collect();
            output(format, headers, &rows, &items);
        }
        WatchlistCommand::Add { name, symbols } => {
            let refs: Vec<&str> = symbols.iter().map(String::as_str).collect();
            client.add_to_watchlist(name, &refs).await?;
            println!("Added {} symbol(s) to watchlist '{name}'", symbols.len());
        }
        WatchlistCommand::Remove { name, symbols } => {
            let refs: Vec<&str> = symbols.iter().map(String::as_str).collect();
            let removed = client.remove_from_watchlist(name, &refs).await?;
            println!(
                "Removed {removed} of {} requested symbol(s) from watchlist '{name}'",
                symbols.len()
            );
        }
    }
    Ok(())
}
