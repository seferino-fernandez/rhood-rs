# rhood-cli

Terminal CLI for the Robinhood API.

Built on [rhood-core](../rhood-core/).

## Build

```bash
cargo build --release -p rhood-cli
```

The binary is written to `target/release/rhood`. Add it to your `PATH` or invoke it directly.

## Install

### Homebrew (macOS & Linux)

```bash
brew tap seferino-fernandez/tools
brew install rhood
```

### Cargo (build from source)

```bash
cargo install rhood-cli
```

### cargo-binstall (prebuilt binary, no compile)

```bash
cargo binstall rhood-cli
```

All three install the `rhood` binary.

## Authentication

Authenticate interactively with your Robinhood credentials:

```bash
rhood login
```

The command prompts for username, password, and MFA code (one-time code or TOTP secret). If credentials are pre-configured in the config file or environment, the corresponding prompts are skipped.

Tokens are cached on disk (default: `~/.rhood/.rhood-token`) so subsequent commands do not re-authenticate. The cache path can be overridden with `--token-cache` or the `RHOOD_TOKEN_CACHE_PATH` environment variable.

To clear the saved session:

```bash
rhood logout
```

## Global Flags

These flags apply to every subcommand.

```shell
--output <table|json|csv>    Output format [default: table]
--config <PATH>              Path to config.toml
--token-cache <PATH>         Path to on-disk token cache
--read-write                 Enable order placement and cancellation
```

Logs are written to stderr. `--output json` and `--output csv` are therefore safe to pipe — stdout carries only structured data.

## Read-Only Mode and Safety

The CLI defaults to read-only mode (`read_only = true` in config). Mutating commands — order buy/sell, recurring create/update/cancel, and watchlist add/remove — are blocked unless `--read-write` is passed.

Order placement also prompts interactively before submitting. Pass `--yes` to skip the prompt when running non-interactively (e.g. in scripts).

## Command Reference

### Stock

Quotes, history, fundamentals, and research data for equities.

| Subcommand      | Description                                    |
| --------------- | ---------------------------------------------- |
| `quote`         | Real-time quotes for one or more symbols       |
| `history`       | Historical OHLCV candles                       |
| `fundamentals`  | Sector, PE ratio, market cap, dividend yield   |
| `latest-prices` | Latest trade price (uses extended-hours price) |
| `earnings`      | Earnings history for a symbol                  |
| `ratings`       | Analyst buy/hold/sell ratings                  |
| `news`          | Recent news articles                           |
| `splits`        | Stock split history                            |
| `tags`          | Instruments in a category tag                  |

```bash
# Real-time quotes
rhood stock quote AAPL TSLA MSFT

# One year of daily candles
rhood stock history AAPL --span year --interval day

# Fundamentals for multiple symbols
rhood stock fundamentals AAPL TSLA

# Latest prices (extended-hours aware)
rhood stock latest-prices AAPL NVDA

# Earnings history
rhood stock earnings AAPL

# Analyst ratings summary
rhood stock ratings NVDA

# Recent news
rhood stock news TSLA

# Split history
rhood stock splits AAPL

# Symbols in the "100-most-popular" tag
rhood stock tags 100-most-popular
```

`--interval` values: `5minute`, `10minute`, `hour` (default), `day`, `week`

`--span` values: `day`, `week` (default), `month`, `3month`, `year`, `5year`

`--bounds` values: `regular` (default), `extended`, `trading`

### Option

Option contract quotes, positions, and order history.

| Subcommand     | Description                                     |
| -------------- | ----------------------------------------------- |
| `positions`    | Open option positions                           |
| `orders`       | Option order history (all or open only)         |
| `cancel-order` | Cancel an open option order by ID               |
| `quote`        | Live bid/ask, Greeks, volume, and open interest |

```bash
# Open option positions
rhood option positions

# All option orders
rhood option orders

# Open option orders only
rhood option orders --open

# Orders updated since a date
rhood option orders --since 2025-01-01

# Cancel an open option order
rhood option cancel-order <ORDER_UUID>

# Quote a single contract (compact view)
rhood option quote AAPL --strike 200 --expiry 2025-06-20 --type call

# Quote with full Greeks
rhood option quote AAPL --strike 200 --expiry 2025-06-20 --type call --detail

# Quote multiple contracts at once
rhood option quote NKE \
  --strike 80 --strike 85 \
  --expiry 2025-07-18 --expiry 2025-07-18 \
  --type call --type call
```

