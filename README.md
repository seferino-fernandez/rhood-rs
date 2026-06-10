# rhood-rs

Rust client library, CLI, and MCP server for the Robinhood API.

> **Disclaimer:** This is an unofficial, community-driven project and is not affiliated with, endorsed by, or supported by Robinhood Markets, Inc. Use at your own risk.

Inspired by [robin_stocks](https://github.com/jmfernandes/robin_stocks), the excellent Python library for Robinhood.

## Crates

| Crate                              | Description                        |
| ---------------------------------- | ---------------------------------- |
| [`rhood-core`](crates/rhood-core/) | Async Robinhood API client library |
| [`rhood-cli`](crates/rhood-cli/)   | Terminal CLI (`rhood`) for trading |
| [`rhood-mcp`](crates/rhood-mcp/)   | MCP server for LLM-driven trading  |

## Quick Start

```bash
# Build
cargo build --release

# Login interactively
rhood login

# Stock quotes
rhood stock quote AAPL TSLA

# Historical data
rhood stock history AAPL --span year --interval day

# Portfolio
rhood account portfolio

# Place an order (orders require --read-write; you'll be prompted to confirm)
rhood --read-write order buy AAPL --quantity 10 --type market

# Output as JSON
rhood --output json stock quote AAPL
```

## CLI

`rhood-cli` is a terminal interface to the Robinhood API. See the [rhood-cli README](crates/rhood-cli/) for the full reference (commands, flags, output formats, config).

## MCP Server

`rhood-mcp` exposes Robinhood operations as MCP tools for LLM clients. See the [rhood-mcp README](crates/rhood-mcp/) for the full reference (auth modes, OAuth, CLI flags, env vars).

### Setup

The fastest path is the stdio transport with Claude Code:

```bash
claude mcp add rhood --transport stdio -- rhood-mcp --transport stdio
```

For other clients (Claude Desktop, Cursor, shared `.mcp.json`), HTTP transport, OAuth setup, environment variables, and the full CLI reference, see [crates/rhood-mcp/](crates/rhood-mcp/).

### Available Tools

Tools are grouped by domain: **Stocks**, **Options**, **Futures**, **Indices**, **Account**, **Income** (dividends, interest, transfers), **Research**, **Recurring investments**, **Watchlists**, **User**, **Market**, **Orders**.

A few representative tools:

| Tool                | What it does                           |
| ------------------- | -------------------------------------- |
| `get_stock_quotes`  | Current quotes for stock tickers       |
| `get_portfolio`     | Portfolio summary (equity, cash, etc.) |
| `place_stock_order` | Stage a stock order for confirmation   |
| `confirm_order`     | Submit a previously staged order       |
| `cancel_order`      | Cancel an open stock order             |

For the live tool list, call MCP `tools/list` against the running server. Order placement is two-step: stage with `place_*`, then submit with `confirm_order`. Read-only mode (default) hides write tools.

## Testing

```bash
cargo test                    # All unit tests
cargo test -p rhood-core      # Core library only
```

or:

```bash
just test
```

## License

[MIT](./LICENSE.md)
