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

## Installation

### Homebrew (macOS & Linux)

```bash
brew tap seferino-fernandez/tools
brew install rhood
brew install rhood-mcp
```

### Cargo (build from source)

```bash
cargo install rhood-cli
cargo install rhood-mcp
```

### cargo-binstall (prebuilt binaries, no compile)

```bash
cargo binstall rhood-cli
cargo binstall rhood-mcp
```

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

# Portfolio returns chart model
rhood account performance

# IPO Access offerings or current empty state
rhood ipo list

# If more than one brokerage account exists, discover and select one
rhood account list
rhood --account ABC123 account portfolio

# Place an order (--read-write leaves read-only mode; you'll be prompted to confirm)
rhood --read-write order buy AAPL --quantity 10 --type market

# Output as JSON
rhood --output json stock quote AAPL
```

## CLI

`rhood-cli` is a terminal interface to the Robinhood API. See the [rhood-cli README](crates/rhood-cli/) for the full reference (commands, flags, output formats, config).

## Instrument Identity

`rhood-core` owns Instrument Identity Enrichment for stock records and research
responses. It validates and resolves each Instrument Reference once, preserves an
upstream Wire Symbol, and fills a missing Ticker Symbol when resolution succeeds.
Normal reads are best-effort: they keep the source record when enrichment misses or
cannot complete. Explicit symbol resolution is strict. Research symbol arrays remain
aligned with their source Instrument References, using `null` for unresolved entries.

## MCP Server

`rhood-mcp` exposes Robinhood operations as MCP tools for LLM clients. It serves only
MCP **2026-07-28** (`rmcp 3.x`): clients requesting an earlier revision are rejected
during initialization, and HTTP requests are stateless per SEP-2567. See the
[rhood-mcp README](crates/rhood-mcp/) for the full reference (auth modes, OAuth, CLI
flags, env vars).

### Setup

The fastest path is the stdio transport with a Claude Code release that supports MCP
2026-07-28:

```bash
claude mcp add rhood --transport stdio -- rhood-mcp --transport stdio
```

For Claude Desktop, Cursor, shared `.mcp.json`, and other configurations, first verify
the client supports MCP 2026-07-28. Then see [crates/rhood-mcp/](crates/rhood-mcp/) for
transport, OAuth, environment-variable, and full CLI setup.

### Available Tools

Tools are grouped by domain: **Stocks**, **Options**, **Futures**, **Indices**, **IPO Access**, **Account**, **Income** (dividends, interest, transfers), **Research**, **Recurring investments**, **Watchlists**, **User**, **Market**, **Orders**.

A few representative tools:

| Tool                        | What it does                                                 |
| --------------------------- | ------------------------------------------------------------ |
| `get_stock_quotes`          | Current quotes for stock tickers                             |
| `get_accounts`              | Discover canonical brokerage account selectors               |
| `get_portfolio`             | Portfolio summary (equity, cash, etc.)                       |
| `get_portfolio_performance` | Raw portfolio returns chart and rendering metadata           |
| `get_ipo_access_list`       | IPO Access offerings or the authenticated user's empty state |
| `place_stock_order`         | Prepare exact wire fields, confirm, then submit once         |
| `cancel_order`              | Cancel an open stock order                                   |

For the live tool list, page through MCP `tools/list` against the running server. Each response contains at most 20 definitions; pass `nextCursor` back as `cursor` until it is absent. Account-sensitive tools accept `account_number`. It may be omitted only when unified discovery returns exactly one account; otherwise call `get_accounts` and pass a canonical number. Order placement uses MCP elicitation. Stock and option orders are both fully prepared before confirmation, so the human authorises the exact executable order rather than its parameters: stock preparation freezes quote collar data, fractional-share conversion, client reference, and form version, and option preparation freezes the resolved contract and its payload alongside the same client reference. After acceptance, the client retries the same call with the sealed state and submits that exact preparation once. Read-only mode (default) hides write tools.

## Testing

```bash
just verify
```

The verification gate checks formatting, all targets and features, strict Clippy warnings,
and the full nextest suite. Use `cargo test -p rhood-core`, `cargo test -p rhood-cli`, or
`cargo test -p rhood-mcp` for focused development.

## License

[MIT](./LICENSE.md)