`--type` values: `call`, `put`

### Order

Place, list, and cancel stock orders.

| Subcommand | Description                                  |
| ---------- | -------------------------------------------- |
| `buy`      | Place a buy order (requires `--read-write`)  |
| `sell`     | Place a sell order (requires `--read-write`) |
| `list`     | List stock orders (all or filtered)          |
| `cancel`   | Cancel an open stock order by ID             |

```bash
# Buy 10 shares at market (interactive confirmation)
rhood --read-write order buy AAPL --quantity 10

# Buy $50 worth at market, skip confirmation prompt
rhood --read-write order buy AAPL --dollar-amount 50 --yes

# Sell 5 shares with a limit order
rhood --read-write order sell TSLA --quantity 5 --type limit --limit 250.00

# Extended-hours limit buy
rhood --read-write order buy NVDA --quantity 2 --type limit --limit 130.00 --market-hours extended

# List all stock orders
rhood order list

# List open orders only
rhood order list --status open

# List orders updated since a date
rhood order list --since 2025-01-01

# Cancel an order by ID
rhood order cancel <ORDER_ID>
```

`--type` values: `market` (default), `limit`

`--trigger` values: `immediate` (default), `stop`

`--market-hours` values: `regular` (default), `extended`, `all-day`

`--limit` is required when `--type limit` is set; `--stop-price` is required when `--trigger stop` is set.

### Market

Exchange listings and trading hours.

| Subcommand | Description                       |
| ---------- | --------------------------------- |
| `list`     | All available market exchanges    |
| `hours`    | Trading hours for a specific date |
| `today`    | Today's trading hours             |
| `movers`   | Top 20 daily movers               |

```bash
# List all exchanges
rhood market list

# Trading hours for NYSE on a specific date
rhood market hours XNYS 2025-07-04

# Today's hours for NASDAQ
rhood market today XNAS

# Top 20 daily movers
rhood market movers
```

Market Identifier Codes (MICs): `XNYS` (NYSE), `XNAS` (NASDAQ), `ARCX` (NYSE Arca), and others returned by `rhood market list`.

### Account

Positions, portfolio, profile, and income history.

| Subcommand      | Description                                          |
| --------------- | ---------------------------------------------------- |
| `positions`     | Open stock positions (non-zero quantity)             |
| `all-positions` | All stock positions including closed (zero quantity) |
| `portfolio`     | Portfolio summary (equity, market value)             |
| `profile`       | Account number, buying power, and cash               |
| `buying-power`  | Unified account summary                              |
| `dividends`     | Dividend payment history                             |
| `interest`      | Interest and sweep payment history                   |
| `transfers`     | All transfers (ACH, wire, debit card)                |
| `documents`     | Account documents (statements, tax forms)            |
| `day-trades`    | Recent day trades and PDT status                     |
| `user-profile`  | Authenticated user profile                           |

```bash
rhood account positions
rhood account all-positions
rhood account portfolio
rhood account profile
rhood account buying-power

# Dividend history
rhood account dividends

# Dividend history since a date
rhood account dividends --since 2025-01-01

# Total dividends earned (single value)
rhood account dividends --total

rhood account interest
rhood account transfers

# All documents
rhood account documents

# Filter by type
rhood account documents --doc-type account_statement
rhood account documents --doc-type trade_confirm

rhood account day-trades
rhood account user-profile
```

### Futures

Futures contract lookup, real-time quotes, and order history.

| Subcommand | Description                                |
| ---------- | ------------------------------------------ |
| `contract` | Look up a futures contract by symbol       |
| `quote`    | Real-time quotes for one or more contracts |
| `orders`   | Futures order history                      |
| `account`  | Show the futures account ID                |

```bash
# Contract details
rhood futures contract ESH26

# Real-time quotes for multiple contracts
rhood futures quote ESH26 NQM26

# Futures order history
rhood futures orders

# Orders updated since a date
rhood futures orders --since 2025-01-01

# Futures account ID
rhood futures account
```

