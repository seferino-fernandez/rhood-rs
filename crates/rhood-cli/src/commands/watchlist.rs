use crate::output::{OutputFormat, output};
use clap::Subcommand;
use rhood_core::{RhoodConfig, RobinhoodClient};

#[derive(serde::Serialize)]
struct WatchlistAddResult<'a> {
    watchlist_name: &'a str,
    requested_symbol_count: usize,
    added_symbol_count: usize,
}

#[derive(serde::Serialize)]
struct WatchlistRemoveResult<'a> {
    watchlist_name: &'a str,
    requested_symbol_count: usize,
    removed_symbol_count: usize,
}

struct WatchlistOutput<T> {
    payload: T,
    headers: [&'static str; 3],
    rows: Vec<Vec<String>>,
}

fn map_add_result<'a>(
    name: &'a str,
    requested_symbols: &[String],
) -> WatchlistOutput<WatchlistAddResult<'a>> {
    let requested_symbol_count = requested_symbols.len();
    WatchlistOutput {
        payload: WatchlistAddResult {
            watchlist_name: name,
            requested_symbol_count,
            added_symbol_count: requested_symbol_count,
        },
        headers: ["Watchlist", "Requested Symbols", "Added Symbols"],
        rows: vec![vec![
            name.to_string(),
            requested_symbol_count.to_string(),
            requested_symbol_count.to_string(),
        ]],
    }
}

fn map_remove_result<'a>(
    name: &'a str,
    requested_symbols: &[String],
    removed_symbol_count: usize,
) -> WatchlistOutput<WatchlistRemoveResult<'a>> {
    let requested_symbol_count = requested_symbols.len();
    WatchlistOutput {
        payload: WatchlistRemoveResult {
            watchlist_name: name,
            requested_symbol_count,
            removed_symbol_count,
        },
        headers: ["Watchlist", "Requested Symbols", "Removed Symbols"],
        rows: vec![vec![
            name.to_string(),
            requested_symbol_count.to_string(),
            removed_symbol_count.to_string(),
        ]],
    }
}

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
                        item.volume.map_or_else(String::new, |volume| {
                            #[expect(
                                clippy::cast_possible_truncation,
                                reason = "the table intentionally renders upstream f64 volume as an integer; no in-code integrality check is performed"
                            )]
                            let volume = volume as i64;
                            format!("{volume}")
                        }),
                    ]
                })
                .collect();
            output(format, headers, &rows, &items);
        }
        WatchlistCommand::Add { name, symbols } => {
            let refs: Vec<&str> = symbols.iter().map(String::as_str).collect();
            client.add_to_watchlist(name, &refs).await?;
            let mapped = map_add_result(name, symbols);
            if matches!(format, OutputFormat::Table) {
                println!("Added {} symbol(s) to watchlist '{name}'", symbols.len());
            } else {
                output(format, &mapped.headers, &mapped.rows, &mapped.payload);
            }
        }
        WatchlistCommand::Remove { name, symbols } => {
            let refs: Vec<&str> = symbols.iter().map(String::as_str).collect();
            let removed = client.remove_from_watchlist(name, &refs).await?;
            let mapped = map_remove_result(name, symbols, removed);
            if matches!(format, OutputFormat::Table) {
                println!(
                    "Removed {removed} of {} requested symbol(s) from watchlist '{name}'",
                    symbols.len()
                );
            } else {
                output(format, &mapped.headers, &mapped.rows, &mapped.payload);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{map_add_result, map_remove_result};

    #[test]
    fn add_mapping_carries_the_watchlist_and_counts_to_json_and_the_row() {
        let symbols = vec!["AAPL".to_string(), "NVDA".to_string()];
        let mapped = map_add_result("Growth Picks", &symbols);

        assert_eq!(
            (
                serde_json::to_value(&mapped.payload).unwrap(),
                mapped.headers,
                mapped.rows,
            ),
            (
                serde_json::json!({
                    "watchlist_name": "Growth Picks",
                    "requested_symbol_count": 2,
                    "added_symbol_count": 2,
                }),
                ["Watchlist", "Requested Symbols", "Added Symbols"],
                vec![vec![
                    "Growth Picks".to_string(),
                    "2".to_string(),
                    "2".to_string(),
                ]],
            )
        );
    }

    #[test]
    fn remove_mapping_carries_requested_and_removed_counts_to_json_and_the_row() {
        let symbols = vec!["AAPL".to_string(), "NVDA".to_string(), "MSFT".to_string()];
        let mapped = map_remove_result("Rotation List", &symbols, 2);

        assert_eq!(
            (
                serde_json::to_value(&mapped.payload).unwrap(),
                mapped.headers,
                mapped.rows,
            ),
            (
                serde_json::json!({
                    "watchlist_name": "Rotation List",
                    "requested_symbol_count": 3,
                    "removed_symbol_count": 2,
                }),
                ["Watchlist", "Requested Symbols", "Removed Symbols"],
                vec![vec![
                    "Rotation List".to_string(),
                    "3".to_string(),
                    "2".to_string(),
                ]],
            )
        );
    }
}
