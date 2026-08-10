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

# Place an order (orders require --read-write; you'll be prompted to confirm)
rhood --read-write order buy AAPL --quantity 10 --type market

# Output as JSON
rhood --output json stock quote AAPL
```

## CLI

`rhood-cli` is a terminal interface to the Robinhood API. See the [rhood-cli README](crates/rhood-cli/) for the full reference (commands, flags, output formats, config).

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

Tools are grouped by domain: **Stocks**, **Options**, **Futures**, **Indices**, **Account**, **Income** (dividends, interest, transfers), **Research**, **Recurring investments**, **Watchlists**, **User**, **Market**, **Orders**.

A few representative tools:

| Tool                | What it does                                                 |
| ------------------- | ------------------------------------------------------------ |
| `get_stock_quotes`  | Current quotes for stock tickers                             |
| `get_portfolio`     | Portfolio summary (equity, cash, etc.)                       |
| `place_stock_order` | Request confirmation; the same tool submits after acceptance |
| `cancel_order`      | Cancel an open stock order                                   |

For the live tool list, page through MCP `tools/list` against the running server. Each response contains at most 20 definitions; pass `nextCursor` back as `cursor` until it is absent. Order placement uses MCP elicitation: a `place_*` call returns an order summary asking the human to confirm; after acceptance, the client retries the same call with the confirmation response, which submits the order. Read-only mode (default) hides write tools.

## Testing

```bash
just verify
```

The verification gate checks formatting, all targets and features, strict Clippy warnings,
and the full nextest suite. Use `cargo test -p rhood-core`, `cargo test -p rhood-cli`, or
`cargo test -p rhood-mcp` for focused development.

## License

[MIT](./LICENSE.md)