### Index

Index quotes and index option contracts.

| Subcommand | Description                                      |
| ---------- | ------------------------------------------------ |
| `quote`    | Real-time index quotes (SPX, NDX, VIX, RUT, XSP) |
| `options`  | Search for index option contracts                |

```bash
# Index quotes
rhood index quote SPX NDX VIX

# SPX call options expiring on a date
rhood index options SPX --expiry 2025-06-20 --option-type call

# Filter by strike as well
rhood index options SPX --expiry 2025-06-20 --option-type put --strike 5500
```

`--option-type` values: `call`, `put`

### Recurring

Recurring investment schedules.

| Subcommand  | Description                                           |
| ----------- | ----------------------------------------------------- |
| `list`      | All recurring investment schedules                    |
| `create`    | Create a new schedule (requires `--read-write`)       |
| `update`    | Update an existing schedule (requires `--read-write`) |
| `cancel`    | Cancel a schedule (requires `--read-write`)           |
| `next-date` | Look up the next investment date                      |

```bash
# List all schedules
rhood recurring list

# Create a $25 weekly investment in TSLA
rhood --read-write recurring create TSLA \
  --amount 25 \
  --frequency weekly \
  --start-date 2025-06-01

# Update amount on an existing schedule
rhood --read-write recurring update <SCHEDULE_ID> --amount 50

# Pause a schedule
rhood --read-write recurring update <SCHEDULE_ID> --state paused

# Cancel a schedule
rhood --read-write recurring cancel <SCHEDULE_ID>

# Next investment date for a biweekly schedule
rhood recurring next-date --frequency biweekly --start-date 2025-06-01
```

`--frequency` values: `weekly`, `biweekly`, `monthly`

`--source` values: `buying-power` (default), `ach`

`--state` values for `update`: `active`, `paused`

### Watchlist

Manage Robinhood watchlists.

| Subcommand | Description                                               |
| ---------- | --------------------------------------------------------- |
| `list`     | All watchlists                                            |
| `show`     | Items in a specific watchlist                             |
| `add`      | Add symbols to a watchlist (requires `--read-write`)      |
| `remove`   | Remove symbols from a watchlist (requires `--read-write`) |

```bash
# List all watchlists
rhood watchlist list

# Show items in a watchlist
rhood watchlist show "My First List"

# Add symbols
rhood --read-write watchlist add "My First List" AAPL NVDA MSFT

# Remove a symbol
rhood --read-write watchlist remove "My First List" NVDA
```

## Output Formats and Scripting

The default output format is a human-readable table. Use `--output json` or `--output csv` for machine-readable output.

```bash
# Parse quotes with jq
rhood --output json stock quote AAPL TSLA | jq '.[] | {symbol, last_trade_price}'

# Write portfolio summary to CSV
rhood --output csv account portfolio > portfolio.csv

# Pipe option positions to a file
rhood --output json option positions > positions.json
```

Logs are always written to stderr, so stdout contains only structured data when `--output json` or `--output csv` is active. This makes it safe to pipe output without stripping log lines.

## Configuration

Config file default location: `~/.config/rhood/config.toml`

Override with `--config <PATH>` or the `RHOOD_CONFIG` environment variable.

Precedence: CLI flags > environment variables > config file.

See [config.toml.example](../../config.toml.example) for all available fields and their defaults. Key CLI fields:

| Field / Env                                        | Description                                          |
| -------------------------------------------------- | ---------------------------------------------------- |
| `read_only` / `RHOOD_READ_ONLY`                    | Disable write operations (default: `true`)           |
| `auth.token_cache_path` / `RHOOD_TOKEN_CACHE_PATH` | Path to cached auth token                            |
| `auth.username` / `RHOOD_USERNAME`                 | Pre-configure Robinhood username                     |
| `auth.password` / `RHOOD_PASSWORD`                 | Pre-configure Robinhood password                     |
| `auth.mfa_secret` / `RHOOD_MFA`                    | TOTP secret for MFA                                  |
| `log.level` / `RHOOD_LOG_LEVEL`                    | Log level: `trace`, `debug`, `info`, `warn`, `error` |
