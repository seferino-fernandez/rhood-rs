# rhood-core

Async Rust client library for the [Robinhood](https://robinhood.com) trading API.

Powers the [`rhood-cli`](https://github.com/seferino-fernandez/rhood-rs/tree/main/crates/rhood-cli) terminal app and the [`rhood-mcp`](https://github.com/seferino-fernandez/rhood-rs/tree/main/crates/rhood-mcp) Model Context Protocol server.

> This is an unofficial, community-driven project and is not affiliated with, endorsed by, or supported by Robinhood Markets, Inc. Use at your own risk.

## Features

- Authenticated HTTP client (`RobinhoodClient`) with disk-backed token cache and OAuth refresh.
- Shared authenticated-read pacing, cooldown, and bounded retries across client clones.
- Endpoint coverage for stocks, options, futures contracts, quotes, orders, and positions, indices, account data and portfolio performance, IPO Access reads, watchlists, recurring investments, dividends, transfers, and research.
- Typed error model (`RhoodError`) covering challenges, rate limits, read-only mode, and API errors.
- Configuration via TOML files, environment variables, or programmatic construction.
- Unified brokerage account discovery with fail-closed explicit selection for multi-account and IRA users.
- Two-step stock order preparation and submission with exact confirmation data, a 60-second lifetime, no redirects, and no automatic retries.
- Core-owned Instrument Identity Enrichment for stock positions, dividends, orders, news, tags, and splits.

## Configuration and Safety

The client defaults to read-only mode. Configuration can come from
`~/.config/rhood/config.toml`, environment variables, or direct `RhoodConfig`
construction. See [config.toml.example](../../config.toml.example) for all fields.

On Unix, configuration and secret files containing credentials must grant access only
to their owner. Cached OAuth tokens are written with owner-only permissions and replaced
atomically so a failed refresh does not destroy the previous session.

Authenticated GET request starts are spaced by 250 milliseconds by default. Connection
errors, timeouts, and HTTP 429 responses receive up to three total attempts with
exponential backoff. Valid `Retry-After` delays up to 60 seconds establish a cooldown
shared by every clone of the client; larger values are returned without an early retry.
Server errors, API/JSON errors, and all POST, PATCH, DELETE, and stock-order submissions
execute once. Configure these defaults under `[http]` or with
`RHOOD_HTTP_READ_REQUEST_INTERVAL_MS`, `RHOOD_HTTP_READ_MAX_ATTEMPTS`, and
`RHOOD_HTTP_READ_RETRY_BACKOFF_MS`; all values must be positive.

## Example

```rust,no_run
use rhood_core::RobinhoodClient;

# async fn run() -> rhood_core::Result<()> {
let client = RobinhoodClient::new()?;
client.login_from_cache().await?;

let quotes = client.get_quotes(&["AAPL", "TSLA"]).await?;
for quote in quotes {
    println!("{:?}: {:?}", quote.symbol, quote.last_trade_price);
}

let accounts = client.get_accounts().await?;
println!("accounts: {}", accounts.len());
let selected = client.resolve_account(None).await?;
println!("selected: {}", selected.selection_label());
# Ok(())
# }
```

Account-sensitive methods require a canonical account number. `resolve_account(None)` succeeds only when discovery returns exactly one usable account; otherwise pass a number returned by `get_accounts`.

`get_portfolio_performance` returns Robinhood's current Bonfire chart rendering model. Its plotted values are returns, not equity candles, and the endpoint does not accept the retired historical route's interval, span, or bounds controls. Known chart components remain raw so callers can render or inspect them without inventing financial values.

`get_ipo_access_list` and `get_ipo_access_cards` expose the authenticated Bonfire offering list and card view models. Their presentation fields remain tolerant and raw. Summary, order-entry, allocation, receipt, reservation, and write workflows are intentionally not implemented because populated contracts have not been verified.

Interactive stock-order callers should call `prepare_stock_order`, show `PreparedStockOrder::preview`, and then consume the preparation with `submit_prepared_stock_order`. Immediate market orders require positive bid and ask quotes. Dollar buys are converted to an estimated fractional quantity and are not broker-native notional orders. If submission is uncertain, use the returned client reference to reconcile open and recent orders before deciding whether to retry.

## Instrument Identity Enrichment

An Instrument Reference is the source identifier or URL that names a Robinhood
instrument. A Ticker Symbol is a display identifier such as `AAPL`. The core validates
and resolves Instrument References once at the shared boundary. It preserves a Wire
Symbol supplied by the response and fills only missing symbols.

Normal reads for positions, dividends, stock orders, news, tags, and splits use
best-effort enrichment. A resolution miss or enrichment failure leaves the source
record intact rather than failing the read. Research arrays remain positionally aligned
with their source references: `NewsArticle::related_symbols` aligns with
`related_instruments`, and `TagResult::instrument_symbols` aligns with `instruments`.
Each unresolved entry is `None`, which serializes as `null`.

Use the strict public resolver when resolution itself is the requested operation:

```rust,no_run
# use rhood_core::RobinhoodClient;
# async fn run(client: &RobinhoodClient) -> rhood_core::Result<()> {
let references = vec![
    "https://api.robinhood.com/instruments/<uuid>/".to_owned(),
];
let symbols: Vec<Option<String>> = client.resolve_instrument_symbols(&references).await?;
# Ok(())
# }
```

`resolve_instrument_symbols` validates every input before making an HTTP request.
A valid reference absent from the upstream response yields `None`; invalid references,
transport failures, and decoding failures return an error.

See the [API documentation](https://docs.rs/rhood-core) for the full reference.

## Verification

```bash
cargo test -p rhood-core --all-features
cargo clippy -p rhood-core --all-targets --all-features -- -D warnings
```

## License

[MIT](https://github.com/seferino-fernandez/rhood-rs/blob/main/LICENSE.md)
