//! `rhood` — a terminal CLI for the Robinhood trading API.
//!
//! Wraps [`rhood_core`] with a [`clap`]-based command surface for stocks,
//! options, futures, indices, orders, watchlists, and recurring
//! investments. Output renders as a human table by default; pass
//! `--output json` or `--output csv` for machine-readable formats.
//!
//! Order placement is gated behind `--read-write`; otherwise the CLI runs
//! in read-only mode and refuses mutating operations.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod commands;
mod output;
mod utils;

use clap::{Parser, Subcommand};
use output::OutputFormat;
use rhood_core::RhoodConfig;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "rhood", about = "Robinhood trading from the terminal", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Output format
    #[arg(long, global = true, default_value = "table")]
    output: OutputFormat,

    /// Path to config.toml
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// Path to the on-disk token cache (overrides `auth.token_cache_path`
    /// and `RHOOD_TOKEN_CACHE_PATH`).
    #[arg(long, global = true, value_name = "PATH")]
    token_cache: Option<PathBuf>,

    /// Enable write operations (order placement and cancellation)
    #[arg(long, global = true)]
    read_write: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Authenticate with Robinhood (prompts for username, password, and MFA code)
    // Global flags (--output, --read-write) are inherited by clap but unused here;
    // filtering per-subcommand in clap derive is invasive, so we accept the noise.
    Login,
    /// Clear saved session credentials
    // Global flags (--output, --read-write) are inherited by clap but unused here;
    // filtering per-subcommand in clap derive is invasive, so we accept the noise.
    Logout,
    /// Stock quotes, history, and fundamentals
    Stock {
        #[command(subcommand)]
        command: commands::stock::StockCommand,
    },
    /// Option contract quotes
    Option {
        #[command(subcommand)]
        command: commands::option::OptionCommand,
    },
    /// Place, list, and cancel orders
    Order {
        #[command(subcommand)]
        command: commands::order::OrderCommand,
    },
    /// Market exchanges and trading hours
    Market {
        #[command(subcommand)]
        command: commands::market::MarketCommand,
    },
    /// View positions and portfolio
    Account {
        #[command(subcommand)]
        command: commands::account::AccountCommand,
    },
    /// Futures contracts, quotes, and orders
    Futures {
        #[command(subcommand)]
        command: commands::futures::FuturesCommand,
    },
    /// Index quotes and index options
    Index {
        #[command(subcommand)]
        command: commands::index::IndexCommand,
    },
    /// Manage recurring investment schedules
    Recurring {
        #[command(subcommand)]
        command: commands::recurring::RecurringCommand,
    },
    /// Manage watchlists
    Watchlist {
        #[command(subcommand)]
        command: commands::watchlist::WatchlistCommand,
    },
}

#[tokio::main]
async fn main() {
    if let Err(error) = try_main().await {
        eprintln!("rhood: {error}");
        std::process::exit(1);
    }
}

async fn try_main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let mut config = RhoodConfig::load(cli.config.as_deref())?;
    if let Some(path) = cli.token_cache.as_deref() {
        config.auth.token_cache_path = path.to_string_lossy().into_owned();
        config.normalize();
    }
    if cli.read_write {
        config.read_only = false;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new(format!("rhood_core={}", config.log.level))
            }),
        )
        .with_writer(std::io::stderr)
        .init();

    match &cli.command {
        Command::Login => commands::login::run_login(config).await,
        Command::Logout => commands::login::run_logout(config).await,
        Command::Stock { command } => commands::stock::run(command, cli.output, config).await,
        Command::Option { command } => commands::option::run(command, cli.output, config).await,
        Command::Order { command } => commands::order::run(command, cli.output, config).await,
        Command::Market { command } => commands::market::run(command, cli.output, config).await,
        Command::Account { command } => commands::account::run(command, cli.output, config).await,
        Command::Futures { command } => commands::futures::run(command, cli.output, config).await,
        Command::Index { command } => commands::index::run(command, cli.output, config).await,
        Command::Recurring { command } => {
            commands::recurring::run(command, cli.output, config).await
        }
        Command::Watchlist { command } => {
            commands::watchlist::run(command, cli.output, config).await
        }
    }
}
