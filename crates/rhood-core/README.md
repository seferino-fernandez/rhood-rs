# rhood-core

Async Rust client library for the [Robinhood](https://robinhood.com) trading API.

Powers the [`rhood-cli`](https://github.com/seferino-fernandez/rhood-rs/tree/main/crates/rhood-cli) terminal app and the [`rhood-mcp`](https://github.com/seferino-fernandez/rhood-rs/tree/main/crates/rhood-mcp) Model Context Protocol server.

> This is an unofficial, community-driven project and is not affiliated with, endorsed by, or supported by Robinhood Markets, Inc. Use at your own risk.

## Features

- Authenticated HTTP client (`RobinhoodClient`) with disk-backed token cache and OAuth refresh.
- Endpoint coverage for stocks, options, futures, indices, orders, account, watchlists, recurring investments, dividends, transfers, and research.
- Typed error model (`RhoodError`) covering challenges, rate limits, read-only mode, and API errors.
- Configuration via TOML files, environment variables, or programmatic construction.

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
# Ok(())
# }
```

See the [API documentation](https://docs.rs/rhood-core) for the full reference.

## License

[MIT](https://github.com/seferino-fernandez/rhood-rs/blob/main/LICENSE.md)
